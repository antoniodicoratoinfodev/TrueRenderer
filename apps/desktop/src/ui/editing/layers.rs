use super::{EditRecipe, adjustment_default};
use crate::i18n::Language;
use eframe::egui;
use tr_core::editing::{
    layers::{self, Combine, Id, Layer, MaskKind, Operator, TOOLS},
    masks::{Mask, Shape},
};
mod curves;
mod selective;
mod toning;

#[derive(Default)]
pub(in crate::ui) struct Controls {
    pub photo: String,
    pub view_layers: bool,
    pub selected: Option<Id>,
    pub component: Option<Id>,
    pub pick: bool,
    pick_component: bool,
    sample_operator: usize,
    level_pick: Option<(usize, usize, layers::LevelsAction)>,
    analysis_operator: Option<usize>,
    analysis_source: String,
    input_analysis: Option<curves::InputAnalysis>,
    pending_auto: Option<(usize, layers::LevelsAction)>,
    load_native: bool,
    pub overlay: bool,
    pub paint: bool,
    search: String,
    search_index: usize,
    channel: usize,
    band: usize,
    grade_zone: usize,
    gradient_stop: usize,
    selective_family: usize,
    new_stroke: bool,
    gesture_start: Option<EditRecipe>,
    anchor: Option<[f32; 2]>,
    error: Option<String>,
}

pub(super) fn color_wheel(
    ui: &mut egui::Ui,
    lang: Language,
    hue: &mut f32,
    amount: &mut f32,
) -> egui::Response {
    let side = 132_f32.min(ui.available_width());
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let radius = side / 2. - 7.;
    let center = rect.center();
    let before = (*hue, *amount);
    if ui.is_enabled() {
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let delta = pos - center;
            *amount = (delta.length() / radius * 100.).clamp(0., 100.);
            if *amount > 0.01 {
                *hue = (-delta.y).atan2(delta.x).to_degrees().rem_euclid(360.);
            }
        }
        if response.double_clicked() {
            *hue = 0.;
            *amount = 0.;
        }
        if response.has_focus() {
            for (key, dh, da) in [
                (egui::Key::ArrowLeft, -1., 0.),
                (egui::Key::ArrowRight, 1., 0.),
                (egui::Key::ArrowUp, 0., 1.),
                (egui::Key::ArrowDown, 0., -1.),
            ] {
                // Use each event's modifiers, not the final frame snapshot:
                // Shift can be released before the queued arrow is processed.
                let steps = ui.input_mut(|i| {
                    10 * i.count_and_consume_key(egui::Modifiers::SHIFT, key)
                        + i.count_and_consume_key(egui::Modifiers::NONE, key)
                }) as f32;
                if steps != 0. {
                    *hue = (*hue + dh * steps).rem_euclid(360.);
                    *amount = (*amount + da * steps).clamp(0., 100.);
                }
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Home)) {
                *hue = 0.;
                *amount = 0.;
            }
        }
    }
    if before != (*hue, *amount) {
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(center, egui::Color32::from_gray(150));
        for n in 0..=72 {
            let f = n as f32 / 72.;
            let angle = f * std::f32::consts::TAU;
            let color =
                egui::ecolor::Hsva::new(f, 0.78, if ui.is_enabled() { 0.8 } else { 0.4 }, 1.);
            mesh.colored_vertex(
                center + radius * egui::vec2(angle.cos(), -angle.sin()),
                color.into(),
            );
            if n > 0 {
                mesh.add_triangle(0, n, n + 1);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        let angle = hue.to_radians();
        let handle = center + radius * *amount / 100. * egui::vec2(angle.cos(), -angle.sin());
        ui.painter()
            .circle_stroke(handle, 5., egui::Stroke::new(3., egui::Color32::BLACK));
        ui.painter()
            .circle_stroke(handle, 5., egui::Stroke::new(1.5, egui::Color32::WHITE));
        if response.has_focus() {
            ui.painter()
                .circle_stroke(center, radius + 4., ui.visuals().selection.stroke);
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Slider,
            ui.is_enabled(),
            lang.text("Ruota cromatica"),
        )
    });
    response.on_hover_text(lang.text("Trascina: tonalità e cromia. Frecce: regolazione fine; Maiusc: passo 10; Home o doppio clic: azzera."))
}

