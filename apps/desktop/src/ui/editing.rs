use super::*;
use tr_core::editing::{CurvePoint, EditRecipe, RgbAreaSample, sample_rgb_area};
use tr_store::LoadedEdit;
mod advanced;
#[cfg(test)]
mod controls_tests;
mod curve;
mod transfer;

#[derive(Clone, Copy)]
enum ResetGroup {
    Light,
    Curve,
    Color,
}
impl ResetGroup {
    fn apply(self, recipe: &mut EditRecipe) {
        // Preserve the native development and process version of the revision.
        match self {
            Self::Light => {
                recipe.exposure_ev = 0.;
                recipe.brightness = 0.;
                recipe.contrast = 0.;
                recipe.highlights = 0.;
                recipe.shadows = 0.;
                recipe.whites = 0.;
                recipe.blacks = 0.;
            }
            Self::Curve => recipe.curve.clear(),
            Self::Color => {
                recipe.temperature = 0.;
                recipe.tint = 0.;
                recipe.saturation = 0.;
                recipe.vibrance = 0.;
                recipe.protect_warm = false;
            }
        }
    }
}

fn reset_group_button(
    ui: &mut egui::Ui,
    label: &str,
    group: ResetGroup,
    draft: &mut EditRecipe,
) -> bool {
    let mut reset = draft.clone();
    group.apply(&mut reset);
    if ui
        .add_enabled(
            reset != *draft,
            egui::Button::new(label).small().frame_when_inactive(false),
        )
        .clicked()
    {
        *draft = reset;
        true
    } else {
        false
    }
}

// Keep the numeric field and slider in one response so a drag still commits once.
fn adjustment<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    suffix: &str,
    step: Option<f64>,
) -> egui::Response {
    let control_id = ui.make_persistent_id(label);
    ui.scope_builder(egui::UiBuilder::new().id(control_id), |ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        ui.spacing_mut().interact_size.y = 20.;
        ui.spacing_mut().button_padding.y = 2.;
        let speed = step.unwrap_or_else(|| {
            if Num::INTEGRAL {
                1.
            } else {
                (range.end().to_f64() - range.start().to_f64()) / 1000.
            }
        });
        let (label_id, number) = ui
            .horizontal(|ui| {
                let label_id = ui.label(label).id;
                let number = ui
                    .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::DragValue::new(value)
                                .range(range.clone())
                                .speed(speed)
                                .suffix(suffix),
                        )
                        .labelled_by(label_id)
                    })
                    .inner;
                (label_id, number)
            })
            .inner;
        let slider = ui
            .scope(|ui| {
                ui.spacing_mut().slider_width = ui.available_width();
                let mut slider = egui::Slider::new(value, range)
                    .show_value(false)
                    .logarithmic(suffix == " K");
                if let Some(step) = step {
                    slider = slider.step_by(step);
                }
                ui.add(slider).labelled_by(label_id)
            })
            .inner;
        number.union(slider)
    })
    .inner
}

fn apple_wb_controls(
    ui: &mut egui::Ui,
    lang: Language,
    wb: &mut tr_core::decoder::RawWhiteBalance,
) -> (bool, bool) {
    let mut changed = false;
    let mut commit = false;
    let as_shot = wb.is_as_shot();
    let mut kelvin = if as_shot { 6500 } else { wb.apple_temperature };
    let mut tint = wb.apple_tint;
    // Keep the row and widget IDs stable when the first drag leaves as-shot.
    ui.add_sized(
        [ui.available_width(), 32.],
        egui::Label::new(lang.text(if as_shot {
            "Come scattato · il controllo manuale parte da 6500 K"
        } else {
            "Personalizzato"
        }))
        .wrap(),
    );
    let temperature = adjustment(
        ui,
        lang.text("Temperatura"),
        &mut kelvin,
        2000..=50000,
        " K",
        Some(1.),
    );
    let tint_response = adjustment(
        ui,
        lang.text("Tinta RAW"),
        &mut tint,
        -150..=150,
        "",
        Some(1.),
    );
    if temperature.changed() || tint_response.changed() {
        wb.apple_temperature = kelvin;
        wb.apple_tint = tint;
        changed = true;
    }
    for response in [temperature, tint_response] {
        commit |= response.drag_stopped() || (response.changed() && !response.dragged());
    }
    if ui
        .add_enabled(
            !as_shot,
            egui::Button::new(lang.text("WB RAW come scattato")),
        )
        .clicked()
    {
        *wb = Default::default();
        changed = true;
        commit = true;
    }
    (changed, commit)
}

fn adjustment_heading(
    ui: &mut egui::Ui,
    lang: Language,
    title: &str,
    reset: &str,
    group: ResetGroup,
    draft: &mut EditRecipe,
) -> bool {
    ui.add_space(4.);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(lang.text(title)).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            reset_group_button(ui, lang.text(reset), group, draft)
        })
        .inner
    })
    .inner
}

#[derive(Default)]
pub(super) struct EditEntry {
    pub loaded: Option<LoadedEdit>,
    pub draft: Option<EditRecipe>,
    pub loading: bool,
    pub pending: bool,
    pub error: Option<String>,
}
impl EditEntry {
    fn dirty(&self) -> bool {
        self.loaded
            .as_ref()
            .zip(self.draft.as_ref())
            .is_some_and(|(saved, draft)| saved.recipe != *draft)
    }
}

