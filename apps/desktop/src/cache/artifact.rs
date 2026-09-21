//! Bounded v2 descriptors and independent fp32 row records, in the v1 namespace
//! of managed filenames so old maintenance counts every byte as well.
use super::*;
use tr_core::{
    budget::MemoryBudget, color::LinearImage, preview::PreviewRequest, provider::ImageLevels,
};
use tr_render::PreparedPreview;

const V2: &[u8; 8] = b"TRCACHE2";
const BLOCK: usize = 4 * 1024 * 1024;
const HEADER_LIMIT: usize = 65536;

pub enum Lookup {
    Hit(RasterInfo, Box<PreparedPreview>),
    Missing,
    Invalid,
    Busy,
    Disabled,
    Limited(String),
}
impl std::fmt::Debug for Lookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hit(..) => f.write_str("Hit"),
            Self::Missing => f.write_str("Missing"),
            Self::Invalid => f.write_str("Invalid"),
            Self::Busy => f.write_str("Busy"),
            Self::Disabled => f.write_str("Disabled"),
            Self::Limited(e) => f.debug_tuple("Limited").field(e).finish(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    key: String,
    digest: String,
    request: PreviewRequest,
    info: RasterInfo,
    base: u32,
    opaque: bool,
    levels: Vec<[u32; 2]>,
    blocks: Vec<String>,
    histogram: Vec<Vec<u32>>,
}
impl Manager {
    pub fn preview_key(&self, digest: &str, request: PreviewRequest) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    "tr-preview-v3-engines",
                    request.raw_engine.recipe(),
                    digest,
                    &self.fingerprint,
                    request,
                    "full-decode-reference-mips",
                    "fp32-le-premultiplied-linear-Rec2020",
                    "applied-orientation-source-centers"
                ))
                .unwrap()
            )
        )
    }
    pub fn load_preview(
        &self,
        folder: &Path,
        digest: &str,
        request: PreviewRequest,
        budget: &MemoryBudget,
        cancelled: &impl Fn() -> bool,
    ) -> Lookup {
        let _reading = self.read_demand();
        if !self.settings().enabled {
            return Lookup::Disabled;
        }
        let result = (|| -> Result<_> {
            let cache = Folder::open(folder, false, false)?;
            // Look up a bounded set of compatible representations. Only the
            // requested mip tail is read, never the expensive ancestor pixels.
            // Engine and quality are kept explicit, including Standard vs Full.
            let mut candidate = None;
            for edge in std::iter::once(request.edge).chain(
                (tr_core::preview::THUMBNAIL_EDGE_STEP..=4096)
                    .step_by(tr_core::preview::THUMBNAIL_EDGE_STEP as usize)
                    .chain(std::iter::once(0))
                    .filter(|edge| *edge != request.edge),
            ) {
                let stored = PreviewRequest { edge, ..request };
                if stored.maximum_level_edge() < request.maximum_level_edge() {
                    continue;
                }
                let key = self.preview_key(digest, stored);
                match cache
                    .entries
                    .open_file(&format!("{key}.tvc"), false, false, false)
                {
                    Ok(file) => {
                        candidate = Some((file, stored, key));
                        break;
                    }
                    Err(e)
                        if e.downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
                    Err(e) => return Err(e),
                }
            }
            let Some((descriptor, stored_request, key)) = candidate else {
                return self.legacy_preview(&cache, digest, request, budget, cancelled);
            };
            let bytes = read_record(&descriptor, HEADER_LIMIT, cancelled)?;
            ensure!(
                descriptor
                    .metadata()?
                    .modified()?
                    .elapsed()
                    .unwrap_or_default()
                    <= Duration::from_secs(self.settings().unused_days as u64 * 86400),
                "Derivato scaduto"
            );
            let header: Descriptor = serde_json::from_slice(&bytes)?;
            ensure!(
                header.key == key && header.digest == digest && header.request == stored_request,
                "Identità derivato incoerente"
            );
            protocol::validate_info(&header.info)?;
            ensure!(
                header.levels.len() <= 32 && !header.levels.is_empty() && header.base < 32,
                "Livelli fuori quota"
            );
            let mut expected_size = [header.info.source_width, header.info.source_height];
            let mut expected_base = 0;
            while expected_size[0].max(expected_size[1]) > stored_request.maximum_level_edge() {
                expected_size = [expected_size[0].div_ceil(2), expected_size[1].div_ceil(2)];
                expected_base += 1;
            }
            ensure!(
                header.base == expected_base,
                "Dettaglio cache insufficiente o livello non canonico per la richiesta"
            );
            let (_, requested_base) = protocol::mip_geometry(
                [header.info.source_width, header.info.source_height],
                request.maximum_level_edge(),
            );
            ensure!(
                requested_base >= header.base,
                "Dettaglio cache insufficiente"
            );
            let skip = (requested_base - header.base) as usize;
            let mut size = [header.info.source_width, header.info.source_height];
            for _ in 0..header.base {
                ensure!(size != [1, 1], "Livello derivato ridondante");
                size = [size[0].div_ceil(2), size[1].div_ceil(2)];
            }
            let mut total = 0u64;
            let mut blocks = 0usize;
            for (i, level) in header.levels.iter().enumerate() {
                ensure!(
                    *level == size && size[0] <= 32768 && size[1] <= 32768,
                    "Geometria derivato incoerente"
                );
                ensure!(
                    i + 1 == header.levels.len() || size != [1, 1],
                    "Catena ridondante"
                );
                let bytes = size[0] as u64 * size[1] as u64 * 16;
                if i >= skip {
                    total = total
                        .checked_add(bytes)
                        .ok_or_else(|| anyhow::anyhow!("Dimensioni in overflow"))?;
                }
                blocks += (bytes as usize).div_ceil(BLOCK);
                size = [size[0].div_ceil(2), size[1].div_ceil(2)];
            }
            ensure!(
                header.levels.last() == Some(&[1, 1])
                    && blocks == header.blocks.len()
                    && blocks <= 1024,
                "Derivato incompleto"
            );
            ensure!(
                header.histogram.len() == 3
                    && header.histogram.iter().all(|h| h.len() == 256
                        && h.iter().map(|v| *v as u64).sum::<u64>()
                            == header.levels[0][0] as u64 * header.levels[0][1] as u64),
                "Istogramma derivato incoerente"
            );
            let Some(mut lease) = budget.try_reserve(total + BLOCK as u64 + HEADER_LIMIT as u64)
            else {
                return Ok(Lookup::Limited(format!(
                    "Derivato richiede {} MiB; memoria disponibile {} MiB",
                    total.div_ceil(MIB),
                    budget.usage().limit.saturating_sub(budget.usage().reserved) / MIB
                )));
            };
            let mut names = header.blocks.iter();
            let mut levels = Vec::new();
            for (index, [width, height]) in header.levels.iter().enumerate() {
                let count = *width as usize * *height as usize;
                if index < skip {
                    for _ in 0..(count * 16).div_ceil(BLOCK) {
                        names.next();
                    }
                    continue;
                }
                let mut pixels = Vec::with_capacity(count);
                while pixels.len() < count {
                    let name = names.next().unwrap();
                    ensure!(
                        managed(&format!("{name}.tvc"), ".tvc"),
                        "Riferimento cache non valido"
                    );
                    let file =
                        cache
                            .entries
                            .open_file(&format!("{name}.tvc"), false, false, false)?;
                    let bytes = read_record(&file, BLOCK, cancelled)?;
                    ensure!(
                        format!("{:x}", Sha256::digest(&bytes)) == *name,
                        "Blocco sostituito"
                    );
                    ensure!(
                        bytes.len() == ((count - pixels.len()) * 16).min(BLOCK),
                        "Lunghezza blocco incoerente"
                    );
                    for p in bytes.as_chunks::<16>().0 {
                        let pixel = std::array::from_fn(|c| {
                            f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap())
                        });
                        pixels.push(pixel);
                    }
                    let _ = Directory::touch(&file);
                }
                levels.push(LinearImage::new(*width, *height, pixels)?);
            }
            let mut image = ImageLevels::restore(
                [header.info.source_width, header.info.source_height],
                requested_base,
                header.opaque,
                levels,
            )?;
            ensure!(
                image.source().width.max(image.source().height) <= request.maximum_level_edge(),
                "Résolution derivato incoerente"
            );
            if request == PreviewRequest::full() {
                ensure!(image.base_level() == 0, "FullSource senza LOD 0");
            }
            lease.shrink(total);
            image.attach_lease(lease);
            let histogram = if skip == 0 {
                let mut histogram = [[0; 256]; 3];
                for (dest, src) in histogram.iter_mut().zip(header.histogram) {
                    dest.copy_from_slice(&src);
                }
                histogram
            } else {
                image.source().histogram()
            };
            let _ = Directory::touch(&descriptor);
            Ok(Lookup::Hit(
                header.info,
                Box::new(PreparedPreview {
                    image: Arc::new(image),
                    histogram,
                }),
            ))
        })();
        let mut stats = self.statistics.lock().unwrap();
        match result {
            Ok(hit @ Lookup::Hit(..)) => {
                stats.hits += 1;
                hit
            }
            Ok(other) => other,
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock) =>
            {
                stats.busy += 1;
                Lookup::Busy
            }
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                stats.misses += 1;
                Lookup::Missing
            }
            Err(_) => {
                stats.corrupt += 1;
                Lookup::Invalid
            }
        }
    }
    fn legacy_preview(
        &self,
        cache: &Folder,
        digest: &str,
        request: PreviewRequest,
        budget: &MemoryBudget,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Lookup> {
        if request.raw_engine != tr_core::decoder::RawEngine::default() {
            return Ok(Lookup::Missing);
        }
        // Reuse only the exact recognized pipeline fingerprint. Older app/OS
        // fingerprints remain safe misses under the common quota.
        let key = self.key(digest);
        let file = cache
            .entries
            .open_file(&format!("{key}.tvc"), false, false, false)?;
        ensure!(
            file.metadata()?.modified()?.elapsed().unwrap_or_default()
                <= Duration::from_secs(self.settings().unused_days as u64 * 86400),
            "Cache v1 scaduta"
        );
        let Some(mut lease) = budget.try_reserve(file.metadata()?.len().saturating_add(MIB)) else {
            return Ok(Lookup::Missing);
        };
        let (info, prepared) = read_artifact(&file, &key, digest, cancelled)?;
        let pyramid = Arc::try_unwrap(prepared.pyramid)
            .ok()
            .ok_or_else(|| anyhow::anyhow!("Piramide v1 condivisa"))?;
        let mut image = ImageLevels::from_pyramid(pyramid, request)?;
        let histogram = image.source().histogram();
        lease.shrink(image.byte_len() as u64);
        image.attach_lease(lease);
        let _ = Directory::touch(&file);
        Ok(Lookup::Hit(
            info,
            Box::new(PreparedPreview {
                image: Arc::new(image),
                histogram,
            }),
        ))
    }
    pub fn store_preview(
        &self,
        folder: &Path,
        digest: &str,
        request: PreviewRequest,
        info: &RasterInfo,
        preview: &PreparedPreview,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        let settings = self.settings();
        if !settings.enabled || cancelled() {
            return Ok(());
        }
        let key = self.preview_key(digest, request);
        // The logical job, including its descriptor/record overhead, is admitted
        // before any block. Splitting cannot bypass temporary_mib.
        let required = preview.image.byte_len() as u64 + HEADER_LIMIT as u64 + 48 * 1024;
        let quota = settings.disk_mib * MIB;
        ensure!(
            required <= quota && required <= settings.temporary_mib * MIB,
            "Derivato oltre quota disco/temporanei"
        );
        self.wait_for_readers(cancelled)?;
        {
            let cache = Folder::open(folder, true, true)?;
            let (entries, removed) =
                cache.trim_with_minimum(quota - required, settings.unused_days, quota / 5)?;
            self.update_usage(folder, entries, removed);
            ensure!(
                fs2::available_space(&cache.root.path)? >= required + settings.free_mib * MIB,
                "Riserva disco insufficiente"
            );
        }
        let mut blocks = Vec::new();
        let mut published = Vec::new();
        let mut batch = Vec::new();
        for level in preview.image.levels() {
            for chunk in level.pixels.chunks(BLOCK / 16) {
                ensure!(
                    !cancelled() && self.settings().enabled,
                    "Persistenza annullata"
                );
                let mut bytes = Vec::with_capacity(chunk.len() * 16);
                for p in chunk {
                    for c in p {
                        bytes.extend_from_slice(&c.to_le_bytes());
                    }
                }
                let name = format!("{:x}", Sha256::digest(&bytes));
                self.append_record(
                    folder,
                    &mut batch,
                    &mut published,
                    EncodedRecord::new(name.clone(), &bytes),
                    cancelled,
                )?;
                blocks.push(name);
            }
        }
        let header = Descriptor {
            key: key.clone(),
            digest: digest.into(),
            request,
            info: info.clone(),
            base: preview.image.base_level(),
            opaque: preview.image.opaque(),
            levels: preview
                .image
                .levels()
                .iter()
                .map(|l| [l.width, l.height])
                .collect(),
            blocks,
            histogram: preview.histogram.iter().map(|h| h.to_vec()).collect(),
        };
        let bytes = serde_json::to_vec(&header)?;
        ensure!(bytes.len() <= HEADER_LIMIT, "Descrittore oltre quota");
        self.append_record(
            folder,
            &mut batch,
            &mut published,
            EncodedRecord::new(key, &bytes),
            cancelled,
        )?;
        self.publish_records(folder, &batch, &published, cancelled)?;
        let mut statistics = self.statistics.lock().unwrap();
        statistics.writes += 1;
        statistics.message = "Cache della cartella aggiornata".into();
        Ok(())
    }
    fn append_record(
        &self,
        folder: &Path,
        batch: &mut Vec<EncodedRecord>,
        published: &mut Vec<String>,
        record: EncodedRecord,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        // Keep each lock interval bounded by the same 4 MiB payload envelope.
        // Small mip records share one quota scan, instead of reopening every
        // cache entry separately for each of the tiny levels.
        if batch.iter().map(|r| r.bytes.len()).sum::<usize>() + record.bytes.len() > BLOCK + 44 {
            self.publish_records(folder, batch, published, cancelled)?;
            published.extend(batch.drain(..).map(|r| r.key));
        }
        batch.push(record);
        Ok(())
    }
    fn publish_records(
        &self,
        folder: &Path,
        records: &[EncodedRecord],
        protected: &[String],
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        ensure!(
            records.iter().map(|r| r.bytes.len()).sum::<usize>() <= BLOCK + 44,
            "Blocco di scrittura oltre quota"
        );
        let settings = self.settings();
        ensure!(settings.enabled && !cancelled(), "Persistenza annullata");
        // Records are already encoded and hashed outside the lock. Every .part
        // stays protected by the unchanged v1 lock until rename/removal. A later
        // descriptor in this batch is published only after all preceding blocks.
        self.wait_for_readers(cancelled)?;
        let cache = Folder::open(folder, true, true)?;
        for name in protected {
            cache
                .entries
                .open_file(&format!("{name}.tvc"), false, false, false)?;
        }
        let mut entries = Folder::files(&cache.entries, ".tvc")?;
        let result = (|| -> Result<()> {
            for record in records {
                ensure!(
                    !cancelled() && self.settings().enabled,
                    "Persistenza annullata"
                );
                let target = format!("{}.tvc", record.key);
                if let Ok(file) = cache.entries.open_file(&target, false, false, false) {
                    if read_record(&file, BLOCK.max(HEADER_LIMIT), cancelled)
                        .is_ok_and(|v| v == record.bytes[12..record.bytes.len() - 32])
                    {
                        continue;
                    }
                    cache.entries.remove(&target)?;
                    entries.retain(|e| e.name != target);
                }
                // Reuse the snapshot only inside this exclusive lock. Other
                // instances, including v1, are re-observed at every batch.
                ensure!(
                    entries.iter().map(|e| e.bytes).sum::<u64>() + record.bytes.len() as u64
                        <= self.settings().disk_mib * MIB,
                    "Quota occupata durante la scrittura"
                );
                ensure!(
                    fs2::available_space(&cache.root.path)?
                        >= record.bytes.len() as u64 + self.settings().free_mib * MIB,
                    "Riserva disco insufficiente"
                );
                let name = format!("{}.part", record.key);
                let mut file = cache.tmp.open_file(&name, true, true, true)?;
                self.statistics.lock().unwrap().temporary_bytes = record.bytes.len() as u64;
                let result = (|| -> Result<()> {
                    file.write_all(&record.bytes)?;
                    file.sync_data()?;
                    ensure!(
                        !cancelled() && self.settings().enabled,
                        "Persistenza annullata"
                    );
                    cache.tmp.publish(&name, &cache.entries, &target)?;
                    Ok(())
                })();
                drop(file);
                let _ = cache.tmp.remove(&name);
                self.statistics.lock().unwrap().temporary_bytes = 0;
                result?;
                entries.push(Entry {
                    name: target,
                    bytes: record.bytes.len() as u64,
                    modified: SystemTime::now(),
                });
            }
            Ok(())
        })();
        self.update_usage(folder, entries, 0);
        result
    }
}
struct EncodedRecord {
    key: String,
    bytes: Vec<u8>,
}
impl EncodedRecord {
    fn new(key: String, payload: &[u8]) -> Self {
        let mut bytes = Vec::with_capacity(payload.len() + 44);
        bytes.extend_from_slice(V2);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        let digest = Sha256::digest(&bytes);
        bytes.extend_from_slice(&digest);
        Self { key, bytes }
    }
}

