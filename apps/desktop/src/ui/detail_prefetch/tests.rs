use super::*;
use crate::ui::settings_regressions::{app, settle};

pub(in crate::ui) fn fixture() -> (
    tempfile::TempDir,
    egui::Context,
    TrueRenderer,
    Item,
    CachedImage,
) {
    let (dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    app.cancel_folder_preparation();
    app.service.cache = Arc::new(crate::cache::Manager::new(app.cache_settings.clone()));
    app.service.preview_writer = Arc::new(crate::cache::writer::Writer::start(
        app.service.cache.clone(),
        app.service.generation.clone(),
    ));
    app.cache.clear();
    app.state.view = ViewMode::Preview;
    app.state.transform.zoom = None;
    app.frame_number = 100;
    let item = app.state.items[0].clone();
    app.state.current = Some(item.id.clone());
    app.quality_overrides
        .insert(item.id.clone(), PreviewQuality::Full);
    app.edit_result(
        item.id.clone(),
        Ok(tr_store::LoadedEdit {
            asset_id: item.id.clone(),
            source_digest: item.digest.clone(),
            generation: 0,
            revision: 0,
            recipe: tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine),
            can_undo: false,
            can_redo: false,
        }),
    );
    let raster = tr_core::color::LinearImage::new(
        129,
        65,
        (0..129 * 65)
            .map(|i| {
                [
                    i as f32 / 10000.,
                    0.2,
                    0.8,
                    if i % 13 == 0 { 0.5 } else { 1. },
                ]
            })
            .collect(),
    )
    .unwrap();
    let mut levels = ImageLevels::from_source(raster, app.preview_request(&item, 0)).unwrap();
    levels.attach_lease(
        app.service
            .cache
            .memory
            .try_reserve(levels.byte_len() as u64)
            .unwrap(),
    );
    let pyramid = Arc::new(levels);
    let native = CachedImage {
        digest: item.digest.clone(),
        histogram: pyramid.source().histogram(),
        pyramid,
        touched: 0,
        transport: "test",
        worker_pid: None,
        info: RasterInfo {
            shooting: None,
            scientific: None,
            reference_mip: None,
            width: 129,
            height: 65,
            source_width: 129,
            source_height: 65,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        },
    };
    let request = app.preview_request(&item, 16);
    let mut fit = native.clone();
    let bytes = native.pyramid.levels()[native.pyramid.requested_base(request)..]
        .iter()
        .map(|l| l.pixels.len() as u64 * 16)
        .sum();
    fit.pyramid = Arc::new(
        native
            .pyramid
            .detach(
                request,
                app.service.cache.memory.try_reserve(bytes).unwrap(),
            )
            .unwrap(),
    );
    fit.touched = app.frame_number;
    app.cache.insert((item.id.clone(), request), fit);
    view(&mut app, &item);
    (dir, ctx, app, item, native)
}

fn view(app: &mut TrueRenderer, item: &Item) {
    app.demand.clear();
    app.demand_jobs.clear();
    app.primary_demand.clear();
    app.detail_prefetch.begin_frame();
    let request = app.preview_request(item, 16);
    app.demand.insert((item.id.clone(), request));
    app.primary_demand.insert(item.id.clone());
    app.detail_prefetch.observe(item, request);
}

fn dwell(app: &mut TrueRenderer) {
    for target in app.detail_prefetch.targets.values_mut() {
        target.since = Instant::now() - Duration::from_secs(1);
    }
}

#[test]
fn fitted_view_prepares_detail_after_dwell_without_changing_quality_or_visible_pixels() {
    for quality in [PreviewQuality::Full, PreviewQuality::Standard] {
        let (_dir, ctx, mut app, item, _) = fixture();
        let fit = app.cache.values().next().unwrap().clone();
        app.quality_overrides.insert(item.id.clone(), quality);
        view(&mut app, &item);
        app.cache.insert(app.image_key(&item, 16), fit.clone());
        app.active_detail_demand(&ctx);
        assert!(app.demand_jobs.is_empty());
        dwell(&mut app);
        app.active_detail_demand(&ctx);
        let job = &app.demand_jobs[0];
        assert_eq!(job.request, app.preview_request(&item, 0));
        assert_eq!(job.request.quality, quality);
        assert_eq!(job.priority, PreviewPriority::AheadRegion);
        assert_eq!(app.state.transform.zoom, None);
        assert_eq!(
            app.cache[&app.image_key(&item, 16)].pyramid.id(),
            fit.pyramid.id()
        );
        assert_eq!(app.demand_jobs.len(), 1);
    }
}

