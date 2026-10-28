use super::{EditRecipe, adjustment_default, palette};
use super::{help, panels};
use crate::i18n::Language;
use eframe::egui;
use tr_core::editing::{
    layers::{self, Combine, Id, Layer, MaskKind, Operator, TOOLS},
    masks::{Mask, Shape},
};
mod curves;
mod document;
mod mask_panel;
mod panel;
mod selective;
mod toning;

#[derive(Default)]
pub(in crate::ui) struct Controls {
    pub photo: String,
    pub view_layers: bool,
    pub view_tools: bool,
    pub(super) mask_view: bool,
    scroll_to_layer: bool,
    apply_selected: bool,
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
    overlay_operator: Option<usize>,
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
    document_input: Option<(String, tr_core::provider::ImageLevels)>,
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
        for fraction in [0.5, 1.] {
            ui.painter().circle_stroke(
                center,
                radius * fraction,
                egui::Stroke::new(0.7, egui::Color32::from_white_alpha(55)),
            );
        }
        for n in 0..12 {
            let angle = n as f32 * std::f32::consts::TAU / 12.;
            let direction = egui::vec2(angle.cos(), -angle.sin());
            ui.painter().line_segment(
                [
                    center + direction * (radius + 1.),
                    center + direction * (radius + 4.),
                ],
                egui::Stroke::new(1., egui::Color32::from_gray(140)),
            );
        }
        for axis in [egui::vec2(1., 0.), egui::vec2(0., 1.)] {
            ui.painter().line_segment(
                [center - axis * 3., center + axis * 3.],
                egui::Stroke::new(1., egui::Color32::from_gray(215)),
            );
        }
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
    let r = adjustment_default(ui, lang, lang.text(name), v, range, "", None, default).0;
    *commit |= r.drag_stopped() || (r.changed() && !r.dragged());
}

