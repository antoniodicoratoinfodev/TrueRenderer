//! Cache deletion revokes image work before touching disk and resumes after its result.
use super::*;
use std::sync::mpsc::{Receiver, TryRecvError};

pub(super) struct CacheAction {
    rx: Receiver<Result<(), String>>,
    cleared_folder: Option<PathBuf>,
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};

    fn resident(app: &mut TrueRenderer, item: &Item, request: PreviewRequest) -> CachedImage {
        let pyramid = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(16, 8, vec![[0.2, 0.3, 0.4, 1.]; 128]).unwrap(),
                request,
            )
            .unwrap(),
        );
        let cached = CachedImage {
            digest: item.digest.clone(),
            info: RasterInfo {
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
            },
            histogram: pyramid.source().histogram(),
            pyramid,
            touched: app.frame_number,
            transport: "test",
            worker_pid: None,
        };
        app.cache.insert((item.id.clone(), request), cached.clone());
        cached
    }

    #[test]
    fn clear_retries_readers_and_discards_old_images_without_changing_user_data() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.start_folder_preparation(true);
        let item = app.state.items[1].clone();
        let request = app.folder_load.request.unwrap();
        let cached = resident(&mut app, &item, request);
        let preview = tr_render::PreparedPreview {
            image: cached.pyramid.clone(),
            histogram: cached.histogram,
        };
        let manager = app.service.cache.clone();
        manager
            .store_preview(
                &app.folder,
                &item.digest,
                request,
                &cached.info,
                &preview,
                &|| false,
            )
            .unwrap();
        assert!(manager.stats().entries > 0);
        app.command(Command::Select {
            id: item.id.clone(),
            extend: true,
        });
        app.state.transform.zoom = Some(1.75);
        app.capture_picker_areas(&cached.pyramid, 5, 5, true);
        let selected = app.state.selected.clone();
        let sources: Vec<_> = app
            .state
            .items
            .iter()
            .map(|i| (i.path.clone(), std::fs::read(&i.path).unwrap()))
            .collect();
        let backup = dir.path().join("data/backups/keep");
        std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
        std::fs::write(&backup, b"durable backup").unwrap();
        app.cache_settings.save(&app.settings_data).unwrap();
        let settings = std::fs::read(app.settings_data.join("settings.json")).unwrap();
        let generation = app.generation;
        let reader = manager.read_demand();
        app.start_cache_action(false);
        assert!(app.cache.is_empty());
        assert!(app.editing.picker_areas.iter().all(Option::is_none));
        assert!(app.presenter.is_idle());
        app.ensure_image(&item, 512);
        app.background_demand(&ctx);
        assert!(app.demand.is_empty() && app.demand_jobs.is_empty());
        app.start_cache_action(false); // A second action cannot replace its receiver.
        app.set_quality(PreviewQuality::Full);
        assert_eq!(app.generation, generation + 1);
        std::thread::sleep(Duration::from_millis(350));
        app.poll_cache_action(&ctx);
        assert!(
            app.clearing_current_folder(),
            "Reader contention must retry"
        );
        drop(reader);
        settle(&mut app, &ctx, true);
        assert_eq!(app.status, "Cache della cartella svuotata");
        assert_eq!(manager.stats().entries, 0);
        assert!(matches!(
            manager.load_preview(&app.folder, &item.digest, request, &manager.memory, &|| {
                false
            }),
            crate::cache::Lookup::Missing
        ));

        // A completed decode already queued before revocation cannot restore
        // either the old pixels or progress in the restarted preparation.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        app.service.events = rx;
        tx.send(Event::Image {
            id: item.id.clone(),
            request,
            generation,
            result: Box::new(Ok(crate::service::PreviewDecoded {
                digest: item.digest.clone(),
                info: cached.info,
                prepared: preview,
                transport: "test",
                worker_pid: None,
            })),
        })
        .unwrap();
        app.poll(&ctx);
        assert!(app.cache.is_empty());
        assert_eq!(app.folder_progress(), (0, 2, 0.));
        app.ensure_image(&item, 512);
        assert_eq!(app.demand_jobs.len(), 1);
        assert_eq!(app.demand_jobs[0].generation, generation + 1);
        assert!(app.demand_jobs[0].resident.is_none());
        assert_eq!(app.state.selected, selected);
        assert_eq!(app.state.current.as_deref(), Some(item.id.as_str()));
        assert_eq!(app.state.transform.zoom, Some(1.75));
        for (path, bytes) in sources {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(backup).unwrap(), b"durable backup");
        assert_eq!(
            std::fs::read(app.settings_data.join("settings.json")).unwrap(),
            settings
        );
    }

    #[test]
    fn clear_keeps_pending_scan_and_reports_failure_without_stalling_preparation() {
        let (_dir, ctx, mut app) = app();
        assert!(app.scanning);
        app.start_cache_action(false);
        settle(&mut app, &ctx, false);
        assert!(app.scanning);
        settle(&mut app, &ctx, true);
        assert_eq!(app.folder_progress(), (0, 2, 0.));
        let owner = app.folder.join(crate::cache::NAME).join("OWNER");
        std::fs::write(&owner, b"foreign cache").unwrap();
        app.start_cache_action(false);
        settle(&mut app, &ctx, true);
        assert!(app.status.starts_with("Cache:"), "{}", app.status);
        assert!(!app.clearing_current_folder());
        assert_eq!(app.folder_progress(), (0, 2, 0.));
        assert_eq!(std::fs::read(owner).unwrap(), b"foreign cache");
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.demand_jobs.is_empty() {
            app.poll(&ctx);
            app.background_demand(&ctx);
            assert!(
                Instant::now() < deadline,
                "Preparation did not resume after recipe loading"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.demand_jobs.len(), 1);
    }

    #[test]
    fn finishing_clear_after_navigation_does_not_restart_the_new_folder() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let manager = app.service.cache.clone();
        let reader = manager.read_demand();
        app.start_cache_action(false);
        let folder = dir.path().join("another-folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::copy(&app.state.items[0].path, folder.join("new.png")).unwrap();
        let folder = folder.canonicalize().unwrap();
        app.open_folder(folder.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.folder != folder || app.scanning {
            app.poll(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.clearing_current_folder());
        assert!(app.cache_action.is_some());
        let item = app.state.items[0].clone();
        let request = app.folder_load.request.unwrap();
        let image = resident(&mut app, &item, request);
        app.rebuild.clear(); // New folder already prepared while old clear waited.
        let generation = app.generation;
        drop(reader);
        settle(&mut app, &ctx, true);
        assert_eq!(app.generation, generation);
        assert!(app.rebuild.is_empty());
        assert_eq!(
            app.cache[&(item.id, request)].pyramid.id(),
            image.pyramid.id()
        );
    }
}

impl CacheAction {
    pub(super) fn new(rx: Receiver<Result<(), String>>) -> Self {
        Self {
            rx,
            cleared_folder: None,
        }
    }
}

impl TrueRenderer {
    pub(super) fn clearing_current_folder(&self) -> bool {
        self.cache_action
            .as_ref()
            .and_then(|action| action.cleared_folder.as_ref())
            == Some(&self.folder)
    }

    pub(super) fn poll_cache_action(&mut self, ctx: &egui::Context) {
        let Some(action) = &self.cache_action else {
            return;
        };
        let result = match action.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Disconnected) => Err("Operazione cache interrotta".into()),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
        };
        let action = self.cache_action.take().unwrap();
        let cleared = action.cleared_folder.is_some();
        if action.cleared_folder.as_ref() == Some(&self.folder) {
            // A scan keeps its own identity. If still pending, its completion
            // starts preparation with the new inventory and an empty signature.
            // Resume even on failure, without claiming the disk was cleared.
            self.resume_loading_after_cache_clear();
        }
        self.status = match result {
            Ok(()) if cleared => "Cache della cartella svuotata".into(),
            Ok(()) => "Impostazioni/cache aggiornate".into(),
            Err(error) => error,
        };
        ctx.request_repaint();
    }

    /// Shared by preferences and the loading popup; never invents a catalogue scan.
    pub(super) fn start_cache_action(&mut self, save: bool) {
        if self.cache_action.is_some() {
            return;
        }
        if save && self.cache_settings.quality != self.service.cache.settings().quality {
            self.quality_overrides.clear();
        }
        self.errors.clear();
        if save {
            self.apply_settings();
        } else {
            self.reset_loading_for_cache_clear();
            // The same epoch cancels lookup, decoder and optional disk writes.
            // Manager serializes deletion with the complete active write job.
            self.invalidate_previews();
        }
        let cache = self.service.cache.clone();
        let folder = self.folder.clone();
        let data = self.settings_data.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.cache_action = Some(CacheAction {
            rx,
            cleared_folder: (!save).then(|| folder.clone()),
        });
        self.status = "Aggiornamento cache…".into();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                if save {
                    cache.save_current(&data)?;
                }
                let start = Instant::now();
                loop {
                    match cache.maintain(&folder, !save) {
                        Err(e)
                            if e.downcast_ref::<std::io::Error>()
                                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
                                && start.elapsed() < Duration::from_secs(3) =>
                        {
                            std::thread::sleep(Duration::from_millis(25))
                        }
                        result => return result,
                    }
                }
            })()
            .map_err(|e| format!("Cache: {e:#}"));
            let _ = tx.send(result);
        });
    }
}
