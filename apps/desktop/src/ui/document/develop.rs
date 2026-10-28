use super::*;
impl Editor {
    pub(super) fn develop_controls(&mut self, ui: &mut egui::Ui) {
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
                if self.tab == Tab::Develop {
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
                    ui.add_enabled(
                        !switches.contains("light-panel"),
                        egui::Slider::new(&mut recipe.exposure_ev, -10. ..=10.)
                            .text("Esposizione EV"),
                    );
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
                        let (changed, _) = crate::ui::editing::curve::controls(
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
                }
                ui.collapsing("Regolazioni e maschere del livello", |ui| {
                    self.effects.bind(&format!("document:{}", self.selected));
                    if self.effects.view_tools {
                        self.effects
                            .show_tools(ui, crate::i18n::Language::Italian, &mut recipe);
                    } else {
                        self.effects.view_layers = true;
                        self.effects
                            .show(ui, crate::i18n::Language::Italian, &mut recipe);
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
    }
}
