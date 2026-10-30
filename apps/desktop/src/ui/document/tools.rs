use super::*;
impl Editor {
    pub(super) fn tool_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Selezione e ritocco");
        ui.horizontal_wrapped(|ui| {
            for (tool, name) in [
                (Tool::Move, "Sposta"),
                (Tool::Paint, "Pennello"),
                (Tool::Erase, "Gomma"),
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
        if matches!(
            self.tool,
            Tool::Paint | Tool::Erase | Tool::Clone | Tool::Heal | Tool::AutoHeal
        ) {
            ui.add(egui::Slider::new(&mut self.radius, 0.5..=512.).text("Raggio px"));
            ui.add(egui::Slider::new(&mut self.hardness, 0. ..=1.).text("Durezza"));
            ui.add(egui::Slider::new(&mut self.strength, 0. ..=1.).text("Intensità"));
            if self.tool == Tool::Paint {
                ui.color_edit_button_rgba_unmultiplied(&mut self.color);
            }
            if matches!(self.tool, Tool::Clone | Tool::Heal) {
                ui.small("Alt+clic campiona dal livello selezionato prima del tratto.");
            }
        }
        ui.separator();
        ui.label("Selezione sulla tela");
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
        self.selection_copy_button(ui);
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
                            if x < 0 || y < 0 || x >= d.width as i64 || y >= d.height as i64 {
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
    }
}
