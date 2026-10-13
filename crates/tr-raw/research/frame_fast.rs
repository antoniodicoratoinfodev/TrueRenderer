//! Full-sensor execution check on the hash-pinned, authorized laboratory RAF.
//! Numerical/streaming evidence only: this is not photographic ground truth.
use super::{bounded_fast as bounded_constrained, constrained, hash};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::PathBuf, time::Instant};

const REGION_WIDTH: usize = 384;
const REGION_HEIGHT: usize = 288;
const TOLERANCE: f32 = 2e-6;

#[test]
#[ignore = "offline DR6 timing research campaign; needs TR_FUJI_LAB_MANIFEST and TR_XTRANS_TIMING_OUTPUT"]
fn optimized_timing_research_campaign() -> Result<()> {
    use std::hint::black_box;
    let config: Value = serde_json::from_slice(&fs::read(std::env::var("TR_FUJI_LAB_MANIFEST")?)?)?;
    let bytes = fs::read(config["source"].as_str().context("source")?)?;
    ensure!(hash(&bytes) == config["source_sha256"], "source hash");
    let raw = crate::raf::Raf::parse(&bytes)?;
    let mut sensor = raw.mosaic()?;
    let (gains, _) = sensor.calibration.resolve([1.; 3])?;
    for (i, v) in sensor.values.iter_mut().enumerate() {
        *v *= gains[sensor.cfa.color(i % sensor.width, i / sensor.width)];
    }
    let (w, h) = (768, 512);
    let (left, top) = ((sensor.width - w) / 2, (sensor.height - h) / 2);
    let cfa = sensor.cfa.shifted(left, top);
    let photo: Vec<_> = (0..w * h)
        .map(|i| sensor.values[(top + i / w) * sensor.width + left + i % w])
        .collect();
    let structured: Vec<_> = (0..w * h)
        .map(|i| 0.4 + 0.3 * (i as f32 % w as f32 * 0.61 + (i / w) as f32 * 0.09).sin())
        .collect();
    let mut reports = vec![];
    for (name, mosaic) in [
        ("RAF fixed central crop", photo),
        ("analytic directional texture", structured),
    ] {
        let execute = |fast: bool| -> Result<Vec<[f32; 3]>> {
            let mut output = vec![[0.; 3]; w * h];
            if fast {
                bounded_constrained::reconstruct_into(
                    &mosaic,
                    w,
                    h,
                    cfa,
                    || false,
                    |x, y, p| {
                        output[y * w + x] = p;
                        Ok(())
                    },
                )?;
            } else {
                super::bounded_constrained::reconstruct_into(
                    &mosaic,
                    w,
                    h,
                    cfa,
                    || false,
                    |x, y, p| {
                        output[y * w + x] = p;
                        Ok(())
                    },
                )?;
            }
            Ok(output)
        };
        let expected = execute(false)?;
        let mut timings: [Vec<f64>; 2] = std::array::from_fn(|_| vec![]);
        let mut maximum = 0f32;
        for iteration in 0..33 {
            for implementation in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = Instant::now();
                let actual = execute(implementation == 1)?;
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                black_box(&actual);
                if iteration >= 3 {
                    timings[implementation].push(elapsed);
                }
                for (a, b) in actual.iter().zip(&expected) {
                    for c in 0..3 {
                        maximum = maximum.max((a[c] - b[c]).abs());
                    }
                }
                ensure!(maximum <= TOLERANCE, "timed execution differs");
            }
        }
        let baseline = super::super::timing_summary(&timings[0]);
        let fast = super::super::timing_summary(&timings[1]);
        reports.push(
            json!({"input":name,"width":w,"height":h,"baseline_dr6":baseline,"optimized_dr6":fast,
            "p50_ratio":fast["p50_ms"].as_f64().unwrap()/baseline["p50_ms"].as_f64().unwrap(),
            "maximum_absolute_error":maximum}),
        );
    }
    fs::OpenOptions::new().write(true).create_new(true)
        .open(std::env::var("TR_XTRANS_TIMING_OUTPUT")?)?.write_all(&serde_json::to_vec_pretty(&json!({
        "source_sha256":config["source_sha256"],"warmups":3,"repetitions":30,"order":"alternating",
        "cases":reports,"limits":"Warm crop kernel including output allocation; no cold start, thermal control or universal frame latency claim."}))?)?;
    Ok(())
}

