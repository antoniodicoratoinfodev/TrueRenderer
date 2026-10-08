use crate::i18n::Language;
use eframe::egui;
use tr_core::editing::EditRecipe;

#[derive(Clone, Copy)]
struct Groups {
    light: bool,
    curve: bool,
    color: bool,
}
impl Default for Groups {
    fn default() -> Self {
        Self {
            light: true,
            curve: true,
            color: true,
        }
    }
}
impl Groups {
    fn merge(self, source: &EditRecipe, destination: &EditRecipe) -> anyhow::Result<EditRecipe> {
        source.validate()?;
        destination.validate()?;
        let mut result = destination.clone();
        if self.light {
            result.exposure_ev = source.exposure_ev;
            result.brightness = source.brightness;
            result.contrast = source.contrast;
            result.highlights = source.highlights;
            result.shadows = source.shadows;
            result.whites = source.whites;
            result.blacks = source.blacks;
        }
        if self.curve {
            result.curve.clone_from(&source.curve);
            if tr_core::editing::curve_has_adjusted_endpoints(&result.curve) {
                result.require_process(4);
            }
        }
        if self.color {
            result.temperature = source.temperature;
            result.tint = source.tint;
            result.saturation = source.saturation;
            result.vibrance = source.vibrance;
            result.protect_warm = source.protect_warm;
            // Process 2 adds vibrance without changing the earlier operations.
            // Never downgrade an existing destination or import a RAW recipe.
            if result.vibrance != 0. || result.protect_warm {
                result.require_process(2);
            }
        }
        result.validate()?;
        Ok(result)
    }
}

#[derive(Default)]
pub(in crate::ui) struct Clipboard {
    copied: Option<(String, EditRecipe)>,
    groups: Groups,
}
impl Clipboard {
    pub(in crate::ui) fn copy(&mut self, name: &str, recipe: &EditRecipe) {
        self.copied = Some((name.into(), recipe.clone()));
    }

    pub(in crate::ui) fn paste(&self, destination: &EditRecipe) -> Option<EditRecipe> {
        let (_, source) = self.copied.as_ref()?;
        self.groups
            .merge(source, destination)
            .ok()
            .filter(|r| r != destination)
    }

