//! Opt-in numerical/photographic diagnostics. Artifacts stay outside source control.
use crate::{Raw, highlights, wb::Mosaic};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use tr_core::color::LinearImage;

// Frozen photographic HL2 policy, retained solely as a numerical comparator.
#[allow(dead_code)]
#[path = "highlights_hl2.rs"]
mod baseline_hl2;

#[allow(dead_code)]
#[path = "highlights_hl3.rs"]
mod baseline_hl3;

// HL3's original demosaic, WB arithmetic, projection, crop and orientation.
// Comparing recoveries through the new demosaic would confound both changes.
fn original_demosaic(mut raw: Mosaic) -> LinearImage {
    let (gains, matrix) = raw.calibration.resolve([1.; 3]).unwrap();
    for (i, v) in raw.values.iter_mut().enumerate() {
        *v *= gains[raw.cfa.color(i % raw.width, i / raw.width)];
    }
    if raw.cfa.period == 2 {
        return crate::render::frame_reference::develop(
            &raw.values,
            raw.width,
            raw.height,
            raw.cfa,
            matrix,
            raw.crop,
            raw.orientation,
        )
        .unwrap();
    }
    let [left, top, w, h] = raw.crop;
    let (ow, oh) = crate::render::oriented_size(w as u32, h as u32, raw.orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    crate::xtrans_dr6::bounded::reconstruct_into(
        &raw.values,
        raw.width,
        raw.height,
        raw.cfa,
        || false,
        |x, y, rgb| {
            if x < left || y < top || x >= left + w || y >= top + h {
                return Ok(());
            }
            let (cx, cy) = (x - left, y - top);
            let rec = crate::color::project(matrix, rgb);
            let (ox, oy) = match raw.orientation {
                2 => (w - 1 - cx, cy),
                3 => (w - 1 - cx, h - 1 - cy),
                4 => (cx, h - 1 - cy),
                5 => (cy, cx),
                6 => (h - 1 - cy, cx),
                7 => (h - 1 - cy, w - 1 - cx),
                8 => (cy, w - 1 - cx),
                _ => (cx, cy),
            };
            pixels[oy * ow as usize + ox] = [rec[0], rec[1], rec[2], 1.];
            Ok(())
        },
    )
    .unwrap();
    LinearImage::new(ow, oh, pixels).unwrap()
}

fn mosaic(bytes: &[u8]) -> Mosaic {
    match Raw::parse(bytes).unwrap() {
        Raw::Dng(r) => r.mosaic().unwrap(),
        Raw::Nef(r) => r.mosaic().unwrap(),
        Raw::Raf(r) => r.mosaic().unwrap(),
    }
}

fn read_f32(path: &Path) -> Vec<f32> {
    let b = fs::read(path).unwrap();
    assert!(b.len().is_multiple_of(4));
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect()
}

fn metrics(image: &LinearImage, truth: &[f32], rect: [usize; 4]) -> Value {
    let w = image.width as usize;
    let [x0, y0, width, height] = rect;
    assert_eq!(truth.len(), image.pixels.len() * 3);
    assert!(x0 + width < w && y0 + height < image.height as usize);
    let (mut rgb, mut chroma, mut gradient) = (0., 0., 0.);
    let (mut rgb_max, mut chroma_max, mut gradient_max) = (0f64, 0f64, 0f64);
    let (mut rgb_site, mut chroma_site, mut gradient_site) = (None, None, None);
    let mut chroma_count = 0;
    for y in y0..y0 + height {
        for x in x0..x0 + width {
            let i = y * w + x;
            let a = image.pixels[i];
            let b = &truth[i * 3..i * 3 + 3];
            let sa = f64::from(a[0]) + f64::from(a[1]) + f64::from(a[2]);
            let sb = f64::from(b[0]) + f64::from(b[1]) + f64::from(b[2]);
            for c in 0..3 {
                let error = f64::from(a[c]) - f64::from(b[c]);
                rgb += error.powi(2);
                if error.abs() > rgb_max {
                    rgb_max = error.abs();
                    rgb_site = Some([x, y, c]);
                }
                if sa > 0.01 && sb > 0.01 {
                    let error = f64::from(a[c]) / sa - f64::from(b[c]) / sb;
                    chroma += error.powi(2);
                    if error.abs() > chroma_max {
                        chroma_max = error.abs();
                        chroma_site = Some([x, y, c]);
                    }
                }
                for j in [i + 1, i + w] {
                    let error = f64::from(image.pixels[j][c])
                        - f64::from(a[c])
                        - f64::from(truth[j * 3 + c])
                        + f64::from(b[c]);
                    gradient += error.powi(2);
                    if error.abs() > gradient_max {
                        gradient_max = error.abs();
                        gradient_site = Some([x, y, c, usize::from(j == i + w)]);
                    }
                }
            }
            chroma_count += usize::from(sa > 0.01 && sb > 0.01);
        }
    }
    let count = (width * height * 3) as f64;
    json!({"rect":rect,"rgb_rmse":(rgb/count).sqrt(),"gradient_rmse":(gradient/(2.*count)).sqrt(),
        "chroma_rmse":(chroma/(chroma_count*3) as f64).sqrt(),"chroma_pixels":chroma_count,
        "rgb_max_abs":rgb_max,"gradient_max_abs":gradient_max,
        "chroma_max_abs":(chroma_count > 0).then_some(chroma_max),
        "rgb_max_site_xyc":rgb_site,"chroma_max_site_xyc":chroma_site,
        "gradient_max_site_xyc_axis":gradient_site})
}

fn common_chroma(a: &LinearImage, b: &LinearImage, truth: &[f32], rect: [usize; 4]) -> Value {
    let mut sse = [0f64; 2];
    let mut maximum = [0f64; 2];
    let mut count = 0usize;
    let mut excluded = [0usize; 3];
    let [x, y, w, h] = rect;
    for yy in y..y + h {
        for xx in x..x + w {
            let i = yy * a.width as usize + xx;
            let p = [
                &a.pixels[i][..3],
                &b.pixels[i][..3],
                &truth[i * 3..i * 3 + 3],
            ];
            let sums = p.map(|v| v.iter().map(|&f| f64::from(f)).sum::<f64>());
            for k in 0..3 {
                excluded[k] += usize::from(sums[k] <= 0.01);
            }
            if sums.iter().any(|&v| v <= 0.01) {
                continue;
            }
            count += 1;
            for k in 0..2 {
                for (&measured, &reference) in p[k].iter().zip(p[2]) {
                    let error = f64::from(measured) / sums[k] - f64::from(reference) / sums[2];
                    sse[k] += error * error;
                    maximum[k] = maximum[k].max(error.abs());
                }
            }
        }
    }
    json!({"pixels":count,"excluded_before_after_truth":excluded,
        "before_after_rmse":sse.map(|v|(count>0).then(||(v/(3*count) as f64).sqrt())),
        "before_after_max_abs":maximum.map(|v|(count>0).then_some(v))})
}

#[test]
fn metrics_retain_isolated_colour_and_detail_errors() {
    let mut pixels = vec![[1.; 4]; 64];
    pixels[3 * 8 + 4][0] = 1.5;
    let image = LinearImage::new(8, 8, pixels).unwrap();
    let result = metrics(&image, &[1.; 64 * 3], [0, 0, 7, 7]);
    assert_eq!(result["rgb_max_abs"], 0.5);
    assert_eq!(result["rgb_max_site_xyc"], json!([4, 3, 0]));
    assert_eq!(result["gradient_max_abs"], 0.5);
    assert!(result["rgb_rmse"].as_f64().unwrap() < 0.05);
    assert!(result["chroma_max_abs"].as_f64().unwrap() > 0.09);
    assert_eq!(result["chroma_max_site_xyc"], json!([4, 3, 0]));
}

#[test]
#[ignore = "explicit manifest/output under a private research root"]
fn research_campaign_highlights() {
    let input = std::env::var("TR_HL_MANIFEST").unwrap();
    let output = std::env::var("TR_HL_REPORT").unwrap();
    assert!(!Path::new(&output).exists());
    let manifest: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    let compare_hl2 = std::env::var_os("TR_HL_BASELINE_HL2").is_some();
    let compare_hl3 = std::env::var_os("TR_HL_BASELINE_HL3").is_some();
    assert!(!(compare_hl2 && compare_hl3));
    if compare_hl3 {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(include_bytes!("../src/highlights/edge.rs"))
            ),
            "c35fd64f38022087ca3644cd912da125fe30944b68ee465ee84af5f1f0334dd1",
            "HL3 shared edge equations changed"
        );
    }
    let mut rows = vec![];
    for source in manifest["sources"].as_array().unwrap() {
        let path = Path::new(source["source"].as_str().unwrap());
        let bytes = fs::read(path).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(hash, source["sha256"].as_str().unwrap());
        let raw = mosaic(&bytes);
        let mut values = raw.values.clone();
        let evidence = highlights::recover(
            &mut values,
            &raw.saturation,
            raw.width,
            raw.active_width,
            raw.height,
            raw.cfa,
        );
        let valid_changes = (0..values.len())
            .filter(|&i| {
                raw.saturation[i] != highlights::SATURATED
                    && values[i].to_bits() != raw.values[i].to_bits()
            })
            .count();
        assert_eq!(valid_changes, 0);
        let cfa_path = path.with_extension("cfa");
        let mut sensor_error = json!(null);
        if cfa_path.exists() {
            let truth = read_f32(&cfa_path);
            assert_eq!(truth.len(), values.len());
            let mut squared = [0.; 2];
            let mut maximum = [0f64; 2];
            for i in 0..values.len() {
                if raw.saturation[i] == highlights::SATURATED {
                    squared[0] += (f64::from(raw.values[i]) - f64::from(truth[i])).powi(2);
                    squared[1] += (f64::from(values[i]) - f64::from(truth[i])).powi(2);
                    maximum[0] =
                        maximum[0].max((f64::from(raw.values[i]) - f64::from(truth[i])).abs());
                    maximum[1] = maximum[1].max((f64::from(values[i]) - f64::from(truth[i])).abs());
                }
            }
            sensor_error = json!({"truth_sha256":format!("{:x}",Sha256::digest(fs::read(cfa_path).unwrap())),
                "saturated_rmse":squared.map(|s|(s/evidence.saturated.max(1) as f64).sqrt()),
                "saturated_max_abs":maximum});
        }
        let mut before = raw;
        let mut row = json!({"id":source["id"],"source_sha256":hash,"saturated_sites":evidence.saturated,
            "reconstructed_sites":evidence.reconstructed,"supported_cells":evidence.supported_cells,
            "valid_sites_changed":valid_changes,"sensor_error":sensor_error});
        if let Some(truthpath) = source["truth"].as_str() {
            row["unsupported_by_column"] = json!(
                (0..before.width)
                    .map(|x| (0..before.height)
                        .filter(|y| {
                            let i = y * before.width + x;
                            before.saturation[i] == highlights::SATURATED
                                && values[i] == before.values[i]
                        })
                        .count())
                    .collect::<Vec<_>>()
            );
            if compare_hl2 {
                let base = baseline_hl2::recover(
                    &mut before.values,
                    &before.saturation,
                    before.width,
                    before.active_width,
                    before.height,
                    before.cfa,
                );
                row["baseline_reconstructed_sites"] = json!(base.reconstructed);
            }
            if compare_hl3 {
                let base = baseline_hl3::recover(
                    &mut before.values,
                    &before.saturation,
                    before.width,
                    before.active_width,
                    before.height,
                    before.cfa,
                );
                row["baseline_reconstructed_sites"] = json!(base.reconstructed);
            }
            before.saturation.fill(0);
            let baseline = if compare_hl3 {
                original_demosaic(before)
            } else {
                before.develop([1.; 3]).unwrap()
            };
            let after = crate::develop(&bytes, [1.; 3]).unwrap();
            let truth = read_f32(Path::new(truthpath));
            // This optional counterfactual separates recovery error from the
            // demosaic's response to the ideal, unclipped analytical mosaic.
            // It is not a second physical camera reference.
            if std::env::var_os("TR_HL_UNCLIPPED_REFERENCE").is_some()
                && path.with_extension("cfa").exists()
            {
                let mut oracle = mosaic(&bytes);
                oracle.values = read_f32(&path.with_extension("cfa"));
                oracle.saturation.fill(0);
                let oracle_image = oracle.develop([1.; 3]).unwrap();
                row["unclipped_reference_demosaic"] = json!(
                    source["patches"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| {
                            let rect = std::array::from_fn(|i| p[i].as_u64().unwrap() as usize);
                            metrics(&oracle_image, &truth, rect)
                        })
                        .collect::<Vec<_>>()
                );
            }
            let truth_hash = format!("{:x}", Sha256::digest(fs::read(truthpath).unwrap()));
            assert_eq!(truth_hash, source["truth_sha256"].as_str().unwrap());
            row["truth_sha256"] = json!(truth_hash);
            row["output_identical"] = json!(baseline.pixels == after.pixels);
            row["render_hashes"] = json!([&baseline, &after].map(|image| {
                let mut hash = Sha256::new();
                for pixel in &image.pixels {
                    for channel in pixel {
                        hash.update(channel.to_le_bytes());
                    }
                }
                format!("{:x}", hash.finalize())
            }));
            row["patches"] = json!(source["patches"].as_array().unwrap().iter().map(|p| {
                let rect=std::array::from_fn(|i|p[i].as_u64().unwrap() as usize);
                json!({"before":metrics(&baseline,&truth,rect),"after":metrics(&after,&truth,rect),"common_chroma":common_chroma(&baseline,&after,&truth,rect)})
            }).collect::<Vec<_>>());
            let (_, _, repacked) = crate::repack_dng(&bytes, crate::MAX_SOURCE_BYTES).unwrap();
            assert_eq!(
                crate::unpack(&bytes).unwrap().2,
                crate::unpack(&repacked).unwrap().2
            );
            assert_eq!(
                after.pixels,
                crate::develop(&repacked, [1.; 3]).unwrap().pixels
            );
            row["repack_sensor_and_render_exact"] = json!(true);
        }
        assert_eq!(
            hash,
            format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
        );
        println!("{}: {evidence:?}", source["id"]);
        rows.push(row);
    }
    let report = json!({"protocol":"TR-HL4-v1","baseline_policy":if compare_hl3 {"hl3 with original dir1/DR6 demosaic"} else if compare_hl2 {"hl2 with current demosaic"} else {"no-recovery with current demosaic"},"recipe":crate::RECIPE,"scope":"Known-model or uncalibrated private inputs; no physical colour truth inferred.","sources":rows});
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
