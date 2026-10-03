use super::*;
use crate::ui::settings_regressions::{app, settle};

fn draw(app: &mut TrueRenderer, ctx: &egui::Context, item: &Item) -> (f32, bool) {
    app.frame_number += 1;
    app.presenter.begin_capture();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640., 480.),
            )),
            ..Default::default()
        },
        |ui| app.paint_view(ui, item, "single"),
    );
    output.textures_delta.clear();
    let coverage = app.presenter.coverage();
    assert_eq!(coverage.len(), 1, "The edited viewer stopped drawing");
    (coverage[0].fraction, coverage[0].exact)
}

fn converge(app: &mut TrueRenderer, ctx: &egui::Context, item: &Item, continuous: bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_edit_preview();
        app.presenter.poll(ctx);
        let (coverage, exact) = draw(app, ctx, item);
        if continuous {
            assert!(
                coverage > 0.999,
                "Editing left a hole in the photo: {coverage}"
            );
        }
        if exact && app.editing.previews.contains_key(&item.id) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "The final edit never reached the viewer"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn draw_thumbnail(
    app: &mut TrueRenderer,
    ctx: &egui::Context,
    item: &Item,
    grid: bool,
) -> (f32, bool, Option<u64>) {
    app.presenter.begin_capture();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        app.thumbnail(ui, item, Vec2::new(256., 220.), grid);
    });
    output.textures_delta.clear();
    let coverage = app.presenter.coverage();
    assert_eq!(coverage.len(), 1);
    (
        coverage[0].fraction,
        coverage[0].exact,
        coverage[0].displayed_source,
    )
}

