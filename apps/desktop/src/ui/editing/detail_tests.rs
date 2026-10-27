use super::*;
use crate::ui::detail_prefetch::tests::fixture;

fn finish(app: &mut TrueRenderer) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.editing.inflight.is_some() {
        app.poll_edit_preview();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn background_headroom_keeps_visible_edits_and_releases_them_after_navigation() {
    let (_dir, _ctx, mut app, item, _native) = fixture();
    // Isolate renderer ownership: an asynchronous disk writer legitimately
    // retains its own lease until persistence finishes.
    let mut settings = app.service.cache.settings();
    settings.enabled = false;
    app.service.cache.configure(settings);
    let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.exposure_ev = 0.75;
    app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe);
    let fit = app.cache[&app.image_key(&item, 16)].pyramid.clone();
    app.edited_preview(&item.id, &item.digest, fit.clone());
    finish(&mut app);
    let visible = Arc::downgrade(&app.editing.previews[&item.id].image);
    let renders = app.editing.work_stats.renders.load(Ordering::Relaxed);
    // Even an impossible background request must not discard the visible
    // result and make the next frame compute it again indefinitely.
    app.trim_images_for_headroom(u64::MAX);
    assert!(app.editing.previews.contains_key(&item.id));
    let reused = app.edited_preview(&item.id, &item.digest, fit).unwrap();
    assert!(Arc::ptr_eq(&reused, &visible.upgrade().unwrap()));
    assert!(app.editing.inflight.is_none());
    assert_eq!(
        app.editing.work_stats.renders.load(Ordering::Relaxed),
        renders
    );
    drop(reused);
    // Once no current frame needs the source, reclaim its edit in the same
    // pass so its memory is available to the waiting admission.
    app.frame_number += 3;
    app.trim_images_for_headroom(u64::MAX);
    assert!(!app.editing.previews.contains_key(&item.id));
    assert!(visible.upgrade().is_none());
}

#[test]
fn folder_edit_is_revoked_by_tool_and_layer_changes_and_only_the_latest_recipe_is_used() {
    for layered in [false, true] {
        let (_dir, ctx, mut app, item, native) = fixture();
        let mut settings = app.service.cache.settings();
        settings.enabled = false;
        app.service.cache.configure(settings);
        let mut recipe = if layered {
            crate::verify_advanced::layered_recipe(app.cache_settings.raw_engine)
        } else {
            crate::verify_advanced::recipe(app.cache_settings.raw_engine)
        };
        app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe.clone());
        assert!(!app.prepare_folder_edit(&item, &native).unwrap());
        assert!(app.editing.folder_preparing);
        recipe.exposure_ev = -0.5;
        if let Some(stack) = &mut recipe.layers {
            stack.layers[0].opacity = 0.4;
        }
        app.apply_edit_draft(&item.id, recipe.clone());
        app.background_demand(&ctx);
        finish(&mut app);
        assert!(app.editing.batch_prepared.is_empty());
        assert!(app.editing.folder_completed.is_none());
        assert!(app.editing.preview_errors.is_empty());
        app.editing.changed_at.clear();
        assert!(!app.prepare_folder_edit(&item, &native).unwrap());
        finish(&mut app);
        let ready = &app.editing.batch_prepared[&(item.id.clone(), native.pyramid.id())];
        assert_eq!(ready.recipe, recipe);
        let mut expected = native.pyramid.source().clone();
        recipe
            .apply_preview(&mut expected, native.pyramid.source_size(), 0)
            .unwrap();
        assert_eq!(ready.image.source().pixels, expected.pixels);
    }
}

#[test]
fn memory_pressure_pauses_folder_edits_without_fabricating_completion() {
    let (_dir, ctx, mut app, item, native) = fixture();
    let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.exposure_ev = 0.5;
    app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe);
    app.cache.insert(app.image_key(&item, 0), native);
    app.start_folder_preparation(true);
    app.service
        .cache
        .set_pressure(Some(tr_platform::MemoryPressure::Warning));
    app.folder_preparation_demand(&ctx);
    assert!(app.editing.inflight.is_none());
    assert!(app.editing.batch_prepared.is_empty());
    assert_eq!(app.folder_progress().0, 0);
    app.service.cache.set_pressure(None);
    app.folder_preparation_demand(&ctx);
    assert!(app.editing.folder_preparing);
    finish(&mut app);
    assert!(!app.editing.batch_prepared.is_empty());
}

