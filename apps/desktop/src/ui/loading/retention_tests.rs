use super::*;
use crate::ui::settings_regressions::{app, settle};

fn neutral(app: &mut TrueRenderer, item: &Item) {
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
}

fn cached(app: &TrueRenderer, width: u32) -> CachedImage {
    let mut image = ImageLevels::from_source(
        tr_core::color::LinearImage::new(
            width,
            width / 2,
            vec![[0.2, 0.3, 0.4, 1.]; (width * width / 2) as usize],
        )
        .unwrap(),
        PreviewRequest::full(),
    )
    .unwrap();
    image.attach_lease(
        app.service
            .cache
            .memory
            .try_reserve(image.byte_len() as u64)
            .unwrap(),
    );
    let pyramid = Arc::new(image);
    CachedImage {
        digest: "a".repeat(64),
        info: RasterInfo {
            shooting: None,
            scientific: None,
            reference_mip: None,
            width,
            height: width / 2,
            source_width: width,
            source_height: width / 2,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "linear Rec2020".into(),
            filter: "reference".into(),
            orientation: "applied".into(),
        },
        histogram: pyramid.source().histogram(),
        pyramid,
        touched: 0,
        transport: "test",
        worker_pid: None,
    }
}

#[test]
fn folder_detaches_viewers_before_completion_without_reopening_sources() {
    for enabled in [false, true] {
        for foreground in [false, true] {
            for quality in [PreviewQuality::Full, PreviewQuality::Standard] {
                let (dir, ctx, mut app) = app();
                settle(&mut app, &ctx, true);
                app.cache_settings.enabled = enabled;
                // Scaled quota: a native 512x256 pyramid cannot be retained.
                app.cache_settings.disk_mib = 1;
                app.cache_settings.free_mib = 0;
                app.cache_settings.quality = quality;
                app.service.cache.configure(app.cache_settings.clone());
                app.start_folder_preparation(true);
                app.folder_load.foreground = foreground;
                app.folder_load.viewer_edge = 64;
                app.frame_number = 10;
                let items = app.state.items.clone();
                let mut parents = HashSet::new();
                let mut expected = HashMap::new();
                for item in &items {
                    neutral(&mut app, item);
                    let native = cached(&app, 512);
                    let request = app.preview_request_for_mode(item, 64, false);
                    let base = native.pyramid.requested_base(request);
                    expected.insert(item.id.clone(), native.pyramid.levels()[base..].to_vec());
                    parents.insert(native.pyramid.id());
                    app.cache.insert(
                        (
                            item.id.clone(),
                            app.preview_request_for_mode(item, 0, false),
                        ),
                        native,
                    );
                }
                app.service.cache.preview_writes.store(1, Ordering::Release);
                app.folder_preparation_demand(&ctx);
                let front = app.preview_request_for_mode(&items[0], 0, false);
                assert_eq!(
                    app.cache[&(items[0].id.clone(), front)].touched,
                    app.frame_number
                );
                app.service.cache.preview_writes.store(0, Ordering::Release);
                app.demand.clear();
                app.folder_preparation_demand(&ctx);
                assert_eq!(
                    app.folder_progress().0,
                    0,
                    "Native readiness alone must not complete a photo"
                );
                assert!(
                    app.demand_jobs[0].resident.is_some(),
                    "Derive on I/O lane, without source decode"
                );
                app.preparation_paused = true;
                app.demand.clear();
                app.demand_jobs.clear();
                app.folder_preparation_demand(&ctx);
                assert!(app.demand_jobs.is_empty());
                assert_eq!(app.folder_progress().0, 0);
                app.preparation_paused = false;
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    app.frame_number += 1;
                    app.poll(&ctx);
                    app.demand.clear();
                    app.demand_jobs.clear();
                    app.folder_preparation_demand(&ctx);
                    app.flush_demand();
                    if !app.folder_load.finishing {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Preparation stalled: {:?}",
                        app.errors
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(app.errors.is_empty());
                assert_eq!(app.folder_progress(), (2, 2, 1.));
                assert!(!app.folder_loading_blocks());
                app.cache.retain(|_, c| !parents.contains(&c.pyramid.id()));
                app.demand_jobs.clear();
                for item in &items {
                    app.ensure_image(item, 64);
                    let ready = &app.cache[&app.image_key(item, 64)];
                    for (actual, expected) in ready.pyramid.levels().iter().zip(&expected[&item.id])
                    {
                        assert_eq!(actual.pixels, expected.pixels);
                    }
                }
                assert!(
                    app.demand_jobs.is_empty(),
                    "Prepared viewers must be immediately reusable"
                );
                assert_eq!(app.service.cache.stats().decode_jobs, 0);
                if !enabled {
                    assert!(!dir.path().join("corpus").join(crate::cache::NAME).exists());
                }
            }
        }
    }
}

#[test]
fn reclamation_keeps_nearby_viewers_and_removes_all_native_aliases() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    app.service
        .cache
        .memory
        .configure(app.service.cache.memory.usage().limit);
    app.cache.clear();
    app.frame_number = 20;
    app.folder_load.viewer_edge = 64;
    let items = app.state.items.clone();
    app.state.current = Some(items[0].id.clone());
    let baseline = app.service.cache.memory.usage().reserved;
    let native_request = PreviewRequest::full();
    let viewer_request = PreviewRequest {
        edge: 64,
        ..native_request
    };
    let mut preview_bytes = 0;
    for item in &items {
        let small = cached(&app, 64);
        preview_bytes += small.pyramid.byte_len() as u64;
        app.cache.insert((item.id.clone(), viewer_request), small);
    }
    let native = cached(&app, 512);
    let native_id = native.pyramid.id();
    app.cache
        .insert((items[0].id.clone(), native_request), native.clone());
    app.cache.insert(
        (
            items[0].id.clone(),
            PreviewRequest {
                edge: 4096,
                ..native_request
            },
        ),
        native,
    );
    let limit = app.service.cache.memory.usage().limit;
    app.trim_images_for_headroom(limit - baseline - preview_bytes);
    assert!(app.cache.values().all(|c| c.pyramid.id() != native_id));
    assert_eq!(
        app.cache.len(),
        2,
        "One large ancestor must not evict both ready previews"
    );
    let needed = limit - baseline - preview_bytes / 2;
    assert!(app.service.cache.memory.try_reserve(needed).is_none());
    app.poll(&ctx);
    assert_eq!(app.cache.len(), 1);
    assert!(
        app.cache
            .contains_key(&(items[0].id.clone(), viewer_request))
    );
    assert!(app.service.cache.memory.try_reserve(needed).is_some());
    app.trim_images(true);
    assert!(
        app.cache.is_empty(),
        "OS memory pressure still releases optional retention"
    );
    assert_eq!(app.service.cache.memory.usage().reserved, baseline);
    // A paused/cancelled folder has no prepared edge. Actual viewing previews
    // must still retain the recent set ahead of speculative neighbours.
    app.folder_load.viewer_edge = 0;
    app.viewer_prefetch_edge = 64;
    app.recently_viewed.push_back(items[1].id.clone());
    for item in &items {
        let small = cached(&app, 64);
        app.cache.insert((item.id.clone(), viewer_request), small);
    }
    app.trim_images_for_headroom(limit - baseline - preview_bytes / 2);
    assert_eq!(app.cache.len(), 1);
    assert!(
        app.cache
            .contains_key(&(items[1].id.clone(), viewer_request))
    );
}

