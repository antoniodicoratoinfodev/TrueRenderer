use super::*;
use tr_core::editing::{CurvePoint, EditRecipe};
use tr_store::LoadedEdit;

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
    String,
    u64,
    u32,
    EditRecipe,
    Result<Arc<ImageLevels>, String>,
);
type ThumbnailOutcome = (
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
    recipe: EditRecipe,
    image: Arc<ImageLevels>,
    touched: u64,
}
pub(super) struct EditingUi {
    pub entries: HashMap<String, EditEntry>,
    pub show_original: bool,
    pub verify_final: Option<String>,
    previews: HashMap<String, EditPreview>,
    inflight: Option<(String, u64, EditRecipe)>,
    tx: std::sync::mpsc::Sender<PreviewOutcome>,
    rx: std::sync::mpsc::Receiver<PreviewOutcome>,
    preview_errors: HashMap<String, (u64, u32, EditRecipe, String)>,
    pub picker_error: Option<(String, String)>,
    smoke_ready_at: Option<Instant>,
    thumbnails: HashMap<u64, ThumbnailPreview>,
    thumbnail_inflight: HashSet<u64>,
    thumbnail_errors: HashMap<u64, (String, EditRecipe, String)>,
    thumbnail_tx: std::sync::mpsc::Sender<ThumbnailOutcome>,
    thumbnail_rx: std::sync::mpsc::Receiver<ThumbnailOutcome>,
}
impl Default for EditingUi {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (thumbnail_tx, thumbnail_rx) = std::sync::mpsc::channel();
        Self {
            entries: HashMap::new(),
            show_original: false,
            verify_final: None,
            previews: HashMap::new(),
            inflight: None,
            tx,
            rx,
            preview_errors: HashMap::new(),
            picker_error: None,
            smoke_ready_at: None,
            thumbnails: HashMap::new(),
            thumbnail_inflight: HashSet::new(),
            thumbnail_errors: HashMap::new(),
            thumbnail_tx,
            thumbnail_rx,
        }
    }
}
impl TrueRenderer {
    pub(super) fn develop_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(50));
        let timed_out = self.started.elapsed() > Duration::from_secs(90) || self.fatal;
        let target = self
            .state
            .items
            .iter()
            .find(|item| item.name == "02_Paesaggio_analitico.png")
            .cloned();
        if timed_out {
            let _ = std::fs::write(
                self.root.join("reports/develop-ui.json"),
                b"{\"passed\":false,\"reason\":\"timeout or fatal error\"}",
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let Some(item) = target else {
            return;
        };
        match self.smoke_stage {
            0 if !self.scanning => {
                self.set_language(Language::Italian);
                self.show_inspector = true;
                self.state.view = ViewMode::Preview;
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                self.smoke_stage = 1;
            }
            1 => {
                self.ensure_edit_loaded(&item);
                if let Some(entry) = self.editing.entries.get_mut(&item.id)
                    && let Some(saved) = &entry.loaded
                    && !entry.pending
                {
                    let mut recipe = saved.recipe.clone();
                    recipe.exposure_ev = 0.75;
                    recipe.temperature = 25.;
                    recipe.tint = -8.;
                    if recipe != saved.recipe {
                        entry.draft = Some(recipe);
                        self.commit_edit(&item.id);
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
                    self.cache
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
                    self.smoke_stage = 3;
                    self.capture_screenshot(ctx, "develop-viewer");
                }
            }
            3 if self.screenshots.contains("develop-viewer") => {
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
                let edit = self
                    .editing
                    .entries
                    .get(&item.id)
                    .and_then(|entry| entry.loaded.as_ref());
                let unchanged = tr_platform::snapshot(&item.path)
                    .is_ok_and(|(_, digest)| digest == item.digest);
                let report = serde_json::json!({
                    "application":"TrueRenderer",
                    "version":env!("CARGO_PKG_VERSION"),
                    "passed":edit.is_some_and(|saved| saved.generation > 0) && unchanged && !self.presenter.has_errors() && self.errors.is_empty() && !self.fatal,
                    "fixture":item.name,
                    "source_digest":item.digest,
                    "recipe":edit.map(|saved| &saved.recipe),
                    "saved_generation":edit.map(|saved| saved.generation),
                    "source_unchanged":unchanged,
                    "screenshots":["develop-viewer.png","develop-grid.png"],
                    "scope":"Native macOS UI with generated PNG and isolated library; saved exposure and relative RGB correction, edited viewer/grid. No RAW WB, physical display or Windows qualification."
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
    pub(super) fn clear_edit_preview(&mut self) {
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
        if let Some(preview) = self.editing.previews.remove(id) {
            self.presenter
                .invalidate_sources(&HashSet::from([preview.image.id()]));
        }
        self.editing.preview_errors.remove(id);
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
            .retain(|_, (source, _, _, _)| !pressure && live.contains(source));
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
        let engine = self.cache_settings.raw_engine;
        if self.request(Request::LoadEdit {
            id: id.clone(),
            engine,
        }) {
            self.editing.entries.entry(id).or_default().loading = true;
        }
    }
    pub(super) fn edit_result(&mut self, id: String, result: Result<LoadedEdit, String>) {
        self.clear_edit_thumbnails(&id);
        if self.sample_item_id.as_deref() == Some(id.as_str()) {
            self.sample = None;
            self.sample_item_id = None;
            self.sample_level = None;
            self.sample_from_current_render = false;
        }
        let entry = self.editing.entries.entry(id).or_default();
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
            recipe.raw_engine = self.cache_settings.raw_engine;
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
    fn step_edit(&mut self, id: &str, undo: bool) {
        let Some(entry) = self.editing.entries.get(id) else {
            return;
        };
        if entry.pending || entry.dirty() {
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
    pub(super) fn editing_controls(&mut self, ui: &mut egui::Ui, item: &Item) {
        let lang = self.cache_settings.language;
        self.ensure_edit_loaded(item);
        ui.add_space(12.);
        egui::CollapsingHeader::new(lang.text("Sviluppo"))
            .id_salt("photographic-edit")
            .default_open(self.state.view != ViewMode::Grid)
            .show(ui, |ui| {
                let Some(entry) = self.editing.entries.get(&item.id) else {
                    ui.label(lang.text("Caricamento ricetta…"));
                    return;
                };
                let error = entry.error.clone();
                let saved = entry.loaded.clone();
                let pending = entry.pending;
                let existing_draft = entry.draft.clone();
                if let Some(error) = &error {
                    ui.colored_label(AMBER, error);
                    if ui.button(lang.text("Riprova salvataggio")).clicked() {
                        if saved.is_some() {
                            self.commit_edit(&item.id);
                        } else {
                            self.editing.entries.remove(&item.id);
                        }
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
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can_undo && !pending,
                            egui::Button::new(lang.text("Annulla sviluppo")),
                        )
                        .clicked()
                    {
                        self.step_edit(&item.id, true);
                    }
                    if ui
                        .add_enabled(
                            can_redo && !pending,
                            egui::Button::new(lang.text("Ripeti sviluppo")),
                        )
                        .clicked()
                    {
                        self.step_edit(&item.id, false);
                    }
                    if ui.button(lang.text("Prima/Dopo")).clicked() {
                        self.editing.show_original = !self.editing.show_original;
                        self.sample = None;
                        self.sample_item_id = None;
                        self.sample_level = None;
                        self.sample_from_current_render = false;
                    }
                });
                if ui.button(lang.text("Verifica resa finale")).clicked() {
                    self.editing.verify_final = Some(item.id.clone());
                    self.quality_overrides
                        .insert(item.id.clone(), PreviewQuality::Full);
                    self.clear_edit_preview();
                }
                ui.add_enabled_ui(!pending, |ui| {
                    ui.label(RichText::new(lang.text("Luce")).strong());
                    for (label, value, min, max, suffix) in [
                        ("Esposizione", &mut draft.exposure_ev, -10., 10., " EV"),
                        ("Luminosità", &mut draft.brightness, -100., 100., ""),
                        ("Contrasto", &mut draft.contrast, -100., 100., ""),
                        ("Alte luci", &mut draft.highlights, -100., 100., ""),
                        ("Ombre", &mut draft.shadows, -100., 100., ""),
                        ("Bianchi", &mut draft.whites, -100., 100., ""),
                        ("Neri", &mut draft.blacks, -100., 100., ""),
                    ] {
                        let response = ui.add(
                            egui::Slider::new(value, min..=max)
                                .text(lang.text(label))
                                .suffix(suffix),
                        );
                        changed |= response.changed();
                        commit |=
                            response.drag_stopped() || (response.changed() && !response.dragged());
                    }
                    ui.label(RichText::new(lang.text("Curva tonale")).strong());
                    let mut mid = draft.curve.get(1).map_or(0.5, |p| p.y);
                    let response = ui.add(
                        egui::Slider::new(&mut mid, 0. ..=1.).text(lang.text("Mezzitoni curva")),
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
                    commit |=
                        response.drag_stopped() || (response.changed() && !response.dragged());
                    ui.label(
                        RichText::new(lang.text("Colore del render · correzione RGB relativa"))
                            .strong(),
                    );
                    for (label, value) in [
                        ("Temperatura RGB", &mut draft.temperature),
                        ("Tinta RGB", &mut draft.tint),
                        ("Saturazione", &mut draft.saturation),
                    ] {
                        let response =
                            ui.add(egui::Slider::new(value, -100. ..=100.).text(lang.text(label)));
                        changed |= response.changed();
                        commit |=
                            response.drag_stopped() || (response.changed() && !response.dragged());
                    }
                    ui.label(
                        RichText::new(
                            lang.text("La correzione RGB non cambia il WB RAW del decoder."),
                        )
                        .small()
                        .color(MUTED),
                    );
                    let sample = if self.sample_item_id.as_deref() == Some(item.id.as_str())
                        && self.sample_level == Some(0)
                        && self.sample_from_current_render
                    {
                        self.sample
                            .as_ref()
                            .map(|sample| (sample.working, sample.x, sample.y))
                    } else {
                        None
                    };
                    if let Some((_, x, y)) = sample {
                        ui.label(localized_format!(
                            lang,
                            "Ultimo campione RGB: {}, {}",
                            "Last RGB sample: {}, {}",
                            x,
                            y
                        ));
                    }
                    if self.sample_item_id.as_deref() == Some(item.id.as_str())
                        && (self.sample_level != Some(0) || !self.sample_from_current_render)
                    {
                        ui.label(
                            lang.text("Verifica la resa finale e campiona la vista modificata."),
                        );
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
                                sample.is_some(),
                                egui::Button::new(lang.text("Neutralizza campione RGB")),
                            )
                            .clicked()
                            && let Some((pixel, _, _)) = sample
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
                        ui.colored_label(AMBER, error);
                    }
                    if ui.button(lang.text("Sviluppo originale")).clicked() {
                        draft = EditRecipe::neutral(saved.recipe.raw_engine);
                        changed = true;
                        commit = true;
                    }
                });
                if changed {
                    self.editing.verify_final = None;
                    self.editing.picker_error = None;
                    self.editing.entries.get_mut(&item.id).unwrap().draft = Some(draft);
                    self.sample = None;
                    self.sample_item_id = None;
                    self.sample_level = None;
                    self.sample_from_current_render = false;
                    self.clear_edit_preview();
                    self.clear_edit_thumbnails(&item.id);
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
                    RichText::new(
                        lang.text("Vista modificata provvisoria; export alla risoluzione nativa."),
                    )
                    .small()
                    .color(MUTED),
                );
            });
    }
    pub(super) fn poll_edit_preview(&mut self) {
        while let Ok((id, source, base, recipe, result)) = self.editing.rx.try_recv() {
            self.editing.inflight = None;
            if self.editing.entries.get(&id).and_then(|e| e.draft.as_ref()) != Some(&recipe) {
                continue;
            }
            match result {
                Ok(image) => {
                    self.clear_edit_preview_for(&id);
                    if self.editing.previews.len() >= 2
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
                            recipe,
                            image,
                            touched: self.frame_number,
                        },
                    );
                }
                Err(error) => {
                    self.editing
                        .preview_errors
                        .insert(id, (source, base, recipe, error));
                }
            }
        }
        while let Ok((id, digest, source, recipe, result)) = self.editing.thumbnail_rx.try_recv() {
            self.editing.thumbnail_inflight.remove(&source);
            let current = self.editing.entries.get(&id).is_some_and(|entry| {
                entry
                    .loaded
                    .as_ref()
                    .is_some_and(|saved| saved.source_digest == digest)
                    && entry.draft.as_ref() == Some(&recipe)
            });
            if !current {
                continue;
            }
            match result {
                Ok((image, histogram)) => {
                    self.editing.thumbnail_errors.remove(&source);
                    if self.editing.thumbnails.len() >= 64
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
        if saved.source_digest != digest {
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
        if self.editing.thumbnail_inflight.len() < 2
            && !self.editing.thumbnail_inflight.contains(&source)
        {
            let index = neutral
                .levels()
                .iter()
                .position(|level| level.width.max(level.height) <= 512)
                .unwrap_or(neutral.levels().len() - 1);
            let bytes = neutral.levels()[index].pixels.len() as u64 * 16 * 3;
            if let Some(lease) = self.service.cache.memory.try_reserve(bytes) {
                let tx = self.editing.thumbnail_tx.clone();
                let wake = self.service.wake.clone();
                let id = id.to_owned();
                let digest = digest.to_owned();
                let base = neutral.base_level() + index as u32;
                let size = neutral.source_size();
                self.editing.thumbnail_inflight.insert(source);
                std::thread::spawn(move || {
                    let result = (|| -> anyhow::Result<_> {
                        let mut raster = neutral.levels()[index].clone();
                        recipe.apply(&mut raster)?;
                        let mut image =
                            ImageLevels::from_reference_mip(raster, size, base, neutral.opaque())?;
                        image.attach_lease(lease);
                        let histogram = image.source().histogram();
                        Ok((Arc::new(image), histogram))
                    })()
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send((id, digest, source, recipe, result));
                    wake.request_repaint();
                });
            }
        }
        None
    }
    pub(super) fn edited_preview(
        &mut self,
        id: &str,
        digest: &str,
        neutral: Arc<ImageLevels>,
    ) -> Option<Arc<ImageLevels>> {
        let entry = self.editing.entries.get(id)?;
        let saved = entry.loaded.as_ref()?;
        if saved.source_digest != digest {
            return None;
        }
        let recipe = entry.draft.as_ref().unwrap_or(&saved.recipe).clone();
        if recipe.is_neutral() || self.editing.show_original {
            return Some(neutral);
        }
        let source = neutral.id();
        let index = if self.editing.verify_final.as_deref() == Some(id) && neutral.base_level() == 0
        {
            0
        } else {
            neutral
                .levels()
                .iter()
                .position(|l| l.width.max(l.height) <= 1024)
                .unwrap_or(neutral.levels().len() - 1)
        };
        let base = neutral.base_level() + index as u32;
        if let Some(ready) = self.editing.previews.get_mut(id)
            && ready.source == source
            && ready.recipe == recipe
            && ready.image.base_level() == base
        {
            ready.touched = self.frame_number;
            return Some(ready.image.clone());
        }
        if self.editing.preview_errors.get(id).is_some_and(
            |(failed_source, failed_base, failed_recipe, _)| {
                *failed_source == source && *failed_base == base && *failed_recipe == recipe
            },
        ) {
            return None;
        }
        self.clear_edit_preview_for(id);
        if self.editing.inflight.is_none() {
            let level = &neutral.levels()[index];
            let bytes = level.pixels.len() as u64 * 16 * 3;
            if let Some(lease) = self.service.cache.memory.try_reserve(bytes) {
                let tx = self.editing.tx.clone();
                let wake = self.service.wake.clone();
                let id = id.to_owned();
                let size = neutral.source_size();
                self.editing.inflight = Some((id.clone(), source, recipe.clone()));
                std::thread::spawn(move || {
                    let result = (|| -> anyhow::Result<Arc<ImageLevels>> {
                        let mut raster = neutral.levels()[index].clone();
                        recipe.apply(&mut raster)?;
                        let mut image =
                            ImageLevels::from_reference_mip(raster, size, base, neutral.opaque())?;
                        image.attach_lease(lease);
                        Ok(Arc::new(image))
                    })()
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send((id, source, base, recipe, result));
                    wake.request_repaint();
                });
            } else {
                self.editing.preview_errors.insert(
                    id.into(),
                    (
                        0,
                        base,
                        recipe,
                        "Memoria insufficiente per l'anteprima modificata".into(),
                    ),
                );
            }
        }
        None
    }
    pub(super) fn edited_preview_error(&self, id: &str) -> Option<&str> {
        self.editing
            .preview_errors
            .get(id)
            .map(|(_, _, _, error)| error.as_str())
    }
}