type PreviewOutcome = (
    u64,
    String,
    u64,
    u32,
    bool,
    EditRecipe,
    Result<Arc<ImageLevels>, String>,
);
type ProofCapture = (Vec<u8>, [u32; 2], [f32; 4], &'static str);

mod continuity_probe;
#[cfg(all(test, any(windows, target_os = "macos")))]
mod continuity_tests;
type ThumbnailOutcome = (
    u64,
    String,
    String,
    u64,
    EditRecipe,
    Result<(Arc<ImageLevels>, [[u32; 256]; 3]), String>,
);
struct ThumbnailPreview {
    id: String,
    recipe: EditRecipe,
    image: Arc<ImageLevels>,
    histogram: [[u32; 256]; 3],
    touched: u64,
}
struct EditPreview {
    source: u64,
    proof: bool,
    recipe: EditRecipe,
    image: Arc<ImageLevels>,
    touched: u64,
}
pub(super) struct EditingUi {
    pub(super) advanced: advanced::Controls,
    continuity: continuity_probe::Probe,
    clipboard: transfer::Clipboard,
    wb_pending: bool,
    pub(super) wb_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    wb_error: Option<(String, String)>,
    wb_smoke_started: bool,
    wb_smoke_result: Option<String>,
    pub entries: HashMap<String, EditEntry>,
    pub show_original: bool,
    pub verify_final: Option<String>,
    pub output_proof: bool,
    previews: HashMap<String, EditPreview>,
    inflight: Option<(String, u64, EditRecipe)>,
    preview_epoch: u64,
    inflight_epoch: u64,
    refinement_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    interactive_millis: Arc<std::sync::atomic::AtomicU64>,
    changed_at: HashMap<String, Instant>,
    revision: u64,
    pub(super) raw_wb_anchor: HashMap<String, tr_core::decoder::RawWhiteBalance>,
    pub(super) display_wb: HashMap<String, tr_core::decoder::RawWhiteBalance>,
    thumbnail_started: HashMap<String, Instant>,
    tx: std::sync::mpsc::Sender<PreviewOutcome>,
    rx: std::sync::mpsc::Receiver<PreviewOutcome>,
    preview_errors: HashMap<String, PreviewFailure>,
    pub picker_error: Option<(String, String)>,
    pub picker_side: u32,
    pub picker_areas: [Option<Result<RgbAreaSample, String>>; 2],
    smoke_ready_at: Option<Instant>,
    smoke_proof_capture: Option<ProofCapture>,
    smoke_proof_result: Option<serde_json::Value>,
    smoke_source_digest: Option<String>,
    smoke_export_started: bool,
    smoke_export_result: Option<serde_json::Value>,
    thumbnails: HashMap<u64, ThumbnailPreview>,
    thumbnail_inflight: HashSet<u64>,
    thumbnail_errors: HashMap<u64, (String, EditRecipe, String)>,
    thumbnail_tx: std::sync::mpsc::Sender<ThumbnailOutcome>,
    thumbnail_rx: std::sync::mpsc::Receiver<ThumbnailOutcome>,
}
struct PreviewFailure {
    source: u64,
    base: u32,
    proof: bool,
    recipe: EditRecipe,
    message: String,
}
// The neutral input already owns its lease. Reserve the new canonical pyramid
// plus filtering scratch, including transient old/new horizontal allocations
// and conservative axis-table overhead. No native decoder runs in this stage.
fn preview_working_bytes(mut width: u32, mut height: u32) -> u64 {
    let mut retained = u64::from(width) * u64::from(height) * 16;
    let mut peak = retained;
    while width > 1 || height > 1 {
        let (next_width, next_height) = (width.div_ceil(2), height.div_ceil(2));
        let next = u64::from(next_width) * u64::from(next_height) * 16;
        let horizontal = u64::from(next_width) * u64::from(height) * 16;
        let axes = (u64::from(next_width) + u64::from(next_height)) * 512;
        peak = peak.max(retained + next + 2 * horizontal + axes);
        retained += next;
        (width, height) = (next_width, next_height);
    }
    peak + 64 * 1024
}
impl Default for EditingUi {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (thumbnail_tx, thumbnail_rx) = std::sync::mpsc::channel();
        Self {
            advanced: Default::default(),
            continuity: Default::default(),
            clipboard: transfer::Clipboard::default(),
            wb_pending: false,
            wb_cancel: None,
            wb_error: None,
            wb_smoke_started: false,
            wb_smoke_result: None,
            entries: HashMap::new(),
            show_original: false,
            verify_final: None,
            output_proof: false,
            previews: HashMap::new(),
            inflight: None,
            preview_epoch: 0,
            inflight_epoch: 0,
            refinement_cancel: None,
            interactive_millis: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            changed_at: HashMap::new(),
            revision: 0,
            raw_wb_anchor: HashMap::new(),
            display_wb: HashMap::new(),
            thumbnail_started: HashMap::new(),
            tx,
            rx,
            preview_errors: HashMap::new(),
            picker_error: None,
            picker_side: 5,
            picker_areas: [None, None],
            smoke_ready_at: None,
            smoke_proof_capture: None,
            smoke_proof_result: None,
            smoke_source_digest: None,
            smoke_export_started: false,
            smoke_export_result: None,
            thumbnails: HashMap::new(),
            thumbnail_inflight: HashSet::new(),
            thumbnail_errors: HashMap::new(),
            thumbnail_tx,
            thumbnail_rx,
        }
    }
}
impl TrueRenderer {
    pub(super) fn raw_wb_result(
        &mut self,
        job: crate::photo_export::WbJob,
        result: Result<tr_core::decoder::RawWhiteBalance, String>,
    ) {
        if !self
            .editing
            .wb_cancel
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &job.cancel))
        {
            return;
        }
        self.editing.wb_pending = false;
        self.editing.wb_cancel = None;
        if job.cancel.load(Ordering::Acquire) {
            self.status = self
                .cache_settings
                .language
                .text("Analisi WB RAW annullata")
                .to_owned();
            return;
        }
        if self.editing.wb_smoke_started {
            self.editing.wb_smoke_result = Some(
                result
                    .as_ref()
                    .map(|_| "ok".to_owned())
                    .unwrap_or_else(|e| e.clone()),
            );
        }
        let valid = self.generation == job.generation
            && self
                .state
                .items
                .iter()
                .any(|i| i.id == job.item.id && i.digest == job.item.digest)
            && self.editing.entries.get(&job.item.id).is_some_and(|e| {
                !e.pending
                    && e.loaded
                        .as_ref()
                        .is_some_and(|s| s.generation == job.revision)
                    && e.draft.as_ref().or(e.loaded.as_ref().map(|s| &s.recipe))
                        == Some(&job.recipe)
            });
        if !valid {
            return;
        }
        match result {
            Ok(wb) => {
                let mut recipe = job.recipe;
                recipe.raw_wb = wb;
                self.editing.entries.get_mut(&job.item.id).unwrap().draft = Some(recipe);
                self.sample = None;
                self.sample_from_current_render = false;
                self.clear_edit_preview();
                self.clear_edit_thumbnails(&job.item.id);
                self.commit_edit(&job.item.id);
            }
            Err(error) => self.editing.wb_error = Some((job.item.id, error)),
        }
    }
    pub(super) fn capture_picker_areas(
        &mut self,
        image: &ImageLevels,
        x: u32,
        y: u32,
        current: bool,
    ) {
        self.editing.picker_areas = [5, 11].map(|side| {
            (current && image.base_level() == 0).then(|| {
                sample_rgb_area(image.source(), x, y, side).map_err(|error| error.to_string())
            })
        });
        self.editing.picker_error = None;
    }

    pub(super) fn output_proof_for(&self, id: &str) -> bool {
        self.editing.output_proof && self.editing.verify_final.as_deref() == Some(id)
    }
    fn edit_source_matches(&self, id: &str, recorded: &str, digest: &str, source: u64) -> bool {
        if recorded == digest {
            return true;
        }
        // External scans record an observation token. The accepted cache result
        // belongs to that request's source epoch and contains its verified hash.
        // A source change invalidates the cache and advances the epoch first.
        recorded.starts_with("unverified:")
            && self
                .state
                .items
                .iter()
                .any(|item| item.id == id && item.digest == recorded)
            && self.cache.iter().any(|((photo, _), cached)| {
                photo == id && cached.digest == digest && cached.pyramid.id() == source
            })
    }
    pub(super) fn request_final_preview(&mut self, id: &str, proof: bool) {
        self.editing.verify_final = Some(id.into());
        self.editing.output_proof = proof;
        self.editing.show_original = false;
        self.quality_overrides
            .insert(id.into(), PreviewQuality::Full);
        self.sample = None;
        self.sample_from_current_render = false;
        self.clear_edit_preview();
    }
    pub(super) fn develop_smoke(&mut self, ctx: &egui::Context) {
        let proof_smoke = std::env::args().any(|arg| arg == "--output-proof-smoke");
        let args: Vec<_> = std::env::args().collect();
        let opened = args
            .iter()
            .position(|a| a == "--open")
            .and_then(|i| args.get(i + 1))
            .and_then(|p| std::fs::canonicalize(p).ok());
        ctx.request_repaint_after(Duration::from_millis(50));
        // Preserve the ordinary smoke budget for loading, saving and export,
        // in addition to the broker's complete native-analysis allowance.
        let timeout = develop_smoke_timeout(args.iter().any(|a| a == "--native-wb-smoke"));
        let timed_out = self.started.elapsed() > timeout || self.fatal;
        let target = self
            .state
            .items
            .iter()
            .find(|item| {
                opened
                    .as_ref()
                    .map_or(item.name == "02_Paesaggio_analitico.png", |p| {
                        &item.path == p
                    })
            })
            .cloned();
        if self.frame_number.is_multiple_of(30) || timed_out {
            let memory = self.service.cache.memory.usage();
            let diagnostic = serde_json::json!({
                "stage":self.smoke_stage,"scanning":self.scanning,"items":self.state.items.len(),
                "target_found":target.is_some(),"status":self.status,"errors":self.errors,
                "verify_final":self.editing.verify_final,"output_proof":self.editing.output_proof,
                "cache":self.cache.iter().map(|((id,request),c)|serde_json::json!({"id":id,"request":format!("{request:?}"),"source":c.pyramid.id(),"base":c.pyramid.base_level(),"size":c.pyramid.source_size(),"digest":c.digest})).collect::<Vec<_>>(),
                "demand":self.demand.iter().map(|k|format!("{k:?}")).collect::<Vec<_>>(),
                "pending":self.pending_images.iter().map(|k|format!("{k:?}")).collect::<Vec<_>>(),
                "edits":self.editing.entries.iter().map(|(id,e)|serde_json::json!({"id":id,"digest":e.loaded.as_ref().map(|e|&e.source_digest),"pending":e.pending,"loading":e.loading,"dirty":e.dirty(),"error":e.error})).collect::<Vec<_>>(),
                "memory":{"limit":memory.limit,"reserved":memory.reserved,"peak":memory.peak,"rejected":memory.rejected},
                "inflight":self.editing.inflight,"presenter_idle":self.presenter.is_idle(),"presenter_errors":self.presenter.has_errors(),
                "previews":self.editing.previews.iter().map(|(id,p)|serde_json::json!({"id":id,"source":p.source,"image":p.image.id(),"proof":p.proof,"base":p.image.base_level(),"size":p.image.source_size()})).collect::<Vec<_>>(),
                "preview_errors":self.editing.preview_errors.values().map(|p|p.message.as_str()).collect::<Vec<_>>(),
                "captures":self.presenter.captures().iter().map(|c|serde_json::json!({"source":c.source,"compute":c.compute,"size":c.region.size,"rect":format!("{:?}",c.rect),"clip":format!("{:?}",c.clip),"fully_visible":c.clip.contains_rect(c.rect)})).collect::<Vec<_>>()
            });
            let _ = std::fs::write(
                self.root.join("reports/develop-progress.json"),
                serde_json::to_vec_pretty(&diagnostic).unwrap(),
            );
        }
        if timed_out {
            let report = serde_json::json!({
                "passed":false,
                "reason":if self.fatal { "fatal error" } else { "timeout" },
                "status":self.status,
                "stage":self.smoke_stage,
                "elapsed_seconds":self.started.elapsed().as_secs_f64(),
                "timeout_seconds":timeout.as_secs(),
                "native_wb_analysis":self.editing.wb_smoke_result,
                "decode_errors":self.errors,
                "presentation_errors":self.presenter.has_errors(),
            });
            let _ = std::fs::write(
                self.root.join("reports/develop-ui.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let Some(item) = target else {
            return;
        };
        if self.editing.continuity.active() && !self.edit_continuity_tick(&item) {
            return;
        }
        match self.smoke_stage {
            0 if !self.scanning => {
                self.editing.smoke_source_digest = tr_platform::snapshot(&item.path)
                    .ok()
                    .map(|(_, digest)| digest);
                self.set_language(Language::Italian);
                self.show_inspector = true;
                self.state.view = ViewMode::Preview;
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                if args.iter().any(|arg| arg == "--edit-zoom-smoke") {
                    self.state.transform.set_zoom(1.);
                }
                self.smoke_stage = 1;
            }
            1 => {
                self.ensure_edit_loaded(&item);
                if args.iter().any(|a| a == "--native-wb-smoke") {
                    if !self.editing.wb_smoke_started {
                        // Selection starts preview reads that own snapshot slots.
                        // Let them finish before this one-shot WB probe, just as
                        // the export probe does after entering the grid.
                        if !self.pending_images.is_empty()
                            || self.editing.inflight.is_some()
                            || !self.editing.thumbnail_inflight.is_empty()
                        {
                            return;
                        }
                        let Some(saved) = self
                            .editing
                            .entries
                            .get(&item.id)
                            .and_then(|e| e.loaded.clone())
                        else {
                            return;
                        };
                        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                        self.editing.wb_cancel = Some(cancel.clone());
                        let job = crate::photo_export::WbJob {
                            cancel,
                            item: item.clone(),
                            recipe: saved.recipe,
                            analysis: tr_core::raw_wb::Analysis::Auto,
                            generation: self.generation,
                            revision: saved.generation,
                        };
                        self.editing.wb_smoke_started =
                            self.request(Request::RawWhiteBalance(Box::new(job)));
                        self.editing.wb_pending = self.editing.wb_smoke_started;
                        return;
                    }
                    if self.editing.wb_pending {
                        return;
                    }
                    if self.editing.wb_smoke_result.as_deref() != Some("ok") {
                        self.status = self.editing.wb_smoke_result.clone().unwrap_or_default();
                        self.fatal = true;
                        return;
                    }
                }
                if let Some(entry) = self.editing.entries.get_mut(&item.id)
                    && let Some(saved) = &entry.loaded
                    && !entry.pending
                {
                    let mut recipe = saved.recipe.clone();
                    recipe.exposure_ev = 0.75;
                    if args.iter().any(|a| a == "--vibrance-smoke") {
                        recipe.process_version = 2;
                        recipe.vibrance = 65.;
                        recipe.protect_warm = true;
                    }
                    recipe.temperature = 25.;
                    recipe.tint = -8.;
                    if args.iter().any(|a| a == "--advanced-edit-smoke") {
                        recipe = crate::verify_advanced::recipe(recipe.raw_engine);
                    }
                    if args.iter().any(|a| a == "--raw-wb-smoke") {
                        recipe.raw_wb = if recipe.raw_engine == tr_core::decoder::RawEngine::Apple {
                            tr_core::decoder::RawWhiteBalance {
                                apple_temperature: 4500,
                                apple_tint: 12,
                                ..Default::default()
                            }
                        } else {
                            tr_core::decoder::RawWhiteBalance {
                                red: 1500,
                                blue: 750,
                                ..Default::default()
                            }
                        };
                    }

                    if args.iter().any(|a| a == "--auto-smoke") {
                        let source = self
                            .cache
                            .iter()
                            .find(|((id, _), cached)| {
                                id == &item.id && cached.pyramid.base_level() == 0
                            })
                            .map(|(_, cached)| cached.pyramid.clone());
                        let Some(source) = source else {
                            self.request_final_preview(&item.id, false);
                            return;
                        };
                        if let Err(error) = recipe
                            .auto_exposure(source.source())
                            .and_then(|()| recipe.auto_rgb(source.source()))
                        {
                            self.status = error.to_string();
                            self.fatal = true;
                            return;
                        }
                    }
                    if recipe != saved.recipe {
                        entry.draft = Some(recipe);
                        self.commit_edit(&item.id);
                    }
                    if proof_smoke {
                        self.request_final_preview(&item.id, true);
                    }
                    self.smoke_stage = 2;
                }
            }
            2 => {
                let saved = self.editing.entries.get(&item.id).is_some_and(|entry| {
                    entry
                        .loaded
                        .as_ref()
                        .is_some_and(|edit| edit.generation > 0)
                        && !entry.pending
                        && !entry.dirty()
                });
                if !saved {
                    self.commit_edit(&item.id);
                }
                let rendered = self.editing.previews.get(&item.id).is_some_and(|preview| {
                    (!proof_smoke || (preview.proof && preview.image.base_level() == 0))
                        && self
                            .cache
                            .values()
                            .any(|cached| cached.pyramid.id() == preview.source)
                });
                if saved && rendered {
                    self.editing.smoke_ready_at.get_or_insert_with(Instant::now);
                } else {
                    self.editing.smoke_ready_at = None;
                }
                if saved
                    && rendered
                    && self
                        .editing
                        .smoke_ready_at
                        .is_some_and(|start| start.elapsed() > Duration::from_secs(1))
                    && self.presenter.is_idle()
                {
                    if args.iter().any(|a| a == "--edit-continuity-smoke")
                        && !self.editing.continuity.done()
                    {
                        self.edit_continuity_tick(&item);
                        return;
                    }
                    if proof_smoke {
                        let image = &self.editing.previews[&item.id].image;
                        let Some(capture) = self
                            .presenter
                            .captures()
                            .iter()
                            .find(|c| c.source == image.id() && c.clip.contains_rect(c.rect))
                        else {
                            return;
                        };
                        let Ok(reference) = image.render(capture.region) else {
                            return;
                        };
                        let ppp = ctx.pixels_per_point();
                        self.editing.smoke_proof_capture = Some((
                            reference.to_display(),
                            capture.region.size,
                            [
                                capture.rect.min.x * ppp,
                                capture.rect.min.y * ppp,
                                capture.rect.max.x * ppp,
                                capture.rect.max.y * ppp,
                            ],
                            capture.compute,
                        ));
                    }
                    self.smoke_stage = 3;
                    self.capture_screenshot(ctx, "develop-viewer");
                }
            }
            3 if self.screenshots.contains("develop-viewer") => {
                if let Some((expected, size, rect, compute)) =
                    self.editing.smoke_proof_capture.take()
                {
                    let result = (|| -> anyhow::Result<_> {
                        let screen =
                            image::open(self.root.join("reports/develop-viewer.png"))?.to_rgba8();
                        let screen = egui::ColorImage::from_rgba_unmultiplied(
                            [screen.width() as usize, screen.height() as usize],
                            screen.as_raw(),
                        );
                        navigation::compare(&screen, &expected, size, rect)
                    })();
                    self.editing.smoke_proof_result = Some(match result {
                        Ok((maximum, differing)) => {
                            serde_json::json!({"passed":maximum<=1,"maximum_error_u8":maximum,"differing_channels":differing,"size":size,"compute":compute})
                        }
                        Err(error) => serde_json::json!({"passed":false,"error":error.to_string()}),
                    });
                }
                self.state.view = ViewMode::Grid;
                self.editing.smoke_ready_at = Some(Instant::now());
                self.smoke_stage = 4;
            }
            4 => {
                let thumbnail = self
                    .editing
                    .thumbnails
                    .values()
                    .any(|preview| preview.id == item.id);
                if thumbnail
                    && self
                        .editing
                        .smoke_ready_at
                        .is_some_and(|start| start.elapsed() > Duration::from_secs(1))
                    && self.presenter.is_idle()
                {
                    self.smoke_stage = 5;
                    self.capture_screenshot(ctx, "develop-grid");
                }
            }
            5 if self.screenshots.contains("develop-grid") => {
                let export_smoke = proof_smoke && args.iter().any(|a| a == "--proof-export");
                if export_smoke && !self.editing.smoke_export_started {
                    // Entering the grid starts new preview reads. Their snapshot
                    // slots must drain before this one-shot export probe starts.
                    if !self.pending_images.is_empty()
                        || self.editing.inflight.is_some()
                        || !self.editing.thumbnail_inflight.is_empty()
                    {
                        return;
                    }
                    let destination = self.root.join("var/develop-export");
                    if let Err(error) = std::fs::create_dir_all(&destination) {
                        self.editing.smoke_export_result =
                            Some(serde_json::json!({"passed":false,"error":error.to_string()}));
                    } else if let Some(edit) = self
                        .editing
                        .entries
                        .get(&item.id)
                        .and_then(|e| e.loaded.as_ref())
                    {
                        let job = crate::photo_export::Job {
                            item: item.clone(),
                            options: tr_core::export::Options {
                                format: tr_core::export::Format::Png16,
                                ..Default::default()
                            },
                            engine: edit.recipe.raw_engine,
                            source_digest: edit.source_digest.clone(),
                            recipe: Some(edit.recipe.clone()),
                            destination,
                            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        };
                        if !self.request(Request::ExportPhoto(job)) {
                            return;
                        }
                    }
                    self.editing.smoke_export_started = true;
                }
                if export_smoke && self.editing.smoke_export_result.is_none() {
                    return;
                }
                let edit = self
                    .editing
                    .entries
                    .get(&item.id)
                    .and_then(|entry| entry.loaded.as_ref());
                let unchanged = tr_platform::snapshot(&item.path).is_ok_and(|(_, digest)| {
                    self.editing.smoke_source_digest.as_ref() == Some(&digest)
                });
                let wb_observed = edit.is_some_and(|saved| {
                    self.cache.iter().any(|((id, request), cached)| {
                        id == &item.id
                            && request.raw_wb == saved.recipe.raw_wb
                            && cached.info.format == "RAW"
                            && cached.info.input_color.contains("WB")
                    })
                });
                let report = serde_json::json!({
                    "application":"TrueRenderer",
                    "version":env!("CARGO_PKG_VERSION"),
                    "passed":(!args.iter().any(|a|a=="--raw-wb-smoke") || wb_observed) && edit.is_some_and(|saved| saved.generation > 0) && unchanged && !self.presenter.has_errors() && self.errors.is_empty() && !self.fatal && (!proof_smoke || self.editing.smoke_proof_result.as_ref().is_some_and(|r|r["passed"]==true)) && (!export_smoke || self.editing.smoke_export_result.as_ref().is_some_and(|r|r["passed"]==true)),
                    "raw_wb_observed":wb_observed,
                    "native_wb_analysis":self.editing.wb_smoke_result,
                    "timeout_seconds":timeout.as_secs(),
                    "auto_actions":args.iter().any(|a|a=="--auto-smoke"),
                    "output_proof_surface":self.editing.smoke_proof_result,
                    "export":self.editing.smoke_export_result,
                    "fixture":item.name,
                    "source_digest":self.editing.smoke_source_digest,
                    "recipe":edit.map(|saved| &saved.recipe),
                    "saved_generation":edit.map(|saved| saved.generation),
                    "source_unchanged":unchanged,
                    "screenshots":["develop-viewer.png","develop-grid.png"],
                    "platform":std::env::consts::OS,
                    "scope":"Native UI with isolated library and generated or explicit source; saved recipe, edited viewer/grid, optional RAW WB and Auto actions. Surface and export scope are reported separately. No universal RAW colour or physical display qualification."
                });
                let _ = std::fs::write(
                    self.root.join("reports/develop-ui.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
    pub(super) fn record_develop_export(
        &mut self,
        result: &Result<crate::photo_export::Completed, String>,
    ) {
        if self.editing.smoke_export_started {
            self.editing.smoke_export_result = Some(match result {
                Ok(done) => {
                    serde_json::json!({"passed":true,"size":[done.info.width,done.info.height],"file":done.path.file_name().unwrap_or_default().to_string_lossy()})
                }
                Err(error) => serde_json::json!({"passed":false,"error":error}),
            });
        }
    }
    pub(super) fn clear_edit_preview(&mut self) {
        self.cancel_edit_refinement();
        self.editing.raw_wb_anchor.clear();
        self.editing.display_wb.clear();
        self.editing.preview_epoch = self.editing.preview_epoch.wrapping_add(1);
        let sources = self
            .editing
            .previews
            .drain()
            .map(|(_, preview)| preview.image.id())
            .collect();
        self.presenter.invalidate_sources(&sources);
        self.editing.preview_errors.clear();
    }
    fn clear_edit_preview_for(&mut self, id: &str) {
        self.cancel_edit_refinement();
        self.editing.preview_epoch = self.editing.preview_epoch.wrapping_add(1);
        if let Some(preview) = self.editing.previews.remove(id) {
            self.presenter
                .invalidate_sources(&HashSet::from([preview.image.id()]));
        }
        self.editing.preview_errors.remove(id);
    }
    fn supersede_edit_preview_for(&mut self, id: &str) {
        self.cancel_edit_refinement();
        self.editing.preview_epoch = self.editing.preview_epoch.wrapping_add(1);
        if let Some(preview) = self.editing.previews.remove(id) {
            self.presenter
                .supersede_sources(&HashSet::from([preview.image.id()]));
        }
        self.editing.preview_errors.remove(id);
    }
    fn supersede_edit_thumbnails(&mut self, id: &str) {
        // The small histogram/image remains bounded by the existing thumbnail
        // cache. It can describe the last preview while the replacement runs.
        let sources = self
            .editing
            .thumbnails
            .values()
            .filter(|preview| preview.id == id)
            .map(|preview| preview.image.id())
            .collect();
        self.presenter.supersede_sources(&sources);
        self.editing
            .thumbnail_errors
            .retain(|_, (photo, _, _)| photo != id);
    }
    pub(super) fn clear_edit_thumbnails(&mut self, id: &str) {
        self.editing
            .thumbnail_errors
            .retain(|_, (photo, _, _)| photo != id);
        let sources: Vec<_> = self
            .editing
            .thumbnails
            .iter()
            .filter(|(_, preview)| preview.id == id)
            .map(|(source, _)| *source)
            .collect();
        let removed: HashSet<_> = sources
            .into_iter()
            .filter_map(|source| self.editing.thumbnails.remove(&source))
            .map(|preview| preview.image.id())
            .collect();
        self.presenter.invalidate_sources(&removed);
    }
    pub(super) fn prune_edit_previews(&mut self, pressure: bool) {
        let live: HashSet<_> = self.cache.values().map(|c| c.pyramid.id()).collect();
        let stale: Vec<_> = self
            .editing
            .previews
            .iter()
            .filter(|(_, preview)| pressure || !live.contains(&preview.source))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.clear_edit_preview_for(&id);
        }
        self.editing
            .preview_errors
            .retain(|_, error| !pressure && live.contains(&error.source));
        let stale: Vec<_> = self
            .editing
            .thumbnails
            .keys()
            .filter(|source| pressure || !live.contains(source))
            .copied()
            .collect();
        let removed: HashSet<_> = stale
            .into_iter()
            .filter_map(|source| self.editing.thumbnails.remove(&source))
            .map(|preview| preview.image.id())
            .collect();
        self.presenter.invalidate_sources(&removed);
        self.editing
            .thumbnail_errors
            .retain(|source, _| !pressure && live.contains(source));
    }
    pub(super) fn ensure_edit_loaded(&mut self, item: &Item) {
        if !item.approved
            || self
                .editing
                .entries
                .get(&item.id)
                .is_some_and(|e| e.loaded.is_some() || e.loading || e.error.is_some())
        {
            return;
        }
        let id = item.id.clone();
        let engine = self.service.cache.settings().raw_engine;
        if self.request(Request::LoadEdit {
            id: id.clone(),
            engine,
        }) {
            self.editing.entries.entry(id).or_default().loading = true;
        }
    }
    pub(super) fn edit_result(&mut self, id: String, result: Result<LoadedEdit, String>) {
        let previous = self.editing.entries.get(&id).and_then(|e| e.draft.clone());
        if self.sample_item_id.as_deref() == Some(id.as_str()) {
            self.sample = None;
            self.sample_item_id = None;
            self.sample_level = None;
            self.sample_from_current_render = false;
        }
        let entry = self.editing.entries.entry(id.clone()).or_default();
        entry.loading = false;
        entry.pending = false;
        match result {
            Ok(saved) => {
                if !entry.dirty() || entry.draft.is_none() {
                    entry.draft = Some(saved.recipe.clone());
                }
                entry.loaded = Some(saved);
                entry.error = None;
            }
            Err(error) => {
                entry.error = Some(error.clone());
                self.status = format!("Sviluppo non salvato: {error}");
            }
        }
        // A save acknowledgement of the same draft must not invalidate its
        // preview. History/reloads do replace the draft and revoke older work.
        if entry.draft != previous {
            self.editing.display_wb.remove(&id);
            self.editing.raw_wb_anchor.remove(&id);
            self.supersede_edit_preview_for(&id);
            self.supersede_edit_thumbnails(&id);
        }
    }
    pub(super) fn commit_edit(&mut self, id: &str) {
        let Some(entry) = self.editing.entries.get(id) else {
            return;
        };
        if entry.pending || !entry.dirty() {
            return;
        }
        let (Some(saved), Some(recipe)) = (&entry.loaded, &entry.draft) else {
            return;
        };
        let (expected_generation, mut recipe) = (saved.generation, recipe.clone());
        if expected_generation == 0 {
            recipe.raw_engine = self.service.cache.settings().raw_engine;
        }
        if let Err(error) = recipe.validate() {
            self.editing.entries.get_mut(id).unwrap().error = Some(format!("{error:#}"));
            return;
        }
        if self.request(Request::SaveEdit {
            id: id.into(),
            expected_generation,
            recipe: recipe.clone(),
        }) {
            let entry = self.editing.entries.get_mut(id).unwrap();
            entry.draft = Some(recipe);
            entry.pending = true;
            entry.error = None;
        }
    }
    /// Called only from the image keyboard context, after modal/focus guards.
    pub(super) fn editing_keyboard(&mut self, ctx: &egui::Context) -> bool {
        let m = ctx.input(|i| i.modifiers);
        let key = ctx.input(|i| {
            [egui::Key::Z, egui::Key::C, egui::Key::V]
                .into_iter()
                .find(|key| i.key_pressed(*key))
        });
        let history = m.command && m.alt && key == Some(egui::Key::Z);
        // egui-winit turns command+C/V into system clipboard events even with
        // extra modifiers. Keep recipe transfer on a chord delivered as keys.
        let transfer = !m.command
            && !m.ctrl
            && m.shift
            && m.alt
            && matches!(key, Some(egui::Key::C | egui::Key::V));
        if !history && !transfer {
            return false;
        }
        // Consume recognized chords even if unavailable: never fall through to
        // annotation undo or another action while loading/saving a recipe.
        let Some(item) = self.state.current_item().cloned() else {
            return true;
        };
        let ready = !self.editing.wb_pending
            && self.editing.entries.get(&item.id).is_some_and(|e| {
                e.loaded.is_some() && !e.loading && !e.pending && !e.dirty() && e.error.is_none()
            });
        if !ready {
            return true;
        }
        if history {
            self.step_edit(&item.id, !m.shift);
        } else {
            let recipe = self.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .clone();
            if key == Some(egui::Key::C) {
                self.editing.clipboard.copy(&item.name, &recipe);
                self.status = "Regolazioni copiate nella sessione".into();
            } else if let Some(pasted) = self.editing.clipboard.paste(&recipe) {
                self.editing.show_original = false;
                self.apply_edit_draft(&item.id, pasted);
                self.commit_edit(&item.id);
            }
        }
        true
    }

    pub(super) fn apply_edit_draft(&mut self, id: &str, draft: EditRecipe) {
        if let Err(error) = draft.validate() {
            self.status = format!("Regolazione non valida: {error}");
            return;
        }
        if !self.output_proof_for(id) {
            self.editing.verify_final = None;
        }
        self.editing.picker_error = None;
        let compatible = self
            .editing
            .entries
            .get(id)
            .and_then(|e| e.draft.as_ref())
            .is_some_and(|old| old.raw_engine == draft.raw_engine && old.raw_wb == draft.raw_wb);
        self.cancel_edit_refinement();
        if !compatible || draft.is_neutral() {
            self.supersede_edit_preview_for(id);
        }
        if let Some(old) = self.editing.entries.get(id).and_then(|e| e.draft.as_ref())
            && old.raw_engine == draft.raw_engine
            && old.raw_wb != draft.raw_wb
        {
            self.editing
                .raw_wb_anchor
                .entry(id.into())
                .or_insert(old.raw_wb);
            self.editing
                .display_wb
                .entry(id.into())
                .or_insert(old.raw_wb);
        }
        self.editing.revision = self.editing.revision.wrapping_add(1);
        self.editing.changed_at.insert(id.into(), Instant::now());
        self.editing.entries.get_mut(id).unwrap().draft = Some(draft);
        self.sample = None;
        self.sample_item_id = None;
        self.sample_level = None;
        self.sample_from_current_render = false;
        // Keep the complete prior preview and any admitted presentation alive.
        // A new pointer event must not revoke a frame before it can be drawn.
        self.editing.preview_errors.remove(id);
        self.editing
            .thumbnail_errors
            .retain(|_, (photo, _, _)| photo != id);
    }

    fn step_edit(&mut self, id: &str, undo: bool) {
        let Some(entry) = self.editing.entries.get(id) else {
            return;
        };
        if entry.pending || entry.dirty() || self.editing.wb_pending {
            return;
        }
        let Some(saved) = &entry.loaded else {
            return;
        };
        if (undo && !saved.can_undo) || (!undo && !saved.can_redo) {
            return;
        }
        let expected_generation = saved.generation;
        if self.request(Request::StepEdit {
            id: id.into(),
            expected_generation,
            undo,
        }) {
            self.editing.entries.get_mut(id).unwrap().pending = true;
        }
    }
    pub(super) fn edits_have_pending(&self) -> bool {
        self.editing
            .entries
            .values()
            .any(|e| e.pending || e.dirty())
    }
    pub(super) fn commit_all_edits(&mut self) {
        let ids: Vec<_> = self.editing.entries.keys().cloned().collect();
        for id in ids {
            self.commit_edit(&id);
        }
    }
    fn secondary_edit_actions(
        &mut self,
        ui: &mut egui::Ui,
        item: &Item,
        draft: &mut EditRecipe,
        ready: bool,
    ) -> bool {
        let lang = self.cache_settings.language;
        let pasted = self
            .editing
            .clipboard
            .controls(ui, lang, &item.name, draft, ready);
        if ui
            .button(lang.text("Verifica resa finale"))
            .on_hover_text(lang.text("PNG/TIFF16 · sRGB · dimensioni native"))
            .clicked()
        {
            self.request_final_preview(&item.id, true);
        }
        if self.output_proof_for(&item.id)
            && ui
                .button(lang.text("Torna al render esteso fp32"))
                .clicked()
        {
            self.request_final_preview(&item.id, false);
        }
        pasted
    }

    pub(super) fn editing_controls(&mut self, ui: &mut egui::Ui, item: &Item) {
        let lang = self.cache_settings.language;
        self.ensure_edit_loaded(item);
        // A new RAW WB temporarily removes the preceding histogram. Use an
        // explicit scope so its changing widget count cannot cancel a slider
        // gesture or lose the release event that saves the finished draft.
        let controls_id = ui.make_persistent_id(("photographic-edit", &item.id));
        ui.scope_builder(egui::UiBuilder::new().id(controls_id), |ui| {
            let Some(entry) = self.editing.entries.get(&item.id) else {
                ui.label(lang.text("Caricamento ricetta…"));
                return;
            };
            let error = entry.error.clone();
            let saved = entry.loaded.clone();
            let pending = entry.pending || self.editing.wb_pending;
            let existing_draft = entry.draft.clone();
            let dirty = entry.dirty();
            if let Some(error) = &error {
                ui.colored_label(AMBER, error);
                if ui.button(lang.text("Riprova salvataggio")).clicked() {
                    if dirty {
                        self.commit_edit(&item.id);
                    } else {
                        // A failed load/history step has no draft to save.
                        // Reload the confirmed head instead of leaving a dead retry.
                        self.editing.entries.remove(&item.id);
                        self.ensure_edit_loaded(item);
                    }
                }
                if dirty
                    && ui
                        .button(lang.text("Scarta bozza e ricarica ricetta"))
                        .clicked()
                {
                    self.editing.entries.remove(&item.id);
                    self.ensure_edit_loaded(item);
                }
                return;
            }
            let Some(saved) = saved else {
                ui.label(lang.text("Caricamento ricetta…"));
                return;
            };
            let mut draft = existing_draft.unwrap_or_else(|| saved.recipe.clone());
            let can_undo = saved.can_undo;
            let can_redo = saved.can_redo;
            let mut changed = false;
            let mut commit = false;
            let native_size = self
                .cache
                .iter()
                .find(|((id, request), _)| {
                    id == &item.id
                        && request.raw_engine == draft.raw_engine
                        && request.raw_wb == draft.raw_wb
                })
                .map(|(_, cached)| cached.pyramid.source_size());
            let short = ui.ctx().content_rect().height() < 500.;
            let mut pasted = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().button_padding.x = 6.;
                if ui
                    .add_enabled(
                        can_undo && !pending,
                        egui::Button::new(localized_format!(lang, "Annulla", "Undo"))
                            .frame_when_inactive(false),
                    )
                    .on_hover_text("Cmd/Ctrl + Alt + Z")
                    .clicked()
                {
                    self.step_edit(&item.id, true);
                }
                if ui
                    .add_enabled(
                        can_redo && !pending,
                        egui::Button::new(lang.text("Ripeti")).frame_when_inactive(false),
                    )
                    .on_hover_text("Cmd/Ctrl + Alt + Shift + Z")
                    .clicked()
                {
                    self.step_edit(&item.id, false);
                }
                if ui
                    .add(
                        egui::Button::new(lang.text("Prima/Dopo"))
                            .frame_when_inactive(self.editing.show_original)
                            .selected(self.editing.show_original),
                    )
                    .clicked()
                {
                    self.editing.show_original = !self.editing.show_original;
                    self.sample = None;
                    self.sample_item_id = None;
                    self.sample_level = None;
                    self.sample_from_current_render = false;
                }
                if short {
                    let ready = !pending
                        && draft == saved.recipe
                        && !self.editing.entries[&item.id].pending;
                    ui.menu_button(lang.text("Azioni"), |ui| {
                        ui.set_max_width(280.);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        pasted = self.secondary_edit_actions(ui, item, &mut draft, ready);
                    });
                }
            });
            if !short {
                let ready =
                    !pending && draft == saved.recipe && !self.editing.entries[&item.id].pending;
                pasted = self.secondary_edit_actions(ui, item, &mut draft, ready);
            }
            if pasted {
                changed = true;
                commit = true;
                self.editing.show_original = false;
            }
            if self.editing.wb_pending {
                ui.label(lang.text("Analisi WB RAW…"));
                if let Some(cancel) = &self.editing.wb_cancel {
                    if cancel.load(Ordering::Acquire) {
                        ui.label(lang.text("Annullamento WB RAW…"));
                    } else if ui.button(lang.text("Annulla analisi WB RAW")).clicked() {
                        cancel.store(true, Ordering::Release);
                    }
                }
            }
            ui.add_enabled_ui(!pending, |ui| {
                let engine = self.preview_request(item, 0).raw_engine;
                let is_raw = self.cache.iter().any(|((id, request), cached)| {
                    id == &item.id && request.raw_engine == engine && cached.info.format == "RAW"
                });
                if is_raw {
                    if let Some((_, error)) = self
                        .editing
                        .wb_error
                        .as_ref()
                        .filter(|(id, _)| id == &item.id)
                    {
                        ui.colored_label(AMBER, lang.text(error));
                    }
                    ui.small(lang.text(
                        "WB nativo: analisi senza regolazioni creative; Auto assume grigio medio",
                    ));
                    let mut analysis = None;
                    if ui.button(lang.text("Auto WB RAW")).clicked() {
                        analysis = Some(tr_core::raw_wb::Analysis::Auto);
                    }
                    let point = self
                        .sample
                        .as_ref()
                        .filter(|_| {
                            self.sample_item_id.as_deref() == Some(item.id.as_str())
                                && self.sample_level == Some(0)
                                && self.sample_from_current_render
                                && !self.output_proof_for(&item.id)
                        })
                        .and_then(|point| {
                            let native = native_size?;
                            if let Some(a) = &draft.advanced {
                                let output = a.geometry.output_size(native);
                                let p = a.geometry.source_point(
                                    [
                                        (point.x as f64 + 0.5) / output[0] as f64,
                                        (point.y as f64 + 0.5) / output[1] as f64,
                                    ],
                                    native,
                                )?;
                                if p.iter().any(|v| !(0. ..1.).contains(v)) {
                                    return None;
                                }
                                Some((
                                    (p[0] * native[0] as f64).floor() as u32,
                                    (p[1] * native[1] as f64).floor() as u32,
                                ))
                            } else {
                                Some((point.x, point.y))
                            }
                        });
                    if ui
                        .add_enabled(
                            point.is_some(),
                            egui::Button::new(lang.text("WB RAW da area 5×5")),
                        )
                        .clicked()
                    {
                        let point = point.unwrap();
                        analysis = Some(tr_core::raw_wb::Analysis::Patch {
                            x: point.0,
                            y: point.1,
                            side: 5,
                        });
                    }
                    if let Some(analysis) = analysis {
                        self.editing.wb_error = None;
                        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                        self.editing.wb_cancel = Some(cancel.clone());
                        self.editing.wb_pending = self.request(Request::RawWhiteBalance(Box::new(
                            crate::photo_export::WbJob {
                                cancel,
                                item: item.clone(),
                                recipe: draft.clone(),
                                analysis,
                                generation: self.generation,
                                revision: saved.generation,
                            },
                        )));
                    }
                    ui.label(RichText::new(lang.text("Bilanciamento del bianco RAW")).strong());
                    if engine == tr_core::decoder::RawEngine::Apple {
                        let (wb_changed, wb_commit) =
                            apple_wb_controls(ui, lang, &mut draft.raw_wb);
                        changed |= wb_changed;
                        commit |= wb_commit;
                    } else {
                        ui.label(lang.text(
                            "Guadagni sensore relativi a come scattato · prima del demosaic",
                        ));
                        for (label, value) in [
                            ("Rosso RAW", &mut draft.raw_wb.red),
                            ("Blu RAW", &mut draft.raw_wb.blue),
                        ] {
                            let mut gain = f32::from(*value) / 1000.;
                            let response = adjustment(
                                ui,
                                lang.text(label),
                                &mut gain,
                                0.25..=4.,
                                " ×",
                                Some(0.001),
                            );
                            if response.changed() {
                                *value = (gain * 1000.).round() as u16;
                                changed = true;
                            }
                            commit |= response.drag_stopped()
                                || (response.changed() && !response.dragged());
                        }
                        if ui.button(lang.text("WB RAW come scattato")).clicked() {
                            draft.raw_wb = Default::default();
                            changed = true;
                            commit = true;
                        }
                    }
                }
                if adjustment_heading(
                    ui,
                    lang,
                    "Luce",
                    "Azzera luce",
                    ResetGroup::Light,
                    &mut draft,
                ) {
                    changed = true;
                    commit = true;
                }

                for (label, value, min, max, suffix) in [
                    ("Esposizione", &mut draft.exposure_ev, -10., 10., " EV"),
                    ("Luminosità", &mut draft.brightness, -100., 100., ""),
                    ("Contrasto", &mut draft.contrast, -100., 100., ""),
                    ("Alte luci", &mut draft.highlights, -100., 100., ""),
                    ("Ombre", &mut draft.shadows, -100., 100., ""),
                    ("Bianchi", &mut draft.whites, -100., 100., ""),
                    ("Neri", &mut draft.blacks, -100., 100., ""),
                ] {
                    let response = adjustment(ui, lang.text(label), value, min..=max, suffix, None);
                    changed |= response.changed();
                    commit |=
                        response.drag_stopped() || (response.changed() && !response.dragged());
                }
                let request = self.preview_request(item, 0);
                let auto_source = (!changed && !self.editing.show_original)
                    .then(|| {
                        self.cache
                            .iter()
                            .find(|((id, r), cached)| {
                                id == &item.id
                                    && r.raw_engine == request.raw_engine
                                    && r.raw_wb == draft.raw_wb
                                    && cached.pyramid.base_level() == 0
                            })
                            .map(|(_, c)| c.pyramid.clone())
                    })
                    .flatten();
                ui.horizontal_wrapped(|ui| {
                    if auto_source.is_none()
                        && ui.button(lang.text("Carica nativo per Auto")).clicked()
                    {
                        self.request_final_preview(&item.id, false);
                    }
                    for (label, rgb) in [
                        ("Auto esposizione", false),
                        ("Auto RGB · grigio medio", true),
                    ] {
                        if ui
                            .add_enabled(auto_source.is_some(), egui::Button::new(lang.text(label)))
                            .clicked()
                        {
                            let source = auto_source.as_ref().unwrap().source();
                            let result = if rgb {
                                draft.auto_rgb(source)
                            } else {
                                draft.auto_exposure(source)
                            };
                            match result {
                                Ok(()) => {
                                    changed = true;
                                    commit = true;
                                    self.editing.picker_error = None;
                                }
                                Err(error) => {
                                    self.editing.picker_error =
                                        Some((item.id.clone(), error.to_string()));
                                }
                            }
                        }
                    }
                });
                ui.label(
                    RichText::new(lang.text(
                        "Auto RGB assume una scena mediamente neutra; non cambia il WB RAW.",
                    ))
                    .small()
                    .color(MUTED),
                );
                if adjustment_heading(
                    ui,
                    lang,
                    "Curva tonale",
                    "Azzera curva",
                    ResetGroup::Curve,
                    &mut draft,
                ) {
                    changed = true;
                    commit = true;
                }

                let mut mid = draft
                    .curve
                    .windows(2)
                    .find(|p| p[0].x <= 0.5 && p[1].x >= 0.5)
                    .map_or(0.5, |p| {
                        p[0].y + (p[1].y - p[0].y) * (0.5 - p[0].x) / (p[1].x - p[0].x)
                    });
                let response = adjustment(
                    ui,
                    lang.text("Mezzitoni curva"),
                    &mut mid,
                    0. ..=1.,
                    "",
                    None,
                );
                if response.changed() {
                    draft.curve = if (mid - 0.5).abs() < 1e-6 {
                        vec![]
                    } else {
                        vec![
                            CurvePoint { x: 0., y: 0. },
                            CurvePoint { x: 0.5, y: mid },
                            CurvePoint { x: 1., y: 1. },
                        ]
                    };
                    changed = true;
                }
                commit |= response.drag_stopped() || (response.changed() && !response.dragged());
                let (curve_changed, curve_commit) = curve::controls(ui, lang, &mut draft.curve);
                changed |= curve_changed;
                commit |= curve_commit;
                if adjustment_heading(
                    ui,
                    lang,
                    "Colore RGB",
                    "Azzera colore",
                    ResetGroup::Color,
                    &mut draft,
                ) {
                    changed = true;
                    commit = true;
                }
                for (label, value) in [
                    ("Temperatura RGB", &mut draft.temperature),
                    ("Tinta RGB", &mut draft.tint),
                    ("Saturazione", &mut draft.saturation),
                ] {
                    let response = adjustment(ui, lang.text(label), value, -100. ..=100., "", None);
                    changed |= response.changed();
                    commit |=
                        response.drag_stopped() || (response.changed() && !response.dragged());
                }
                let response = adjustment(
                    ui,
                    lang.text("Vividezza"),
                    &mut draft.vibrance,
                    -100. ..=100.,
                    "",
                    None,
                );
                let protection =
                    ui.checkbox(&mut draft.protect_warm, lang.text("Proteggi toni caldi"));
                if response.changed() || protection.changed() {
                    draft.process_version = draft.process_version.max(2);
                    changed = true;
                }
                commit |= response.drag_stopped()
                    || (response.changed() && !response.dragged())
                    || protection.changed();
                ui.small(
                    lang.text("Vividezza: processo 2 · protezione indicativa, non rileva la pelle"),
                );
                ui.label(
                    RichText::new(lang.text("La correzione RGB non cambia il WB RAW del decoder."))
                        .small()
                        .color(MUTED),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(lang.text("Area contagocce"));
                    for side in [1, 5, 11] {
                        if ui
                            .selectable_value(
                                &mut self.editing.picker_side,
                                side,
                                format!("{side}×{side}"),
                            )
                            .changed()
                        {
                            self.editing.picker_error = None;
                        }
                    }
                });
                let sample = if !changed
                    && self.sample_item_id.as_deref() == Some(item.id.as_str())
                    && self.sample_level == Some(0)
                    && self.sample_from_current_render
                    && !self.output_proof_for(&item.id)
                {
                    self.sample
                        .as_ref()
                        .map(|sample| (sample.working, sample.x, sample.y))
                } else {
                    None
                };
                let area = match self.editing.picker_side {
                    5 => self.editing.picker_areas[0].as_ref(),
                    11 => self.editing.picker_areas[1].as_ref(),
                    _ => None,
                };
                let picker_pixel = sample.and_then(|(pixel, _, _)| {
                    if self.editing.picker_side == 1 {
                        Some(pixel)
                    } else {
                        area.and_then(|result| result.as_ref().ok())
                            .map(|s| s.working)
                    }
                });
                if sample.is_some()
                    && let Some(area) = area
                {
                    match area {
                        Ok(area) => {
                            ui.label(localized_format!(
                                lang,
                                "Pixel validi: {}/{} · dispersione cromatica: {:.3}",
                                "Valid pixels: {}/{} · chromatic spread: {:.3}",
                                area.valid,
                                area.total,
                                area.chroma_spread
                            ));
                        }
                        Err(error) => {
                            ui.colored_label(AMBER, lang.text(error));
                        }
                    }
                }
                if let Some((_, x, y)) = sample {
                    ui.label(localized_format!(
                        lang,
                        "Ultimo campione RGB: {}, {}",
                        "Last RGB sample: {}, {}",
                        x,
                        y
                    ));
                }
                if self.output_proof_for(&item.id) {
                    ui.label(lang.text("Per il contagocce torna al render esteso fp32."));
                } else if self.sample_item_id.as_deref() == Some(item.id.as_str())
                    && (self.sample_level != Some(0) || !self.sample_from_current_render)
                {
                    ui.label(lang.text("Verifica la resa finale e campiona la vista modificata."));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button(lang.text("Azzera correzione RGB")).clicked() {
                        draft.temperature = 0.;
                        draft.tint = 0.;
                        changed = true;
                        commit = true;
                    }
                    if ui
                        .add_enabled(
                            picker_pixel.is_some(),
                            egui::Button::new(lang.text("Neutralizza campione RGB")),
                        )
                        .clicked()
                        && let Some(pixel) = picker_pixel
                    {
                        match draft.neutralize_render_sample(pixel) {
                            Ok(()) => {
                                changed = true;
                                commit = true;
                                self.editing.picker_error = None;
                            }
                            Err(error) => {
                                self.editing.picker_error =
                                    Some((item.id.clone(), format!("{error:#}")));
                            }
                        }
                    }
                });
                if let Some((_, error)) = self
                    .editing
                    .picker_error
                    .as_ref()
                    .filter(|(id, _)| id == &item.id)
                {
                    ui.colored_label(AMBER, lang.text(error));
                }
                let (advanced_changed, advanced_commit) =
                    self.editing
                        .advanced
                        .show(ui, lang, &mut draft, native_size);
                changed |= advanced_changed;
                commit |= advanced_commit;
                if ui.button(lang.text("Sviluppo originale")).clicked() {
                    draft = EditRecipe::neutral(saved.recipe.raw_engine);
                    changed = true;
                    commit = true;
                }
            });
            if changed {
                self.apply_edit_draft(&item.id, draft);
            }
            if commit {
                self.commit_edit(&item.id);
            }
            let entry = self.editing.entries.get(&item.id).unwrap();
            ui.label(lang.text(if entry.pending {
                "Salvataggio…"
            } else if entry.dirty() {
                "Modifiche in corso"
            } else {
                "Ricetta salvata nella libreria"
            }));
            ui.label(
                RichText::new(lang.text(if self.output_proof_for(&item.id) {
                    "Anteprima export sRGB16 · PNG/TIFF · dimensioni native"
                } else if self.editing.verify_final.as_deref() == Some(&item.id) {
                    "Resa finale alla risoluzione nativa"
                } else {
                    "Vista modificata provvisoria; export alla risoluzione nativa."
                }))
                .small()
                .color(MUTED),
            );
        });
    }
    pub(super) fn poll_edit_preview(&mut self) {
        while let Ok((generation, id, source, base, proof, recipe, result)) =
            self.editing.rx.try_recv()
        {
            let active = self
                .editing
                .inflight
                .as_ref()
                .is_some_and(|(photo, input, requested)| {
                    photo == &id && *input == source && *requested == recipe
                });
            let revoked = active && self.editing.inflight_epoch != self.editing.preview_epoch;
            let cancelled = active
                && self
                    .editing
                    .refinement_cancel
                    .as_ref()
                    .is_some_and(|cancel| cancel.load(Ordering::Acquire));
            let intermediate = active
                && self.editing.inflight_epoch == self.editing.preview_epoch
                && self
                    .editing
                    .entries
                    .get(&id)
                    .and_then(|e| e.draft.as_ref())
                    .is_some_and(|latest| {
                        latest.raw_engine == recipe.raw_engine
                            && latest.raw_wb == recipe.raw_wb
                            && !latest.is_neutral()
                    });
            if active {
                self.editing.inflight = None;
                self.editing.refinement_cancel = None;
            }
            if cancelled
                || revoked
                || (!active && result.is_ok())
                || generation != self.generation
                || (!intermediate
                    && self.editing.entries.get(&id).and_then(|e| e.draft.as_ref())
                        != Some(&recipe))
                || self.output_proof_for(&id) != proof
            {
                continue;
            }
            match result {
                Ok(image) => {
                    self.editing.preview_errors.remove(&id);
                    if self.editing.previews.len() >= 2
                        && !self.editing.previews.contains_key(&id)
                        && let Some(oldest) = self
                            .editing
                            .previews
                            .iter()
                            .min_by_key(|(_, p)| p.touched)
                            .map(|(id, _)| id.clone())
                    {
                        self.clear_edit_preview_for(&oldest);
                    }
                    self.editing.previews.insert(
                        id,
                        EditPreview {
                            source,
                            proof,
                            recipe,
                            image,
                            touched: self.frame_number,
                        },
                    );
                }
                Err(error) => {
                    if self.editing.entries.get(&id).and_then(|e| e.draft.as_ref()) != Some(&recipe)
                    {
                        continue;
                    }
                    self.editing.preview_errors.insert(
                        id,
                        PreviewFailure {
                            source,
                            base,
                            proof,
                            recipe,
                            message: error,
                        },
                    );
                }
            }
        }
        while let Ok((generation, id, digest, source, recipe, result)) =
            self.editing.thumbnail_rx.try_recv()
        {
            self.editing.thumbnail_inflight.remove(&source);
            let current = generation == self.generation
                && self.editing.entries.get(&id).is_some_and(|entry| {
                    entry.loaded.as_ref().is_some_and(|saved| {
                        self.edit_source_matches(&id, &saved.source_digest, &digest, source)
                    }) && entry.draft.as_ref() == Some(&recipe)
                });
            if !current {
                continue;
            }
            match result {
                Ok((image, histogram)) => {
                    self.editing.thumbnail_errors.remove(&source);
                    if !self.editing.thumbnails.contains_key(&source)
                        && self.editing.thumbnails.len() >= 64
                        && let Some(oldest) = self
                            .editing
                            .thumbnails
                            .iter()
                            .min_by_key(|(_, preview)| preview.touched)
                            .map(|(source, _)| *source)
                        && let Some(old) = self.editing.thumbnails.remove(&oldest)
                    {
                        self.presenter
                            .invalidate_sources(&HashSet::from([old.image.id()]));
                    }
                    if let Some(old) = self.editing.thumbnails.get(&source) {
                        self.presenter
                            .supersede_sources(&HashSet::from([old.image.id()]));
                    }
                    self.editing.thumbnails.insert(
                        source,
                        ThumbnailPreview {
                            id,
                            recipe,
                            image,
                            histogram,
                            touched: self.frame_number,
                        },
                    );
                }
                Err(error) => {
                    self.editing
                        .thumbnail_errors
                        .insert(source, (id, recipe, error));
                }
            }
        }
    }
    pub(super) fn edited_thumbnail_error(&self, source: u64) -> Option<&str> {
        self.editing
            .thumbnail_errors
            .get(&source)
            .map(|(_, _, error)| error.as_str())
    }
    pub(super) fn edited_thumbnail_histogram(&self, source: u64) -> Option<[[u32; 256]; 3]> {
        self.editing.thumbnails.get(&source).map(|p| p.histogram)
    }
    pub(super) fn edited_thumbnail(
        &mut self,
        id: &str,
        digest: &str,
        neutral: Arc<ImageLevels>,
    ) -> Option<Arc<ImageLevels>> {
        let entry = self.editing.entries.get(id)?;
        let saved = entry.loaded.as_ref()?;
        if !self.edit_source_matches(id, &saved.source_digest, digest, neutral.id()) {
            return None;
        }
        let recipe = entry.draft.as_ref().unwrap_or(&saved.recipe).clone();
        if recipe.is_neutral() || self.editing.show_original {
            return Some(neutral);
        }
        let source = neutral.id();
        if let Some(ready) = self.editing.thumbnails.get_mut(&source)
            && ready.id == id
            && ready.recipe == recipe
        {
            ready.touched = self.frame_number;
            return Some(ready.image.clone());
        }
        if self
            .editing
            .thumbnail_errors
            .get(&source)
            .is_some_and(|(photo, failed, _)| photo == id && *failed == recipe)
        {
            return None;
        }
        self.editing.thumbnail_errors.remove(&source);
        if self.edit_interactive(id) && self.editing.raw_wb_anchor.contains_key(id) {
            return None;
        }
        if self.edit_interactive(id)
            && self
                .editing
                .thumbnail_started
                .get(id)
                .is_some_and(|started| started.elapsed() < Duration::from_millis(120))
        {
            return None;
        }
        if self.editing.thumbnail_inflight.len() < 2
            && !self.editing.thumbnail_inflight.contains(&source)
        {
            let index = neutral
                .levels()
                .iter()
                .position(|level| level.width.max(level.height) <= 512)
                .unwrap_or(neutral.levels().len() - 1);
            let level = &neutral.levels()[index];
            let bytes = preview_working_bytes(level.width, level.height)
                + recipe.scratch_bytes(level.width, level.height);
            if let Some(mut lease) = self.service.cache.memory.try_reserve(bytes) {
                self.editing
                    .thumbnail_started
                    .insert(id.into(), Instant::now());
                let tx = self.editing.thumbnail_tx.clone();
                let wake = self.service.wake.clone();
                let id = id.to_owned();
                let digest = digest.to_owned();
                let base = neutral.base_level() + index as u32;
                let size = neutral.source_size();
                self.editing.thumbnail_inflight.insert(source);
                let generation = self.generation;
                std::thread::spawn(move || {
                    let result = (|| -> anyhow::Result<_> {
                        let mut raster = neutral.levels()[index].clone();
                        let size = recipe.apply_preview(&mut raster, size, base)?;
                        let opaque = raster.pixels.iter().all(|p| p[3] == 1.);
                        let base = base.min(
                            31 - size[0].max(size[1]).leading_zeros()
                                + u32::from(!size[0].max(size[1]).is_power_of_two()),
                        );
                        let mut image =
                            ImageLevels::from_reference_mip(raster, size, base, opaque)?;
                        lease.shrink(image.byte_len() as u64);
                        image.attach_lease(lease);
                        let histogram = image.source().histogram();
                        Ok((Arc::new(image), histogram))
                    })()
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send((generation, id, digest, source, recipe, result));
                    wake.request_repaint();
                });
            }
        }
        None
    }
    pub(super) fn edited_source_size(&self, id: &str, native: [u32; 2]) -> [u32; 2] {
        if self.editing.show_original {
            return native;
        }
        self.editing
            .entries
            .get(id)
            .and_then(|entry| {
                entry
                    .draft
                    .as_ref()
                    .or_else(|| entry.loaded.as_ref().map(|saved| &saved.recipe))
            })
            .and_then(|recipe| recipe.advanced.as_ref())
            .map_or(native, |advanced| advanced.geometry.output_size(native))
    }

    pub(super) fn edited_preview(
        &mut self,
        id: &str,
        digest: &str,
        neutral: Arc<ImageLevels>,
    ) -> Option<Arc<ImageLevels>> {
        let entry = self.editing.entries.get(id)?;
        let saved = entry.loaded.as_ref()?;
        if !self.edit_source_matches(id, &saved.source_digest, digest, neutral.id()) {
            return None;
        }
        let recipe = entry.draft.as_ref().unwrap_or(&saved.recipe).clone();
        let proof = self.output_proof_for(id);
        if (recipe.is_neutral() && !proof) || self.editing.show_original {
            return Some(neutral);
        }
        if proof && neutral.base_level() != 0 {
            return None; // A reduced source cannot simulate output before filtering.
        }
        let source = neutral.id();
        let interactive =
            self.edit_interactive(id) && !proof && self.editing.verify_final.as_deref() != Some(id);
        let edge = if self.editing.interactive_millis.load(Ordering::Relaxed) > 24 {
            512
        } else {
            1024
        };
        let index = if !interactive {
            0
        } else {
            neutral
                .levels()
                .iter()
                .position(|l| l.width.max(l.height) <= edge)
                .unwrap_or(neutral.levels().len() - 1)
        };
        let base = neutral.base_level() + index as u32;
        if let Some(ready) = self.editing.previews.get_mut(id)
            && ready.source == source
            && ready.proof == proof
            && ready.recipe == recipe
            && ready.image.base_level()
                == recipe.advanced.as_ref().map_or(base, |a| {
                    let edge = a
                        .geometry
                        .output_size(neutral.source_size())
                        .into_iter()
                        .max()
                        .unwrap();
                    base.min(31 - edge.leading_zeros() + u32::from(!edge.is_power_of_two()))
                })
        {
            ready.touched = self.frame_number;
            return Some(ready.image.clone());
        }
        if self.editing.preview_errors.get(id).is_some_and(|failure| {
            failure.source == source
                && failure.base == base
                && failure.proof == proof
                && failure.recipe == recipe
        }) {
            return None;
        }
        if self.editing.inflight.is_none() {
            let level = &neutral.levels()[index];
            let bytes = preview_working_bytes(level.width, level.height)
                + recipe.scratch_bytes(level.width, level.height);
            if let Some(mut lease) = self.service.cache.memory.try_reserve(bytes) {
                let tx = self.editing.tx.clone();
                let wake = self.service.wake.clone();
                let id = id.to_owned();
                let size = neutral.source_size();
                self.editing.inflight = Some((id.clone(), source, recipe.clone()));
                self.editing.inflight_epoch = self.editing.preview_epoch;
                let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                if !interactive && !proof {
                    self.editing.refinement_cancel = Some(cancel.clone());
                }
                let timing = self.editing.interactive_millis.clone();
                let generation = self.generation;
                std::thread::spawn(move || {
                    let started = Instant::now();
                    let cancelled = || cancel.load(Ordering::Acquire);
                    let result = (|| -> anyhow::Result<Arc<ImageLevels>> {
                        anyhow::ensure!(!cancelled(), "Editing annullato");
                        let mut raster = neutral.levels()[index].clone();
                        let size = recipe.apply_preview_cancellable(
                            &mut raster,
                            size,
                            base,
                            &cancelled,
                        )?;
                        if proof {
                            tr_core::export::proof_srgb16(&mut raster);
                        }
                        let opaque = raster.pixels.iter().all(|p| p[3] == 1.);
                        let base = base.min(
                            31 - size[0].max(size[1]).leading_zeros()
                                + u32::from(!size[0].max(size[1]).is_power_of_two()),
                        );
                        let mut image = ImageLevels::from_reference_mip_cancellable(
                            raster, size, base, opaque, &cancelled,
                        )?;
                        // Filtering scratch is gone; only resident pixels remain.
                        lease.shrink(image.byte_len() as u64);
                        image.attach_lease(lease);
                        Ok(Arc::new(image))
                    })()
                    .map_err(|e| format!("{e:#}"));
                    if interactive && result.is_ok() {
                        timing.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                    }
                    let _ = tx.send((generation, id, source, base, proof, recipe, result));
                    wake.request_repaint();
                });
            } else {
                self.editing.preview_errors.insert(
                    id.into(),
                    PreviewFailure {
                        source: 0,
                        base,
                        proof,
                        recipe,
                        message: "Memoria insufficiente per l'anteprima modificata".into(),
                    },
                );
            }
        }
        None
    }
    fn cancel_edit_refinement(&mut self) {
        if let Some(cancel) = &self.editing.refinement_cancel {
            cancel.store(true, Ordering::Release);
        }
    }
    pub(super) fn edit_interactive(&self, id: &str) -> bool {
        let active = self.editing.changed_at.get(id).is_some_and(|changed| {
            changed.elapsed() < Duration::from_millis(120)
                || self.context.input(|i| i.pointer.any_down())
        });
        if active {
            self.context
                .request_repaint_after(Duration::from_millis(125));
        }
        active
    }
    pub(super) fn live_edit_for(
        &self,
        id: &str,
        digest: &str,
        source: &ImageLevels,
        input_wb: tr_core::decoder::RawWhiteBalance,
    ) -> Option<tr_render::live_edit::LiveEdit> {
        let entry = self.editing.entries.get(id)?;
        let saved = entry.loaded.as_ref()?;
        let recipe = entry.draft.as_ref()?;
        if self.editing.show_original
            || self.output_proof_for(id)
            || source.scientific()
            || !self.edit_source_matches(id, &saved.source_digest, digest, source.id())
            || (!self.edit_interactive(id) && input_wb == recipe.raw_wb)
            || (recipe.is_neutral() && input_wb == recipe.raw_wb)
        {
            return None;
        }
        let input_gains = wb_draft_gains(recipe.raw_engine, input_wb, recipe.raw_wb);
        let mut edit = tr_render::live_edit::LiveEdit {
            recipe: recipe.clone(),
            input_gains,
            max_edge: 1024,
            revision: self.editing.revision,
        };
        edit.max_edge = self
            .presenter
            .interactive_edit_edge(&edit)
            .min(self.viewer_prefetch_edge.clamp(512, 2048));
        Some(edit)
    }
    // Only the display may consume an intermediate complete draft. Samplers,
    // export and exact verification continue to use edited_preview's strict key.
    pub(super) fn progressive_edit_preview(
        &self,
        id: &str,
        source: u64,
    ) -> Option<Arc<ImageLevels>> {
        let current = self.editing.entries.get(id)?.draft.as_ref()?;
        let preview = self.editing.previews.get(id)?;
        (preview.source == source
            && preview.proof == self.output_proof_for(id)
            && preview.recipe.raw_engine == current.raw_engine
            && preview.recipe.raw_wb == current.raw_wb
            && !self.editing.show_original)
            .then(|| preview.image.clone())
    }
    pub(super) fn edited_preview_error(&self, id: &str) -> Option<&str> {
        self.editing
            .preview_errors
            .get(id)
            .map(|failure| failure.message.as_str())
    }
}

// Display-only approximation between two native WB requests. Sensor-space
// gains and Apple's illuminant model are not equivalent to working-space RGB;
// these pixels are explicitly provisional and never sampled or exported.
fn wb_draft_gains(
    engine: tr_core::decoder::RawEngine,
    from: tr_core::decoder::RawWhiteBalance,
    to: tr_core::decoder::RawWhiteBalance,
) -> [f32; 3] {
    if from == to {
        return [1.; 3];
    }
    if engine == tr_core::decoder::RawEngine::Apple {
        let temperature = |wb: tr_core::decoder::RawWhiteBalance| {
            if wb.is_as_shot() {
                6500.
            } else {
                wb.apple_temperature as f32
            }
        };
        let warm = (temperature(to) / temperature(from)).powf(0.8);
        let tint = ((to.apple_tint as f32 - from.apple_tint as f32) / 300.).exp();
        [warm * tint, 1. / tint, tint / warm].map(|v| v.clamp(0.05, 20.))
    } else {
        [
            to.red as f32 / from.red as f32,
            1.,
            to.blue as f32 / from.blue as f32,
        ]
    }
}

fn develop_smoke_timeout(native_wb: bool) -> Duration {
    Duration::from_secs(90)
        + if native_wb {
            tr_platform::RAW_WB_TIMEOUT
        } else {
            Duration::ZERO
        }
}

#[cfg(test)]
mod smoke_deadline_tests {
    use super::*;
    #[test]
    fn native_analysis_keeps_the_full_budget_for_the_rest_of_the_smoke() {
        let ordinary = develop_smoke_timeout(false);
        let native = develop_smoke_timeout(true);
        assert_eq!(ordinary, Duration::from_secs(90));
        let elapsed = Duration::from_secs(30) + tr_platform::RAW_WB_TIMEOUT;
        assert!(elapsed > ordinary);
        assert!(elapsed < native);
        assert_eq!(native - ordinary, tr_platform::RAW_WB_TIMEOUT);
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod progressive_tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};
    #[test]
    fn raw_wb_gesture_coalesces_native_requests_and_keeps_saved_recipe_exact() {
        for engine in tr_core::decoder::RawEngine::choices() {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            let item = app.state.items[0].clone();
            let neutral = EditRecipe::neutral(engine);
            app.editing.entries.insert(
                item.id.clone(),
                EditEntry {
                    loaded: Some(LoadedEdit {
                        asset_id: item.id.clone(),
                        source_digest: item.digest.clone(),
                        generation: 1,
                        revision: 1,
                        recipe: neutral.clone(),
                        can_undo: false,
                        can_redo: false,
                    }),
                    draft: Some(neutral.clone()),
                    ..Default::default()
                },
            );
            let source = ImageLevels::from_source(
                tr_core::color::LinearImage::new(64, 32, vec![[0.2, 0.3, 0.4, 1.]; 2048]).unwrap(),
                PreviewRequest::full(),
            )
            .unwrap();
            let mut draft = neutral.clone();
            for step in 1..=12 {
                if engine == tr_core::decoder::RawEngine::Apple {
                    draft.raw_wb.apple_temperature = 4000 + step * 100;
                } else {
                    draft.raw_wb.red = 1000 + step * 40;
                }
                app.apply_edit_draft(&item.id, draft.clone());
                assert_eq!(app.preview_request(&item, 0).raw_wb, neutral.raw_wb);
                let live = app
                    .live_edit_for(&item.id, &item.digest, &source, neutral.raw_wb)
                    .unwrap();
                assert_ne!(live.input_gains, [1.; 3]);
                assert_eq!(live.recipe, draft);
                assert_eq!(
                    app.editing.entries[&item.id]
                        .loaded
                        .as_ref()
                        .unwrap()
                        .recipe,
                    neutral
                );
            }
            app.editing
                .changed_at
                .insert(item.id.clone(), Instant::now() - Duration::from_secs(1));
            assert_eq!(app.preview_request(&item, 0).raw_wb, draft.raw_wb);
            assert!(
                app.live_edit_for(&item.id, &item.digest, &source, neutral.raw_wb)
                    .is_some()
            );
            assert!(
                app.live_edit_for(&item.id, &item.digest, &source, draft.raw_wb)
                    .is_none()
            );
            app.editing.show_original = true;
            assert!(
                app.live_edit_for(&item.id, &item.digest, &source, neutral.raw_wb)
                    .is_none()
            );
            assert_eq!(app.preview_request(&item, 0).raw_wb, neutral.raw_wb);
            app.editing.show_original = false;
            app.request_final_preview(&item.id, true);
            assert_eq!(app.preview_request(&item, 0).raw_wb, draft.raw_wb);
            assert!(
                app.live_edit_for(&item.id, &item.digest, &source, neutral.raw_wb)
                    .is_none()
            );
            if engine == tr_core::decoder::RawEngine::Apple {
                draft.raw_wb.apple_temperature += 100;
            } else {
                draft.raw_wb.red += 100;
            }
            app.apply_edit_draft(&item.id, draft.clone());
            assert!(app.edit_interactive(&item.id));
            assert_eq!(
                app.preview_request(&item, 0).raw_wb,
                draft.raw_wb,
                "Output proof must never borrow a different native WB input"
            );
        }
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    use std::path::Path;

    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn cache_clear_rejects_late_edited_results_and_preserves_the_recipe() {
        let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
        crate::ui::settings_regressions::settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.exposure_ev = 0.75;
        app.editing.entries.insert(
            item.id.clone(),
            EditEntry {
                loaded: Some(tr_store::LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: item.digest.clone(),
                    generation: 3,
                    revision: 2,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe.clone()),
                ..Default::default()
            },
        );
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(8, 8, vec![[0.2, 0.3, 0.4, 1.]; 64]).unwrap(),
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        let old = app.generation;
        for (generation, accepted) in [(old, true), (old, false), (old + 1, true)] {
            if !accepted {
                app.start_cache_action(false);
                assert!(app.editing.previews.is_empty() && app.editing.thumbnails.is_empty());
            }
            if accepted {
                app.editing.inflight = Some((item.id.clone(), image.id(), recipe.clone()));
                app.editing.inflight_epoch = app.editing.preview_epoch;
            }
            app.editing
                .tx
                .send((
                    generation,
                    item.id.clone(),
                    image.id(),
                    0,
                    false,
                    recipe.clone(),
                    Ok(image.clone()),
                ))
                .unwrap();
            app.editing
                .thumbnail_tx
                .send((
                    generation,
                    item.id.clone(),
                    item.digest.clone(),
                    image.id(),
                    recipe.clone(),
                    Ok((image.clone(), image.source().histogram())),
                ))
                .unwrap();
            app.poll_edit_preview();
            assert_eq!(app.editing.previews.contains_key(&item.id), accepted);
            assert_eq!(app.editing.thumbnails.contains_key(&image.id()), accepted);
            let entry = &app.editing.entries[&item.id];
            assert_eq!(entry.draft.as_ref(), Some(&recipe));
            assert_eq!(entry.loaded.as_ref().unwrap().generation, 3);
            assert_eq!(entry.loaded.as_ref().unwrap().revision, 2);
        }
        crate::ui::settings_regressions::settle(&mut app, &ctx, true);
    }

    fn edited_recipe() -> EditRecipe {
        let mut recipe = EditRecipe::neutral(tr_core::decoder::RawEngine::TrueRenderer);
        recipe.process_version = 2;
        recipe.raw_wb.red = 1500;
        recipe.raw_wb.blue = 800;
        recipe.exposure_ev = 1.;
        recipe.brightness = 12.;
        recipe.contrast = 15.;
        recipe.highlights = -20.;
        recipe.shadows = 25.;
        recipe.whites = 8.;
        recipe.blacks = -6.;
        recipe.temperature = 25.;
        recipe.tint = -8.;
        recipe.saturation = 20.;
        recipe.vibrance = 40.;
        recipe.protect_warm = true;
        recipe.curve = vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 0.5, y: 0.6 },
            CurvePoint { x: 1., y: 1. },
        ];
        recipe
    }

    #[test]
    fn group_resets_preserve_other_adjustments_and_durable_history() {
        for group in [ResetGroup::Light, ResetGroup::Curve, ResetGroup::Color] {
            let dir = tempfile::tempdir().unwrap();
            let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
            let item = catalog
                .observe(Path::new("synthetic.png"), "synthetic", 4)
                .unwrap();
            let original = edited_recipe();
            let saved = catalog.save_edit(&item.id, 0, &original).unwrap();
            let mut reset = original.clone();
            group.apply(&mut reset);
            reset.validate().unwrap();
            assert_eq!(reset.raw_wb, original.raw_wb);
            assert_eq!(reset.raw_engine, original.raw_engine);
            assert_eq!(reset.process_version, original.process_version);
            // Only the selected family's serialized values may differ.
            let before = serde_json::to_value(&original).unwrap();
            let after = serde_json::to_value(&reset).unwrap();
            let keys: &[&str] = match group {
                ResetGroup::Light => &[
                    "exposure_ev",
                    "brightness",
                    "contrast",
                    "highlights",
                    "shadows",
                    "whites",
                    "blacks",
                ],
                ResetGroup::Curve => &["curve"],
                ResetGroup::Color => &[
                    "temperature",
                    "tint",
                    "saturation",
                    "vibrance",
                    "protect_warm",
                ],
            };
            for (key, value) in before.as_object().unwrap() {
                if !keys.contains(&key.as_str()) {
                    assert_eq!(Some(value), after.get(key));
                } else if key == "curve" {
                    assert!(reset.curve.is_empty());
                } else if key == "protect_warm" {
                    assert!(!reset.protect_warm);
                } else {
                    assert_eq!(after.get(key).and_then(|v| v.as_f64()).unwrap_or(0.), 0.);
                }
            }
            let second = catalog
                .save_edit(&item.id, saved.generation, &reset)
                .unwrap();
            let undone = catalog
                .step_edit(&item.id, second.generation, true)
                .unwrap();
            assert_eq!(undone.recipe, original);
            drop(catalog);
            let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
            let reopened = catalog.load_edit(&item.id, original.raw_engine).unwrap();
            let redone = catalog
                .step_edit(&item.id, reopened.generation, false)
                .unwrap();
            assert_eq!(redone.recipe, reset);
            group.apply(&mut reset);
            assert_eq!(redone.recipe, reset);
        }
    }

    #[test]
    fn adjustment_keeps_drag_commit_and_numeric_entry_at_compact_widths() {
        for lang in [Language::Italian, Language::English] {
            for width in [200., 248., 328.] {
                let ctx = egui::Context::default();
                style::apply(&ctx);
                let mut value = 0_f32;
                let mut commits = 0;
                let mut frame = |events: Vec<egui::Event>| {
                    let mut rect = egui::Rect::NOTHING;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 180.),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            let response = adjustment(
                                ui,
                                lang.text("Esposizione"),
                                &mut value,
                                -10. ..=10.,
                                " EV",
                                None,
                            );
                            commits += usize::from(
                                response.drag_stopped()
                                    || (response.changed() && !response.dragged()),
                            );
                            rect = response.rect;
                        },
                    );
                    output.textures_delta.clear();
                    assert!(
                        rect.left() >= 0. && rect.right() <= width,
                        "{lang:?}: {rect:?}"
                    );
                    (rect, value, commits)
                };
                let (rect, _, _) = frame(vec![]);
                let pointer = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                let start = egui::pos2(rect.center().x, rect.bottom() - 10.);
                let end = egui::pos2(rect.right() - 35., start.y);
                frame(vec![egui::Event::PointerMoved(start), pointer(start, true)]);
                let (_, moved, during) = frame(vec![egui::Event::PointerMoved(end)]);
                assert!(moved > 0.);
                assert_eq!(
                    during, 0,
                    "A slider drag must not create intermediate revisions"
                );
                let (_, moved, after) = frame(vec![pointer(end, false)]);
                assert!(moved > 0.);
                assert_eq!(after, 1);
                let number = egui::pos2(rect.right() - 24., rect.top() + 10.);
                frame(vec![
                    egui::Event::PointerMoved(number),
                    pointer(number, true),
                ]);
                frame(vec![pointer(number, false)]);
                let (_, typed, saved) = frame(vec![
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            command: true,
                            ctrl: true,
                            ..Default::default()
                        },
                    },
                    egui::Event::Text("0.75".into()),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                assert_eq!(typed, 0.75, "Numeric entry {lang:?} at {width}");
                assert_eq!(saved, 2);
            }
        }
    }

    #[test]
    fn reset_button_requires_enabled_changed_recipe_and_a_click() {
        for enabled in [false, true] {
            let ctx = egui::Context::default();
            let mut recipe = edited_recipe();
            let original = recipe.clone();
            let mut rect = egui::Rect::NOTHING;
            let mut clicked = false;
            for pressed in [None, Some(true), Some(false)] {
                let events = pressed.map_or_else(Vec::new, |pressed| {
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                });
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            clicked |= reset_group_button(
                                ui,
                                "Reset light",
                                ResetGroup::Light,
                                &mut recipe,
                            );
                            rect = ui.min_rect();
                        });
                    },
                );
                output.textures_delta.clear();
            }
            assert_eq!(clicked, enabled);
            assert_eq!(
                recipe.exposure_ev,
                if enabled { 0. } else { original.exposure_ev }
            );
            assert_eq!(recipe.raw_wb, original.raw_wb);
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod shortcut_tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};
    fn edit_key(
        app: &mut TrueRenderer,
        ctx: &egui::Context,
        key: egui::Key,
        alt: bool,
        shift: bool,
    ) {
        let modifiers = egui::Modifiers {
            alt,
            shift,
            command: key == egui::Key::Z,
            ..egui::Modifiers::NONE
        };
        for pressed in [true, false] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events: vec![
                        egui::Event::ModifiersChanged(modifiers),
                        egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers,
                        },
                    ],
                    ..Default::default()
                },
                |ui| app.keyboard(ui.ctx()),
            );
            out.textures_delta.clear();
        }
    }

    fn settle_edits(app: &mut TrueRenderer, ctx: &egui::Context, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            settle(app, ctx, true);
            let e = &app.editing.entries[id];
            assert!(e.error.is_none(), "{:?}", e.error);
            if e.loaded.is_some() && !e.loading && !e.pending {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn click_edit_control(app: &mut TrueRenderer, ctx: &egui::Context, item: &Item, label: &str) {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        app.state.view = ViewMode::Preview;
        let mut pos = None;
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                app.editing_controls(ui, item)
            });
            pos = out.shapes.iter().find_map(|s| find(&s.shape, label));
            out.textures_delta.clear();
        }
        let pos = pos.unwrap_or_else(|| panic!("Missing control: {label}"));
        for pressed in [true, false] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| app.editing_controls(ui, item),
            );
            out.textures_delta.clear();
        }
    }

    #[test]
    fn retry_after_history_error_reloads_without_creating_a_revision() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.cache_settings.language = Language::Italian;
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        settle_edits(&mut app, &ctx, &item.id);
        let before = app.editing.entries[&item.id].loaded.clone().unwrap();
        app.edit_result(
            item.id.clone(),
            Err("Transient history read failure".into()),
        );
        click_edit_control(&mut app, &ctx, &item, "Riprova salvataggio");
        settle_edits(&mut app, &ctx, &item.id);
        let after = app.editing.entries[&item.id].loaded.as_ref().unwrap();
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.recipe, before.recipe);
    }

    #[test]
    fn failed_drafts_can_be_retried_or_explicitly_discarded_in_both_languages() {
        for lang in [Language::Italian, Language::English] {
            let (dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            app.cache_settings.language = lang;
            let item = app.state.items[0].clone();
            app.ensure_edit_loaded(&item);
            settle_edits(&mut app, &ctx, &item.id);
            let mut draft = app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .clone();
            draft.exposure_ev = 1.;
            app.apply_edit_draft(&item.id, draft.clone());
            app.edit_result(item.id.clone(), Err("Temporary write failure".into()));
            assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&draft));
            click_edit_control(&mut app, &ctx, &item, lang.text("Riprova salvataggio"));
            settle_edits(&mut app, &ctx, &item.id);
            assert_eq!(
                app.editing.entries[&item.id]
                    .loaded
                    .as_ref()
                    .unwrap()
                    .recipe,
                draft
            );

            // A second writer changes the durable head. Retrying our stale draft
            // must preserve it and report the conflict, never overwrite that head.
            let mut catalog = tr_store::Catalog::open(&dir.path().join("data")).unwrap();
            let mut other = draft.clone();
            other.exposure_ev = 4.;
            let saved = catalog.save_edit(&item.id, 1, &other).unwrap();
            draft.exposure_ev = 2.;
            app.apply_edit_draft(&item.id, draft.clone());
            app.commit_edit(&item.id);
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.editing.entries[&item.id].pending {
                settle(&mut app, &ctx, true);
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(app.editing.entries[&item.id].error.is_some());
            assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&draft));
            click_edit_control(
                &mut app,
                &ctx,
                &item,
                lang.text("Scarta bozza e ricarica ricetta"),
            );
            settle_edits(&mut app, &ctx, &item.id);
            let reloaded = app.editing.entries[&item.id].loaded.as_ref().unwrap();
            assert_eq!(reloaded.generation, saved.generation);
            assert_eq!(reloaded.recipe, other);
            assert_eq!(
                catalog
                    .load_edit(&item.id, other.raw_engine)
                    .unwrap()
                    .generation,
                saved.generation
            );
        }
    }

    #[test]
    fn unsaved_preferences_never_select_the_engine_of_a_new_edit() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let active_engine = app.service.cache.settings().raw_engine;
        app.cache_settings.raw_engine = tr_core::decoder::RawEngine::choices()
            .find(|engine| *engine != active_engine)
            .unwrap();
        app.ensure_edit_loaded(&item);
        settle_edits(&mut app, &ctx, &item.id);
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .raw_engine,
            active_engine
        );
        let mut draft = EditRecipe::neutral(active_engine);
        draft.exposure_ev = 1.;
        app.apply_edit_draft(&item.id, draft);
        app.commit_edit(&item.id);
        settle_edits(&mut app, &ctx, &item.id);
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .raw_engine,
            active_engine
        );
    }

    #[test]
    fn changing_photo_commits_the_previous_draft() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[app.state.visible[0]].clone();
        app.command(Command::Select {
            id: item.id.clone(),
            extend: false,
        });
        app.ensure_edit_loaded(&item);
        settle_edits(&mut app, &ctx, &item.id);
        let mut draft = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .recipe
            .clone();
        draft.exposure_ev = 1.25;
        app.apply_edit_draft(&item.id, draft.clone());
        app.command(Command::Move(1));
        assert_ne!(app.state.current.as_deref(), Some(item.id.as_str()));
        assert!(app.editing.entries[&item.id].pending);
        settle_edits(&mut app, &ctx, &item.id);
        let entry = &app.editing.entries[&item.id];
        assert!(!entry.dirty());
        assert_eq!(entry.loaded.as_ref().unwrap().recipe, draft);
        assert_eq!(entry.loaded.as_ref().unwrap().generation, 1);
    }

    #[test]
    fn editing_shortcuts_copy_paste_and_step_persisted_history() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let items = app.state.items.clone();
        for item in &items {
            app.ensure_edit_loaded(item);
            settle_edits(&mut app, &ctx, &item.id);
        }
        let source = &items[0].id;
        let target = &items[1].id;
        let mut recipe = app.editing.entries[source]
            .loaded
            .as_ref()
            .unwrap()
            .recipe
            .clone();
        recipe.exposure_ev = 1.25;
        app.editing.entries.get_mut(source).unwrap().draft = Some(recipe.clone());
        app.commit_edit(source);
        settle_edits(&mut app, &ctx, source);
        app.state.current = Some(source.clone());
        edit_key(&mut app, &ctx, egui::Key::C, true, true);
        app.state.current = Some(target.clone());
        edit_key(&mut app, &ctx, egui::Key::V, true, true);
        settle_edits(&mut app, &ctx, target);
        assert_eq!(
            app.editing.entries[target]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .exposure_ev,
            1.25
        );
        let generation = app.editing.entries[target]
            .loaded
            .as_ref()
            .unwrap()
            .generation;
        edit_key(&mut app, &ctx, egui::Key::V, true, true);
        assert!(!app.editing.entries[target].pending);
        assert_eq!(
            app.editing.entries[target]
                .loaded
                .as_ref()
                .unwrap()
                .generation,
            generation
        );
        edit_key(&mut app, &ctx, egui::Key::Z, true, false);
        settle_edits(&mut app, &ctx, target);
        assert_eq!(
            app.editing.entries[target]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .exposure_ev,
            0.
        );
        edit_key(&mut app, &ctx, egui::Key::Z, true, true);
        settle_edits(&mut app, &ctx, target);
        assert_eq!(
            app.editing.entries[target]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .exposure_ev,
            1.25
        );
        assert_eq!(
            app.editing.entries[source].loaded.as_ref().unwrap().recipe,
            recipe
        );
        assert!(app.state.items.iter().all(|i| i.annotation.rating == 0));
    }

    #[test]
    fn editing_shortcuts_respect_focus_modal_and_pending_work() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        settle_edits(&mut app, &ctx, &item.id);
        app.state.current = Some(item.id.clone());
        // Copy a non-neutral saved recipe, then restore the destination state.
        app.editing
            .entries
            .get_mut(&item.id)
            .unwrap()
            .loaded
            .as_mut()
            .unwrap()
            .recipe
            .exposure_ev = 1.;
        app.editing.entries.get_mut(&item.id).unwrap().draft = None;
        edit_key(&mut app, &ctx, egui::Key::C, true, true);
        app.editing
            .entries
            .get_mut(&item.id)
            .unwrap()
            .loaded
            .as_mut()
            .unwrap()
            .recipe
            .exposure_ev = 0.;
        for mode in 0..8 {
            let focus = egui::Id::new("other-control");
            match mode {
                0 => app.editing.wb_pending = true,
                1 => app.editing.entries.get_mut(&item.id).unwrap().pending = true,
                2 => app.show_settings = true,
                3 => ctx.memory_mut(|m| m.request_focus(focus)),
                4 => app.editing.entries.get_mut(&item.id).unwrap().error = Some("test".into()),
                5 => app.editing.entries.get_mut(&item.id).unwrap().loading = true,
                7 => app.photo_export.open = true,
                _ => {
                    let mut r = app.editing.entries[&item.id]
                        .loaded
                        .as_ref()
                        .unwrap()
                        .recipe
                        .clone();
                    r.exposure_ev = 0.5;
                    app.editing.entries.get_mut(&item.id).unwrap().draft = Some(r);
                }
            }
            let before = app.editing.entries[&item.id].draft.clone();
            edit_key(&mut app, &ctx, egui::Key::V, true, true);
            assert_eq!(app.editing.entries[&item.id].draft, before, "mode={mode}");
            app.editing.wb_pending = false;
            app.show_settings = false;
            app.photo_export.open = false;
            ctx.memory_mut(|m| m.surrender_focus(focus));
            let e = app.editing.entries.get_mut(&item.id).unwrap();
            e.pending = false;
            e.loading = false;
            e.error = None;
            e.draft = None;
        }
        edit_key(&mut app, &ctx, egui::Key::V, true, true);
        assert!(app.editing.entries[&item.id].pending);
        settle_edits(&mut app, &ctx, &item.id);
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .exposure_ev,
            1.
        );
    }
}