fn slider(
    ui: &mut egui::Ui,
    lang: Language,
    name: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    commit: &mut bool,
) {
    // Reading a valid extended-domain recipe must never clamp its saved values
    // to the compact slider's default display interval.
    let range = range.start().min(*v)..=range.end().max(*v);
    let r = adjustment_default(ui, lang.text(name), v, range, "", None, default).0;
    *commit |= r.drag_stopped() || (r.changed() && !r.dragged());
}

fn mask_rows(graph: &layers::MaskGraph) -> Vec<(usize, Id)> {
    let mut rows = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut pending: Vec<_> = graph.root.map(|id| (0, id)).into_iter().collect();
    while let Some((depth, id)) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = graph.nodes.iter().find(|n| n.id == id) else {
            continue;
        };
        rows.push((depth, id));
        match node.kind {
            MaskKind::Add(a, b) | MaskKind::Subtract(a, b) | MaskKind::Intersect(a, b) => {
                pending.push((depth + 1, b));
                pending.push((depth + 1, a));
            }
            MaskKind::Invert(a) => pending.push((depth + 1, a)),
            _ => (),
        }
    }
    rows
}

impl Controls {
    pub fn bind(&mut self, photo: &str) {
        if self.photo != photo {
            *self = Self {
                photo: photo.into(),
                view_layers: self.view_layers,
                ..Default::default()
            };
        }
    }
    pub fn preview_input(&self, id: &str, recipe: &mut EditRecipe) {
        if self.photo == id
            && self.view_layers
            && self.analysis_operator.is_some()
            && !self.overlay
            && !self.pick_component
            && let Some(input) = self.analysis_preview(recipe)
        {
            *recipe = input;
            return;
        }
        if self.photo == id
            && self.view_layers
            && (self.pick || self.overlay)
            && let Some(stack) = &mut recipe.layers
            && let Some(i) = stack
                .layers
                .iter()
                .position(|l| Some(l.id) == self.selected)
        {
            stack.layers.truncate(i);
        }
    }
    pub fn gesture_active(&self, id: &str, recipe: &EditRecipe) -> bool {
        self.photo == id && self.view_layers
            && recipe.layers.as_ref().and_then(|s| s.layers.iter().find(|l| Some(l.id)==self.selected))
            .is_some_and(|l| !l.locked && (self.pick || (self.paint && l.mask.nodes.iter().any(|n| Some(n.id)==self.component && matches!(&n.kind,MaskKind::Shape(m) if matches!(m.shape,Shape::Brush|Shape::Linear|Shape::Radial))))))
    }
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
        ui.add_enabled_ui(count < layers::MAX_LAYERS, |ui| {
            ui.menu_button(lang.text("Aggiungi regolazione"), |ui| {
                ui.set_min_width(280.);
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(lang.text("Cerca strumento…")),
                );
                if search.changed() {
                    self.search_index = 0;
                }
                let query = self.search.to_lowercase();
                let matches: Vec<_> = TOOLS
                    .iter()
                    .filter(|tool| {
                        let haystack =
                            format!("{} {} {}", tool.name, lang.text(tool.name), tool.keywords)
                                .to_lowercase();
                        query.split_whitespace().all(|word| haystack.contains(word))
                    })
                    .collect();
                if search.has_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        self.search_index = self.search_index.saturating_add(1);
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        self.search_index = self.search_index.saturating_sub(1);
                    }
                }
                self.search_index = self.search_index.min(matches.len().saturating_sub(1));
                let activate = (search.has_focus() || search.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                for (i, tool) in matches.into_iter().enumerate() {
                    if ui
                        .selectable_label(i == self.search_index, lang.text(tool.name))
                        .on_hover_text(lang.text(tool.description))
                        .clicked()
                        || (activate && i == self.search_index)
                    {
                        let layer = Layer::new(lang.text(tool.name), tool.operator());
                        self.selected = Some(layer.id);
                        self.component = None;
                        self.paint = false;
                        self.pick = false;
                        self.overlay = false;
                        recipe.layer_stack().layers.push(layer);
                        commit = true;
                        ui.close();
                    }
                }
            });
        });
        if recipe.layers.is_none() {
            ui.label(lang.text("Aggiungi una regolazione per creare un livello fotografico."));
            return (false, false);
        }
        let stack = recipe.layers.as_mut().unwrap();
        if !stack.layers.iter().any(|l| Some(l.id) == self.selected) {
            self.cancel_analysis();
            self.selected = stack.layers.last().map(|l| l.id);
            self.component = None;
            self.pick = false;
            self.paint = false;
            self.overlay = false;
        }
        let mut action = None;
        // The top row is the last operation, matching the actual evaluator.
        for i in (0..stack.layers.len()).rev() {
            let layer = &mut stack.layers[i];
            ui.push_id(layer.id, |ui| {
                ui.horizontal(|ui| {
                    commit |= ui
                        .checkbox(&mut layer.enabled, "")
                        .on_hover_text(lang.text("Visibilità livello"))
                        .changed();
                    let r = ui.add_sized(
                        [ui.available_width() - 34., 32.],
                        egui::Button::new(&layer.name).selected(self.selected == Some(layer.id)),
                    );
                    if r.clicked() && self.selected != Some(layer.id) {
                        self.cancel_analysis();
                        self.selected = Some(layer.id);
                        self.component = None;
                        self.pick = false;
                        self.overlay = false;
                        self.paint = false;
                    }
                    ui.menu_button("…", |ui| {
                        if ui
                            .checkbox(&mut layer.locked, lang.text("Blocca modifiche"))
                            .changed()
                        {
                            commit = true;
                        }
                        for (code, label, enabled) in [
                            (0, "Duplica livello", count < layers::MAX_LAYERS),
                            (1, "Sposta sopra", i + 1 < count && !layer.locked),
                            (2, "Sposta sotto", i > 0 && !layer.locked),
                            (3, "Elimina livello", !layer.locked),
                        ] {
                            if ui
                                .add_enabled(enabled, egui::Button::new(lang.text(label)))
                                .clicked()
                            {
                                action = Some((i, code));
                                ui.close();
                            }
                        }
                    });
                });
            });
        }
        ui.label(lang.text("Base fotografica · sorgente in sola lettura"));
        if let Some((i, action)) = action {
            self.cancel_analysis();
            match action {
                0 => {
                    let l = stack.layers[i].duplicate();
                    self.selected = Some(l.id);
                    stack.layers.insert(i + 1, l);
                }
                1 => stack.layers.swap(i, i + 1),
                2 => stack.layers.swap(i, i - 1),
                _ => {
                    stack.layers.remove(i);
                    self.selected = None;
                }
            }
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
            ui.label(format!("{}: {}", lang.text("Modifica livello"), layer.name));
            ui.add_enabled_ui(!layer.locked, |ui| {
                let rename = ui.text_edit_singleline(&mut layer.name);
                commit |= rename.lost_focus();
                slider(
                    ui,
                    lang,
                    "Intensità livello",
                    &mut layer.opacity,
                    0. ..=1.,
                    1.,
                    &mut commit,
                );
                ui.small(lang.text("Fusione normale · prima di ritaglio e rotazione"));
                for (i, op) in layer.operators.iter_mut().enumerate() {
                    ui.push_id((layer.id, "operator", i), |ui| {
                        ui.heading(lang.text(op.tool().name));
                        self.operator(ui, lang, op, i, &mut commit);
                    });
                }
                ui.separator();
                self.mask_controls(ui, lang, layer, &mut commit);
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
            ui.colored_label(super::AMBER, lang.text(error));
        }
        (recipe != &before, commit)
    }
    fn operator(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        op: &mut Operator,
        index: usize,
        commit: &mut bool,
    ) {
        match op {
            Operator::SampleColor(s) => {
                ui.horizontal_wrapped(|ui| {
                    let selected = self.pick
                        && !self.pick_component
                        && self.level_pick.is_none()
                        && self.sample_operator == index;
                    if ui
                        .selectable_label(selected, lang.text("Campiona sulla foto"))
                        .clicked()
                    {
                        self.cancel_analysis();
                        self.pick = !selected;
                        self.pick_component = false;
                        self.sample_operator = index;
                        self.paint = false;
                        self.overlay = false;
                    }
                    ui.checkbox(&mut self.overlay, lang.text("Mostra area"));
                });
                if self.pick {
                    ui.label(
                        lang.text(
                            "Clic sulla foto: campione all'ingresso del livello. Esc annulla.",
                        ),
                    );
                }
                ui.small(lang.text("Intervallo: tonalità, cromia e luminanza Rec.2020 lineari"));
                egui::CollapsingHeader::new(lang.text("Intervallo del campione")).show(ui, |ui| {
                    for (i, name) in ["Rosso", "Verde", "Blu"].into_iter().enumerate() {
                        slider(
                            ui,
                            lang,
                            name,
                            &mut s.range.reference[i],
                            -1. ..=4.,
                            0.,
                            commit,
                        );
                    }
                    slider(
                        ui,
                        lang,
                        "Ampiezza tonalità (°)",
                        &mut s.range.width[0],
                        0.1..=180.,
                        40.,
                        commit,
                    );
                    slider(
                        ui,
                        lang,
                        "Ampiezza cromia",
                        &mut s.range.width[1],
                        0.0001..=4.,
                        0.4,
                        commit,
                    );
                    slider(
                        ui,
                        lang,
                        "Ampiezza luminanza",
                        &mut s.range.width[2],
                        0.0001..=4.,
                        0.4,
                        commit,
                    );
                    slider(
                        ui,
                        lang,
                        "Sfumatura",
                        &mut s.range.softness,
                        0.01..=1.,
                        0.5,
                        commit,
                    );
                });
                for (i, name) in ["Tonalità (°)", "Saturazione", "Luminanza"]
                    .into_iter()
                    .enumerate()
                {
                    let limit = if i == 0 { 180. } else { 100. };
                    slider(
                        ui,
                        lang,
                        name,
                        &mut s.correction[i],
                        -limit..=limit,
                        0.,
                        commit,
                    );
                }
                egui::CollapsingHeader::new(lang.text("Uniformità verso il campione")).show(
                    ui,
                    |ui| {
                        for (i, name) in [
                            "Uniformità tonalità",
                            "Uniformità cromia",
                            "Uniformità luminanza",
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            slider(ui, lang, name, &mut s.uniformity[i], 0. ..=100., 0., commit);
                        }
                    },
                );
            }
            Operator::Light {
                exposure,
                temperature,
                tint,
                saturation,
            } => {
                slider(
                    ui,
                    lang,
                    "Esposizione (EV)",
                    exposure,
                    -10. ..=10.,
                    0.,
                    commit,
                );
                for (name, v) in [
                    ("Temperatura RGB", temperature),
                    ("Tinta RGB", tint),
                    ("Saturazione", saturation),
                ] {
                    slider(ui, lang, name, v, -100. ..=100., 0., commit);
                }
            }
            Operator::Curves(curves) => {
                self.channel_selector(ui, lang);
                ui.small(lang.text("RGB lineare · interpolazione monotona a tratti"));
                ui.push_id(self.channel, |ui| {
                    *commit |= super::curve::controls_with_label(
                        ui,
                        lang,
                        &mut curves[self.channel],
                        "Curva a punti · RGB lineare",
                    )
                    .1;
                });
            }
            Operator::Levels(levels) => {
                let mut g = layers::LevelAdjustments {
                    channels: *levels,
                    ..Default::default()
                };
                self.tonal_levels(ui, lang, &mut g, index, commit);
                *levels = g.channels;
            }
            Operator::TonalLevels(g) => self.tonal_levels(ui, lang, g, index, commit),
            Operator::LuminanceCurve(g) => self.luminance_curve(ui, lang, g, commit),
            Operator::ParametricCurve(g) => self.parametric_curve(ui, lang, g, commit),
            Operator::Mixer(m) => {
                *commit |= ui
                    .checkbox(&mut m.monochrome, lang.text("Bianco e nero"))
                    .changed();
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in [
                        "Rosso",
                        "Arancio",
                        "Giallo",
                        "Verde",
                        "Acquamarina",
                        "Blu",
                        "Viola",
                        "Magenta",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.band, i, lang.text(name));
                    }
                });
                let b = &mut m.bands[self.band];
                for (name, v) in [
                    ("Tonalità", &mut b.hue),
                    ("Saturazione", &mut b.saturation),
                    ("Luminanza", &mut b.luminance),
                ] {
                    slider(ui, lang, name, v, -100. ..=100., 0., commit);
                }
            }
            Operator::Channels { matrix, offset } => {
                ui.small(lang.text("Coefficienti RGB · valori negativi conservati"));
                for i in 0..3 {
                    ui.push_id(i, |ui| {
                        ui.label(["R", "G", "B"][i]);
                        for j in 0..3 {
                            slider(
                                ui,
                                lang,
                                ["R", "G", "B"][j],
                                &mut matrix[i][j],
                                -4. ..=4.,
                                u8::from(i == j) as f32,
                                commit,
                            );
                        }
                        slider(ui, lang, "Offset", &mut offset[i], -4. ..=4., 0., commit);
                    });
                }
            }
            Operator::Grading(g) => self.grading(ui, lang, g, commit),
            Operator::BlackAndWhite(g) => self.black_white(ui, lang, g, commit),
            Operator::ColorFilter(g) => self.color_filter(ui, lang, g, commit),
            Operator::GradientMap(g) => self.gradient_map(ui, lang, g, commit),
            Operator::SelectiveColor(g) => self.selective_color(ui, lang, g, commit),
            Operator::Colorize(g) => self.colorize(ui, lang, g, commit),
            Operator::TonalAdjustments(g) => self.tonal(ui, lang, g, commit),
            Operator::ExposureGamma(g) => self.exposure_gamma(ui, lang, g, commit),
        }
        ui.menu_button(lang.text("Azioni regolazione"), |ui| {
            if ui.button(lang.text("Ripristina sezione")).clicked() {
                self.cancel_analysis();
                *op = op.tool().operator();
                *commit = true;
                ui.close();
            }
        });
    }
    fn channel_selector(&mut self, ui: &mut egui::Ui, lang: Language) {
        let before = self.channel;
        ui.horizontal_wrapped(|ui| {
            for (i, name) in ["RGB", "Rosso", "Verde", "Blu"].into_iter().enumerate() {
                ui.selectable_value(&mut self.channel, i, lang.text(name));
            }
        });
        if before != self.channel && self.level_pick.is_some() {
            self.level_pick = None;
            self.pick = false;
        }
    }
    fn mask_controls(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        layer: &mut Layer,
        commit: &mut bool,
    ) {
        ui.strong(lang.text("Maschera del livello"));
        let mut add = None;
        ui.horizontal_wrapped(|ui| {
            for (combine, name) in [
                (Combine::Add, "Aggiungi"),
                (Combine::Subtract, "Sottrai"),
                (Combine::Intersect, "Interseca"),
            ] {
                ui.add_enabled_ui(layer.mask.nodes.len() + 2 <= layers::MAX_MASK_NODES, |ui| {
                    ui.menu_button(lang.text(name), |ui| {
                        for (shape, label) in [
                            (Shape::Brush, "Pennello"),
                            (Shape::Linear, "Gradiente"),
                            (Shape::Radial, "Radiale"),
                            (Shape::Luminance, "Luminanza"),
                        ] {
                            if ui.button(lang.text(label)).clicked() {
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
                        if ui.button(lang.text("Colore a campioni")).clicked() {
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
            *commit = true;
        }
        if layer.mask.nodes.is_empty() {
            ui.label(lang.text("Maschera bianca · foto intera"));
        }
        let mut remove = None;
        for (depth, id) in mask_rows(&layer.mask) {
            let node = layer.mask.nodes.iter_mut().find(|n| n.id == id).unwrap();
            let label = match &node.kind {
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
            };
            ui.push_id(node.id, |ui| {
                let selected = ui
                    .horizontal(|ui| {
                        ui.add_space(depth as f32 * 10.);
                        ui.selectable_label(self.component == Some(node.id), lang.text(label))
                            .clicked()
                    })
                    .inner;
                if selected {
                    self.component = Some(node.id);
                    self.paint = false;
                    self.pick = false;
                }
                if self.component != Some(node.id) {
                    return;
                }
                if let MaskKind::Color(c) = &mut node.kind {
                    if ui
                        .selectable_label(
                            self.pick && self.pick_component,
                            lang.text("Campiona sulla foto"),
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
                        ui.checkbox(&mut self.paint, lang.text("Disegna sulla foto"));
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
                    *commit |= ui
                        .checkbox(&mut m.invert, lang.text("Inverti maschera"))
                        .changed();
                }
                if ui.button(lang.text("Elimina componente")).clicked() {
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
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    layer.mask.nodes.len() < layers::MAX_MASK_NODES,
                    egui::Button::new(lang.text("Inverti maschera")),
                )
                .clicked()
            {
                layer.mask.invert();
                *commit = true;
            }
            ui.checkbox(&mut self.overlay, lang.text("Mostra area"));
            if ui.button(lang.text("Fine selezione")).clicked() {
                self.cancel_analysis();
                self.paint = false;
                self.pick = false;
                self.overlay = false;
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

impl super::TrueRenderer {
    pub(in crate::ui) fn layer_interaction(
        &mut self,
        ui: &egui::Ui,
        item: &super::Item,
        response: &egui::Response,
        native: [u32; 2],
        image: Option<&tr_core::provider::ImageLevels>,
        sample: Option<&tr_render::Sample>,
    ) {
        if !self.photo_edit_ready(&item.id)
            || self.editing.layers.photo != item.id
            || !self.editing.layers.view_layers
        {
            return;
        }
        let Some(mut recipe) = self.editing.entries[&item.id].draft.clone() else {
            return;
        };
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(original) = self.editing.layers.gesture_start.take() {
                self.apply_edit_draft(&item.id, original);
            }
            self.editing.layers.pick = false;
            self.editing.layers.cancel_analysis();
            self.editing.layers.paint = false;
            self.editing.layers.overlay = false;
            return;
        }
        if let Some(image) = image {
            self.editing.layers.prepare_analysis(&recipe, image);
        }
        if self.editing.layers.pick {
            if response.clicked() {
                if image.is_some_and(|i| i.base_level() == 0)
                    && let Some(p) = sample.filter(|p| p.working[3] > 0.01)
                {
                    let rgb = std::array::from_fn(|c| p.working[c] / p.working[3]);
                    if self.editing.layers.level_pick.is_some() {
                        match self.editing.layers.accept_level_sample(
                            &mut recipe,
                            image.unwrap(),
                            rgb,
                            [p.x, p.y],
                        ) {
                            Ok(()) => {
                                self.apply_edit_draft(&item.id, recipe);
                                self.commit_edit(&item.id);
                            }
                            Err(e) => self.editing.layers.error = Some(e.to_string()),
                        }
                        return;
                    }
                    if let Some(layer) = recipe.layers.as_mut().and_then(|s| {
                        s.layers
                            .iter_mut()
                            .find(|l| Some(l.id) == self.editing.layers.selected)
                    }) && !layer.locked
                    {
                        if self.editing.layers.pick_component {
                            if let Some(MaskKind::Color(c)) = layer
                                .mask
                                .nodes
                                .iter_mut()
                                .find(|n| Some(n.id) == self.editing.layers.component)
                                .map(|n| &mut n.kind)
                            {
                                c.reference = rgb;
                            }
                        } else if let Some(Operator::SampleColor(s)) =
                            layer.operators.get_mut(self.editing.layers.sample_operator)
                        {
                            s.range.reference = rgb;
                        }
                        self.editing.layers.pick = false;
                        self.editing.layers.error = None;
                        self.apply_edit_draft(&item.id, recipe);
                        self.commit_edit(&item.id);
                    }
                } else {
                    self.editing.layers.error = Some(
                        "Campione non pronto: usa Verifica resa finale e una zona non trasparente."
                            .into(),
                    );
                }
            }
            return;
        }
        let geometry = recipe
            .advanced
            .as_ref()
            .map(|a| a.geometry.clone())
            .unwrap_or_default();
        let output = geometry.output_size(native);
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
        let rect = egui::Rect::from_min_size(top, physical);
        let map = |p: egui::Pos2| {
            if !rect.contains(p) || !response.rect.contains(p) {
                return None;
            }
            geometry
                .source_point(
                    [
                        ((p.x - top.x) / physical.x) as f64,
                        ((p.y - top.y) / physical.y) as f64,
                    ],
                    native,
                )
                .filter(|p| p.iter().all(|v| (0. ..=1.).contains(v)))
                .map(|p| p.map(|v| v as f32))
        };
        let before = recipe.clone();
        let Some(layer) = recipe.layers.as_mut().and_then(|s| {
            s.layers
                .iter_mut()
                .find(|l| Some(l.id) == self.editing.layers.selected)
        }) else {
            return;
        };
        if self.editing.layers.overlay
            && let Some(image) = image
        {
            let raster = image.source();
            let painter = ui.painter().with_clip_rect(response.rect);
            for y in 0..48 {
                for x in 0..48 {
                    let uv = [(x as f64 + 0.5) / 48., (y as f64 + 0.5) / 48.];
                    let Some(xy) = geometry.source_point(uv, native) else {
                        continue;
                    };
                    let px = (uv[0] * raster.width as f64) as usize;
                    let py = (uv[1] * raster.height as f64) as usize;
                    let p = raster.pixels[py * raster.width as usize + px];
                    if p[3] <= 0.01 {
                        continue;
                    }
                    let guide = std::array::from_fn(|c| p[c] / p[3]);
                    let mut w = layer.mask.weight(
                        xy.map(|v| v as f32),
                        guide,
                        native[1] as f32 / native[0] as f32,
                    );
                    if let Some(Operator::SampleColor(s)) = layer.operators.first() {
                        w *= s.range.weight(guide);
                    }
                    if w > 0.01 {
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                top + egui::vec2(
                                    x as f32 / 48. * physical.x,
                                    y as f32 / 48. * physical.y,
                                ),
                                physical / 48.,
                            ),
                            0,
                            egui::Color32::from_rgba_unmultiplied(40, 180, 240, (w * 85.) as u8),
                        );
                    }
                }
            }
        }
        if layer.locked || !self.editing.layers.paint {
            return;
        }
        let Some(MaskKind::Shape(mask)) = layer
            .mask
            .nodes
            .iter_mut()
            .find(|n| Some(n.id) == self.editing.layers.component)
            .map(|n| &mut n.kind)
        else {
            return;
        };
        if !matches!(mask.shape, Shape::Brush | Shape::Radial | Shape::Linear) {
            return;
        }
        let origin = (response.drag_started() || response.clicked())
            .then(|| {
                ui.input(|i| i.pointer.press_origin())
                    .or(response.interact_pointer_pos())
            })
            .flatten()
            .and_then(map);
        let point = response.interact_pointer_pos().and_then(map);
        if origin.is_some() {
            self.editing.layers.gesture_start = Some(before.clone());
            self.editing.layers.anchor = origin;
            self.editing.layers.new_stroke = true;
        }
        if let Some(pointer) = response.hover_pos() {
            let radius = mask.radius * physical.x;
            let painter = ui.painter().with_clip_rect(response.rect);
            for r in [radius, radius * (1. - mask.feather)] {
                painter.circle_stroke(pointer, r, egui::Stroke::new(1., egui::Color32::WHITE));
            }
        }
        if response.dragged() || response.clicked() || response.drag_stopped() {
            if mask.shape == Shape::Brush {
                let count = before
                    .advanced
                    .as_ref()
                    .map_or(0, |a| a.masks.iter().map(|m| m.points.len()).sum::<usize>())
                    + before.layers.as_ref().map_or(0, |s| s.point_count());
                let mut remaining = 512_usize.saturating_sub(count);
                for p in [origin, point].into_iter().flatten() {
                    if remaining == 0 {
                        self.editing.layers.error =
                            Some("Massimo 512 punti di pennello per ricetta".into());
                        break;
                    }
                    if self.editing.layers.new_stroke
                        || mask
                            .points
                            .last()
                            .is_none_or(|q| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() > 0.001)
                    {
                        if self.editing.layers.new_stroke && !mask.points.is_empty() {
                            mask.breaks.push(mask.points.len());
                        }
                        mask.points.push(p);
                        self.editing.layers.new_stroke = false;
                        remaining -= 1;
                    }
                }
            } else if let Some(p) = point {
                if let Some(start) = self.editing.layers.anchor {
                    mask.center = start;
                    let dx = p[0] - start[0];
                    let dy = (p[1] - start[1]) * native[1] as f32 / native[0] as f32;
                    let d = (dx * dx + dy * dy).sqrt();
                    if d > 0.005 {
                        mask.radius = d.min(1.);
                        if mask.shape == Shape::Linear {
                            mask.angle = dy.atan2(dx).to_degrees();
                        }
                    }
                } else {
                    mask.center = p;
                }
            }
            if recipe != before {
                self.apply_edit_draft(&item.id, recipe);
            }
        }
        if response.drag_stopped() || response.clicked() {
            self.editing.layers.gesture_start = None;
            self.editing.layers.anchor = None;
            self.commit_edit(&item.id);
        }
    }
}
