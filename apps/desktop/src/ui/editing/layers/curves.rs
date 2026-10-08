use super::{Controls, EditRecipe, slider};
use crate::i18n::Language;
use eframe::egui;
use sha2::{Digest, Sha256};
use tr_core::{editing::layers::*, provider::ImageLevels};

pub(super) struct InputAnalysis {
    image_id: u64,
    input_key: String,
    stats: LevelStatistics,
    record: LevelAnalysis,
}

fn input_recipe(recipe: &EditRecipe, selected: Option<Id>, operator: usize) -> Option<EditRecipe> {
    let mut input = recipe.clone();
    input
        .layers
        .as_mut()?
        .retain_before_operator(selected?, operator)
        .ok()?;
    Some(input)
}
fn key(recipe: &EditRecipe) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(recipe).expect("recipe serialization"))
    )
}
fn levels(op: &Operator) -> Option<LevelAdjustments> {
    match op {
        Operator::Levels(channels) => Some(LevelAdjustments {
            channels: *channels,
            ..Default::default()
        }),
        Operator::TonalLevels(g) => Some(g.as_ref().clone()),
        _ => None,
    }
}

impl Controls {
    pub fn set_analysis_source(&mut self, digest: &str) {
        if self.analysis_source != digest {
            self.cancel_analysis();
            self.analysis_source = digest.into();
        }
    }
    pub fn take_native_request(&mut self) -> bool {
        std::mem::take(&mut self.load_native)
    }
    pub(super) fn cancel_analysis(&mut self) {
        if self.level_pick.is_some() {
            self.pick = false;
        }
        self.level_pick = None;
        self.analysis_operator = None;
        self.input_analysis = None;
        self.pending_auto = None;
    }
    pub(super) fn analysis_preview(&self, recipe: &EditRecipe) -> Option<EditRecipe> {
        input_recipe(recipe, self.selected, self.analysis_operator?)
    }
    pub(super) fn refresh_analysis(&mut self, recipe: &EditRecipe) {
        if let Some(cache) = &self.input_analysis
            && self
                .analysis_preview(recipe)
                .is_none_or(|r| key(&r) != cache.input_key)
        {
            self.input_analysis = None;
        }
    }
    pub(super) fn prepare_analysis(&mut self, recipe: &EditRecipe, image: &ImageLevels) {
        if self.pick || image.base_level() != 0 {
            return;
        }
        let Some(index) = self.analysis_operator else {
            return;
        };
        let Some(layer) = recipe.layers.as_ref().and_then(|s| {
            s.layers
                .iter()
                .find(|l| Some(l.id) == self.selected && !l.locked)
        }) else {
            return;
        };
        let Some(g) = layer.operators.get(index).and_then(levels) else {
            self.cancel_analysis();
            return;
        };
        let Some(input) = self.analysis_preview(recipe) else {
            return;
        };
        let input_key = key(&input);
        if self.input_analysis.as_ref().is_some_and(|a| {
            a.image_id == image.id()
                && a.input_key == input_key
                && a.stats.composite == g.channels[0]
        }) {
            return;
        }
        match LevelStatistics::from_image(image.source(), g.channels[0]) {
            Ok(stats) => {
                let record = self.analysis_record(&input, image, LevelsAction::AutoComposite, 0);
                self.input_analysis = Some(InputAnalysis {
                    image_id: image.id(),
                    input_key,
                    stats,
                    record,
                });
                self.error = None;
            }
            Err(e) => {
                self.input_analysis = None;
                self.error = Some(e.to_string());
            }
        }
    }
    fn analysis_record(
        &self,
        input: &EditRecipe,
        image: &ImageLevels,
        action: LevelsAction,
        channel: usize,
    ) -> LevelAnalysis {
        LevelAnalysis {
            action,
            channel,
            source_digest: self.analysis_source.clone(),
            input_recipe_hash: key(input),
            geometry: input
                .advanced
                .as_ref()
                .map(|a| a.geometry.clone())
                .unwrap_or_default(),
            image_size: [image.source().width, image.source().height],
            valid_samples: 1,
            considered_samples: 1,
            sample_xy: None,
            sample_rgb: None,
        }
    }
    pub(super) fn apply_auto_request(&mut self, recipe: &mut EditRecipe) -> bool {
        let Some((index, action)) = self.pending_auto.take() else {
            return false;
        };
        let result = (|| -> anyhow::Result<()> {
            let input = input_recipe(recipe, self.selected, index)
                .ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
            let cache = self
                .input_analysis
                .as_ref()
                .filter(|a| {
                    a.input_key == key(&input) && a.record.source_digest == self.analysis_source
                })
                .ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
            let op = recipe
                .layers
                .as_mut()
                .and_then(|s| {
                    s.layers
                        .iter_mut()
                        .find(|l| Some(l.id) == self.selected && !l.locked)
                })
                .and_then(|l| l.operators.get_mut(index))
                .ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
            let mut g = levels(op).ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
            let mut record = cache.record.clone();
            record.action = action;
            g.auto(&cache.stats, record)?;
            *op = Operator::TonalLevels(Box::new(g));
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.cancel_analysis();
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }
    pub(super) fn accept_level_sample(
        &mut self,
        recipe: &mut EditRecipe,
        image: &ImageLevels,
        rgb: [f32; 3],
        xy: [u32; 2],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(image.base_level() == 0, "Campione livelli non nativo");
        let (index, channel, action) = self
            .level_pick
            .ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
        let input = input_recipe(recipe, self.selected, index)
            .ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
        let mut record = self.analysis_record(&input, image, action, channel);
        record.sample_xy = Some(xy);
        record.sample_rgb = Some(rgb);
        let op = recipe
            .layers
            .as_mut()
            .and_then(|s| {
                s.layers
                    .iter_mut()
                    .find(|l| Some(l.id) == self.selected && !l.locked)
            })
            .and_then(|l| l.operators.get_mut(index))
            .ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
        let mut g = levels(op).ok_or_else(|| anyhow::anyhow!("Analisi livelli superata"))?;
        g.pick(record)?;
        *op = Operator::TonalLevels(Box::new(g));
        self.cancel_analysis();
        self.error = None;
        Ok(())
    }
    pub(super) fn tonal_levels(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut LevelAdjustments,
        index: usize,
        commit: &mut bool,
    ) {
        self.channel_selector(ui, lang);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(
                    self.analysis_operator == Some(index),
                    lang.text("Analizza ingresso"),
                )
                .clicked()
            {
                self.cancel_analysis();
                self.analysis_operator = Some(index);
                self.pick = false;
                self.pick_component = false;
                self.paint = false;
                self.overlay = false;
                self.load_native = true;
            }
            if self.analysis_operator == Some(index)
                && ui.button(lang.text("Fine analisi")).clicked()
            {
                self.cancel_analysis();
            }
        });
        if self.analysis_operator == Some(index) {
            ui.small(lang.text("Ingresso della regolazione · intero ritaglio · maschera esclusa"));
            if let Some(cache) = self
                .input_analysis
                .as_ref()
                .filter(|a| a.stats.composite == g.channels[0])
            {
                histogram(ui, &cache.stats.histogram[self.channel]);
                ui.small(lang.text(if self.channel == 0 {
                    "Istogramma Y lineare 0–1 · code ai bordi"
                } else {
                    "Istogramma canale dopo composito · 0–1"
                }));
            } else {
                ui.small(lang.text("Preparazione ingresso nativo…"));
            }
        }
        let ready = self.analysis_operator == Some(index)
            && self
                .input_analysis
                .as_ref()
                .is_some_and(|a| a.stats.composite == g.channels[0]);
        ui.horizontal_wrapped(|ui| {
            for (action, name) in [
                (LevelsAction::AutoComposite, "Auto composito"),
                (LevelsAction::AutoChannels, "Auto canali indipendenti"),
            ] {
                if ui
                    .add_enabled(ready, egui::Button::new(lang.text(name)))
                    .clicked()
                {
                    self.pending_auto = Some((index, action));
                }
            }
        });
        ui.small(lang.text("Auto 1–99% · canali indipendenti possono cambiare il colore"));
        ui.horizontal_wrapped(|ui| {
            for (action, name) in [
                (LevelsAction::Black, "Campiona nero"),
                (LevelsAction::Gray, "Campiona grigio"),
                (LevelsAction::White, "Campiona bianco"),
            ] {
                let selected = self.pick && self.level_pick == Some((index, self.channel, action));
                if ui.selectable_label(selected, lang.text(name)).clicked() {
                    self.cancel_analysis();
                    if !selected {
                        self.level_pick = Some((index, self.channel, action));
                        self.analysis_operator = Some(index);
                        self.pick = true;
                        self.pick_component = false;
                        self.paint = false;
                        self.overlay = false;
                        self.load_native = true;
                    }
                }
            }
        });
        if self.level_pick.is_some_and(|(i, _, _)| i == index) {
            ui.small(
                lang.text("Clic sull'ingresso nativo · Esc annulla · grigio RGB allinea i canali"),
            );
        }
        let before = g.channels;
        let l = &mut g.channels[self.channel];
        slider(
            ui,
            lang,
            "Nero ingresso",
            &mut l.black,
            -1. ..=l.white - 0.001,
            0.,
            commit,
        );
        slider(
            ui,
            lang,
            "Bianco ingresso",
            &mut l.white,
            l.black + 0.001..=4.,
            1.,
            commit,
        );
        slider(ui, lang, "Gamma", &mut l.gamma, 0.1..=10., 1., commit);
        let white = l.output[1];
        slider(
            ui,
            lang,
            "Nero uscita",
            &mut l.output[0],
            -1. ..=white,
            0.,
            commit,
        );
        let black = l.output[0];
        slider(
            ui,
            lang,
            "Bianco uscita",
            &mut l.output[1],
            black..=4.,
            1.,
            commit,
        );
        if ui.button(lang.text("Ripristina canale")).clicked() {
            *l = Default::default();
            *commit = true;
        }
        if before != g.channels {
            g.analysis = None;
        }
        if g.analysis.is_some() {
            ui.small(lang.text("Analisi salvata · ricalcolo solo su richiesta"));
        }
    }
    pub(super) fn luminance_curve(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut LuminanceCurve,
        commit: &mut bool,
    ) {
        ui.small(lang.text("Y lineare · differenze fra i canali conservate"));
        *commit |= super::super::curve::controls_with_label(
            ui,
            lang,
            &mut g.points,
            "Curva a punti · luminanza lineare",
        )
        .1;
    }
    pub(super) fn parametric_curve(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut ParametricCurve,
        commit: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            *commit |= ui
                .selectable_value(&mut g.space, CurveSpace::Luminance, lang.text("Luminanza"))
                .changed();
            *commit |= ui
                .selectable_value(&mut g.space, CurveSpace::Rgb, "RGB")
                .changed();
        });
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 96.), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 4, egui::Color32::from_gray(22));
        let pos = |x: f32, y: f32| {
            egui::pos2(
                rect.left() + x * rect.width(),
                rect.bottom() - y * rect.height(),
            )
        };
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_top()],
            egui::Stroke::new(1., egui::Color32::from_gray(64)),
        );
        for b in g.boundaries {
            ui.painter().line_segment(
                [pos(b, 0.), pos(b, 1.)],
                egui::Stroke::new(1., egui::Color32::from_gray(64)),
            );
        }
        ui.painter().add(egui::Shape::line(
            (0..=128)
                .map(|i| {
                    let x = i as f32 / 128.;
                    pos(x, g.value(x))
                })
                .collect(),
            egui::Stroke::new(2., egui::Color32::LIGHT_GRAY),
        ));
        ui.small(lang.text("Zone lineari 0–1 · estremi e valori estesi conservati"));
        for (name, v) in ["Ombre", "Toni scuri", "Toni chiari", "Luci"]
            .into_iter()
            .zip(&mut g.amounts)
        {
            slider(ui, lang, name, v, -100. ..=100., 0., commit);
        }
        egui::CollapsingHeader::new(lang.text("Confini delle zone")).show(ui, |ui| {
            for i in 0..3 {
                let lo = if i == 0 {
                    0.02
                } else {
                    g.boundaries[i - 1] + 0.02
                };
                let hi = if i == 2 {
                    0.98
                } else {
                    g.boundaries[i + 1] - 0.02
                };
                slider(
                    ui,
                    lang,
                    ["Confine ombre", "Confine mezzitoni", "Confine luci"][i],
                    &mut g.boundaries[i],
                    lo..=hi,
                    [0.25, 0.5, 0.75][i],
                    commit,
                );
            }
        });
    }
}

