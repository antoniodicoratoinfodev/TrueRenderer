//! The stack chooses a layer; the inspector edits that layer's effects or mask.
use super::*;
use crate::ui::style;

fn tools_menu(ui: &mut egui::Ui, lang: Language) -> Option<&'static layers::Tool> {
    let mut chosen = None;
    ui.set_width(232.);
    egui::ScrollArea::vertical()
        .id_salt("layer-tool-menu")
        .max_height(320.)
        .show(ui, |ui| {
            for tool in TOOLS {
                ui.horizontal(|ui| {
                    palette::icon(ui, lang, tool.id);
                    if help::button(ui, lang, tool.name).clicked() {
                        chosen = Some(tool);
                        ui.close();
                    }
                });
            }
        });
    chosen
}

fn layer_menu(
    ui: &mut egui::Ui,
    lang: Language,
    layer: &mut Layer,
    index: usize,
    count: usize,
    commit: &mut bool,
) -> Option<(usize, u8)> {
    let mut action = None;
    ui.set_width(220.);
    ui.add_enabled_ui(!layer.locked, |ui| {
        let label = help::label(ui, lang, "Nome del livello");
        let rename = ui
            .add(egui::TextEdit::singleline(&mut layer.name).desired_width(ui.available_width()))
            .labelled_by(label.id);
        *commit |= rename.lost_focus();
        percent_slider(ui, lang, "Riempimento", &mut layer.fill, commit);
    });
    ui.separator();
    *commit |= help::checkbox(ui, lang, "Blocca modifiche", &mut layer.locked).changed();
    for (code, label, enabled) in [
        (0, "Duplica livello", count < layers::MAX_LAYERS),
        (1, "Sposta sopra", index + 1 < count && !layer.locked),
        (2, "Sposta sotto", index > 0 && !layer.locked),
        (3, "Elimina livello", !layer.locked),
    ] {
        if ui
            .add_enabled(enabled, egui::Button::new(lang.text(label)))
            .clicked()
        {
            action = Some((index, code));
            ui.close();
        }
    }
    action
}

