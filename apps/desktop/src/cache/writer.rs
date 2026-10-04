//! Optional persistence owns byte credits until completion and never blocks UI delivery.
use super::Manager;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use tr_core::{
    budget::{Lease, MemoryBudget},
    preview::PreviewRequest,
    protocol::RasterInfo,
};
use tr_render::PreparedPreview;
pub struct WriteJob {
    pub folder: PathBuf,
    pub digest: String,
    pub request: PreviewRequest,
    pub info: RasterInfo,
    pub preview: PreparedPreview,
    pub generation: u64,
    native_limited: bool,
    _queue: Lease,
    _scratch: Lease,
    _pending: PendingWrite,
}
struct PendingWrite(Arc<AtomicUsize>);
impl Drop for PendingWrite {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub struct Writer {
    tx: Option<mpsc::SyncSender<WriteJob>>,
    bytes: MemoryBudget,
    cache: Arc<Manager>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

fn viewer_tail(
    cache: &Manager,
    request: PreviewRequest,
    preview: &PreparedPreview,
) -> anyhow::Result<(PreviewRequest, PreparedPreview)> {
    anyhow::ensure!(request.edge == 0, "Nessun ripiego cache");
    let request = PreviewRequest {
        edge: 2048,
        ..request
    };
    let base = preview.image.requested_base(request);
    if base == 0 {
        return Ok((request, preview.clone()));
    }
    let bytes = preview.image.levels()[base..]
        .iter()
        .map(|level| level.pixels.len() as u64 * 16)
        .sum();
    let credits = cache
        .memory
        .try_reserve(bytes)
        .ok_or_else(|| anyhow::anyhow!("Memoria insufficiente per conservare l'anteprima"))?;
    let image = Arc::new(preview.image.detach(request, credits)?);
    let histogram = image.source().histogram();
    Ok((request, PreparedPreview { image, histogram }))
}
impl Writer {
    pub fn start(cache: Arc<Manager>, generation: Arc<AtomicU64>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<WriteJob>(16);
        let bytes = MemoryBudget::new((cache.memory.usage().limit / 4).min(512 * 1024 * 1024));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_cache = cache.clone();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let mut last_cleanup = std::time::Instant::now() - std::time::Duration::from_secs(60);
            loop {
                if worker_stop.load(Ordering::Acquire) {
                    break;
                }
                let settings = worker_cache.settings();
                if settings.enabled
                    && settings.clean_known_folders
                    && last_cleanup.elapsed() >= std::time::Duration::from_secs(60)
                {
                    if let Err(error) = worker_cache.maintain_known(&|| {
                        worker_stop.load(Ordering::Acquire) || worker_cache.settings() != settings
                    }) {
                        worker_cache.note(format!("Manutenzione cache: {error:#}"));
                    }
                    last_cleanup = std::time::Instant::now();
                }
                let job = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                    Ok(job) => job,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let cancelled = || {
                    worker_stop.load(Ordering::Acquire)
                        || worker_cache.under_pressure()
                        || generation.load(Ordering::Acquire) != job.generation
                };
                if cancelled() {
                    continue;
                }
                if let Err(error) = worker_cache.store_preview(
                    &job.folder,
                    &job.digest,
                    job.request,
                    &job.info,
                    &job.preview,
                    &cancelled,
                ) {
                    // Preserve browsing speed when the disk quota cannot keep
                    // every native ancestor. The already computed viewer mip is
                    // copied with its own credits; no new decode/filter runs.
                    let fallback = (|| -> anyhow::Result<()> {
                        anyhow::ensure!(
                            job.request.edge == 0 && !cancelled(),
                            "Nessun ripiego cache"
                        );
                        let (request, preview) =
                            viewer_tail(&worker_cache, job.request, &job.preview)?;
                        worker_cache.store_preview(
                            &job.folder,
                            &job.digest,
                            request,
                            &job.info,
                            &preview,
                            &cancelled,
                        )?;
                        Ok(())
                    })();
                    if fallback.is_ok() {
                        worker_cache
                            .statistics
                            .lock()
                            .unwrap()
                            .native_preview_fallbacks += 1;
                        worker_cache.note(format!(
                            "Anteprima conservata; dettaglio nativo non conservato: {error:#}"
                        ));
                    } else {
                        worker_cache.note(format!("Persistenza anteprima saltata: {error:#}"));
                    }
                } else if job.native_limited {
                    worker_cache
                        .statistics
                        .lock()
                        .unwrap()
                        .native_preview_fallbacks += 1;
                    worker_cache.note(
                        "Anteprima conservata; dettaglio nativo oltre quota della coda cache"
                            .into(),
                    );
                }
            }
        });
        Self {
            tx: Some(tx),
            bytes,
            cache,
            stop,
            worker: Some(worker),
        }
    }
    pub fn submit(
        &self,
        folder: PathBuf,
        digest: String,
        request: PreviewRequest,
        info: RasterInfo,
        preview: PreparedPreview,
        generation: u64,
    ) {
        if !self.cache.settings().enabled || self.cache.under_pressure() {
            return;
        }
        // Viewer tails can exceed 64 MiB (D750 Retina: about 123 MiB).
        // Bound the entire queue by the live budget, while the images themselves
        // continue owning their global credits until the writer releases them.
        self.bytes
            .configure((self.cache.memory.usage().limit / 4).min(512 * 1024 * 1024));
        // A native pyramid larger than the queue must not prevent persistence
        // of its already computed viewer tail. This runs on the decode worker.
        let native_limited =
            request.edge == 0 && preview.image.byte_len() as u64 > self.bytes.usage().limit;
        let (request, preview) = if native_limited {
            match viewer_tail(&self.cache, request, &preview) {
                Ok(tail) => tail,
                Err(error) => {
                    self.cache
                        .note(format!("Persistenza anteprima saltata: {error:#}"));
                    return;
                }
            }
        } else {
            (request, preview)
        };
        // Queue accounting is a sublimit. ImageLevels already owns its resident
        // credits in the global budget, so those pixels are not counted twice.
        let Some(queue) = self.bytes.try_reserve(preview.image.byte_len() as u64) else {
            self.cache
                .note("Persistenza anteprima saltata: coda oltre quota".into());
            return;
        };
        let Some(scratch) = self.cache.memory.try_reserve(16 * 1024 * 1024) else {
            self.cache
                .note("Persistenza anteprima saltata: memoria insufficiente".into());
            return;
        };
        if let Some(tx) = &self.tx {
            self.cache.preview_writes.fetch_add(1, Ordering::AcqRel);
            let _ = tx.try_send(WriteJob {
                folder,
                digest,
                request,
                info,
                preview,
                generation,
                native_limited,
                _queue: queue,
                _scratch: scratch,
                _pending: PendingWrite(self.cache.preview_writes.clone()),
            });
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn native_preparation_keeps_all_fitted_previews_when_native_detail_exceeds_disk_quota() {
        let folder = tempfile::tempdir().unwrap();
        let cache = Arc::new(Manager::new(super::super::Settings {
            memory_mib: 1024,
            diagnostic_fixed_memory: true,
            disk_mib: 64,
            free_mib: 0,
            ..Default::default()
        }));
        let writer = Writer::start(cache.clone(), Arc::new(AtomicU64::new(1)));
        let mut requests = Vec::new();
        let engines: Vec<_> = tr_core::decoder::RawEngine::choices().collect();
        for index in 0..4 {
            let engine = engines
                .get(index % engines.len().max(1))
                .copied()
                .unwrap_or_default();
            let request = PreviewRequest {
                raw_engine: engine,
                ..PreviewRequest::full()
            };
            requests.push(request);
            let color = [0.1 + index as f32 * 0.1, 0.2, 0.3, 1.];
            let mut image = tr_core::provider::ImageLevels::from_source(
                tr_core::color::LinearImage::new(5000, 400, vec![color; 5000 * 400]).unwrap(),
                request,
            )
            .unwrap();
            image.attach_lease(cache.memory.try_reserve(image.byte_len() as u64).unwrap());
            let image = Arc::new(image);
            let info = RasterInfo {
                shooting: None,
                scientific: None,
                reference_mip: None,
                width: 5000,
                height: 400,
                source_width: 5000,
                source_height: 400,
                native_bits: 32,
                format: "test".into(),
                decoder: "test".into(),
                input_color: "linear Rec2020".into(),
                filter: "reference".into(),
                orientation: "applied".into(),
            };
            writer.submit(
                folder.path().into(),
                format!("source-{index}"),
                request,
                info,
                PreparedPreview {
                    histogram: image.source().histogram(),
                    image,
                },
                1,
            );
            let deadline = Instant::now() + Duration::from_secs(30);
            while cache.preview_writes_pending() {
                assert!(Instant::now() < deadline, "writer did not complete");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(
            cache.stats().native_preview_fallbacks > 0,
            "Native disk pressure was not exercised"
        );
        for (index, request) in requests.into_iter().enumerate() {
            let fitted = PreviewRequest {
                edge: 2400,
                ..request
            };
            let super::super::Lookup::Hit(_, preview) = cache.load_preview(
                folder.path(),
                &format!("source-{index}"),
                fitted,
                &cache.memory,
                &|| false,
            ) else {
                panic!("Prepared fitted preview {index} was evicted by native detail");
            };
            assert_eq!(preview.image.base_level(), 1);
            assert_eq!(preview.image.source().width, 2500);
            assert!(
                (preview.image.source().pixels[100][0] - (0.1 + index as f32 * 0.1)).abs() < 1e-5
            );
        }
        assert!(cache.stats().bytes <= 64 * 1024 * 1024);
    }

    #[test]
    fn viewer_larger_than_64_mib_is_persisted_and_pending_is_released() {
        for (memory_mib, width, height, edge, limited) in
            [(2048, 2112, 1536, 2048, false), (768, 5000, 2048, 0, true)]
        {
            let folder = tempfile::tempdir().unwrap();
            let cache = Arc::new(Manager::new(super::super::Settings {
                memory_mib,
                diagnostic_fixed_memory: true,
                free_mib: 0,
                ..Default::default()
            }));
            let writer = Writer::start(cache.clone(), Arc::new(AtomicU64::new(1)));
            let request = PreviewRequest {
                edge,
                ..PreviewRequest::full()
            };
            let mut image = tr_core::provider::ImageLevels::from_source(
                tr_core::color::LinearImage::new(
                    width,
                    height,
                    vec![[0.2, 0.3, 0.4, 1.]; (width * height) as usize],
                )
                .unwrap(),
                request,
            )
            .unwrap();
            let bytes = image.byte_len() as u64;
            assert!(bytes > 64 * 1024 * 1024);
            image.attach_lease(cache.memory.try_reserve(bytes).unwrap());
            let image = Arc::new(image);
            let info = RasterInfo {
                shooting: None,
                scientific: None,
                reference_mip: None,
                width,
                height,
                source_width: width,
                source_height: height,
                native_bits: 32,
                format: "test".into(),
                decoder: "test".into(),
                input_color: "linear Rec2020".into(),
                filter: "reference".into(),
                orientation: "applied".into(),
            };
            cache.maintain(folder.path(), false).unwrap();
            let reader = cache.read_demand();
            writer.submit(
                folder.path().into(),
                "b".repeat(64),
                request,
                info,
                PreparedPreview {
                    histogram: image.source().histogram(),
                    image: image.clone(),
                },
                1,
            );
            assert!(
                cache.preview_writes_pending(),
                "Viewer must be queued before reporting readiness"
            );
            drop(reader);
            let deadline = Instant::now() + Duration::from_secs(20);
            while cache.preview_writes_pending() {
                assert!(Instant::now() < deadline, "Writer did not finish");
                thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(cache.stats().writes, 1);
            assert_eq!(cache.stats().native_preview_fallbacks, u64::from(limited));
            let request = PreviewRequest {
                edge: 2048,
                ..request
            };
            let super::super::Lookup::Hit(_, loaded) = cache.load_preview(
                folder.path(),
                &"b".repeat(64),
                request,
                &cache.memory,
                &|| false,
            ) else {
                panic!("Prepared viewer must survive RAM eviction");
            };
            assert_eq!(
                loaded.image.source().pixels,
                image.levels()[image.requested_base(request)].pixels
            );
            drop(loaded);
            drop(image);
            drop(writer);
            assert!(!cache.preview_writes_pending());
            assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
        }
    }

    #[test]
    fn revoked_writer_cannot_repopulate_a_cleared_cache() {
        let folder = tempfile::tempdir().unwrap();
        let cache = Arc::new(Manager::new(super::super::Settings {
            free_mib: 0,
            ..Default::default()
        }));
        let generation = Arc::new(AtomicU64::new(1));
        let writer = Writer::start(cache.clone(), generation.clone());
        let request = PreviewRequest::full();
        let image = Arc::new(
            tr_core::provider::ImageLevels::from_source(
                tr_core::color::LinearImage::new(16, 8, vec![[0.2, 0.3, 0.4, 1.]; 128]).unwrap(),
                request,
            )
            .unwrap(),
        );
        let info = RasterInfo {
            shooting: None,
            scientific: None,
            reference_mip: None,
            width: 16,
            height: 8,
            source_width: 16,
            source_height: 8,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        };
        let submit = |epoch| {
            writer.submit(
                folder.path().into(),
                "a".repeat(64),
                request,
                info.clone(),
                PreparedPreview {
                    image: image.clone(),
                    histogram: image.source().histogram(),
                },
                epoch,
            )
        };
        cache.maintain(folder.path(), false).unwrap();
        let reader = cache.read_demand();
        submit(1);
        submit(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        while cache.stats().writer_yields == 0 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        generation.store(2, Ordering::Release);
        drop(reader);
        cache.maintain(folder.path(), true).unwrap();
        while cache.memory.usage().reserved > cache.baseline_bytes {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().writes, 0);
        submit(2);
        while cache.stats().writes == 0 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            cache.stats().entries > 0,
            "The new generation must still persist"
        );
    }
}
