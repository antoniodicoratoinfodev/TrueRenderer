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

#[allow(dead_code)]
#[path = "highlights_hl4.rs"]
mod baseline_hl4;

#[allow(dead_code)]
#[path = "bayer_dir3.rs"]
mod baseline_dir3;

#[allow(dead_code)]
#[path = "bayer_dir4.rs"]
mod baseline_dir4;

#[allow(dead_code)]
#[path = "highlights_hl5.rs"]
mod baseline_hl5;

#[allow(dead_code)]
#[path = "bayer_dir5.rs"]
mod baseline_dir5;

// Preserve the original WB solver too: cal2 accepts some formerly rejected
// profiles. Those new profiles cannot serve as old-HL comparison inputs.
fn original_calibration(c: &crate::color::Calibration) -> crate::color_cal1::Calibration {
    crate::color_cal1::Calibration {
        cm: c.cm,
        cc: c.cc,
        fm: c.fm,
        temperatures: c.temperatures,
        analog: c.analog,
        neutral: c.neutral,
    }
}

// HL3's original demosaic, WB arithmetic, projection, crop and orientation.
// Comparing recoveries through the new demosaic would confound both changes.
fn original_demosaic(mut raw: Mosaic) -> LinearImage {
    let (gains, matrix) = original_calibration(&raw.calibration)
        .resolve([1.; 3])
        .unwrap();
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

// Frozen HL4 recovery with the dir2 or dir3 demosaic. Shared edge equations
// must retain the qualified hashes; the recovery policy is frozen separately.
fn hl4_reference(mut raw: Mosaic, dir3: bool, hl5: bool, dir4: bool, dir5: bool) -> LinearImage {
    for (bytes, expected) in [
        (
            include_bytes!("xtrans_dir6.rs").as_slice(),
            "e9a860a975cfe77ae10d0cc546ecc0a5afb87cbd84db74a305d3011212812ba6",
        ),
        (
            include_bytes!("../src/xtrans_dr6/mod.rs").as_slice(),
            "973d9ea86a871a923ce01898979ba337c00ff03c75eb04f37986ec0a1a605810",
        ),
        (
            include_bytes!("../src/xtrans_dr6/bounded.rs").as_slice(),
            "0e4d4143cbc16f98f9ba5530aca54fb1feedaf4e9cf03bdb5f5fab7464c1612f",
        ),
        (
            include_bytes!("../src/xtrans_dr6/residual.rs").as_slice(),
            "e2146c1d390c7d3756e394305592f89c7f9767c347ba42c6732e20670155113d",
        ),
        (
            include_bytes!("highlights_hl4.rs").as_slice(),
            "e4b10165f6ae3ee5448fe0c712128398f22bef1022defc43e88b5843af675e29",
        ),
        (
            include_bytes!("bayer_dir3.rs").as_slice(),
            "a4f4081f9ab05bc60743dd8d83959ecb9885c33bdb45bbf963d2c277d22551ea",
        ),
        (
            include_bytes!("edge_planes_dir6.rs").as_slice(),
            "5a920d3b8a9bf0af019fde6b7b10b989da545d20d0ea6249ecd148be12486297",
        ),
        (
            include_bytes!("xtrans_steps_dir6.rs").as_slice(),
            "62f13d57ce8e491167c0f6d6b5d65ca33720b0dbc981adf2ecef8b7c3789e54e",
        ),
        (
            include_bytes!("color_cal1.rs").as_slice(),
            "4792f42736503cf239fcc9b300887e2d804bdb0f1f990e32e17fc42c5c5f9f1a",
        ),
        (
            include_bytes!("../src/cfa.rs").as_slice(),
            "d172498b7015c6765fc73116e99ba5e0dbf2cd6566c140e9652669269deb3e3f",
        ),
        (
            include_bytes!("../src/highlights/edge.rs").as_slice(),
            "c35fd64f38022087ca3644cd912da125fe30944b68ee465ee84af5f1f0334dd1",
        ),
        (
            include_bytes!("../src/highlights/occlusion.rs").as_slice(),
            "a08b7e2cf3fb7e28348fd77aa42ded2f526d95fa61b96d0ed17abdce98debe72",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected);
    }
    let mut unsupported = Vec::new();
    if hl5 {
        assert_eq!(
            format!("{:x}", Sha256::digest(include_bytes!("highlights_hl5.rs"))),
            "a3552ce323283ba95f3749c7d8f4b9a65c59f0ef8877327f40e3e88d802b3d64",
            "Shared HL5 recovery changed: this is no longer the frozen baseline"
        );
        baseline_hl5::recover_with_support(
            &mut raw.values,
            &raw.saturation,
            raw.width,
            raw.active_width,
            raw.height,
            raw.cfa,
            Some(&mut unsupported),
        );
    } else {
        baseline_hl4::recover_with_support(
            &mut raw.values,
            &raw.saturation,
            raw.width,
            raw.active_width,
            raw.height,
            raw.cfa,
            Some(&mut unsupported),
        );
    }
    let (gains, matrix) = if dir4 || dir5 {
        assert_eq!(
            format!("{:x}", Sha256::digest(include_bytes!("../src/color.rs"))),
            "0c632b4c1d43bccca112d6766fa21358ef81076bcebdad849cce0850e5c0d351"
        );
        raw.calibration.resolve([1.; 3]).unwrap()
    } else {
        original_calibration(&raw.calibration)
            .resolve([1.; 3])
            .unwrap()
    };
    for (i, v) in raw.values.iter_mut().enumerate() {
        *v *= gains[raw.cfa.color(i % raw.width, i / raw.width)];
    }
    if dir5 {
        assert_eq!(
            format!("{:x}", Sha256::digest(include_bytes!("bayer_dir5.rs"))),
            "ced735d9562289ece19f656d3548e59609e7a453c8cb4ae092f3498263e792b7"
        );
        let clipped = if raw.cfa.period == 2 {
            baseline_dir5::original_clipping_bits(&raw.saturation)
        } else {
            Vec::new()
        };
        return baseline_dir5::develop_supported(
            (
                &raw.values,
                baseline_dir5::Support::new(&unsupported, raw.active_width.div_ceil(6))
                    .with_original_clipping(&clipped, raw.width),
            ),
            raw.width,
            raw.height,
            raw.cfa,
            matrix,
            raw.crop,
            raw.orientation,
        )
        .unwrap();
    }
    if dir4 {
        assert_eq!(
            format!("{:x}", Sha256::digest(include_bytes!("bayer_dir4.rs"))),
            "8fb4d08fdbc8ea12d4f871ac819cf33ff4a1dfbb671b6127fc26a13998887592"
        );
        let clipped = if raw.cfa.period == 2 {
            baseline_dir4::original_clipping_bits(&raw.saturation)
        } else {
            Vec::new()
        };
        return baseline_dir4::develop_supported(
            (
                &raw.values,
                baseline_dir4::Support::new(&unsupported, raw.active_width.div_ceil(6))
                    .with_original_clipping(&clipped, raw.width),
            ),
            raw.width,
            raw.height,
            raw.cfa,
            matrix,
            raw.crop,
            raw.orientation,
        )
        .unwrap();
    }
    if dir3 {
        let clipped = if raw.cfa.period == 2 {
            crate::render::original_clipping_bits(&raw.saturation)
        } else {
            Vec::new()
        };
        return baseline_dir3::develop_supported(
            (
                &raw.values,
                baseline_dir3::Support::new(&unsupported, raw.active_width.div_ceil(6))
                    .with_original_clipping(&clipped, raw.width),
            ),
            raw.width,
            raw.height,
            raw.cfa,
            matrix,
            raw.crop,
            raw.orientation,
        )
        .unwrap();
    }
    crate::render::dir2_reference::develop_supported(
        (
            &raw.values,
            crate::render::Support::new(&unsupported, raw.active_width.div_ceil(6)),
        ),
        raw.width,
        raw.height,
        raw.cfa,
        matrix,
        raw.crop,
        raw.orientation,
    )
    .unwrap()
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
    let compare_hl4 = std::env::var_os("TR_HL_BASELINE_HL4").is_some();
    let compare_hl5_dir5 = std::env::var_os("TR_HL_BASELINE_HL5_DIR5").is_some();
    let compare_hl5_dir4 = std::env::var_os("TR_HL_BASELINE_HL5_DIR4").is_some();
    let compare_hl5_dir3 = std::env::var_os("TR_HL_BASELINE_HL5_DIR3").is_some();
    let compare_hl4_dir3 = std::env::var_os("TR_HL_BASELINE_HL4_DIR3").is_some();
    assert!(
        [
            compare_hl2,
            compare_hl3,
            compare_hl4,
            compare_hl4_dir3,
            compare_hl5_dir3,
            compare_hl5_dir4,
            compare_hl5_dir5,
        ]
        .into_iter()
        .filter(|b| *b)
        .count()
            <= 1
    );
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
            if !compare_hl4
                && !compare_hl4_dir3
                && !compare_hl5_dir3
                && !compare_hl5_dir4
                && !compare_hl5_dir5
            {
                before.saturation.fill(0);
            }
            let baseline = if compare_hl4
                || compare_hl4_dir3
                || compare_hl5_dir3
                || compare_hl5_dir4
                || compare_hl5_dir5
            {
                hl4_reference(
                    before,
                    compare_hl4_dir3 || compare_hl5_dir3,
                    compare_hl5_dir3 || compare_hl5_dir4 || compare_hl5_dir5,
                    compare_hl5_dir4,
                    compare_hl5_dir5,
                )
            } else if compare_hl3 {
                original_demosaic(before)
            } else {
                before.develop([1.; 3]).unwrap()
            };
            let after = if let Ok(root) = std::env::var("TR_HL_PRIVATE_MOSAIC_PROBE") {
                // Explicit test-only probe: the caller supplies sensor-domain
                // candidates, not an image reference. Never product behaviour.
                let mut probe = mosaic(&bytes);
                let candidate = read_f32(
                    &Path::new(&root).join(format!("{}.probe.f32", source["id"].as_str().unwrap())),
                );
                assert_eq!(candidate.len(), probe.values.len());
                for (i, &v) in candidate.iter().enumerate() {
                    assert!(v.is_finite());
                    if probe.saturation[i] != highlights::SATURATED {
                        assert_eq!(v.to_bits(), probe.values[i].to_bits());
                    }
                }
                probe.values = candidate;
                probe.saturation.fill(0);
                probe.develop([1.; 3]).unwrap()
            } else {
                crate::develop(&bytes, [1.; 3]).unwrap()
            };
            let truth = read_f32(Path::new(truthpath));
            if let Ok(root) = std::env::var("TR_HL_SAMPLE_DUMP") {
                let raw = mosaic(&bytes);
                let (gains, matrix) = raw.calibration.resolve([1.; 3]).unwrap();
                let destination =
                    Path::new(&root).join(format!("{}.json", source["id"].as_str().unwrap()));
                assert!(!destination.exists());
                let dump = json!({"source_sha256":hash,
                    "size":[raw.width,raw.height], "crop":raw.crop,
                    "orientation":raw.orientation, "gains":gains,"matrix":matrix,
                    "cfa":(0..raw.cfa.period*raw.cfa.period).map(|i|raw.cfa.color(i%raw.cfa.period,i/raw.cfa.period)).collect::<Vec<_>>(),
                    "measured":raw.values,"state":raw.saturation,
                    "recovered":values,"rendered":after.pixels,"truth":truth});
                fs::write(destination, serde_json::to_vec(&dump).unwrap()).unwrap();
            }
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
            if std::env::var_os("TR_HL_PRIVATE_MOSAIC_PROBE").is_none() {
                assert!(
                    after.pixels == crate::develop(&repacked, [1.; 3]).unwrap().pixels,
                    "reopened image differs from developed source"
                );
                row["repack_sensor_and_render_exact"] = json!(true);
            } else {
                row["private_mosaic_probe"] = json!(true);
            }
        }
        assert_eq!(
            hash,
            format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
        );
        println!("{}: {evidence:?}", source["id"]);
        rows.push(row);
    }
    let report = json!({"protocol":"TR-HL4-v1","baseline_policy":if compare_hl5_dir5 {"frozen hl5/cal2 with dir5/DR7 demosaic"} else if compare_hl5_dir4 {"hl5/cal2 with frozen dir4/DR7 demosaic"} else if compare_hl5_dir3 {"hl5 with frozen dir3/DR7 demosaic"} else if compare_hl4_dir3 {"frozen hl4 with dir3/DR7 demosaic"} else if compare_hl4 {"hl4 with frozen dir2/DR7 demosaic"} else if compare_hl3 {"hl3 with original dir1/DR6 demosaic"} else if compare_hl2 {"hl2 with current demosaic"} else {"no-recovery with current demosaic"},"recipe":crate::RECIPE,"scope":"Known-model or uncalibrated private inputs; no physical colour truth inferred.","sources":rows});
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
