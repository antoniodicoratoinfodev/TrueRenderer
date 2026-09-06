use crate::decode_pool::{DecodePool, Job};
use eframe::egui;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use tr_core::{Annotation, Item};
use tr_platform::CorpusPolicy;
use tr_render::PreparedImage;
use tr_store::Catalog;

pub enum Request {
    Scan {
        folder: PathBuf,
        generation: u64,
    },
    Decode {
        item: Item,
        edge: u32,
        urgent: bool,
        generation: u64,
    },
    Save {
        id: String,
        expected: u64,
        annotation: Annotation,
    },
    Undo,
    Backup,
    Export(PathBuf),
    Shutdown,
}
pub struct PreparedDecoded {
    pub digest: String,
    pub info: tr_core::protocol::RasterInfo,
    pub prepared: PreparedImage,
    pub transport: &'static str,
    pub worker_pid: Option<u32>,
}
pub enum Event {
    DecodeDeferred {
        id: String,
        edge: u32,
        generation: u64,
    },
    Scanned {
        items: Vec<Item>,
        folder: PathBuf,
        generation: u64,
        note: String,
    },
    Image {
        id: String,
        edge: u32,
        generation: u64,
        result: Box<Result<PreparedDecoded, String>>,
    },
    Saved {
        id: String,
        annotation: Annotation,
        revision: u64,
        undo_available: bool,
    },
    SaveFailed {
        id: String,
        error: String,
    },
    Status(String),
    Fatal(String),
    Stopped,
}
pub struct Service {
    pub high: mpsc::SyncSender<Request>,
    pub low: mpsc::SyncSender<Request>,
    pub events: mpsc::Receiver<Event>,
    pub generation: Arc<AtomicU64>,
}

