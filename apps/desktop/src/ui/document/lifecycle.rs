//! Document lifetime is independent from browser selection; there is no second workspace.
use super::*;
use std::path::{Component, Path};

impl Editor {
    pub fn matches(&self, path: &Path) -> bool {
        self.doc.is_some()
            && (self.path.as_deref() == Some(path) || self.source_path.as_deref() == Some(path))
    }
    pub fn loading(&self, path: &Path) -> bool {
        self.import_path.as_deref() == Some(path)
    }
    pub fn location(&self) -> Option<PathBuf> {
        self.path.clone().or_else(|| self.source_path.clone())
    }
    pub fn select(&mut self, path: &Path) -> bool {
        if self.matches(path) {
            self.attempted = None;
            return true;
        }
        self.end_stroke();
        self.effects.stop_document_gesture();
        if !tr_core::document::is_document_path(path) {
            self.attempted = None;
            return false;
        }
        if self.attempted.as_deref() == Some(path) || self.import.is_some() {
            return false;
        }
        self.open(path.to_path_buf())
    }
    pub fn unavailable(&mut self, ui: &mut egui::Ui) {
        self.messages(ui);
        if self.import.is_none() {
            ui.label("Le modifiche del lavoro precedente sono conservate. Selezionalo nella cartella per riprenderle.");
            if ui.button("Riprova apertura").clicked() {
                self.attempted = None;
            }
        }
    }
    pub fn messages(&self, ui: &mut egui::Ui) {
        if !self.error.is_empty() {
            ui.colored_label(Color32::YELLOW, &self.error);
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        if self.import.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Preparazione immagine alla risoluzione nativa…");
            });
        }
    }
    pub fn information(&self, ui: &mut egui::Ui) {
        if let Some(d) = &self.doc {
            ui.label(format!(
                "{} × {} px · {} livelli",
                d.width,
                d.height,
                d.layers.len()
            ));
            ui.small(match d.physical_resolution() {
                Some(r) => format!("{:.2} × {:.2} PPI · risoluzione del documento", r.x, r.y),
                None => "DPI/PPI non dichiarati nella sorgente".into(),
            });
            ui.label(if self.dirty {
                "• Modifiche non salvate"
            } else {
                "Progetto salvato"
            });
            ui.small("Composito nativo · Adatta / 1:1");
        }
    }
    pub(super) fn suggested_name(&self) -> String {
        self.location()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .map(|s| format!("{s}.truerenderer"))
            .unwrap_or_else(|| "Documento.truerenderer".into())
    }
    pub(super) fn saved(&mut self, path: PathBuf, result: Result<()>, reveal: bool) {
        match result {
            Ok(()) => {
                self.path = Some(path.clone());
                self.source_path = None;
                self.source_id = None;
                self.dirty = false;
                self.error.clear();
                self.notice = "Progetto salvato".into();
                self.published.push_back(path.clone());
                if reveal {
                    self.reveal = Some(path);
                }
            }
            Err(e) => self.error = e.to_string(),
        }
    }
    pub(super) fn create_in_folder(&mut self) -> Result<()> {
        let name = self.name.trim();
        ensure!(
            !name.is_empty()
                && Path::new(name).components().count() == 1
                && matches!(
                    Path::new(name).components().next(),
                    Some(Component::Normal(_))
                )
                && !name.contains(['/', '\\', ':']),
            "Usare un nome di file senza cartelle"
        );
        let mut path = self.directory.join(name);
        if !tr_core::document::is_document_path(&path) {
            path.set_file_name(format!("{name}.truerenderer"));
        }
        let w = self.unit.pixels(self.width, self.ppi)?;
        let h = self.unit.pixels(self.height, self.ppi)?;
        let pixels = u64::from(w) * u64::from(h);
        ensure!(
            pixels <= tr_core::document::MAX_CANVAS_PIXELS as u64,
            "Dimensioni oltre 64 Mi pixel"
        );
        ensure!(
            self.admit_bytes(tr_core::document::admission_bytes(pixels * 16, pixels)),
            "{}",
            self.error
        );
        let mut d = Document::new(
            w,
            h,
            self.ppi,
            if self.transparent {
                [0.; 4]
            } else {
                tr_core::color::from_encoded_srgb(self.color)
            },
        )?;
        d.unit = self.unit;
        d.color_space = self.space;
        // A colliding name (including a race) must never overwrite another project.
        tr_store::document::create(&path, &d)?;
        self.replace(d);
        self.saved(path, Ok(()), true);
        self.select_tab(Tab::Develop);
        Ok(())
    }
    pub fn new_dialog(&mut self, ctx: &egui::Context) {
        if !self.new_dialog {
            return;
        }
        egui::Window::new("Nuovo documento")
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Nome");
                    ui.text_edit_singleline(&mut self.name);
                });
                ui.label(format!("Cartella: {}", self.directory.display()));
                ui.horizontal(|ui| {
                    ui.label("Larghezza");
                    ui.add(egui::DragValue::new(&mut self.width).range(0.01..=65536.));
                });
                ui.horizontal(|ui| {
                    ui.label("Altezza");
                    ui.add(egui::DragValue::new(&mut self.height).range(0.01..=65536.));
                });
                ui.horizontal(|ui| {
                    for (unit, label) in [
                        (Unit::Pixels, "px"),
                        (Unit::Centimeters, "cm"),
                        (Unit::Millimeters, "mm"),
                        (Unit::Inches, "pollici"),
                    ] {
                        ui.selectable_value(&mut self.unit, unit, label);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("PPI");
                    ui.add(egui::DragValue::new(&mut self.ppi).range(1. ..=9600.));
                });
                if ui.button("Scambia orientamento").clicked() {
                    std::mem::swap(&mut self.width, &mut self.height);
                }
                ui.checkbox(&mut self.transparent, "Sfondo trasparente");
                if !self.transparent {
                    ui.color_edit_button_rgba_unmultiplied(&mut self.color);
                }
                ui.horizontal(|ui| {
                    ui.label("Spazio di uscita");
                    ui.selectable_value(&mut self.space, ColorSpace::Srgb, "sRGB");
                    ui.selectable_value(
                        &mut self.space,
                        ColorSpace::LinearRec2020,
                        "Rec.2020 lineare",
                    );
                });
                if !self.error.is_empty() {
                    ui.colored_label(Color32::YELLOW, &self.error);
                }
                ui.horizontal(|ui| {
                    if ui.button("Crea").clicked() && self.replace_allowed() && self.admit() {
                        match self.create_in_folder() {
                            Ok(()) => self.new_dialog = false,
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                    if ui.button("Annulla").clicked() {
                        self.new_dialog = false;
                    }
                });
            });
    }
    pub fn keyboard(&mut self, ctx: &egui::Context) {
        if self.import.is_some()
            || self.new_dialog
            || ctx.text_edit_focused()
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let command = egui::Modifiers::COMMAND;
        let shift = command | egui::Modifiers::SHIFT;
        if ctx.input_mut(|i| i.consume_key(shift, egui::Key::Z)) {
            self.redo_edit();
        } else if ctx.input_mut(|i| i.consume_key(command, egui::Key::Z)) {
            self.undo_edit();
        }
        if ctx.input_mut(|i| i.consume_key(shift, egui::Key::S)) {
            self.save(true);
        } else if ctx.input_mut(|i| i.consume_key(command, egui::Key::S)) {
            self.save(false);
        }
    }
    pub fn thumbnail(&mut self, ui: &mut egui::Ui, item: &tr_core::Item, area: egui::Rect) {
        if self.matches(&item.path) {
            self.render_texture(ui.ctx());
            if let Some(texture) = &self.texture {
                thumbnails::paint(ui, texture, area);
            }
        } else {
            self.previews.show(ui, item, area);
        }
    }
    pub fn invalidate_preview(&mut self, path: &Path) {
        self.previews.invalidate(path);
    }
}
