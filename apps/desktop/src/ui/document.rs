//! Raster editing within the normal browser and viewer, sharing the application's isolated decoder.
use anyhow::{Result, ensure};
use eframe::egui::{self, Color32};
use std::{collections::VecDeque, path::PathBuf, sync::mpsc};
use tr_core::{
    color::LinearImage,
    document::{Blend, Brush, ColorSpace, Document, RasterLayer, SelectionMode, Unit},
};

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Move,
    Paint,
    Erase,
    Clone,
    Heal,
    AutoHeal,
    Free,
    Polygon,
    Rectangle,
}

mod conversion;
pub(in crate::ui) use conversion::PhotoAction;
mod develop;
mod export;
mod lifecycle;
mod panels;
mod presentation;
mod probe;
mod thumbnails;
mod tools;
#[cfg(test)]
mod workflow_tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Develop,
    Layers,
    Tools,
    Export,
}
impl Tab {
    pub const ALL: [Self; 4] = [Self::Develop, Self::Layers, Self::Tools, Self::Export];
    pub fn label(self) -> &'static str {
        match self {
            Self::Develop => "Sviluppo",
            Self::Layers => "Livelli",
            Self::Tools => "Tools",
            Self::Export => "Export",
        }
    }
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Layers,
            2 => Self::Tools,
            3 => Self::Export,
            _ => Self::Develop,
        }
    }
}
type Imported = Result<crate::decode_pool::ImportedDocumentImage>;
pub(in crate::ui) struct CanvasCoordinates<'a> {
    pub layer: &'a RasterLayer,
    pub rect: egui::Rect,
    pub size: [u32; 2],
}
impl CanvasCoordinates<'_> {
    fn canvas_point(&self, p: egui::Pos2) -> [f32; 2] {
        [
            (p.x - self.rect.left()) / self.rect.width() * self.size[0] as f32,
            (p.y - self.rect.top()) / self.rect.height() * self.size[1] as f32,
        ]
    }
    pub fn source_uv(&self, p: egui::Pos2) -> Option<[f32; 2]> {
        if !self.rect.contains(p) {
            return None;
        }
        let p = self.layer.source_point(self.canvas_point(p))?;
        let uv = [
            p[0] / self.layer.width as f32,
            p[1] / self.layer.height as f32,
        ];
        uv.iter().all(|v| (0. ..=1.).contains(v)).then_some(uv)
    }
    pub fn output_point(&self, p: egui::Pos2) -> Option<[f32; 2]> {
        if !self.rect.contains(p) {
            return None;
        }
        let p = self.layer.transform.inverse(self.canvas_point(p));
        p.iter().all(|v| *v >= 0.).then_some(p)
    }
    pub fn aspect(&self) -> f32 {
        self.layer.height as f32 / self.layer.width as f32
    }
}
pub(super) struct Editor {
    pub tab: Tab,
    pub source_id: Option<String>,
    import_photo: Option<String>,
    conversion: Option<conversion::Conversion>,
    publishing: Option<conversion::Publishing>,
    source_path: Option<PathBuf>,
    return_photo: Option<PathBuf>,
    confirm_return: bool,
    import_path: Option<PathBuf>,
    pub directory: PathBuf,
    name: String,
    pub published: VecDeque<PathBuf>,
    pub reveal: Option<PathBuf>,
    pub requested_tab: Option<Tab>,
    attempted: Option<PathBuf>,
    previews: thumbnails::Previews,
    export_format: export::ImageFormat,
    export_quality: u8,
    export_edge: u32,
    export_resolution: Option<tr_core::protocol::Resolution>,
    export_space: Option<ColorSpace>,
    notice: String,
    doc: Option<Document>,
    path: Option<PathBuf>,
    dirty: bool,
    undo: VecDeque<Document>,
    redo: VecDeque<Document>,
    selected: usize,
    tool: Tool,
    rgb_picker: bool,
    radius: f32,
    hardness: f32,
    strength: f32,
    color: [f32; 4],
    selection_mode: SelectionMode,
    feather: f32,
    points: Vec<[f32; 2]>,
    source: Option<[f32; 2]>,
    stroke: Option<Document>,
    last_point: Option<[f32; 2]>,
    stroke_origin: Option<[f32; 2]>,
    texture: Option<egui::TextureHandle>,
    composite: Option<tr_core::resample::Pyramid>,
    view_texture: Option<egui::TextureHandle>,
    view_region: Option<tr_core::resample::Region>,
    needs_render: bool,
    error: String,
    service: mpsc::SyncSender<crate::service::Request>,
    memory: tr_core::budget::MemoryBudget,
    admission: Option<std::sync::Arc<crate::document_memory::Admission>>,
    import: Option<mpsc::Receiver<Imported>>,
    new_dialog: bool,
    width: f64,
    height: f64,
    ppi: f64,
    unit: Unit,
    space: ColorSpace,
    transparent: bool,
    adjustments: super::editing::advanced::Controls,
    effects: super::editing::layers::Controls,
}
impl Editor {
    pub fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"passed":self.doc.is_some() && self.error.is_empty(),"error":self.error,
            "layers":self.doc.as_ref().map(|d|d.layers.len()),"bytes":self.doc.as_ref().map(Document::storage_bytes),
            "scope":"Native document open and presentation; not retouch quality or cross-platform qualification"})
    }
    pub fn new(
        service: mpsc::SyncSender<crate::service::Request>,
        memory: tr_core::budget::MemoryBudget,
    ) -> Self {
        Self {
            tab: Tab::Develop,
            source_id: None,
            import_photo: None,
            conversion: None,
            publishing: None,
            source_path: None,
            return_photo: None,
            confirm_return: false,
            import_path: None,
            directory: PathBuf::new(),
            name: "Documento".into(),
            published: VecDeque::new(),
            reveal: None,
            requested_tab: None,
            attempted: None,
            previews: thumbnails::Previews::new(memory.clone()),
            export_format: export::ImageFormat::Png16,
            export_quality: 95,
            export_edge: 0,
            export_resolution: None,
            export_space: None,
            notice: String::new(),
            doc: None,
            path: None,
            dirty: false,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            selected: 0,
            tool: Tool::Move,
            rgb_picker: false,
            radius: 20.,
            hardness: 0.5,
            strength: 1.,
            color: [0., 0., 0., 1.],
            selection_mode: SelectionMode::Replace,
            feather: 0.,
            points: vec![],
            source: None,
            stroke: None,
            last_point: None,
            stroke_origin: None,
            texture: None,
            composite: None,
            view_texture: None,
            view_region: None,
            needs_render: false,
            error: String::new(),
            service,
            memory,
            admission: None,
            import: None,
            new_dialog: false,
            width: 1920.,
            height: 1080.,
            ppi: 300.,
            unit: Unit::Pixels,
            space: ColorSpace::Srgb,
            transparent: true,
            adjustments: Default::default(),
            effects: Default::default(),
        }
    }
    pub fn new_document(&mut self) {
        self.name = "Documento".into();
        let mut suffix = 2;
        while self
            .directory
            .join(format!("{}.truerenderer", self.name))
            .exists()
        {
            self.name = format!("Documento {suffix}");
            suffix += 1;
        }
        self.error.clear();
        self.new_dialog = true;
    }
    pub fn has_document(&self) -> bool {
        self.doc.is_some()
    }
    pub fn import_failed(&self) -> bool {
        !self.busy() && !self.error.is_empty()
    }
    pub fn select_tab(&mut self, tab: Tab) {
        self.rgb_picker = false;
        self.end_stroke();
        self.points.clear();
        self.effects.stop_document_gesture();
        self.adjustments.paint = false;
        self.adjustments.horizon = false;
        self.adjustments.overlay = false;
        self.tab = tab;
        self.tool = Tool::Move;
    }
    pub fn begin_photo(
        &mut self,
        id: String,
        path: PathBuf,
        recipe: tr_core::editing::EditRecipe,
        expected_digest: String,
        tab: Tab,
    ) -> bool {
        if self.busy() || !self.replace_allowed() {
            return false;
        }
        self.select_tab(tab);
        if self.queue_import(path.clone(), recipe, Some(expected_digest)) {
            self.import_photo = Some(id);
            self.import_path = Some(path);
            return true;
        }
        false
    }
    fn replace_allowed(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        if !self.dirty {
            return true;
        }
        match rfd::MessageDialog::new().set_title("TrueRenderer")
            .set_description("Salvare il progetto corrente prima di aprire un altro lavoro? Sì: salva. No: sostituisci. Annulla: resta qui.")
            .set_buttons(rfd::MessageButtons::YesNoCancel).show() {
            rfd::MessageDialogResult::Yes => { self.save(false); self.reveal = None; !self.dirty },
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }
    pub fn allow_close(&mut self) -> bool {
        if self.busy() {
            self.error = "Attendere il caricamento prima di chiudere".into();
            return false;
        }
        if !self.dirty {
            return true;
        }
        match rfd::MessageDialog::new().set_title("TrueRenderer")
            .set_description("Salvare le modifiche al documento prima di chiudere? Sì: salva. No: scarta. Annulla: torna al documento.")
            .set_buttons(rfd::MessageButtons::YesNoCancel).show() {
            rfd::MessageDialogResult::Yes => { self.save(false); self.reveal = None; !self.dirty },
            rfd::MessageDialogResult::No => { self.dirty=false; true },
            _ => false,
        }
    }
    pub fn open(&mut self, path: PathBuf) -> bool {
        self.attempted = Some(path.clone());
        if !self.replace_allowed() {
            self.reveal = self.location();
            return false;
        }
        if !self.admit() {
            return false;
        }
        let required = tr_store::document::allocation(&path)
            .map(|(storage, pixels)| tr_core::document::admission_bytes(storage, pixels));
        match required {
            Ok(bytes) if self.admit_bytes(bytes) => {}
            Ok(_) => return false,
            Err(e) => {
                self.error = e.to_string();
                return false;
            }
        }
        match tr_store::document::open_admitted(&path, self.admission.as_ref().unwrap().bytes()) {
            Ok(doc) => {
                self.replace(doc);
                self.path = Some(path);
                self.dirty = false;
                self.select_tab(Tab::Layers);
                true
            }
            Err(e) => {
                self.error = e.to_string();
                false
            }
        }
    }
    fn admit(&mut self) -> bool {
        self.admit_bytes(3 * 1024 * 1024 * 1024)
    }
    fn admit_bytes(&mut self, bytes: u64) -> bool {
        let admission = self.admission.get_or_insert_with(|| {
            std::sync::Arc::new(crate::document_memory::Admission::new(self.memory.clone()))
        });
        if !admission.ensure(bytes) {
            self.error = format!(
                "Memoria insufficiente per lavorare a risoluzione nativa (richiesti {:.1} GiB). Liberare memoria e riprovare.",
                bytes as f64 / 1073741824.
            );
            false
        } else {
            true
        }
    }
    fn admit_edit(&mut self, growth: u64) -> bool {
        let Some(doc) = &self.doc else {
            return false;
        };
        let storage =
            (doc.storage_bytes() as u64 + growth).min(tr_core::document::MAX_DOCUMENT_BYTES as u64);
        self.admit_bytes(tr_core::document::admission_bytes(
            storage,
            doc.largest_pixels(),
        ))
    }
    fn replace(&mut self, doc: Document) {
        self.rgb_picker = false;
        self.return_photo = None;
        self.confirm_return = false;
        self.doc = Some(doc);
        self.export_resolution = None;
        self.export_space = None;
        self.export_edge = 0;
        self.path = None;
        self.source_id = None;
        self.source_path = None;
        self.dirty = true;
        self.undo.clear();
        self.redo.clear();
        self.selected = 0;
        self.points.clear();
        self.source = None;
        self.stroke = None;
        self.texture = None;
        self.composite = None;
        self.view_texture = None;
        self.view_region = None;
        self.needs_render = true;
        self.error.clear();
        self.notice.clear();
        self.adjustments = Default::default();
        self.effects = Default::default();
        if let Some(admission) = &self.admission
            && std::sync::Arc::strong_count(admission) == 1
        {
            let doc = self.doc.as_ref().unwrap();
            admission.shrink(tr_core::document::admission_bytes(
                doc.storage_bytes() as u64,
                doc.largest_pixels(),
            ));
        }
    }
    fn remember(&mut self, old: Document) {
        let history_bytes = self
            .doc
            .as_ref()
            .map_or(0, Document::storage_bytes)
            .max(old.storage_bytes())
            .max(256 * 1024 * 1024);
        self.redo.clear();
        self.undo.push_back(old);
        while self.undo.len() > 32
            || self.undo.iter().map(Document::storage_bytes).sum::<usize>() > history_bytes
        {
            self.undo.pop_front();
        }
        self.dirty = true;
        self.notice.clear();
        self.needs_render = true;
    }
    fn edit(&mut self, action: impl FnOnce(&mut Document, usize) -> Result<()>) {
        // Include a new selection/mask; raster insertion additionally reserves its growth.
        if !self.admit_edit(self.doc.as_ref().map_or(0, |d| d.largest_pixels() * 8)) {
            return;
        }
        let Some(doc) = &mut self.doc else {
            return;
        };
        let before = doc.clone();
        match action(doc, self.selected).and_then(|()| doc.validate()) {
            Ok(()) => {
                self.error.clear();
                if *doc != before {
                    self.remember(before);
                }
            }
            Err(e) => {
                *doc = before;
                self.error = e.to_string();
            }
        }
    }
    pub fn save(&mut self, copy: bool) {
        if self.busy() {
            return;
        }
        self.end_stroke();
        let Some(doc) = &self.doc else {
            return;
        };
        let path = if copy || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("TrueRenderer", &["truerenderer"])
                .set_directory(&self.directory)
                .set_file_name(self.suggested_name())
                .save_file()
        } else {
            self.path.clone()
        };
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("truerenderer");
            }
            let result = tr_store::document::save(&path, doc);
            self.saved(path, result, true);
        }
    }
    fn import_image(&mut self, _ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_directory(&self.directory)
            .pick_file()
        {
            self.queue_import(
                path,
                tr_core::editing::EditRecipe::neutral(Default::default()),
                None,
            );
        }
    }
    fn queue_import(
        &mut self,
        path: PathBuf,
        recipe: tr_core::editing::EditRecipe,
        expected_digest: Option<String>,
    ) -> bool {
        if self.busy() || !self.admit() {
            return false;
        }
        let Some(import_admission) = self.memory.try_reserve(3 * 1024 * 1024 * 1024) else {
            self.error = "Memoria insufficiente per decoder e snapshot di importazione.".into();
            return false;
        };
        let (tx, rx) = mpsc::channel();
        let append = expected_digest.is_none()
            && (self.doc.is_some() || self.conversion.as_ref().is_some_and(|c| c.base.is_some()));
        let job = crate::decode_pool::DocumentImport {
            path,
            recipe,
            expected_digest,
            result: tx,
            memory: import_admission,
            document: self.admission.as_ref().unwrap().clone(),
            append,
            existing_storage: self.doc.as_ref().map_or(0, |d| d.storage_bytes() as u64)
                + self
                    .conversion
                    .as_ref()
                    .and_then(|c| c.base.as_ref())
                    .map_or(0, |d| d.storage_bytes() as u64),
            existing_pixels: self.doc.as_ref().map_or(0, Document::largest_pixels).max(
                self.conversion
                    .as_ref()
                    .and_then(|c| c.base.as_ref())
                    .map_or(0, Document::largest_pixels),
            ),
        };
        match self
            .service
            .try_send(crate::service::Request::ImportDocument(job))
        {
            Ok(()) => {
                self.import = Some(rx);
                self.error.clear();
                true
            }
            Err(_) => {
                self.error = "Servizio occupato: riprovare l'importazione".into();
                false
            }
        }
    }
    pub fn poll(&mut self) {
        self.poll_publishing();
        let imported = self.import.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("Importazione interrotta")))
            }
        });
        if let Some(result) = imported {
            self.import = None;
            if self.conversion.is_some() {
                self.conversion_result(result);
                return;
            }
            match result {
                Ok(imported) => {
                    let layer = imported.layer;
                    let photo = self.import_photo.take();
                    if photo.is_some() || self.doc.is_none() {
                        let size = [layer.width, layer.height];
                        let doc = Document {
                            version: 2,
                            width: size[0],
                            height: size[1],
                            ppi: imported.resolution.map_or(300., |r| r.x),
                            resolution: imported.resolution,
                            unit: Unit::Pixels,
                            color_space: ColorSpace::Srgb,
                            layers: vec![layer],
                            selection: vec![],
                        };
                        match doc.validate() {
                            Ok(()) => {
                                self.replace(doc);
                                self.source_id = photo;
                                self.source_path = self.import_path.take();
                                self.return_photo = self.source_path.clone();
                            }
                            Err(e) => self.error = e.to_string(),
                        }
                    } else {
                        self.edit(|d, _| d.insert(layer));
                        if self.error.is_empty() {
                            self.select_layer(self.doc.as_ref().unwrap().layers.len() - 1);
                        }
                    }
                }
                Err(e) => {
                    self.import_photo = None;
                    self.import_path = None;
                    self.error = e.to_string();
                }
            }
        }
    }
    pub fn canvas(&mut self, ui: &mut egui::Ui, transform: &mut tr_core::ViewTransform) {
        if self.doc.is_none() {
            self.unavailable(ui);
            return;
        }
        self.render_texture(ui.ctx());
        ui.scope(|ui| {
            let viewport = ui.available_rect_before_wrap();
            ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
            if self.composite.is_none() {
                return;
            }
            let doc = self.doc.as_ref().unwrap();
            let dimensions = egui::vec2(doc.width as f32, doc.height as f32);
            let scale = transform
                .scale(
                    [dimensions.x, dimensions.y],
                    [viewport.width(), viewport.height()],
                    ui.ctx().pixels_per_point(),
                )
                .max(0.001);
            let image_size = dimensions * scale;
            let rect = egui::Rect::from_min_size(
                viewport.center()
                    - egui::vec2(
                        transform.center[0] * image_size.x,
                        transform.center[1] * image_size.y,
                    ),
                image_size,
            );
            let rect = if transform.zoom == Some(1.) {
                let ppp = ui.ctx().pixels_per_point();
                egui::Rect::from_min_size((rect.min * ppp).round() / ppp, rect.size())
            } else {
                rect
            };
            let response = ui.interact(
                viewport,
                ui.id().with("document-canvas"),
                egui::Sense::click_and_drag(),
            );
            self.paint_composite(ui, rect, viewport, scale);
            let mut response = response;
            response.rect = rect;
            let pan = ui.input(|i| i.key_down(egui::Key::Space) || i.pointer.middle_down());
            if response.hovered() {
                let zoom = ui.input(|i| i.zoom_delta());
                if zoom != 1. {
                    transform.set_zoom(scale * ui.ctx().pixels_per_point() * zoom);
                }
            }
            if pan {
                if response.dragged() {
                    let delta = ui.input(|i| i.pointer.delta());
                    transform.center[0] -= delta.x / image_size.x;
                    transform.center[1] -= delta.y / image_size.y;
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                self.end_stroke();
                return;
            }
            if self.busy() || self.tab == Tab::Export {
                return;
            }
            let doc = self.doc.as_ref().unwrap();
            if self.tool == Tool::Move
                && let Some(layer) = doc.layers.get(self.selected)
            {
                let size = layer
                    .recipe
                    .advanced
                    .as_ref()
                    .map_or([layer.width, layer.height], |a| {
                        a.geometry.output_size([layer.width, layer.height])
                    });
                let corners = [
                    [0., 0.],
                    [size[0] as f32, 0.],
                    [size[0] as f32, size[1] as f32],
                    [0., size[1] as f32],
                ]
                .map(|p| {
                    let p = layer.transform.forward(p);
                    response.rect.min + egui::vec2(p[0], p[1]) * scale
                });
                let painter = ui.painter().with_clip_rect(response.rect);
                for i in 0..4 {
                    painter.line_segment(
                        [corners[i], corners[(i + 1) % 4]],
                        egui::Stroke::new(1., Color32::from_rgb(80, 190, 240)),
                    );
                }
            }

            if self.photographic_interaction(ui, &response) {
                return;
            }
            let doc = self.doc.as_ref().unwrap();
            let point = response.interact_pointer_pos().map(|p| {
                [
                    (p.x - response.rect.left()) / scale,
                    (p.y - response.rect.top()) / scale,
                ]
            });
            if let Some(p) = response.hover_pos()
                && matches!(
                    self.tool,
                    Tool::Paint | Tool::Erase | Tool::Clone | Tool::Heal | Tool::AutoHeal
                )
            {
                let painter = ui.painter().with_clip_rect(response.rect);
                painter.circle_stroke(
                    p,
                    self.radius * scale,
                    egui::Stroke::new(1., Color32::WHITE),
                );
                painter.circle_stroke(
                    p,
                    (self.radius * self.hardness) * scale,
                    egui::Stroke::new(1., Color32::BLACK),
                );
            }
            if response.clicked()
                && let Some(p) = point
            {
                if ui.input(|i| i.modifiers.alt) {
                    if let Some(l) = doc.layers.get(self.selected) {
                        self.source = l.source_point(p);
                    }
                    return;
                }
                if self.tool == Tool::Polygon {
                    if self.points.len() < 4096 {
                        self.points.push(p);
                    }
                    if response.double_clicked() && self.points.len() >= 3 {
                        self.finish_selection();
                    }
                } else if !matches!(self.tool, Tool::Free | Tool::Rectangle | Tool::Move)
                    && self.begin_stroke()
                {
                    self.paint_at(p);
                    self.end_stroke();
                }
            }
            if response.drag_started() {
                if !self.begin_stroke() {
                    return;
                }
                self.last_point = ui
                    .input(|i| i.pointer.press_origin())
                    .map(|p| {
                        [
                            (p.x - response.rect.left()) / scale,
                            (p.y - response.rect.top()) / scale,
                        ]
                    })
                    .or(point);
                if matches!(self.tool, Tool::Free | Tool::Rectangle) {
                    self.points.clear();
                    if let Some(p) = self.last_point {
                        self.points.push(p);
                    }
                } else if let Some(p) = self.last_point {
                    self.paint_at(p);
                }
            }
            if response.dragged()
                && self.stroke.is_some()
                && let Some(p) = point
            {
                if self.tool == Tool::Move {
                    if let Some(last) = self.last_point
                        && let Some(l) = self.doc.as_mut().unwrap().layers.get_mut(self.selected)
                        && !l.locked
                    {
                        l.transform.x += p[0] - last[0];
                        l.transform.y += p[1] - last[1];
                        self.needs_render = true;
                    }
                } else if matches!(self.tool, Tool::Free | Tool::Rectangle) {
                    if self.points.len() < 4096 {
                        if self.tool == Tool::Rectangle && self.points.len() > 1 {
                            self.points[1] = p;
                        } else {
                            self.points.push(p);
                        }
                    }
                } else {
                    let from = self.last_point.unwrap_or(p);
                    let distance = (p[0] - from[0]).hypot(p[1] - from[1]);
                    let steps = (distance / (self.radius * 0.2).max(0.5))
                        .ceil()
                        .clamp(1., 4096.) as usize;
                    for i in 1..=steps {
                        let t = i as f32 / steps as f32;
                        self.paint_at([
                            from[0] + t * (p[0] - from[0]),
                            from[1] + t * (p[1] - from[1]),
                        ]);
                    }
                }
                self.last_point = Some(p);
            }
            if response.drag_stopped() {
                if matches!(self.tool, Tool::Free | Tool::Rectangle) {
                    if self.tool == Tool::Rectangle && self.points.len() == 2 {
                        let a = self.points[0];
                        let b = self.points[1];
                        self.points = vec![a, [b[0], a[1]], b, [a[0], b[1]]];
                    }
                    self.stroke = None;
                    self.finish_selection();
                } else {
                    self.end_stroke();
                }
            }
            for pair in self.points.windows(2) {
                ui.painter().line_segment(
                    [pair[0], pair[1]].map(|p| response.rect.min + egui::vec2(p[0], p[1]) * scale),
                    egui::Stroke::new(1., Color32::YELLOW),
                );
            }
        });
    }
    fn begin_stroke(&mut self) -> bool {
        if !self.admit_edit(0) {
            return false;
        }
        self.stroke = self.doc.clone();
        self.stroke_origin = None;
        self.stroke.is_some()
    }
    fn end_stroke(&mut self) {
        if let Some(old) = self.stroke.take()
            && self.doc.as_ref() != Some(&old)
        {
            if let Some(Err(error)) = self.doc.as_ref().map(Document::validate) {
                self.doc = Some(old);
                self.error = error.to_string();
                self.needs_render = true;
            } else {
                self.remember(old);
            }
        }
        self.last_point = None;
    }
    fn finish_selection(&mut self) {
        let points = std::mem::take(&mut self.points);
        if points.len() < 3 {
            return;
        }
        let mode = self.selection_mode;
        let feather = self.feather;
        self.edit(|d, _| d.select_polygon(&points, mode, feather));
    }
    fn paint_at(&mut self, p: [f32; 2]) {
        if self.stroke.is_none() {
            return;
        }
        let brush = match self.tool {
            Tool::Paint => Brush::Paint(tr_core::color::from_encoded_srgb(self.color)),
            Tool::Erase => Brush::Erase,
            Tool::Clone => Brush::Clone,
            Tool::Heal => Brush::Heal,
            Tool::AutoHeal => Brush::AutoHeal,
            _ => return,
        };
        let origin = *self.stroke_origin.get_or_insert(p);
        let source = self
            .stroke
            .as_ref()
            .and_then(|d| d.layers.get(self.selected));
        let offset = self.source.zip(source).and_then(|(s, l)| {
            let anchor = l.source_point(origin)?;
            Some([s[0] - anchor[0], s[1] - anchor[1]])
        });
        let source = source.zip(if brush == Brush::AutoHeal {
            Some([0.; 2])
        } else {
            offset
        });
        if let Some(d) = &mut self.doc {
            match d.dab(
                self.selected,
                p,
                self.radius,
                self.hardness,
                self.strength,
                brush,
                source,
            ) {
                Ok(()) => {
                    self.needs_render = true;
                    self.error.clear();
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }
    fn photographic_interaction(&mut self, ui: &egui::Ui, response: &egui::Response) -> bool {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.rgb_picker = false;
            if let Some(old) = self.stroke.take() {
                self.doc = Some(old);
                self.needs_render = true;
            }
            self.points.clear();
            self.last_point = None;
            self.effects.stop_document_gesture();
            self.adjustments.paint = false;
            self.adjustments.horizon = false;
            self.adjustments.overlay = false;
            return true;
        }
        let Some(doc) = &self.doc else {
            return false;
        };
        let Some(layer) = doc.layers.get(self.selected) else {
            return false;
        };
        let before = layer.recipe.clone();
        let mut recipe = before.clone();
        let map = CanvasCoordinates {
            layer,
            rect: response.rect,
            size: [doc.width, doc.height],
        };
        if self.rgb_picker {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if !layer.locked
                && response.clicked()
                && let Some(p) = response
                    .interact_pointer_pos()
                    .and_then(|p| map.source_uv(p))
            {
                let result = develop::sampled_white_balance(layer, p);
                match result {
                    Ok(recipe) => {
                        self.rgb_picker = false;
                        self.edit(|d, i| {
                            d.layers[i].recipe = recipe;
                            Ok(())
                        });
                    }
                    Err(error) => self.error = error.to_string(),
                }
            }
            return true;
        }
        let effects_active = self
            .effects
            .document_canvas(ui, response, &map, &mut recipe);
        let active = effects_active
            || self
                .adjustments
                .document_canvas(ui, response, &map, &mut recipe);
        if active && (response.clicked() || response.drag_started()) {
            self.begin_stroke();
        }
        if recipe != before {
            if self.stroke.is_some() {
                match recipe.validate() {
                    Ok(()) => {
                        self.doc.as_mut().unwrap().layers[self.selected].recipe = recipe;
                        self.needs_render = true;
                    }
                    Err(e) => self.error = e.to_string(),
                }
            } else {
                self.edit(|d, i| {
                    d.layers[i].recipe = recipe;
                    Ok(())
                });
            }
        }
        if active && (response.clicked() || response.drag_stopped()) {
            self.end_stroke();
        }
        active
    }
}

#[cfg(test)]
fn encode_png(doc: &Document, output: &mut impl std::io::Write) -> Result<()> {
    export::encode_image(doc, export::ImageFormat::Png16, 95, 0, output)
}
fn encode_png_image(
    image: &LinearImage,
    space: ColorSpace,
    resolution: Option<tr_core::protocol::Resolution>,
    eight_bit: bool,
    output: &mut impl std::io::Write,
) -> Result<()> {
    let mut pixels = Vec::with_capacity(image.pixels.len() * 8);
    for p in &image.pixels {
        let encoded = space.rgba16(*p).0;
        for channel in encoded {
            if eight_bit {
                pixels.push(((channel as u32 + 128) / 257) as u8);
            } else {
                pixels.extend_from_slice(&channel.to_be_bytes());
            }
        }
    }
    {
        let mut info = png::Info::with_size(image.width, image.height);
        if space != ColorSpace::Srgb {
            info.icc_profile = Some(std::borrow::Cow::Owned(space.icc()?));
        }
        let mut encoder = png::Encoder::with_info(output, info)?;
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(if eight_bit {
            png::BitDepth::Eight
        } else {
            png::BitDepth::Sixteen
        });
        match space {
            ColorSpace::AdobeRgb | ColorSpace::DisplayP3 => {}
            ColorSpace::Srgb => {
                encoder.set_source_srgb(png::SrgbRenderingIntent::RelativeColorimetric)
            }
            ColorSpace::LinearRec2020 => {
                encoder.set_source_gamma(png::ScaledFloat::new(1.));
                encoder.set_source_chromaticities(png::SourceChromaticities::new(
                    (0.3127, 0.3290),
                    (0.708, 0.292),
                    (0.170, 0.797),
                    (0.131, 0.046),
                ));
            }
        }
        if let Some(r) = resolution {
            encoder.set_pixel_dims(Some(png::PixelDimensions {
                xppu: (r.x / 0.0254).round() as u32,
                yppu: (r.y / 0.0254).round() as u32,
                unit: png::Unit::Meter,
            }));
        }
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&pixels)?;
        writer.finish()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photographic_mask_drag_on_document_is_editable_and_one_undo() {
        use tr_core::editing::masks::{Mask, Shape};
        let mut doc = Document::new(64, 64, 300., [0.2, 0.3, 0.4, 1.]).unwrap();
        doc.layers[0]
            .recipe
            .advanced
            .get_or_insert_with(Default::default)
            .masks
            .push(Mask {
                shape: Shape::Brush,
                exposure: 1.,
                ..Default::default()
            });
        doc.layers[0].recipe.require_process(3);
        let original = doc.clone();
        let mut editor = Editor::new(
            mpsc::sync_channel(1).0,
            tr_core::budget::MemoryBudget::new(4 * 1024 * 1024 * 1024),
        );
        editor.replace(doc);
        editor.adjustments.paint = true;
        let ctx = egui::Context::default();
        let p = egui::pos2(30., 30.);
        let q = egui::pos2(90., 90.);
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            vec![egui::Event::PointerMoved(egui::pos2(50., 50.))],
            vec![egui::Event::PointerMoved(q)],
            vec![egui::Event::PointerButton {
                pos: q,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400., 300.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200., 200.)),
                        egui::Id::new("mask-canvas"),
                        egui::Sense::click_and_drag(),
                    );
                    editor.photographic_interaction(ui, &response);
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(editor.undo.len(), 1);
        assert_eq!(editor.undo[0], original);
        let doc = editor.doc.as_ref().unwrap();
        assert_eq!(doc.layers[0].pixels, original.layers[0].pixels);
        let mask = &doc.layers[0].recipe.advanced.as_ref().unwrap().masks[0];
        assert!(mask.points.len() >= 2);
        assert!((mask.points[0][0] - 0.15).abs() < 0.0001);
        doc.validate().unwrap();
    }
    #[test]
    fn png_export_declares_profile_ppi_and_preserves_alpha() {
        for space in ColorSpace::ALL {
            let mut doc = Document::new(2, 1, 300., [0.25, 0.125, 0., 0.5]).unwrap();
            doc.color_space = space;
            let mut bytes = vec![];
            encode_png(&doc, &mut bytes).unwrap();
            let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
                .read_info()
                .unwrap();
            assert_eq!(reader.info().pixel_dims.unwrap().xppu, 11811);
            assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
            match space {
                ColorSpace::AdobeRgb | ColorSpace::DisplayP3 => {
                    assert!(reader.info().icc_profile.is_some())
                }
                ColorSpace::Srgb => assert!(reader.info().srgb.is_some()),
                ColorSpace::LinearRec2020 => {
                    assert_eq!(reader.info().gama_chunk, Some(png::ScaledFloat::new(1.)));
                    assert!(reader.info().chrm_chunk.is_some());
                }
            }
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut pixels).unwrap();
            assert_eq!(u16::from_be_bytes([pixels[6], pixels[7]]), 32768);
            assert_eq!(doc.layers[0].pixels[0], [0.25, 0.125, 0., 0.5]);
        }
    }
    #[test]
    fn ordinary_canvas_preserves_document_and_stroke_is_one_undo() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(
            mpsc::sync_channel(1).0,
            tr_core::budget::MemoryBudget::new(4 * 1024 * 1024 * 1024),
        );
        let original = Document::new(32, 24, 300., [0.2, 0.3, 0.4, 1.]).unwrap();
        editor.replace(original.clone());
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200., 800.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    editor.poll();
                    egui::Panel::right("test-inspector")
                        .show(ui, |ui| editor.inspector_contents(ui));
                    egui::CentralPanel::default()
                        .show(ui, |ui| editor.canvas(ui, &mut Default::default()));
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(editor.doc.as_ref(), Some(&original));
        assert!(editor.undo.is_empty());
        editor.tool = Tool::Erase;
        editor.begin_stroke();
        editor.paint_at([10., 10.]);
        editor.paint_at([11., 10.]);
        editor.end_stroke();
        assert_eq!(editor.undo.len(), 1);
        assert_eq!(editor.undo[0], original);
        assert_ne!(editor.doc.as_ref(), Some(&original));
        let revised = editor.doc.clone();
        editor.edit(|d, _| {
            d.ppi = 0.;
            Ok(())
        });
        assert_eq!(editor.doc, revised);
    }
}
