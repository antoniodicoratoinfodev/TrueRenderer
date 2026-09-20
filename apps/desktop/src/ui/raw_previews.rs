//! Opt-in demand-path diagnostic; use private copies, never a user's catalogue.
use super::*;
use anyhow::{Result, ensure};
use std::path::Path;
use tr_core::decoder::RawEngine;

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
                let start = Instant::now();
                let mut ready = false;
                while start.elapsed() < Duration::from_secs(120) {
                    app.frame_number += 1;
                    app.demand.clear();
                    app.primary_demand.clear();
                    app.demand_jobs.clear();
                    app.poll(&ctx);
                    for item in visible {
                        app.ensure_image(item, 512);
                    }
                    // The inspector is a simultaneous consumer of the selected RAW.
                    app.ensure_image(&visible[0], 1024);
                    ready = app.demand.iter().all(|key| app.cache.contains_key(key));
                    app.trim_images(false);
                    app.flush_demand();
                    app.poll_cache_action(&ctx);
                    if ready || !app.errors.is_empty() || app.fatal {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
                let passed = ready && app.errors.is_empty() && !app.fatal;
                rows.push(serde_json::json!({"engine":engine,"quality":quality,"batch":batch,"images":visible.len(),"passed":passed,"errors":app.errors,"seconds":start.elapsed().as_secs_f64()}));
                std::fs::write(
                    &report_path,
                    serde_json::to_vec_pretty(
                        &serde_json::json!({"passed":false,"complete":false,"rows":rows}),
                    )?,
                )?;
                eprintln!(
                    "{engine:?}/{quality:?}/batch {batch}: {passed} {:?}",
                    app.errors
                );
                // Keep failures as evidence but independently exercise other engines.
                if !passed {
                    app.invalidate_raw_engine();
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
