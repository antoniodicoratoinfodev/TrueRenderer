use super::{EditRecipe, adjustment};
use crate::i18n::Language;
use eframe::egui;
use tr_core::editing::masks::{Mask, Shape};

#[derive(Default)]
pub(in crate::ui) struct Controls {
    pub mask: usize,
    pub paint: bool,
    pub horizon: bool,
    horizon_start: Option<(String, [f32; 2], egui::Pos2)>,
    pub overlay: bool,
    new_stroke: bool,
}

fn slider(
    ui: &mut egui::Ui,
    lang: Language,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    commit: &mut bool,
) {
    let r = adjustment(ui, lang.text(label), value, min..=max, "", None);
    *commit |= r.drag_stopped() || (r.changed() && !r.dragged());
}
fn reset<T: Default + PartialEq>(
    ui: &mut egui::Ui,
    lang: Language,
    value: &mut T,
    commit: &mut bool,
) {
    if ui
        .add_enabled(
            *value != T::default(),
            egui::Button::new(lang.text("Ripristina sezione")).small(),
        )
        .clicked()
    {
        *value = T::default();
        *commit = true;
    }
}

impl Controls {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        recipe: &mut EditRecipe,
        native: Option<[u32; 2]>,
    ) -> (bool, bool) {
        let mut a = recipe.advanced.clone().unwrap_or_default();
        let before = a.clone();
        let mut commit = false;
        ui.separator();
        egui::CollapsingHeader::new(lang.text("Ritaglio e geometria"))
            .id_salt("geometry")
            .show(ui, |ui| {
                let g = &mut a.geometry;
                ui.horizontal_wrapped(|ui| {
                    if ui.button("↶ 90°").clicked() {
                        g.quarter_turns = (g.quarter_turns + 3) % 4;
                        commit = true;
                    }
                    if ui.button("↷ 90°").clicked() {
                        g.quarter_turns = (g.quarter_turns + 1) % 4;
                        commit = true;
                    }
                    commit |= ui
                        .checkbox(&mut g.flip_horizontal, lang.text("Specchio orizzontale"))
                        .changed();
                    commit |= ui
                        .checkbox(&mut g.flip_vertical, lang.text("Specchio verticale"))
                        .changed();
                });
                ui.label(lang.text("Rapporto ritaglio"));
                ui.horizontal_wrapped(|ui| {
                    for (label, ratio) in [
                        ("Originale", 0.),
                        ("1:1", 1.),
                        ("3:2", 1.5),
                        ("4:3", 4. / 3.),
                        ("5:4", 1.25),
                        ("16:9", 16. / 9.),
                    ] {
                        if ui
                            .add_enabled(
                                native.is_some(),
                                egui::Button::new(lang.text(label)).small(),
                            )
                            .clicked()
                        {
                            let [mut w, mut h] = native.unwrap().map(|v| v as f32);
                            if g.quarter_turns % 2 == 1 {
                                std::mem::swap(&mut w, &mut h);
                            }
                            let ratio = if ratio == 0. { w / h } else { ratio };
                            let width = (ratio / (w / h)).min(1.);
                            let height = ((w / h) / ratio).min(1.);
                            g.crop = [
                                (1. - width) / 2.,
                                (1. - height) / 2.,
                                (1. + width) / 2.,
                                (1. + height) / 2.,
                            ];
                            commit = true;
                        }
                    }
                });
                let [l, t, r, b] = g.crop;
                for (i, label, min, max) in [
                    (0, "Bordo sinistro", 0., r - 0.01),
                    (1, "Bordo superiore", 0., b - 0.01),
                    (2, "Bordo destro", l + 0.01, 1.),
                    (3, "Bordo inferiore", t + 0.01, 1.),
                ] {
                    let mut percent = g.crop[i] * 100.;
                    let previous = percent;
                    slider(
                        ui,
                        lang,
                        label,
                        &mut percent,
                        min * 100.,
                        max * 100.,
                        &mut commit,
                    );
                    if previous != percent {
                        g.crop[i] = percent / 100.;
                    }
                }
                slider(
                    ui,
                    lang,
                    "Raddrizzamento (°)",
                    &mut g.angle,
                    -45.,
                    45.,
                    &mut commit,
                );
                if ui
                    .checkbox(
                        &mut self.horizon,
                        lang.text("Traccia linea per raddrizzare"),
                    )
                    .changed()
                {
                    self.horizon_start = None;
                    if self.horizon {
                        self.paint = false;
                    }
                }
                slider(
                    ui,
                    lang,
                    "Prospettiva orizzontale",
                    &mut g.perspective[0],
                    -100.,
                    100.,
                    &mut commit,
                );
                slider(
                    ui,
                    lang,
                    "Prospettiva verticale",
                    &mut g.perspective[1],
                    -100.,
                    100.,
                    &mut commit,
                );
                slider(
                    ui,
                    lang,
                    "Scala geometria",
                    &mut g.scale,
                    1.,
                    4.,
                    &mut commit,
                );
                if ui
                    .add_enabled(
                        native.is_some(),
                        egui::Button::new(lang.text("Elimina bordi vuoti")),
                    )
                    .clicked()
                {
                    let size = native.unwrap();
                    let valid = |g: &tr_core::editing::geometry::Geometry| {
                        (0..=64).all(|i| {
                            let t = i as f64 / 64.;
                            [[t, 0.], [t, 1.], [0., t], [1., t]].into_iter().all(|p| {
                                g.source_point(p, size)
                                    .is_some_and(|p| p.iter().all(|v| (0.001..=0.999).contains(v)))
                            })
                        })
                    };
                    // Only commit a solution that actually covers all sampled edges.
                    let mut candidate = g.clone();
                    candidate.scale = 4.;
                    if valid(&candidate) {
                        let (mut low, mut high) = (g.scale, 4.);
                        for _ in 0..24 {
                            candidate.scale = (low + high) * 0.5;
                            if valid(&candidate) {
                                high = candidate.scale;
                            } else {
                                low = candidate.scale;
                            }
                        }
                        g.scale = high;
                        commit = true;
                    }
                }
                if let Some(native) = native {
                    let [w, h] = g.output_size(native);
                    ui.small(format!("{w} × {h} px"));
                }
                if ui.button(lang.text("Ripristina geometria")).clicked() {
                    let previous = g.clone();
                    *g = Default::default();
                    g.distortion = previous.distortion;
                    g.moustache = previous.moustache;
                    g.vignette = previous.vignette;
                    g.ca = previous.ca;
                    g.defringe = previous.defringe;
                    commit = true;
                }
            });
        egui::CollapsingHeader::new(lang.text("Ottica manuale"))
            .id_salt("optics")
            .show(ui, |ui| {
                ui.small(lang.text(
                    "Correzioni manuali aggiuntive. Verifica quelle già applicate dal motore.",
                ));
                let g = &mut a.geometry;
                for (label, v, min, max) in [
                    ("Distorsione", &mut g.distortion, -100., 100.),
                    ("Distorsione a baffo", &mut g.moustache, -100., 100.),
                    ("Vignettatura ottica", &mut g.vignette, -100., 100.),
                    ("Defringe viola/verde", &mut g.defringe, 0., 100.),
                ] {
                    slider(ui, lang, label, v, min, max, &mut commit);
                }
                ui.small(
                    lang.text("CA residua sul render RGB; non riallinea i canali del sensore RAW."),
                );
                slider(
                    ui,
                    lang,
                    "CA residua rosso (px)",
                    &mut g.ca[0],
                    -10.,
                    10.,
                    &mut commit,
                );
                slider(
                    ui,
                    lang,
                    "CA residua blu (px)",
                    &mut g.ca[1],
                    -10.,
                    10.,
                    &mut commit,
                );
                if ui.button(lang.text("Ripristina ottica")).clicked() {
                    g.distortion = 0.;
                    g.moustache = 0.;
                    g.vignette = 0.;
                    g.ca = [0.; 2];
                    g.defringe = 0.;
                    commit = true;
                }
            });
        egui::CollapsingHeader::new(lang.text("Presenza e dettaglio"))
            .id_salt("detail")
            .show(ui, |ui| {
                for (label, v, min, max) in [
                    ("Texture", &mut a.detail.texture, -100., 100.),
                    ("Chiarezza", &mut a.detail.clarity, -100., 100.),
                    ("Rimozione foschia", &mut a.detail.dehaze, -100., 100.),
                    ("Nitidezza", &mut a.detail.sharpen, 0., 150.),
                    ("Raggio nativo (px)", &mut a.detail.radius, 0.3, 3.),
                    ("Soglia rumore", &mut a.detail.threshold, 0., 100.),
                    ("Rumore luminanza", &mut a.detail.luminance_noise, 0., 100.),
                    ("Rumore cromatico", &mut a.detail.chroma_noise, 0., 100.),
                ] {
                    slider(ui, lang, label, v, min, max, &mut commit);
                }
                ui.small(
                    lang.text("Per valutare il dettaglio usa Verifica resa finale e zoom 100%."),
                );
                reset(ui, lang, &mut a.detail, &mut commit);
            });
        egui::CollapsingHeader::new(lang.text("Colore avanzato"))
            .id_salt("advanced-color")
            .show(ui, |ui| {
                commit |= ui
                    .checkbox(&mut a.color.monochrome, lang.text("Bianco e nero"))
                    .changed();
                for (i, label) in [
                    "Rosso",
                    "Arancio",
                    "Giallo",
                    "Verde",
                    "Acquamarina",
                    "Blu",
                    "Viola",
                    "Magenta",
                ]
                .iter()
                .enumerate()
                {
                    egui::CollapsingHeader::new(lang.text(label))
                        .id_salt(("hsl", i))
                        .show(ui, |ui| {
                            let b = &mut a.color.bands[i];
                            for (name, v) in [
                                ("Tonalità", &mut b.hue),
                                ("Saturazione", &mut b.saturation),
                                ("Luminanza", &mut b.luminance),
                            ] {
                                slider(ui, lang, name, v, -100., 100., &mut commit);
                            }
                        });
                }
                for (i, label) in ["Grading ombre", "Grading mezzitoni", "Grading luci"]
                    .iter()
                    .enumerate()
                {
                    egui::CollapsingHeader::new(lang.text(label))
                        .id_salt(("grading", i))
                        .show(ui, |ui| {
                            slider(
                                ui,
                                lang,
                                "Tonalità (°)",
                                &mut a.color.grading[i].hue,
                                0.,
                                360.,
                                &mut commit,
                            );
                            slider(
                                ui,
                                lang,
                                "Intensità",
                                &mut a.color.grading[i].amount,
                                0.,
                                100.,
                                &mut commit,
                            );
                        });
                }
                for (i, label) in ["Curva rosso", "Curva verde", "Curva blu"]
                    .iter()
                    .enumerate()
                {
                    slider(
                        ui,
                        lang,
                        label,
                        &mut a.color.rgb_midtones[i],
                        -100.,
                        100.,
                        &mut commit,
                    );
                }
                reset(ui, lang, &mut a.color, &mut commit);
            });
        egui::CollapsingHeader::new(lang.text("Maschere locali"))
            .id_salt("local-masks")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            a.masks.len() < 16,
                            egui::Button::new(lang.text("Aggiungi maschera")),
                        )
                        .clicked()
                    {
                        a.masks.push(Mask::default());
                        self.mask = a.masks.len() - 1;
                        commit = true;
                    }
                    if !a.masks.is_empty() && ui.button(lang.text("Elimina maschera")).clicked() {
                        a.masks.remove(self.mask.min(a.masks.len() - 1));
                        self.mask = self.mask.saturating_sub(1);
                        commit = true;
                    }
                });
                if a.masks.is_empty() {
                    self.paint = false;
                    return;
                }
                self.mask = self.mask.min(a.masks.len() - 1);
                egui::ComboBox::from_id_salt("mask-index")
                    .selected_text(format!("{} {}", lang.text("Maschera"), self.mask + 1))
                    .show_ui(ui, |ui| {
                        for i in 0..a.masks.len() {
                            ui.selectable_value(
                                &mut self.mask,
                                i,
                                format!("{} {}", lang.text("Maschera"), i + 1),
                            );
                        }
                    });
                let m = &mut a.masks[self.mask];
                commit |= ui
                    .checkbox(&mut m.enabled, lang.text("Attiva maschera"))
                    .changed();
                ui.horizontal_wrapped(|ui| {
                    for (shape, label) in [
                        (Shape::Radial, "Radiale"),
                        (Shape::Linear, "Gradiente"),
                        (Shape::Brush, "Pennello"),
                        (Shape::Luminance, "Luminanza"),
                        (Shape::Hue, "Tonalità"),
                    ] {
                        commit |= ui
                            .selectable_value(&mut m.shape, shape, lang.text(label))
                            .changed();
                    }
                });
                if matches!(m.shape, Shape::Radial | Shape::Linear | Shape::Brush) {
                    if ui
                        .checkbox(&mut self.paint, lang.text("Disegna sulla foto"))
                        .changed()
                        && self.paint
                    {
                        self.horizon = false;
                        self.horizon_start = None;
                    }
                    ui.checkbox(&mut self.overlay, lang.text("Mostra area maschera"));
                    ui.small(
                        lang.text("Trascina sulla foto; disattiva Disegna per spostare la vista."),
                    );
                    slider(ui, lang, "Centro X", &mut m.center[0], 0., 1., &mut commit);
                    slider(ui, lang, "Centro Y", &mut m.center[1], 0., 1., &mut commit);
                    slider(
                        ui,
                        lang,
                        "Raggio maschera",
                        &mut m.radius,
                        0.005,
                        1.,
                        &mut commit,
                    );
                    if m.shape == Shape::Linear {
                        slider(
                            ui,
                            lang,
                            "Angolo gradiente",
                            &mut m.angle,
                            -180.,
                            180.,
                            &mut commit,
                        );
                    }
                    if m.shape == Shape::Brush
                        && ui.button(lang.text("Cancella pennellate")).clicked()
                    {
                        m.points.clear();
                        m.breaks.clear();
                        commit = true;
                    }
                } else {
                    self.paint = false;
                    let [lo, hi] = m.interval;
                    slider(
                        ui,
                        lang,
                        "Intervallo minimo",
                        &mut m.interval[0],
                        0.,
                        hi - 0.001,
                        &mut commit,
                    );
                    slider(
                        ui,
                        lang,
                        "Intervallo massimo",
                        &mut m.interval[1],
                        lo + 0.001,
                        1.,
                        &mut commit,
                    );
                }
                slider(ui, lang, "Sfumatura", &mut m.feather, 0.01, 1., &mut commit);
                commit |= ui
                    .checkbox(&mut m.invert, lang.text("Inverti maschera"))
                    .changed();
                slider(
                    ui,
                    lang,
                    "Esposizione locale (EV)",
                    &mut m.exposure,
                    -5.,
                    5.,
                    &mut commit,
                );
                slider(
                    ui,
                    lang,
                    "Temperatura locale",
                    &mut m.warmth,
                    -100.,
                    100.,
                    &mut commit,
                );
                slider(
                    ui,
                    lang,
                    "Saturazione locale",
                    &mut m.saturation,
                    -100.,
                    100.,
                    &mut commit,
                );
                ui.small(lang.text("Maschere salvate con la foto, prima di ritaglio e rotazione."));
            });
        let changed = a != before;
        if changed {
            recipe.advanced = Some(a);
            recipe.process_version = 3;
        }
        (changed, commit)
    }
}

