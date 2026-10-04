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
    finishing: bool,
    viewer_edge: u32,
    refining: Option<(String, PreviewRequest)>,
}

#[derive(Default)]
pub(super) struct Probe {
    clears: Vec<serde_json::Value>,
    stale_sources: HashSet<u64>,
    decodes_before: u64,
    failure: Option<String>,
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

    pub(super) fn reset_loading_for_cache_clear(&mut self) {
        // Preserve a running session's foreground/background choice, including
        // "Continue in background". Completed/cancelled sessions use preferences.
        let active = self.scanning || !self.rebuild.is_empty() || self.folder_load.finishing;
        let foreground = self.folder_load.foreground;
        let popup = self.folder_load.popup;
        let scan_failed = self.folder_load.scan_failed;
        self.begin_folder_loading();
        self.folder_load.scan_failed = scan_failed;
        self.folder_load.popup |= popup;
        if active {
            self.folder_load.foreground = foreground;
            self.folder_load.popup = popup || foreground;
        }
    }

    pub(super) fn resume_loading_after_cache_clear(&mut self) {
        // Deleting derived files cannot turn an incomplete inventory into a
        // successfully prepared folder. A successful rescan can restart it.
        if !self.folder_load.scan_failed {
            self.start_folder_preparation(true);
        }
    }