#[test]
fn editing_keeps_the_last_frame_through_rapid_drafts_and_presentation() {
    for (proof, zoom) in [
        (false, None),
        (false, Some(1.)),
        (true, None),
        (true, Some(1.)),
    ] {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        // All inputs are synthetic; decoding is unnecessary for this temporal test.
        let request = app.preview_request(&item, 0);
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(128, 96, vec![[0.1, 0.2, 0.3, 1.]; 128 * 96])
                    .unwrap(),
                request,
            )
            .unwrap(),
        );
        let info = RasterInfo {
            shooting: None,
            scientific: None,
            reference_mip: None,
            width: 128,
            height: 96,
            source_width: 128,
            source_height: 96,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        };
        app.cache.insert(
            (item.id.clone(), request),
            CachedImage {
                digest: item.digest.clone(),
                histogram: source.source().histogram(),
                pyramid: source.clone(),
                info,
                touched: 0,
                transport: "test",
                worker_pid: None,
            },
        );
        let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
        app.editing.entries.insert(
            item.id.clone(),
            EditEntry {
                loaded: Some(LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: item.digest.clone(),
                    generation: 1,
                    revision: 1,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe.clone()),
                ..Default::default()
            },
        );
        app.state.view = ViewMode::Preview;
        app.state.transform.zoom = zoom;
        if proof {
            app.request_final_preview(&item.id, true);
            converge(&mut app, &ctx, &item, false);
        } else {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.presenter.poll(&ctx);
                if draw(&mut app, &ctx, &item).1 {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        recipe.exposure_ev = 0.25;
        app.apply_edit_draft(&item.id, recipe.clone());
        converge(&mut app, &ctx, &item, true);
        let cached = app.cache[&(item.id.clone(), request)].clone();
        app.cache.insert(app.image_key(&item, 256), cached);
        for grid in [false, true] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.poll_edit_preview();
                app.presenter.poll(&ctx);
                if draw_thumbnail(&mut app, &ctx, &item, grid).1 {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let previous_thumbnail = app.editing.thumbnails[&source.id()].image.id();
        for step in 1..=20 {
            recipe.exposure_ev = step as f32 / 10.;
            recipe.brightness = step as f32;
            app.apply_edit_draft(&item.id, recipe.clone());
            // Intentionally withhold completions: no frame may revert to the
            // original or disappear while a newer draft is being computed.
            let (coverage, exact) = draw(&mut app, &ctx, &item);
            assert!(
                coverage > 0.999,
                "Draft {step} discarded the displayed edit"
            );
            assert!(!exact, "An old frame was reported as the current edit");
            for grid in [false, true] {
                assert_eq!(
                    draw_thumbnail(&mut app, &ctx, &item, grid),
                    (1., false, Some(previous_thumbnail)),
                    "Thumbnail flashed on draft {step}"
                );
            }
        }
        converge(&mut app, &ctx, &item, true);
        let final_image = app.editing.previews[&item.id].image.clone();
        // A late CPU result from an earlier gesture must not roll back the view.
        let obsolete = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .recipe
            .clone();
        app.editing
            .tx
            .send((
                app.generation,
                item.id.clone(),
                source.id(),
                0,
                proof,
                obsolete,
                Ok(source.clone()),
            ))
            .unwrap();
        app.poll_edit_preview();
        assert_eq!(app.editing.previews[&item.id].image.id(), final_image.id());
        let deadline = Instant::now() + Duration::from_secs(5);
        let thumbnail = loop {
            app.poll_edit_preview();
            if let Some(image) = app.edited_thumbnail(&item.id, &item.digest, source.clone()) {
                break image;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(app.editing.previews[&item.id].recipe, recipe);
        assert_eq!(app.editing.previews[&item.id].proof, proof);
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .generation,
            1
        );
        let mut saved = app.editing.entries[&item.id].loaded.clone().unwrap();
        saved.recipe = recipe;
        saved.generation += 1;
        saved.revision += 1;
        app.edit_result(item.id.clone(), Ok(saved));
        assert_eq!(app.editing.previews[&item.id].image.id(), final_image.id());
        assert_eq!(
            app.edited_thumbnail(&item.id, &item.digest, source.clone())
                .unwrap()
                .id(),
            thumbnail.id()
        );
        assert_eq!(draw(&mut app, &ctx, &item), (1., true));
        // Before has its own display identity, then After recovers its frame.
        app.editing.show_original = true;
        assert_eq!(draw(&mut app, &ctx, &item), (0., false));
        app.editing.show_original = false;
        assert_eq!(draw(&mut app, &ctx, &item), (1., true));

        // A failed edit must retain the last displayed image, expose its error,
        // and wait for a changed request instead of retrying each frame.
        let mut failed = app.editing.entries[&item.id].draft.clone().unwrap();
        failed.exposure_ev += 0.125;
        app.apply_edit_draft(&item.id, failed.clone());
        app.editing
            .tx
            .send((
                app.generation,
                item.id.clone(),
                source.id(),
                0,
                proof,
                failed.clone(),
                Err("synthetic edit failure".into()),
            ))
            .unwrap();
        app.editing
            .thumbnail_tx
            .send((
                app.generation,
                item.id.clone(),
                item.digest.clone(),
                source.id(),
                failed.clone(),
                Err("synthetic thumbnail failure".into()),
            ))
            .unwrap();
        app.poll_edit_preview();
        for _ in 0..3 {
            assert_eq!(draw(&mut app, &ctx, &item), (1., false));
            for grid in [false, true] {
                let (coverage, exact, displayed) = draw_thumbnail(&mut app, &ctx, &item, grid);
                assert_eq!((coverage, exact), (1., false));
                assert_ne!(displayed, Some(source.id()));
            }
            assert!(app.editing.inflight.is_none() && app.editing.thumbnail_inflight.is_empty());
        }
        assert_eq!(
            app.edited_preview_error(&item.id),
            Some("synthetic edit failure")
        );
        assert_eq!(
            app.edited_thumbnail_error(source.id()),
            Some("synthetic thumbnail failure")
        );
        failed.exposure_ev += 0.125;
        app.apply_edit_draft(&item.id, failed);
        converge(&mut app, &ctx, &item, true);
        assert!(app.edited_preview_error(&item.id).is_none());

        // Reset is an intentional return to the original. It must replace the
        // edited frame continuously and converge in both working/proof modes.
        let neutral = EditRecipe::neutral(app.cache_settings.raw_engine);
        app.apply_edit_draft(&item.id, neutral.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll_edit_preview();
            app.presenter.poll(&ctx);
            let (coverage, exact) = draw(&mut app, &ctx, &item);
            assert_eq!(coverage, 1.);
            if exact {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        if proof {
            assert_eq!(app.editing.previews[&item.id].recipe, neutral);
        } else {
            assert_eq!(app.presenter.captures()[0].source, source.id());
        }
    }
}
