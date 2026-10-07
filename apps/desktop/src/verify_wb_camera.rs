//! Compare native RAW adjustments with metadata-only camera WB variants over XPC.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::path::Path;
use tr_core::{
    decoder::{RawEngine, RawWhiteBalance},
    editing::EditRecipe,
    export::{Format, MAX_ENCODED, Options},
};

#[derive(Deserialize)]
struct Manifest {
    source: String,
    source_sha256: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    engine: RawEngine,
    camera: String,
    camera_sha256: String,
    wb: RawWhiteBalance,
    tolerance: f32,
}

pub fn run(root: &Path, worker: &Path, folder: &Path) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "WB camera probe requires isolated platform services"
    );
    std::fs::create_dir_all(root.join("reports"))?;
    let report = root.join("reports/wb-camera.json");
    std::fs::write(&report, br#"{"passed":false,"reason":"probe incomplete"}"#)?;
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(folder.join("manifest.json"))?)?;
    let source = folder.join(&manifest.source);
    let (_, digest) = tr_platform::snapshot(&source)?;
    ensure!(digest == manifest.source_sha256, "Source checksum mismatch");
    let mut results = vec![];
    for case in manifest.cases {
        case.wb.validate_for(case.engine)?;
        let camera = folder.join(&case.camera);
        let (_, camera_digest) = tr_platform::snapshot(&camera)?;
        ensure!(
            camera_digest == case.camera_sha256,
            "Camera variant checksum mismatch"
        );
        for slot in 0..2 {
            let mut broker = tr_platform::Broker::with_slot(worker.into(), slot);
            broker.set_raw_engine(case.engine);
            let camera_image = broker.decode(&camera, &camera_digest, 0)?.raster;
            let original = broker.decode(&source, &digest, 0)?.raster;
            broker.set_raw_white_balance(case.wb);
            let changed = broker.decode(&source, &digest, 0)?.raster;
            ensure!(
                (changed.width, changed.height) == (camera_image.width, camera_image.height),
                "Different native sizes"
            );
            let mut maximum = 0_f32;
            let mut sum = 0_f64;
            for (a, b) in changed.pixels.iter().zip(&camera_image.pixels) {
                for (a, b) in a.iter().zip(b) {
                    ensure!(a.is_finite() && b.is_finite(), "Nonfinite WB result");
                    let error = (a - b).abs();
                    maximum = maximum.max(error);
                    sum += f64::from(error);
                }
            }
            ensure!(
                changed.pixels != original.pixels,
                "WB adjustment had no effect"
            );
            // Export receives the same native WB through its frozen recipe.
            let snapshot = broker.prepare_snapshot(&source, &digest, || false)?;
            let mut recipe = EditRecipe::neutral(case.engine);
            recipe.raw_wb = case.wb;
            let mut exports = vec![];
            for format in [Format::Png16, Format::Tiff16] {
                let (_, bytes) = broker.export_edited_snapshot_bounded(
                    snapshot.clone(),
                    Options {
                        format,
                        ..Default::default()
                    },
                    Some(recipe.clone()),
                    MAX_ENCODED,
                    || false,
                )?;
                let encoded = image::load_from_memory(&bytes)?.to_rgba16();
                ensure!(
                    encoded.dimensions() == (changed.width, changed.height),
                    "Export dimensions"
                );
                let mut error = 0_u16;
                for (p, e) in changed.pixels.iter().zip(encoded.pixels()) {
                    for (a, b) in tr_core::export::srgb16(*p).0.into_iter().zip(e.0) {
                        error = error.max(a.abs_diff(b));
                    }
                }
                ensure!(error == 0, "Export WB parity: {error}");
                exports.push(serde_json::json!({"format":format,"maximum_error_u16":error}));
            }
            broker.set_raw_white_balance(Default::default());
            ensure!(
                broker.decode(&source, &digest, 0)?.raster.pixels == original.pixels,
                "As-shot reset mismatch"
            );
            let passed = maximum <= case.tolerance;
            results.push(serde_json::json!({"engine":case.engine,"slot":slot,"wb":case.wb,"camera":case.camera,"maximum_error_fp32":maximum,"mean_error_fp32":sum/(changed.pixels.len()*4) as f64,"tolerance":case.tolerance,"passed":passed,"exports":exports,"as_shot_reset_exact":true,"size":[changed.width,changed.height]}));
            println!(
                "{:?}, slot {slot}, {}: maximum {maximum}, passed={passed}",
                case.engine, case.camera
            );
        }
        ensure!(
            tr_platform::snapshot(&camera)?.1 == camera_digest,
            "Camera variant changed"
        );
    }
    let unchanged = tr_platform::snapshot(&source)?.1 == digest;
    let passed = unchanged && results.len() == 24 && results.iter().all(|r| r["passed"] == true);
    std::fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":passed,"source_digest":digest,"source_unchanged":unchanged,"cases":results,"scope":"Same generated single-illuminant Bayer samples, only AsShotNeutral changes; native fp32 Rec2020 comparison, both isolated workers, exact PNG/TIFF16 parity, as-shot reset. Not a camera JPEG match or camera calibration qualification."}),
        )?,
    )?;
    ensure!(passed, "Native WB vs camera metadata comparison failed");
    Ok(())
}