    pub(super) fn start_folder_preparation(&mut self, force: bool) {
        // Existing measurement probes deliberately control their own demand.
        if (self.smoke || self.navigation_probe.is_some())
            && !std::env::args()
                .any(|a| a == "--folder-loading-smoke" || a == "--navigation-prepared-folder")
        {
            return;
        }
        if self.scanning || self.clearing_current_folder() {
            return;
        }
        let settings = self.service.cache.settings();
        let request = PreviewRequest {
            raw_wb: Default::default(),
            raw_engine: settings.raw_engine,
            quality: settings.quality,
            // Build the finest detail of the selected quality now: native for
            // Full, capped at 2048 for Standard. The same graph serves every
            // fitted viewer and zoom, without a second RAW development.
            edge: 0,
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
        self.folder_load.finishing = true;
        self.folder_load.viewer_edge = 0;
        self.folder_load.refining = None;
        if self.folder_load.foreground {
            self.preparation_paused = false;
            self.cache.clear();
            self.presenter.release_optional_frames();
        }
    }

    pub(super) fn folder_loading_blocks(&self) -> bool {
        self.folder_load.foreground
            && (self.scanning
                || !self.rebuild.is_empty()
                || self.folder_load.finishing
                || self.clearing_current_folder())
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
        self.folder_load.finishing = false;
        self.folder_load.foreground = false;
        self.folder_load.refining = None;
        // Next frame's ViewDemand cancels only this consumer, never saves/scans.
    }

    pub(super) fn folder_preparation_demand(&mut self, ctx: &egui::Context) -> bool {
        if self.clearing_current_folder() {
            ctx.request_repaint_after(Duration::from_millis(50));
            return true;
        }
        // Persist the prepared viewer before advancing to another image. The
        // bounded writer runs independently; UI and visible cache reads proceed.
        // Its submission is registered before delivery of the decoded result.
        if self.folder_load.finishing && self.service.cache.preview_writes_pending() {
            if let Some(item) = self.rebuild.front() {
                let key = self.folder_load.refining.clone().unwrap_or_else(|| {
                    (
                        item.id.clone(),
                        self.preview_request_for_mode(item, 0, false),
                    )
                });
                if let Some(cached) = self.cache.get_mut(&key) {
                    // A slow or failed write must not let residency trimming
                    // discard this photo before the RAM refinement can consume it.
                    cached.touched = self.frame_number;
                }
                if self.pending_images.contains(&key) || self.cache.contains_key(&key) {
                    self.demand.insert(key);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
            return true;
        }
        if self.rebuild.is_empty() {
            self.folder_load.finishing = false;
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
        if self.folder_load.request.is_none() {
            return true;
        }
        if self.folder_load.viewer_edge == 0 {
            // Cover the physical window, including a viewer opened from the grid.
            // The native result is detached on the I/O lane, never copied by UI.
            let size = ctx.content_rect().size() * ctx.pixels_per_point();
            self.folder_load.viewer_edge = size.x.max(size.y).ceil().clamp(1., 4096.) as u32;
        }
        // Finish resident/failed entries without resubmitting an already known error.
        for _ in 0..16 {
            let Some(item) = self.rebuild.front().cloned() else {
                return true;
            };
            // Saved engine/WB belong to the photo, not to the current global
            // selection or the temporary Before view. Resolve them before any
            // preparation request, including photos never visited in the UI.
            self.ensure_edit_loaded(&item);
            let entry = self.editing.entries.get(&item.id);
            if entry.is_some_and(|entry| entry.loaded.is_none() && entry.error.is_some()) {
                self.rebuild.pop_front();
                self.folder_load.errors += 1;
                continue;
            }
            if !entry.is_some_and(|entry| entry.loaded.is_some()) {
                return true;
            }
            let request = self.preview_request_for_mode(&item, 0, false);
            let native_key = (item.id.clone(), request);
            let viewer_key = (
                item.id.clone(),
                PreviewRequest {
                    edge: self.folder_load.viewer_edge,
                    ..request
                },
            );
            if self
                .folder_load
                .refining
                .as_ref()
                .is_some_and(|key| key != &viewer_key)
            {
                self.folder_load.refining = None;
            }
            if self.folder_load.refining.is_none() && self.cache.contains_key(&native_key) {
                self.folder_load.refining = Some(viewer_key.clone());
            }
            let key = self
                .folder_load
                .refining
                .clone()
                .unwrap_or(native_key.clone());
            let failed = self.errors.contains_key(&format!("{}:{:?}", key.0, key.1));
            if self.cache.contains_key(&key) || failed {
                // Make an unused native ancestor reclaimable immediately, while
                // preserving the independently owned viewing preview and aliases.
                if let Some(native) = self.cache.get(&native_key) {
                    let source = native.pyramid.id();
                    if self
                        .cache
                        .get(&viewer_key)
                        .is_some_and(|c| c.pyramid.id() != source)
                        && !self
                            .cache
                            .iter()
                            .any(|(k, c)| c.pyramid.id() == source && self.demand.contains(k))
                    {
                        for cached in self.cache.values_mut().filter(|c| c.pyramid.id() == source) {
                            cached.touched = self.frame_number.saturating_sub(2);
                        }
                    }
                }
                self.rebuild.pop_front();
                self.folder_load.refining = None;
                self.folder_load.errors += usize::from(failed);
            } else if self.folder_load.refining.is_some() {
                // This phase also runs with persistence disabled or over quota.
                // Keep the native parent alive until its detached tail is ready.
                if let Some(native) = self.cache.get_mut(&native_key) {
                    native.touched = self.frame_number;
                }
                self.ensure_preview_request(&item, key.1, self.folder_preparation_priority());
                if self.cache.contains_key(&key) {
                    // Equivalent mip geometry aliases immediately. Do not add a
                    // 100 ms repaint wait per photo in an already warm folder.
                    continue;
                }
                return true;
            } else {
                break;
            }
        }
        if let Some(item) = self.rebuild.front() {
            let request = self.preview_request_for_mode(item, 0, false);
            let key = (item.id.clone(), request);
            if self.pending_images.contains(&key) {
                // The active decode owns most credits: keep its demand alive
                // instead of cancelling it when checking admission headroom.
                self.demand.insert(key);
                return true;
            }
        }
        if self.service.cache.memory.usage().reserved
            > self
                .service
                .cache
                .memory
                .usage()
                .limit
                .saturating_sub(self.service.cache.background_headroom())
        {
            self.trim_images_for_headroom(self.service.cache.background_headroom());
        }
        let memory = self.service.cache.memory.usage();
        if self.service.cache.under_pressure()
            || memory.reserved
                > memory
                    .limit
                    .saturating_sub(self.service.cache.background_headroom())
            || self.demand.iter().any(|key| {
                !self.cache.contains_key(key)
                    && !self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))
            })
        {
            self.folder_load.waiting = true;
            return true;
        }
        if let Some(item) = self.rebuild.front().cloned() {
            let request = self.preview_request_for_mode(&item, 0, false);
            self.ensure_preview_request(&item, request, self.folder_preparation_priority());
        }
        true
    }

    fn folder_preparation_priority(&self) -> PreviewPriority {
        if self.folder_load.foreground {
            PreviewPriority::Immediate
        } else {
            PreviewPriority::Background
        }
    }

    pub(super) fn prepared_viewer_edge(&self) -> u32 {
        self.folder_load.viewer_edge
    }

    pub(super) fn folder_progress(&self) -> (usize, usize, f32) {
        let total = self.rebuild_total + self.folder_load.skipped;
        let remaining = if self.folder_load.cancelled {
            self.folder_load.cancelled_remaining
        } else {
            self.rebuild.len()
        };
        let remaining = if self.folder_load.finishing && self.service.cache.preview_writes_pending()
        {
            remaining.max(1)
        } else {
            remaining
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
        if self.folder_load.request.is_none() && !self.scanning && !self.clearing_current_folder() {
            return;
        }
        let lang = self.cache_settings.language;
        let (_, _, progress) = self.folder_progress();
        let clearing = self.clearing_current_folder();
        let text = if clearing {
            lang.text("Svuotamento cache…").into()
        } else if self.scanning {
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
            egui::ProgressBar::new(if self.scanning || clearing {
                0.
            } else {
                progress
            })
            .desired_width(width)
            .desired_height(24.)
            .text(text)
            .animate(self.scanning || clearing),
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
        if self.clearing_current_folder() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(lang.text("Svuotamento cache…"));
            });
        } else if self.scanning {
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
        if self.folder_load.finishing && self.service.cache.preview_writes_pending() {
            ui.label(lang.text("Salvataggio anteprime…"));
        }
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
        ui.separator();
        if ui
            .add_enabled(
                self.cache_action.is_none(),
                egui::Button::new(lang.text("Svuota cache cartella")),
            )
            .clicked()
        {
            self.start_cache_action(false);
        }
    }

    pub(super) fn folder_loading_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(50));
        let failed = self.fatal
            || self.folder_load.errors > 0
            || self.loading_probe.failure.is_some()
            || self.started.elapsed() > Duration::from_secs(600);
        if failed || self.smoke_stage == 5 {
            let (done, total, _) = self.folder_progress();
            let report = serde_json::json!({"passed": !failed && total > 0 && done == total,
                "complete":true,"processed":done,"total":total,"errors":self.folder_load.errors,
                "foreground_and_background":self.smoke_stage==5,"screenshots":self.screenshots,
                "cache_clears":self.loading_probe.clears,"failure":self.loading_probe.failure,
                "scope":"Isolated native folder loading and cache clear during foreground/background preparation, disk deletion and fresh decoding. Fitted viewer and thumbnail preparation; not native 1:1, GPU readiness or guaranteed disk retention beyond configured quotas."});
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
                    self.smoke_stage = 10;
                }
            }
            10 if !self.pending_images.is_empty() || !self.cache.is_empty() => {
                self.clear_loading_smoke();
                self.smoke_stage = 11;
            }
            11 | 13 | 15 if self.cache_action.is_none() => {
                let passed = self.status == "Cache della cartella svuotata"
                    && self.service.cache.stats().entries == 0
                    && self.cache.is_empty()
                    && self.folder_progress().0 == 0;
                if let Some(clear) = self.loading_probe.clears.last_mut() {
                    clear["disk_empty_before_reload"] = passed.into();
                }
                if !passed {
                    self.loading_probe.failure = Some(format!("Clear incomplete: {}", self.status));
                }
                self.smoke_stage = match self.smoke_stage {
                    11 => 2,
                    13 => 14,
                    _ => 4,
                };
            }
            2 if self.rebuild.is_empty()
                && !self.folder_loading_blocks()
                && self.demand.iter().all(|key| self.cache.contains_key(key))
                && self.presenter.is_idle() =>
            {
                if !self.screenshots.contains("folder-loading-complete") {
                    self.capture_screenshot(ctx, "folder-loading-complete");
                } else {
                    self.verify_loading_regenerated();
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
                    self.clear_loading_smoke();
                    self.smoke_stage = 13;
                }
            }
            14 if !self.pending_images.is_empty() || !self.cache.is_empty() => {
                self.clear_loading_smoke();
                self.smoke_stage = 15;
            }
            4 if self.rebuild.is_empty()
                && !self.folder_load.finishing
                && !self.cache.is_empty()
                && self.presenter.is_idle() =>
            {
                self.verify_loading_regenerated();
                self.smoke_stage = 5;
            }
            _ => {}
        }
    }

    fn clear_loading_smoke(&mut self) {
        let generation = self.generation;
        self.loading_probe
            .stale_sources
            .extend(self.cache.values().map(|c| c.pyramid.id()));
        self.loading_probe.decodes_before = self.service.cache.stats().decode_jobs;
        let mut record = serde_json::json!({
            "foreground":self.folder_load.foreground,
            "pending_images":self.pending_images.len(), "resident_images":self.cache.len(),
            "generation_before":generation, "decode_jobs_before":self.loading_probe.decodes_before,
        });
        self.start_cache_action(false);
        let passed = self.generation == generation + 1
            && self.cache.is_empty()
            && self.pending_images.is_empty()
            && self.demand_jobs.is_empty()
            && self.presenter.is_idle();
        record["revoked_and_invalidated"] = passed.into();
        self.loading_probe.clears.push(record);
        if !passed {
            self.loading_probe.failure = Some("Old image work survived clear".into());
        }
    }

    fn verify_loading_regenerated(&mut self) {
        let decoded = self.service.cache.stats().decode_jobs - self.loading_probe.decodes_before;
        let passed = decoded > 0
            && self
                .cache
                .values()
                .all(|c| !self.loading_probe.stale_sources.contains(&c.pyramid.id()));
        if let Some(clear) = self.loading_probe.clears.last_mut() {
            clear["new_decode_jobs"] = decoded.into();
            clear["fresh_images_after_reload"] = passed.into();
        }
        if !passed {
            self.loading_probe.failure = Some("Folder did not regenerate fresh images".into());
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod retention_tests;

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};
    use eframe::egui;

    fn load_neutral(app: &mut TrueRenderer, item: &Item) {
        app.edit_result(
            item.id.clone(),
            Ok(tr_store::LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 0,
                revision: 0,
                recipe: tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine),
                can_undo: false,
                can_redo: false,
            }),
        );
    }

    #[test]
    fn folder_prepares_all_detail_of_the_selected_quality_for_every_engine() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        for engine in tr_core::decoder::RawEngine::choices() {
            app.cache_settings.raw_engine = engine;
            app.apply_settings();
            for quality in [PreviewQuality::Standard, PreviewQuality::Full] {
                app.set_quality(quality);
                ctx.set_pixels_per_point(2.);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1440., 900.),
                        )),
                        ..Default::default()
                    },
                    |_| app.start_folder_preparation(true),
                );
                output.textures_delta.clear();
                let request = app.folder_load.request.unwrap();
                let viewer = PreviewRequest {
                    edge: 2700,
                    quality,
                    ..request
                };
                let source = [6016, 4016];
                assert!(
                    tr_core::protocol::mip_geometry(source, request.maximum_level_edge()).1
                        <= tr_core::protocol::mip_geometry(source, viewer.maximum_level_edge()).1,
                    "Folder completion must cover the fitted D750 viewer: {request:?}",
                );
                assert_eq!(request.raw_engine, engine);
                assert_eq!(
                    request.edge, 0,
                    "Folder preparation must include the selected quality's finest detail"
                );
            }
        }
    }

    #[test]
    fn folder_preparation_uses_saved_raw_recipe_before_scheduling_pixels() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let mut recipe =
            tr_core::editing::EditRecipe::neutral(tr_core::decoder::RawEngine::LibRawAhd);
        recipe.raw_wb.red = 1500;
        recipe.raw_wb.blue = 800;
        app.edit_result(
            item.id.clone(),
            Ok(tr_store::LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 1,
                revision: 1,
                recipe: recipe.clone(),
                can_undo: true,
                can_redo: false,
            }),
        );
        app.editing.show_original = true;
        app.start_folder_preparation(true);
        app.folder_preparation_demand(&ctx);
        let job = app
            .demand_jobs
            .iter()
            .find(|job| job.item.id == item.id)
            .unwrap();
        assert_eq!(job.request.raw_engine, recipe.raw_engine);
        assert_eq!(job.request.raw_wb, recipe.raw_wb);
    }

    #[test]
    fn thumbnail_readiness_does_not_complete_viewer_preparation() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.set_quality(PreviewQuality::Full);
        app.start_folder_preparation(true);
        let item = app.rebuild[0].clone();
        load_neutral(&mut app, &item);
        let request = PreviewRequest {
            edge: 512,
            ..app.folder_load.request.unwrap()
        };
        let source = Arc::new(
            ImageLevels::from_reference_mip(
                tr_core::color::LinearImage::new(752, 502, vec![[0.2, 0.3, 0.4, 1.]; 752 * 502])
                    .unwrap(),
                [6016, 4016],
                3,
                true,
            )
            .unwrap(),
        );
        let info = RasterInfo {
            shooting: None,
            scientific: None,
            reference_mip: None,
            width: 6016,
            height: 4016,
            source_width: 6016,
            source_height: 4016,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        };
        app.cache.insert(
            (item.id.clone(), request),
            CachedImage {
                digest: "a".repeat(64),
                info,
                histogram: source.source().histogram(),
                pyramid: source,
                touched: 0,
                transport: "test",
                worker_pid: None,
            },
        );
        app.folder_preparation_demand(&ctx);
        assert_eq!(app.folder_progress(), (0, 2, 0.));
        assert_eq!(app.demand_jobs.len(), 1);
        assert_eq!(app.demand_jobs[0].item.id, item.id);
        assert!(app.demand_jobs[0].resident.is_none());
    }

    #[test]
    fn completion_waits_for_persistence_and_cancel_does_not_block() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.cache_settings.folder_loading = crate::cache::FolderLoading::Foreground;
        app.apply_settings();
        app.rebuild.clear();
        app.service.cache.preview_writes.store(1, Ordering::Release);
        assert!(app.folder_loading_blocks());
        assert!(app.folder_preparation_demand(&ctx));
        assert!(app.folder_load.popup);
        assert_eq!(app.folder_progress(), (1, 2, 0.5));
        app.service.cache.preview_writes.store(0, Ordering::Release);
        app.folder_preparation_demand(&ctx);
        assert_eq!(app.folder_progress(), (2, 2, 1.));
        assert!(!app.folder_loading_blocks());
        assert!(!app.folder_load.popup);
        app.start_folder_preparation(true);
        let request = app.folder_load.request.unwrap();
        let key = (app.rebuild[0].id.clone(), request);
        app.pending_images.insert(key.clone());
        app.service.cache.preview_writes.store(1, Ordering::Release);
        app.folder_preparation_demand(&ctx);
        assert!(
            app.demand.contains(&key),
            "Writer must not cancel the active decode"
        );
        app.cancel_folder_preparation();
        assert!(!app.folder_loading_blocks());
        assert_eq!(app.folder_progress(), (0, 2, 0.));
        app.service.cache.preview_writes.store(0, Ordering::Release);
    }

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
        load_neutral(&mut app, &item);
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

    #[test]
    fn clearing_cache_revokes_loading_and_restarts_every_thumbnail() {
        for mode in [
            crate::cache::FolderLoading::Background,
            crate::cache::FolderLoading::Foreground,
        ] {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            app.cache_settings.folder_loading = mode;
            app.apply_settings();
            app.begin_folder_loading();
            app.start_folder_preparation(true);
            let request = app.folder_load.request.unwrap();
            app.rebuild.pop_front();
            let pending = (app.rebuild[0].id.clone(), request);
            app.pending_images.insert(pending.clone());
            app.demand.insert(pending);
            let generation = app.generation;
            app.start_cache_action(false);
            assert_eq!(
                app.generation,
                generation + 1,
                "Old loading jobs must be revoked"
            );
            assert!(app.pending_images.is_empty());
            assert!(app.demand.is_empty());
            app.background_demand(&ctx);
            assert!(
                app.demand_jobs.is_empty(),
                "No reads before clearing completes"
            );
            settle(&mut app, &ctx, true);
            assert_eq!(app.folder_progress(), (0, 2, 0.));
            assert_eq!(
                app.folder_loading_blocks(),
                mode == crate::cache::FolderLoading::Foreground
            );
        }
    }

    #[test]
    fn clear_restarts_completed_and_cancelled_loads_and_preserves_background_override() {
        for state in ["completed", "cancelled", "background"] {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            app.cache_settings.folder_loading = crate::cache::FolderLoading::Foreground;
            app.apply_settings();
            match state {
                "completed" => {
                    app.rebuild.clear();
                    app.folder_preparation_demand(&ctx);
                }
                "cancelled" => app.cancel_folder_preparation(),
                _ => {
                    app.folder_load.foreground = false;
                    app.preparation_paused = true;
                }
            }
            assert!(!app.folder_loading_blocks());
            app.start_cache_action(false);
            settle(&mut app, &ctx, true);
            assert_eq!(app.folder_progress(), (0, 2, 0.));
            assert_eq!(app.folder_loading_blocks(), state != "background");
            assert!(!app.folder_load.cancelled);
            assert_eq!(app.preparation_paused, state == "background");
        }
    }

    #[test]
    fn clear_does_not_certify_a_scan_that_failed_during_deletion() {
        let (_dir, ctx, mut app) = app();
        app.start_cache_action(false);
        app.folder_scan_failed();
        app.scanning = false;
        settle(&mut app, &ctx, false);
        assert!(app.folder_load.scan_failed);
        assert!(app.folder_load.request.is_none());
        assert!(app.rebuild.is_empty());
        assert!(!app.folder_loading_blocks());
    }

    #[test]
    fn clear_button_works_inside_modal_and_background_popup_in_both_languages() {
        fn label(shape: &egui::epaint::Shape, text: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == text => {
                    Some(t.visual_bounding_rect().center())
                }
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|s| label(s, text)),
                _ => None,
            }
        }
        for language in [
            crate::i18n::Language::Italian,
            crate::i18n::Language::English,
        ] {
            for foreground in [false, true] {
                let (_dir, ctx, mut app) = app();
                settle(&mut app, &ctx, true);
                app.cache_settings.language = language;
                app.start_folder_preparation(true);
                app.folder_load.foreground = foreground;
                app.folder_load.popup = true;
                let generation = app.generation;
                let mut draw = |events| {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1000., 700.),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| app.folder_loading_popup(ui.ctx()),
                    );
                    output.textures_delta.clear();
                    output
                        .shapes
                        .iter()
                        .find_map(|s| label(&s.shape, language.text("Svuota cache cartella")))
                };
                draw(vec![]);
                draw(vec![]);
                let pos = draw(vec![]).expect("Clear button is visible");
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
                assert_eq!(app.generation, generation + 1);
                assert!(app.clearing_current_folder());
                settle(&mut app, &ctx, true);
                assert_eq!(app.folder_progress(), (0, 2, 0.));
                assert_eq!(app.folder_loading_blocks(), foreground);
            }
        }
    }
}