pub(super) fn percent_slider(
    ui: &mut egui::Ui,
    lang: Language,
    name: &str,
    value: &mut f32,
    commit: &mut bool,
) {
    let mut percent = *value * 100.;
    let response = adjustment_default(
        ui,
        lang,
        lang.text(name),
        &mut percent,
        0. ..=100.,
        " %",
        None,
        100.,
    )
    .0;
    if response.changed() {
        *value = percent / 100.;
    }
    *commit |= response.drag_stopped() || (response.changed() && !response.dragged());
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
                view_tools: self.view_tools,
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
    fn overlay_control(&mut self, ui: &mut egui::Ui, lang: Language, operator: Option<usize>) {
        let mut active = self.overlay && self.overlay_operator == operator;
        if help::checkbox(ui, lang, "Mostra area", &mut active).changed() {
            self.cancel_analysis();
            self.pick = false;
            self.paint = false;
            self.overlay = active;
            self.overlay_operator = operator;
        }
    }
    pub(super) fn overlay_weight(
        &self,
        layer: &Layer,
        xy: [f32; 2],
        guide: [f32; 3],
        aspect: f32,
    ) -> f32 {
        let mask = layer.mask.weight(xy, guide, aspect);
        match self.overlay_operator {
            Some(index) => match layer.operators.get(index) {
                Some(Operator::SampleColor(s)) => mask * s.range.weight(guide),
                _ => 0.,
            },
            None => mask,
        }
    }
    fn select_layer(&mut self, id: Id) {
        self.cancel_analysis();
        self.selected = Some(id);
        self.mask_view = false;
        self.scroll_to_layer = true;
        self.component = None;
        self.paint = false;
        self.pick = false;
        self.overlay = false;
    }
    pub fn open_tools(&mut self, selected: bool) {
        self.cancel_analysis();
        self.view_layers = false;
        self.view_tools = true;
        self.apply_selected = selected;
        self.paint = false;
        self.pick = false;
        self.overlay = false;
    }
    fn apply_tool(
        &mut self,
        recipe: &mut EditRecipe,
        tool: &layers::Tool,
        lang: Language,
    ) -> anyhow::Result<()> {
        let mut candidate = recipe.clone();
        let id = if self.apply_selected {
            let layer = candidate
                .layers
                .as_mut()
                .and_then(|s| s.layers.iter_mut().find(|l| Some(l.id) == self.selected))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Seleziona un livello modificabile con spazio per uno strumento."
                    )
                })?;
            anyhow::ensure!(
                !layer.locked && layer.operators.len() < layers::MAX_OPERATORS,
                "Seleziona un livello modificabile con spazio per uno strumento."
            );
            layer.operators.push(tool.operator());
            layer.id
        } else {
            let layer = Layer::new(lang.text(tool.name), tool.operator());
            let id = layer.id;
            candidate.layer_stack().layers.push(layer);
            id
        };
        candidate.validate()?;
        *recipe = candidate;
        self.select_layer(id);
        self.view_tools = false;
        self.view_layers = true;
        self.error = None;
        Ok(())
    }
    pub fn show_tools(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        recipe: &mut EditRecipe,
    ) -> bool {
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.;
            for (selected, title, description) in [
                (false, "Nuovo livello", "Su un nuovo livello"),
                (true, "Livello selezionato", "Sul livello selezionato"),
            ] {
                if panels::segment(ui, self.apply_selected == selected, lang.text(title), width)
                    .on_hover_text(lang.text(description))
                    .clicked()
                {
                    self.apply_selected = selected;
                }
            }
        });
        let selected = recipe
            .layers
            .as_ref()
            .and_then(|s| s.layers.iter().find(|l| Some(l.id) == self.selected));
        let allowed = if self.apply_selected {
            if let Some(layer) = selected {
                ui.add(egui::Label::new(&layer.name).truncate())
                    .on_hover_text(&layer.name);
            }
            selected.is_some_and(|l| !l.locked && l.operators.len() < layers::MAX_OPERATORS)
        } else {
            recipe.layers.as_ref().map_or(0, |s| s.layers.len()) < layers::MAX_LAYERS
        };
        if !allowed {
            help::label(
                ui,
                lang,
                if self.apply_selected {
                    "Seleziona un livello modificabile con spazio per uno strumento."
                } else {
                    "Massimo 16 livelli fotografici"
                },
            );
        }
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .desired_width(ui.available_width())
                .margin(egui::Margin::symmetric(10, 7))
                .hint_text(lang.text("Cerca strumento…")),
        );
        if search.changed() {
            self.search_index = 0;
        }
        let query = self.search.to_lowercase();
        let matches: Vec<_> = TOOLS
            .iter()
            .filter(|tool| {
                let haystack = format!("{} {} {}", tool.name, lang.text(tool.name), tool.keywords)
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
        let activate = ui.is_enabled()
            && (search.has_focus() || search.lost_focus())
            && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut chosen = None;
        if matches.is_empty() {
            help::label(ui, lang, "Nessuno strumento trovato");
        }
        ui.add_enabled_ui(allowed, |ui| {
            for (i, tool) in matches.into_iter().enumerate() {
                let response = panels::tool_button(
                    ui,
                    lang,
                    tool,
                    i == self.search_index && (search.has_focus() || !self.search.is_empty()),
                );
                if response.clicked() || (allowed && activate && i == self.search_index) {
                    chosen = Some(tool);
                }
            }
        });
        if let Some(tool) = chosen {
            match self.apply_tool(recipe, tool, lang) {
                Ok(()) => return true,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(super::AMBER, lang.text(error));
        }
        false
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
                    if help::selectable_label(ui, lang, "Campiona sulla foto", selected).clicked() {
                        self.cancel_analysis();
                        self.pick = !selected;
                        self.pick_component = false;
                        self.sample_operator = index;
                        self.paint = false;
                        self.overlay = false;
                    }
                    self.overlay_control(ui, lang, Some(index));
                });
                if self.pick
                    && !self.pick_component
                    && self.level_pick.is_none()
                    && self.sample_operator == index
                {
                    ui.label(
                        lang.text(
                            "Clic sulla foto: campione all'ingresso del livello. Esc annulla.",
                        ),
                    );
                }
                ui.small(lang.text("Intervallo: tonalità, cromia e luminanza Rec.2020 lineari"));
                help::CollapsingHeader::new(lang, "Intervallo del campione").show(ui, |ui| {
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
                help::CollapsingHeader::new(lang, "Uniformità verso il campione").show(ui, |ui| {
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
                });
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
                    *commit |= super::curve::controls_with_color(
                        ui,
                        lang,
                        &mut curves[self.channel],
                        "Curva a punti · RGB lineare",
                        [
                            egui::Color32::LIGHT_GRAY,
                            palette::RED,
                            palette::GREEN,
                            palette::BLUE,
                        ][self.channel],
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
                *commit |= help::checkbox(ui, lang, "Bianco e nero", &mut m.monochrome).changed();
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
                        if palette::choice(
                            ui,
                            lang,
                            self.band == i,
                            lang.text(name),
                            palette::family(name),
                        )
                        .clicked()
                        {
                            self.band = i;
                        }
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
                super::visuals::channels(ui, lang, matrix);
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
            if help::button(ui, lang, "Ripristina sezione").clicked() {
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
                if palette::choice(
                    ui,
                    lang,
                    self.channel == i,
                    lang.text(name),
                    palette::family(name),
                )
                .clicked()
                {
                    self.channel = i;
                }
            }
        });
        if before != self.channel && self.level_pick.is_some() {
            self.level_pick = None;
            self.pick = false;
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
            super::mask_overlay::paint(ui, rect, response.rect, [40, 180, 240], 85, |uv| {
                let Some(xy) = geometry.source_point(uv, native) else {
                    return 0.;
                };
                let px = ((uv[0] * raster.width as f64) as usize).min(raster.width as usize - 1);
                let py = ((uv[1] * raster.height as f64) as usize).min(raster.height as usize - 1);
                let p = raster.pixels[py * raster.width as usize + px];
                if p[3] <= 0.01 {
                    return 0.;
                }
                let guide = std::array::from_fn(|c| p[c] / p[3]);
                self.editing.layers.overlay_weight(
                    layer,
                    xy.map(|v| v as f32),
                    guide,
                    native[1] as f32 / native[0] as f32,
                )
            });
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

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn tool_destination_respects_layer_and_operator_limits_atomically() {
        let mut recipe = EditRecipe::neutral(Default::default());
        for _ in 0..layers::MAX_LAYERS {
            recipe.layer_stack().layers.push(Layer::empty("Empty"));
        }
        let mut controls = Controls {
            selected: Some(recipe.layers.as_ref().unwrap().layers[0].id),
            ..Default::default()
        };
        let before = recipe.clone();
        assert!(
            controls
                .apply_tool(&mut recipe, &TOOLS[1], Language::English)
                .is_err()
        );
        assert_eq!(recipe, before);
        controls.open_tools(true);
        for _ in 0..layers::MAX_OPERATORS {
            controls
                .apply_tool(&mut recipe, &TOOLS[1], Language::English)
                .unwrap();
        }
        let before = recipe.clone();
        assert!(
            controls
                .apply_tool(&mut recipe, &TOOLS[1], Language::English)
                .is_err()
        );
        assert_eq!(recipe, before);
        assert_eq!(
            recipe.layers.as_ref().unwrap().layers.len(),
            layers::MAX_LAYERS
        );
        recipe.validate().unwrap();
    }
}
