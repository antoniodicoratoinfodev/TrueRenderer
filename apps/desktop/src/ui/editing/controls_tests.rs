use super::*;
use std::path::Path;

#[test]
fn creative_grain_controls_commit_and_keep_settings_when_disabled() {
    use tr_core::editing::grain::{Distribution, Grain};
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        crate::ui::style::apply(&ctx);
        let mut grain = Grain::default();
        let draw = |events, grain: &mut Grain, commits: &mut usize| {
            frame(&ctx, events, |ui| {
                ui.set_width(260.);
                panels::control_style(ui);
                let mut commit = false;
                advanced::grain_controls(ui, lang, grain, &mut commit);
                *commits += usize::from(commit);
            })
        };
        let mut commits = 0;
        let mut shapes = draw(vec![], &mut grain, &mut commits);
        assert_eq!(commits, 0);
        for label in [
            "Aggiungi grana",
            "Gaussiana",
            "Grana monocromatica",
            "Nuova trama",
            "Aggiungi grana",
        ] {
            let position = shapes
                .texts
                .iter()
                .find(|(text, _)| text == lang.text(label))
                .unwrap()
                .1
                .center();
            let before = commits;
            for pressed in [true, false] {
                draw(click(position, pressed), &mut grain, &mut commits);
            }
            assert_eq!(commits, before + 1, "{label} {lang:?}");
            shapes = draw(vec![], &mut grain, &mut commits);
        }
        assert!(!grain.enabled);
        assert_eq!(grain.distribution, Distribution::Gaussian);
        assert!(!grain.monochrome);
        assert_eq!(grain.seed, 1);
        // Disabled controls cannot change the retained pattern.
        let position = shapes
            .texts
            .iter()
            .find(|(text, _)| text == lang.text("Nuova trama"))
            .unwrap()
            .1
            .center();
        let before = grain.clone();
        for pressed in [true, false] {
            draw(click(position, pressed), &mut grain, &mut commits);
        }
        assert_eq!(grain, before);
        grain.enabled = true;
        let shapes = draw(vec![], &mut grain, &mut commits);
        let position = shapes
            .texts
            .iter()
            .find(|(text, _)| text == lang.text("Ripristina grana"))
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            draw(click(position, pressed), &mut grain, &mut commits);
        }
        assert_eq!(grain, Grain::default());
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn automatic_wb_button_reveals_area_options_without_editing_the_recipe() {
    let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
    crate::ui::settings_regressions::settle(&mut app, &ctx, true);
    crate::ui::style::apply(&ctx);
    let item = app.state.items[0].clone();
    let recipe = EditRecipe::neutral(Default::default());
    app.state.current = Some(item.id.clone());
    app.editing.entries.insert(
        item.id.clone(),
        EditEntry {
            loaded: Some(LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 0,
                revision: 0,
                recipe: recipe.clone(),
                can_undo: false,
                can_redo: false,
            }),
            draft: Some(recipe.clone()),
            ..Default::default()
        },
    );
    for lang in [Language::Italian, Language::English] {
        app.cache_settings.language = lang;
        app.editing.wb_picker = Default::default();
        app.editing.picker_side = 5;
        let draw = |ui: &mut egui::Ui, app: &mut TrueRenderer| {
            ui.set_width(260.);
            panels::control_style(ui);
            app.wb_picker_controls(ui, &item, true, &recipe);
        };
        let shapes = frame(&ctx, vec![], |ui| draw(ui, &mut app));
        assert!(!shapes.texts.iter().any(|(text, _)| text == "1×1"));
        let button = shapes
            .texts
            .iter()
            .find(|(text, _)| text == lang.text("WB automatico"))
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            frame(&ctx, click(button, pressed), |ui| draw(ui, &mut app));
        }
        assert!(app.wb_picker_active(&item.id));
        let shapes = frame(&ctx, vec![], |ui| draw(ui, &mut app));
        for side in tr_core::editing::WB_PICKER_SIDES {
            assert!(
                shapes
                    .texts
                    .iter()
                    .any(|(text, _)| text == &format!("{side}×{side}"))
            );
        }
        let option = shapes
            .texts
            .iter()
            .find(|(text, _)| text == "3×3")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            frame(&ctx, click(option, pressed), |ui| draw(ui, &mut app));
        }
        assert_eq!(app.editing.picker_side, 3);
        assert!(app.wb_picker_active(&item.id));
        assert!(!app.editing.wb_pending);
        assert!(!app.editing.entries[&item.id].pending);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
        for pressed in [true, false] {
            frame(&ctx, click(button, pressed), |ui| draw(ui, &mut app));
        }
        assert!(!app.wb_picker_active(&item.id));
    }
}

#[test]
fn sensor_wb_ui_preserves_saved_gains_and_resets_photographic_axes() {
    use tr_core::decoder::{RawEngine, RawWhiteBalance};
    for lang in [Language::Italian, Language::English] {
        for engine in [
            RawEngine::LibRawBilinear,
            RawEngine::LibRawAhd,
            RawEngine::TrueRenderer,
            RawEngine::TrueRendererExperimental,
        ] {
            let mut cases = vec![RawWhiteBalance::default()];
            for red in [250, 251, 800, 1333, 3999, 4000] {
                for blue in [250, 251, 800, 1333, 3999, 4000] {
                    cases.push(RawWhiteBalance {
                        red,
                        blue,
                        ..Default::default()
                    });
                }
            }
            if engine == RawEngine::TrueRendererExperimental {
                cases
                    .push(RawWhiteBalance::precise_sensor_gains([1.333317, 1., 0.800273]).unwrap());
            }
            for original in cases {
                let ctx = egui::Context::default();
                let mut wb = original;
                for _ in 0..3 {
                    let shapes = frame(&ctx, vec![], |ui| {
                        assert_eq!(
                            sensor_wb_controls(ui, lang, engine, &mut wb),
                            (false, false)
                        );
                    });
                    assert!(
                        shapes
                            .texts
                            .iter()
                            .any(|(s, _)| s == lang.text("Temperatura"))
                    );
                    assert!(shapes.texts.iter().any(|(s, _)| s == lang.text("Tinta")));
                    assert!(
                        !shapes
                            .texts
                            .iter()
                            .any(|(s, _)| s == lang.text("Rosso RAW") || s == lang.text("Blu RAW"))
                    );
                    assert_eq!(wb, original);
                }
            }
            for axis in 0..2 {
                let ctx = egui::Context::default();
                let original = if engine == RawEngine::TrueRendererExperimental {
                    RawWhiteBalance::precise_sensor_gains([1.333317, 1., 0.800273]).unwrap()
                } else {
                    RawWhiteBalance {
                        red: 1333,
                        blue: 800,
                        ..Default::default()
                    }
                };
                let mut wb = original;
                let shapes = frame(&ctx, vec![], |ui| {
                    sensor_wb_controls(ui, lang, engine, &mut wb);
                });
                let position = shapes.knobs[axis];
                for pressed in [true, false, true, false] {
                    frame(&ctx, click(position, pressed), |ui| {
                        sensor_wb_controls(ui, lang, engine, &mut wb);
                    });
                }
                let before = original.gains_f64();
                let after = wb.gains_f64();
                let tolerance = if engine == RawEngine::TrueRendererExperimental {
                    0.000003
                } else {
                    0.003
                };
                if axis == 0 {
                    assert!((after[0] - after[2]).abs() < tolerance);
                    assert!((before[0] * before[2] - after[0] * after[2]).abs() < tolerance);
                } else {
                    assert!((after[0] * after[2] - 1.).abs() < tolerance);
                    assert!((before[0] / before[2] - after[0] / after[2]).abs() < tolerance);
                }
                wb.validate_for(engine).unwrap();
                assert_ne!(wb_draft_gains(engine, original, wb), [1.; 3]);
            }
        }
    }
}

#[test]
fn sensor_wb_temperature_and_tint_drag_commit_for_every_gain_engine() {
    use tr_core::decoder::{RawEngine, RawWhiteBalance};
    for lang in [Language::Italian, Language::English] {
        for engine in [
            RawEngine::LibRawBilinear,
            RawEngine::LibRawAhd,
            RawEngine::TrueRenderer,
            RawEngine::TrueRendererExperimental,
        ] {
            for axis in 0..2 {
                let ctx = egui::Context::default();
                let mut wb = RawWhiteBalance::default();
                let shapes = frame(&ctx, vec![], |ui| {
                    sensor_wb_controls(ui, lang, engine, &mut wb);
                });
                let knob = shapes.knobs[axis];
                frame(&ctx, click(knob, true), |ui| {
                    sensor_wb_controls(ui, lang, engine, &mut wb);
                });
                let target = knob + egui::vec2(37., 0.);
                frame(&ctx, vec![egui::Event::PointerMoved(target)], |ui| {
                    sensor_wb_controls(ui, lang, engine, &mut wb);
                });
                let mut committed = false;
                frame(&ctx, click(target, false), |ui| {
                    committed = sensor_wb_controls(ui, lang, engine, &mut wb).1;
                });
                assert!(committed);
                let [r, _, b] = wb.gains_f64();
                assert!(r > 1.);
                if axis == 0 {
                    assert!(b < 1.);
                } else {
                    assert_eq!(r, b);
                }
                wb.validate_for(engine).unwrap();
                let saved = wb;
                frame(&ctx, vec![], |ui| {
                    assert_eq!(
                        sensor_wb_controls(ui, lang, engine, &mut wb),
                        (false, false)
                    );
                });
                assert_eq!(wb, saved);
            }
        }
    }
}

#[test]
fn curves_and_levels_panels_open_in_both_languages_without_rewriting_values() {
    use tr_core::editing::layers::*;
    for lang in [Language::Italian, Language::English] {
        for op in crate::verify_advanced::curves_recipe(Default::default(), false)
            .layers
            .unwrap()
            .layers
            .remove(0)
            .operators
        {
            let ctx = egui::Context::default();
            let title = lang.text(op.tool().name).to_owned();
            let mut r = EditRecipe::neutral(Default::default());
            r.layer_stack().layers.push(Layer::new("Curves", op));
            let before = r.clone();
            let mut controls = super::layers::Controls::default();
            controls.bind("A");
            for _ in 0..2 {
                let shapes = frame(&ctx, vec![], |ui| {
                    assert_eq!(controls.show(ui, lang, &mut r), (false, false));
                });
                assert!(shapes.texts.iter().any(|(s, _)| s == &title));
            }
            assert_eq!(r, before);
            r.validate().unwrap();
        }
    }
}