#[test]
fn visible_dependencies_win_and_admitted_detail_survives_its_own_memory_usage() {
    let (_dir, ctx, mut app, item, _) = fixture();
    app.active_detail_demand(&ctx);
    dwell(&mut app);
    let missing = app.image_key(&app.state.items[1], 128);
    app.demand.insert(missing.clone());
    app.active_detail_demand(&ctx);
    assert!(app.demand_jobs.is_empty());
    app.demand.remove(&missing);
    app.active_detail_demand(&ctx);
    let key = app.image_key(&item, 0);
    assert!(app.demand.contains(&key));
    app.pending_images.insert(key.clone());
    app.promoted
        .insert(key.clone(), PreviewPriority::AheadRegion);
    view(&mut app, &item);
    let reserved = app.service.cache.memory.usage().reserved;
    app.service.cache.memory.configure(reserved + 1024);
    app.active_detail_demand(&ctx);
    assert!(app.demand.contains(&key));
    assert!(app.demand_jobs.is_empty());
    app.promoted
        .insert(key.clone(), PreviewPriority::Background);
    app.active_detail_demand(&ctx);
    assert_eq!(app.demand_jobs[0].priority, PreviewPriority::AheadRegion);
    app.state.transform.zoom = Some(1.);
    app.ensure_image_priority(&item, 0, PreviewPriority::Refinement);
    assert_eq!(app.demand_jobs[0].priority, PreviewPriority::Refinement);
}

#[test]
fn prepared_detail_is_optional_and_eviction_does_not_restart_it() {
    let (_dir, ctx, mut app, item, native) = fixture();
    let key = app.image_key(&item, 0);
    let native_id = native.pyramid.id();
    let bytes = native.pyramid.byte_len() as u64;
    app.cache.insert(key.clone(), native);
    app.active_detail_demand(&ctx);
    assert_eq!(app.cache[&key].touched, 0);
    assert!(!app.demand.contains(&key));
    let before = app.service.cache.memory.usage().reserved;
    app.trim_images(true);
    assert!(app.cache.values().all(|c| c.pyramid.id() != native_id));
    assert_eq!(app.service.cache.memory.usage().reserved, before - bytes);
    dwell(&mut app);
    app.active_detail_demand(&ctx);
    assert!(app.demand_jobs.is_empty());
    assert!(app.cache.contains_key(&app.image_key(&item, 16)));
}

#[test]
fn optional_native_aliases_share_retention_priority_but_still_yield_to_reclamation() {
    let (_dir, ctx, mut app, item, native) = fixture();
    let native_id = native.pyramid.id();
    let bytes = native.pyramid.byte_len() as u64;
    app.cache.insert(app.image_key(&item, 0), native.clone());
    app.cache.insert(app.image_key(&item, 64), native.clone());
    let mut other = native;
    let mut levels =
        ImageLevels::from_source(other.pyramid.source().clone(), PreviewRequest::full()).unwrap();
    levels.attach_lease(
        app.service
            .cache
            .memory
            .try_reserve(levels.byte_len() as u64)
            .unwrap(),
    );
    other.pyramid = Arc::new(levels);
    let other_id = other.pyramid.id();
    let neighbor = app.state.items[1].clone();
    app.cache.insert(app.image_key(&neighbor, 16), other);
    app.viewer_prefetch_edge = 16;
    app.active_detail_demand(&ctx);
    let usage = app.service.cache.memory.usage();
    app.trim_images_for_headroom(usage.limit - usage.reserved + bytes);
    assert!(app.cache.values().any(|c| c.pyramid.id() == native_id));
    assert!(app.cache.values().all(|c| c.pyramid.id() != other_id));
    app.trim_images(true);
    assert!(app.cache.values().all(|c| c.pyramid.id() != native_id));
}

#[test]
fn navigation_recipe_source_and_pressure_revoke_speculation() {
    for change in [
        "navigation",
        "quality",
        "source",
        "pressure",
        "pause",
        "grid",
    ] {
        let (_dir, ctx, mut app, item, _) = fixture();
        app.active_detail_demand(&ctx);
        dwell(&mut app);
        app.active_detail_demand(&ctx);
        let old = app.image_key(&item, 0);
        app.pending_images.insert(old.clone());
        let mut next = item.clone();
        match change {
            "navigation" => next = app.state.items[1].clone(),
            "quality" => {
                app.quality_overrides
                    .insert(item.id.clone(), PreviewQuality::Standard);
            }
            "source" => {
                app.generation += 1;
                app.pending_images.clear();
                next.observation.push('x');
            }
            "pressure" => {
                app.service
                    .cache
                    .set_pressure(Some(tr_platform::MemoryPressure::Critical));
            }
            "pause" => app.preparation_paused = true,
            "grid" => app.state.view = ViewMode::Grid,
            _ => unreachable!(),
        }
        view(&mut app, &next);
        app.active_detail_demand(&ctx);
        assert!(!app.demand.contains(&old), "{change}");
        assert!(app.demand_jobs.is_empty(), "{change}");
    }
}

#[test]
fn compare_preserves_each_lanes_raw_white_balance() {
    let (_dir, ctx, mut app, item, _) = fixture();
    let original = app.preview_request(&item, 16);
    let mut adjusted = original;
    adjusted.raw_wb.red = 1500;
    app.detail_prefetch.observe(&item, adjusted);
    app.active_detail_demand(&ctx);
    dwell(&mut app);
    app.active_detail_demand(&ctx);
    assert_eq!(app.demand_jobs.len(), 2);
    for request in [original, adjusted] {
        assert!(
            app.demand_jobs
                .iter()
                .any(|j| j.request == PreviewRequest { edge: 0, ..request })
        );
    }
}