#[test]
fn equivalent_viewer_geometry_completes_without_an_extra_repaint_or_job() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    app.start_folder_preparation(true);
    app.rebuild.truncate(1);
    app.rebuild_total = 1;
    app.folder_load.viewer_edge = 64;
    let item = app.rebuild[0].clone();
    neutral(&mut app, &item);
    let native = cached(&app, 128);
    let source = native.pyramid.id();
    app.cache.insert(
        (
            item.id.clone(),
            app.preview_request_for_mode(&item, 0, false),
        ),
        native,
    );
    app.folder_preparation_demand(&ctx);
    assert_eq!(app.folder_progress(), (1, 1, 1.));
    assert!(app.demand_jobs.is_empty());
    assert_eq!(app.cache[&app.image_key(&item, 64)].pyramid.id(), source);
}

#[test]
fn recipe_change_and_cache_clear_revoke_the_refinement_phase() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    app.start_folder_preparation(true);
    app.folder_load.viewer_edge = 64;
    let item = app.rebuild[0].clone();
    neutral(&mut app, &item);
    let old = app.preview_request_for_mode(&item, 0, false);
    let native = cached(&app, 512);
    app.cache.insert((item.id.clone(), old), native);
    app.folder_preparation_demand(&ctx);
    assert!(app.folder_load.refining.is_some());
    let mut saved = app.editing.entries[&item.id].loaded.clone().unwrap();
    saved.generation = 1;
    saved.recipe.raw_engine = tr_core::decoder::RawEngine::LibRawAhd;
    saved.recipe.raw_wb.red = 1500;
    app.edit_result(item.id.clone(), Ok(saved));
    app.demand.clear();
    app.demand_jobs.clear();
    app.folder_preparation_demand(&ctx);
    assert!(app.folder_load.refining.is_none());
    assert_eq!(app.folder_progress().0, 0);
    assert!(
        app.demand_jobs
            .iter()
            .any(|j| j.request.edge == 0 && j.request.raw_wb.red == 1500)
    );
    let generation = app.generation;
    app.cancel_folder_preparation();
    assert!(app.folder_load.refining.is_none());
    app.start_cache_action(false);
    assert_eq!(app.generation, generation + 1);
    assert!(app.cache.is_empty());
    settle(&mut app, &ctx, true);
    assert_eq!(app.folder_progress().0, 0);
    assert!(app.folder_load.refining.is_none());
}