#[test]
fn level_channel_reset_is_scoped_and_changing_channel_cancels_the_picker() {
    use tr_core::editing::layers::*;
    let mut g = LevelAdjustments::default();
    g.channels[1].gamma = 1.2;
    g.channels[2].black = -0.1;
    let mut r = EditRecipe::neutral(Default::default());
    r.layer_stack().layers.push(Layer::new(
        "Levels",
        Operator::TonalLevels(Box::new(g.clone())),
    ));
    let ctx = egui::Context::default();
    let mut controls = super::layers::Controls::default();
    controls.bind("A");
    let mut commits = 0;
    for (label, expected) in [
        ("Red", 0),
        ("Reset channel", 1),
        ("Pick gray", 1),
        ("Green", 1),
    ] {
        let mut draw = |ui: &mut egui::Ui| {
            commits += usize::from(controls.show(ui, Language::English, &mut r).1);
        };
        let shapes = frame(&ctx, vec![], &mut draw);
        let pos = shapes
            .texts
            .iter()
            .find(|(s, _)| s == label)
            .unwrap()
            .1
            .center();
        frame(&ctx, click(pos, true), &mut draw);
        frame(&ctx, click(pos, false), &mut draw);
        assert_eq!(commits, expected);
        if label == "Pick gray" {
            assert!(controls.pick && controls.take_native_request());
        }
    }
    assert!(!controls.pick);
    let Operator::TonalLevels(actual) = &r.layers.as_ref().unwrap().layers[0].operators[0] else {
        panic!()
    };
    assert_eq!(actual.channels[1], TonalLevel::default());
    assert_eq!(actual.channels[2], g.channels[2]);
}

#[test]
fn frozen_auto_levels_and_curves_reopen_and_undo_redo_without_reanalysis() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    let digest = "a".repeat(64);
    let item = catalog
        .observe(Path::new("synthetic.png"), &digest, 4)
        .unwrap();
    let original = crate::verify_advanced::curves_recipe(Default::default(), false);
    let first = catalog.save_edit(&item.id, 0, &original).unwrap();
    let source = tr_core::color::LinearImage::new(
        128,
        96,
        (0..128 * 96)
            .map(|i| {
                let v = 0.1 + (i % 128) as f32 / 160.;
                [v, v * 0.8, v * 0.6, 1.]
            })
            .collect(),
    )
    .unwrap();
    let mut analyzed = original.clone();
    crate::verify_advanced::freeze_levels_auto(&mut analyzed, &source, &digest, true).unwrap();
    let saved = catalog
        .save_edit(&item.id, first.generation, &analyzed)
        .unwrap();
    drop(catalog);
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    assert_eq!(
        catalog
            .load_edit(&item.id, Default::default())
            .unwrap()
            .recipe,
        analyzed
    );
    let undo = catalog.step_edit(&item.id, saved.generation, true).unwrap();
    assert_eq!(undo.recipe, original);
    let redo = catalog.step_edit(&item.id, undo.generation, false).unwrap();
    assert_eq!(redo.recipe, analyzed);
    assert_eq!(
        catalog
            .save_edit(&item.id, redo.generation, &analyzed)
            .unwrap()
            .generation,
        redo.generation
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn level_picker_waits_for_native_pixels_and_rejects_transparent_clicks() {
    use tr_core::editing::layers::*;
    let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
    crate::ui::settings_regressions::settle(&mut app, &ctx, true);
    let item = app.state.items[0].clone();
    let mut recipe = EditRecipe::neutral(Default::default());
    let mut layer = Layer::new(
        "Levels",
        Operator::Light {
            exposure: 1.,
            temperature: 0.,
            tint: 0.,
            saturation: 0.,
        },
    );
    layer
        .operators
        .push(Operator::Levels([TonalLevel::default(); 4]));
    let id = layer.id;
    recipe.layer_stack().layers.push(layer);
    app.state.current = Some(item.id.clone());
    app.editing.entries.insert(
        item.id.clone(),
        EditEntry {
            loaded: Some(LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 0,
                revision: 0,
                recipe: recipe.clone(),
                can_undo: false,
                can_redo: false,
            }),
            draft: Some(recipe.clone()),
            ..Default::default()
        },
    );
    app.editing.layers.bind(&item.id);
    app.editing.layers.view_layers = true;
    app.editing.layers.selected = Some(id);
    app.editing.layers.set_analysis_source(&item.digest);
    let mut panel = recipe.clone();
    let mut draw = |ui: &mut egui::Ui| {
        app.editing.layers.show(ui, Language::English, &mut panel);
    };
    let shapes = frame(&ctx, vec![], &mut draw);
    let pos = shapes
        .texts
        .iter()
        .find(|(s, _)| s == "Pick gray")
        .unwrap()
        .1
        .center();
    frame(&ctx, click(pos, true), &mut draw);
    frame(&ctx, click(pos, false), &mut draw);
    let image = ImageLevels::from_source(
        tr_core::color::LinearImage::new(32, 16, vec![[0.2, 0.3, 0.4, 1.]; 512]).unwrap(),
        PreviewRequest::full(),
    )
    .unwrap();
    for stage in 0..3 {
        let sample = tr_render::Sample {
            x: 2,
            y: 1,
            working: if stage == 1 {
                [0.; 4]
            } else {
                [0.2, 0.3, 0.4, 1.]
            },
            display: [0; 4],
        };
        let mut draw = |ui: &mut egui::Ui| {
            let response = ui.interact(
                egui::Rect::from_min_max(egui::pos2(10., 10.), egui::pos2(290., 290.)),
                egui::Id::new("level-pick"),
                egui::Sense::click(),
            );
            app.layer_interaction(
                ui,
                &item,
                &response,
                [32, 16],
                (stage > 0).then_some(&image),
                Some(&sample),
            );
        };
        let p = egui::pos2(120., 120.);
        frame(&ctx, vec![], &mut draw);
        frame(&ctx, click(p, true), &mut draw);
        frame(&ctx, click(p, false), &mut draw);
        if stage < 2 {
            assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
            assert!(app.editing.layers.pick);
        }
    }
    assert!(!app.editing.layers.pick);
    let actual = app.editing.entries[&item.id].draft.as_ref().unwrap();
    assert_eq!(
        actual.layers.as_ref().unwrap().layers[0].operators[0],
        recipe.layers.as_ref().unwrap().layers[0].operators[0]
    );
    let Operator::TonalLevels(g) = &actual.layers.as_ref().unwrap().layers[0].operators[1] else {
        panic!()
    };
    assert_eq!(g.analysis.as_ref().unwrap().sample_xy, Some([2, 1]));
}

#[test]
fn color_wheel_drag_keyboard_reset_and_disabled_state_share_the_slider_values() {
    fn run(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        enabled: bool,
        hue: &mut f32,
        amount: &mut f32,
    ) -> (egui::Rect, bool) {
        let mut rect = egui::Rect::NOTHING;
        let mut commit = false;
        frame(ctx, events, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let r = super::layers::color_wheel(ui, Language::English, hue, amount);
                rect = r.rect;
                commit = r.drag_stopped() || (r.changed() && !r.dragged());
            });
        });
        (rect, commit)
    }
    for enabled in [true, false] {
        let ctx = egui::Context::default();
        let (mut hue, mut amount) = (20., 10.);
        let (rect, _) = run(&ctx, vec![], enabled, &mut hue, &mut amount);
        let start = rect.center() + egui::vec2(35., 0.);
        let end = rect.center() + egui::vec2(0., -40.);
        run(&ctx, click(start, true), enabled, &mut hue, &mut amount);
        assert!(
            !run(
                &ctx,
                vec![egui::Event::PointerMoved(end)],
                enabled,
                &mut hue,
                &mut amount
            )
            .1
        );
        let committed = run(&ctx, click(end, false), enabled, &mut hue, &mut amount).1;
        assert_eq!(committed, enabled);
        if enabled {
            assert!((hue - 90.).abs() < 1e-4 && amount > 60.);
        } else {
            assert_eq!((hue, amount), (20., 10.));
        }
        for (key, modifiers) in [
            (egui::Key::ArrowRight, egui::Modifiers::SHIFT),
            (egui::Key::Home, egui::Modifiers::NONE),
        ] {
            let event = egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers,
            };
            let changed = run(&ctx, vec![event], enabled, &mut hue, &mut amount).1;
            assert_eq!(changed, enabled);
            if enabled {
                if key == egui::Key::Home {
                    assert_eq!((hue, amount), (0., 0.));
                } else {
                    assert!((hue - 100.).abs() < 1e-4);
                }
            } else {
                assert_eq!((hue, amount), (20., 10.));
            }
        }
    }
}

#[test]
fn selective_and_tonal_panels_preserve_saved_values_in_both_languages() {
    use tr_core::editing::layers::*;
    for lang in [Language::Italian, Language::English] {
        let mut operators = crate::verify_advanced::selective_recipe(Default::default(), true)
            .layers
            .unwrap()
            .layers
            .remove(0)
            .operators;
        operators.push(Operator::ExposureGamma(ExposureGamma {
            exposure: -10.,
            offset: -4.,
            gamma: 0.25,
            ..Default::default()
        }));
        operators.push(Operator::TonalAdjustments(TonalAdjustments {
            pivot: 4.,
            whites: 100.,
            blacks: -100.,
            ..Default::default()
        }));
        for op in operators {
            let ctx = egui::Context::default();
            let title = lang.text(op.tool().name).to_owned();
            if lang == Language::English {
                assert_ne!(title, op.tool().name);
            }
            let mut r = EditRecipe::neutral(Default::default());
            r.layer_stack().layers.push(Layer::new("Local color", op));
            let before = r.clone();
            let mut controls = super::layers::Controls::default();
            controls.bind("A");
            for _ in 0..2 {
                let shapes = frame(&ctx, vec![], |ui| {
                    assert_eq!(controls.show(ui, lang, &mut r), (false, false));
                });
                assert!(shapes.texts.iter().any(|(s, _)| s == &title));
            }
            r.validate().unwrap();
            assert_eq!(r, before);
        }
    }
}

