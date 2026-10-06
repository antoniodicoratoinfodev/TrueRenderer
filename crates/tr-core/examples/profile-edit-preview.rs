//! CPU-stage diagnostic on generated pixels, without a decoder or UI.
//! Usage: cargo-local.sh run --release -p tr-core --example profile-edit-preview
//!        -- OUTPUT.json [SAMPLES=20] [THREADS=available-1]
//! No command-to-display latency, RAW-engine ranking or quality claim is made.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{hint::black_box, time::Instant};
use tr_core::{
    color::LinearImage,
    decoder::RawEngine,
    editing::{CurvePoint, EditRecipe, masks::Mask},
    provider::ImageLevels,
};

fn recipes() -> Vec<(&'static str, EditRecipe)> {
    let mut exposure = EditRecipe::neutral(RawEngine::TrueRenderer);
    exposure.exposure_ev = 0.5;
    let mut empty_advanced = exposure.clone();
    empty_advanced.process_version = 3;
    empty_advanced.advanced = Some(Box::default());
    let mut tone = exposure.clone();
    tone.process_version = 2;
    tone.brightness = 12.;
    tone.contrast = 15.;
    tone.highlights = -20.;
    tone.shadows = 20.;
    tone.temperature = 10.;
    tone.saturation = 12.;
    tone.vibrance = 15.;
    tone.curve = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 0.5, y: 0.55 },
        CurvePoint { x: 1., y: 1. },
    ];
    let mut detail = tone.clone();
    detail.process_version = 3;
    detail.advanced = Some(Box::default());
    let advanced = detail.advanced.as_mut().unwrap();
    advanced.detail.texture = 20.;
    advanced.detail.clarity = 20.;
    advanced.detail.sharpen = 40.;
    advanced.detail.luminance_noise = 20.;
    advanced.detail.chroma_noise = 20.;
    advanced.detail.dehaze = 10.;
    let mut color_masks = tone.clone();
    color_masks.process_version = 3;
    color_masks.advanced = Some(Box::default());
    let advanced = color_masks.advanced.as_mut().unwrap();
    advanced.color.bands[0].saturation = 20.;
    advanced.color.grading[0].hue = 220.;
    advanced.color.grading[0].amount = 15.;
    advanced.masks = (0..4)
        .map(|i| Mask {
            center: [0.2 + i as f32 * 0.2, 0.5],
            exposure: 0.3,
            warmth: 10.,
            ..Default::default()
        })
        .collect();
    let mut geometry = tone.clone();
    geometry.process_version = 3;
    geometry.advanced = Some(Box::default());
    let advanced = geometry.advanced.as_mut().unwrap();
    advanced.geometry.angle = 2.;
    advanced.geometry.distortion = 10.;
    advanced.geometry.vignette = 20.;
    vec![
        ("exposure", exposure),
        ("exposure_with_neutral_advanced", empty_advanced),
        ("tone_color_curve", tone),
        ("tone_and_detail", detail),
        ("tone_and_color_4_masks", color_masks),
        ("tone_and_geometry", geometry),
    ]
}

fn raster([width, height]: [u32; 2]) -> Result<LinearImage> {
    LinearImage::new(
        width,
        height,
        (0..width * height)
            .map(|i| {
                let x = (i % width) as f32 / width as f32;
                let y = (i / width) as f32 / height as f32;
                let noise = ((i.wrapping_mul(1664525).wrapping_add(1013904223) >> 16) as f32
                    / 65535.
                    - 0.5)
                    * 0.03;
                [
                    0.02 + 0.8 * x + noise,
                    0.03 + 0.7 * y,
                    0.1 + 0.6 * x * y,
                    1.,
                ]
            })
            .collect(),
    )
}