#[test]
fn neighbors_refine_during_folder_loading_despite_a_ready_filmstrip() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    app.start_folder_preparation(true);
    app.state.view = ViewMode::Preview;
    app.viewer_prefetch_edge = 1024;
    let items = app.state.items.clone();
    for item in &items {
        neutral(&mut app, item);
    }
    let current = app.image_key(&items[0], 1024);
    let thumb = app.image_key(&items[1], 128);
    let next = app.image_key(&items[1], 1024);
    let first_image = cached(&app, 128);
    let mut second_image = cached(&app, 32);
    let mut mip = ImageLevels::from_reference_mip(
        second_image.pyramid.source().clone(),
        [4096, 2048],
        7,
        true,
    )
    .unwrap();
    mip.attach_lease(
        app.service
            .cache
            .memory
            .try_reserve(mip.byte_len() as u64)
            .unwrap(),
    );
    second_image.pyramid = Arc::new(mip);
    app.cache.insert(current.clone(), first_image);
    app.cache.insert(thumb.clone(), second_image);
    app.demand = HashSet::from([current, thumb]);
    app.primary_demand.insert(items[0].id.clone());
    app.foreground_demand = app.demand.clone();
    app.navigation_changed = Instant::now() - Duration::from_secs(10);
    app.background_demand(&ctx);
    let job = app
        .demand_jobs
        .iter()
        .find(|job| (job.item.id.clone(), job.request) == next)
        .unwrap();
    assert_eq!(job.priority, PreviewPriority::NeighborPreview);
    assert!(!app.rebuild.is_empty());
    app.pending_images.insert(next.clone());
    app.promoted
        .insert(next.clone(), PreviewPriority::NeighborPreview);
    app.demand.remove(&next);
    app.demand_jobs.clear();
    let reserve = app
        .service
        .cache
        .memory
        .try_reserve(app.service.cache.memory.usage().limit / 2)
        .unwrap();
    app.background_demand(&ctx);
    assert!(
        app.demand.contains(&next),
        "The pending neighbour owns credits and must not be cancelled for admission headroom"
    );
    drop(reserve);
}

