//! Opt-in demand-path diagnostic; use private copies, never a user's catalogue.
use super::*;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::path::Path;
use tr_core::decoder::RawEngine;

fn pixel_digest(image: &tr_core::provider::ImageLevels) -> String {
    pixel_digest_from(image, 0)
}
fn pixel_digest_from(image: &tr_core::provider::ImageLevels, skip: usize) -> String {
    let mut hash = Sha256::new();
    for level in &image.levels()[skip..] {
        hash.update(level.width.to_le_bytes());
        hash.update(level.height.to_le_bytes());
        for pixel in &level.pixels {
            for value in pixel {
                hash.update(value.to_le_bytes());
            }
        }
    }
    format!("{:x}", hash.finalize())
}

/// Exercise the real folder scheduler, then discard RAM and visit every viewer.
/// Source copies and disposable catalogues must be below the diagnostic root.
pub fn folder_loading(root: &Path, worker: &Path, folder: &Path) -> Result<()> {
    use crate::cache::{FolderLoading, Settings};
    let policy = std::env::args()
        .skip_while(|arg| arg != "--folder-cache-policy")
        .nth(1)
        .unwrap_or_else(|| "normal".into());
    ensure!(
        matches!(policy.as_str(), "normal" | "disabled" | "limited"),
        "Unknown folder cache policy"
    );
    let session_previews = policy != "normal";
    let edges: &[u32] = if session_previews {
        &[2700]
    } else {
        &[2700, 0]
    };
    let report_path = root.join("folder-previews.json");
    let mut rows = vec![];
    let report = |rows: &Vec<serde_json::Value>, complete: bool, unchanged: bool| -> Result<()> {
        std::fs::write(
            &report_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "passed":complete && unchanged && rows.iter().all(|r| r["passed"] == true),
                "complete":complete, "sources_unchanged":unchanged, "rows":rows, "cache_policy":policy,
                "scope": if session_previews {
                    "Production folder scheduler, all RAW engines, 1440x900 points / Retina 2x, 2 GiB. Cold preparation with disabled disk cache or 64 MiB quota, then discard native RAM entries and open fitted viewers using retained previews. Private copies and isolated catalogues; optional saved recipe differs from global engine/WB. No GPU/display, colour or physical-memory qualification."
                } else {
                    "Production folder scheduler, all RAW engines, 1440x900 points / Retina 2x, 2 GiB; private source copies and isolated catalogues. Cold preparation then RAM eviction and fitted/native-quality viewers reloaded from disk. Optional saved recipe on the second photo uses a different engine and native WB. No GPU/display, colour or physical-memory qualification."
                }
            }))?,
        )?;
        Ok(())
    };
    report(&rows, false, false)?;
    let folder = &folder.canonicalize()?;
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Isolated decoder required"
    );
    ensure!(
        folder.canonicalize()?.starts_with(root.canonicalize()?),
        "Use private input copies below the diagnostic root"
    );
    let mut source_hashes = None;
    for engine in RawEngine::choices() {
        for quality in [PreviewQuality::Full, PreviewQuality::Standard] {
            for mode in [FolderLoading::Foreground, FolderLoading::Background] {
                let data = tempfile::Builder::new()
                    .prefix("folder-preview-data-")
                    .tempdir_in(root)?;
                Settings {
                    raw_engine: engine,
                    quality,
                    folder_loading: mode,
                    memory_mib: 2048,
                    diagnostic_no_prefetch: true,
                    diagnostic_fixed_memory: true,
                    enabled: policy != "disabled",
                    disk_mib: if policy == "limited" { 64 } else { 4096 },
                    ..Default::default()
                }
                .save(data.path())?;
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(2.);
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = TrueRenderer::new(
                    &cc,
                    root.into(),
                    data.path().into(),
                    worker.into(),
                    Startup {
                        navigation: false,
                        fixed_memory_mib: Some(2048),
                        smoke: false,
                        sampling_smoke: false,
                        external_smoke: false,
                        settings_smoke: false,
                        open: Some(folder.into()),
                    },
                );
                app.cache_settings.diagnostic_no_prefetch = true;
                app.service.cache.configure(app.cache_settings.clone());
                let input = || egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440., 900.),
                    )),
                    ..Default::default()
                };
                let started = Instant::now();
                while app.scanning {
                    let mut output = ctx.run_ui(input(), |_| app.poll(&ctx));
                    output.textures_delta.clear();
                    ensure!(
                        !app.fatal && started.elapsed() < Duration::from_secs(30),
                        "Scan failed"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                let items = app.state.items.clone();
                ensure!(
                    !items.is_empty() && items.iter().all(|i| i.approved),
                    "No eligible images"
                );
                if source_hashes.is_none() {
                    source_hashes = Some(
                        items
                            .iter()
                            .map(|i| tr_platform::snapshot(&i.path).map(|s| (i.path.clone(), s.1)))
                            .collect::<Result<Vec<_>>>()?,
                    );
                }
                if std::env::args().any(|arg| arg == "--folder-saved-recipes") {
                    let item = items
                        .get(1)
                        .ok_or_else(|| anyhow::anyhow!("Two private copies required"))?;
                    app.ensure_edit_loaded(item);
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !app
                        .editing
                        .entries
                        .get(&item.id)
                        .is_some_and(|e| e.loaded.is_some())
                    {
                        app.poll(&ctx);
                        ensure!(Instant::now() < deadline, "Recipe load timeout");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let different = RawEngine::choices()
                        .find(|candidate| *candidate != engine)
                        .unwrap();
                    let mut recipe = tr_core::editing::EditRecipe::neutral(different);
                    if different == RawEngine::Apple {
                        recipe.raw_wb.apple_temperature = 4500;
                        recipe.raw_wb.apple_tint = 8;
                    } else {
                        recipe.raw_wb.red = 1200;
                        recipe.raw_wb.blue = 850;
                    }
                    // Save directly through the catalogue service: this diagnostic
                    // must not replace the recipe's explicit engine on first save.
                    app.request(Request::SaveEdit {
                        id: item.id.clone(),
                        expected_generation: 0,
                        recipe,
                    });
                    while app.editing.entries[&item.id]
                        .loaded
                        .as_ref()
                        .unwrap()
                        .generation
                        == 0
                    {
                        app.poll(&ctx);
                        ensure!(Instant::now() < deadline, "Recipe save timeout");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                app.start_cache_action(false);
                while app.cache_action.is_some() {
                    app.poll_cache_action(&ctx);
                    ensure!(
                        started.elapsed() < Duration::from_secs(60),
                        "Cache clear timeout"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                ensure!(
                    app.status == "Cache della cartella svuotata",
                    "Clear failed: {}",
                    app.status
                );
                let before = app.service.cache.stats();
                let started = Instant::now();
                let mut pixels = HashMap::new();
                loop {
                    let mut output = ctx.run_ui(input(), |_| {
                        app.frame_number += 1;
                        app.demand.clear();
                        app.primary_demand.clear();
                        app.demand_jobs.clear();
                        app.poll(&ctx);
                        if mode == FolderLoading::Background {
                            app.ensure_image(&items[0], 512);
                        }
                        app.trim_images(false);
                        app.background_demand(&ctx);
                        app.flush_demand();
                    });
                    output.textures_delta.clear();
                    for ((id, r), cached) in &app.cache {
                        let item = items.iter().find(|item| item.id == *id).unwrap();
                        if *r == app.preview_request_for_mode(item, 0, false) {
                            for &edge in edges {
                                pixels.entry((id.clone(), edge)).or_insert_with(|| {
                                    pixel_digest_from(
                                        &cached.pyramid,
                                        cached
                                            .pyramid
                                            .requested_base(PreviewRequest { edge, ..*r }),
                                    )
                                });
                            }
                        }
                    }
                    ensure!(
                        !app.fatal
                            && app.errors.is_empty()
                            && started.elapsed() < Duration::from_secs(600),
                        "Folder preparation failed: {:?}",
                        app.errors
                    );
                    if app.folder_progress() == (items.len(), items.len(), 1.)
                        && !app.folder_loading_blocks()
                        && app.pending_images.is_empty()
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let preparation_seconds = started.elapsed().as_secs_f64();
                let prepared = app.service.cache.stats();
                let request = app.folder_load.request.unwrap();
                ensure!(
                    pixels.len() == items.len() * edges.len(),
                    "Not all viewer results were observed"
                );
                if session_previews {
                    app.cache.retain(|(_, request), _| request.edge != 0);
                } else {
                    app.cache.clear();
                }
                let mut visits = vec![];
                for item in &items {
                    for &edge in edges {
                        if !session_previews {
                            app.cache.clear();
                        }
                        let before = app.service.cache.stats();
                        let started = Instant::now();
                        loop {
                            app.frame_number += 1;
                            app.demand.clear();
                            app.primary_demand.clear();
                            app.demand_jobs.clear();
                            app.poll(&ctx);
                            app.ensure_image_priority(item, edge, PreviewPriority::Immediate);
                            app.ensure_image(item, 256);
                            app.flush_demand();
                            if app.cache.contains_key(&app.image_key(item, edge))
                                && app.cache.contains_key(&app.image_key(item, 256))
                            {
                                break;
                            }
                            ensure!(
                                !app.fatal
                                    && app.errors.is_empty()
                                    && started.elapsed() < Duration::from_secs(60),
                                "Viewer failed: {:?}",
                                app.errors
                            );
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        let elapsed = started.elapsed().as_secs_f64();
                        let after = app.service.cache.stats();
                        let cached = &app.cache[&app.image_key(item, edge)];
                        let identical =
                            pixel_digest(&cached.pyramid) == pixels[&(item.id.clone(), edge)];
                        visits.push(serde_json::json!({"edge":edge,"request":app.preview_request(item,edge),"decoder":cached.info.decoder,"base":cached.pyramid.base_level(),"seconds":elapsed,"decode_jobs":after.decode_jobs-before.decode_jobs,"disk_hits":after.hits-before.hits,"pixels_identical":identical}));
                    }
                }
                let passed = visits
                    .iter()
                    .all(|v| v["decode_jobs"] == 0 && v["pixels_identical"] == true);
                let memory = app.service.cache.memory.usage();
                rows.push(serde_json::json!({"passed":passed,"engine":engine,"quality":quality,"mode":mode,"request":request,"images":items.len(),"preparation_seconds":preparation_seconds,"preparation_decode_jobs":prepared.decode_jobs-before.decode_jobs,"preparation_writes":prepared.writes-before.writes,"native_cache_fallbacks":prepared.native_preview_fallbacks-before.native_preview_fallbacks,"viewer_visits":visits,"accounted_memory_peak":memory.peak,"memory_limit":memory.limit}));
                report(&rows, false, false)?;
                eprintln!(
                    "Folder {engine:?}/{quality:?}/{mode:?}: {passed}, {preparation_seconds:.3}s, {} viewers",
                    items.len()
                );
                ensure!(
                    passed,
                    "Prepared viewers triggered new decoding or changed pixels"
                );
            }
        }
    }
    let unchanged =
        source_hashes
            .unwrap()
            .iter()
            .try_fold(true, |same, (path, digest)| -> Result<bool> {
                Ok(same && tr_platform::snapshot(path)?.1 == *digest)
            })?;
    report(&rows, true, unchanged)?;
    ensure!(unchanged, "Sources changed");
    Ok(())
}

pub fn run(root: &Path, worker: &Path, folder: &Path) -> Result<()> {
    let report_path = root.join("raw-previews.json");
    std::fs::write(
        &report_path,
        serde_json::to_vec_pretty(&serde_json::json!({"passed":false,"complete":false,"rows":[]}))?,
    )?;
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Isolated decoder required"
    );
    ensure!(
        folder.canonicalize()?.starts_with(root.canonicalize()?),
        "Use private input copies below the diagnostic root"
    );
    let data = tempfile::Builder::new()
        .prefix("preview-data-")
        .tempdir_in(root)?;
    let settings = crate::cache::Settings {
        memory_mib: 2048,
        diagnostic_fixed_memory: true,
        quality: PreviewQuality::Full,
        ..Default::default()
    };
    settings.save(data.path())?;
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = TrueRenderer::new(
        &cc,
        root.into(),
        data.path().into(),
        worker.into(),
        Startup {
            navigation: false,
            fixed_memory_mib: Some(2048),
            smoke: false,
            sampling_smoke: false,
            external_smoke: false,
            settings_smoke: false,
            open: Some(folder.into()),
        },
    );
    let started = Instant::now();
    while app.scanning {
        app.poll(&ctx);
        ensure!(
            !app.fatal && started.elapsed() < Duration::from_secs(30),
            "Scan failed: {}",
            app.status
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let items = app.state.items.clone();
    ensure!(!items.is_empty(), "No images");
    let source_hashes = items
        .iter()
        .map(|item| tr_platform::snapshot(&item.path).map(|s| s.1))
        .collect::<Result<Vec<_>>>()?;
    app.state.view = ViewMode::Grid;
    let mut rows = vec![];
    for engine in RawEngine::choices() {
        app.cache_settings.raw_engine = engine;
        app.apply_settings();
        for quality in [PreviewQuality::Full, PreviewQuality::Standard] {
            app.set_quality(quality);
            for (batch, visible) in items.chunks(12).enumerate() {
                for phase in ["grid", "viewer_filmstrip", "grid_return"] {
                    app.state.view = if phase == "viewer_filmstrip" {
                        ViewMode::Preview
                    } else {
                        ViewMode::Grid
                    };
                    let before = app.service.cache.stats();
                    let start = Instant::now();
                    let mut ready = false;
                    let mut first_ready = None;
                    let mut visible_ready = None;
                    let thumb_edge = if phase == "viewer_filmstrip" {
                        256
                    } else {
                        512
                    };
                    while start.elapsed() < Duration::from_secs(120) {
                        app.frame_number += 1;
                        app.demand.clear();
                        app.primary_demand.clear();
                        app.demand_jobs.clear();
                        app.poll(&ctx);
                        for item in visible {
                            app.ensure_image(item, thumb_edge);
                        }
                        app.ensure_image_priority(
                            &visible[0],
                            if phase == "viewer_filmstrip" {
                                2048
                            } else {
                                1024
                            },
                            PreviewPriority::Immediate,
                        );
                        let count = visible
                            .iter()
                            .filter(|item| app.cache.contains_key(&app.image_key(item, thumb_edge)))
                            .count();
                        if count > 0 && first_ready.is_none() {
                            first_ready = Some(start.elapsed().as_secs_f64());
                        }
                        if count == visible.len() && visible_ready.is_none() {
                            visible_ready = Some(start.elapsed().as_secs_f64());
                        }
                        ready = app.demand.iter().all(|key| app.cache.contains_key(key));
                        app.trim_images(false);
                        app.flush_demand();
                        app.poll_cache_action(&ctx);
                        if ready || !app.errors.is_empty() || app.fatal {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(16));
                    }
                    let seconds = start.elapsed().as_secs_f64();
                    let memory = app.service.cache.memory.usage();
                    let passed = ready
                        && app.errors.is_empty()
                        && !app.fatal
                        && !memory.automatic
                        && memory.limit == 2048 * 1024 * 1024;
                    let after = app.service.cache.stats();
                    let pixels: Vec<_> = visible
                        .iter()
                        .filter_map(|item| {
                            app.cache
                                .get(&app.image_key(item, thumb_edge))
                                .map(|image| pixel_digest(&image.pyramid))
                        })
                        .collect();
                    rows.push(serde_json::json!({"engine":engine,"quality":quality,"batch":batch,"phase":phase,"images":visible.len(),"passed":passed,"errors":app.errors,"seconds":seconds,"first_thumbnail_seconds":first_ready,"all_thumbnails_seconds":visible_ready,"decode_jobs":after.decode_jobs-before.decode_jobs,"disk_hits":after.hits-before.hits,"pixel_digests":pixels,"cache_statistics":after,"memory_limit":memory.limit,"memory_peak":memory.peak}));
                    std::fs::write(
                        &report_path,
                        serde_json::to_vec_pretty(
                            &serde_json::json!({"passed":false,"complete":false,"rows":rows}),
                        )?,
                    )?;
                    eprintln!(
                        "{engine:?}/{quality:?}/batch {batch}/{phase}: {passed}, {seconds:.3}s, {} decodes {:?}",
                        after.decode_jobs - before.decode_jobs,
                        app.errors
                    );
                    // Keep failures as evidence but independently exercise other engines.
                    if !passed {
                        app.invalidate_previews();
                    }
                }
            }
        }
    }
    let sources_unchanged = items.iter().zip(&source_hashes).try_fold(
        true,
        |unchanged, (item, expected)| -> Result<bool> {
            Ok(unchanged && tr_platform::snapshot(&item.path)?.1 == *expected)
        },
    )?;
    let passed = sources_unchanged && rows.iter().all(|row| row["passed"] == true);
    std::fs::write(
        &report_path,
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":passed,"complete":true,"sources_unchanged":sources_unchanged,"rows":rows,"scope":"Production UI/service grid demand for batches of 12 thumbnails plus inspector at 2 GiB; headless, no display/GPU or colour qualification."}),
        )?,
    )?;
    ensure!(passed, "Preview demand failed: {}", report_path.display());
    Ok(())
}