#[test]
fn interactive_viewer_and_thumbnail_results_do_not_write_the_disk_cache() {
    let (_dir, _ctx, mut app, item, native) = fixture();
    app.cache.insert(app.image_key(&item, 0), native.clone());
    let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.exposure_ev = 0.5;
    app.apply_edit_draft(&item.id, recipe);
    app.editing
        .changed_at
        .insert(item.id.clone(), Instant::now() + Duration::from_secs(10));
    app.edited_preview(&item.id, &item.digest, native.pyramid.clone());
    app.edited_thumbnail(&item.id, &item.digest, native.pyramid);
    finish(&mut app);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.editing.thumbnail_inflight.is_empty() {
        app.poll_edit_preview();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.service.cache.stats().writes, 0);
    assert!(!app.service.cache.preview_writes_pending());
}

#[test]
fn prepared_edit_keeps_fit_visible_then_promotes_without_new_work_and_matches_native_reference() {
    let (_dir, _ctx, mut app, item, native) = fixture();
    let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.exposure_ev = 0.75;
    recipe.temperature = 12.;
    recipe.curve = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 0.4, y: 0.47 },
        CurvePoint { x: 1., y: 1. },
    ];
    app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe.clone());
    let fit = app.cache[&app.image_key(&item, 16)].pyramid.clone();
    app.edited_preview(&item.id, &item.digest, fit.clone());
    finish(&mut app);
    let visible = app.editing.previews[&item.id].image.clone();
    app.prefetch_edit_detail(&item.id, &item.digest, native.pyramid.clone());
    assert!(app.editing.preparing);
    assert!(Arc::ptr_eq(
        &app.edited_preview(&item.id, &item.digest, fit).unwrap(),
        &visible
    ));
    finish(&mut app);
    assert!(Arc::ptr_eq(&app.editing.previews[&item.id].image, &visible));
    let prepared = app.editing.prepared[&item.id].image.clone();
    assert_eq!(prepared.base_level(), 0);
    let mut expected = native.pyramid.source().clone();
    recipe
        .apply_preview(&mut expected, native.pyramid.source_size(), 0)
        .unwrap();
    assert_eq!(prepared.source().pixels, expected.pixels);
    let full = app
        .edited_preview(&item.id, &item.digest, native.pyramid)
        .unwrap();
    assert!(Arc::ptr_eq(&prepared, &full));
    assert!(app.editing.inflight.is_none());
    assert!(app.editing.prepared.is_empty());
}

#[test]
fn obsolete_preparation_cannot_replace_a_new_recipe_or_survive_navigation() {
    for change_recipe in [false, true] {
        let (_dir, _ctx, mut app, item, native) = fixture();
        let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.exposure_ev = 1.;
        app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe.clone());
        let reserved = app.service.cache.memory.usage().reserved;
        app.prefetch_edit_detail(&item.id, &item.digest, native.pyramid.clone());
        assert!(app.editing.preparing);
        if change_recipe {
            recipe.exposure_ev = -1.;
            app.apply_edit_draft(&item.id, recipe);
        } else {
            app.retain_prepared_edits(&HashSet::new());
        }
        finish(&mut app);
        assert!(app.editing.prepared.is_empty());
        assert!(app.editing.previews.is_empty());
        assert!(app.editing.preview_errors.is_empty());
        assert_eq!(app.service.cache.memory.usage().reserved, reserved);
    }
}

#[test]
fn optional_edit_yields_to_low_memory_and_releases_credits_under_pressure() {
    let (_dir, _ctx, mut app, item, native) = fixture();
    let mut recipe = EditRecipe::neutral(app.cache_settings.raw_engine);
    recipe.exposure_ev = 0.5;
    app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe);
    let before = app.service.cache.memory.usage();
    app.service
        .cache
        .memory
        .configure(before.reserved + native.pyramid.byte_len() as u64);
    app.prefetch_edit_detail(&item.id, &item.digest, native.pyramid.clone());
    assert!(app.editing.inflight.is_none());
    assert!(app.editing.preview_errors.is_empty());
    assert_eq!(app.service.cache.memory.usage().rejected, before.rejected);
    app.service.cache.memory.configure(before.limit);
    app.prefetch_edit_detail(&item.id, &item.digest, native.pyramid);
    finish(&mut app);
    assert_eq!(app.editing.prepared.len(), 1);
    app.prune_edit_previews(true);
    assert!(app.editing.prepared.is_empty());
}
