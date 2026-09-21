//! Opt-in demand-path diagnostic; use private copies, never a user's catalogue.
use super::*;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::path::Path;
use tr_core::decoder::RawEngine;

fn pixel_digest(image: &tr_core::provider::ImageLevels) -> String {
    let mut hash = Sha256::new();
    for level in image.levels() {
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
                    let passed = ready && app.errors.is_empty() && !app.fatal;
                    let after = app.service.cache.stats();
                    let pixels: Vec<_> = visible
                        .iter()
                        .filter_map(|item| {
                            app.cache
                                .get(&app.image_key(item, thumb_edge))
                                .map(|image| pixel_digest(&image.pyramid))
                        })
                        .collect();
                    rows.push(serde_json::json!({"engine":engine,"quality":quality,"batch":batch,"phase":phase,"images":visible.len(),"passed":passed,"errors":app.errors,"seconds":seconds,"first_thumbnail_seconds":first_ready,"all_thumbnails_seconds":visible_ready,"decode_jobs":after.decode_jobs-before.decode_jobs,"disk_hits":after.hits-before.hits,"pixel_digests":pixels,"cache_statistics":after}));
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
                        app.invalidate_raw_engine();
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