#[test]
fn selective_family_navigation_is_not_an_edit_and_reset_is_limited_to_that_family() {
    use tr_core::editing::layers::*;
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut s = SelectiveColor::default();
        s.adjustments[0] = [10., 20., 30., 40.];
        s.adjustments[8] = [-10., -20., -30., -40.];
        let mut r = EditRecipe::neutral(Default::default());
        r.layer_stack()
            .layers
            .push(Layer::new("Selective", Operator::SelectiveColor(s.clone())));
        let mut controls = super::layers::Controls::default();
        controls.bind("A");
        let mut commits = 0;
        for (label, expected_commits) in [
            (format!("{} •", lang.text("Neri")), 0),
            (lang.text("Assoluto").into(), 1),
            (lang.text("Ripristina famiglia").into(), 2),
        ] {
            let mut draw = |ui: &mut egui::Ui| {
                commits += usize::from(controls.show(ui, lang, &mut r).1);
            };
            let shapes = frame(&ctx, vec![], &mut draw);
            let pos = shapes
                .texts
                .iter()
                .find(|(s, _)| s == &label)
                .unwrap()
                .1
                .center();
            frame(&ctx, click(pos, true), &mut draw);
            frame(&ctx, click(pos, false), &mut draw);
            assert_eq!(commits, expected_commits);
            let Operator::SelectiveColor(current) =
                &r.layers.as_ref().unwrap().layers[0].operators[0]
            else {
                unreachable!()
            };
            assert_eq!(current.adjustments[0], s.adjustments[0]);
            if expected_commits == 0 {
                assert_eq!(current, &s);
            }
            if expected_commits == 2 {
                assert_eq!(current.adjustments[8], [0.; 4]);
                assert_eq!(current.method, SelectiveMethod::Absolute);
            }
        }
    }
}

#[test]
fn selective_recipe_methods_reopen_undo_redo_without_spurious_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    let item = catalog
        .observe(Path::new("synthetic.png"), "synthetic", 4)
        .unwrap();
    let relative = crate::verify_advanced::selective_recipe(Default::default(), false);
    let first = catalog.save_edit(&item.id, 0, &relative).unwrap();
    let absolute = crate::verify_advanced::selective_recipe(Default::default(), true);
    let saved = catalog
        .save_edit(&item.id, first.generation, &absolute)
        .unwrap();
    drop(catalog);
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    assert_eq!(
        catalog
            .load_edit(&item.id, Default::default())
            .unwrap()
            .recipe,
        absolute
    );
    let undo = catalog.step_edit(&item.id, saved.generation, true).unwrap();
    assert_eq!(undo.recipe, relative);
    let redo = catalog.step_edit(&item.id, undo.generation, false).unwrap();
    assert_eq!(redo.recipe, absolute);
    assert_eq!(
        catalog
            .save_edit(&item.id, redo.generation, &absolute)
            .unwrap()
            .generation,
        redo.generation
    );
}

#[test]
fn creative_panels_open_in_both_languages_without_changing_saved_values() {
    use tr_core::editing::layers::*;
    let recipe = crate::verify_advanced::toning_recipe(Default::default());
    for lang in [Language::Italian, Language::English] {
        for op in recipe.layers.as_ref().unwrap().layers[0].operators.clone() {
            let ctx = egui::Context::default();
            let mut r = EditRecipe::neutral(Default::default());
            r.layer_stack().layers.push(Layer::new("Look", op));
            let before = r.clone();
            let mut controls = super::layers::Controls::default();
            controls.bind("A");
            for _ in 0..2 {
                let shapes = frame(&ctx, vec![], |ui| {
                    assert_eq!(controls.show(ui, lang, &mut r), (false, false));
                });
                let title = lang.text(
                    before.layers.as_ref().unwrap().layers[0].operators[0]
                        .tool()
                        .name,
                );
                assert!(shapes.texts.iter().any(|(s, _)| s == title));
            }
            assert_eq!(r, before);
            if lang == Language::English {
                assert_ne!(
                    lang.text(
                        before.layers.as_ref().unwrap().layers[0].operators[0]
                            .tool()
                            .name
                    ),
                    before.layers.as_ref().unwrap().layers[0].operators[0]
                        .tool()
                        .name
                );
            }
        }
    }
}

#[test]
fn gradient_stop_insert_remove_is_reversible_and_commits_once_per_click() {
    use tr_core::editing::layers::*;
    let ctx = egui::Context::default();
    let original = GradientMap {
        amount: 100.,
        ..Default::default()
    };
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.layer_stack().layers.push(Layer::new(
        "Palette",
        Operator::GradientMap(original.clone()),
    ));
    let before = recipe.clone();
    let mut controls = super::layers::Controls::default();
    controls.bind("A");
    let mut commits = 0;
    for label in ["Add point", "Delete point"] {
        let mut draw = |ui: &mut egui::Ui| {
            commits += usize::from(controls.show(ui, Language::English, &mut recipe).1);
        };
        let shapes = frame(&ctx, vec![], &mut draw);
        let pos = shapes
            .texts
            .iter()
            .find(|(s, _)| s == label)
            .unwrap()
            .1
            .center();
        frame(&ctx, click(pos, true), &mut draw);
        frame(&ctx, click(pos, false), &mut draw);
        let Operator::GradientMap(g) = &recipe.layers.as_ref().unwrap().layers[0].operators[0]
        else {
            unreachable!()
        };
        assert_eq!(g.stops.len(), if label == "Add point" { 3 } else { 2 });
        for t in [-1., 0., 0.3, 0.7, 1., 3.] {
            assert_eq!(g.color_at(t), original.color_at(t));
        }
        recipe.validate().unwrap();
    }
    assert_eq!(commits, 2);
    assert_eq!(recipe, before);
}

#[test]
fn creative_recipe_reopens_and_undo_redo_preserve_all_operator_versions() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    let item = catalog
        .observe(Path::new("synthetic.png"), "synthetic", 4)
        .unwrap();
    let historical = crate::verify_advanced::layered_recipe(Default::default());
    let first = catalog.save_edit(&item.id, 0, &historical).unwrap();
    let creative = crate::verify_advanced::toning_recipe(Default::default());
    let saved = catalog
        .save_edit(&item.id, first.generation, &creative)
        .unwrap();
    drop(catalog);
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    let loaded = catalog.load_edit(&item.id, Default::default()).unwrap();
    assert_eq!(loaded.recipe, creative);
    let undo = catalog.step_edit(&item.id, saved.generation, true).unwrap();
    assert_eq!(undo.recipe, historical);
    let redo = catalog.step_edit(&item.id, undo.generation, false).unwrap();
    assert_eq!(redo.recipe, creative);
    assert_eq!(
        catalog
            .save_edit(&item.id, redo.generation, &creative)
            .unwrap()
            .generation,
        redo.generation
    );
}

#[test]
fn layer_panel_open_and_scope_changes_do_not_rewrite_extended_values() {
    use tr_core::editing::layers::{Layer, Operator, TonalLevel};
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut levels = [TonalLevel::default(); 4];
        levels[0].black = -5.;
        levels[0].white = 8.;
        levels[0].output = [-4., 12.];
        let mut recipe = EditRecipe::neutral(Default::default());
        recipe
            .layer_stack()
            .layers
            .push(Layer::new("Extended", Operator::Levels(levels)));
        let before = recipe.clone();
        let mut controls = super::layers::Controls::default();
        controls.bind("A");
        for _ in 0..2 {
            frame(&ctx, vec![], |ui| {
                assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
            });
        }
        assert_eq!(recipe, before);
        controls.pick = true;
        controls.overlay = true;
        controls.paint = true;
        controls.bind("B");
        assert!(
            !controls.pick && !controls.overlay && !controls.paint && controls.selected.is_none()
        );
        let mut viewed = recipe.clone();
        controls.preview_input("A", &mut viewed);
        assert_eq!(viewed, recipe);
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn layered_brush_is_source_anchored_and_escape_restores_the_whole_gesture() {
    use tr_core::editing::{
        Advanced,
        layers::{Combine, Layer, MaskKind, Operator},
        masks::{Mask, Shape},
    };
    for cancel in [false, true] {
        let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
        crate::ui::settings_regressions::settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.require_process(3);
        let mut a = Advanced::default();
        a.geometry.crop = [0.125, 0., 0.875, 1.];
        recipe.advanced = Some(Box::new(a));
        let mut layer = Layer::new(
            "Brush",
            Operator::Light {
                exposure: 1.,
                temperature: 0.,
                tint: 0.,
                saturation: 0.,
            },
        );
        let component = layer.mask.append(
            MaskKind::Shape(Mask {
                shape: Shape::Brush,
                ..Default::default()
            }),
            Combine::Add,
        );
        let id = layer.id;
        recipe.layer_stack().layers.push(layer);
        app.state.current = Some(item.id.clone());
        app.editing.entries.insert(
            item.id.clone(),
            EditEntry {
                loaded: Some(LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: item.digest.clone(),
                    generation: 0,
                    revision: 0,
                    recipe: recipe.clone(),
                    can_undo: false,
                    can_redo: false,
                }),
                draft: Some(recipe.clone()),
                ..Default::default()
            },
        );
        app.editing.layers.bind(&item.id);
        app.editing.layers.view_layers = true;
        app.editing.layers.selected = Some(id);
        app.editing.layers.component = Some(component);
        app.editing.layers.paint = true;
        let mut draw = |ui: &mut egui::Ui| {
            let response = ui.interact(
                egui::Rect::from_min_max(egui::pos2(10., 10.), egui::pos2(290., 290.)),
                egui::Id::new("layer-brush"),
                egui::Sense::click_and_drag(),
            );
            app.layer_interaction(ui, &item, &response, [1024, 768], None, None);
        };
        frame(&ctx, vec![], &mut draw);
        frame(&ctx, click(egui::pos2(60., 150.), true), &mut draw);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(240., 150.))],
            &mut draw,
        );
        if cancel {
            frame(
                &ctx,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                &mut draw,
            );
        }
        frame(&ctx, click(egui::pos2(260., 150.), false), &mut draw);
        let entry = &app.editing.entries[&item.id];
        if cancel {
            assert_eq!(entry.draft.as_ref(), Some(&recipe));
            assert!(!entry.pending);
        } else {
            assert!(entry.pending);
            let MaskKind::Shape(m) = &entry
                .draft
                .as_ref()
                .unwrap()
                .layers
                .as_ref()
                .unwrap()
                .layers[0]
                .mask
                .nodes[0]
                .kind
            else {
                panic!("brush");
            };
            assert!((m.points[0][0] - (0.125 + 0.75 * 50. / 280.)).abs() < 1e-6);
            assert!((m.points.last().unwrap()[0] - (0.125 + 0.75 * 250. / 280.)).abs() < 1e-6);
        }
    }
}