impl super::TrueRenderer {
    fn photo_edit_ready(&self, id: &str) -> bool {
        self.state.current.as_deref() == Some(id)
            && !self.editing.show_original
            && !self.editing.wb_pending
            && self.editing.entries.get(id).is_some_and(|entry| {
                !entry.loading && !entry.pending && entry.loaded.is_some() && entry.draft.is_some()
            })
    }

    pub(in crate::ui) fn photo_gesture_active(&self, id: &str) -> bool {
        self.photo_edit_ready(id)
            && (self.editing.advanced.horizon
                || (self.editing.advanced.paint
                    && self.editing.entries[id]
                        .draft
                        .as_ref()
                        .and_then(|r| r.advanced.as_ref())
                        .and_then(|a| a.masks.get(self.editing.advanced.mask))
                        .is_some_and(|m| {
                            matches!(m.shape, Shape::Radial | Shape::Linear | Shape::Brush)
                        })))
    }

    pub(in crate::ui) fn local_mask_interaction(
        &mut self,
        ui: &egui::Ui,
        item: &super::Item,
        response: &egui::Response,
        native: [u32; 2],
    ) {
        // Match the Develop panel's busy state and target only its selected
        // photo. The other comparison pane must remain a view, not an editor.
        if !self.photo_edit_ready(&item.id) {
            return;
        }
        // Gesture coordinates depend on geometry and the viewport, never on
        // whether the asynchronous renderer currently supplies a pixel sample.
        let output = self.edited_source_size(&item.id, native);
        let ppp = ui.ctx().pixels_per_point();
        let size = output.map(|v| v as f32);
        let scale =
            self.state
                .transform
                .scale(size, [response.rect.width(), response.rect.height()], ppp);
        let physical = egui::vec2(
            (size[0] * scale * ppp).round().max(1.),
            (size[1] * scale * ppp).round().max(1.),
        ) / ppp;
        let top = response.rect.center()
            - egui::vec2(
                physical.x * self.state.transform.center[0],
                physical.y * self.state.transform.center[1],
            );
        let top = egui::pos2((top.x * ppp).round() / ppp, (top.y * ppp).round() / ppp);
        let image_rect = egui::Rect::from_min_size(top, physical);
        let point_at = |pointer: egui::Pos2| {
            Some(pointer)
                .filter(|p| image_rect.contains(*p) && response.rect.contains(*p))
                .map(|p| {
                    [
                        ((p.x - top.x) / physical.x) as f64,
                        ((p.y - top.y) / physical.y) as f64,
                    ]
                })
        };
        let point = response.interact_pointer_pos().and_then(point_at);
        // egui recognizes a drag only after crossing its movement threshold.
        // Retain the press position as well as that first moved position.
        let press = response
            .drag_started()
            .then(|| ui.input(|i| i.pointer.press_origin()))
            .flatten();
        let origin = press.and_then(point_at);
        if self.editing.advanced.horizon && !self.editing.show_original {
            if response.drag_started()
                && let (Some(point), Some(pointer)) = (origin, press)
            {
                self.editing.advanced.horizon_start = Some((
                    item.id.clone(),
                    [point[0] as f32 * size[0], point[1] as f32 * size[1]],
                    pointer,
                ));
            }
            if let Some((id, _, start)) = &self.editing.advanced.horizon_start
                && id == &item.id
                && let Some(pointer) = response.interact_pointer_pos()
            {
                ui.painter().with_clip_rect(response.rect).line_segment(
                    [*start, pointer],
                    egui::Stroke::new(2., egui::Color32::LIGHT_BLUE),
                );
            }
            if response.drag_stopped() {
                if let Some((id, start, _)) = self.editing.advanced.horizon_start.take()
                    && id == item.id
                    && let Some(point) = point
                {
                    let dx = point[0] as f32 * size[0] - start[0];
                    let dy = point[1] as f32 * size[1] - start[1];
                    if dx * dx + dy * dy > 16. {
                        let correction = (dy.atan2(dx).to_degrees() + 45.).rem_euclid(90.) - 45.;
                        if let Some(mut recipe) =
                            self.editing.entries.get(&id).and_then(|e| e.draft.clone())
                        {
                            let a = recipe.advanced.get_or_insert_with(Default::default);
                            a.geometry.angle = (a.geometry.angle - correction).clamp(-45., 45.);
                            recipe.process_version = 3;
                            self.apply_edit_draft(&id, recipe);
                            self.commit_edit(&id);
                        }
                    }
                }
                self.editing.advanced.horizon = false;
            }
            return;
        }
        if response.drag_started() || response.clicked() {
            self.editing.advanced.new_stroke = true;
        }
        if self.editing.show_original {
            return;
        }
        let Some(entry) = self.editing.entries.get(&item.id) else {
            return;
        };
        if entry.pending {
            return;
        }
        let Some(mut recipe) = entry.draft.clone() else {
            return;
        };
        let Some(a) = &mut recipe.advanced else {
            return;
        };
        let index = self.editing.advanced.mask;
        let mut count = a.masks.iter().map(|m| m.points.len()).sum::<usize>();
        let Some(mask) = a.masks.get_mut(index) else {
            return;
        };
        if self.editing.advanced.overlay
            && matches!(mask.shape, Shape::Radial | Shape::Linear | Shape::Brush)
        {
            let painter = ui.painter().with_clip_rect(response.rect);
            for y in 0..40 {
                for x in 0..40 {
                    let uv = [(x as f64 + 0.5) / 40., (y as f64 + 0.5) / 40.];
                    let Some(p) = a.geometry.source_point(uv, native) else {
                        continue;
                    };
                    let weight = mask.weight(
                        p.map(|v| v as f32),
                        [0.5; 3],
                        native[1] as f32 / native[0] as f32,
                    );
                    if weight > 0.01 {
                        let min = top
                            + egui::vec2(x as f32 / 40. * physical.x, y as f32 / 40. * physical.y);
                        painter.rect_filled(
                            egui::Rect::from_min_size(min, physical / 40.),
                            0,
                            egui::Color32::from_rgba_unmultiplied(
                                230,
                                65,
                                75,
                                (weight * 65.) as u8,
                            ),
                        );
                    }
                }
            }
        }
        if !self.photo_gesture_active(&item.id) {
            return;
        }
        if response.dragged() || response.drag_stopped() || response.clicked() {
            let mut changed = false;
            for p in [origin.filter(|_| mask.shape == Shape::Brush), point]
                .into_iter()
                .flatten()
                .filter_map(|p| a.geometry.source_point(p, native))
                .filter(|p| p.iter().all(|v| (0. ..=1.).contains(v)))
            {
                let p = p.map(|v| v as f32);
                if mask.shape == Shape::Brush {
                    if count >= 512 {
                        self.status = self
                            .cache_settings
                            .language
                            .text("Massimo 512 punti di pennello per ricetta")
                            .into();
                    } else if self.editing.advanced.new_stroke
                        || mask
                            .points
                            .last()
                            .is_none_or(|q| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() > 0.001)
                    {
                        if !mask.points.is_empty() && self.editing.advanced.new_stroke {
                            mask.breaks.push(mask.points.len());
                        }
                        mask.points.push(p);
                        count += 1;
                        self.editing.advanced.new_stroke = false;
                        changed = true;
                    }
                } else if mask.center != p {
                    mask.center = p;
                    changed = true;
                }
            }
            if changed {
                self.apply_edit_draft(&item.id, recipe);
            }
        }
        if response.drag_stopped() || response.clicked() {
            self.commit_edit(&item.id);
        }
    }
}
