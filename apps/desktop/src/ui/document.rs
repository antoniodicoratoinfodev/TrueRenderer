//! Raster document workspace, sharing the application's isolated decoder.
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

type Imported = Result<RasterLayer>;
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
    pub active: bool,
    doc: Option<Document>,
    path: Option<PathBuf>,
    dirty: bool,
    undo: VecDeque<Document>,
    redo: VecDeque<Document>,
    selected: usize,
    tool: Tool,
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
    needs_render: bool,
    error: String,
    service: mpsc::SyncSender<crate::service::Request>,
    memory: tr_core::budget::MemoryBudget,
    admission: Option<std::sync::Arc<tr_core::budget::Lease>>,
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
            active: false,
            doc: None,
            path: None,
            dirty: false,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            selected: 0,
            tool: Tool::Move,
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
    fn discard_allowed(&self) -> bool {
        if self.import.is_some() {
            return false;
        }
        !self.dirty
            || rfd::MessageDialog::new()
                .set_title("TrueRenderer")
                .set_description("Scartare le modifiche non salvate al documento?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }
    pub fn allow_close(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        match rfd::MessageDialog::new().set_title("TrueRenderer")
            .set_description("Salvare le modifiche al documento prima di chiudere? Sì: salva. No: scarta. Annulla: torna al documento.")
            .set_buttons(rfd::MessageButtons::YesNoCancel).show() {
            rfd::MessageDialogResult::Yes => { self.save(false); !self.dirty },
            rfd::MessageDialogResult::No => { self.dirty=false; true },
            _ => false,
        }
    }
    pub fn open(&mut self, path: PathBuf) {
        if !self.admit() {
            return;
        }
        if !self.discard_allowed() {
            return;
        }
        match tr_store::document::open(&path) {
            Ok(doc) => {
                self.replace(doc);
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => {
                self.error = e.to_string();
                self.active = true;
            }
        }
    }
    fn admit(&mut self) -> bool {
        if self.admission.is_none() {
            // Conservative envelope: durable raster, two history branches,
            // pre-stroke snapshot, render/filter scratch, display and file IO.
            self.admission = self
                .memory
                .try_reserve(3 * 1024 * 1024 * 1024)
                .map(std::sync::Arc::new);
        }
        if self.admission.is_none() {
            self.error = "Memoria insufficiente per aprire lo spazio documento; liberare le anteprime e riprovare.".into();
            self.active = true;
            false
        } else {
            true
        }
    }
    fn replace(&mut self, doc: Document) {
        self.doc = Some(doc);
        self.path = None;
        self.dirty = true;
        self.undo.clear();
        self.redo.clear();
        self.selected = 0;
        self.points.clear();
        self.source = None;
        self.stroke = None;
        self.texture = None;
        self.needs_render = true;
        self.active = true;
        self.error.clear();
        self.adjustments = Default::default();
        self.effects = Default::default();
    }
    pub fn capture_photo(
        &mut self,
        name: String,
        image: &LinearImage,
        recipe: tr_core::editing::EditRecipe,
    ) -> Result<()> {
        ensure!(self.discard_allowed(), "Creazione documento annullata");
        ensure!(self.admit(), "Memoria documento non disponibile");
        ensure!(
            image.pixels.len() <= tr_core::document::MAX_CANVAS_PIXELS,
            "Foto oltre il limite iniziale di 16 MP"
        );
        let mut layer = RasterLayer::new(name, image.clone());
        layer.recipe = recipe;
        let document = Document {
            version: 1,
            width: image.width,
            height: image.height,
            ppi: 300.,
            unit: Unit::Pixels,
            color_space: ColorSpace::Srgb,
            layers: vec![layer],
            selection: vec![],
        };
        document.validate()?;
        self.replace(document);
        Ok(())
    }

    fn remember(&mut self, old: Document) {
        const HISTORY_BYTES: usize = 256 * 1024 * 1024;
        self.redo.clear();
        self.undo.push_back(old);
        while self.undo.len() > 32
            || self.undo.iter().map(Document::storage_bytes).sum::<usize>() > HISTORY_BYTES
        {
            self.undo.pop_front();
        }
        self.dirty = true;
        self.needs_render = true;
    }
    fn edit(&mut self, action: impl FnOnce(&mut Document, usize) -> Result<()>) {
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
    fn save(&mut self, copy: bool) {
        let Some(doc) = &self.doc else {
            return;
        };
        let path = if copy || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("TrueRenderer", &["truerenderer"])
                .set_file_name("Documento.truerenderer")
                .save_file()
        } else {
            self.path.clone()
        };
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("truerenderer");
            }
            match tr_store::document::save(&path, doc) {
                Ok(()) => {
                    self.path = Some(path);
                    self.dirty = false;
                    self.error.clear();
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }
    fn import_image(&mut self, _ctx: &egui::Context) {
        if !self.admit() {
            return;
        }
        if self.import.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let Some(import_admission) = self.memory.try_reserve(3 * 1024 * 1024 * 1024) else {
            self.error = "Memoria insufficiente per decoder e snapshot di importazione.".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        let job = crate::decode_pool::DocumentImport {
            path,
            result: tx,
            memory: import_admission,
            document: self.admission.as_ref().unwrap().clone(),
        };
        match self
            .service
            .try_send(crate::service::Request::ImportDocument(job))
        {
            Ok(()) => self.import = Some(rx),
            Err(_) => self.error = "Servizio occupato: riprovare l'importazione".into(),
        }
    }
    fn export_png(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG 16 bit", &["png"])
            .save_file()
        else {
            return;
        };
        let result = (|| -> Result<()> {
            ensure!(
                !path.exists(),
                "Per preservare gli originali scegliere un nuovo nome per l'export"
            );
            let parent = path.parent().unwrap_or(std::path::Path::new("."));
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            encode_png(doc, temp.as_file_mut())?;
            temp.as_file().sync_all()?;
            temp.persist_noclobber(path).map_err(|e| e.error)?;
            Ok(())
        })();
        if let Err(e) = result {
            self.error = e.to_string();
        }
    }
    fn export_native(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("TrueRenderer", &["truerenderer"])
            .set_file_name("Copia.truerenderer")
            .save_file()
            && let Err(error) = tr_store::document::save(&path, doc)
        {
            self.error = error.to_string();
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if ui.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            )
        }) {
            self.save(true);
        } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save(false);
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
            self.new_dialog = true;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
            && let Some(p) = rfd::FileDialog::new()
                .add_filter("TrueRenderer", &["truerenderer"])
                .pick_file()
        {
            self.open(p);
        }
        let imported = self.import.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("Importazione interrotta")))
            }
        });
        if let Some(result) = imported {
            self.import = None;
            match result {
                Ok(layer) => {
                    if self.doc.is_none() {
                        match Document::new(layer.width, layer.height, 300., [0.; 4]) {
                            Ok(mut d) => {
                                d.layers.clear();
                                self.replace(d);
                            }
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                    self.edit(|d, _| d.insert(layer));
                    if let Some(doc) = &self.doc {
                        self.selected = doc.layers.len().saturating_sub(1);
                    }
                }
                Err(e) => self.error = e.to_string(),
            }
        }
        egui::Panel::top("document-actions").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("TrueRenderer · Documento");
                if ui.button("Fotografie").clicked() {
                    self.active = false;
                }
                if ui.button("Chiudi documento").clicked() && self.discard_allowed() {
                    self.doc = None;
                    self.path = None;
                    self.dirty = false;
                    self.undo.clear();
                    self.redo.clear();
                    self.stroke = None;
                    self.texture = None;
                    self.admission = None;
                    self.active = false;
                }
                if ui.button("Nuovo…").clicked() {
                    self.new_dialog = true;
                }
                if ui.button("Apri…").clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter("TrueRenderer", &["truerenderer"])
                        .pick_file()
                {
                    self.open(p);
                }
                if ui.button("Salva").clicked() {
                    self.save(false);
                }
                if ui.button("Salva con nome…").clicked() {
                    self.save(true);
                }
                if ui.button("Export PNG16…").clicked() {
                    self.export_png();
                }
                if ui.button("Export documento…").clicked() {
                    self.export_native();
                }
                if ui
                    .add_enabled(
                        self.import.is_none(),
                        egui::Button::new("Inserisci immagine…"),
                    )
                    .clicked()
                {
                    self.import_image(&ctx);
                }
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("Annulla"))
                    .clicked()
                    && let Some(old) = self.undo.pop_back()
                {
                    if let Some(doc) = self.doc.replace(old) {
                        self.redo.push_back(doc);
                    }
                    self.dirty = true;
                    self.needs_render = true;
                }
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Ripeti"))
                    .clicked()
                    && let Some(next) = self.redo.pop_back()
                {
                    if let Some(doc) = self.doc.replace(next) {
                        self.undo.push_back(doc);
                    }
                    self.dirty = true;
                    self.needs_render = true;
                }
                ui.label(if self.dirty {
                    "Modifiche non salvate"
                } else {
                    "Salvato"
                });
            });
            if !self.error.is_empty() {
                ui.colored_label(Color32::YELLOW, &self.error);
            }
            if self.import.is_some() {
                ui.spinner();
                ui.label("Decodifica immagine…");
            }
        });
        if self.new_dialog {
            egui::Window::new("Nuovo documento").collapsible(false).show(&ctx,|ui|{
                ui.horizontal(|ui|{ui.label("Larghezza");ui.add(egui::DragValue::new(&mut self.width).range(0.01..=65536.));});
                ui.horizontal(|ui|{ui.label("Altezza");ui.add(egui::DragValue::new(&mut self.height).range(0.01..=65536.));});
                ui.horizontal(|ui|{for (unit,label) in [(Unit::Pixels,"px"),(Unit::Centimeters,"cm"),(Unit::Millimeters,"mm"),(Unit::Inches,"pollici")]{ui.selectable_value(&mut self.unit,unit,label);}});
                ui.horizontal(|ui|{ui.label("PPI");ui.add(egui::DragValue::new(&mut self.ppi).range(1. ..=9600.));});
                if ui.button("Scambia orientamento").clicked(){std::mem::swap(&mut self.width,&mut self.height);}
                ui.checkbox(&mut self.transparent,"Sfondo trasparente");
                if !self.transparent {ui.color_edit_button_rgba_unmultiplied(&mut self.color);}
                ui.horizontal(|ui|{ui.label("Spazio di uscita");ui.selectable_value(&mut self.space,ColorSpace::Srgb,"sRGB");ui.selectable_value(&mut self.space,ColorSpace::LinearRec2020,"Rec.2020 lineare");});
                ui.small("Calcolo interno Rec.2020 lineare. Limite iniziale: 16 MP per tela, 256 MiB di pixel e maschere.");
                ui.horizontal(|ui|{
                    if ui.button("Crea").clicked() && self.discard_allowed() && self.admit(){
                        let result=(||->Result<Document>{let w=self.unit.pixels(self.width,self.ppi)?;let h=self.unit.pixels(self.height,self.ppi)?;
                            let mut d=Document::new(w,h,self.ppi,if self.transparent{[0.;4]}else{tr_core::color::from_encoded_srgb(self.color)})?;
                            d.unit=self.unit;d.color_space=self.space;Ok(d)})();
                        match result{Ok(d)=>{self.replace(d);self.new_dialog=false;},Err(e)=>self.error=e.to_string()}
                    }
                    if ui.button("Annulla").clicked(){self.new_dialog=false;}
                });
            });
        }
        let Some(doc) = &self.doc else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("Crea un documento o inserisci un'immagine.");
            });
            return;
        };
        self.selected = self.selected.min(doc.layers.len().saturating_sub(1));
        egui::Panel::right("document-layers")
            .default_size(280.)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Livelli");
                    let layers = self
                        .doc
                        .as_ref()
                        .unwrap()
                        .layers
                        .iter()
                        .map(|l| (l.name.clone(), l.visible))
                        .collect::<Vec<_>>();
                    for (i, (name, mut visible)) in layers.into_iter().enumerate().rev() {
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut visible, "").changed() {
                                self.edit(|d, _| {
                                    d.layers[i].visible = visible;
                                    Ok(())
                                });
                            }
                            if ui.selectable_label(self.selected == i, name).clicked() {
                                self.selected = i;
                                self.source = None;
                                self.adjustments = Default::default();
                                self.effects = Default::default();
                            }
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Vuoto").clicked() {
                            self.edit(|d, _| {
                                let image = LinearImage::new(
                                    d.width,
                                    d.height,
                                    vec![[0.; 4]; d.width as usize * d.height as usize],
                                )?;
                                d.insert(RasterLayer::new("Livello vuoto".into(), image))
                            });
                            self.selected =
                                self.doc.as_ref().unwrap().layers.len().saturating_sub(1);
                        }
                        if ui.button("Copia livello").clicked() {
                            self.edit(|d, i| d.duplicate(i));
                            self.selected =
                                self.doc.as_ref().unwrap().layers.len().saturating_sub(1);
                        }
                        if ui.button("Copia visibile").clicked() {
                            self.edit(|d, _| {
                                let image = d.render()?;
                                d.insert(RasterLayer::new("Copia visibile".into(), image))
                            });
                            self.selected =
                                self.doc.as_ref().unwrap().layers.len().saturating_sub(1);
                        }
                        if ui.button("Elimina").clicked() {
                            self.edit(|d, i| {
                                if i < d.layers.len() {
                                    ensure!(!d.layers[i].locked, "Livello bloccato");
                                    d.layers.remove(i);
                                }
                                Ok(())
                            });
                        }
                        if ui.button("Su").clicked() {
                            let target = (self.selected + 1)
                                .min(self.doc.as_ref().unwrap().layers.len().saturating_sub(1));
                            self.edit(|d, i| {
                                ensure!(
                                    d.layers.get(i).is_some_and(|l| !l.locked),
                                    "Livello assente o bloccato"
                                );
                                d.layers.swap(i, target);
                                Ok(())
                            });
                            if self.error.is_empty() {
                                self.selected = target;
                            }
                        }
                        if ui.button("Giù").clicked() {
                            let target = self.selected.saturating_sub(1);
                            self.edit(|d, i| {
                                ensure!(
                                    d.layers.get(i).is_some_and(|l| !l.locked),
                                    "Livello assente o bloccato"
                                );
                                d.layers.swap(i, target);
                                Ok(())
                            });
                            if self.error.is_empty() {
                                self.selected = target;
                            }
                        }
                    });
                    if let Some(layer) = self.doc.as_ref().unwrap().layers.get(self.selected) {
                        let mut name = layer.name.clone();
                        let mut locked = layer.locked;
                        let mut opacity = layer.opacity;
                        let mut blend = layer.blend;
                        let mut transform = layer.transform;
                        let mut exposure = layer.recipe.exposure_ev;
                        let mut changed = ui.text_edit_singleline(&mut name).changed();
                        changed |= ui.checkbox(&mut locked, "Blocca").changed();
                        ui.add_enabled_ui(!locked, |ui| {
                            changed |= ui
                                .add(egui::Slider::new(&mut opacity, 0. ..=1.).text("Opacità"))
                                .changed();
                            ui.horizontal(|ui| {
                                for (b, label) in [
                                    (Blend::Normal, "Normale"),
                                    (Blend::Multiply, "Moltiplica"),
                                    (Blend::Screen, "Scolora"),
                                ] {
                                    changed |= ui.selectable_value(&mut blend, b, label).changed();
                                }
                            });
                            for (label, v) in [
                                ("X", &mut transform.x),
                                ("Y", &mut transform.y),
                                ("Scala", &mut transform.scale),
                                ("Rotazione °", &mut transform.angle),
                            ] {
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    changed |= ui.add(egui::DragValue::new(v).speed(0.1)).changed();
                                });
                            }
                            changed |= ui
                                .add_enabled(
                                    !layer.recipe.suspended.contains_key("light-panel"),
                                    egui::Slider::new(&mut exposure, -10. ..=10.)
                                        .text("Esposizione EV"),
                                )
                                .changed();
                        });
                        if changed {
                            self.edit(|d, i| {
                                let l = &mut d.layers[i];
                                l.name = name;
                                l.locked = locked;
                                l.opacity = opacity;
                                l.blend = blend;
                                l.transform = transform;
                                l.recipe.exposure_ev = exposure;
                                Ok(())
                            });
                        }
                    }
                    if let Some(layer) = self.doc.as_ref().unwrap().layers.get(self.selected) {
                        let locked = layer.locked;
                        let original = layer.recipe.clone();
                        let (mut recipe, mut switches) = match original.expanded_sections() {
                            Ok(v) => v,
                            Err(e) => {
                                self.error = e.to_string();
                                return;
                            }
                        };
                        ui.add_enabled_ui(!locked, |ui| {
                            ui.collapsing("Sviluppo del livello raster", |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for (id, name) in [
                                        ("light-panel", "Luce"),
                                        ("rgb-wb-panel", "WB RGB"),
                                        ("curve-panel", "Curva"),
                                        ("color-panel", "Colore"),
                                    ] {
                                        let mut enabled = !switches.contains(id);
                                        if ui.toggle_value(&mut enabled, name).changed() {
                                            if enabled {
                                                switches.remove(id);
                                            } else {
                                                switches.insert(id.into());
                                            }
                                        }
                                    }
                                });
                                for (name, value) in [
                                    ("Luminosità", &mut recipe.brightness),
                                    ("Contrasto", &mut recipe.contrast),
                                    ("Alte luci", &mut recipe.highlights),
                                    ("Ombre", &mut recipe.shadows),
                                    ("Bianchi", &mut recipe.whites),
                                    ("Neri", &mut recipe.blacks),
                                    ("Temperatura RGB", &mut recipe.temperature),
                                    ("Tinta RGB", &mut recipe.tint),
                                    ("Saturazione", &mut recipe.saturation),
                                ] {
                                    let group = match name {
                                        "Temperatura RGB" | "Tinta RGB" => "rgb-wb-panel",
                                        "Saturazione" => "color-panel",
                                        _ => "light-panel",
                                    };
                                    ui.add_enabled(
                                        !switches.contains(group),
                                        egui::Slider::new(value, -100. ..=100.).text(name),
                                    );
                                }
                                ui.add_enabled_ui(!switches.contains("curve-panel"), |ui| {
                                    let (changed, _) = super::editing::curve::controls(
                                        ui,
                                        crate::i18n::Language::Italian,
                                        &mut recipe.curve,
                                    );
                                    if changed {
                                        recipe.require_process(4);
                                    }
                                });
                                self.adjustments.show_with_switches(
                                    ui,
                                    crate::i18n::Language::Italian,
                                    &mut recipe,
                                    None,
                                    &mut switches,
                                );
                            });
                            ui.collapsing("Regolazioni e maschere fotografiche", |ui| {
                                self.effects.bind(&format!("document:{}", self.selected));
                                if self.effects.view_tools {
                                    self.effects.show_tools(
                                        ui,
                                        crate::i18n::Language::Italian,
                                        &mut recipe,
                                    );
                                } else {
                                    self.effects.view_layers = true;
                                    self.effects.show(
                                        ui,
                                        crate::i18n::Language::Italian,
                                        &mut recipe,
                                    );
                                }
                            });
                        });
                        for id in switches {
                            if let Err(e) = recipe.set_section_enabled(&id, false) {
                                self.error = e.to_string();
                                return;
                            }
                        }
                        if recipe != original {
                            self.edit(|d, i| {
                                d.layers[i].recipe = recipe;
                                Ok(())
                            });
                        }
                    }
                    ui.separator();
                    ui.heading("Strumenti");
                    ui.horizontal_wrapped(|ui| {
                        for (tool, name) in [
                            (Tool::Move, "Sposta"),
                            (Tool::Paint, "Pennello"),
                            (Tool::Erase, "Cancella"),
                            (Tool::Clone, "Clona"),
                            (Tool::Heal, "Ripara"),
                            (Tool::AutoHeal, "Ripara rapido"),
                            (Tool::Free, "Libera"),
                            (Tool::Polygon, "Poligonale"),
                            (Tool::Rectangle, "Rettangolo"),
                        ] {
                            if ui.selectable_value(&mut self.tool, tool, name).clicked() {
                                self.points.clear();
                                self.effects.stop_document_gesture();
                                self.adjustments.paint = false;
                                self.adjustments.horizon = false;
                                self.adjustments.overlay = false;
                            }
                        }
                    });
                    ui.add(egui::Slider::new(&mut self.radius, 0.5..=512.).text("Raggio px"));
                    ui.add(egui::Slider::new(&mut self.hardness, 0. ..=1.).text("Durezza"));
                    ui.add(egui::Slider::new(&mut self.strength, 0. ..=1.).text("Intensità"));
                    ui.color_edit_button_rgba_unmultiplied(&mut self.color);
                    ui.small(
                        "Clona/Ripara: Alt+clic campiona dal livello selezionato prima del tratto.",
                    );
                    ui.horizontal_wrapped(|ui| {
                        for (mode, label) in [
                            (SelectionMode::Replace, "Sostituisci"),
                            (SelectionMode::Add, "Aggiungi"),
                            (SelectionMode::Subtract, "Sottrai"),
                            (SelectionMode::Intersect, "Interseca"),
                        ] {
                            ui.selectable_value(&mut self.selection_mode, mode, label);
                        }
                    });
                    ui.add(egui::Slider::new(&mut self.feather, 0. ..=100.).text("Sfumatura px"));
                    if ui.button("Deseleziona").clicked() {
                        self.edit(|d, _| {
                            d.selection.clear();
                            Ok(())
                        });
                    }
                    if ui.button("Selezione → maschera livello").clicked() {
                        self.edit(|d, i| {
                            let l = d
                                .layers
                                .get_mut(i)
                                .ok_or_else(|| anyhow::anyhow!("Livello mancante"))?;
                            ensure!(!l.locked, "Livello bloccato");
                            l.mask = if d.selection.is_empty() {
                                vec![]
                            } else {
                                (0..l.pixels.len())
                                    .map(|i| {
                                        let Some(p) = l.canvas_point([
                                            (i % l.width as usize) as f32 + 0.5,
                                            (i / l.width as usize) as f32 + 0.5,
                                        ]) else {
                                            return 0.;
                                        };
                                        let x = p[0].floor() as i64;
                                        let y = p[1].floor() as i64;
                                        if x < 0
                                            || y < 0
                                            || x >= d.width as i64
                                            || y >= d.height as i64
                                        {
                                            0.
                                        } else {
                                            d.selection[y as usize * d.width as usize + x as usize]
                                        }
                                    })
                                    .collect()
                            };
                            Ok(())
                        });
                    }
                });
            });
        self.canvas(ui);
    }
    fn canvas(&mut self, ui: &mut egui::Ui) {
        if self.needs_render {
            self.effects.invalidate_document_input();
            match self.doc.as_ref().unwrap().render() {
                Ok(image) => {
                    let image = image.reduced(2048);
                    let size = [image.width as usize, image.height as usize];
                    let doc = self.doc.as_ref().unwrap();
                    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
                    for (i, pixel) in image.pixels.iter().enumerate() {
                        let x = i % size[0];
                        let y = i / size[0];
                        let background = if ((x / 12) + (y / 12)).is_multiple_of(2) {
                            0.16
                        } else {
                            0.24
                        };
                        let mut rgba = tr_core::color::display_pixel(*pixel, background);
                        if !doc.selection.is_empty() {
                            let sx = x * doc.width as usize / size[0];
                            let sy = y * doc.height as usize / size[1];
                            let index = sy * doc.width as usize + sx;
                            let selected = doc.selection[index] >= 0.5;
                            let left = sx.saturating_sub(1);
                            let above = sy.saturating_sub(1);
                            if selected
                                && (sx == 0
                                    || sy == 0
                                    || doc.selection[sy * doc.width as usize + left] < 0.5
                                    || doc.selection[above * doc.width as usize + sx] < 0.5)
                                || !selected
                                    && (doc.selection[sy * doc.width as usize + left] >= 0.5
                                        || doc.selection[above * doc.width as usize + sx] >= 0.5)
                            {
                                rgba = [255, 210, 0, 255];
                            }
                        }
                        bytes.extend_from_slice(&rgba);
                    }
                    self.texture = Some(ui.ctx().load_texture(
                        "document",
                        egui::ColorImage::from_rgba_unmultiplied(size, &bytes),
                        egui::TextureOptions::LINEAR,
                    ));
                    self.needs_render = false;
                }
                Err(e) => {
                    self.error = e.to_string();
                    self.needs_render = false;
                }
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(texture) = &self.texture else {
                return;
            };
            let doc = self.doc.as_ref().unwrap();
            let dimensions = egui::vec2(doc.width as f32, doc.height as f32);
            let scale = (ui.available_width() / dimensions.x)
                .min(ui.available_height() / dimensions.y)
                .max(0.001);
            let response = ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(dimensions * scale)
                    .sense(egui::Sense::click_and_drag()),
            );
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
                } else if !matches!(self.tool, Tool::Free | Tool::Rectangle | Tool::Move) {
                    self.begin_stroke();
                    self.paint_at(p);
                    self.end_stroke();
                }
            }
            if response.drag_started() {
                self.begin_stroke();
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
    fn begin_stroke(&mut self) {
        self.stroke = self.doc.clone();
        self.stroke_origin = None;
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

fn encode_png(doc: &Document, output: &mut impl std::io::Write) -> Result<()> {
    let image = doc.render()?;
    let mut pixels = Vec::with_capacity(image.pixels.len() * 8);
    for p in &image.pixels {
        let encoded = match doc.color_space {
            ColorSpace::Srgb => tr_core::export::srgb16(*p).0,
            ColorSpace::LinearRec2020 => {
                let alpha = p[3];
                [
                    if alpha > 0. { p[0] / alpha } else { 0. },
                    if alpha > 0. { p[1] / alpha } else { 0. },
                    if alpha > 0. { p[2] / alpha } else { 0. },
                    alpha,
                ]
                .map(|v| (v.clamp(0., 1.) * 65535.).round() as u16)
            }
        };
        for channel in encoded {
            pixels.extend_from_slice(&channel.to_be_bytes());
        }
    }
    {
        let mut encoder = png::Encoder::new(output, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        match doc.color_space {
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
        let ppm = (doc.ppi / 0.0254).round() as u32;
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: ppm,
            yppu: ppm,
            unit: png::Unit::Meter,
        }));
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
        for space in [ColorSpace::Srgb, ColorSpace::LinearRec2020] {
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
    fn opening_workspace_preserves_document_and_stroke_is_one_undo() {
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
                |ui| editor.show(ui),
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