#[derive(Default)]
struct Shapes {
    texts: Vec<(String, egui::Rect)>,
    knobs: Vec<egui::Pos2>,
}
fn collect(shape: &egui::Shape, out: &mut Shapes) {
    match shape {
        egui::Shape::Text(t) => out.texts.push((
            t.galley.job.text.clone(),
            t.galley.rect.translate(t.pos.to_vec2()),
        )),
        // Locate the actual handle, not the rail midpoint or a section's status dot.
        egui::Shape::Circle(c) if (4. ..15.).contains(&c.radius) => out.knobs.push(c.center),
        egui::Shape::Vec(v) => {
            for s in v {
                collect(s, out);
            }
        }
        _ => {}
    }
}
fn frame(ctx: &egui::Context, events: Vec<egui::Event>, draw: impl FnMut(&mut egui::Ui)) -> Shapes {
    frame_with_modifiers(ctx, events, egui::Modifiers::NONE, draw)
}
fn frame_with_modifiers(
    ctx: &egui::Context,
    mut events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    mut draw: impl FnMut(&mut egui::Ui),
) -> Shapes {
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.);
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320., 2400.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            ui.style_mut().animation_time = 0.;
            draw(ui);
        },
    );
    output.textures_delta.clear();
    let mut shapes = Shapes::default();
    for s in output.shapes {
        collect(&s.shape, &mut shapes);
    }
    shapes
}
fn click(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn native_wb_slider_is_continuous_and_does_not_change_as_shot_on_open() {
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut wb = tr_core::decoder::RawWhiteBalance::default();
        let shapes = frame(&ctx, vec![], |ui| {
            assert_eq!(apple_wb_controls(ui, lang, &mut wb), (false, false));
        });
        assert!(wb.is_as_shot());
        assert!(
            !shapes
                .texts
                .iter()
                .any(|(t, _)| t.contains("3200") || t.contains("7500"))
        );
        let knob = *shapes.knobs.first().expect("Temperature slider handle");
        frame(&ctx, click(knob, true), |ui| {
            apple_wb_controls(ui, lang, &mut wb);
        });
        let target = knob + egui::vec2(37., 0.);
        frame(&ctx, vec![egui::Event::PointerMoved(target)], |ui| {
            apple_wb_controls(ui, lang, &mut wb);
        });
        let mut committed = false;
        frame(&ctx, click(target, false), |ui| {
            committed = apple_wb_controls(ui, lang, &mut wb).1;
        });
        assert!(committed);
        assert!((6501..=50000).contains(&wb.apple_temperature));
        assert!(![3200, 5500, 6500, 7500].contains(&wb.apple_temperature));
        wb.validate_for(tr_core::decoder::RawEngine::Apple).unwrap();
        let saved = wb;
        frame(&ctx, vec![], |ui| {
            assert_eq!(apple_wb_controls(ui, lang, &mut wb), (false, false));
        });
        assert_eq!(wb, saved);
    }
}

#[test]
fn adjustment_drag_survives_changing_sample_labels_before_the_control() {
    let ctx = egui::Context::default();
    let mut value = 0.;
    let mut commits = 0;
    let mut draw = |ui: &mut egui::Ui, sample: bool| {
        if sample {
            ui.label("Last RGB sample");
        }
        let r = adjustment(
            ui,
            Language::English,
            "Local exposure",
            &mut value,
            -5. ..=5.,
            "",
            None,
        );
        commits += usize::from(r.drag_stopped() || (r.changed() && !r.dragged()));
    };
    let shapes = frame(&ctx, vec![], |ui| draw(ui, true));
    let start = shapes.knobs[0];
    frame(&ctx, click(start, true), |ui| draw(ui, true));
    let end = start + egui::vec2(45., 0.);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], |ui| {
        draw(ui, false)
    });
    frame(&ctx, click(end, false), |ui| draw(ui, true));
    assert!(
        value > 1.,
        "The whole drag, not just its initial press, must be applied"
    );
    assert_eq!(commits, 1);
}

#[test]
fn photographic_slider_track_is_continuous_with_legacy_steps() {
    for step in [0.05, 1.] {
        let ctx = egui::Context::default();
        let mut value = 0_f64;
        let mut draw = |ui: &mut egui::Ui| {
            adjustment(
                ui,
                Language::English,
                "Exposure",
                &mut value,
                -1. ..=1.,
                " EV",
                Some(step),
            );
        };
        let start = frame(&ctx, vec![], &mut draw).knobs[0];
        frame(&ctx, click(start, true), &mut draw);
        let end = start + egui::vec2(0.5, 0.);
        frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
        frame(&ctx, click(end, false), &mut draw);
        assert!(value > 0. && value < 0.05, "value={value} step={step}");
        assert!((value / step - (value / step).round()).abs() > 0.001);
    }
}

