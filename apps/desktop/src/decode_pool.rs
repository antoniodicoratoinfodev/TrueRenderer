use crate::service::Event;
use eframe::egui;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tr_core::Item;
use tr_platform::Broker;

pub struct Job {
    pub item: Item,
    pub edge: u32,
    pub urgent: bool,
    pub generation: u64,
}
#[derive(Default)]
struct Queues {
    urgent: VecDeque<Job>,
    thumbnails: VecDeque<Job>,
}
pub struct DecodePool {
    queues: Arc<(Mutex<Queues>, Condvar)>,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}
impl DecodePool {
    pub fn start(
        binary: PathBuf,
        generation: Arc<AtomicU64>,
        events: mpsc::SyncSender<Event>,
        ctx: egui::Context,
        cache: Arc<crate::cache::Manager>,
    ) -> Self {
        let queues = Arc::new((Mutex::new(Queues::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut workers = vec![];
        for slot in 0..2 {
            let queues = queues.clone();
            let stop = stop.clone();
            let generation = generation.clone();
            let events = events.clone();
            let ctx = ctx.clone();
            let binary = binary.clone();
            let cache = cache.clone();
            workers.push(thread::spawn(move || {
                let mut broker = Broker::with_slot(binary, slot);
                let mut last_generation = None;
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    // Revoke the previous domain even when the new folder has no decodable jobs.
                    let current_generation = generation.load(Ordering::Acquire);
                    if last_generation.is_some_and(|previous| previous != current_generation) {
                        broker.recycle();
                        last_generation = None;
                    }
                    let job = {
                        let (lock, ready) = &*queues;
                        let mut queue = lock.lock().expect("decode queue");
                        if queue.urgent.is_empty() && queue.thumbnails.is_empty() {
                            queue = ready
                                .wait_timeout(queue, Duration::from_millis(25))
                                .expect("decode queue")
                                .0;
                        }
                        queue
                            .urgent
                            .pop_front()
                            .or_else(|| queue.thumbnails.pop_front())
                    };
                    let Some(job) = job else {
                        if let Err(error) = broker.supervise_idle() {
                            let _ =
                                events.send(Event::Status(format!("Decoder riciclato: {error:#}")));
                            ctx.request_repaint();
                        }
                        continue;
                    };
                    if job.generation != generation.load(Ordering::Relaxed) {
                        continue;
                    }
                    if last_generation.is_some_and(|previous| previous != job.generation) {
                        broker.recycle();
                    }
                    last_generation = Some(job.generation);
                    let cancelled = || {
                        stop.load(Ordering::Acquire)
                            || job.generation != generation.load(Ordering::Relaxed)
                    };
                    let result =
                        prepare_cached(&cache, &mut broker, &job.item, job.edge, &cancelled)
                            .map_err(|error| format!("{error:#}"));
                    if !cancelled() {
                        if events
                            .send(Event::Image {
                                id: job.item.id,
                                edge: job.edge,
                                generation: job.generation,
                                result: Box::new(result),
                            })
                            .is_err()
                        {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            }));
        }
        Self {
            queues,
            stop,
            workers,
        }
    }
    pub fn submit(&self, job: Job) -> Result<(), Box<Job>> {
        let (lock, ready) = &*self.queues;
        let mut queues = lock.lock().expect("decode queue");
        queues
            .urgent
            .retain(|queued| queued.generation == job.generation);
        queues
            .thumbnails
            .retain(|queued| queued.generation == job.generation);
        if self.stop.load(Ordering::Acquire) || queues.urgent.len() + queues.thumbnails.len() >= 64
        {
            return Err(Box::new(job));
        }
        if job.urgent {
            queues.urgent.push_back(job);
        } else {
            queues.thumbnails.push_back(job);
        }
        ready.notify_one();
        Ok(())
    }
}
pub fn prepare_cached(
    cache: &crate::cache::Manager,
    broker: &mut Broker,
    item: &Item,
    edge: u32,
    cancelled: &impl Fn() -> bool,
) -> anyhow::Result<crate::service::PreparedDecoded> {
    let folder = item
        .path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cartella sorgente assente"))?;
    let source = broker.prepare_snapshot(&item.path, &item.digest, cancelled)?;
    if edge == 0
        && cache.settings().enabled
        && let Some((info, prepared)) = cache.load(folder, source.digest(), cancelled)
    {
        return Ok(crate::service::PreparedDecoded {
            digest: source.digest().to_owned(),
            info,
            prepared,
            transport: "Cache disco · fp32 · Anteprima",
            worker_pid: None,
        });
    }
    let decoded = broker.decode_snapshot_cancellable(source, edge, cancelled)?;
    let prepared = tr_render::prepare(decoded.raster)?;
    if edge == 0
        && let Err(e) = cache.store(folder, &decoded.digest, &decoded.info, &prepared, cancelled)
    {
        cache.note(
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
            {
                "Cache occupata: scrittura saltata, immagine disponibile".into()
            } else {
                format!("Cache saltata, immagine disponibile: {e:#}")
            },
        );
    }
    Ok(crate::service::PreparedDecoded {
        digest: decoded.digest,
        info: decoded.info,
        prepared,
        transport: decoded.transport,
        worker_pid: decoded.worker_pid,
    })
}

impl Drop for DecodePool {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.queues.1.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::Command, time::Instant};

    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn changing_domain_reaps_idle_decoder_without_a_new_job() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let binary = std::env::var_os("TR_WORKER_BINARY").expect("TR_WORKER_BINARY");
        let data = tempfile::tempdir().unwrap();
        let wrapper = data.path().join("observed-worker");
        let marker = data.path().join("pid");
        let quote =
            |path: &std::path::Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > {}\nexec {}\n",
                quote(&marker),
                quote(std::path::Path::new(&binary))
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let (events, received) = mpsc::sync_channel(4);
        let pool = DecodePool::start(
            wrapper,
            generation.clone(),
            events,
            egui::Context::default(),
            Arc::new(crate::cache::Manager::new(crate::cache::Settings {
                enabled: false,
                ..Default::default()
            })),
        );
        let path = root.join("corpus/01_Studio_cromatico.png");
        let (bytes, digest) = tr_platform::snapshot(&path).unwrap();
        assert!(
            pool.submit(Job {
                item: Item {
                    id: "idle-domain-test".into(),
                    path,
                    name: "Studio cromatico".into(),
                    bytes: bytes.len() as u64,
                    digest,
                    approved: true,
                    annotation: tr_core::Annotation::default(),
                    revision: 0,
                },
                edge: 320,
                urgent: false,
                generation: 1,
            })
            .is_ok()
        );
        let Event::Image { result, .. } = received.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected decoded corpus image")
        };
        assert!(result.is_ok());
        let pid: u32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
        // Read-only liveness observation of our fixture; no PID-only termination is used here.
        let alive = || {
            Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(
            alive(),
            "fixture must be idle and alive before changing the domain"
        );
        generation.store(2, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !alive(),
            "the old decoder retained authority without a new job"
        );
        drop(pool);
    }
}