struct Region {
    left: usize,
    top: usize,
    rgb: Vec<[f32; 3]>,
    compared: usize,
    maximum: f32,
    seam_maximum: f32,
    interpolated_bit_differences: usize,
}

#[test]
#[ignore = "offline DR6 frame check; needs TR_FUJI_LAB_MANIFEST and TR_XTRANS_FRAME_OUTPUT"]
fn full_sensor_optimized_research_campaign() -> Result<()> {
    let manifest = PathBuf::from(std::env::var("TR_FUJI_LAB_MANIFEST")?);
    let output = PathBuf::from(std::env::var("TR_XTRANS_FRAME_OUTPUT")?);
    let config: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    let source = config["source"].as_str().context("source")?;
    let bytes = fs::read(source)?;
    ensure!(hash(&bytes) == config["source_sha256"], "source hash");
    let raw = crate::raf::Raf::parse(&bytes)?;
    let mut cases = vec![];
    for relative_wb in [[1., 1., 1.], [1.2, 1., 0.8]] {
        let mut mosaic = raw.mosaic()?;
        let (w, h) = (mosaic.width, mosaic.height);
        ensure!(
            w >= 3 * REGION_WIDTH && h >= 3 * REGION_HEIGHT && mosaic.cfa.period == 6,
            "full-frame laboratory geometry"
        );
        let (gains, _) = mosaic.calibration.resolve(relative_wb)?;
        for (i, v) in mosaic.values.iter_mut().enumerate() {
            *v *= gains[mosaic.cfa.color(i % w, i / w)];
        }
        // The nine regions and their halo are fixed before viewing this RAF.
        // Scalar evaluation is local so it needs no full-frame integral image.
        let xs = [0, (w - REGION_WIDTH) / 2, w - REGION_WIDTH];
        let ys = [0, (h - REGION_HEIGHT) / 2, h - REGION_HEIGHT];
        let mut columns = vec![None; w];
        let mut rows = vec![None; h];
        for (gx, &left) in xs.iter().enumerate() {
            columns[left..left + REGION_WIDTH].fill(Some(gx));
        }
        for (gy, &top) in ys.iter().enumerate() {
            rows[top..top + REGION_HEIGHT].fill(Some(gy));
        }
        let mut regions = vec![];
        for &top in &ys {
            for &left in &xs {
                let halo = bounded_constrained::HALO;
                let (x0, y0) = (left.saturating_sub(halo), top.saturating_sub(halo));
                let (x1, y1) = (
                    (left + REGION_WIDTH + halo).min(w),
                    (top + REGION_HEIGHT + halo).min(h),
                );
                let (rw, rh) = (x1 - x0, y1 - y0);
                let mut tile = Vec::with_capacity(rw * rh);
                for y in y0..y1 {
                    tile.extend_from_slice(&mosaic.values[y * w + x0..y * w + x1]);
                }
                let reference = constrained::reconstruct(&tile, rw, rh, mosaic.cfa.shifted(x0, y0));
                let mut rgb = Vec::with_capacity(REGION_WIDTH * REGION_HEIGHT);
                for y in top..top + REGION_HEIGHT {
                    let start = (y - y0) * rw + left - x0;
                    rgb.extend_from_slice(&reference[start..start + REGION_WIDTH]);
                }
                regions.push(Region {
                    left,
                    top,
                    rgb,
                    compared: 0,
                    maximum: 0.,
                    seam_maximum: 0.,
                    interpolated_bit_differences: 0,
                });
            }
        }
        let mut sha = Sha256::new();
        let mut measured = 0usize;
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        let started = Instant::now();
        let stats = bounded_constrained::reconstruct_into(
            &mosaic.values,
            w,
            h,
            mosaic.cfa,
            || false,
            |x, y, p| {
                let acquired = mosaic.cfa.color(x, y);
                ensure!(p.iter().all(|v| v.is_finite()), "nonfinite frame output");
                ensure!(
                    p[acquired].to_bits() == mosaic.values[y * w + x].to_bits(),
                    "changed acquired sample"
                );
                measured += 1;
                for c in 0..3 {
                    minimum[c] = minimum[c].min(p[c]);
                    maximum[c] = maximum[c].max(p[c]);
                    sha.update(p[c].to_le_bytes());
                }
                if let (Some(gx), Some(gy)) = (columns[x], rows[y]) {
                    let r = &mut regions[gy * 3 + gx];
                    let reference = r.rgb[(y - r.top) * REGION_WIDTH + x - r.left];
                    r.compared += 1;
                    let edge = bounded_constrained::CORE_EDGE;
                    let seam = [x % edge, y % edge]
                        .into_iter()
                        .any(|p| p == 0 || p == edge - 1);
                    for c in 0..3 {
                        let difference = (p[c] - reference[c]).abs();
                        ensure!(
                            difference <= TOLERANCE,
                            "frame/scalar mismatch at ({x},{y})"
                        );
                        r.maximum = r.maximum.max(difference);
                        if seam {
                            r.seam_maximum = r.seam_maximum.max(difference);
                        }
                        if c != acquired && p[c].to_bits() != reference[c].to_bits() {
                            r.interpolated_bit_differences += 1;
                        }
                    }
                }
                Ok(())
            },
        )?;
        ensure!(stats.pixels == w * h && measured == w * h, "frame coverage");
        let elapsed_seconds = started.elapsed().as_secs_f64();
        for r in &regions {
            ensure!(
                r.compared == REGION_WIDTH * REGION_HEIGHT,
                "region coverage"
            );
        }
        let checks: Vec<_> = regions
            .iter()
            .map(|r| {
                json!({
                    "left":r.left,"top":r.top,"width":REGION_WIDTH,"height":REGION_HEIGHT,
                    "pixels_compared":r.compared,"maximum_absolute_error":r.maximum,
                    "seam_maximum_absolute_error":r.seam_maximum,
                    "interpolated_component_bit_differences":r.interpolated_bit_differences,
                })
            })
            .collect();
        println!(
            "DR6 frame {w}x{h}, WB {relative_wb:?}: {} blocks in {elapsed_seconds:.2}s",
            stats.blocks
        );
        cases.push(json!({
            "width":w,"height":h,"crop":mosaic.crop,"orientation":mosaic.orientation,
            "relative_wb":relative_wb,"resolved_gains":gains,"measured_sites":measured,
            "measured_bit_mismatches":0,"nonfinite_values":0,
            "blocks":stats.blocks,"largest_tile_pixels":stats.largest_tile_pixels,
            "stream_order":"block row, block column, local row, local column, RGB little-endian fp32",
            "stream_sha256":format!("{:x}",sha.finalize()),
            "camera_rgb_min":minimum,"camera_rgb_max":maximum,
            "elapsed_seconds_including_checks_and_sink":elapsed_seconds,"scalar_regions":checks,
        }));
    }
    ensure!(
        hash(&fs::read(source)?) == config["source_sha256"],
        "source changed"
    );
    fs::OpenOptions::new().write(true).create_new(true).open(output)?
        .write_all(&serde_json::to_vec_pretty(&json!({
            "candidate":"DR6-fast","passed":true,"source_sha256":config["source_sha256"],
            "scalar_tolerance":TOLERANCE,"scratch_reservation_bytes":bounded_constrained::SCRATCH_RESERVATION_BYTES,
            "cases":cases,"production_promoted":false,"photographic_ground_truth_available":false,
            "limits":[
                "One authorized X-T5 lossy file, two white balances; not a varied photographic corpus.",
                "The scalar checks use nine local regions with halo, not full-frame scalar integral images.",
                "Timings include checks and sink; one run, not a latency or speedup benchmark.",
                "Scratch excludes source, decoded mosaic, reference regions and sink; no RSS or XPC qualification.",
                "Camera RGB only; no new matrix, orientation, export or product recipe."
            ]
        }))?)?;
    Ok(())
}
