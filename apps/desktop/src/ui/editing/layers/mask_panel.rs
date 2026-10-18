//! Mask composition stays visible above the selected component.
use super::*;

fn mask_node_label(kind: &MaskKind) -> &'static str {
    match kind {
        MaskKind::Shape(m) => match m.shape {
            Shape::Brush => "Pennello",
            Shape::Linear => "Gradiente",
            Shape::Radial => "Radiale",
            Shape::Luminance => "Luminanza",
            Shape::Hue => "Tonalità",
        },
        MaskKind::Color(_) => "Colore a campioni",
        MaskKind::Add(..) => "Aggiungi",
        MaskKind::Subtract(..) => "Sottrai",
        MaskKind::Intersect(..) => "Interseca",
        MaskKind::Invert(..) => "Inverti maschera",
        MaskKind::Constant(_) => "Copertura",
    }
}

impl Controls {
    pub(super) fn mask_controls(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        layer: &mut Layer,
        commit: &mut bool,
    ) {
        let mut add = None;
        let empty = layer.mask.nodes.is_empty();
        if empty {
            ui.label(egui::RichText::new(lang.text("Foto intera")).strong());
            ui.label(lang.text("L’effetto si applica a tutta la foto. Crea una maschera per scegliere dove agisce."));
            ui.add_space(4.);
        }
        ui.horizontal_wrapped(|ui| {
            for (combine, name) in [
                (Combine::Add, "Aggiungi"),
                (Combine::Subtract, "Sottrai"),
                (Combine::Intersect, "Interseca"),
            ] {
                if empty && combine != Combine::Add {
                    continue;
                }
                let name = if empty { "Crea maschera" } else { name };
                ui.add_enabled_ui(layer.mask.nodes.len() + 2 <= layers::MAX_MASK_NODES, |ui| {
                    ui.menu_button(lang.text(name), |ui| {
                        for (shape, label) in [
                            (Shape::Brush, "Pennello"),
                            (Shape::Linear, "Gradiente"),
                            (Shape::Radial, "Radiale"),
                            (Shape::Luminance, "Luminanza"),
                        ] {
                            if help::button(ui, lang, label).clicked() {
                                add = Some((
                                    combine,
                                    MaskKind::Shape(Mask {
                                        shape,
                                        ..Default::default()
                                    }),
                                ));
                                ui.close();
                            }
                        }
                        if help::button(ui, lang, "Colore a campioni").clicked() {
                            let range = layer
                                .operators
                                .iter()
                                .find_map(|o| {
                                    if let Operator::SampleColor(s) = o {
                                        Some(s.range.clone())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_default();
                            add = Some((combine, MaskKind::Color(range)));
                            ui.close();
                        }
                    });
                });
            }
        });
        if let Some((combine, kind)) = add {
            self.paint = matches!(&kind,MaskKind::Shape(m) if matches!(m.shape, Shape::Brush | Shape::Linear | Shape::Radial));
            self.component = Some(layer.mask.append(kind, combine));
            self.pick = false;
            self.overlay = true;
            self.overlay_operator = None;
            *commit = true;
        }
        if layer.mask.nodes.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            self.overlay_control(ui, lang, None);
            if (self.paint || self.pick || self.overlay)
                && help::button(ui, lang, "Fine selezione").clicked()
            {
                self.cancel_analysis();
                self.paint = false;
                self.pick = false;
                self.overlay = false;
            }
        });
        if self
            .component
            .is_none_or(|id| !layer.mask.nodes.iter().any(|n| n.id == id))
        {
            self.component = mask_rows(&layer.mask).last().map(|(_, id)| *id);
        }
        // Show the entire tree together; the selected node's controls follow below it.
        let rows = mask_rows(&layer.mask);
        let tree_height = (rows.len() as f32 * 30.).min(144.);
        egui::ScrollArea::vertical()
            .id_salt("mask-tree")
            .min_scrolled_height(tree_height)
            .max_height(tree_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                ui.spacing_mut().button_padding.y = 4.;
                for (depth, id) in rows {
                    let node = layer.mask.nodes.iter().find(|n| n.id == id).unwrap();
                    ui.push_id((node.id, "row"), |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space((depth as f32 * 10.).min(60.));
                            let response = ui.add_sized(
                                [ui.available_width(), 24.],
                                egui::Button::selectable(self.component == Some(node.id), ())
                                    .left_text(lang.text(mask_node_label(&node.kind)))
                                    .truncate()
                                    .frame_when_inactive(self.component == Some(node.id)),
                            );
                            if help::scope(ui, "mask", |ui| {
                                help::attach(ui, response, lang, mask_node_label(&node.kind))
                            })
                            .clicked()
                            {
                                self.component = Some(node.id);
                                self.cancel_analysis();
                                self.paint = false;
                                self.pick = false;
                            }
                        });
                    });
                }
            });
        ui.separator();
        let mut remove = None;
        if let Some(node) = layer
            .mask
            .nodes
            .iter_mut()
            .find(|n| Some(n.id) == self.component)
        {
            ui.push_id((node.id, "controls"), |ui| {
                help::scope(ui, "mask", |ui| {
                    help::label(ui, lang, mask_node_label(&node.kind));
                });
                if let MaskKind::Color(c) = &mut node.kind {
                    if help::selectable_label(
                        ui,
                        lang,
                        "Campiona sulla foto",
                        self.pick && self.pick_component,
                    )
                    .clicked()
                    {
                        self.cancel_analysis();
                        self.pick = !self.pick;
                        self.pick_component = true;
                        self.paint = false;
                        self.overlay = false;
                    }
                    for (i, name) in [
                        "Ampiezza tonalità (°)",
                        "Ampiezza cromia",
                        "Ampiezza luminanza",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let (low, high, default) = if i == 0 {
                            (0.1, 180., 40.)
                        } else {
                            (0.0001, 4., 0.4)
                        };
                        slider(ui, lang, name, &mut c.width[i], low..=high, default, commit);
                    }
                    slider(
                        ui,
                        lang,
                        "Sfumatura",
                        &mut c.softness,
                        0.01..=1.,
                        0.5,
                        commit,
                    );
                }
                if let MaskKind::Constant(v) = &mut node.kind {
                    slider(ui, lang, "Copertura", v, 0. ..=1., 1., commit);
                }
                let operands = match node.kind {
                    MaskKind::Add(a, b) | MaskKind::Subtract(a, b) | MaskKind::Intersect(a, b) => {
                        Some((a, b))
                    }
                    _ => None,
                };
                if let Some((a, b)) = operands {
                    ui.horizontal_wrapped(|ui| {
                        for (kind, label) in [
                            (MaskKind::Add(a, b), "Aggiungi"),
                            (MaskKind::Subtract(a, b), "Sottrai"),
                            (MaskKind::Intersect(a, b), "Interseca"),
                        ] {
                            if ui
                                .selectable_label(node.kind == kind, lang.text(label))
                                .clicked()
                            {
                                node.kind = kind;
                                *commit = true;
                            }
                        }
                    });
                }
                if let MaskKind::Shape(m) = &mut node.kind {
                    if matches!(m.shape, Shape::Brush | Shape::Linear | Shape::Radial) {
                        help::checkbox(ui, lang, "Disegna sulla foto", &mut self.paint);
                        slider(
                            ui,
                            lang,
                            "Raggio maschera",
                            &mut m.radius,
                            0.005..=1.,
                            0.25,
                            commit,
                        );
                    }
                    if m.shape == Shape::Linear {
                        slider(
                            ui,
                            lang,
                            "Angolo gradiente",
                            &mut m.angle,
                            -180. ..=180.,
                            0.,
                            commit,
                        );
                    }
                    if m.shape == Shape::Luminance {
                        let [lo, hi] = m.interval;
                        slider(
                            ui,
                            lang,
                            "Intervallo minimo",
                            &mut m.interval[0],
                            0. ..=hi - 0.001,
                            0.25,
                            commit,
                        );
                        slider(
                            ui,
                            lang,
                            "Intervallo massimo",
                            &mut m.interval[1],
                            lo + 0.001..=1.,
                            0.75,
                            commit,
                        );
                    }
                    slider(
                        ui,
                        lang,
                        "Sfumatura",
                        &mut m.feather,
                        0.01..=1.,
                        0.5,
                        commit,
                    );
                    *commit |=
                        help::checkbox(ui, lang, "Inverti maschera", &mut m.invert).changed();
                }
                if help::button(ui, lang, "Elimina componente").clicked() {
                    remove = Some(node.id);
                }
            });
        }
        if let Some(id) = remove {
            layer.mask.remove(id);
            self.component = None;
            self.paint = false;
            *commit = true;
        }
        ui.menu_button(lang.text("Azioni maschera"), |ui| {
            if ui
                .add_enabled(
                    layer.mask.nodes.len() < layers::MAX_MASK_NODES,
                    egui::Button::new(lang.text("Inverti maschera")),
                )
                .clicked()
            {
                layer.mask.invert();
                *commit = true;
                ui.close();
            }
        });
        if self.overlay {
            self.cancel_analysis();
            ui.small(
                lang.text("Overlay provvisorio sull'ingresso del livello · escluso dall'export"),
            );
        }
    }
}
