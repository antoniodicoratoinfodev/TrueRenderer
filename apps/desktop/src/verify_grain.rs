//! Native creative-grain/export parity on an explicit isolated root.
use anyhow::{Result, ensure};
use std::path::Path;
use tr_core::{
    decoder::RawEngine,
    editing::{
        EditRecipe,
        grain::{Distribution, Grain},
    },
};

pub fn recipe(engine: RawEngine) -> EditRecipe {
    let mut r = EditRecipe::neutral(engine);
    r.process_version = 3;
    r.advanced = Some(Box::default());
    r.advanced.as_mut().unwrap().detail.grain = Grain {
        enabled: true,
        distribution: Distribution::Gaussian,
        amount: 35.,
        size: 2.4,
        opacity: 65.,
        seed: 231,
        ..Default::default()
    };
    r
}

pub fn export(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    use tr_core::export::{Format, MAX_ENCODED, Options};
    let output = root.join("grain-export");
    std::fs::create_dir(&output)?;
    let (_, digest) = tr_platform::snapshot(source)?;
    let engine = RawEngine::TrueRendererExperimental;
    let mut broker = tr_platform::Broker::with_slot(worker.into(), 0);
    broker.set_raw_engine(engine);
    let input = broker.decode(source, &digest, 0)?.raster;
    drop(broker);
    let mut cases = Vec::new();
    for (index, (distribution, monochrome, format, long_edge)) in [
        (Distribution::Uniform, true, Format::Png16, 0),
        (Distribution::Gaussian, false, Format::Tiff16, 0),
        (Distribution::Gaussian, true, Format::Png16, 800),
        (Distribution::Uniform, false, Format::Tiff16, 800),
    ]
    .into_iter()
    .enumerate()
    {
        let mut r = recipe(engine);
        let a = r.advanced.as_mut().unwrap();
        a.detail.grain.distribution = distribution;
        a.detail.grain.monochrome = monochrome;
        a.geometry.crop = [0.1, 0.1, 0.9, 0.9];
        a.geometry.quarter_turns = 1;
        let mut expected = input.clone();
        let started = std::time::Instant::now();
        r.apply(&mut expected)?;
        let render_seconds = started.elapsed().as_secs_f64();
        if long_edge > 0 {
            expected = expected.reduced(long_edge);
        }
        let mut broker = tr_platform::Broker::with_slot(worker.into(), (index % 2) as u32);
        broker.set_raw_engine(engine);
        let snapshot = broker.prepare_snapshot(source, &digest, || false)?;
        let (_, bytes) = broker.export_edited_snapshot_bounded(
            snapshot,
            Options {
                format,
                long_edge,
                ..Default::default()
            },
            Some(r.clone()),
            MAX_ENCODED,
            || false,
        )?;
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
            "Grain dimensions differ"
        );
        let mut maximum = 0;
        for (p, q) in expected.pixels.iter().zip(actual.pixels()) {
            for (a, b) in tr_core::export::srgb16(*p).0.into_iter().zip(q.0) {
                maximum = maximum.max(a.abs_diff(b));
            }
        }
        ensure!(maximum == 0, "Grain export mismatch: {maximum}");
        std::fs::write(
            output.join(format!("case-{index}.{}", format.extension())),
            &bytes,
        )?;
        cases.push(serde_json::json!({"recipe":r,"format":format,"long_edge":long_edge,"maximum_error_u16":maximum,"render_seconds":render_seconds}));
    }
    ensure!(
        tr_platform::snapshot(source)?.1 == digest,
        "Source changed during grain probe"
    );
    std::fs::write(
        output.join("parity.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"source_sha256":digest,"source_unchanged":true,"cases":cases}),
        )?,
    )?;
    println!("Grain export parity passed (four cases)");
    Ok(())
}
