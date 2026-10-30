use super::*;
use crate::ui::editing::panels::{Glyph, icon_button};
impl Editor {
    fn canvas_bytes(&self) -> u64 {
        self.doc
            .as_ref()
            .map_or(0, |d| u64::from(d.width) * u64::from(d.height) * 20)
    }
    fn selected_bytes(&self) -> u64 {
        self.doc
            .as_ref()
            .and_then(|d| d.layers.get(self.selected))
            .map_or(0, |l| u64::from(l.width) * u64::from(l.height) * 20)
    }
    pub fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        self.messages(ui);
        ui.set_width(ui.available_width());
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.style_mut().spacing.scroll.floating = false;
        crate::ui::editing::panels::control_style(ui);
        egui::Frame::new()
            .fill(crate::ui::style::SURFACE)
            .corner_radius(6)
            .inner_margin(6)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Editing raster").strong()).on_hover_text("Le regolazioni agiscono sui pixel del livello selezionato. Il WB sensore si modifica sulla foto originale.");
                if self.return_photo.is_some() {
                    if ui
                        .add_enabled(
                            !self.busy(),
                            egui::Button::new("Torna alla foto…"),
                        )
                        .clicked()
                    {
                        self.confirm_return = true;
                    }
                } else {
                    if ui.add_enabled(!self.busy(), egui::Button::new("Scegli originale…")).on_hover_text("Collega la foto da cui riprendere lo sviluppo; il progetto non contiene il RAW originale.").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_title("Scegli la foto originale").set_directory(&self.directory).pick_file()
                {
                    if !tr_platform::supported_extension(&path) {
                        self.error = "Scegli un file fotografico supportato, non un progetto raster o un altro documento.".into();
                    } else {
                        self.return_photo = Some(path);
                        self.confirm_return = true;
                    }
                }
                }
                });
            });
        let tab = self.tab;
        if let Some(page) =
            crate::ui::editing::panels::tabs(ui, crate::i18n::Language::Italian, tab as usize)
        {
            self.select_tab(Tab::from_index(page));
        }
        ui.separator();
        ui.horizontal(|ui| {
            if icon_button(
                ui,
                Glyph::Undo,
                "Annulla · Cmd/Ctrl + Z",
                !self.undo.is_empty() && !self.busy(),
                false,
            )
            .clicked()
            {
                self.undo_edit();
            }
            if icon_button(
                ui,
                Glyph::Redo,
                "Ripeti · Cmd/Ctrl + Shift + Z",
                !self.redo.is_empty() && !self.busy(),
                false,
            )
            .clicked()
            {
                self.redo_edit();
            }
            if let Some(doc) = &self.doc {
                self.selected = self.selected.min(doc.layers.len().saturating_sub(1));
                let label = doc
                    .layers
                    .get(self.selected)
                    .map(|l| format!("Livello {} · {}", self.selected, l.name))
                    .unwrap_or_default();
                if !label.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&label).strong()).truncate())
                        .on_hover_text(label);
                }
            }
        });
        ui.add_enabled_ui(!self.busy(), |ui| {
            if self.tab == Tab::Export {
                self.export_controls(ui);
                return;
            }
            if self.doc.is_none() {
                if ui.button("Nuovo documento…").clicked() {
                    self.new_document();
                }
                return;
            }
            match self.tab {
                Tab::Layers => self.layer_controls(ui),
                Tab::Develop => {
                    ui.collapsing("Colore del progetto", |ui| {
                        let mut space = self.doc.as_ref().unwrap().color_space;
                        if crate::ui::export::profile_controls(ui, &mut space) {
                            self.edit(|d, _| {
                                d.color_space = space;
                                Ok(())
                            });
                            self.export_space = None;
                        }
                    });
                    ui.collapsing("Sposta livello", |ui| self.move_controls(ui));
                    self.develop_controls(ui);
                }
                Tab::Tools => {
                    self.move_controls(ui);
                    ui.separator();
                    self.tool_controls(ui);
                    ui.separator();
                    self.develop_controls(ui);
                }
                Tab::Export => {}
            }
        });
    }
    pub(super) fn select_layer(&mut self, index: usize) {
        self.rgb_picker = false;
        self.end_stroke();
        self.selected = index;
        self.source = None;
        self.adjustments = Default::default();
        self.effects = Default::default();
    }
    pub(super) fn undo_edit(&mut self) {
        self.end_stroke();
        if let Some(old) = self.undo.pop_back() {
            if let Some(doc) = self.doc.replace(old) {
                self.redo.push_back(doc);
            }
            self.dirty = true;
            self.notice.clear();
            self.needs_render = true;
            self.select_layer(self.selected);
        }
    }
    pub(super) fn redo_edit(&mut self) {
        if let Some(next) = self.redo.pop_back() {
            if let Some(doc) = self.doc.replace(next) {
                self.undo.push_back(doc);
            }
            self.dirty = true;
            self.notice.clear();
            self.needs_render = true;
            self.select_layer(self.selected);
        }
    }
    pub(super) fn layer_controls(&mut self, ui: &mut egui::Ui) {
        let layers: Vec<_> = self
            .doc
            .as_ref()
            .unwrap()
            .layers
            .iter()
            .map(|l| (l.name.clone(), l.visible))
            .collect();
        ui.small("I livelli in alto coprono quelli sottostanti. La foto di base è il livello 0.");
        for (index, (name, mut visible)) in layers.into_iter().enumerate().rev() {
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    Glyph::Eye,
                    if visible {
                        "Nascondi livello"
                    } else {
                        "Mostra livello"
                    },
                    true,
                    visible,
                )
                .clicked()
                {
                    visible = !visible;
                    self.edit(|d, _| {
                        d.layers[index].visible = visible;
                        Ok(())
                    });
                }
                let label = if index == 0 {
                    format!("0 · Foto di partenza · {name}")
                } else {
                    format!("{index} · {name}")
                };
                if ui
                    .add_sized(
                        [ui.available_width(), 28.],
                        egui::Button::selectable(self.selected == index, ())
                            .left_text(&label)
                            .truncate(),
                    )
                    .on_hover_text(&label)
                    .clicked()
                {
                    self.select_layer(index);
                }
            });
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Livello vuoto").clicked() && self.admit_edit(self.canvas_bytes()) {
                self.edit(|d, _| {
                    d.insert(RasterLayer::new(
                        "Livello vuoto".into(),
                        LinearImage::new(
                            d.width,
                            d.height,
                            vec![[0.; 4]; d.width as usize * d.height as usize],
                        )?,
                    ))
                });
                if self.error.is_empty() {
                    self.select_layer(self.doc.as_ref().unwrap().layers.len() - 1);
                }
            }
            if ui.button("Inserisci immagine…").clicked() {
                self.import_image(ui.ctx());
            }
            if ui.button("Copia livello").clicked() && self.admit_edit(self.selected_bytes()) {
                self.edit(|d, i| d.duplicate(i));
                if self.error.is_empty() {
                    self.select_layer(self.selected + 1);
                }
            }
            if ui.button("Copia visibile").clicked() && self.admit_edit(self.canvas_bytes()) {
                self.edit(|d, _| d.insert(RasterLayer::new("Copia visibile".into(), d.render()?)));
                if self.error.is_empty() {
                    self.select_layer(self.doc.as_ref().unwrap().layers.len() - 1);
                }
            }
            self.selection_copy_button(ui);
        });
        let count = self.doc.as_ref().unwrap().layers.len();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.selected > 0, egui::Button::new("Elimina livello"))
                .clicked()
            {
                self.edit(|d, i| d.remove_layer(i));
                self.selected = self
                    .selected
                    .min(self.doc.as_ref().unwrap().layers.len().saturating_sub(1));
            }
            if ui
                .add_enabled(
                    self.selected > 0 && self.selected + 1 < count,
                    egui::Button::new("Su"),
                )
                .clicked()
            {
                self.edit(|d, i| d.move_layer(i, i + 1));
                if self.error.is_empty() {
                    self.select_layer(self.selected + 1);
                }
            }
            if ui
                .add_enabled(self.selected > 1, egui::Button::new("Giù"))
                .clicked()
            {
                self.edit(|d, i| d.move_layer(i, i - 1));
                if self.error.is_empty() {
                    self.select_layer(self.selected - 1);
                }
            }
        });
        if let Some(layer) = self.doc.as_ref().unwrap().layers.get(self.selected) {
            let (mut name, mut locked, mut opacity, mut blend) =
                (layer.name.clone(), layer.locked, layer.opacity, layer.blend);
            let mut changed = ui.text_edit_singleline(&mut name).changed();
            changed |= ui.checkbox(&mut locked, "Blocca livello").changed();
            ui.add_enabled_ui(!locked, |ui| {
                changed |= ui
                    .add(egui::Slider::new(&mut opacity, 0. ..=1.).text("Opacità"))
                    .changed();
                egui::ComboBox::from_id_salt("raster-blend")
                    .selected_text(match blend {
                        Blend::Normal => "Normale",
                        Blend::Multiply => "Moltiplica",
                        Blend::Screen => "Scolora",
                    })
                    .show_ui(ui, |ui| {
                        for (b, label) in [
                            (Blend::Normal, "Normale"),
                            (Blend::Multiply, "Moltiplica"),
                            (Blend::Screen, "Scolora"),
                        ] {
                            changed |= ui.selectable_value(&mut blend, b, label).changed();
                        }
                    });
            });
            if changed {
                self.edit(|d, i| {
                    let l = &mut d.layers[i];
                    l.name = name;
                    l.locked = locked;
                    l.opacity = opacity;
                    l.blend = blend;
                    Ok(())
                });
            }
        }
        ui.separator();
        self.move_controls(ui);
        ui.separator();
        self.develop_controls(ui);
    }
    pub(super) fn selection_copy_button(&mut self, ui: &mut egui::Ui) {
        let enabled = self
            .doc
            .as_ref()
            .is_some_and(|d| d.selection.iter().any(|v| *v > 0.));
        if ui
            .add_enabled(enabled, egui::Button::new("Selezione → nuovo livello"))
            .clicked()
        {
            if !self.admit_edit(self.selected_bytes()) {
                return;
            }
            self.edit(|d, i| d.copy_selection(i));
            if self.error.is_empty() {
                self.select_layer(self.selected + 1);
                self.tool = Tool::Move;
                self.points.clear();
            }
        }
    }
    pub(super) fn move_controls(&mut self, ui: &mut egui::Ui) {
        let Some(layer) = self.doc.as_ref().and_then(|d| d.layers.get(self.selected)) else {
            return;
        };
        let locked = layer.locked;
        let mut transform = layer.transform;
        let mut changed = false;
        ui.add_enabled_ui(!locked, |ui| {
            if ui
                .selectable_label(self.tool == Tool::Move, "Sposta livello sulla foto")
                .clicked()
            {
                self.rgb_picker = false;
                self.tool = Tool::Move;
                self.effects.stop_document_gesture();
                self.adjustments.paint = false;
                self.adjustments.horizon = false;
            }
            ui.small("Trascina il livello nell'immagine. Le trasformazioni restano modificabili.");
            ui.collapsing("Posizione, scala e rotazione", |ui| {
                egui::Grid::new("layer-transform")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (name, value, range) in [
                            ("X · px", &mut transform.x, -65536. ..=65536.),
                            ("Y · px", &mut transform.y, -65536. ..=65536.),
                            ("Scala", &mut transform.scale, 0.01..=100.),
                            ("Rotazione °", &mut transform.angle, -360. ..=360.),
                        ] {
                            ui.label(name);
                            changed |= ui
                                .add(egui::DragValue::new(value).speed(0.1).range(range))
                                .changed();
                            ui.end_row();
                        }
                    });
                if ui.button("Azzera trasformazione").clicked() {
                    transform = Default::default();
                    changed = true;
                }
            });
        });
        if changed {
            self.edit(|d, i| {
                d.layers[i].transform = transform;
                Ok(())
            });
        }
    }
}
