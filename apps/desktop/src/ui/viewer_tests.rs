use super::settings_regressions::{app, settle};
use super::*;

fn cached(item: &Item, base: u32, source_size: [u32; 2]) -> CachedImage {
    let [width, height] = source_size.map(|n| n.div_ceil(1 << base));
    let image = Arc::new(
        ImageLevels::from_reference_mip(
            tr_core::color::LinearImage::new(
                width,
                height,
                vec![[0.2, 0.3, 0.4, 1.]; (width * height) as usize],
            )
            .unwrap(),
            source_size,
            base,
            true,
        )
        .unwrap(),
    );
    CachedImage {
        digest: item.digest.clone(),
        info: RasterInfo {
            shooting: None,
            scientific: None,
            reference_mip: None,
            width: source_size[0],
            height: source_size[1],
            source_width: source_size[0],
            source_height: source_size[1],
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        },
        histogram: image.source().histogram(),
        pyramid: image,
        touched: 0,
        transport: "test",
        worker_pid: None,
    }
}

fn draw(app: &mut TrueRenderer, ctx: &egui::Context, item: &Item, view: &str) -> (f32, bool) {
    app.frame_number += 1;
    app.demand.clear();
    app.primary_demand.clear();
    app.demand_jobs.clear();
    app.presenter.poll(ctx);
    app.presenter.begin_capture();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320., 220.),
            )),
            ..Default::default()
        },
        |ui| app.paint_view(ui, item, view),
    );
    output.textures_delta.clear();
    assert!(!app.presenter.has_errors());
    let coverage = app.presenter.coverage();
    assert_eq!(coverage.len(), 1, "The viewer must draw the selected image");
    (coverage[0].fraction, coverage[0].exact)
}

fn render(app: &mut TrueRenderer, ctx: &egui::Context, item: &Item, continuous: bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let (coverage, exact) = draw(app, ctx, item, "single");
        if continuous {
            assert!(
                coverage > 0.,
                "Refinement discarded the previous image frame"
            );
        }
        if exact {
            return;
        }
        assert!(Instant::now() < deadline, "Viewer did not finish rendering");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn viewer_keeps_image_visible_when_zoom_receives_native_detail() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    let item = app.state.items[0].clone();
    app.cache_settings.quality = PreviewQuality::Full;
    app.service.cache.configure(app.cache_settings.clone());
    app.state.view = ViewMode::Preview;
    let reduced = cached(&item, 1, [512, 256]);
    app.cache.insert(app.image_key(&item, 128), reduced);
    render(&mut app, &ctx, &item, false);
    app.state.transform.set_zoom(3.);
    render(&mut app, &ctx, &item, true);

    // The decoder/cache returns a new provider while the previous CPU mip is
    // evicted. Its displayed texture must cover the upload of native detail.
    app.cache.clear();
    app.cache
        .insert(app.image_key(&item, 0), cached(&item, 0, [512, 256]));
    render(&mut app, &ctx, &item, true);

    // Reopening and a quality change also replace providers of this revision.
    for quality in [PreviewQuality::Standard, PreviewQuality::Full] {
        app.quality_overrides.insert(item.id.clone(), quality);
        app.cache.clear();
        app.cache
            .insert(app.image_key(&item, 0), cached(&item, 0, [512, 256]));
        render(&mut app, &ctx, &item, true);
    }
}

#[test]
fn viewer_never_reprojects_another_asset_revision_recipe_or_coordinate_space() {
    for change in ["asset", "digest", "engine", "wb", "coordinates", "view"] {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let mut item = app.state.items[0].clone();
        app.state.transform.set_zoom(3.);
        app.cache
            .insert(app.image_key(&item, 0), cached(&item, 0, [512, 256]));
        render(&mut app, &ctx, &item, false);
        app.cache.clear();
        let mut replacement = cached(&item, 0, [512, 256]);
        let mut view = "single";
        match change {
            "asset" => item = app.state.items[1].clone(),
            "digest" => replacement.digest = "different source bytes".into(),
            "engine" => {
                app.cache_settings.raw_engine = tr_core::decoder::RawEngine::choices()
                    .find(|e| *e != app.cache_settings.raw_engine)
                    .unwrap();
                app.service.cache.configure(app.cache_settings.clone());
            }
            "wb" => {
                let mut recipe =
                    tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
                recipe.raw_wb.red = 1500;
                app.editing
                    .entries
                    .entry(item.id.clone())
                    .or_default()
                    .draft = Some(recipe);
            }
            "coordinates" => replacement = cached(&item, 0, [1024, 512]),
            "view" => view = "comparison-B",
            _ => unreachable!(),
        }
        app.cache.insert(app.image_key(&item, 0), replacement);
        assert_eq!(draw(&mut app, &ctx, &item, view), (0., false), "{change}");
    }
}

#[test]
fn viewer_replaces_rgb_edits_continuously_but_keeps_output_proofs_separate() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    let item = app.state.items[0].clone();
    app.state.transform.set_zoom(3.);
    let source = cached(&item, 0, [512, 256]);
    app.cache.insert(app.image_key(&item, 0), source.clone());
    render(&mut app, &ctx, &item, false);
    let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.exposure_ev = 1.;
    app.editing.entries.insert(
        item.id.clone(),
        editing::EditEntry {
            loaded: Some(tr_store::LoadedEdit {
                asset_id: item.id.clone(),
                source_digest: item.digest.clone(),
                generation: 1,
                revision: 1,
                recipe: recipe.clone(),
                can_undo: true,
                can_redo: false,
            }),
            draft: Some(recipe),
            ..Default::default()
        },
    );
    for proof in [false, true] {
        app.request_final_preview(&item.id, proof);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            app.poll_edit_preview();
            if app
                .edited_preview(&item.id, &item.digest, source.pyramid.clone())
                .is_some()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        // RGB edits may retain the complete previous display while uploading;
        // it remains inexact. A different output mode has no compatible frame.
        assert_eq!(
            draw(&mut app, &ctx, &item, "single"),
            (if proof { 0. } else { 1. }, false)
        );
        render(&mut app, &ctx, &item, false);
    }
}