#[test]
fn photographic_slider_fine_drag_and_keyboard_leave_other_controls_unchanged() {
    let ctx = egui::Context::default();
    let mut values = [0.123456_f64, 0.7654321];
    let mut integer = 6500;
    let mut commits = 0;
    let mut draw = |ui: &mut egui::Ui| {
        let r = adjustment(
            ui,
            Language::English,
            "Exposure",
            &mut values[0],
            -10. ..=10.,
            " EV",
            Some(0.05),
        );
        commits += usize::from(r.drag_stopped() || (r.changed() && !r.dragged()));
        adjustment(
            ui,
            Language::English,
            "Texture",
            &mut values[1],
            -100. ..=100.,
            "",
            Some(1.),
        );
        adjustment(
            ui,
            Language::English,
            "Temperature",
            &mut integer,
            2000..=50000,
            " K",
            Some(1.),
        );
    };
    let start = frame(&ctx, vec![], &mut draw).knobs[0];
    let mut press = click(start, true);
    if let egui::Event::PointerButton { modifiers, .. } = &mut press[1] {
        *modifiers = egui::Modifiers::SHIFT;
    }
    frame_with_modifiers(&ctx, press, egui::Modifiers::SHIFT, &mut draw);
    let end = start + egui::vec2(1., 0.);
    frame_with_modifiers(
        &ctx,
        vec![egui::Event::PointerMoved(end)],
        egui::Modifiers::SHIFT,
        &mut draw,
    );
    // A second frame with the stationary pointer must not repeat the delta.
    frame_with_modifiers(&ctx, vec![], egui::Modifiers::SHIFT, &mut draw);
    // Releasing Shift with the mouse must not jump to the absolute track position.
    frame(&ctx, click(end, false), &mut draw);
    for (key, modifiers) in [
        (egui::Key::ArrowRight, egui::Modifiers::NONE),
        (egui::Key::ArrowLeft, egui::Modifiers::SHIFT),
    ] {
        frame_with_modifiers(
            &ctx,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            modifiers,
            &mut draw,
        );
    }
    assert!(
        (values[0] - (0.123456 + 0.01 + 0.01 - 0.001)).abs() < 1e-8,
        "fine drag/keyboard value={}",
        values[0]
    );
    assert_eq!(values[1], 0.7654321);
    assert_eq!(integer, 6500);
    assert_eq!(commits, 3, "One drag and two keyboard edits");
}
#[test]
fn advanced_panels_open_without_mutating_recipe_and_mask_add_commits_once() {
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut controls = advanced::Controls::default();
        let mut recipe = crate::verify_advanced::recipe(tr_core::decoder::RawEngine::Apple);
        let original = recipe.clone();
        let mut shapes = frame(&ctx, vec![], |ui| {
            assert_eq!(
                controls.show(ui, lang, &mut recipe, Some([6000, 4000])),
                (false, false)
            );
        });
        for title in [
            "Ritaglio e geometria",
            "Presenza e dettaglio",
            "Colore avanzato",
            "Maschere locali",
        ] {
            let rect = shapes
                .texts
                .iter()
                .find(|(t, _)| t == lang.text(title))
                .unwrap()
                .1;
            for pressed in [true, false] {
                frame(&ctx, click(rect.center(), pressed), |ui| {
                    controls.show(ui, lang, &mut recipe, Some([6000, 4000]));
                });
            }
            shapes = frame(&ctx, vec![], |ui| {
                controls.show(ui, lang, &mut recipe, Some([6000, 4000]));
            });
            assert_eq!(
                recipe, original,
                "Opening {title} must not rewrite stored parameters"
            );
            if title != "Maschere locali" {
                // Exercise closing too, keeping the next section visible in the viewport.
                for pressed in [true, false] {
                    frame(&ctx, click(rect.center(), pressed), |ui| {
                        controls.show(ui, lang, &mut recipe, Some([6000, 4000]));
                    });
                }
                shapes = frame(&ctx, vec![], |ui| {
                    controls.show(ui, lang, &mut recipe, Some([6000, 4000]));
                });
                assert_eq!(recipe, original);
            }
        }
        let add = shapes
            .texts
            .iter()
            .find(|(t, _)| t == lang.text("Aggiungi maschera"))
            .unwrap_or_else(|| panic!("Missing mask action in {lang:?}: {:?}", shapes.texts))
            .1
            .center();
        let mut commits = 0;
        for pressed in [true, false] {
            frame(&ctx, click(add, pressed), |ui| {
                commits += usize::from(controls.show(ui, lang, &mut recipe, Some([6000, 4000])).1);
            });
        }
        assert_eq!(commits, 1);
        assert_eq!(recipe.advanced.as_ref().unwrap().masks.len(), 3);
        recipe.validate().unwrap();
        assert_eq!(recipe.raw_wb, original.raw_wb);
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn slider_gesture_survives_histogram_layout_changes_and_saves_on_release() {
    let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
    crate::ui::settings_regressions::settle(&mut app, &ctx, true);
    let item = app.state.items[0].clone();
    let recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    app.editing.entries.insert(
        item.id.clone(),
        EditEntry {
            loaded: Some(LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 0,
                revision: 0,
                recipe: recipe.clone(),
                can_undo: false,
                can_redo: false,
            }),
            draft: Some(recipe),
            ..Default::default()
        },
    );
    let shapes = frame(&ctx, vec![], |ui| {
        ui.label("Histogram ready");
        app.editing_controls(ui, &item);
    });
    let exposure = shapes
        .texts
        .iter()
        .find(|(text, _)| text == app.cache_settings.language.text("Esposizione"))
        .unwrap()
        .1;
    let start = *shapes
        .knobs
        .iter()
        .find(|p| (p.y - exposure.center().y).abs() < 1.)
        .unwrap();
    frame(&ctx, click(start, true), |ui| {
        ui.label("Histogram ready");
        app.editing_controls(ui, &item);
    });
    let first = start + egui::vec2(24., 0.);
    frame(&ctx, vec![egui::Event::PointerMoved(first)], |ui| {
        app.editing_controls(ui, &item);
    });
    let ev = app.editing.entries[&item.id]
        .draft
        .as_ref()
        .unwrap()
        .exposure_ev;
    assert!(
        ev > 0.,
        "Histogram disappearance must not interrupt the drag"
    );
    assert!(!app.editing.entries[&item.id].pending);
    let end = start + egui::vec2(48., 0.);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], |ui| {
        ui.label("Histogram ready");
        app.editing_controls(ui, &item);
    });
    assert!(
        app.editing.entries[&item.id]
            .draft
            .as_ref()
            .unwrap()
            .exposure_ev
            > ev
    );
    assert!(!app.editing.entries[&item.id].pending);
    frame(&ctx, click(end, false), |ui| {
        app.editing_controls(ui, &item);
    });
    assert!(
        app.editing.entries[&item.id].pending,
        "Release must save the final value"
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn brush_gesture_needs_no_rendered_pixels_and_saves_the_release_position() {
    use tr_core::editing::{
        Advanced,
        masks::{Mask, Shape},
    };
    for (release_outside, initial_points) in [(false, 0), (true, 0), (false, 511)] {
        let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
        crate::ui::settings_regressions::settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.process_version = 3;
        let mut advanced = Advanced::default();
        advanced.geometry.crop = [0.125, 0., 0.875, 1.];
        advanced.masks.push(Mask {
            shape: Shape::Brush,
            points: vec![[0.05, 0.1]; initial_points],
            exposure: 1.,
            ..Default::default()
        });
        recipe.advanced = Some(Box::new(advanced));
        app.editing.entries.insert(
            item.id.clone(),
            EditEntry {
                loaded: Some(LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: item.digest.clone(),
                    generation: 0,
                    revision: 0,
                    recipe: recipe.clone(),
                    can_undo: false,
                    can_redo: false,
                }),
                draft: Some(recipe),
                ..Default::default()
            },
        );
        app.editing.advanced.paint = true;
        let mut draw = |ui: &mut egui::Ui| {
            let rect = egui::Rect::from_min_max(egui::pos2(10., 10.), egui::pos2(290., 290.));
            let response = ui.interact(
                rect,
                egui::Id::new("mask-without-pixels"),
                egui::Sense::click_and_drag(),
            );
            app.local_mask_interaction(ui, &item, &response, [1024, 768]);
        };
        frame(&ctx, vec![], &mut draw);
        frame(&ctx, click(egui::pos2(60., 150.), true), &mut draw);
        for x in [140., 240.] {
            frame(
                &ctx,
                vec![egui::Event::PointerMoved(egui::pos2(x, 150.))],
                &mut draw,
            );
        }
        let x = if release_outside { 319. } else { 260. };
        frame(&ctx, click(egui::pos2(x, 150.), false), &mut draw);
        let e = &app.editing.entries[&item.id];
        assert!(
            e.pending,
            "Release must save even outside the image or before a rendered frame"
        );
        let mask = &e.draft.as_ref().unwrap().advanced.as_ref().unwrap().masks[0];
        let first = &mask.points[initial_points];
        assert!(
            (first[0] - (0.125 + 0.75 * (60. - 10.) / 280.)).abs() < 1e-6,
            "The brush must include the initial press, got {first:?}"
        );
        assert!((first[1] - 0.5).abs() < 1e-6);
        let last = mask.points.last().unwrap();
        let x = if initial_points == 511 {
            assert_eq!(mask.points.len(), 512);
            assert_eq!(mask.breaks, [511]);
            60.
        } else if release_outside {
            240.
        } else {
            260.
        };
        assert!((last[0] - (0.125 + 0.75 * (x - 10.) / 280.)).abs() < 1e-6);
        assert!((last[1] - 0.5).abs() < 1e-6);
        e.draft.as_ref().unwrap().validate().unwrap();
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn straightening_uses_the_press_even_when_the_first_move_reaches_the_endpoint() {
    let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
    crate::ui::settings_regressions::settle(&mut app, &ctx, true);
    let item = app.state.items[0].clone();
    let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.process_version = 3;
    let mut advanced = tr_core::editing::Advanced::default();
    advanced.geometry.crop = [0.125, 0., 0.875, 1.];
    recipe.advanced = Some(Box::new(advanced));
    app.editing.entries.insert(
        item.id.clone(),
        EditEntry {
            loaded: Some(LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 0,
                revision: 0,
                recipe: recipe.clone(),
                can_undo: false,
                can_redo: false,
            }),
            draft: Some(recipe),
            ..Default::default()
        },
    );
    app.editing.advanced.horizon = true;
    let mut draw = |ui: &mut egui::Ui| {
        let response = ui.interact(
            egui::Rect::from_min_max(egui::pos2(10., 10.), egui::pos2(290., 290.)),
            egui::Id::new("horizon-without-pixels"),
            egui::Sense::click_and_drag(),
        );
        app.local_mask_interaction(ui, &item, &response, [1024, 768]);
    };
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, click(egui::pos2(60., 100.), true), &mut draw);
    let end = egui::pos2(240., 160.);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
    frame(&ctx, click(end, false), &mut draw);
    let e = &app.editing.entries[&item.id];
    assert!(e.pending, "Release must save the straightening correction");
    let actual = e
        .draft
        .as_ref()
        .unwrap()
        .advanced
        .as_ref()
        .unwrap()
        .geometry
        .angle;
    assert!((actual + 60_f32.atan2(180.).to_degrees()).abs() < 1e-4);
    assert!(!app.editing.advanced.horizon);
    e.draft.as_ref().unwrap().validate().unwrap();
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn photo_gestures_preserve_recipes_while_busy_or_on_the_other_comparison_photo() {
    for horizon in [false, true] {
        for blocked in ["save", "raw-wb", "other-photo"] {
            let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
            crate::ui::settings_regressions::settle(&mut app, &ctx, true);
            let item = app.state.items[0].clone();
            let recipe = crate::verify_advanced::recipe(app.cache_settings.raw_engine);
            app.editing.entries.insert(
                item.id.clone(),
                EditEntry {
                    loaded: Some(LoadedEdit {
                        asset_id: item.id.clone(),
                        source_digest: item.digest.clone(),
                        generation: 0,
                        revision: 0,
                        recipe: recipe.clone(),
                        can_undo: false,
                        can_redo: false,
                    }),
                    draft: Some(recipe.clone()),
                    pending: blocked == "save",
                    ..Default::default()
                },
            );
            app.state.current = Some(if blocked == "other-photo" {
                app.state.items[1].id.clone()
            } else {
                item.id.clone()
            });
            app.editing.wb_pending = blocked == "raw-wb";
            app.editing.advanced.horizon = horizon;
            app.editing.advanced.paint = !horizon;
            let mut draw = |ui: &mut egui::Ui| {
                let response = ui.interact(
                    egui::Rect::from_min_max(egui::pos2(10., 10.), egui::pos2(290., 290.)),
                    egui::Id::new("blocked-photo-gesture"),
                    egui::Sense::click_and_drag(),
                );
                app.local_mask_interaction(ui, &item, &response, [1024, 768]);
            };
            frame(&ctx, vec![], &mut draw);
            frame(&ctx, click(egui::pos2(110., 100.), true), &mut draw);
            let end = egui::pos2(210., 160.);
            frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
            frame(&ctx, click(end, false), &mut draw);
            let entry = &app.editing.entries[&item.id];
            assert_eq!(
                entry.draft.as_ref(),
                Some(&recipe),
                "{blocked}, horizon={horizon}"
            );
            assert_eq!(
                entry.pending,
                blocked == "save",
                "No unexpected save: {blocked}"
            );
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn resetting_masks_restores_panning_even_if_draw_mode_was_left_on() {
    let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
    crate::ui::settings_regressions::settle(&mut app, &ctx, true);
    let item = app.state.items[0].clone();
    let recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    app.state.current = Some(item.id.clone());
    app.editing.entries.insert(
        item.id.clone(),
        EditEntry {
            loaded: Some(LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 0,
                revision: 0,
                recipe: recipe.clone(),
                can_undo: false,
                can_redo: false,
            }),
            draft: Some(recipe.clone()),
            ..Default::default()
        },
    );
    app.editing.advanced.paint = true;
    app.state.transform.zoom = Some(2.);
    let original_center = app.state.transform.center;
    let image = Arc::new(
        ImageLevels::from_source(
            tr_core::color::LinearImage::new(256, 256, vec![[0.2, 0.2, 0.2, 1.]; 256 * 256])
                .unwrap(),
            PreviewRequest::full(),
        )
        .unwrap(),
    );
    let mut draw = |ui: &mut egui::Ui| {
        let editing_gesture = app.photo_gesture_active(&item.id);
        let (response, _) = tr_render::viewport(
            ui,
            &mut app.presenter,
            &image,
            &mut app.state.transform,
            "pan-after-mask-reset",
            Some(image.source_size()),
            editing_gesture,
        );
        app.local_mask_interaction(ui, &item, &response, [256, 256]);
    };
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, click(egui::pos2(110., 100.), true), &mut draw);
    let end = egui::pos2(210., 160.);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
    frame(&ctx, click(end, false), &mut draw);
    assert!(app.state.transform.center[0] < original_center[0]);
    assert!(app.state.transform.center[1] < original_center[1]);
    assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
    assert!(!app.editing.entries[&item.id].pending);
}

#[test]
fn double_click_slider_resets_explicit_default_but_disabled_controls_do_not() {
    for (initial, default, range) in [
        (3., 0., -10. ..=10.),
        (2., 1., 0.25..=4.),
        (0.8, 0.5, 0. ..=1.),
    ] {
        for enabled in [true, false] {
            let ctx = egui::Context::default();
            let mut value = initial;
            let mut resets = 0;
            let mut draw = |ui: &mut egui::Ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let (r, reset) = adjustment_default(
                        ui,
                        Language::English,
                        "Test",
                        &mut value,
                        range.clone(),
                        "",
                        None,
                        default,
                    );
                    if reset {
                        assert!(r.changed() && !r.dragged());
                        resets += 1;
                    }
                });
            };
            let shapes = frame(&ctx, vec![], &mut draw);
            let pos = shapes.knobs[0];
            for pressed in [true, false, true, false] {
                frame(&ctx, click(pos, pressed), &mut draw);
            }
            assert_eq!(value, if enabled { default } else { initial });
            assert_eq!(resets, usize::from(enabled));
        }
    }
}

