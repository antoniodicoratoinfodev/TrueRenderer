use super::*;

// The neutralizer expects currently rendered pixels, including the existing WB.
// Only nine source pixels are copied; a picker click must not clone the whole layer.
pub(super) fn sampled_white_balance(
    layer: &RasterLayer,
    uv: [f32; 2],
) -> Result<tr_core::editing::EditRecipe> {
    let x = ((uv[0] * layer.width as f32) as u32).min(layer.width - 1);
    let y = ((uv[1] * layer.height as f32) as u32).min(layer.height - 1);
    ensure!(
        x > 0 && y > 0 && x + 1 < layer.width && y + 1 < layer.height,
        "Area RGB incompleta al bordo: campionare più all'interno"
    );
    let pixels = (y - 1..=y + 1)
        .flat_map(|row| {
            (x - 1..=x + 1).map(move |col| layer.pixels[(row * layer.width + col) as usize])
        })
        .collect();
    let mut sample = LinearImage::new(3, 3, pixels)?;
    let mut recipe = layer.recipe.clone();
    recipe.apply(&mut sample)?;
    let sample = tr_core::editing::sample_rgb_area(&sample, 1, 1, 3)?;
    recipe.neutralize_render_sample(sample.working)?;
    Ok(recipe)
}

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
                    let lang = crate::i18n::Language::Italian;
                    crate::ui::editing::panels::Section::new(
                        lang,
                        "rgb-wb-panel",
                        "Bilanciamento del bianco",
                    )
                    .switches(&mut switches)
                    .modified(recipe.temperature != 0. || recipe.tint != 0.)
                    .show(ui, |ui| {
                        if ui
                            .selectable_label(self.rgb_picker, "Contagocce RGB")
                            .clicked()
                        {
                            self.rgb_picker = !self.rgb_picker;
                            self.effects.stop_document_gesture();
                            self.adjustments.paint = false;
                            self.adjustments.horizon = false;
                        }
                        if self.rgb_picker {
                            ui.small(
                                "Clicca un’area neutra del livello · campione 3 × 3 · Esc annulla",
                            );
                        }
                        for (label, value) in [
                            ("Temperatura RGB", &mut recipe.temperature),
                            ("Tinta RGB", &mut recipe.tint),
                        ] {
                            crate::ui::editing::adjustment(
                                ui,
                                lang,
                                label,
                                value,
                                -100. ..=100.,
                                "",
                                None,
                            );
                        }
                        if ui.button("Azzera correzione RGB").clicked() {
                            recipe.temperature = 0.;
                            recipe.tint = 0.;
                        }
                    });
                    let layer = &self.doc.as_ref().unwrap().layers[self.selected];
                    let response = crate::ui::editing::shared::basic_controls(
                        ui,
                        lang,
                        &mut recipe,
                        &mut switches,
                        Some(|rgb, recipe: &mut tr_core::editing::EditRecipe| {
                            let source =
                                LinearImage::new(layer.width, layer.height, layer.pixels.clone())?;
                            if rgb {
                                recipe.auto_rgb(&source)
                            } else {
                                recipe.auto_exposure(&source)
                            }
                        }),
                    );
                    if let Some(result) = response.auto_result {
                        self.error = result.err().unwrap_or_default();
                    }
                    self.adjustments.show_with_switches(
                        ui,
                        crate::i18n::Language::Italian,
                        &mut recipe,
                        Some([layer.width, layer.height]),
                        &mut switches,
                    );
                }
                if switches.contains("rgb-wb-panel")
                    || self.adjustments.paint
                    || self.adjustments.horizon
                {
                    self.rgb_picker = false;
                }
                self.effects.bind(&format!("document:{}", self.selected));
                if self.tab == Tab::Tools {
                    self.effects.view_tools = true;
                    if self
                        .effects
                        .show_tools(ui, crate::i18n::Language::Italian, &mut recipe)
                    {
                        self.tab = Tab::Layers;
                    }
                } else {
                    egui::CollapsingHeader::new("Regolazioni e maschere del livello")
                        .id_salt(("raster-effects", self.tab as usize))
                        .default_open(self.tab == Tab::Layers)
                        .show(ui, |ui| {
                            self.effects.view_layers = true;
                            self.effects
                                .show(ui, crate::i18n::Language::Italian, &mut recipe);
                        });
                }
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
