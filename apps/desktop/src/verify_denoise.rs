//! Explicit private photographic probe; RAW decoding keeps the normal broker/XPC boundary.
use anyhow::{Result, ensure};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};
use tr_core::decoder::RawEngine;

pub fn recipe(engine: RawEngine) -> tr_core::editing::EditRecipe {
    let mut r = tr_core::editing::EditRecipe::neutral(engine);
    r.process_version = 3;
    r.advanced = Some(Box::default());
    let d = &mut r.advanced.as_mut().unwrap().detail;
    d.luminance_noise = 80.;
    d.chroma_noise = 80.;
    d.threshold = 25.;
    r
}

pub fn run(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    let output = root.join("denoise");
    std::fs::create_dir(&output)?;
    let (_, digest) = tr_platform::snapshot(source)?;
    let mut broker = tr_platform::Broker::with_slot(worker.into(), 0);
    broker.set_raw_engine(RawEngine::TrueRendererExperimental);
    let start = std::time::Instant::now();
    let decoded = broker.decode(source, &digest, 0)?;
    let seconds = start.elapsed().as_secs_f64();
    let mut file = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("input.rgba32f"))?,
    );
    for pixel in &decoded.raster.pixels {
        for v in pixel {
            file.write_all(&v.to_le_bytes())?;
        }
    }
    file.flush()?;
    let overview = decoded.raster.reduced(1200);
    image::save_buffer(
        output.join("overview.png"),
        &overview.to_display(),
        overview.width,
        overview.height,
        image::ColorType::Rgba8,
    )?;
    ensure!(
        tr_platform::snapshot(source)?.1 == digest,
        "Source changed during denoise probe"
    );
    let report = serde_json::json!({"source_sha256":digest,"source_unchanged":true,"size":[decoded.raster.width,decoded.raster.height],"decode_seconds":seconds,"info":decoded.info});
    std::fs::write(
        output.join("input.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub fn export(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    use tr_core::export::{Format, MAX_ENCODED, Options};
    let output = root.join("denoise-export");
    std::fs::create_dir(&output)?;
    let (_, digest) = tr_platform::snapshot(source)?;
    let engine = RawEngine::TrueRendererExperimental;
    let r = recipe(engine);
    let mut broker = tr_platform::Broker::with_slot(worker.into(), 0);
    broker.set_raw_engine(engine);
    let mut expected = broker.decode(source, &digest, 0)?.raster;
    eprintln!("Denoise: native decode complete; applying host reference");
    let start = std::time::Instant::now();
    r.apply(&mut expected)?;
    let filter_seconds = start.elapsed().as_secs_f64();
    eprintln!("Denoise: host reference complete in {filter_seconds:.3}s");
    // Fresh service per export: leave the full lifetime CPU reserve available.
    drop(broker);
    let mut cases = vec![];
    for (slot, format) in [(0, Format::Png16), (1, Format::Tiff16)] {
        let mut broker = tr_platform::Broker::with_slot(worker.into(), slot);
        broker.set_raw_engine(engine);
        let snapshot = broker.prepare_snapshot(source, &digest, || false)?;
        eprintln!("Denoise: starting {format:?} export on XPC slot {slot}");
        let start = std::time::Instant::now();
        let (info, bytes) = broker.export_edited_snapshot_bounded(
            snapshot,
            Options {
                format,
                ..Default::default()
            },
            Some(r.clone()),
            MAX_ENCODED,
            || false,
        )?;
        let seconds = start.elapsed().as_secs_f64();
        eprintln!("Denoise: {format:?} export complete in {seconds:.3}s; checking pixels");
        let export_path = output.join(format!(
            "slot-{slot}.{}",
            if slot == 0 { "png" } else { "tiff" }
        ));
        std::fs::write(&export_path, &bytes)?;
        // The generic reader's 512 MiB default cannot hold both TIFF decode
        // buffers at 36 MP. Bound this explicit diagnostic by the known frame;
        // product decoder quotas and sandbox permissions remain unchanged.
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(expected.width);
        limits.max_image_height = Some(expected.height);
        limits.max_alloc =
            Some(u64::from(expected.width) * u64::from(expected.height) * 24 + 64 * 1024 * 1024);
        reader.limits(limits);
        let actual = reader.decode()?.to_rgba16();
        ensure!(
            actual.dimensions() == (expected.width, expected.height),
            "Denoise export dimensions"
        );
        let mut maximum = 0_u16;
        for (p, q) in expected.pixels.iter().zip(actual.pixels()) {
            for (a, b) in tr_core::export::srgb16(*p).0.into_iter().zip(q.0) {
                maximum = maximum.max(a.abs_diff(b));
            }
        }
        ensure!(maximum == 0, "Denoise export mismatch: {maximum}");
        cases.push(serde_json::json!({"slot":slot,"format":format,"seconds":seconds,"maximum_error_u16":maximum,"info":info,"peak_worker_rss_bytes":broker.statistics().peak_rss_bytes,"peak_worker_cpu_nanoseconds":broker.statistics().peak_worker_cpu_nanoseconds}));
    }
    ensure!(
        tr_platform::snapshot(source)?.1 == digest,
        "Source changed during denoise export"
    );
    let report = serde_json::json!({"passed":true,"source_sha256":digest,"source_unchanged":true,"recipe":r,"size":[expected.width,expected.height],"filter_seconds":filter_seconds,"scratch_bound_bytes":r.scratch_bytes(expected.width,expected.height),"cases":cases});
    std::fs::write(
        output.join("parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Denoise export parity passed");
    Ok(())
}
