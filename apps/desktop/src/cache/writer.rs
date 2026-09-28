//! Optional persistence owns byte credits until completion and never blocks UI delivery.
use super::Manager;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    _queue: Lease,
    _scratch: Lease,
}
pub struct Writer {
    tx: Option<mpsc::SyncSender<WriteJob>>,
    bytes: MemoryBudget,
    cache: Arc<Manager>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Writer {
    pub fn start(cache: Arc<Manager>, generation: Arc<AtomicU64>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<WriteJob>(16);
        let bytes = MemoryBudget::new(64 * 1024 * 1024);
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
                    worker_cache.note(format!("Persistenza anteprima saltata: {error:#}"));
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
        // Queue accounting is a sublimit. ImageLevels already owns its resident
        // credits in the global budget, so those pixels are not counted twice.
        let Some(queue) = self.bytes.try_reserve(preview.image.byte_len() as u64) else {
            return;
        };
        let Some(scratch) = self.cache.memory.try_reserve(16 * 1024 * 1024) else {
            return;
        };
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(WriteJob {
                folder,
                digest,
                request,
                info,
                preview,
                generation,
                _queue: queue,
                _scratch: scratch,
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
