//! Folder preparation is a bounded consumer of the normal preview scheduler.
use super::*;
use std::hash::{Hash, Hasher};

#[derive(Default)]
pub(super) struct FolderLoad {
    pub request: Option<PreviewRequest>,
    signature: Option<u64>,
    pub errors: usize,
    skipped: usize,
    foreground: bool,
    popup: bool,
    cancelled: bool,
    cancelled_remaining: usize,
    waiting: bool,
    scan_failed: bool,
}

impl TrueRenderer {
    pub(super) fn begin_folder_loading(&mut self) {
        let foreground =
            self.service.cache.settings().folder_loading == crate::cache::FolderLoading::Foreground;
        self.folder_load = FolderLoad {
            foreground,
            popup: foreground,
            ..Default::default()
        };
    }

    pub(super) fn start_folder_preparation(&mut self, force: bool) {
        // Existing measurement probes deliberately control their own demand.
        if (self.smoke || self.navigation_probe.is_some())
            && !std::env::args().any(|a| a == "--folder-loading-smoke")
        {
            return;
        }
        if self.scanning {
            return;
        }
        let settings = self.service.cache.settings();
        let request = PreviewRequest {
            raw_engine: settings.raw_engine,
            quality: settings.quality,
            edge: 512,
        };
        let mut signature = 0u64;
        for item in &self.state.items {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            (&item.id, &item.observation, item.approved).hash(&mut hash);
            signature ^= hash.finish();
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (signature, self.state.items.len(), &self.folder, request).hash(&mut hash);
        let signature = hash.finish();
        // Periodic rescans, filters and sorting must not restart a completed job.
        if !force && self.folder_load.signature == Some(signature) {
            return;
        }
        self.rebuild = self
            .state
            .items
            .iter()
            .filter(|i| i.approved)
            .cloned()
            .collect();
        self.rebuild_total = self.rebuild.len();
        self.folder_load.signature = Some(signature);
        self.folder_load.request = Some(request);
        self.folder_load.skipped = self.state.items.len() - self.rebuild_total;
        self.folder_load.errors = 0;
        self.folder_load.cancelled = false;
        self.folder_load.cancelled_remaining = 0;
        self.folder_load.scan_failed = false;
        if self.folder_load.foreground {
            self.preparation_paused = false;
            self.cache.clear();
            self.presenter.release_optional_frames();
        }
    }

    pub(super) fn folder_loading_blocks(&self) -> bool {
        self.folder_load.foreground && (self.scanning || !self.rebuild.is_empty())
    }

    pub(super) fn folder_scan_failed(&mut self) {
        self.cancel_folder_preparation();
        self.folder_load.scan_failed = true;
        self.folder_load.signature = None;
    }

    pub(super) fn cancel_folder_preparation(&mut self) {
        self.folder_load.cancelled_remaining = self.rebuild.len();
        self.rebuild.clear();
        self.folder_load.cancelled = true;
        self.folder_load.foreground = false;
        // Next frame's ViewDemand cancels only this consumer, never saves/scans.
    }

    pub(super) fn folder_preparation_demand(&mut self, ctx: &egui::Context) -> bool {
        if self.rebuild.is_empty() {
            if self.folder_load.foreground && !self.scanning {
                self.folder_load.foreground = false;
                if self.folder_load.errors == 0 && self.folder_load.skipped == 0 {
                    self.folder_load.popup = false;
                }
            }
            return false;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        self.folder_load.waiting = false;
        if self.scanning || self.preparation_paused {
            return true;
        }
        let Some(request) = self.folder_load.request else {
            return true;
        };
        // Finish resident/failed entries without resubmitting an already known error.
        for _ in 0..16 {
            let Some(item) = self.rebuild.front() else {
                return true;
            };
            let key = (item.id.clone(), request);
            let failed = self.errors.contains_key(&format!("{}:{:?}", key.0, key.1));
            if self.cache.contains_key(&key) || failed {
                self.rebuild.pop_front();
                self.folder_load.errors += usize::from(failed);
            } else {
                break;
            }
        }
        if let Some(item) = self.rebuild.front() {
            let key = (item.id.clone(), request);
            if self.pending_images.contains(&key) {
                // The active decode owns most credits: keep its demand alive
                // instead of cancelling it when checking admission headroom.
                self.demand.insert(key);
                return true;
            }
        }
        if self.service.cache.memory.usage().reserved > self.service.cache.memory.usage().limit / 2
        {
            self.trim_images(true);
        }
        let memory = self.service.cache.memory.usage();
        if self.service.cache.under_pressure()
            || memory.reserved > memory.limit / 2
            || self.demand.iter().any(|key| {
                !self.cache.contains_key(key)
                    && !self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))
            })
        {
            self.folder_load.waiting = true;
            return true;
        }
        if let Some(item) = self.rebuild.front().cloned() {
            let key = (item.id.clone(), request);
            self.demand.insert(key.clone());
            if !self.pending_images.contains(&key)
                && !self
                    .demand_jobs
                    .iter()
                    .any(|j| j.item.id == item.id && j.request == request)
            {
                self.demand_jobs.push(crate::decode_pool::Job {
                    item,
                    request,
                    priority: if self.folder_load.foreground {
                        PreviewPriority::Immediate
                    } else {
                        PreviewPriority::Background
                    },
                    generation: self.generation,
                });
            }
        }
        true
    }