impl Service {
    pub fn start(root: PathBuf, data: PathBuf, worker: PathBuf, ctx: egui::Context) -> Self {
        let (high, high_rx) = mpsc::sync_channel(64);
        let (low, low_rx) = mpsc::sync_channel(8);
        let (events_tx, events) = mpsc::sync_channel(16);
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = generation.clone();
        thread::spawn(move || {
            let send = |event| {
                let _ = events_tx.send(event);
                ctx.request_repaint();
            };
            let mut catalog = match Catalog::open(&data) {
                Ok(c) => c,
                Err(e) => {
                    send(Event::Fatal(format!("Apertura libreria: {e:#}")));
                    return;
                }
            };
            let external = tr_platform::external_decoding_available(&worker);
            let pool = DecodePool::start(
                worker,
                worker_generation.clone(),
                events_tx.clone(),
                ctx.clone(),
            );
            let policy = CorpusPolicy::default();
            let corpus = root
                .join("corpus")
                .canonicalize()
                .unwrap_or_else(|_| root.join("corpus"));
            let mut undo_stack = VecDeque::<(String, Annotation, Annotation)>::new();
            loop {
                let request = match high_rx.try_recv() {
                    Ok(r) => r,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => {
                        match low_rx.recv_timeout(Duration::from_millis(15)) {
                            Ok(r) => r,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(_) => break,
                        }
                    }
                };
                match request {
                    Request::Scan { folder, generation } => {
                        if generation != worker_generation.load(Ordering::Relaxed) {
                            continue;
                        }
                        let result = (|| -> anyhow::Result<(Vec<Item>, String)> {
                            let folder = folder.canonicalize()?;
                            let controlled_folder = folder == corpus;
                            let mut paths = vec![];
                            let mut errors = 0;
                            for entry in std::fs::read_dir(&folder)? {
                                if generation != worker_generation.load(Ordering::Relaxed) {
                                    anyhow::bail!("Scansione sostituita");
                                }
                                let entry = match entry {
                                    Ok(e) => e,
                                    Err(_) => {
                                        errors += 1;
                                        continue;
                                    }
                                };
                                if !entry.file_type()?.is_file()
                                    || !tr_platform::supported_extension(&entry.path())
                                {
                                    continue;
                                }
                                paths.push(entry.path());
                                anyhow::ensure!(
                                    paths.len() <= 100_000,
                                    "Limite R0: 100.000 file nella cartella"
                                );
                            }
                            paths.sort_by_cached_key(|p| {
                                p.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_lowercase()
                            });
                            let mut items = vec![];
                            for path in paths {
                                if generation != worker_generation.load(Ordering::Relaxed) {
                                    anyhow::bail!("Scansione sostituita");
                                }
                                let metadata = path.metadata()?;
                                let (digest, approved) = if controlled_folder {
                                    match tr_platform::snapshot(&path) {
                                        Ok((_, hash)) => {
                                            let approved = policy.approves(&hash);
                                            (hash, approved)
                                        }
                                        Err(_) => {
                                            errors += 1;
                                            continue;
                                        }
                                    }
                                } else {
                                    // Listing stays cheap; decoder snapshots/hashes the exact private bytes on demand.
                                    (
                                        tr_platform::observation_token(&metadata),
                                        external
                                            && metadata.len()
                                                <= tr_core::protocol::MAX_SOURCE as u64,
                                    )
                                };
                                let asset = catalog.observe(&path, &digest, metadata.len())?;
                                items.push(Item {
                                    id: asset.id,
                                    path: path.clone(),
                                    name: path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into(),
                                    bytes: metadata.len(),
                                    digest,
                                    approved,
                                    annotation: asset.annotation,
                                    revision: asset.revision,
                                });
                            }
                            let note = if controlled_folder {
                                format!("Corpus pronto · {errors} file non leggibili")
                            } else {
                                if external {
                                    "Cartella pronta · decoder di sistema macOS · Anteprima".into()
                                } else {
                                    "File esterni: aprire il bundle macOS con servizi XPC.".into()
                                }
                            };
                            Ok((items, note))
                        })();
                        match result {
                            Ok((items, note)) => send(Event::Scanned {
                                items,
                                folder,
                                generation,
                                note,
                            }),
                            Err(error) => send(Event::Status(format!("Scansione: {error:#}"))),
                        }
                    }
                    Request::Decode {
                        item,
                        edge,
                        urgent,
                        generation,
                    } => {
                        if generation != worker_generation.load(Ordering::Relaxed) {
                            continue;
                        }
                        if let Err(job) = pool.submit(Job {
                            item,
                            edge,
                            urgent,
                            generation,
                        }) {
                            send(Event::DecodeDeferred {
                                id: job.item.id,
                                edge,
                                generation,
                            });
                        }
                    }
                    Request::Save {
                        id,
                        expected,
                        annotation,
                    } => {
                        let result = (|| -> anyhow::Result<(Annotation, u64)> {
                            let before = catalog.get(&id)?.annotation;
                            let revision = catalog.save(&id, expected, &annotation, false)?;
                            Ok((before, revision))
                        })();
                        match result {
                            Ok((before, revision)) => {
                                undo_stack.push_back((id.clone(), before, annotation.clone()));
                                if undo_stack.len() > 200 {
                                    undo_stack.pop_front();
                                }
                                send(Event::Saved {
                                    id,
                                    annotation,
                                    revision,
                                    undo_available: true,
                                });
                            }
                            Err(error) => send(Event::SaveFailed {
                                id,
                                error: format!("{error:#}"),
                            }),
                        }
                    }
                    Request::Undo => {
                        if let Some((id, before, after)) = undo_stack.back().cloned() {
                            let result = (|| -> anyhow::Result<u64> {
                                let current = catalog.get(&id)?;
                                anyhow::ensure!(
                                    current.annotation == after,
                                    "Undo in conflitto con una modifica più recente"
                                );
                                catalog.save(&id, current.revision, &before, true)
                            })();
                            match result {
                                Ok(revision) => {
                                    undo_stack.pop_back();
                                    send(Event::Saved {
                                        id,
                                        annotation: before,
                                        revision,
                                        undo_available: !undo_stack.is_empty(),
                                    });
                                }
                                Err(e) => send(Event::Status(format!("Undo: {e:#}"))),
                            }
                        }
                    }
                    Request::Backup => match catalog.backup() {
                        Ok(path) => send(Event::Status(format!(
                            "Backup verificato: {}",
                            path.display()
                        ))),
                        Err(e) => send(Event::Status(format!("Backup non riuscito: {e:#}"))),
                    },
                    Request::Export(path) => match catalog.export_json(&path) {
                        Ok(()) => {
                            send(Event::Status(format!("Export salvato: {}", path.display())))
                        }
                        Err(e) => send(Event::Status(format!("Export non riuscito: {e:#}"))),
                    },
                    Request::Shutdown => {
                        drop(pool);
                        send(Event::Stopped);
                        break;
                    }
                }
            }
        });
        Self {
            high,
            low,
            events,
            generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn annotation_save_remains_available_while_decoder_is_stalled() {
        use std::{os::unix::fs::PermissionsExt, time::Instant};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let data = tempfile::tempdir().unwrap();
        let worker = data.path().join("stalled-worker");
        let marker = data.path().join("decoder-started");
        let quoted = marker.to_string_lossy().replace('\'', "'\\''");
        std::fs::write(
            &worker,
            format!("#!/bin/sh\n: > '{quoted}'\nexec /bin/sleep 5\n"),
        )
        .unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        let service = Service::start(
            root.clone(),
            data.path().join("db"),
            worker,
            egui::Context::default(),
        );
        service.generation.store(1, Ordering::Relaxed);
        service
            .high
            .send(Request::Scan {
                folder: root.join("corpus"),
                generation: 1,
            })
            .unwrap();
        let Event::Scanned { items, .. } =
            service.events.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("scan failed")
        };
        let item = items[0].clone();
        service
            .low
            .send(Request::Decode {
                item: item.clone(),
                edge: 320,
                urgent: false,
                generation: 1,
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(marker.exists(), "decoder did not start");
        service
            .high
            .send(Request::Save {
                id: item.id,
                expected: 0,
                annotation: Annotation {
                    rating: 5,
                    ..Default::default()
                },
            })
            .unwrap();
        assert!(matches!(
            service.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Saved { revision: 1, .. }
        ));
        service.high.send(Request::Shutdown).unwrap();
        assert!(matches!(
            service.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Stopped
        ));
    }
    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn asynchronous_scan_save_undo_and_invalid_mutation() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let data = tempfile::tempdir().unwrap();
        let worker = std::env::var_os("TR_WORKER_BINARY")
            .map(PathBuf::from)
            .expect("TR_WORKER_BINARY");
        let service = Service::start(
            root.clone(),
            data.path().into(),
            worker,
            egui::Context::default(),
        );
        service.generation.store(1, Ordering::Relaxed);
        service
            .high
            .send(Request::Scan {
                folder: root.join("corpus"),
                generation: 1,
            })
            .unwrap();
        let items = match service
            .events
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            Event::Scanned { items, .. } => items,
            _ => panic!("expected completed scan"),
        };
        assert_eq!(items.len(), 12);
        assert!(items.iter().all(|i| i.approved));
        let id = items[0].id.clone();
        let annotation = Annotation {
            rating: 4,
            label: tr_core::Label::Blue,
            keywords: vec!["test durevole".into()],
        };
        service
            .high
            .send(Request::Save {
                id: id.clone(),
                expected: 0,
                annotation: annotation.clone(),
            })
            .unwrap();
        match service.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Saved {
                annotation: a,
                revision,
                ..
            } => {
                assert_eq!(a, annotation);
                assert_eq!(revision, 1);
            }
            _ => panic!("save failed"),
        }
        service.high.send(Request::Undo).unwrap();
        match service.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Saved {
                annotation,
                revision,
                ..
            } => {
                assert_eq!(annotation, Annotation::default());
                assert_eq!(revision, 2);
            }
            _ => panic!("undo failed"),
        }
        service
            .high
            .send(Request::Save {
                id: id.clone(),
                expected: 2,
                annotation: Annotation {
                    rating: 9,
                    ..Default::default()
                },
            })
            .unwrap();
        assert!(matches!(
            service.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::SaveFailed { .. }
        ));
        service.high.send(Request::Shutdown).unwrap();
        assert!(matches!(
            service.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Stopped
        ));
        let catalog = Catalog::open(data.path()).unwrap();
        assert_eq!(catalog.get(&id).unwrap().revision, 2);
        assert_eq!(catalog.get(&id).unwrap().annotation, Annotation::default());
    }
}
