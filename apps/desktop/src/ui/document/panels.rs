use super::*;
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
        let tab = self.tab;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            let width = ((ui.available_width() - 6.) / 4.).max(0.);
            for (page, name) in [
                (Tab::Develop, "Sviluppo"),
                (Tab::Layers, "Livelli"),
                (Tab::Tools, "Tools"),
                (Tab::Export, "Export"),
            ] {
                if super::super::style::segment(ui, tab == page, name, width).clicked() {
                    self.select_tab(page);
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.undo.is_empty() && !self.busy(),
                    egui::Button::new("Annulla"),
                )
                .clicked()
            {
                self.undo_edit();
            }
            if ui
                .add_enabled(
                    !self.redo.is_empty() && !self.busy(),
                    egui::Button::new("Ripeti"),
                )
                .clicked()
            {
                self.redo_edit();
            }
        });
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
                    self.move_controls(ui);
                    ui.separator();
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
                if ui
                    .checkbox(&mut visible, "")
                    .on_hover_text("Mostra/nascondi livello")
                    .changed()
                {
                    self.edit(|d, _| {
                        d.layers[index].visible = visible;
                        Ok(())
                    });
                }
                if ui
                    .selectable_label(self.selected == index, format!("{index} · {name}"))
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