#[test]
fn double_click_apple_wb_sliders_returns_to_as_shot() {
    for slider in 0..2 {
        let ctx = egui::Context::default();
        let mut wb = tr_core::decoder::RawWhiteBalance {
            apple_temperature: 4500,
            apple_tint: 35,
            ..Default::default()
        };
        let mut draw = |ui: &mut egui::Ui| {
            apple_wb_controls(ui, Language::Italian, &mut wb);
        };
        let shapes = frame(&ctx, vec![], &mut draw);
        let pos = shapes.knobs[slider];
        for pressed in [true, false, true, false] {
            frame(&ctx, click(pos, pressed), &mut draw);
        }
        assert!(wb.is_as_shot(), "slider {slider}: {wb:?}");
    }
}

#[test]
fn reset_all_button_restores_wb_geometry_masks_and_curve_with_durable_undo() {
    for enabled in [true, false] {
        let ctx = egui::Context::default();
        let mut recipe = crate::verify_advanced::recipe(tr_core::decoder::RawEngine::TrueRenderer);
        recipe.raw_wb.red = 1500;
        recipe.process_version = 4;
        recipe.curve = vec![
            CurvePoint { x: 0.1, y: 0.2 },
            CurvePoint { x: 0.9, y: 0.85 },
        ];
        let original = recipe.clone();
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
        let item = catalog
            .observe(Path::new("synthetic.dng"), "synthetic", 4)
            .unwrap();
        let saved = catalog.save_edit(&item.id, 0, &original).unwrap();
        let mut commits = 0;
        let mut draw = |ui: &mut egui::Ui| {
            ui.add_enabled_ui(enabled, |ui| {
                commits += usize::from(reset_all_button(ui, Language::English, &mut recipe));
            });
        };
        let shapes = frame(&ctx, vec![], &mut draw);
        let pos = shapes
            .texts
            .iter()
            .find(|(s, _)| s == "Reset all")
            .unwrap()
            .1
            .center();
        frame(&ctx, click(pos, true), &mut draw);
        frame(&ctx, click(pos, false), &mut draw);
        if enabled {
            assert_eq!(commits, 1);
            assert_eq!(recipe, EditRecipe::neutral(original.raw_engine));
            let reset = catalog
                .save_edit(&item.id, saved.generation, &recipe)
                .unwrap();
            let undo = catalog.step_edit(&item.id, reset.generation, true).unwrap();
            assert_eq!(undo.recipe, original);
            drop(catalog);
            let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
            let redo = catalog.step_edit(&item.id, undo.generation, false).unwrap();
            assert_eq!(redo.recipe, recipe);
        } else {
            assert_eq!(commits, 0);
            assert_eq!(recipe, original);
        }
    }
}

#[test]
fn tools_create_layers_or_populate_selected_empty_layer_in_both_languages() {
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut controls = super::layers::Controls::default();
        let mut recipe = EditRecipe::neutral(Default::default());
        let mut commits = 0;
        for label in [
            "Nuovo livello",
            "Nuovo livello vuoto",
            "Aggiungi regolazione",
            "Luce e colore locale",
        ] {
            let mut draw = |ui: &mut egui::Ui| {
                if controls.view_tools {
                    commits += usize::from(controls.show_tools(ui, lang, &mut recipe));
                } else {
                    commits += usize::from(controls.show(ui, lang, &mut recipe).1);
                }
            };
            let shapes = frame(&ctx, vec![], &mut draw);
            let pos = shapes
                .texts
                .iter()
                .find(|(s, _)| s == lang.text(label))
                .expect(label)
                .1
                .center();
            frame(&ctx, click(pos, true), &mut draw);
            frame(&ctx, click(pos, false), &mut draw);
            if label == "Nuovo livello vuoto" {
                assert!(
                    recipe.layers.as_ref().unwrap().layers[0]
                        .operators
                        .is_empty()
                );
            }
        }
        assert_eq!(commits, 2);
        let stack = recipe.layers.as_ref().unwrap();
        assert_eq!(stack.layers.len(), 1);
        assert_eq!(stack.layers[0].operators.len(), 1);
        let id = stack.layers[0].id;
        let shapes = frame(&ctx, vec![], |ui| {
            controls.show(ui, lang, &mut recipe);
        });
        assert!(shapes.texts.iter().any(|(s, _)| s == lang.text("Opacità")));
        assert!(
            !shapes
                .texts
                .iter()
                .any(|(s, _)| s == lang.text("Riempimento"))
        );
        controls.open_tools(false);
        let before = recipe.clone();
        let shapes = frame(&ctx, vec![], |ui| {
            assert!(!controls.show_tools(ui, lang, &mut recipe));
        });
        assert_eq!(recipe, before);
        let pos = shapes
            .texts
            .iter()
            .find(|(s, _)| s == lang.text("Luce e colore locale"))
            .unwrap()
            .1
            .center();
        frame(&ctx, click(pos, true), |ui| {
            controls.show_tools(ui, lang, &mut recipe);
        });
        frame(&ctx, click(pos, false), |ui| {
            assert!(controls.show_tools(ui, lang, &mut recipe));
        });
        let stack = recipe.layers.as_mut().unwrap();
        assert_eq!(stack.layers.len(), 2);
        assert_eq!(stack.layers[0].id, id);
        stack.layers[1].locked = true;
        controls.open_tools(true);
        let before = recipe.clone();
        frame(&ctx, vec![], |ui| {
            assert!(!controls.show_tools(ui, lang, &mut recipe));
        });
        frame(&ctx, click(pos, true), |ui| {
            assert!(!controls.show_tools(ui, lang, &mut recipe));
        });
        frame(&ctx, click(pos, false), |ui| {
            assert!(!controls.show_tools(ui, lang, &mut recipe));
        });
        assert_eq!(recipe, before);
        recipe.validate().unwrap();
    }
}

#[test]
fn empty_layer_fill_and_later_tool_survive_catalog_reopen_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    let item = catalog
        .observe(Path::new("synthetic.png"), &"a".repeat(64), 4)
        .unwrap();
    let mut empty = EditRecipe::neutral(Default::default());
    let mut layer = tr_core::editing::layers::Layer::empty("Empty");
    layer.opacity = 0.4;
    layer.fill = 0.6;
    empty.layer_stack().layers.push(layer);
    let first = catalog.save_edit(&item.id, 0, &empty).unwrap();
    let mut populated = empty.clone();
    populated.layers.as_mut().unwrap().layers[0]
        .operators
        .push(tr_core::editing::layers::TOOLS[1].operator());
    let second = catalog
        .save_edit(&item.id, first.generation, &populated)
        .unwrap();
    drop(catalog);
    let mut catalog = tr_store::Catalog::open(dir.path()).unwrap();
    assert_eq!(
        catalog
            .load_edit(&item.id, Default::default())
            .unwrap()
            .recipe,
        populated
    );
    let undo = catalog
        .step_edit(&item.id, second.generation, true)
        .unwrap();
    assert_eq!(undo.recipe, empty);
    let redo = catalog.step_edit(&item.id, undo.generation, false).unwrap();
    assert_eq!(redo.recipe, populated);
}