fn read_record(file: &File, maximum: usize, cancelled: &impl Fn() -> bool) -> Result<Vec<u8>> {
    ensure!(!cancelled(), "Lettura annullata");
    let mut file = file;
    let length = file.metadata()?.len();
    ensure!(
        length >= 44 && length <= maximum as u64 + 44,
        "Record fuori quota"
    );
    let mut prefix = [0; 12];
    file.read_exact(&mut prefix)?;
    let size = u32::from_le_bytes(prefix[8..12].try_into()?) as usize;
    ensure!(
        &prefix[..8] == V2 && size <= maximum && length == size as u64 + 44,
        "Record incoerente"
    );
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)?;
    let mut checksum = [0; 32];
    file.read_exact(&mut checksum)?;
    let mut hash = Sha256::new();
    hash.update(prefix);
    hash.update(&bytes);
    ensure!(
        hash.finalize().as_slice() == checksum,
        "Checksum record non valido"
    );
    ensure!(!cancelled(), "Lettura annullata");
    Ok(bytes)
}

/// Protect the newest complete thumbnail sets up to the recoverable minimum.
/// Classification reads only bounded descriptors, never full fp32 payloads.
pub(super) fn protected_thumbnails(
    folder: &Folder,
    entries: &[Entry],
    limit: u64,
) -> std::collections::HashSet<String> {
    let sizes: std::collections::HashMap<_, _> =
        entries.iter().map(|e| (e.name.as_str(), e.bytes)).collect();
    let mut protected = std::collections::HashSet::new();
    let mut retained = 0;
    for entry in entries.iter().rev() {
        if entry.bytes > (HEADER_LIMIT + 44) as u64 {
            continue;
        }
        let Ok(file) = folder.entries.open_file(&entry.name, false, false, false) else {
            continue;
        };
        let Ok(bytes) = read_record(&file, HEADER_LIMIT, &|| false) else {
            continue;
        };
        let Ok(header) = serde_json::from_slice::<Descriptor>(&bytes) else {
            continue;
        };
        if header.request.edge == 0 || header.request.edge > 512 || header.blocks.len() > 1024 {
            continue;
        }
        let names: std::collections::HashSet<_> = std::iter::once(entry.name.clone())
            .chain(header.blocks.iter().map(|b| format!("{b}.tvc")))
            .collect();
        if names.iter().any(|n| !sizes.contains_key(n.as_str())) {
            continue;
        }
        let bytes: u64 = names
            .iter()
            .filter(|n| !protected.contains(*n))
            .map(|n| sizes[n.as_str()])
            .sum();
        if retained + bytes <= limit {
            retained += bytes;
            protected.extend(names);
        }
    }
    protected
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::{
        preview::PreviewQuality,
        resample::{Pyramid, Region},
    };
    fn fixture() -> (RasterInfo, PreparedPreview, PreviewRequest) {
        let source = LinearImage::new(
            513,
            257,
            (0..513 * 257)
                .map(|i| [-0.125, i as f32 / 5000., 2., (i % 17) as f32 / 16.])
                .collect(),
        )
        .unwrap();
        let pyramid = Pyramid::new(source).unwrap();
        let request = PreviewRequest {
            raw_engine: tr_core::decoder::RawEngine::default(),
            quality: PreviewQuality::Full,
            edge: 100,
        };
        let image = ImageLevels::from_pyramid(pyramid, request).unwrap();
        let histogram = image.source().histogram();
        let info = RasterInfo {
            reference_mip: None,
            width: 513,
            height: 257,
            source_width: 513,
            source_height: 257,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        };
        (
            info,
            PreparedPreview {
                image: Arc::new(image),
                histogram,
            },
            request,
        )
    }
    fn manager() -> Manager {
        Manager::new(Settings {
            free_mib: 0,
            ..Settings::default()
        })
    }
    #[test]
    fn smaller_cache_request_reads_only_its_tail_and_keeps_quality_and_engine() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, _preview, mut request) = fixture();
        // Include a non-power-of-two thumbnail bucket in compatible lookup.
        request.edge = 384;
        let source = LinearImage::new(
            513,
            257,
            (0..513 * 257)
                .map(|i| [-0.1, i as f32 / 731., 2., (i % 17) as f32 / 16.])
                .collect(),
        )
        .unwrap();
        let image = ImageLevels::from_source(source, request).unwrap();
        let preview = PreparedPreview {
            histogram: image.source().histogram(),
            image: Arc::new(image),
        };
        cache
            .store_preview(folder.path(), "tail", request, &info, &preview, &|| false)
            .unwrap();
        let smaller = PreviewRequest {
            edge: 32,
            ..request
        };
        let base = preview.image.requested_base(smaller);
        let bytes: u64 = preview.image.levels()[base..]
            .iter()
            .map(|l| l.pixels.len() as u64 * 16)
            .sum();
        // Admission fits the small tail plus the bounded reader scratch, not
        // the source graph. Delete an unused ancestor block to prove no read.
        let budget = MemoryBudget::new(bytes + BLOCK as u64 + HEADER_LIMIT as u64);
        let key = cache.preview_key("tail", request);
        let directory = Folder::open(folder.path(), false, false).unwrap();
        let file = directory
            .entries
            .open_file(&format!("{key}.tvc"), false, false, false)
            .unwrap();
        let header: Descriptor =
            serde_json::from_slice(&read_record(&file, HEADER_LIMIT, &|| false).unwrap()).unwrap();
        drop(file);
        drop(directory);
        std::fs::remove_file(
            folder
                .path()
                .join(NAME)
                .join("entries")
                .join(format!("{}.tvc", header.blocks[0])),
        )
        .unwrap();
        let Lookup::Hit(_, result) =
            cache.load_preview(folder.path(), "tail", smaller, &budget, &|| false)
        else {
            panic!("Compatible tail must load independently");
        };
        assert_eq!(
            result.image.source().pixels,
            preview.image.levels()[base].pixels
        );
        assert_eq!(result.histogram, result.image.source().histogram());
        assert_eq!(budget.usage().reserved, bytes);
        drop(result);
        assert_eq!(budget.usage().reserved, 0);
        for incompatible in [
            PreviewRequest {
                quality: PreviewQuality::Standard,
                ..smaller
            },
            PreviewRequest {
                raw_engine: tr_core::decoder::RawEngine::TrueRenderer,
                ..smaller
            },
        ] {
            assert!(!matches!(
                cache.load_preview(folder.path(), "tail", incompatible, &budget, &|| false),
                Lookup::Hit(..)
            ));
        }
        drop(preview);
    }
    #[test]
    fn linked_descriptors_and_blocks_can_be_read_but_not_replaced_or_collected() {
        for descriptor in [true, false] {
            let folder = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let cache = manager();
            let (info, preview, request) = fixture();
            cache
                .store_preview(folder.path(), "linked", request, &info, &preview, &|| false)
                .unwrap();
            let descriptor_name = format!("{}.tvc", cache.preview_key("linked", request));
            let entry = {
                let disk = Folder::open(folder.path(), false, false).unwrap();
                Folder::files(&disk.entries, ".tvc")
                    .unwrap()
                    .into_iter()
                    .find(|entry| (entry.name == descriptor_name) == descriptor)
                    .unwrap()
                    .name
            };
            let target = folder.path().join(NAME).join("entries").join(entry);
            let original = outside.path().join("retained");
            std::fs::hard_link(&target, &original).unwrap();
            let bytes = std::fs::read(&original).unwrap();
            let modified = std::fs::metadata(&original).unwrap().modified().unwrap();
            assert!(matches!(
                cache.load_preview(folder.path(), "linked", request, &cache.memory, &|| false),
                Lookup::Hit(..)
            ));
            // Valid shared records are reusable without rewriting their metadata.
            cache
                .store_preview(folder.path(), "linked", request, &info, &preview, &|| false)
                .unwrap();
            assert_eq!(std::fs::read(&original).unwrap(), bytes);
            assert_eq!(
                std::fs::metadata(&original).unwrap().modified().unwrap(),
                modified
            );

            // An invalid shared record must cause a miss and a skipped write,
            // preserving both names rather than unlinking and replacing one.
            std::fs::write(&original, b"not a cache record").unwrap();
            let modified = std::fs::metadata(&original).unwrap().modified().unwrap();
            assert!(!matches!(
                cache.load_preview(folder.path(), "linked", request, &cache.memory, &|| false),
                Lookup::Hit(..)
            ));
            assert!(
                cache
                    .store_preview(folder.path(), "linked", request, &info, &preview, &|| false)
                    .is_err()
            );
            // Windows may report that the requested zero quota cannot be met;
            // either way collection must leave this shared record intact.
            let _ = cache.maintain(folder.path(), true);
            assert!(target.exists());
            assert_eq!(std::fs::read(&target).unwrap(), b"not a cache record");
            assert_eq!(std::fs::read(&original).unwrap(), b"not a cache record");
            assert_eq!(
                std::fs::metadata(&original).unwrap().modified().unwrap(),
                modified
            );
        }
    }
    #[test]
    fn preview_hit_refreshes_descriptor_and_every_block_before_expiry() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, preview, request) = fixture();
        cache
            .store_preview(folder.path(), "touch", request, &info, &preview, &|| false)
            .unwrap();
        let names = {
            let disk = Folder::open(folder.path(), false, true).unwrap();
            let entries = Folder::files(&disk.entries, ".tvc").unwrap();
            assert!(entries.len() > 1); // descriptor plus payload blocks
            for entry in &entries {
                disk.entries
                    .open_file(&entry.name, true, false, false)
                    .unwrap()
                    .set_modified(SystemTime::now() - Duration::from_secs(86400 * 20))
                    .unwrap();
            }
            entries.into_iter().map(|e| e.name).collect::<Vec<_>>()
        };
        assert!(matches!(
            cache.load_preview(folder.path(), "touch", request, &cache.memory, &|| false),
            Lookup::Hit(..)
        ));
        let disk = Folder::open(folder.path(), false, true).unwrap();
        for name in names {
            let (_, modified) = disk.entries.file_info(&name).unwrap();
            assert!(
                modified.elapsed().unwrap() < Duration::from_secs(60),
                "timestamp not refreshed: {name}"
            );
        }
        assert_eq!(disk.trim(u64::MAX, 1).unwrap().1, 0);
    }
    #[test]
    fn engines_have_independent_cache_entries_and_legacy_cannot_supply_another_engine() {
        use tr_core::decoder::RawEngine;
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, preview, mut request) = fixture();
        request.raw_engine = RawEngine::LibRawBilinear;
        cache
            .store_preview(
                folder.path(),
                "engine-fixture",
                request,
                &info,
                &preview,
                &|| false,
            )
            .unwrap();
        let original_key = cache.preview_key("engine-fixture", request);
        assert!(matches!(
            cache.load_preview(
                folder.path(),
                "engine-fixture",
                request,
                &cache.memory,
                &|| false
            ),
            Lookup::Hit(..)
        ));
        for engine in [RawEngine::LibRawAhd, RawEngine::TrueRenderer] {
            let changed = PreviewRequest {
                raw_engine: engine,
                ..request
            };
            assert_ne!(original_key, cache.preview_key("engine-fixture", changed));
            assert!(matches!(
                cache.load_preview(
                    folder.path(),
                    "engine-fixture",
                    changed,
                    &cache.memory,
                    &|| false
                ),
                Lookup::Missing
            ));
        }
        assert!(matches!(
            cache.load_preview(
                folder.path(),
                "engine-fixture",
                request,
                &cache.memory,
                &|| false
            ),
            Lookup::Hit(..)
        ));
    }
    #[test]
    fn pending_reader_gets_precedence_before_optional_write() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, preview, request) = fixture();
        let reading = cache.read_demand();
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                cache.store_preview(folder.path(), "priority", request, &info, &preview, &|| {
                    false
                })
            });
            let start = std::time::Instant::now();
            while cache.stats().writer_yields == 0 && start.elapsed() < Duration::from_secs(1) {
                std::thread::yield_now();
            }
            assert!(cache.stats().writer_yields > 0);
            assert!(!folder.path().join(NAME).exists());
            drop(reading);
            writer.join().unwrap().unwrap();
        });
        assert!(matches!(
            cache.load_preview(folder.path(), "priority", request, &cache.memory, &|| false),
            Lookup::Hit(..)
        ));
    }
    #[test]
    fn multi_batch_full_roundtrip_and_cancelled_publication() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (mut info, _, _) = fixture();
        let request = PreviewRequest::full();
        let source = LinearImage::new(
            1025,
            513,
            (0..1025 * 513)
                .map(|i| [-0.125, i as f32 / 4096., 2., 1.])
                .collect(),
        )
        .unwrap();
        let image = ImageLevels::from_source(source, request).unwrap();
        info.width = 1025;
        info.height = 513;
        info.source_width = 1025;
        info.source_height = 513;
        let preview = PreparedPreview {
            histogram: image.source().histogram(),
            image: Arc::new(image),
        };
        let first_bytes: Vec<_> = preview.image.source().pixels[..BLOCK / 16]
            .iter()
            .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
            .collect();
        let first = folder
            .path()
            .join(NAME)
            .join("entries")
            .join(format!("{:x}.tvc", Sha256::digest(&first_bytes)));
        assert!(
            cache
                .store_preview(
                    folder.path(),
                    "interrupted",
                    request,
                    &info,
                    &preview,
                    &|| first.exists()
                )
                .is_err()
        );
        assert!(
            first.exists(),
            "Cancellation must occur after a published data block"
        );
        assert!(matches!(
            cache.load_preview(
                folder.path(),
                "interrupted",
                request,
                &cache.memory,
                &|| false
            ),
            Lookup::Missing
        ));
        assert_eq!(
            std::fs::read_dir(folder.path().join(NAME).join("tmp"))
                .unwrap()
                .count(),
            0
        );
        cache
            .store_preview(folder.path(), "complete", request, &info, &preview, &|| {
                false
            })
            .unwrap();
        let Lookup::Hit(_, actual) =
            cache.load_preview(folder.path(), "complete", request, &cache.memory, &|| false)
        else {
            panic!("Full cache unavailable");
        };
        for (expected, actual) in preview.image.levels().iter().zip(actual.image.levels()) {
            assert_eq!(
                (expected.width, expected.height),
                (actual.width, actual.height)
            );
            assert!(
                expected
                    .pixels
                    .iter()
                    .flatten()
                    .zip(actual.pixels.iter().flatten())
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
        let disk = Folder::open(folder.path(), false, false).unwrap();
        assert!(
            Folder::files(&disk.entries, ".tvc")
                .unwrap()
                .iter()
                .all(|entry| entry.bytes <= (BLOCK + 44) as u64)
        );
    }
    #[test]
    fn thumbnail_minimum_survives_newer_heavy_entries_and_v1_reads_are_bounded() {
        let folder = tempfile::tempdir().unwrap();
        let manager = manager();
        let (info, preview, request) = fixture();
        manager
            .store_preview(folder.path(), "minimum", request, &info, &preview, &|| {
                false
            })
            .unwrap();
        let cache = Folder::open(folder.path(), false, true).unwrap();
        let thumbnail_bytes = Folder::files(&cache.entries, ".tvc")
            .unwrap()
            .iter()
            .map(|e| e.bytes)
            .sum::<u64>();
        let heavy = format!("{}.tvc", "f".repeat(64));
        cache
            .entries
            .open_file(&heavy, true, true, true)
            .unwrap()
            .set_len(thumbnail_bytes * 5)
            .unwrap();
        let (remaining, _) = cache.trim(thumbnail_bytes * 5, 30).unwrap();
        assert!(!remaining.iter().any(|e| e.name == heavy));
        drop(cache);
        assert!(matches!(
            manager.load_preview(folder.path(), "minimum", request, &manager.memory, &|| {
                false
            }),
            Lookup::Hit(..)
        ));
        let source = LinearImage::new(513, 257, vec![[0.2, 0.3, 1.5, 1.]; 513 * 257]).unwrap();
        let prepared = tr_render::prepare(source).unwrap();
        manager
            .store(folder.path(), "legacy", &info, &prepared, &|| false)
            .unwrap();
        let Lookup::Hit(_, loaded) =
            manager.load_preview(folder.path(), "legacy", request, &manager.memory, &|| false)
        else {
            panic!("Known v1 should be reused");
        };
        assert!(loaded.image.base_level() > 0);
        assert!(loaded.image.byte_len() < prepared.pyramid.byte_len() / 4);
        let budget = MemoryBudget::new(1);
        assert!(matches!(
            manager.load_preview(folder.path(), "legacy", request, &budget, &|| false),
            Lookup::Missing
        ));
    }
    #[test]
    fn v2_roundtrip_independent_levels_and_shared_v1_quota() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, preview, request) = fixture();
        cache
            .store_preview(folder.path(), "digest", request, &info, &preview, &|| false)
            .unwrap();
        let bytes = preview.image.byte_len();
        let expected = preview
            .image
            .render(Region::fitted([513, 257], [90, 45]))
            .unwrap()
            .pixels;
        drop(preview);
        let Lookup::Hit(_, loaded) =
            cache.load_preview(folder.path(), "digest", request, &cache.memory, &|| false)
        else {
            panic!("expected verified hit");
        };
        assert_eq!(loaded.image.byte_len(), bytes);
        assert_eq!(
            loaded
                .image
                .render(Region::fitted([513, 257], [90, 45]))
                .unwrap()
                .pixels,
            expected
        );
        assert_eq!(
            cache.memory.usage().reserved,
            cache.baseline_bytes + bytes as u64
        );
        let reader = Folder::open(folder.path(), false, false).unwrap();
        assert!(cache.maintain(folder.path(), true).is_err());
        drop(reader);
        cache.maintain(folder.path(), true).unwrap();
        assert_eq!(
            loaded
                .image
                .render(Region::fitted([513, 257], [90, 45]))
                .unwrap()
                .pixels,
            expected
        );
        drop(loaded);
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
        assert_eq!(cache.stats().bytes, 0);
    }
    #[test]
    fn standard_cannot_satisfy_full_and_corrupt_or_missing_blocks_are_contained() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, preview, request) = fixture();
        cache
            .store_preview(folder.path(), "digest", request, &info, &preview, &|| false)
            .unwrap();
        assert!(matches!(
            cache.load_preview(
                folder.path(),
                "digest",
                PreviewRequest {
                    raw_engine: tr_core::decoder::RawEngine::default(),
                    quality: PreviewQuality::Standard,
                    ..request
                },
                &cache.memory,
                &|| false
            ),
            Lookup::Missing
        ));
        let cache_folder = Folder::open(folder.path(), false, false).unwrap();
        let descriptor = cache_folder
            .entries
            .open_file(
                &format!("{}.tvc", cache.preview_key("digest", request)),
                false,
                false,
                false,
            )
            .unwrap();
        let header: Descriptor =
            serde_json::from_slice(&read_record(&descriptor, HEADER_LIMIT, &|| false).unwrap())
                .unwrap();
        drop(cache_folder);
        let block = folder
            .path()
            .join(NAME)
            .join("entries")
            .join(format!("{}.tvc", header.blocks[0]));
        let mut bytes = std::fs::read(&block).unwrap();
        bytes[16] ^= 1;
        std::fs::write(&block, &bytes).unwrap();
        assert!(matches!(
            cache.load_preview(folder.path(), "digest", request, &cache.memory, &|| false),
            Lookup::Invalid
        ));
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
        std::fs::remove_file(&block).unwrap();
        assert!(matches!(
            cache.load_preview(folder.path(), "digest", request, &cache.memory, &|| false),
            Lookup::Missing
        ));
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
    }
    #[test]
    fn low_budget_busy_disabled_and_cancelled_writes_have_distinct_outcomes() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, preview, request) = fixture();
        cache
            .store_preview(folder.path(), "digest", request, &info, &preview, &|| false)
            .unwrap();
        // A well-checksummed descriptor must not promote a coarse mip to Full.
        cache
            .store_preview(
                folder.path(),
                "insufficient",
                PreviewRequest::full(),
                &info,
                &preview,
                &|| false,
            )
            .unwrap();
        assert!(matches!(
            cache.load_preview(
                folder.path(),
                "insufficient",
                PreviewRequest::full(),
                &cache.memory,
                &|| false
            ),
            Lookup::Invalid
        ));
        let low = MemoryBudget::new(1);
        let outcome = cache.load_preview(folder.path(), "digest", request, &low, &|| false);
        assert!(matches!(outcome, Lookup::Limited(_)), "{outcome:?}");
        let lock = Folder::open(folder.path(), false, true).unwrap();
        assert!(matches!(
            cache.load_preview(folder.path(), "digest", request, &cache.memory, &|| false),
            Lookup::Busy
        ));
        let inherited = lock._lock.try_clone().unwrap();
        drop(lock);
        assert!(
            matches!(
                cache.load_preview(folder.path(), "digest", request, &cache.memory, &|| false),
                Lookup::Hit(..)
            ),
            "Owner must explicitly release lock even while a duplicate exists"
        );
        drop(inherited);
        cache
            .store_preview(
                folder.path(),
                "cancelled",
                request,
                &info,
                &preview,
                &|| true,
            )
            .unwrap();
        assert!(matches!(
            cache.load_preview(folder.path(), "cancelled", request, &cache.memory, &|| {
                false
            }),
            Lookup::Missing
        ));
        cache.configure(Settings {
            enabled: false,
            ..cache.settings()
        });
        assert!(matches!(
            cache.load_preview(folder.path(), "digest", request, &cache.memory, &|| false),
            Lookup::Disabled
        ));
    }
}