fn histogram(ui: &mut egui::Ui, bins: &[u32; 64]) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.), egui::Sense::hover());
    let max = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    ui.painter()
        .rect_filled(rect, 3, egui::Color32::from_gray(22));
    for (i, count) in bins.iter().enumerate() {
        let x = rect.left() + (i as f32 + 0.5) * rect.width() / 64.;
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x, rect.bottom() - *count as f32 / max * rect.height()),
            ],
            egui::Stroke::new((rect.width() / 64.).max(1.), egui::Color32::GRAY),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::{color::LinearImage, preview::PreviewRequest};

    fn setup() -> (Controls, EditRecipe, ImageLevels) {
        let mut r = EditRecipe::neutral(Default::default());
        let light = || Operator::Light {
            exposure: 1.,
            temperature: 0.,
            tint: 0.,
            saturation: 0.,
        };
        r.layer_stack().layers.push(Layer::new("Earlier", light()));
        let mut layer = Layer::new("Target", light());
        layer.operators.extend([
            Operator::Levels([TonalLevel::default(); 4]),
            Operator::Colorize(Colorize {
                amount: 25.,
                ..Default::default()
            }),
        ]);
        layer.opacity = 0.1;
        layer.mask.append(MaskKind::Constant(0.2), Combine::Add);
        let selected = layer.id;
        r.layer_stack().layers.push(layer);
        let mut controls = Controls::default();
        controls.bind("A");
        controls.view_layers = true;
        controls.set_analysis_source(&"a".repeat(64));
        controls.selected = Some(selected);
        controls.analysis_operator = Some(1);
        let input = input_recipe(&r, controls.selected, 1).unwrap();
        let mut raster = LinearImage::new(
            100,
            1,
            (0..100)
                .map(|i| {
                    let v = 0.02 + i as f32 / 1000.;
                    [v, v * 1.1, v * 1.2, 1.]
                })
                .collect(),
        )
        .unwrap();
        input.apply(&mut raster).unwrap();
        let image = ImageLevels::from_source(raster, PreviewRequest::full()).unwrap();
        (controls, r, image)
    }

    #[test]
    fn analysis_uses_preceding_operators_without_the_target_mask_or_opacity() {
        let (mut controls, r, image) = setup();
        assert!((image.source().pixels[0][0] - 0.08).abs() < 1e-6);
        let mut viewed = r.clone();
        controls.preview_input("A", &mut viewed);
        assert_eq!(viewed.layers.as_ref().unwrap().layers[1].operators.len(), 1);
        assert_eq!(viewed.layers.as_ref().unwrap().layers[1].opacity, 1.);
        assert!(
            viewed.layers.as_ref().unwrap().layers[1]
                .mask
                .nodes
                .is_empty()
        );
        controls.prepare_analysis(&r, &image);
        let input = controls.input_analysis.as_ref().unwrap();
        assert_eq!(input.stats.valid, 100);
        assert_eq!(input.record.input_recipe_hash, key(&viewed));
        let mut actual = r.clone();
        controls.pending_auto = Some((1, LevelsAction::AutoComposite));
        assert!(controls.apply_auto_request(&mut actual));
        assert!(!controls.apply_auto_request(&mut actual));
        assert_eq!(
            actual.layers.as_ref().unwrap().layers[0],
            r.layers.as_ref().unwrap().layers[0]
        );
        let before = &r.layers.as_ref().unwrap().layers[1];
        let after = &actual.layers.as_ref().unwrap().layers[1];
        assert_eq!(after.mask, before.mask);
        assert_eq!(after.opacity, before.opacity);
        assert_eq!(after.operators[0], before.operators[0]);
        assert_eq!(after.operators[2], before.operators[2]);
        let Operator::TonalLevels(g) = &after.operators[1] else {
            panic!("legacy levels not promoted")
        };
        assert_eq!(g.analysis.as_ref().unwrap().input_recipe_hash, key(&viewed));
        actual.validate().unwrap();
        assert!(controls.analysis_operator.is_none());
    }

    #[test]
    fn changed_inputs_sources_and_locked_layers_reject_stale_auto_without_edits() {
        for scenario in 0..4 {
            let (mut controls, mut r, image) = setup();
            controls.prepare_analysis(&r, &image);
            controls.pending_auto = Some((1, LevelsAction::AutoChannels));
            match scenario {
                0 => r.exposure_ev = 0.5,
                1 => controls.set_analysis_source(&"c".repeat(64)),
                2 => r.layers.as_mut().unwrap().layers[1].locked = true,
                _ => {
                    if let Operator::Levels(l) =
                        &mut r.layers.as_mut().unwrap().layers[1].operators[1]
                    {
                        l[0].gamma = 2.;
                    }
                }
            }
            let before = r.clone();
            assert!(!controls.apply_auto_request(&mut r));
            assert_eq!(r, before);
        }
        let (mut controls, mut r, image) = setup();
        controls.prepare_analysis(&r, &image);
        r.advanced = Some(Box::default());
        r.advanced.as_mut().unwrap().geometry.crop = [0.1, 0.1, 0.9, 0.9];
        controls.refresh_analysis(&r);
        assert!(controls.input_analysis.is_none());
        controls.bind("B");
        assert!(controls.analysis_operator.is_none());
    }

    #[test]
    fn levels_picker_records_the_correct_operator_channel_and_source() {
        let (mut controls, mut r, image) = setup();
        controls.level_pick = Some((1, 0, LevelsAction::Gray));
        controls.pick = true;
        let before = r.clone();
        controls
            .accept_level_sample(&mut r, &image, [0.2, 0.3, 0.4], [2, 0])
            .unwrap();
        let after = &r.layers.as_ref().unwrap().layers[1];
        assert_eq!(
            after.operators[0],
            before.layers.as_ref().unwrap().layers[1].operators[0]
        );
        let Operator::TonalLevels(g) = &after.operators[1] else {
            panic!()
        };
        let a = g.analysis.as_ref().unwrap();
        assert_eq!(a.source_digest, "a".repeat(64));
        assert_eq!(a.sample_xy, Some([2, 0]));
        assert_eq!(a.sample_rgb, Some([0.2, 0.3, 0.4]));
        assert!(!controls.pick);
        r.validate().unwrap();
    }

    #[test]
    fn cancelling_analysis_restores_the_complete_view_without_mutating_recipe() {
        let (mut controls, r, image) = setup();
        controls.prepare_analysis(&r, &image);
        controls.level_pick = Some((1, 2, LevelsAction::Black));
        controls.pick = true;
        let mut preview = r.clone();
        controls.preview_input("A", &mut preview);
        assert_ne!(preview, r);
        controls.cancel_analysis();
        let mut preview = r.clone();
        controls.preview_input("A", &mut preview);
        assert_eq!(preview, r);
        assert!(controls.input_analysis.is_none() && !controls.pick);
    }
}