#[test]
fn sampled_color_overlays_are_scoped_to_each_operator_and_separate_from_mask() {
    use tr_core::editing::layers::*;
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut recipe = EditRecipe::neutral(Default::default());
        let mut layer = Layer::new("Two samples", TOOLS[1].operator());
        for reference in [[1., 0., 0.], [0., 0., 1.]] {
            layer.operators.push(Operator::SampleColor(SampleColor {
                range: ColorRange {
                    reference,
                    ..Default::default()
                },
                ..Default::default()
            }));
        }
        layer.mask.append(MaskKind::Constant(0.5), Combine::Add);
        layer.opacity = 0.2;
        layer.fill = 0.3;
        recipe.layer_stack().layers.push(layer);
        let before = recipe.clone();
        let mut controls = super::layers::Controls::default();
        for (which, expected) in [(0, [0.5, 0.]), (1, [0., 0.5]), (2, [0.5, 0.5])] {
            if which == 2 {
                let shapes = frame(&ctx, vec![], |ui| {
                    controls.show(ui, lang, &mut recipe);
                });
                let pos = shapes
                    .texts
                    .iter()
                    .find(|(s, _)| s == lang.text("Maschera"))
                    .unwrap()
                    .1
                    .center();
                for pressed in [true, false] {
                    frame(&ctx, click(pos, pressed), |ui| {
                        controls.show(ui, lang, &mut recipe);
                    });
                }
                assert!(!controls.overlay);
            }
            let mut draw = |ui: &mut egui::Ui| {
                assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
            };
            let shapes = frame(&ctx, vec![], &mut draw);
            let pos = shapes
                .texts
                .iter()
                .filter(|(s, _)| s == lang.text("Mostra area"))
                .nth(if which == 2 { 0 } else { which })
                .unwrap()
                .1
                .center();
            frame(&ctx, click(pos, true), &mut draw);
            frame(&ctx, click(pos, false), &mut draw);
            assert!(controls.overlay);
            let layer = &recipe.layers.as_ref().unwrap().layers[0];
            for (guide, expected) in [[1., 0., 0.], [0., 0., 1.]].into_iter().zip(expected) {
                assert!(
                    (controls.overlay_weight(layer, [0.5; 2], guide, 1.) - expected).abs() < 1e-6
                );
            }
            assert_eq!(recipe, before);
        }
    }
}

#[test]
fn tools_keyboard_cannot_apply_while_editing_is_disabled() {
    let ctx = egui::Context::default();
    let mut controls = super::layers::Controls::default();
    controls.open_tools(false);
    let mut recipe = EditRecipe::neutral(Default::default());
    let original = recipe.clone();
    let shapes = frame(&ctx, vec![], |ui| {
        controls.show_tools(ui, Language::English, &mut recipe);
    });
    let pos = shapes
        .texts
        .iter()
        .find(|(s, _)| s == "Search tools…")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        frame(&ctx, click(pos, pressed), |ui| {
            controls.show_tools(ui, Language::English, &mut recipe);
        });
    }
    assert!(ctx.memory(|m| m.focused().is_some()));
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, vec![enter], |ui| {
        ui.add_enabled_ui(false, |ui| {
            assert!(!controls.show_tools(ui, Language::English, &mut recipe));
        });
    });
    assert_eq!(recipe, original);
}

#[test]
fn tool_help_covers_titles_and_visible_parameters_without_changing_recipes() {
    use tr_core::editing::layers::{Layer, TOOLS};
    for lang in [Language::Italian, Language::English] {
        for tool in TOOLS {
            assert!(help::tool_text(lang, tool.id).is_some(), "{}", tool.id);
            let ctx = egui::Context::default();
            ctx.memory_mut(|m| m.set_everything_is_visible(true));
            let mut recipe = EditRecipe::neutral(Default::default());
            recipe
                .layer_stack()
                .layers
                .push(Layer::new("Help coverage", tool.operator()));
            let original = recipe.clone();
            let mut controls = layers::Controls::default();
            controls.bind("help");
            for _ in 0..2 {
                frame(&ctx, vec![], |ui| {
                    assert_eq!(
                        help::description(ui, lang, tool.name),
                        help::tool_text(lang, tool.id)
                    );
                    assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
                });
            }
            assert_eq!(recipe, original, "{}", tool.id);
            let missing = ctx
                .data(|d| d.get_temp::<Vec<String>>(egui::Id::new("missing-control-help")))
                .unwrap_or_default();
            assert!(missing.is_empty(), "{} {lang:?}: {missing:?}", tool.id);
        }
    }
}

#[test]
fn advanced_help_covers_parameters_without_rewriting_the_recipe() {
    use tr_core::editing::masks::Shape;
    for lang in [Language::Italian, Language::English] {
        for shape in [
            Shape::Radial,
            Shape::Linear,
            Shape::Brush,
            Shape::Luminance,
            Shape::Hue,
        ] {
            let ctx = egui::Context::default();
            ctx.memory_mut(|m| m.set_everything_is_visible(true));
            let mut recipe = crate::verify_advanced::recipe(Default::default());
            recipe.advanced.as_mut().unwrap().masks[0].shape = shape;
            let original = recipe.clone();
            let mut controls = advanced::Controls::default();
            for _ in 0..2 {
                frame(&ctx, vec![], |ui| {
                    assert_eq!(
                        controls.show(ui, lang, &mut recipe, Some([1200, 800])),
                        (false, false)
                    );
                });
            }
            assert_eq!(recipe, original);
            let missing = ctx
                .data(|d| d.get_temp::<Vec<String>>(egui::Id::new("missing-control-help")))
                .unwrap_or_default();
            assert!(missing.is_empty(), "{lang:?} {shape:?}: {missing:?}");
        }
    }
}

#[test]
fn tooltip_opens_on_parameter_name_and_does_not_edit_the_value() {
    for lang in [Language::Italian, Language::English] {
        for enabled in [true, false] {
            let ctx = egui::Context::default();
            ctx.all_styles_mut(|s| {
                s.interaction.tooltip_delay = 0.;
                s.interaction.tooltip_grace_time = 0.;
            });
            let mut value = 0.;
            let draw = |ui: &mut egui::Ui, value: &mut f32| {
                ui.add_enabled_ui(enabled, |ui| {
                    adjustment(
                        ui,
                        lang,
                        lang.text("Esposizione"),
                        value,
                        -10. ..=10.,
                        " EV",
                        None,
                    );
                });
            };
            let shapes = frame(&ctx, vec![], |ui| draw(ui, &mut value));
            let label = shapes
                .texts
                .iter()
                .find(|(s, _)| s == lang.text("Esposizione"))
                .unwrap()
                .1
                .center();
            frame(&ctx, vec![egui::Event::PointerMoved(label)], |ui| {
                draw(ui, &mut value)
            });
            let shapes = frame(&ctx, vec![], |ui| draw(ui, &mut value));
            assert_eq!(value, 0.);
            assert!(
                shapes.texts.iter().any(|(s, _)| s.contains("+1 EV")),
                "{lang:?} enabled={enabled}: {:?}",
                shapes.texts
            );
        }
    }
}

#[test]
fn colorize_field_drag_keyboard_and_disabled_state() {
    for enabled in [true, false] {
        let ctx = egui::Context::default();
        let (mut hue, mut chroma) = (30., 50.);
        let mut rect = egui::Rect::NOTHING;
        frame(&ctx, vec![], |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                rect = visuals::color_field(ui, Language::Italian, &mut hue, &mut chroma).rect;
            });
        });
        let target = rect.center() + egui::vec2(20., -15.);
        for pressed in [true, false] {
            frame(&ctx, click(target, pressed), |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    visuals::color_field(ui, Language::Italian, &mut hue, &mut chroma);
                });
            });
        }
        if enabled {
            assert!(hue > 180. && chroma > 50.);
        } else {
            assert_eq!((hue, chroma), (30., 50.));
        }
        let event = egui::Event::Key {
            key: egui::Key::Home,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, vec![event], |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                visuals::color_field(ui, Language::Italian, &mut hue, &mut chroma);
            });
        });
        assert_eq!((hue, chroma), if enabled { (0., 0.) } else { (30., 50.) });
    }
}

#[test]
fn photographic_tools_fit_the_inspector_width() {
    use tr_core::editing::layers::{Layer, TOOLS};
    for lang in [Language::Italian, Language::English] {
        for tool in TOOLS {
            for width in [280., 344., 420.] {
                let ctx = egui::Context::default();
                crate::ui::style::apply(&ctx);
                let mut recipe = EditRecipe::neutral(Default::default());
                recipe
                    .layer_stack()
                    .layers
                    .push(Layer::new(tool.name, tool.operator()));
                let mut controls = layers::Controls::default();
                let mut bounds = (egui::Rect::NOTHING, egui::Rect::NOTHING);
                for _ in 0..8 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1440., 1600.),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            let panel = egui::Panel::right("tool-width")
                                .exact_size(width)
                                .frame(crate::ui::style::panel())
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    panels::control_style(ui);
                                    ui.style_mut().spacing.scroll.floating = false;
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            controls.show(ui, lang, &mut recipe);
                                            bounds.1 = ui.min_rect();
                                        });
                                });
                            bounds.0 = panel.response.rect;
                            egui::CentralPanel::default().show(ui, |_| {});
                        },
                    );
                    output.textures_delta.clear();
                }
                assert!(
                    bounds.1.left() >= bounds.0.left()
                        && bounds.1.right() <= bounds.0.right()
                        && bounds.0.right() <= 1440.,
                    "{lang:?} {width} {}: {bounds:?}",
                    tool.id
                );
            }
        }
    }
}

#[test]
fn layer_navigation_and_mask_tabs_preserve_exact_recipe_values() {
    use tr_core::editing::layers::{Layer, TOOLS};
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut recipe = EditRecipe::neutral(Default::default());
        for name in ["Background correction", "Subject correction"] {
            let mut layer = Layer::new(name, TOOLS[1].operator());
            layer.opacity = 0.333317;
            layer.fill = 0.712349;
            recipe.layer_stack().layers.push(layer);
        }
        let original = recipe.clone();
        let mut controls = layers::Controls::default();
        for label in [
            "Background correction",
            lang.text("Maschera"),
            lang.text("Effetti del livello"),
        ] {
            let shapes = frame(&ctx, vec![], |ui| {
                assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
            });
            let pos = shapes
                .texts
                .iter()
                .find(|(text, _)| text == label)
                .expect(label)
                .1
                .center();
            controls.pick = true;
            controls.paint = true;
            controls.overlay = true;
            for pressed in [true, false] {
                frame(&ctx, click(pos, pressed), |ui| {
                    assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
                });
            }
            assert!(!controls.pick && !controls.paint && !controls.overlay);
            let visible = frame(&ctx, vec![], |ui| {
                controls.show(ui, lang, &mut recipe);
            });
            if label == lang.text("Maschera") {
                assert!(
                    visible
                        .texts
                        .iter()
                        .any(|(text, _)| text == lang.text("Crea maschera"))
                );
                assert!(
                    !visible
                        .texts
                        .iter()
                        .any(|(text, _)| text == lang.text("Aggiungi regolazione"))
                );
            }
            assert_eq!(recipe, original);
        }
        assert_eq!(
            controls.selected,
            Some(recipe.layers.as_ref().unwrap().layers[0].id)
        );
        let shapes = frame(&ctx, vec![], |ui| {
            controls.show(ui, lang, &mut recipe);
        });
        let menu = shapes
            .texts
            .iter()
            .find(|(s, _)| s == "…")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            frame(&ctx, click(menu, pressed), |ui| {
                controls.show(ui, lang, &mut recipe);
            });
        }
        let shapes = frame(&ctx, vec![], |ui| {
            assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
        });
        assert!(
            shapes
                .texts
                .iter()
                .any(|(s, _)| s == lang.text("Nome del livello"))
        );
        assert!(
            shapes
                .texts
                .iter()
                .any(|(s, _)| s == lang.text("Riempimento"))
        );
        assert_eq!(recipe, original);
    }
}