#[test]
fn automatic_neighbors_expand_without_pinning_rebuilding_or_overfilling_the_queue() {
    let (_dir, ctx, mut app) = app();
    settle(&mut app, &ctx, true);
    // This test injects pressure. The live service polls the OS on its own
    // manager and must not overwrite the synthetic value between assertions.
    app.service.cache = Arc::new(crate::cache::Manager::new(app.cache_settings.clone()));
    app.cancel_folder_preparation();
    let template = app.state.items[0].clone();
    let items: Vec<_> = (0..30)
        .map(|n| {
            let mut item = template.clone();
            item.id = format!("neighbor-{n:02}");
            item.name = format!("neighbor-{n:02}.png");
            item
        })
        .collect();
    app.state.reconcile(items.clone(), true);
    app.cache.clear();
    app.state.view = ViewMode::Preview;
    app.viewer_prefetch_edge = 64;
    app.frame_number = 100;
    for item in &items {
        neutral(&mut app, item);
    }
    let current = app.image_key(&items[0], 64);
    for item in &items[..5] {
        let key = app.image_key(item, 64);
        let ready = cached(&app, 64);
        app.cache.insert(key, ready);
    }
    app.demand = HashSet::from([current.clone()]);
    app.primary_demand = HashSet::from([items[0].id.clone()]);
    app.foreground_demand = app.demand.clone();
    app.navigation_changed = Instant::now() - Duration::from_secs(10);
    app.neighbor_demand(&ctx);
    assert_eq!(
        app.demand_jobs.len(),
        2,
        "Only two speculative admissions per frame"
    );
    assert!(
        app.demand_jobs
            .iter()
            .all(|j| !items[..5].iter().any(|i| i.id == j.item.id))
    );
    assert_eq!(
        app.cache[&app.image_key(&items[1], 64)].touched,
        0,
        "Ready speculation must not be pinned"
    );
    let evicted = app.image_key(&items[1], 64);
    app.cache.remove(&evicted);
    let pending = app.demand_jobs[0].clone();
    app.pending_images
        .insert((pending.item.id.clone(), pending.request));
    app.promoted
        .insert((pending.item.id.clone(), pending.request), pending.priority);
    app.demand_jobs.clear();
    app.demand = HashSet::from([current.clone()]);
    app.neighbor_demand(&ctx);
    assert_eq!(app.demand_jobs.len(), 1);
    assert!(app.demand.contains(&(pending.item.id, pending.request)));
    assert!(
        !app.demand.contains(&evicted),
        "Evicted speculation must not restart indefinitely"
    );
    app.service
        .cache
        .set_pressure(Some(tr_platform::MemoryPressure::Warning));
    app.demand_jobs.clear();
    app.demand = HashSet::from([current]);
    app.neighbor_demand(&ctx);
    assert!(app.demand_jobs.is_empty());
    assert_eq!(app.demand.len(), 1, "Pressure revokes speculative demand");
}

#[test]
fn neighbors_skip_unavailable_photos_without_spending_admission_slots() {
    for failure in ["decode", "recipe", "unapproved"] {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.cancel_folder_preparation();
        let template = app.state.items[0].clone();
        let items: Vec<_> = (0..6)
            .map(|n| {
                let mut item = template.clone();
                item.id = format!("neighbor-{n:02}");
                item.name = format!("neighbor-{n:02}.png");
                item.approved = failure != "unapproved" || !(1..=2).contains(&n);
                item
            })
            .collect();
        app.state.reconcile(items.clone(), true);
        app.cache.clear();
        app.state.view = ViewMode::Preview;
        app.viewer_prefetch_edge = 64;
        for item in &items {
            if failure == "recipe" && items[1..=2].iter().any(|i| i.id == item.id) {
                app.edit_result(item.id.clone(), Err("recipe unavailable".into()));
            } else {
                neutral(&mut app, item);
            }
        }
        if failure == "decode" {
            for item in &items[1..=2] {
                let key = app.image_key(item, 64);
                app.errors
                    .insert(format!("{}:{:?}", key.0, key.1), "invalid file".into());
            }
        }
        let current = app.image_key(&items[0], 64);
        app.cache.insert(current.clone(), cached(&app, 64));
        app.demand = HashSet::from([current]);
        app.primary_demand = HashSet::from([items[0].id.clone()]);
        app.foreground_demand = app.demand.clone();
        app.navigation_changed = Instant::now() - Duration::from_secs(10);
        app.neighbor_demand(&ctx);
        let scheduled: Vec<_> = app.demand_jobs.iter().map(|j| j.item.id.as_str()).collect();
        assert_eq!(
            scheduled,
            [items[3].id.as_str(), items[4].id.as_str()],
            "{failure}"
        );
    }
}

