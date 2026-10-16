//! Exact-equivalence and cost checks for the Bayer row scheduler.
use super::{BayerRows, bayer_scratch_reservation_bytes, develop, frame_reference};
use crate::{cfa::Cfa, color::ID};

#[test]
fn streaming_bayer_preserves_bits_across_phases_crops_wb_and_orientation() {
    for (w, h) in [(2, 2), (2, 3), (3, 2), (7, 9), (38, 27), (67, 53)] {
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
            for pattern in 0..4 {
                for gains in [[1.; 3], [1.150317, 1., 0.900273]] {
                    let mosaic: Vec<f32> = (0..w * h)
                        .map(|i| {
                            let value = match pattern {
                                0 => (i * 137 % 997) as f32 / 300. - 0.3,
                                1 => [-0.125, 0.45, 1.5][cfa.color(i % w, i / w)],
                                2 => [-0., 0., -f32::MIN_POSITIVE, f32::MIN_POSITIVE][i % 4],
                                _ => {
                                    0.2 + 0.9
                                        * (0.47 * (i % w) as f32 + 0.31 * (i / w) as f32).sin()
                                }
                            };
                            value * gains[cfa.color(i % w, i / w)]
                        })
                        .collect();
                    for matrix in [
                        ID,
                        [
                            [1.13, -0.22, 0.034],
                            [-0.27, 1.21, 0.08],
                            [0.09, -0.03, 0.97],
                        ],
                    ] {
                        for crop in [
                            [0, 0, w, h],
                            [0, 0, 1, 1],
                            [w - 1, h - 1, 1, 1],
                            [1, 1, w - 1, h - 1],
                            [w / 3, h / 3, w / 2, h / 2],
                        ] {
                            for orientation in 1..=8 {
                                let expected = frame_reference::develop(
                                    &mosaic,
                                    w,
                                    h,
                                    cfa,
                                    matrix,
                                    crop,
                                    orientation,
                                )
                                .unwrap();
                                let actual =
                                    develop(&mosaic, w, h, cfa, matrix, crop, orientation).unwrap();
                                assert_eq!(
                                    (actual.width, actual.height),
                                    (expected.width, expected.height)
                                );
                                for (i, (a, b)) in
                                    actual.pixels.iter().zip(&expected.pixels).enumerate()
                                {
                                    assert_eq!(
                                        a.map(f32::to_bits),
                                        b.map(f32::to_bits),
                                        "{w}x{h}, phase {phase}, pattern {pattern}, crop {crop:?}, orientation {orientation}, pixel {i}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn green_rows_reserve_only_width_dependent_buffers() {
    for w in [2, 7, 6032, 7380] {
        for h in [2, 8193] {
            let rows = BayerRows::new(w, h, Cfa::bayer([0, 1, 1, 2]));
            let allocated = rows.columns.capacity() * size_of::<[usize; 4]>()
                + rows
                    .green
                    .iter()
                    .map(|r| r.capacity() * size_of::<f32>())
                    .sum::<usize>()
                + size_of::<BayerRows>();
            assert!(allocated <= bayer_scratch_reservation_bytes(w, h));
            assert_eq!(rows.green.len(), 3);
            assert!(rows.green.iter().all(|r| r.len() == w));
        }
    }
}

fn image_hash(image: &tr_core::color::LinearImage) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 1024 * 16];
    for pixels in image.pixels.chunks(1024) {
        for (pixel, destination) in pixels.iter().zip(bytes.as_chunks_mut::<16>().0) {
            for (v, b) in pixel.iter().zip(destination.as_chunks_mut::<4>().0) {
                b.copy_from_slice(&v.to_le_bytes());
            }
        }
        hash.update(&bytes[..pixels.len() * 16]);
    }
    format!("{:x}", hash.finalize())
}

#[test]
#[ignore = "offline Bayer equivalence/timing research campaign; needs TR_BAYER_STREAM_MANIFEST and TR_BAYER_STREAM_OUTPUT"]
fn bayer_streaming_research_campaign() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::{fs, hint::black_box, io::Write, time::Instant};
    let source_config = fs::read(std::env::var("TR_BAYER_STREAM_MANIFEST")?)?;
    let config: Value = serde_json::from_slice(&source_config)?;
    let destination = std::env::var("TR_BAYER_STREAM_OUTPUT")?;
    ensure!(
        !std::path::Path::new(&destination).exists(),
        "report already exists"
    );
    let sources = config["sources"].as_array().context("sources")?;
    ensure!(!sources.is_empty(), "empty Bayer campaign");
    let mut cases = vec![];
    for (source_index, source) in sources.iter().enumerate() {
        let path = source["source"].as_str().context("source")?;
        ensure!(
            fs::metadata(path)?.len() <= crate::MAX_SOURCE_BYTES as u64,
            "source size"
        );
        let bytes = fs::read(path)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        ensure!(source["sha256"] == digest, "source hash");
        let raw = crate::Raw::parse(&bytes)?;
        let info = raw.info()?;
        let sensor = match raw {
            crate::Raw::Dng(r) => r.mosaic()?,
            crate::Raw::Nef(r) => r.mosaic()?,
            crate::Raw::Raf(_) => anyhow::bail!("Bayer campaign does not accept RAF"),
        };
        ensure!(sensor.cfa.period == 2, "Bayer CFA required");
        for relative in [[1.; 3], [1.150317, 1., 0.900273]] {
            let (gains, matrix) = sensor.calibration.resolve(relative)?;
            let mosaic: Vec<_> = sensor
                .values
                .iter()
                .enumerate()
                .map(|(i, &v)| v * gains[sensor.cfa.color(i % sensor.width, i / sensor.width)])
                .collect();
            let execute = |streamed: bool| {
                let method = if streamed {
                    develop
                } else {
                    frame_reference::develop
                };
                method(
                    &mosaic,
                    sensor.width,
                    sensor.height,
                    sensor.cfa,
                    matrix,
                    sensor.crop,
                    sensor.orientation,
                )
            };
            let expected = execute(false)?;
            let compare = |actual: &tr_core::color::LinearImage| -> anyhow::Result<()> {
                ensure!(
                    (actual.width, actual.height) == (expected.width, expected.height),
                    "geometry mismatch"
                );
                ensure!(
                    actual
                        .pixels
                        .iter()
                        .zip(&expected.pixels)
                        .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits)),
                    "Bayer float bits changed"
                );
                Ok(())
            };
            compare(&execute(true)?)?;
            let mut timing: Option<Value> = None;
            if relative == [1.; 3] && source["benchmark"] == true {
                let mut timings: [Vec<f64>; 2] = std::array::from_fn(|_| vec![]);
                for iteration in 0..33 {
                    for method in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
                        let start = Instant::now();
                        let actual = execute(method == 1)?;
                        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
                        black_box(&actual);
                        compare(&actual)?;
                        if iteration >= 3 {
                            timings[method].push(elapsed_ms);
                        }
                    }
                }
                let summaries = timings.map(|mut times| {
                    times.sort_by(f64::total_cmp);
                    json!({"p50_ms":(times[14] + times[15]) * 0.5,"p95_ms":times[28],"maximum_ms":times[29],"samples_ms":times})
                });
                let ratio = summaries[1]["p50_ms"].as_f64().unwrap()
                    / summaries[0]["p50_ms"].as_f64().unwrap();
                timing = Some(
                    json!({"reference":summaries[0],"streamed":summaries[1],"p50_ratio":ratio,
                    "speed_target_passed":ratio <= 0.8,"warmups":3,"repetitions":30,"order":"alternating"}),
                );
            }
            cases.push(json!({"source_sha256":digest,"camera":info.camera,"profile_hash":info.profile_hash,
                "sensor_size":[sensor.width,sensor.height],"crop":sensor.crop,"orientation":sensor.orientation,
                "relative_wb":relative,"resolved_gains":gains,"rgba_float_components_compared":expected.pixels.len()*4,
                "bit_mismatches":0,"rgba_f32le_sha256":image_hash(&expected),
                "reference_green_buffer_bytes":sensor.width*sensor.height*4,
                "streamed_scratch_reservation_bytes":bayer_scratch_reservation_bytes(sensor.width, sensor.height),
                "decoder_scratch_peak_bytes":info.memory.scratch_peak_bytes,"kernel_timing":timing}));
        }
        ensure!(
            format!("{:x}", Sha256::digest(fs::read(path)?)) == digest,
            "source changed"
        );
        println!(
            "Bayer stream: {}/{} inputs compared",
            source_index + 1,
            sources.len()
        );
    }
    let report = json!({"protocol":"TRExp-Bayer-streaming-v1","recipe":crate::RECIPE,
        "manifest_sha256":format!("{:x}", Sha256::digest(&source_config)),
        "reference_source_sha256":format!("{:x}", Sha256::digest(include_bytes!("bayer_frame_reference.rs"))),
        "candidate_source_sha256":format!("{:x}", Sha256::digest(include_bytes!("../src/render.rs"))),
        "passed":true,"cases":cases,"numerical_recipe_change":false,
        "limits":"Exact equivalence of existing reconstruction; no physical color qualification. Timers cover full demosaic/matrix/crop/orientation/output allocation, exclude parse/codec/WB/checks. Warm timings without thermal control; scratch accounting is not RSS."});
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
