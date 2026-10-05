use super::*;

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
        egui::Shape::Circle(c) if c.radius < 15. => out.knobs.push(c.center),
        egui::Shape::Rect(r) if r.rect.width() > 100. && r.rect.height() < 15. => {
            out.knobs.push(r.rect.center())
        }
        egui::Shape::Vec(v) => {
            for s in v {
                collect(s, out);
            }
        }
        _ => {}
    }
}
fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    mut draw: impl FnMut(&mut egui::Ui),
) -> Shapes {
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
        let r = adjustment(ui, "Local exposure", &mut value, -5. ..=5., "", None);
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
        }
        let add = shapes
            .texts
            .iter()
            .find(|(t, _)| t == lang.text("Aggiungi maschera"))
            .unwrap()
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
    let start = shapes.knobs[0];
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