#[test]
fn invalidated_neighbors_can_be_prepared_again_in_the_same_view() {
    for source_change in [false, true] {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.cancel_folder_preparation();
        app.cache.clear();
        app.state.view = ViewMode::Preview;
        app.viewer_prefetch_edge = 64;
        let items = app.state.items.clone();
        for item in &items {
            neutral(&mut app, item);
        }
        let current = app.image_key(&items[0], 64);
        let next = app.image_key(&items[1], 64);
        app.prefetched_this_view.insert(next.clone());
        app.foreground_demand = HashSet::from([current.clone()]);
        if source_change {
            app.apply_source_changes(vec![crate::source_monitor::Change {
                id: items[1].id.clone(),
                observation: "changed".into(),
                bytes: items[1].bytes,
                available: true,
            }]);
        } else {
            app.invalidate_previews();
        }
        app.cache.insert(current.clone(), cached(&app, 64));
        app.demand = HashSet::from([current]);
        app.primary_demand = HashSet::from([items[0].id.clone()]);
        app.navigation_changed = Instant::now() - Duration::from_secs(10);
        app.neighbor_demand(&ctx);
        assert!(
            app.demand_jobs
                .iter()
                .any(|j| (j.item.id.clone(), j.request) == next),
            "Invalidation must allow a fresh preview without requiring navigation: source_change={source_change}"
        );
    }
}

#[test]
fn preparation_recovers_after_a_large_request_or_pressure_reduces_the_budget() {
    for rejected in [false, true] {
        for foreground in [false, true] {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            app.service.cache = Arc::new(crate::cache::Manager::new(app.cache_settings.clone()));
            app.cache.clear();
            app.service
                .cache
                .memory
                .configure_automatic(8_000_000_000, 14_000_000_000);
            let observed = if rejected {
                20_000_000_000
            } else {
                10_000_000_000
            };
            app.service.cache.observe_working_bytes(observed);
            let work = app.service.cache.memory.try_reserve(observed);
            assert_eq!(work.is_none(), rejected);
            drop(work);
            app.service
                .cache
                .set_pressure(Some(tr_platform::MemoryPressure::Warning));
            app.service
                .cache
                .set_pressure(Some(tr_platform::MemoryPressure::Normal));
            assert_eq!(app.service.cache.memory.usage().limit, 8_000_000_000);

            app.start_folder_preparation(true);
            app.folder_load.foreground = foreground;
            for item in app.state.items.clone() {
                neutral(&mut app, &item);
            }
            app.demand.clear();
            app.demand_jobs.clear();
            app.folder_preparation_demand(&ctx);
            assert!(
                !app.folder_load.waiting,
                "An old peak must not block a new folder forever"
            );
            assert_eq!(app.demand_jobs.len(), 1);

            app.cancel_folder_preparation();
            app.cache.clear();
            let current = app.state.items[0].clone();
            let key = app.image_key(&current, 64);
            let ready = cached(&app, 64);
            app.cache.insert(key.clone(), ready);
            app.state.view = ViewMode::Preview;
            app.viewer_prefetch_edge = 64;
            app.primary_demand = HashSet::from([current.id]);
            app.demand = HashSet::from([key]);
            app.foreground_demand = app.demand.clone();
            app.demand_jobs.clear();
            app.navigation_changed = Instant::now() - Duration::from_secs(10);
            app.neighbor_demand(&ctx);
            assert_eq!(
                app.demand_jobs.len(),
                1,
                "Neighbour preparation must also recover"
            );
        }
    }
}
