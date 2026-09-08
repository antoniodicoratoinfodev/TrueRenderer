//! Disposable, lossless, bounded folder cache. It never contains annotations.
mod artifact;
mod directory;
pub mod writer;
pub use artifact::Lookup;
mod settings;
use anyhow::{Result, ensure};
use directory::Directory;
use serde::{Deserialize, Serialize};
pub use settings::{PerformanceProfile, Prefetch, Settings};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, FileTimes},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicI8, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tr_core::{
    protocol::{self, RasterInfo},
    resample::Pyramid,
};
use tr_render::PreparedImage;

pub const NAME: &str = ".truerenderer-cache";
const OWNER: &[u8] = b"TrueRenderer disposable cache v1\n";
const MAGIC: &[u8; 8] = b"TRCACHE1";
const MIB: u64 = 1024 * 1024;
#[derive(Default, Clone, Serialize)]
pub struct Statistics {
    pub hits: u64,
    pub decode_jobs: u64,
    pub coalesced_consumers: u64,
    pub worker_peak_rss_bytes: u64,
    pub worker_peak_footprint_bytes: u64,
    pub misses: u64,
    pub writes: u64,
    pub corrupt: u64,
    pub busy: u64,
    pub evictions: u64,
    pub writer_yields: u64,
    pub memory_pressure: Option<tr_platform::MemoryPressure>,
    pub folder: String,
    pub bytes: u64,
    pub entries: u64,
    pub temporary_bytes: u64,
    pub message: String,
}
pub struct Manager {
    settings: RwLock<Settings>,
    statistics: Mutex<Statistics>,
    settings_writer: Mutex<()>,
    readers: AtomicUsize,
    pressure: AtomicI8,
    battery: std::sync::atomic::AtomicBool,
    fingerprint: String,
    pub memory: tr_core::budget::MemoryBudget,
    pub physical_mib: u64,
    pub baseline_bytes: u64,
    _baseline: tr_core::budget::Lease,
}
pub struct ReadDemand<'a>(&'a AtomicUsize);
impl Drop for ReadDemand<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Manager {
    pub fn new(settings: Settings) -> Self {
        let _ = tr_core::compute::configure(settings.effective_threads());
        let physical_mib = tr_platform::physical_memory_mib();
        let memory =
            tr_core::budget::MemoryBudget::new(settings.effective_memory_mib(physical_mib) * MIB);
        // Explicit allowance for the host, persistent worker contexts and device
        // infrastructure. Incremental image/native scratch remains separately
        // reserved; this estimate is validated by sampled process reports.
        let baseline_bytes = (384 * MIB).min(memory.usage().limit);
        let baseline = memory
            .try_reserve(baseline_bytes)
            .expect("minimum memory exceeds baseline");
        let os = if cfg!(target_os = "macos") {
            std::process::Command::new("/usr/bin/sw_vers")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default()
        } else {
            std::env::consts::OS.into()
        };
        Self {
            memory,
            physical_mib,
            baseline_bytes,
            _baseline: baseline,
            settings: RwLock::new(settings),
            statistics: Mutex::new(Statistics::default()),
            settings_writer: Mutex::new(()),
            readers: AtomicUsize::new(0),
            pressure: AtomicI8::new(-1),
            battery: std::sync::atomic::AtomicBool::new(false),
            fingerprint: format!(
                "{}:{}:{}:Apple-TR-linear-v1:fp32-premultiplied-Rec2020",
                env!("CARGO_PKG_VERSION"),
                tr_core::resample::VERSION,
                os
            ),
        }
    }
    pub fn read_demand(&self) -> ReadDemand<'_> {
        self.readers.fetch_add(1, Ordering::AcqRel);
        ReadDemand(&self.readers)
    }
    fn wait_for_readers(&self, cancelled: &impl Fn() -> bool) -> Result<()> {
        let start = std::time::Instant::now();
        let mut noted = false;
        while self.readers.load(Ordering::Acquire) != 0 {
            if !noted {
                self.statistics.lock().unwrap().writer_yields += 1;
                noted = true;
            }
            ensure!(
                !cancelled() && start.elapsed() < Duration::from_millis(250),
                "Persistenza rinviata per letture prioritarie"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    pub fn under_pressure(&self) -> bool {
        self.pressure.load(Ordering::Acquire) > 0
    }
    pub fn effective_threads(&self) -> usize {
        self.threads_with_resources(&self.settings())
    }
    fn threads_with_resources(&self, settings: &Settings) -> usize {
        let threads = settings.threads_for_power(self.on_battery());
        if self.under_pressure() {
            threads.min(2)
        } else {
            threads
        }
    }
    pub fn set_pressure(&self, pressure: Option<tr_platform::MemoryPressure>) -> bool {
        let next = pressure.map_or(-1, |p| p as i8);
        if self.pressure.swap(next, Ordering::AcqRel) == next {
            return false;
        }
        self.statistics.lock().unwrap().memory_pressure = pressure;
        let _ = tr_core::compute::configure(self.effective_threads());
        true
    }
    pub fn on_battery(&self) -> bool {
        self.battery.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn refresh_power(&self) {
        let next = tr_platform::on_battery();
        if self.battery.swap(next, std::sync::atomic::Ordering::AcqRel) != next {
            let settings = self.settings();
            let _ = tr_core::compute::configure(self.threads_with_resources(&settings));
        }
    }
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }
    pub fn configure(&self, settings: Settings) {
        if let Err(error) = tr_core::compute::configure(self.threads_with_resources(&settings)) {
            self.note(format!("Pool CPU: {error:#}"));
        }
        self.memory
            .configure(settings.effective_memory_mib(self.physical_mib) * MIB);
        *self.settings.write().unwrap() = settings;
    }
    /// Serialize persistence and snapshot the latest live settings after acquiring
    /// the writer, so rapid quality changes cannot restore an older preference.
    pub fn save_current(&self, data: &Path) -> Result<()> {
        let _writer = self.settings_writer.lock().unwrap();
        self.settings().save(data)
    }
    pub fn decoded(&self, consumers: usize, stats: &tr_platform::BrokerStatistics) {
        let mut s = self.statistics.lock().unwrap();
        s.decode_jobs += 1;
        s.coalesced_consumers += consumers.saturating_sub(1) as u64;
        s.worker_peak_rss_bytes = s.worker_peak_rss_bytes.max(stats.peak_rss_bytes);
        s.worker_peak_footprint_bytes = s
            .worker_peak_footprint_bytes
            .max(stats.peak_footprint_bytes);
    }
    pub fn stats(&self) -> Statistics {
        self.statistics.lock().unwrap().clone()
    }
    pub fn note(&self, message: String) {
        self.statistics.lock().unwrap().message = message;
    }
    pub fn key(&self, digest: &str) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&("tr-cache-v1", digest, &self.fingerprint)).unwrap()
            )
        )
    }
    pub fn maintain(&self, folder: &Path, clear: bool) -> Result<()> {
        let settings = self.settings();
        if !settings.enabled && !clear {
            self.note("Cache disco disattivata · solo RAM".into());
            return Ok(());
        }
        self.wait_for_readers(&|| false)?;
        let cache = Folder::open(folder, true, true)?;
        let (entries, removed) = cache.trim(
            if clear { 0 } else { settings.disk_mib * MIB },
            if clear { 0 } else { settings.unused_days },
        )?;
        self.update_usage(folder, entries, removed);
        self.note(
            if clear {
                "Cache della cartella svuotata"
            } else {
                "Cache della cartella pronta"
            }
            .into(),
        );
        Ok(())
    }
    fn update_usage(&self, folder: &Path, entries: Vec<Entry>, removed: u64) {
        let mut s = self.statistics.lock().unwrap();
        s.folder = folder.display().to_string();
        s.bytes = entries.iter().map(|e| e.bytes).sum();
        s.entries = entries.len() as u64;
        s.temporary_bytes = 0;
        s.evictions += removed;
    }
    pub fn load(
        &self,
        folder: &Path,
        digest: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Option<(RasterInfo, PreparedImage)> {
        if !self.settings().enabled {
            return None;
        }
        let key = self.key(digest);
        let result = (|| -> Result<_> {
            let cache = Folder::open(folder, false, false)?;
            let file = cache
                .entries
                .open_file(&format!("{key}.tvc"), false, false, false)?;
            let expired = file.metadata()?.modified()?.elapsed().unwrap_or_default()
                > Duration::from_secs(self.settings().unused_days as u64 * 86400);
            ensure!(!expired, "Cache scaduta");
            match read_artifact(&file, &key, digest, cancelled) {
                Ok(value) => {
                    let _ = file.set_times(FileTimes::new().set_modified(SystemTime::now()));
                    Ok(value)
                }
                Err(e) => {
                    self.statistics.lock().unwrap().corrupt += 1;
                    Err(e)
                }
            }
        })();
        let mut stats = self.statistics.lock().unwrap();
        match result {
            Ok(value) => {
                stats.hits += 1;
                stats.message = "Immagine riutilizzata dalla cache".into();
                Some(value)
            }
            Err(_) => {
                stats.misses += 1;
                None
            }
        }
    }
    pub fn store(
        &self,
        folder: &Path,
        digest: &str,
        info: &RasterInfo,
        prepared: &PreparedImage,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        let settings = self.settings();
        if !settings.enabled || cancelled() {
            return Ok(());
        }
        let key = self.key(digest);
        let header = Header {
            key: key.clone(),
            digest: digest.into(),
            info: info.clone(),
            levels: prepared
                .pyramid
                .levels()
                .iter()
                .map(|l| [l.width, l.height])
                .collect(),
            histogram: prepared.histogram.iter().map(|h| h.to_vec()).collect(),
        };
        let json = serde_json::to_vec(&header)?;
        ensure!(json.len() <= 65536, "Header cache fuori quota");
        let required = 12 + json.len() as u64 + prepared.pyramid.byte_len() as u64 + 32;
        let quota = settings.disk_mib * MIB;
        ensure!(
            required <= quota && required <= settings.temporary_mib * MIB,
            "Immagine oltre quota cache/temporanei; mantenuta in RAM"
        );
        let cache = Folder::open(folder, true, true)?;
        let target = format!("{key}.tvc");
        // Remove our previous invalid entry, never an unknown filename or a directory.
        if cache
            .entries
            .open_file(&target, false, false, false)
            .is_ok()
        {
            cache.entries.remove(&target)?;
        }
        let (entries, removed) = cache.trim(quota - required, settings.unused_days)?;
        self.update_usage(folder, entries, removed);
        ensure!(
            fs2::available_space(&cache.root.path)? >= required + settings.free_mib * MIB,
            "Spazio libero riservato: cache saltata"
        );
        ensure!(!cancelled(), "Cache annullata");
        let name = format!("{key}.part");
        let file = cache.tmp.open_file(&name, true, true, true)?;
        self.statistics.lock().unwrap().temporary_bytes = required;
        let result = (|| -> Result<()> {
            let mut writer = HashWriter {
                inner: BufWriter::with_capacity(256 * 1024, &file),
                hash: Sha256::new(),
                cancelled,
            };
            writer.write_all(MAGIC)?;
            writer.write_all(&(json.len() as u32).to_le_bytes())?;
            writer.write_all(&json)?;
            for level in prepared.pyramid.levels() {
                protocol::write_raster(&mut writer, level)?;
            }
            let digest = writer.hash.finalize();
            writer.inner.write_all(&digest)?;
            writer.inner.flush()?;
            file.sync_data()?;
            ensure!(!cancelled(), "Cache annullata");
            cache.tmp.publish(&name, &cache.entries, &target)?;
            Ok(())
        })();
        drop(file);
        let _ = cache.tmp.remove(&name);
        self.statistics.lock().unwrap().temporary_bytes = 0;
        result?;
        let (entries, removed) = cache.trim(quota, settings.unused_days)?;
        self.update_usage(folder, entries, removed);
        let mut stats = self.statistics.lock().unwrap();
        stats.writes += 1;
        stats.message = "Cache della cartella aggiornata".into();
        Ok(())
    }
}
struct Folder {
    root: Directory,
    entries: Directory,
    tmp: Directory,
    _lock: File,
}
impl Drop for Folder {
    fn drop(&mut self) {
        // A concurrent fork can briefly inherit even a CLOEXEC descriptor until
        // exec. Explicit unlock ends this critical section at the owner's drop,
        // instead of leaving a transient Busy on an unrelated child descriptor.
        let _ = fs2::FileExt::unlock(&self._lock);
    }
}
struct Entry {
    name: String,
    bytes: u64,
    modified: SystemTime,
}
fn managed(name: &str, suffix: &str) -> bool {
    name.strip_suffix(suffix).is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
impl Folder {
    fn open(folder: &Path, create: bool, exclusive: bool) -> Result<Self> {
        let parent = Directory::open(folder)?;
        let (root, new) = parent.child(NAME, create)?;
        if new {
            let mut marker = root.open_file("OWNER", true, true, true)?;
            marker.write_all(OWNER)?;
            marker.sync_all()?;
        }
        let mut marker = root.open_file("OWNER", false, false, false)?;
        ensure!(
            marker.metadata()?.len() == OWNER.len() as u64,
            "Cartella esistente non riconosciuta: nessuna modifica"
        );
        let mut bytes = Vec::new();
        marker.read_to_end(&mut bytes)?;
        ensure!(bytes == OWNER, "Cartella cache non riconosciuta");
        let lock = root.open_file("cache.lock", true, create, false)?;
        if exclusive {
            fs2::FileExt::try_lock_exclusive(&lock)?
        } else {
            fs2::FileExt::try_lock_shared(&lock)?
        }
        let (entries, _) = root.child("entries", create)?;
        let (tmp, _) = root.child("tmp", create)?;
        Ok(Self {
            root,
            entries,
            tmp,
            _lock: lock,
        })
    }
    fn files(dir: &Directory, suffix: &str) -> Result<Vec<Entry>> {
        let mut entries = vec![];
        for (index, entry) in std::fs::read_dir(&dir.path)?.enumerate() {
            ensure!(index < 100000, "Indice cache oltre quota");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !managed(&name, suffix) {
                continue;
            }
            if let Ok((bytes, modified)) = dir.file_info(&name) {
                entries.push(Entry {
                    name,
                    bytes,
                    modified,
                });
            }
        }
        ensure!(entries.len() <= 100000, "Indice cache oltre quota");
        Ok(entries)
    }
    fn trim(&self, limit: u64, days: u32) -> Result<(Vec<Entry>, u64)> {
        self.trim_with_minimum(limit, days, limit / 5)
    }
    fn trim_with_minimum(
        &self,
        limit: u64,
        days: u32,
        thumbnail_minimum: u64,
    ) -> Result<(Vec<Entry>, u64)> {
        // Exclusive lock excludes all active readers and writers in this folder.
        for entry in Self::files(&self.tmp, ".part")? {
            self.tmp.remove(&entry.name)?;
        }
        let mut entries = Self::files(&self.entries, ".tvc")?;
        entries.sort_by_key(|e| e.modified);
        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        let protected = if days > 0 && total > limit {
            artifact::protected_thumbnails(self, &entries, thumbnail_minimum)
        } else {
            std::collections::HashSet::new()
        };
        // Unused thumbnail space is lent to other classes. Only the guaranteed
        // minimum is placed after ordinary LRU candidates; expiry still wins.
        ensure!(
            entries
                .iter()
                .filter(|e| protected.contains(&e.name))
                .map(|e| e.bytes)
                .sum::<u64>()
                <= limit,
            "Artefatto invaderebbe la riserva miniature"
        );
        entries.sort_by_key(|e| (protected.contains(&e.name), e.modified));
        let mut removed = 0;
        entries.retain(|entry| {
            let expired = days == 0
                || entry.modified.elapsed().unwrap_or_default()
                    > Duration::from_secs(days as u64 * 86400);
            if (expired || total > limit) && self.entries.remove(&entry.name).is_ok() {
                total = total.saturating_sub(entry.bytes);
                removed += 1;
                false
            } else {
                true
            }
        });
        ensure!(
            total <= limit,
            "Quota cache non applicabile; scrittura saltata"
        );
        Ok((entries, removed))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    key: String,
    digest: String,
    info: RasterInfo,
    levels: Vec<[u32; 2]>,
    histogram: Vec<Vec<u32>>,
}
fn read_artifact(
    file: &File,
    key: &str,
    digest: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<(RasterInfo, PreparedImage)> {
    let len = file.metadata()?.len();
    let mut reader = HashReader {
        inner: BufReader::with_capacity(256 * 1024, file),
        hash: Sha256::new(),
        cancelled,
    };
    let mut prefix = [0; 12];
    reader.read_exact(&mut prefix)?;
    ensure!(&prefix[..8] == MAGIC, "Versione cache sconosciuta");
    let header_len = u32::from_le_bytes(prefix[8..].try_into()?) as usize;
    ensure!(header_len <= 65536, "Header cache fuori quota");
    let mut bytes = vec![0; header_len];
    reader.read_exact(&mut bytes)?;
    let header: Header = serde_json::from_slice(&bytes)?;
    ensure!(
        header.key == key && header.digest == digest,
        "Chiave cache obsoleta"
    );
    protocol::validate_info(&header.info)?;
    ensure!(
        header.levels.first() == Some(&[header.info.width, header.info.height])
            && !header.levels.is_empty()
            && header.levels.len() <= 32,
        "Dimensioni cache incoerenti"
    );
    let mut pixel_bytes = 0u64;
    for (i, [w, h]) in header.levels.iter().copied().enumerate() {
        ensure!(
            w > 0
                && h > 0
                && w <= 32768
                && h <= 32768
                && w as u64 * h as u64 <= tr_core::color::MAX_PIXELS as u64,
            "Livello cache fuori quota"
        );
        if i > 0 {
            let [pw, ph] = header.levels[i - 1];
            ensure!(
                (pw > 1 || ph > 1) && w == pw.div_ceil(2) && h == ph.div_ceil(2),
                "Piramide cache non valida"
            );
        }
        pixel_bytes += w as u64 * h as u64 * 16;
    }
    ensure!(
        header.levels.last() == Some(&[1, 1]) && len == 12 + header_len as u64 + pixel_bytes + 32,
        "Cache incompleta o troncata"
    );
    ensure!(
        header.histogram.len() == 3
            && header.histogram.iter().all(|h| h.len() == 256
                && h.iter().map(|v| *v as u64).sum::<u64>()
                    == header.info.width as u64 * header.info.height as u64),
        "Istogramma cache non valido"
    );
    let mut levels = vec![];
    for [w, h] in &header.levels {
        let mut info = header.info.clone();
        info.width = *w;
        info.height = *h;
        levels.push(protocol::read_raster(&mut reader, &info)?);
    }
    let actual = reader.hash.finalize();
    let mut expected = [0; 32];
    reader.inner.read_exact(&mut expected)?;
    ensure!(actual.as_slice() == expected, "Checksum cache non valido");
    let mut histogram = [[0; 256]; 3];
    for (dest, source) in histogram.iter_mut().zip(header.histogram) {
        dest.copy_from_slice(&source);
    }
    Ok((
        header.info,
        PreparedImage {
            pyramid: Arc::new(Pyramid::from_levels(levels)?),
            histogram,
        },
    ))
}
struct HashReader<'a, R, F> {
    inner: R,
    hash: Sha256,
    cancelled: &'a F,
}
impl<R: Read, F: Fn() -> bool> Read for HashReader<'_, R, F> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::other("Cache annullata"));
        }
        let n = self.inner.read(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
}
struct HashWriter<'a, W, F> {
    inner: W,
    hash: Sha256,
    cancelled: &'a F,
}
impl<W: Write, F: Fn() -> bool> Write for HashWriter<'_, W, F> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::other("Cache annullata"));
        }
        let n = self.inner.write(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    fn image() -> (RasterInfo, PreparedImage) {
        let pixels = vec![
            [-0.125, 0.5, 1.75, 1.0],
            [0., 0., 0., 0.],
            [0.25, 0.125, 0.5, 0.5],
            [0.6, 0.3, 0.1, 1.0],
            [0.7, 0.1, 0.2, 1.0],
            [0.2, 0.1, 0.7, 1.0],
        ];
        let image = tr_core::color::LinearImage::new(3, 2, pixels).unwrap();
        let info = RasterInfo {
            width: 3,
            height: 2,
            source_width: 3,
            source_height: 2,
            native_bits: 16,
            format: "PNG".into(),
            decoder: "own test".into(),
            input_color: "linear Rec2020".into(),
            filter: "none".into(),
            orientation: "1".into(),
        };
        (info, tr_render::prepare(image).unwrap())
    }
    fn manager() -> Manager {
        Manager::new(Settings {
            free_mib: 0,
            ..Default::default()
        })
    }
    #[test]
    fn pressure_limits_new_work_without_releasing_active_credits_or_preferences() {
        let cache = manager();
        let settings = serde_json::to_vec(&cache.settings()).unwrap();
        let lease = cache.memory.try_reserve(1024 * 1024).unwrap();
        let reserved = cache.memory.usage().reserved;
        assert!(cache.set_pressure(Some(tr_platform::MemoryPressure::Warning)));
        assert!(cache.under_pressure());
        assert!(cache.effective_threads() <= 2);
        assert_eq!(cache.memory.usage().reserved, reserved);
        assert!(cache.set_pressure(Some(tr_platform::MemoryPressure::Critical)));
        assert_eq!(cache.memory.usage().reserved, reserved);
        assert!(cache.set_pressure(Some(tr_platform::MemoryPressure::Normal)));
        assert!(!cache.under_pressure());
        assert_eq!(serde_json::to_vec(&cache.settings()).unwrap(), settings);
        drop(lease);
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
    }
    #[test]
    fn roundtrip_is_bit_exact_and_invalidates_by_content_and_pipeline() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, p) = image();
        cache
            .store(folder.path(), "source-a", &info, &p, &|| false)
            .unwrap();
        let (_, loaded) = cache.load(folder.path(), "source-a", &|| false).unwrap();
        for (a, b) in p.pyramid.levels().iter().zip(loaded.pyramid.levels()) {
            assert!(
                a.pixels
                    .iter()
                    .flatten()
                    .zip(b.pixels.iter().flatten())
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
        assert_eq!(p.histogram, loaded.histogram);
        assert!(cache.load(folder.path(), "source-b", &|| false).is_none());
        let mut other = manager();
        other.fingerprint.push_str("changed");
        assert!(other.load(folder.path(), "source-a", &|| false).is_none());
        assert!(
            std::fs::read_dir(folder.path().join(NAME).join("tmp"))
                .unwrap()
                .next()
                .is_none()
        );
    }
    #[test]
    fn corrupt_truncated_and_cancelled_entries_are_misses_without_partial_publish() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, p) = image();
        cache
            .store(folder.path(), "a", &info, &p, &|| false)
            .unwrap();
        let path = folder
            .path()
            .join(NAME)
            .join("entries")
            .join(format!("{}.tvc", cache.key("a")));
        let original = std::fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        std::fs::write(&path, corrupt).unwrap();
        assert!(cache.load(folder.path(), "a", &|| false).is_none());
        std::fs::write(&path, &original[..15]).unwrap();
        assert!(cache.load(folder.path(), "a", &|| false).is_none());
        let count = std::cell::Cell::new(0);
        let cancel = || {
            count.set(count.get() + 1);
            count.get() > 4
        };
        assert!(cache.store(folder.path(), "b", &info, &p, &cancel).is_err());
        assert!(
            !folder
                .path()
                .join(NAME)
                .join("entries")
                .join(format!("{}.tvc", cache.key("b")))
                .exists()
        );
        assert!(
            std::fs::read_dir(folder.path().join(NAME).join("tmp"))
                .unwrap()
                .next()
                .is_none()
        );
        cache
            .store(folder.path(), "a", &info, &p, &|| false)
            .unwrap();
        assert!(cache.load(folder.path(), "a", &|| false).is_some());
    }
    #[test]
    fn quota_lru_expiry_and_temporary_recovery_preserve_unowned_files() {
        let folder = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, p) = image();
        cache
            .store(folder.path(), "a", &info, &p, &|| false)
            .unwrap();
        cache
            .store(folder.path(), "b", &info, &p, &|| false)
            .unwrap();
        let disk = Folder::open(folder.path(), false, true).unwrap();
        let a = format!("{}.tvc", cache.key("a"));
        let b = format!("{}.tvc", cache.key("b"));
        disk.entries
            .open_file(&a, false, false, false)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(86400 * 10))
            .unwrap();
        let bytes = disk
            .entries
            .open_file(&b, false, false, false)
            .unwrap()
            .metadata()
            .unwrap()
            .len();
        disk.tmp
            .open_file(
                &format!("{}.part", cache.key("abandoned")),
                true,
                true,
                true,
            )
            .unwrap()
            .write_all(b"incomplete")
            .unwrap();
        std::fs::write(disk.entries.path.join("personal-note.txt"), b"keep me").unwrap();
        let (entries, removed) = disk.trim(bytes, 30).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(entries[0].name, b);
        assert!(!disk.entries.path.join(&a).exists());
        assert!(std::fs::read_dir(&disk.tmp.path).unwrap().next().is_none());
        disk.entries
            .open_file(&b, false, false, false)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(86400 * 31))
            .unwrap();
        assert!(disk.trim(u64::MAX, 30).unwrap().0.is_empty());
        assert_eq!(
            std::fs::read(disk.entries.path.join("personal-note.txt")).unwrap(),
            b"keep me"
        );
    }
    #[test]
    fn symlinks_hardlinks_busy_readers_and_unowned_directories_are_not_modified() {
        let folder = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let cache = manager();
        let (info, p) = image();
        symlink(outside.path(), folder.path().join(NAME)).unwrap();
        assert!(cache.maintain(folder.path(), true).is_err());
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        std::fs::remove_file(folder.path().join(NAME)).unwrap();
        std::fs::create_dir(folder.path().join(NAME)).unwrap();
        assert!(cache.maintain(folder.path(), false).is_err());
        std::fs::remove_dir(folder.path().join(NAME)).unwrap();
        cache
            .store(folder.path(), "a", &info, &p, &|| false)
            .unwrap();
        let disk = Folder::open(folder.path(), false, false).unwrap();
        assert!(cache.maintain(folder.path(), true).is_err());
        drop(disk);
        let outside_file = outside.path().join("photo.raw");
        std::fs::write(&outside_file, b"original").unwrap();
        let target = folder
            .path()
            .join(NAME)
            .join("entries")
            .join(format!("{}.tvc", cache.key("link")));
        symlink(&outside_file, &target).unwrap();
        assert!(cache.load(folder.path(), "link", &|| false).is_none());
        cache.maintain(folder.path(), true).unwrap();
        assert_eq!(std::fs::read(&outside_file).unwrap(), b"original");
        std::fs::remove_file(&target).unwrap();
        std::fs::hard_link(&outside_file, &target).unwrap();
        assert!(
            cache
                .store(folder.path(), "link", &info, &p, &|| false)
                .is_err()
        );
        assert_eq!(std::fs::read(outside_file).unwrap(), b"original");
    }
    #[test]
    fn settings_persist_and_resource_limits_skip_cache_without_touching_sources() {
        let data = tempfile::tempdir().unwrap();
        let settings = Settings {
            disk_mib: 128,
            temporary_mib: 32,
            unused_days: 7,
            free_mib: 0,
            enabled: false,
            ..Default::default()
        };
        settings.save(data.path()).unwrap();
        assert_eq!(Settings::load(data.path()).unwrap(), settings);
        let cache = Manager::new(settings);
        cache.maintain(data.path(), false).unwrap();
        assert!(!data.path().join(NAME).exists());
        let (info, p) = image();
        cache.configure(Settings {
            temporary_mib: 0,
            free_mib: 0,
            ..Default::default()
        });
        assert!(
            cache
                .store(data.path(), "large", &info, &p, &|| false)
                .is_err()
        );
        cache.configure(Settings {
            free_mib: u64::MAX / (1024 * 1024) - 4096,
            ..Default::default()
        });
        assert!(
            cache
                .store(data.path(), "space", &info, &p, &|| false)
                .is_err()
        );
        assert!(cache.load(data.path(), "large", &|| false).is_none());
    }
}