#[test]
fn layer_percentage_entry_changes_only_on_input_and_commits_once() {
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut value = 0.333317_f32;
        let mut commits = 0;
        let mut draw = |ui: &mut egui::Ui| {
            let mut commit = false;
            layers::percent_slider(ui, lang, "Opacità", &mut value, &mut commit);
            commits += usize::from(commit);
        };
        let shapes = frame(&ctx, vec![], &mut draw);
        let pos = shapes
            .texts
            .iter()
            .find(|(text, _)| text.ends_with(" %"))
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            frame(&ctx, click(pos, pressed), &mut draw);
        }
        frame(
            &ctx,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text("42.5137".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            &mut draw,
        );
        assert!((value - 0.425137).abs() < 1e-7);
        assert_eq!(commits, 1);
    }
}

#[test]
fn panel_modified_badge_does_not_interrupt_a_slider_gesture() {
    let ctx = egui::Context::default();
    let mut value = 0_f32;
    let mut commits = 0;
    let mut control_id = None;
    let mut draw = |ui: &mut egui::Ui| {
        panels::Section::new(Language::English, "gesture", "Luce")
            .modified(value != 0.)
            .show(ui, |ui| {
                let r = adjustment(
                    ui,
                    Language::English,
                    "Exposure",
                    &mut value,
                    -10. ..=10.,
                    " EV",
                    None,
                );
                if let Some(id) = control_id {
                    assert_eq!(r.id, id);
                } else {
                    control_id = Some(r.id);
                }
                commits += usize::from(r.drag_stopped() || (r.changed() && !r.dragged()));
            });
    };
    let shapes = frame(&ctx, vec![], &mut draw);
    let knob = shapes.knobs[0];
    frame(&ctx, click(knob, true), &mut draw);
    for x in [10., 25., 40.] {
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(knob + egui::vec2(x, 0.))],
            &mut draw,
        );
    }
    frame(&ctx, click(knob + egui::vec2(40., 0.), false), &mut draw);
    assert!(value > 0.);
    assert_eq!(commits, 1);
}

#[test]
fn adjustment_rows_align_labels_sliders_and_values_without_wrapping() {
    for lang in [Language::Italian, Language::English] {
        for width in [220., 260., 306., 364.] {
            let ctx = egui::Context::default();
            crate::ui::style::apply(&ctx);
            let mut values = [0.75_f32, -12.5, 33.3];
            let original = values;
            let mut responses = Vec::new();
            let labels = ["Esposizione", "Alte luci", "Luminosità"];
            let shapes = frame(&ctx, vec![], |ui| {
                ui.set_width(width);
                panels::control_style(ui);
                responses.clear();
                for (i, label) in labels.iter().enumerate() {
                    let r = adjustment(
                        ui,
                        lang,
                        label,
                        &mut values[i],
                        -100. ..=100.,
                        if i == 0 { " EV" } else { "" },
                        None,
                    );
                    assert!(!r.changed());
                    responses.push(r.rect);
                }
            });
            assert_eq!(values, original);
            let first_label = shapes
                .texts
                .iter()
                .find(|(s, _)| s == lang.text(labels[0]))
                .unwrap()
                .1;
            for (i, label) in labels.iter().enumerate() {
                let text = shapes
                    .texts
                    .iter()
                    .find(|(s, _)| s == lang.text(label))
                    .unwrap()
                    .1;
                assert!(text.height() < 20., "{lang:?} {width}: {text:?}");
                assert!((text.left() - first_label.left()).abs() < 0.5);
                assert!(
                    (text.center().y - responses[i].center().y).abs() < 2.,
                    "{lang:?} {width}: {text:?}, {:?}",
                    responses[i]
                );
                assert!(
                    responses[i].right() <= width + 0.5,
                    "{width}: {:?}",
                    responses[i]
                );
                assert!((responses[i].right() - responses[0].right()).abs() < 0.5);
                assert!(responses[i].height() <= 28.);
            }
        }
    }
}

#[test]
fn layer_mask_creation_and_selection_keep_the_tree_separate_from_controls() {
    use tr_core::editing::{
        layers::{Combine, Layer, MaskKind, TOOLS},
        masks::{Mask, Shape},
    };
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut recipe = EditRecipe::neutral(Default::default());
        let layer = Layer::new("Local", TOOLS[1].operator());
        let id = layer.id;
        recipe.layer_stack().layers.push(layer);
        let mut controls = layers::Controls::default();
        controls.selected = Some(id);
        controls.mask_view = true;
        let mut commits = 0;
        for label in ["Crea maschera", "Pennello"] {
            let mut draw = |ui: &mut egui::Ui| {
                commits += usize::from(controls.show(ui, lang, &mut recipe).1);
            };
            let shapes = frame(&ctx, vec![], &mut draw);
            let pos = shapes
                .texts
                .iter()
                .find(|(s, _)| s == lang.text(label))
                .expect(label)
                .1
                .center();
            for pressed in [true, false] {
                frame(&ctx, click(pos, pressed), &mut draw);
            }
        }
        assert_eq!(commits, 1);
        assert!(controls.paint && controls.overlay);
        let layer = &mut recipe.layers.as_mut().unwrap().layers[0];
        assert_eq!(layer.mask.nodes.len(), 1);
        layer.mask.append(
            MaskKind::Shape(Mask {
                shape: Shape::Radial,
                ..Default::default()
            }),
            Combine::Subtract,
        );
        let before = recipe.clone();
        let shapes = frame(&ctx, vec![], |ui| {
            assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
        });
        let brush = shapes
            .texts
            .iter()
            .find(|(s, _)| s == lang.text("Pennello"))
            .unwrap()
            .1;
        let radial = shapes
            .texts
            .iter()
            .find(|(s, _)| s == lang.text("Radiale"))
            .unwrap()
            .1;
        let radius = shapes
            .texts
            .iter()
            .find(|(s, _)| s == lang.text("Raggio maschera"))
            .unwrap()
            .1;
        assert!(radius.top() > brush.bottom() && radius.top() > radial.bottom());
        for pressed in [true, false] {
            frame(&ctx, click(radial.center(), pressed), |ui| {
                assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
            });
        }
        assert!(!controls.paint && !controls.pick);
        assert_eq!(recipe, before);
        recipe.validate().unwrap();
    }
}

#[test]
fn a_full_layer_stack_keeps_the_selected_inspector_near_the_top() {
    use tr_core::editing::layers::{Layer, MAX_LAYERS, TOOLS};
    for lang in [Language::Italian, Language::English] {
        let ctx = egui::Context::default();
        let mut recipe = EditRecipe::neutral(Default::default());
        for i in 0..MAX_LAYERS {
            recipe
                .layer_stack()
                .layers
                .push(Layer::new(format!("Layer {i}"), TOOLS[1].operator()));
        }
        let before = recipe.clone();
        let mut controls = layers::Controls::default();
        for _ in 0..3 {
            let shapes = frame(&ctx, vec![], |ui| {
                assert_eq!(controls.show(ui, lang, &mut recipe), (false, false));
            });
            let opacity = shapes
                .texts
                .iter()
                .find(|(s, _)| s == lang.text("Opacità"))
                .unwrap()
                .1;
            let effects = shapes
                .texts
                .iter()
                .find(|(s, _)| s == lang.text("Effetti del livello"))
                .unwrap()
                .1;
            assert!(
                opacity.bottom() < 340. && effects.bottom() < 420.,
                "{lang:?}: {opacity:?}, {effects:?}"
            );
        }
        assert_eq!(recipe, before);
    }
}

#[test]
fn historical_noise_updates_only_after_explicit_action_and_commits_once() {
    use tr_core::editing::detail::Denoise;
    for (lang, version) in [
        (Language::Italian, Denoise::Legacy),
        (Language::English, Denoise::Legacy),
        (Language::Italian, Denoise::SpectralV1),
        (Language::English, Denoise::SpectralV1),
    ] {
        let ctx = egui::Context::default();
        ctx.memory_mut(|m| m.set_everything_is_visible(true));
        let mut controls = advanced::Controls::default();
        let mut recipe = crate::verify_advanced::recipe(Default::default());
        recipe.advanced.as_mut().unwrap().detail.denoise = version;
        let original = recipe.clone();
        let mut shapes = Shapes::default();
        for _ in 0..2 {
            shapes = frame(&ctx, vec![], |ui| {
                assert_eq!(
                    controls.show(ui, lang, &mut recipe, Some([6000, 4000])),
                    (false, false)
                );
            });
        }
        assert_eq!(recipe, original);
        let position = shapes
            .texts
            .iter()
            .find(|(t, _)| t == lang.text("Aggiorna riduzione rumore"))
            .unwrap()
            .1
            .center();
        let mut commits = 0;
        for pressed in [true, false] {
            frame(&ctx, click(position, pressed), |ui| {
                commits += usize::from(controls.show(ui, lang, &mut recipe, Some([6000, 4000])).1);
            });
        }
        assert_eq!(commits, 1);
        let mut expected = original;
        expected.advanced.as_mut().unwrap().detail.denoise = Denoise::MultiscaleV2;
        assert_eq!(recipe, expected);
        recipe.validate().unwrap();
    }
}