    pub(super) fn folder_progress(&self) -> (usize, usize, f32) {
        let total = self.rebuild_total + self.folder_load.skipped;
        let remaining = if self.folder_load.cancelled {
            self.folder_load.cancelled_remaining
        } else {
            self.rebuild.len()
        };
        let done = total.saturating_sub(remaining);
        (
            done,
            total,
            if total == 0 {
                1.0
            } else {
                done as f32 / total as f32
            },
        )
    }

    pub(super) fn folder_loading_button(&mut self, ui: &mut egui::Ui, compact: bool) {
        if self.folder_load.request.is_none() && !self.scanning {
            return;
        }
        let lang = self.cache_settings.language;
        let (_, _, progress) = self.folder_progress();
        let text = if self.scanning {
            lang.text("Conteggio file…").into()
        } else if self.folder_load.cancelled {
            lang.text("Annullato").into()
        } else {
            format!(
                "{} {:.0}%{}",
                lang.text("Cartella"),
                (progress * 100.).floor(),
                if self.folder_load.errors > 0 || self.folder_load.skipped > 0 {
                    " !"
                } else {
                    ""
                }
            )
        };
        let width = if compact { 100. } else { 134. };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 24.), egui::Sense::click());
        ui.put(
            rect,
            egui::ProgressBar::new(if self.scanning { 0. } else { progress })
                .desired_width(width)
                .desired_height(24.)
                .text(text)
                .animate(self.scanning),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                lang.text("Caricamento cartella"),
            )
        });
        if response.clicked() {
            self.folder_load.popup = true;
        }
    }

    pub(super) fn folder_loading_popup(&mut self, ctx: &egui::Context) {
        if !self.folder_load.popup && !self.folder_loading_blocks() {
            return;
        }
        if self.folder_loading_blocks() {
            egui::Modal::new(egui::Id::new("folder-loading-modal"))
                .show(ctx, |ui| self.folder_loading_contents(ui));
        } else {
            let mut open = true;
            egui::Window::new(self.cache_settings.language.text("Caricamento cartella"))
                .id(egui::Id::new("folder-loading-window"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| self.folder_loading_contents(ui));
            self.folder_load.popup &= open;
        }
    }

    fn folder_loading_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        ui.set_width(360.);
        if self.folder_loading_blocks() {
            ui.heading(lang.text("Caricamento cartella"));
        }
        ui.label(self.folder.display().to_string());
        if self.scanning {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(localized_format!(
                    lang,
                    "Conteggio file: {} trovati",
                    "Counting files: {} found",
                    self.state.items.len()
                ));
            });
        } else {
            let (done, total, progress) = self.folder_progress();
            ui.add(egui::ProgressBar::new(progress).text(format!(
                "{done} / {total} · {:.0}%",
                (progress * 100.).floor()
            )));
            ui.label(localized_format!(
                lang,
                "Errori: {} · Non supportati: {}",
                "Errors: {} · Unsupported: {}",
                self.folder_load.errors,
                self.folder_load.skipped
            ));
        }
        ui.label(lang.text("Preparazione delle miniature, non del dettaglio 1:1."));
        if self.folder_load.cancelled {
            ui.label(lang.text("Preparazione annullata"));
        }
        if self.folder_load.scan_failed {
            ui.colored_label(AMBER, lang.text("Lettura della cartella incompleta"));
        }
        if self.folder_load.waiting {
            ui.label(lang.text("In attesa delle viste attive o di memoria disponibile."));
        }
        if self.preparation_paused {
            ui.label(lang.text("Preparazione in pausa"));
        }
        ui.horizontal_wrapped(|ui| {
            if self.folder_loading_blocks() {
                if ui.button(lang.text("Continua in background")).clicked() {
                    self.folder_load.foreground = false;
                    self.folder_load.popup = false;
                }
            } else if !self.rebuild.is_empty()
                && ui.button(lang.text("Completa in primo piano")).clicked()
            {
                self.folder_load.foreground = true;
                self.preparation_paused = false;
                self.cache.clear();
                self.presenter.release_optional_frames();
            }
            if !self.rebuild.is_empty() {
                if ui
                    .button(lang.text(if self.preparation_paused {
                        "Riprendi"
                    } else {
                        "Pausa"
                    }))
                    .clicked()
                {
                    self.preparation_paused = !self.preparation_paused;
                }
                if ui.button(lang.text("Annulla preparazione")).clicked() {
                    self.cancel_folder_preparation();
                }
            }
        });
    }

    pub(super) fn folder_loading_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(50));
        let failed = self.fatal
            || self.folder_load.errors > 0
            || self.started.elapsed() > Duration::from_secs(600);
        if failed || self.smoke_stage == 5 {
            let (done, total, _) = self.folder_progress();
            let report = serde_json::json!({"passed": !failed && total > 0 && done == total,
                "complete":true,"processed":done,"total":total,"errors":self.folder_load.errors,
                "foreground_and_background":self.smoke_stage==5,"screenshots":self.screenshots,
                "scope":"Isolated native folder loading: modal, background detail popup and completion. Thumbnail readiness, not native full-resolution residency or guaranteed disk persistence."});
            let _ = std::fs::write(
                self.root.join("reports/folder-loading.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        match self.smoke_stage {
            0 if !self.scanning && !self.rebuild.is_empty() => {
                self.folder_load.foreground = true;
                self.folder_load.popup = true;
                self.preparation_paused = true;
                self.smoke_stage = 1;
                self.raw_engine_smoke_ready_at = Some(Instant::now());
            }
            1 if self
                .raw_engine_smoke_ready_at
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(250)) =>
            {
                if !self.screenshots.contains("folder-loading-foreground") {
                    self.capture_screenshot(ctx, "folder-loading-foreground");
                } else {
                    self.preparation_paused = false;
                    self.smoke_stage = 2;
                }
            }
            2 if self.rebuild.is_empty()
                && !self.folder_loading_blocks()
                && self.demand.iter().all(|key| self.cache.contains_key(key))
                && self.presenter.is_idle() =>
            {
                if !self.screenshots.contains("folder-loading-complete") {
                    self.capture_screenshot(ctx, "folder-loading-complete");
                } else {
                    self.start_folder_preparation(true);
                    self.folder_load.foreground = false;
                    self.folder_load.popup = true;
                    self.preparation_paused = true;
                    self.smoke_stage = 3;
                    self.raw_engine_smoke_ready_at = Some(Instant::now());
                }
            }
            3 if self
                .raw_engine_smoke_ready_at
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(250)) =>
            {
                if !self.screenshots.contains("folder-loading-background") {
                    self.capture_screenshot(ctx, "folder-loading-background");
                } else {
                    self.preparation_paused = false;
                    self.smoke_stage = 4;
                }
            }
            4 if self.rebuild.is_empty() => self.smoke_stage = 5,
            _ => {}
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use crate::ui::settings_regressions::{app, settle};
    use eframe::egui;

    #[test]
    fn clicking_the_compact_progress_opens_details() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.start_folder_preparation(true);
        let mut position = egui::Pos2::ZERO;
        let mut draw = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    position = ui.next_widget_position() + egui::vec2(50., 12.);
                    app.folder_loading_button(ui, true);
                },
            );
            output.textures_delta.clear();
            position
        };
        let pos = draw(vec![]);
        draw(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        draw(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert!(app.folder_load.popup);
    }

    #[test]
    fn loading_setting_migrates_and_round_trips_without_changing_quality() {
        let (dir, _, _) = app();
        let old: crate::cache::Settings = serde_json::from_str(r#"{"quality":"Full"}"#).unwrap();
        assert_eq!(old.folder_loading, crate::cache::FolderLoading::Background);
        let saved = crate::cache::Settings {
            folder_loading: crate::cache::FolderLoading::Foreground,
            ..old
        };
        saved.save(dir.path()).unwrap();
        assert_eq!(crate::cache::Settings::load(dir.path()).unwrap(), saved);
    }

    #[test]
    fn cancellation_rescan_and_sort_do_not_fabricate_completion() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.start_folder_preparation(true);
        assert_eq!(app.folder_progress(), (0, 2, 0.));
        app.rebuild.pop_front();
        app.cancel_folder_preparation();
        assert_eq!(app.folder_progress(), (1, 2, 0.5));
        app.state.items.reverse();
        app.start_folder_preparation(false);
        assert!(app.folder_load.cancelled);
        assert_eq!(app.folder_progress(), (1, 2, 0.5));
        app.begin_folder_loading();
        app.start_folder_preparation(false);
        assert_eq!(app.folder_progress(), (0, 2, 0.));
    }

    #[test]
    fn errors_skips_pause_and_active_credit_pressure_are_bounded() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.state.items[1].approved = false;
        app.start_folder_preparation(true);
        assert_eq!(app.folder_load.skipped, 1);
        let item = app.rebuild[0].clone();
        let request = app.folder_load.request.unwrap();
        app.preparation_paused = true;
        app.folder_preparation_demand(&ctx);
        assert!(app.demand_jobs.is_empty());
        app.preparation_paused = false;
        let key = (item.id.clone(), request);
        app.pending_images.insert(key.clone());
        let memory = app.service.cache.memory.clone();
        let _working = memory.try_reserve(memory.usage().limit / 2).unwrap();
        app.folder_preparation_demand(&ctx);
        assert!(
            app.demand.contains(&key),
            "Active decode must survive its own memory reservation"
        );
        app.pending_images.clear();
        app.errors
            .insert(format!("{}:{request:?}", item.id), "Unreadable test".into());
        app.folder_preparation_demand(&ctx);
        assert!(app.rebuild.is_empty());
        assert_eq!(app.folder_load.errors, 1);
        assert_eq!(app.folder_progress(), (2, 2, 1.));
    }

    #[test]
    fn foreground_blocks_until_complete_without_changing_selection_or_zoom() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let selected = app.state.current.clone();
        app.state.transform.zoom = Some(1.7);
        app.cache_settings.folder_loading = crate::cache::FolderLoading::Foreground;
        app.apply_settings();
        assert!(app.folder_loading_blocks());
        app.rebuild.clear();
        app.folder_preparation_demand(&ctx);
        assert!(!app.folder_loading_blocks());
        assert!(!app.folder_load.popup);
        assert_eq!(app.state.current, selected);
        assert_eq!(app.state.transform.zoom, Some(1.7));
        app.start_folder_preparation(true);
        app.folder_scan_failed();
        assert!(!app.folder_loading_blocks());
        assert!(app.folder_load.scan_failed);
    }
}