impl Controls {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        recipe: &mut EditRecipe,
    ) -> (bool, bool) {
        self.refresh_analysis(recipe);
        let before = recipe.clone();
        let mut commit = false;
        let count = recipe.layers.as_ref().map_or(0, |s| s.layers.len());
        let mut create_empty = false;
        let mut create_tool = None;
        ui.horizontal(|ui| {
            help::label(ui, lang, "Pila livelli");
            ui.label(
                egui::RichText::new(count.to_string())
                    .small()
                    .color(style::MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(count < layers::MAX_LAYERS, |ui| {
                    ui.menu_button(lang.text("Nuovo livello"), |ui| {
                        if help::button(ui, lang, "Nuovo livello vuoto").clicked() {
                            create_empty = true;
                            ui.close();
                        }
                        ui.separator();
                        ui.small(lang.text("Crea con una regolazione"));
                        create_tool = tools_menu(ui, lang);
                    })
                    .response
                    .on_hover_text(
                        lang.text("Crea un livello vuoto o già pronto con uno strumento."),
                    );
                });
            });
        });
        if create_empty {
            let layer = Layer::empty(lang.text("Livello vuoto"));
            self.select_layer(layer.id);
            recipe.layer_stack().layers.push(layer);
            commit = true;
        }
        if let Some(tool) = create_tool {
            self.apply_selected = false;
            match self.apply_tool(recipe, tool, lang) {
                Ok(()) => commit = true,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        if recipe.layers.as_ref().is_none_or(|s| s.layers.is_empty()) {
            ui.add_space(8.);
            ui.label(egui::RichText::new(lang.text("Nessun livello aggiunto")).strong());
            ui.label(lang.text("Usa Nuovo livello per aggiungere una regolazione. Puoi limitarla a una parte della foto con una maschera."));
            ui.add_space(8.);
            self.base_link(ui, lang);
            return (recipe != &before, commit);
        }
        let stack = recipe.layers.as_mut().unwrap();
        let count = stack.layers.len();
        if !stack.layers.iter().any(|l| Some(l.id) == self.selected) {
            self.select_layer(stack.layers.last().unwrap().id);
        }
        let mut action = None;
        // Keep long stacks bounded so the selected layer's controls stay reachable.
        let stack_height = if ui.ctx().content_rect().height() < 600. {
            104.
        } else {
            196.
        };
        egui::ScrollArea::vertical()
            .id_salt(("layer-stack", &self.photo))
            .max_height(stack_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                for i in (0..count).rev() {
                    let layer = &mut stack.layers[i];
                    ui.push_id(layer.id, |ui| {
                        let selected = self.selected == Some(layer.id);
                        let row = egui::Frame::new()
                            .fill(if selected {
                                style::SELECTED
                            } else {
                                style::SURFACE
                            })
                            .corner_radius(4)
                            .inner_margin(4)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(3., 0.);
                                ui.horizontal(|ui| {
                                    if panels::icon_button(
                                        ui,
                                        panels::Glyph::Eye,
                                        lang.text(if layer.enabled {
                                            "Nascondi livello"
                                        } else {
                                            "Mostra livello"
                                        }),
                                        true,
                                        layer.enabled,
                                    )
                                    .clicked()
                                    {
                                        layer.enabled = !layer.enabled;
                                        commit = true;
                                    }
                                    // Reserve the menu's real native width, including its padding.
                                    let menu_width = ui
                                        .painter()
                                        .layout_no_wrap(
                                            "…".into(),
                                            egui::TextStyle::Button.resolve(ui.style()),
                                            ui.visuals().text_color(),
                                        )
                                        .size()
                                        .x
                                        + 2. * ui.spacing().button_padding.x;
                                    let width = (ui.available_width()
                                        - menu_width.max(ui.spacing().interact_size.x)
                                        - ui.spacing().item_spacing.x)
                                        .max(0.);
                                    ui.vertical(|ui| {
                                        ui.set_width(width);
                                        let name = ui
                                            .add_sized(
                                                [width, 22.],
                                                egui::Button::selectable(selected, ())
                                                    .left_text(
                                                        egui::RichText::new(&layer.name).strong(),
                                                    )
                                                    .frame(false)
                                                    .truncate(),
                                            )
                                            .on_hover_text(&layer.name);
                                        if name.clicked() && !selected {
                                            self.select_layer(layer.id);
                                        }
                                        let summary = format!(
                                            "{} {} · {}{}",
                                            layer.operators.len(),
                                            lang.text(if layer.operators.len() == 1 {
                                                "regolazione"
                                            } else {
                                                "regolazioni"
                                            }),
                                            lang.text(if layer.mask.nodes.is_empty() {
                                                "Foto intera"
                                            } else {
                                                "Con maschera"
                                            }),
                                            if layer.locked {
                                                format!(" · {}", lang.text("Bloccato"))
                                            } else {
                                                String::new()
                                            }
                                        );
                                        let response = ui
                                            .add(
                                                egui::Label::new(
                                                    egui::RichText::new(&summary)
                                                        .small()
                                                        .color(style::MUTED),
                                                )
                                                .truncate()
                                                .sense(egui::Sense::click()),
                                            )
                                            .on_hover_text(summary);
                                        if response.clicked() && !selected {
                                            self.select_layer(layer.id);
                                        }
                                    });
                                    ui.menu_button("…", |ui| {
                                        action = layer_menu(ui, lang, layer, i, count, &mut commit);
                                    })
                                    .response
                                    .on_hover_text(lang.text("Azioni del livello"));
                                });
                            })
                            .response;
                        if selected {
                            if self.scroll_to_layer {
                                row.scroll_to_me(None);
                                self.scroll_to_layer = false;
                            }
                            ui.painter().vline(
                                row.rect.left() + 1.,
                                row.rect.y_range().shrink(5.),
                                egui::Stroke::new(2., style::SELECTION_EDGE),
                            );
                        }
                    });
                }
            });
        self.base_link(ui, lang);
        if let Some((i, code)) = action {
            match code {
                0 => {
                    let layer = stack.layers[i].duplicate();
                    self.select_layer(layer.id);
                    stack.layers.insert(i + 1, layer);
                }
                1 => stack.layers.swap(i, i + 1),
                2 => stack.layers.swap(i, i - 1),
                _ => {
                    stack.layers.remove(i);
                    if let Some(layer) = stack
                        .layers
                        .get(i.min(stack.layers.len().saturating_sub(1)))
                    {
                        self.select_layer(layer.id);
                    } else {
                        self.selected = None;
                    }
                }
            }
            self.cancel_analysis();
            self.pick = false;
            self.paint = false;
            self.overlay = false;
            self.component = None;
            commit = true;
        }
        if let Some(layer) = stack
            .layers
            .iter_mut()
            .find(|l| Some(l.id) == self.selected)
        {
            ui.separator();
            ui.push_id(("selected-layer", layer.id), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&layer.name).strong()).truncate()).on_hover_text(&layer.name);
                if !layer.enabled { ui.small(lang.text("Livello nascosto · attiva l’occhio per vedere l’effetto")); }
                if layer.locked { ui.small(lang.text("Livello bloccato · sbloccalo dal menu del livello")); }
                ui.add_enabled_ui(!layer.locked, |ui| {
                    percent_slider(ui, lang, "Opacità", &mut layer.opacity, &mut commit);
                });
                let previous_view = self.mask_view;
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.;
                    for (mask, title) in [(false, "Effetti del livello"), (true, "Maschera")] {
                        let r = ui.add_sized([width, 28.], egui::Button::selectable(self.mask_view == mask, lang.text(title)).wrap());
                        if help::attach(ui, r, lang, title).clicked() { self.mask_view = mask; }
                    }
                });
                if previous_view != self.mask_view {
                    self.cancel_analysis();
                    self.pick = false;
                    self.paint = false;
                    self.overlay = false;
                }
                ui.add_space(4.);
                ui.add_enabled_ui(!layer.locked, |ui| {
                    if self.mask_view {
                        self.mask_controls(ui, lang, layer, &mut commit);
                    } else {
                        let mut add = None;
                        ui.add_enabled_ui(layer.operators.len() < layers::MAX_OPERATORS, |ui| {
                            ui.menu_button(lang.text("Aggiungi regolazione"), |ui| { add = tools_menu(ui, lang); })
                                .response.on_hover_text(lang.text("Aggiunge uno strumento al livello selezionato, con la stessa opacità e maschera."));
                        });
                        if let Some(tool) = add {
                            self.cancel_analysis();
                            self.pick = false;
                            self.paint = false;
                            self.overlay = false;
                            layer.operators.push(tool.operator());
                            commit = true;
                        }
                        if layer.operators.is_empty() {
                            ui.add_space(4.);
                            ui.label(lang.text("Questo livello non modifica ancora la foto. Aggiungi la prima regolazione."));
                        }
                        for (i, op) in layer.operators.iter_mut().enumerate() {
                            ui.push_id((layer.id, "operator", i), |ui| {
                                let tool = op.tool();
                                let heading = panels::Section::new(lang, "operator-panel", tool.name)
                                    .icon(tool.id).modified(!op.is_neutral()).show(ui, |ui| {
                                        help::scope(ui, tool.id, |ui| self.operator(ui, lang, op, i, &mut commit));
                                    });
                                if i == 0 && std::env::args().any(|a| a == "--color-controls-ui-smoke") {
                                    heading.scroll_to_me(Some(egui::Align::Min));
                                }
                            });
                        }
                    }
                });
            });
        }
        commit |= self.apply_auto_request(recipe);
        if recipe != &before
            && let Err(error) = recipe.validate()
        {
            *recipe = before.clone();
            self.error = Some(error.to_string());
            commit = false;
        }
        if let Some(error) = &self.error {
            ui.colored_label(super::super::AMBER, lang.text(error));
        }
        (recipe != &before, commit)
    }

    fn base_link(&mut self, ui: &mut egui::Ui, lang: Language) {
        if help::button(ui, lang, "Sviluppo di base").clicked() {
            self.open_tools(false);
            self.view_tools = false;
        }
    }
}