fn stats(values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    json!({
        "samples_ms": values,
        "min_ms": sorted[0],
        "median_ms": (sorted[(count - 1) / 2] + sorted[count / 2]) / 2.,
        "max_ms": sorted[count - 1],
        // Small, exploratory runs must not advertise tail-latency qualification.
        "p95_ms": (count >= 100).then(|| sorted[(count * 95).div_ceil(100) - 1]),
    })
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        (2..=4).contains(&args.len()),
        "Expected OUTPUT.json [SAMPLES] [THREADS]"
    );
    let count: usize = args.get(2).map_or(Ok(20), |v| v.parse())?;
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    let threads: usize = args
        .get(3)
        .map_or(Ok(available.saturating_sub(1).max(1)), |v| v.parse())?;
    ensure!(
        (3..=1000).contains(&count),
        "Samples must be between 3 and 1000"
    );
    ensure!(
        (1..=available).contains(&threads),
        "Threads exceed available CPUs"
    );
    tr_core::compute::configure(threads)?;
    let native = [6032_u32, 4032_u32];
    let mut cases = vec![];
    for base in [4, 3, 2] {
        let size = native.map(|v| v.div_ceil(1 << base));
        let source = raster(size)?;
        for (name, initial_recipe) in recipes() {
            let mut stages: [Vec<f64>; 5] = Default::default();
            let mut histogram_times = vec![];
            let mut retained_bytes = 0;
            let mut output_digest = String::new();
            for sample in 0..count + 2 {
                // Two warmups, then changing drafts of one continuous control.
                let mut recipe = initial_recipe.clone();
                recipe.exposure_ev += (sample % 17) as f32 * 0.01;
                let start = Instant::now();
                let mut image = black_box(&source).clone();
                let cloned = Instant::now();
                let output_size = black_box(&recipe).apply_preview(&mut image, native, base)?;
                let edited = Instant::now();
                let opaque = image.pixels.iter().all(|p| p[3] == 1.);
                let checked = Instant::now();
                let levels = ImageLevels::from_reference_mip(image, output_size, base, opaque)?;
                let finished = Instant::now();
                retained_bytes = levels.byte_len();
                // Separate diagnostic only: the app's histogram uses its own
                // <=512px derivative; this is not part of the viewer worker.
                let histogram = black_box(levels.source().histogram());
                let histogram_finished = Instant::now();
                ensure!(
                    histogram.iter().all(|channel| {
                        channel.iter().map(|v| u64::from(*v)).sum::<u64>()
                            == u64::from(size[0]) * u64::from(size[1])
                    }),
                    "Histogram lost pixels"
                );
                black_box(&levels);
                if sample == count + 1 {
                    let mut hash = Sha256::new();
                    for level in levels.levels() {
                        for value in level.pixels.iter().flatten() {
                            hash.update(value.to_le_bytes());
                        }
                    }
                    output_digest = format!("{:x}", hash.finalize());
                }
                if sample >= 2 {
                    for (stage, (end, begin)) in stages.iter_mut().zip([
                        (cloned, start),
                        (edited, cloned),
                        (checked, edited),
                        (finished, checked),
                        (finished, start),
                    ]) {
                        stage.push((end - begin).as_secs_f64() * 1000.);
                    }
                    histogram_times.push((histogram_finished - finished).as_secs_f64() * 1000.);
                }
            }
            let case = json!({
                "name": name, "recipe": initial_recipe, "native_geometry": native,
                "mip_base": base, "size": size, "retained_bytes": retained_bytes,
                "last_output_sha256": output_digest,
                "clone": stats(&stages[0]), "edit": stats(&stages[1]),
                "opacity": stats(&stages[2]), "pyramid": stats(&stages[3]),
                "worker_total": stats(&stages[4]),
                "histogram_separate": stats(&histogram_times),
            });
            eprintln!(
                "{size:?} {name}: edit {:.2} ms, pyramid {:.2} ms, worker {:.2} ms (medians)",
                case["edit"]["median_ms"].as_f64().unwrap(),
                case["pyramid"]["median_ms"].as_f64().unwrap(),
                case["worker_total"]["median_ms"].as_f64().unwrap()
            );
            cases.push(case);
        }
    }
    let report = json!({
        "schema": 1,
        "scope": "CPU edit-worker stages on synthetic opaque linear Rec.2020 pixels; not UI/GPU/RAW or physical-display latency. Base 3 matches the ordinary <=1024px edit mip for the stated geometry in both preview qualities; bases 4/2 explore size cost only. No quality equivalence between mips is claimed.",
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "debug_assertions": cfg!(debug_assertions),
        "available_parallelism": available, "compute_pool_threads": threads,
        "sample_count_per_case": count, "warmups_per_case": 2,
        "input": "Generated independently at each mip geometry; no RAW decoding, source photography or catalog access.",
        "cases": cases,
    });
    std::fs::write(&args[1], serde_json::to_string_pretty(&report)? + "\n")
        .context("Write profile report")?;
    Ok(())
}
