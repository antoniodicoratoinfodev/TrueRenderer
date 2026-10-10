//! Explicitly invoked private campaign; all external decoding goes through XPC.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use tr_core::{
    decoder::{RawEngine, RawWhiteBalance},
    editing::EditRecipe,
    export::{Format, MAX_ENCODED, Options},
};

#[derive(Deserialize)]
struct Manifest {
    corpus: Vec<PathBuf>,
    dng: Vec<PathBuf>,
    convert_d750: Vec<PathBuf>,
}

fn check_dng(root: &Path, worker: &Path, path: &Path, full: bool) -> Result<serde_json::Value> {
    let digest = tr_platform::snapshot(path)?.1;
    let mut cases = vec![];
    let mut reference = None;
    for slot in 0..2 {
        let mut broker = tr_platform::Broker::with_slot(worker.into(), slot);
        broker.set_raw_engine(RawEngine::TrueRendererExperimental);
        let source = broker.prepare_snapshot(path, &digest, || false)?;
        let info = broker.probe_snapshot(&source, || false)?;
        let started = Instant::now();
        let decoded = broker.decode_snapshot_cancellable(
            source.clone(),
            if full { 0 } else { 1024 },
            || false,
        )?;
        ensure!(
            decoded.info.format == "RAW"
                && decoded
                    .info
                    .decoder
                    .contains(RawEngine::TrueRendererExperimental.recipe()),
            "Experimental recipe missing"
        );
        let image = decoded.raster;
        if let Some(expected) = &reference {
            ensure!(expected == &image.pixels, "XPC slots differ");
        } else {
            reference = Some(image.pixels.clone());
        }
        let elapsed = started.elapsed().as_secs_f64();
        let minimum = image
            .pixels
            .iter()
            .flat_map(|p| p[..3].iter())
            .copied()
            .fold(f32::INFINITY, f32::min);
        let maximum = image
            .pixels
            .iter()
            .flat_map(|p| p[..3].iter())
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        let mut roundtrip = None;
        if slot == 0 {
            let (_, bytes) = broker.export_snapshot_bounded(
                source.clone(),
                Options {
                    format: Format::DngRaw,
                    ..Default::default()
                },
                MAX_ENCODED,
                || false,
            )?;
            let copy = root
                .join("var")
                .join(format!("repacked-{}.dng", &digest[..12]));
            std::fs::write(&copy, bytes)?;
            let copy_digest = tr_platform::snapshot(&copy)?.1;
            let reopened = broker
                .decode(&copy, &copy_digest, if full { 0 } else { 1024 })?
                .raster;
            let exact = reopened.pixels == image.pixels
                && (reopened.width, reopened.height) == (image.width, image.height);
            ensure!(exact, "DNG repack changed rendered samples");
            roundtrip = Some(exact);
            let preview = image.reduced(1200);
            image::save_buffer(
                root.join("var")
                    .join(format!("experimental-{}.png", &digest[..12])),
                &preview.to_display(),
                preview.width,
                preview.height,
                image::ColorType::Rgba8,
            )?;
        }
        let wb = RawWhiteBalance {
            red: 1150,
            blue: 900,
            ..Default::default()
        };
        broker.set_raw_white_balance(wb);
        let wb_probe = broker.probe_snapshot(&source, || false)?;
        let wb_image = broker.decode_snapshot_cancellable(source.clone(), 1024, || false)?;
        ensure!(
            wb_probe.input_color == wb_image.info.input_color,
            "WB probe and decode differ"
        );
        ensure!(
            wb_image.raster.pixels != image.reduced(1024).pixels,
            "WB has no effect"
        );
        let mut recipe = EditRecipe::neutral(RawEngine::TrueRendererExperimental);
        recipe.raw_wb = wb;
        let mut exports = vec![];
        for format in [Format::Png16, Format::Tiff16] {
            let (_, bytes) = broker.export_edited_snapshot_bounded(
                source.clone(),
                Options {
                    format,
                    long_edge: 1024,
                    ..Default::default()
                },
                Some(recipe.clone()),
                MAX_ENCODED,
                || false,
            )?;
            let encoded = image::load_from_memory(&bytes)?.to_rgba16();
            ensure!(
                encoded.dimensions() == (wb_image.raster.width, wb_image.raster.height),
                "Experimental RGB export dimensions differ"
            );
            let mut maximum_error = 0_u16;
            for (p, e) in wb_image.raster.pixels.iter().zip(encoded.pixels()) {
                for (a, b) in tr_core::export::srgb16(*p).0.into_iter().zip(e.0) {
                    maximum_error = maximum_error.max(a.abs_diff(b));
                }
            }
            ensure!(
                maximum_error == 0,
                "Experimental RGB export differs: {maximum_error}"
            );
            exports.push(serde_json::json!({"format":format,"long_edge":1024,"maximum_error_u16":maximum_error}));
        }
        broker.set_raw_white_balance(Default::default());
        cases.push(serde_json::json!({"slot":slot,"worker_pid":decoded.worker_pid,"info":info,"seconds":elapsed,"linear_min":minimum,"linear_max":maximum,"repack_pixels_exact":roundtrip,"wb_probe_decode_match":true,"rgb_exports":exports,"observed_peak_rss_bytes":broker.statistics().peak_rss_bytes}));
    }
    ensure!(tr_platform::snapshot(path)?.1 == digest, "Original changed");
    Ok(
        serde_json::json!({"source":path,"sha256":digest,"cases":cases,"slots_pixels_exact":true,"original_unchanged":true}),
    )
}

