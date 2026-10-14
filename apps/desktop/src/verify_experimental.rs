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
    #[serde(default)]
    nef: Vec<PathBuf>,
    #[serde(default)]
    raf: Vec<PathBuf>,
    convert_d750: Vec<PathBuf>,
    #[serde(default)]
    provenance: serde_json::Value,
}

fn check_raw(root: &Path, worker: &Path, path: &Path, full: bool) -> Result<serde_json::Value> {
    let digest = tr_platform::snapshot(path)?.1;
    let mut cases = vec![];
    let mut reference = None;
    for slot in 0..2 {
        let mut broker = tr_platform::Broker::with_slot(worker.into(), slot);
        broker.set_raw_engine(RawEngine::TrueRendererExperimental);
        let source = broker.prepare_snapshot(path, &digest, || false)?;
        let info = broker.probe_snapshot(&source, || false)?;
        let cancelled_pid = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("raf"))
        {
            let pid = broker.statistics().worker_pid;
            let stops = broker.statistics().forced_stops;
            let started = Instant::now();
            let failure = match broker.decode_snapshot_cancellable(source.clone(), 0, || {
                started.elapsed() >= std::time::Duration::from_millis(150)
            }) {
                Ok(_) => anyhow::bail!("RAF cancellation finished without interruption"),
                Err(error) => error.to_string(),
            };
            ensure!(
                failure.contains("annullata")
                    && started.elapsed() < std::time::Duration::from_secs(3)
                    && broker.statistics().forced_stops == stops + 1,
                "RAF in-flight cancellation did not stop its worker: {failure}"
            );
            pid
        } else {
            None
        };
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
        if let Some(pid) = cancelled_pid {
            ensure!(
                decoded.worker_pid != Some(pid),
                "Cancelled RAF worker was not replaced"
            );
        }
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
        let (roundtrip, repacked_path, repacked_sha256) = {
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
                .join(format!("repacked-{}-slot{slot}.dng", &digest[..12]));
            std::fs::write(&copy, bytes)?;
            let copy_digest = tr_platform::snapshot(&copy)?.1;
            let reopened = broker
                .decode(&copy, &copy_digest, if full { 0 } else { 1024 })?
                .raster;
            let exact = reopened.pixels == image.pixels
                && (reopened.width, reopened.height) == (image.width, image.height);
            ensure!(exact, "DNG repack changed rendered samples");
            (exact, copy, copy_digest)
        };
        if slot == 0 {
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
        let wb = RawWhiteBalance::precise_sensor_gains([1.150317, 1., 0.900273])?;
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
        let mut wb_estimates = vec![];
        for analysis in [
            tr_core::raw_wb::Analysis::Auto,
            tr_core::raw_wb::Analysis::Patch {
                x: info.width / 2,
                y: info.height / 2,
                side: 11,
            },
        ] {
            let started = Instant::now();
            let estimated = broker.estimate_raw_wb(source.clone(), analysis, || false);
            let seconds = started.elapsed().as_secs_f64();
            let copy = broker.prepare_snapshot(&repacked_path, &repacked_sha256, || false)?;
            let reopened = broker.estimate_raw_wb(copy, analysis, || false);
            let value = match (estimated, reopened) {
                (Ok(wb), Ok(copy_wb)) => {
                    ensure!(wb == copy_wb, "CFA WB differs after RAW repack");
                    serde_json::json!({"analysis":analysis,"accepted":true,"wb":wb,"repack_exact":true,"seconds":seconds})
                }
                (Err(error), Err(copy_error)) => {
                    let message = format!("{error:#}");
                    let copy_message = format!("{copy_error:#}");
                    for refusal in [&message, &copy_message] {
                        ensure!(
                            refusal.contains("WB CFA:")
                                || refusal.contains("DNG: bianco")
                                || refusal.contains("DNG: CCT"),
                            "Unexpected WB failure: {refusal}"
                        );
                    }
                    serde_json::json!({"analysis":analysis,"accepted":false,"explicit_refusal":message,"repack_refusal":format!("{copy_error:#}"),"seconds":seconds})
                }
                (Err(error), Ok(_)) => {
                    anyhow::bail!("CFA WB original failed while repack succeeded: {error:#}")
                }
                (Ok(_), Err(error)) => {
                    anyhow::bail!("CFA WB repack failed while original succeeded: {error:#}")
                }
            };
            wb_estimates.push(value);
        }
        cases.push(serde_json::json!({"slot":slot,"worker_pid":decoded.worker_pid,"cancelled_raf_worker_pid":cancelled_pid,"info":info,"seconds":elapsed,"linear_min":minimum,"linear_max":maximum,"repack_pixels_exact":roundtrip,"repacked_path":repacked_path,"repacked_sha256":repacked_sha256,"wb_probe_decode_match":true,"cfa_wb_estimates":wb_estimates,"rgb_exports":exports,"observed_peak_rss_bytes":broker.statistics().peak_rss_bytes,"cpu_recycles":broker.statistics().cpu_recycles,"observed_peak_worker_cpu_nanoseconds":broker.statistics().peak_worker_cpu_nanoseconds}));
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
    let mut results = serde_json::json!({"passed":false,"complete":false,"corpus":[],"dng":[],"nef":[],"raf":[],"converted_d750":[],"input_provenance":manifest.provenance,"limits":"Functional subset only. The optional convert_d750 branch uses legacy LibRaw solely to create test DNG input. Provenance of supplied DNG files is declared by the input manifest, not inferred by this harness. Direct RAW checks apply only to the explicitly listed subtype. No measured colour accuracy, p95 or memory gate."});
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
        eprintln!("Experimental DNG validation");
        results["dng"]
            .as_array_mut()
            .unwrap()
            .push(check_raw(root, worker, &path, true)?);
        std::fs::write(&report, serde_json::to_vec_pretty(&results)?)?;
    }
    for (index, path) in manifest.nef.iter().enumerate() {
        eprintln!(
            "Experimental direct NEF validation {}/{}",
            index + 1,
            manifest.nef.len()
        );
        results["nef"]
            .as_array_mut()
            .unwrap()
            .push(check_raw(root, worker, path, true)?);
        std::fs::write(&report, serde_json::to_vec_pretty(&results)?)?;
    }
    for (index, path) in manifest.raf.iter().enumerate() {
        eprintln!(
            "Experimental direct RAF validation {}/{}",
            index + 1,
            manifest.raf.len()
        );
        results["raf"]
            .as_array_mut()
            .unwrap()
            .push(check_raw(root, worker, path, true)?);
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
        let mut value = check_raw(root, worker, &converted, true)?;
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