    pub(in crate::ui) fn menu(&mut self, ui: &mut egui::Ui, lang: Language) -> bool {
        ui.checkbox(&mut self.groups.light, lang.text("Luce"));
        ui.checkbox(&mut self.groups.curve, lang.text("Curva tonale"));
        ui.checkbox(&mut self.groups.color, lang.text("Colore RGB"));
        ui.small(
            lang.text("Solo questa sessione. Motore e WB RAW della destinazione sono conservati."),
        );
        ui.add_enabled(
            self.copied.is_some() && (self.groups.light || self.groups.curve || self.groups.color),
            egui::Button::new(lang.text("Incolla gruppi selezionati")),
        )
        .clicked()
    }
    /// Session-local snapshot; never reads or changes the system clipboard.
    pub(in crate::ui) fn controls(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        name: &str,
        draft: &mut EditRecipe,
        ready: bool,
    ) -> bool {
        let mut pasted = false;
        egui::CollapsingHeader::new(lang.text("Copia e incolla regolazioni"))
            .id_salt("edit-transfer")
            .show(ui, |ui| {
                if ui
                    .add_enabled(ready, egui::Button::new(lang.text("Copia regolazioni")))
                    .on_hover_text("Alt + Shift + C")
                    .clicked()
                {
                    self.copy(name, draft);
                }
                if let Some((source_name, source)) = &self.copied {
                    ui.label(format!("{}: {}", lang.text("Copiate da"), source_name));
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.groups.light, lang.text("Luce"));
                        ui.checkbox(&mut self.groups.curve, lang.text("Curva tonale"));
                        ui.checkbox(&mut self.groups.color, lang.text("Colore RGB"));
                    });
                    let merged = self.groups.merge(source, draft);
                    let changed = merged.as_ref().is_ok_and(|recipe| recipe != draft);
                    if ui
                        .add_enabled(
                            ready && changed,
                            egui::Button::new(lang.text("Incolla gruppi selezionati")),
                        )
                        .on_hover_text("Alt + Shift + V")
                        .clicked()
                    {
                        *draft = merged.as_ref().unwrap().clone();
                        pasted = true;
                    }
                    if let Err(error) = &merged {
                        ui.label(error.to_string());
                    }
                } else {
                    ui.small(lang.text("Copia da una foto, poi seleziona la destinazione."));
                }
                ui.small(lang.text(
                    "Solo questa sessione. Motore e WB RAW della destinazione sono conservati.",
                ));
            });
        pasted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::{decoder::RawEngine, editing::CurvePoint};

    fn frame(
        ctx: &egui::Context,
        clipboard: &mut Clipboard,
        draft: &mut EditRecipe,
        lang: Language,
        width: f32,
        ready: bool,
        events: Vec<egui::Event>,
    ) -> (bool, Vec<(String, egui::Rect)>) {
        fn collect(shape: &egui::Shape, text: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => text.push((
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }
        let mut pasted = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1000.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                pasted = clipboard.controls(ui, lang, "source.png", draft, ready);
            },
        );
        output.textures_delta.clear();
        let mut text = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        (pasted, text)
    }

    #[test]
    fn copying_movable_endpoints_upgrades_only_the_required_process() {
        let mut source = EditRecipe::neutral(RawEngine::Apple);
        source.process_version = 4;
        source.curve = vec![CurvePoint { x: 0.1, y: 0.2 }, CurvePoint { x: 0.9, y: 0.8 }];
        let destination = EditRecipe::neutral(RawEngine::Apple);
        let copied = Groups {
            light: false,
            curve: true,
            color: false,
        }
        .merge(&source, &destination)
        .unwrap();
        assert_eq!(copied.process_version, 4);
        assert_eq!(copied.curve, source.curve);
    }

    #[test]
    fn transfer_controls_wrap_and_guard_paste_in_both_languages() {
        for lang in [Language::Italian, Language::English] {
            for width in [220., 300., 440.] {
                for mode in 0..4 {
                    let ctx = egui::Context::default();
                    ctx.style_mut_of(egui::Theme::Dark, |s| s.animation_time = 0.);
                    ctx.style_mut_of(egui::Theme::Light, |s| s.animation_time = 0.);
                    let mut clipboard = Clipboard {
                        copied: Some((format!("{}.png", "long_source_name_".repeat(12)), source())),
                        groups: if mode == 1 {
                            Groups {
                                light: false,
                                curve: false,
                                color: false,
                            }
                        } else {
                            Groups::default()
                        },
                    };
                    let mut draft = if mode == 2 {
                        source()
                    } else {
                        EditRecipe::neutral(RawEngine::Apple)
                    };
                    let before = draft.clone();
                    let ready = mode != 0;
                    let header =
                        frame(&ctx, &mut clipboard, &mut draft, lang, width, ready, vec![]).1[0].1;
                    for pressed in [true, false] {
                        let pos = header.center();
                        frame(
                            &ctx,
                            &mut clipboard,
                            &mut draft,
                            lang,
                            width,
                            ready,
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                    }
                    let mut text = Vec::new();
                    for _ in 0..3 {
                        text =
                            frame(&ctx, &mut clipboard, &mut draft, lang, width, ready, vec![]).1;
                    }
                    let paste = text
                        .iter()
                        .find(|(t, _)| t == lang.text("Incolla gruppi selezionati"))
                        .unwrap_or_else(|| panic!("Missing paste: {text:?}"))
                        .1;
                    for (label, rect) in &text {
                        assert!(
                            rect.min.x >= 0. && rect.max.x <= width + 1.,
                            "{lang:?} width={width}: {label}: {rect:?}"
                        );
                    }
                    let mut pasted = false;
                    for pressed in [true, false] {
                        let pos = paste.center();
                        pasted |= frame(
                            &ctx,
                            &mut clipboard,
                            &mut draft,
                            lang,
                            width,
                            ready,
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        )
                        .0;
                    }
                    assert_eq!(pasted, mode == 3);
                    if mode != 3 {
                        assert_eq!(draft, before);
                    } else {
                        assert_eq!(draft.exposure_ev, 1.5);
                        assert_eq!(draft.raw_engine, before.raw_engine);
                        assert_eq!(draft.raw_wb, before.raw_wb);
                    }
                }
            }
        }
    }

    fn source() -> EditRecipe {
        let mut r = EditRecipe::neutral(RawEngine::TrueRenderer);
        r.raw_wb.red = 1400;
        r.exposure_ev = 1.5;
        r.brightness = 10.;
        r.contrast = 20.;
        r.highlights = -30.;
        r.shadows = 40.;
        r.whites = 15.;
        r.blacks = -10.;
        r.curve = vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 0.5, y: 0.6 },
            CurvePoint { x: 1., y: 1. },
        ];
        r.temperature = 20.;
        r.tint = -10.;
        r.saturation = 15.;
        r.vibrance = 30.;
        r.protect_warm = true;
        r.process_version = 2;
        r
    }

    #[test]
    fn selective_transfer_preserves_destination_raw_and_unselected_groups() {
        let source = source();
        let mut destination = EditRecipe::neutral(RawEngine::Apple);
        destination.raw_wb.apple_temperature = 4500;
        destination.raw_wb.apple_tint = 12;
        let light_fields = [
            "exposure_ev",
            "brightness",
            "contrast",
            "highlights",
            "shadows",
            "whites",
            "blacks",
        ];
        let color_fields = [
            "temperature",
            "tint",
            "saturation",
            "vibrance",
            "protect_warm",
        ];
        for mask in 0..8 {
            let groups = Groups {
                light: mask & 1 != 0,
                curve: mask & 2 != 0,
                color: mask & 4 != 0,
            };
            let result = groups.merge(&source, &destination).unwrap();
            let actual = serde_json::to_value(&result).unwrap();
            let from = serde_json::to_value(&source).unwrap();
            let to = serde_json::to_value(&destination).unwrap();
            for (key, value) in actual.as_object().unwrap() {
                let copied = (groups.light && light_fields.contains(&key.as_str()))
                    || (groups.curve && key == "curve")
                    || (groups.color
                        && (color_fields.contains(&key.as_str()) || key == "process_version"));
                assert_eq!(
                    Some(value),
                    if copied { from.get(key) } else { to.get(key) },
                    "mask={mask} field={key}"
                );
            }
            assert_eq!(groups.merge(&source, &result).unwrap(), result);
        }
        // Copying older color values never downgrades a process-2 destination.
        assert_eq!(
            Groups::default()
                .merge(&destination, &source)
                .unwrap()
                .process_version,
            2
        );
        let mut invalid = source.clone();
        invalid.exposure_ev = f32::NAN;
        assert!(Groups::default().merge(&invalid, &destination).is_err());
    }

    #[test]
    fn copied_snapshot_and_destination_history_survive_source_changes_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
        let a = catalog
            .observe(std::path::Path::new("a.png"), "a", 4)
            .unwrap();
        let b = catalog
            .observe(std::path::Path::new("b.png"), "b", 4)
            .unwrap();
        let from = catalog.save_edit(&a.id, 0, &source()).unwrap();
        let mut clipboard = Clipboard {
            copied: Some(("a.png".into(), from.recipe.clone())),
            groups: Groups::default(),
        };
        clipboard.groups.curve = false;
        let original = EditRecipe::neutral(RawEngine::Apple);
        let prior = catalog.save_edit(&b.id, 0, &original).unwrap();
        let pasted = clipboard
            .groups
            .merge(&clipboard.copied.as_ref().unwrap().1, &prior.recipe)
            .unwrap();
        let saved = catalog.save_edit(&b.id, prior.generation, &pasted).unwrap();
        catalog
            .save_edit(
                &a.id,
                from.generation,
                &EditRecipe::neutral(RawEngine::TrueRenderer),
            )
            .unwrap();
        assert_eq!(clipboard.copied.as_ref().unwrap().1, source());
        let undone = catalog.step_edit(&b.id, saved.generation, true).unwrap();
        assert_eq!(undone.recipe, original);
        drop(catalog);
        let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
        let reopened = catalog.load_edit(&b.id, RawEngine::Apple).unwrap();
        let redone = catalog
            .step_edit(&b.id, reopened.generation, false)
            .unwrap();
        assert_eq!(redone.recipe, pasted);
        assert_eq!(redone.source_digest, prior.source_digest);
        assert!(redone.recipe.curve.is_empty());
    }
}