pub fn run(root: &Path, worker: &Path, manifest: &Path) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Experimental campaign requires OS isolation"
    );
    std::fs::create_dir_all(root.join("var"))?;
    std::fs::create_dir_all(root.join("reports"))?;
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let report = root.join("reports/experimental-private.json");
    let mut results = serde_json::json!({"passed":false,"complete":false,"corpus":[],"dng":[],"converted_d750":[],"limits":"Functional subset only. D750 conversions use legacy LibRaw solely to create test DNG input; this is not autonomous NEF decoding or independent camera calibration. No measured colour accuracy, p95 or memory gate."});
    std::fs::write(&report, serde_json::to_vec_pretty(&results)?)?;
    for (index, path) in manifest.corpus.iter().enumerate() {
        eprintln!(
            "Experimental corpus probe {}/{}",
            index + 1,
            manifest.corpus.len()
        );
        let digest = tr_platform::snapshot(path)?.1;
        let mut broker = tr_platform::Broker::with_slot(worker.into(), (index % 2) as u32);
        let source = broker.prepare_snapshot(path, &digest, || false)?;
        broker.set_raw_engine(RawEngine::LibRawBilinear);
        let legacy = match broker.probe_snapshot(&source, || false) {
            Ok(info) => serde_json::json!({"accepted":true,"info":info}),
            Err(error) => serde_json::json!({"accepted":false,"error":format!("{error:#}")}),
        };
        broker.set_raw_engine(RawEngine::TrueRendererExperimental);
        let error = broker
            .probe_snapshot(&source, || false)
            .expect_err("NEF/RAF must not become a hidden legacy decode");
        ensure!(tr_platform::snapshot(path)?.1 == digest, "Original changed");
        results["corpus"].as_array_mut().unwrap().push(serde_json::json!({"source":path,"sha256":digest,"legacy_metadata":legacy,"experimental_supported":false,"explicit_refusal":format!("{error:#}"),"original_unchanged":true}));
        std::fs::write(&report, serde_json::to_vec_pretty(&results)?)?;
    }
    for path in manifest.dng {
        eprintln!("Experimental generated DNG validation");
        results["dng"]
            .as_array_mut()
            .unwrap()
            .push(check_dng(root, worker, &path, true)?);
        std::fs::write(&report, serde_json::to_vec_pretty(&results)?)?;
    }
    for (index, path) in manifest.convert_d750.into_iter().enumerate() {
        eprintln!("Experimental real-sensor DNG validation {}", index + 1);
        let digest = tr_platform::snapshot(&path)?.1;
        let mut broker = tr_platform::Broker::new(worker.into());
        broker.set_raw_engine(RawEngine::TrueRenderer);
        let source = broker.prepare_snapshot(&path, &digest, || false)?;
        let (_, bytes) = broker.export_snapshot_bounded(
            source,
            Options {
                format: Format::DngRaw,
                ..Default::default()
            },
            MAX_ENCODED,
            || false,
        )?;
        // A service slot owns one active lease. Release the conversion broker
        // before the independent Experimental pass acquires both slots.
        drop(broker);
        let converted = root.join("var").join(format!("d750-derived-{index}.dng"));
        std::fs::write(&converted, bytes)?;
        let mut value = check_dng(root, worker, &converted, true)?;
        value["input_conversion"] = serde_json::json!({"engine":RawEngine::TrueRenderer,"source_sha256":digest,"profile_source":"legacy LibRaw D65 table, embedded in generated DNG"});
        ensure!(
            tr_platform::snapshot(&path)?.1 == digest,
            "Original changed"
        );
        results["converted_d750"]
            .as_array_mut()
            .unwrap()
            .push(value);
        std::fs::write(&report, serde_json::to_vec_pretty(&results)?)?;
    }
    results["passed"] = true.into();
    results["complete"] = true.into();
    std::fs::write(report, serde_json::to_vec_pretty(&results)?)?;
    println!("Experimental campaign passed; private report retained under isolated root.");
    Ok(())
}
