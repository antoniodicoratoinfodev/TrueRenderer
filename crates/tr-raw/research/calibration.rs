//! Offline CFA patch metrology, before WB, demosaic and profile conversion.
use crate::{Raw, cfa::Cfa};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::PathBuf};

fn legacy_wb_gains([red, _, blue]: [f64; 3]) -> [f64; 3] {
    tr_core::decoder::RawWhiteBalance {
        red: (red * 1000.).round() as u16,
        blue: (blue * 1000.).round() as u16,
        ..Default::default()
    }
    .gains_f64()
}

#[test]
fn wb_audit_uses_stored_legacy_fp32_gains() {
    let gains = legacy_wb_gains([1.33349049, 1., 0.75049049]);
    assert_eq!(gains, [f64::from(1.333f32), 1., f64::from(0.750f32)]);
    assert_ne!(gains[0], 1.333f64);
}

#[derive(Clone, Copy)]
struct PhaseMoments {
    count: usize,
    mean: f64,
    m2: f64,
    minimum: f64,
    maximum: f64,
    negative: usize,
    saturated: usize,
}

impl PhaseMoments {
    fn empty() -> Self {
        Self {
            count: 0,
            mean: 0.,
            m2: 0.,
            minimum: f64::INFINITY,
            maximum: f64::NEG_INFINITY,
            negative: 0,
            saturated: 0,
        }
    }
    fn push(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (value - self.mean);
        self.minimum = self.minimum.min(value);
        self.maximum = self.maximum.max(value);
        self.negative += usize::from(value < 0.);
        self.saturated += usize::from(value >= 0.985);
    }
    fn report(self, phase: usize, cfa: Cfa) -> Value {
        json!({"phase_xy":[phase % cfa.period, phase / cfa.period],"color":cfa.map[phase],
            "sample_count":self.count,"mean":(self.count > 0).then_some(self.mean),
            "sample_variance":(self.count > 1).then(|| self.m2 / (self.count - 1) as f64),
            "minimum":(self.count > 0).then_some(self.minimum),
            "maximum":(self.count > 0).then_some(self.maximum),
            "negative_sites":self.negative,"saturated_sites":self.saturated})
    }
}

fn patch(values: &[f32], width: usize, height: usize, cfa: Cfa, rect: [usize; 4]) -> Result<Value> {
    let [left, top, w, h] = rect;
    ensure!(
        width.checked_mul(height) == Some(values.len()),
        "mosaic geometry"
    );
    ensure!(
        w > 0
            && h > 0
            && left.checked_add(w).is_some_and(|v| v <= width)
            && top.checked_add(h).is_some_and(|v| v <= height),
        "patch outside sensor"
    );
    let mut n = [0usize; 3];
    let mut mean = [0.; 3];
    let mut m2 = [0.; 3];
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut saturated = [0usize; 3];
    let mut negative = [0usize; 3];
    let mut phases = vec![PhaseMoments::empty(); cfa.period * cfa.period];
    for y in top..top + h {
        for x in left..left + w {
            let c = cfa.color(x, y);
            let v = f64::from(values[y * width + x]);
            ensure!(v.is_finite(), "nonfinite calibrated site");
            phases[(y % cfa.period) * cfa.period + x % cfa.period].push(v);
            n[c] += 1;
            let d = v - mean[c];
            mean[c] += d / n[c] as f64;
            m2[c] += d * (v - mean[c]);
            low[c] = low[c].min(v);
            high[c] = high[c].max(v);
            saturated[c] += usize::from(v >= 0.985);
            negative[c] += usize::from(v < 0.);
        }
    }
    ensure!(
        n.into_iter().all(|v| v >= 3),
        "insufficient sites per channel"
    );
    let variance: [f64; 3] = std::array::from_fn(|c| m2[c] / (n[c] - 1) as f64);
    let green_comparison = if cfa.period == 2 {
        let phase = |neighbor| {
            (0..4)
                .find(|&i| cfa.map[i] == 1 && cfa.color((i % 2) ^ 1, i / 2) == neighbor)
                .unwrap()
        };
        let (red, blue) = (phase(0), phase(2));
        let (gr, gb) = (phases[red], phases[blue]);
        let difference = (gr.count > 0 && gb.count > 0 && gr.mean > 0. && gb.mean > 0.)
            .then(|| (gr.mean - gb.mean) / ((gr.mean + gb.mean) * 0.5));
        Some(
            json!({"green_red_row":[red % 2, red / 2],"green_blue_row":[blue % 2, blue / 2],
            "relative_mean_difference":difference}),
        )
    } else {
        None
    };
    Ok(
        json!({"rect_sensor":rect,"camera_rgb":mean,"sample_count":n,"sample_variance":variance,
        "minimum":low,"maximum":high,"saturated_sites":saturated,"negative_sites":negative,
        "fit_eligible_no_saturation":saturated == [0; 3],
        "phase_statistics":phases.into_iter().enumerate().map(|(i,p)|p.report(i,cfa)).collect::<Vec<_>>(),
        "bayer_green_comparison":green_comparison}),
    )
}

#[test]
fn cfa_metrology_preserves_extended_samples_and_checks_geometry() {
    for cfa in [
        Cfa::bayer([0, 1, 1, 2]),
        Cfa::new(6, &crate::cfa::XTRANS).unwrap(),
    ] {
        let (w, h) = (24, 18);
        let colors = [-0.125, 0.5, 1.25];
        let values: Vec<_> = (0..w * h)
            .map(|i| colors[cfa.color(i % w, i / w)])
            .collect();
        let r = patch(&values, w, h, cfa, [3, 2, 15, 12]).unwrap();
        assert_eq!(r["camera_rgb"], json!(colors));
        assert_eq!(r["sample_variance"], json!([0., 0., 0.]));
        assert_eq!(r["fit_eligible_no_saturation"], false);
        assert!(r["negative_sites"][0].as_u64().unwrap() > 0);
        for rect in [
            [0, 0, 1, 1],
            [23, 0, 2, 18],
            [usize::MAX, 0, 2, 2],
            [0, 0, 0, 4],
        ] {
            assert!(patch(&values, w, h, cfa, rect).is_err());
        }
    }
}

#[test]
fn cfa_patch_moments_and_isolated_saturation_match_direct_samples() {
    let (w, h) = (25, 19);
    for base in [
        Cfa::bayer([0, 1, 1, 2]),
        Cfa::new(6, &crate::cfa::XTRANS).unwrap(),
    ] {
        for phase in 0..base.period * base.period {
            let cfa = base.shifted(phase % base.period, phase / base.period);
            let mut values: Vec<f32> = (0..w * h).map(|i| (i % 73) as f32 / 100. - 0.1).collect();
            values[5 * w + 7] = 1.;
            let report = patch(&values, w, h, cfa, [3, 2, 18, 15]).unwrap();
            for c in 0..3 {
                let mut direct = vec![];
                for y in 2..17 {
                    for x in 3..21 {
                        if cfa.color(x, y) == c {
                            direct.push(f64::from(values[y * w + x]));
                        }
                    }
                }
                let mean = direct.iter().sum::<f64>() / direct.len() as f64;
                let variance = direct.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                    / (direct.len() - 1) as f64;
                assert_eq!(report["sample_count"][c], direct.len());
                assert!((report["camera_rgb"][c].as_f64().unwrap() - mean).abs() < 1e-14);
                assert!((report["sample_variance"][c].as_f64().unwrap() - variance).abs() < 1e-14);
                assert_eq!(
                    report["maximum"][c].as_f64().unwrap(),
                    direct.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                );
                assert_eq!(
                    report["minimum"][c].as_f64().unwrap(),
                    direct.iter().copied().fold(f64::INFINITY, f64::min)
                );
                assert_eq!(
                    report["negative_sites"][c],
                    direct.iter().filter(|&&v| v < 0.).count()
                );
                assert_eq!(
                    report["saturated_sites"][c],
                    direct.iter().filter(|&&v| v >= 0.985).count()
                );
                assert!(mean < 0.985);
            }
            assert_eq!(report["fit_eligible_no_saturation"], false);
            for (index, moment) in report["phase_statistics"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                let mut direct = vec![];
                for y in 2..17 {
                    for x in 3..21 {
                        if (y % cfa.period) * cfa.period + x % cfa.period == index {
                            direct.push(f64::from(values[y * w + x]));
                        }
                    }
                }
                let mean = direct.iter().sum::<f64>() / direct.len() as f64;
                let variance = direct.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                    / (direct.len() - 1) as f64;
                assert_eq!(
                    moment["phase_xy"],
                    json!([index % cfa.period, index / cfa.period])
                );
                assert_eq!(moment["color"], cfa.map[index]);
                assert_eq!(moment["sample_count"], direct.len());
                assert!((moment["mean"].as_f64().unwrap() - mean).abs() < 1e-14);
                assert!((moment["sample_variance"].as_f64().unwrap() - variance).abs() < 1e-14);
                assert_eq!(
                    moment["minimum"].as_f64().unwrap(),
                    direct.iter().copied().fold(f64::INFINITY, f64::min)
                );
                assert_eq!(
                    moment["maximum"].as_f64().unwrap(),
                    direct.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                );
                assert_eq!(
                    moment["negative_sites"],
                    direct.iter().filter(|&&v| v < 0.).count()
                );
                assert_eq!(
                    moment["saturated_sites"],
                    direct.iter().filter(|&&v| v >= 0.985).count()
                );
            }
        }
    }
}

#[test]
fn bayer_green_means_remain_distinguishable_when_their_pooled_mean_is_neutral() {
    for phase in 0..4 {
        let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
        for (gr, gb) in [(0.25, 0.75), (-0.125, 0.5)] {
            let values: Vec<f32> = (0..24 * 18)
                .map(|i| {
                    let (x, y) = (i % 24, i / 24);
                    if cfa.color(x, y) != 1 {
                        0.5
                    } else if cfa.color(x ^ 1, y) == 0 {
                        gr
                    } else {
                        gb
                    }
                })
                .collect();
            let report = patch(&values, 24, 18, cfa, [3, 5, 16, 10]).unwrap();
            assert!(
                (report["camera_rgb"][1].as_f64().unwrap() - f64::from((gr + gb) / 2.)).abs()
                    < 1e-14
            );
            let comparison = &report["bayer_green_comparison"];
            for (key, neighbor) in [("green_red_row", 0), ("green_blue_row", 2)] {
                let xy = comparison[key].as_array().unwrap();
                let (x, y) = (
                    xy[0].as_u64().unwrap() as usize,
                    xy[1].as_u64().unwrap() as usize,
                );
                assert_eq!(cfa.color(x, y), 1);
                assert_eq!(cfa.color(x ^ 1, y), neighbor);
            }
            if gr > 0. {
                assert_eq!(comparison["relative_mean_difference"], -1.);
                assert!(
                    report["camera_rgb"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|v| (v.as_f64().unwrap() - 0.5).abs() < 1e-14)
                );
            } else {
                assert!(comparison["relative_mean_difference"].is_null());
            }
        }
    }
}

#[test]
fn small_xtrans_regions_do_not_invent_phase_means_or_variances() {
    let cfa = Cfa::new(6, &crate::cfa::XTRANS).unwrap();
    let report = patch(&vec![0.5; 8 * 7], 8, 7, cfa, [1, 2, 6, 3]).unwrap();
    assert!(report["bayer_green_comparison"].is_null());
    let mut empty = 0;
    for phase in report["phase_statistics"].as_array().unwrap() {
        assert!(phase["sample_variance"].is_null());
        if phase["sample_count"] == 0 {
            empty += 1;
            for key in ["mean", "minimum", "maximum"] {
                assert!(phase[key].is_null());
            }
        } else {
            assert_eq!(phase["sample_count"], 1);
            assert_eq!(phase["mean"], 0.5);
        }
    }
    assert_eq!(empty, 18);
}

#[test]
#[ignore = "offline research campaign; needs TR_COLOR_PATCH_MANIFEST and TR_COLOR_PATCH_OUTPUT"]
fn sensor_patch_research_campaign() -> Result<()> {
    let config: Value = serde_json::from_slice(&fs::read(PathBuf::from(std::env::var(
        "TR_COLOR_PATCH_MANIFEST",
    )?))?)?;
    let source = config["source"].as_str().context("source")?;
    ensure!(
        fs::metadata(source)?.len() <= crate::MAX_SOURCE_BYTES as u64,
        "source size"
    );
    let bytes = fs::read(source)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    ensure!(config["source_sha256"] == hash, "source hash");
    let raw = Raw::parse(&bytes)?;
    let info = raw.info()?;
    let mosaic = match raw {
        Raw::Dng(raw) => raw.mosaic()?,
        Raw::Nef(raw) => raw.mosaic()?,
        Raw::Raf(raw) => raw.mosaic()?,
    };
    let regions = config["patches"].as_array().context("patches")?;
    ensure!(!regions.is_empty() && regions.len() <= 4096, "patch count");
    let mut patches = vec![];
    let mut ids = std::collections::HashSet::new();
    for region in regions {
        let id = region["id"].as_str().context("patch id")?;
        ensure!(
            !id.is_empty() && ids.insert(id),
            "duplicate or empty patch id"
        );
        let rect: [usize; 4] = serde_json::from_value(region["rect_sensor"].clone())?;
        let mut measurement = patch(
            &mosaic.values,
            mosaic.width,
            mosaic.height,
            mosaic.cfa,
            rect,
        )?;
        measurement["id"] = json!(id);
        patches.push(measurement);
    }
    ensure!(
        format!("{:x}", Sha256::digest(fs::read(source)?)) == hash,
        "source changed"
    );
    let report = json!({"schema":2,"source_sha256":hash,"camera":info.camera,
        "recipe":crate::RECIPE,"sensor_size":[mosaic.width,mosaic.height],"crop":mosaic.crop,
        "orientation":mosaic.orientation,"cfa_period":mosaic.cfa.period,
        "cfa_pattern":&mosaic.cfa.map[..mosaic.cfa.period*mosaic.cfa.period],"patches":patches,
        "domain":"black/white calibrated CFA, before WB; sensor coordinates before crop/orientation",
        "limits":"Sample variance is not total measurement uncertainty. Phase/green differences may come from scene structure, shading or noise; no sensor correction is inferred. No target/reference association or camera qualification is inferred."});
    let output = PathBuf::from(std::env::var("TR_COLOR_PATCH_OUTPUT")?);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

#[test]
#[ignore = "offline WB precision research campaign; needs TR_COLOR_WB_MANIFEST and TR_COLOR_WB_OUTPUT"]
fn wb_precision_research_campaign() -> Result<()> {
    let config: Value = serde_json::from_slice(&fs::read(std::env::var("TR_COLOR_WB_MANIFEST")?)?)?;
    let mut cameras = vec![];
    let sources = config["sources"].as_array().context("sources")?;
    ensure!(!sources.is_empty(), "empty WB sources");
    for entry in sources {
        let source = entry["source"].as_str().context("source")?;
        ensure!(
            fs::metadata(source)?.len() <= crate::MAX_SOURCE_BYTES as u64,
            "source size"
        );
        let bytes = fs::read(source)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        ensure!(entry["sha256"] == digest, "source hash");
        let raw = Raw::parse(&bytes)?;
        let info = raw.info()?;
        let mosaic = match raw {
            Raw::Dng(r) => r.mosaic()?,
            Raw::Nef(r) => r.mosaic()?,
            Raw::Raf(r) => r.mosaic()?,
        };
        let calibration = mosaic.calibration;
        let grid: [f64; 9] = [
            0.25049049, 0.33349049, 0.50049049, 0.75049049, 1.00049049, 1.33349049, 1.75049049,
            2.50049049, 3.99949049,
        ];
        let mut cases = vec![];
        let mut rejected = vec![];
        for red in grid {
            for blue in grid {
                let exact = [red, 1., blue];
                let quantized = legacy_wb_gains(exact);
                let precise =
                    tr_core::decoder::RawWhiteBalance::precise_sensor_gains(exact)?.gains_f64();
                let triple = (
                    calibration.resolve(exact),
                    calibration.resolve(quantized),
                    calibration.resolve(precise),
                );
                let ((g, m), (qg, qm), (pg, pm)) = match triple {
                    (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                    (a, b, c) => {
                        rejected.push(json!({"relative":exact,"continuous_error":a.err().map(|e|e.to_string()),
                        "quantized_error":b.err().map(|e|e.to_string()),"precise_error":c.err().map(|e|e.to_string())}));
                        continue;
                    }
                };
                for rgb in [
                    [0.18; 3],
                    [0.4, 0.2, 0.1],
                    [0.1, 0.4, 0.2],
                    [0.1, 0.2, 0.4],
                    [0.6, 0.3, 0.15],
                    [0.08, 0.04, 0.02],
                    [-0.01, 0.02, 0.03],
                    [1.2, 0.8, 0.5],
                ] {
                    let camera: [f32; 3] = std::array::from_fn(|c| {
                        (rgb[c] * calibration.neutral[c] / calibration.neutral[1] / exact[c]) as f32
                    });
                    let apply = |g: [f32; 3], m: crate::color::Matrix| {
                        let wb: [f32; 3] = std::array::from_fn(|c| camera[c] * g[c]);
                        m.map(|r| {
                            let r = r.map(|v| v as f32);
                            r[0] * wb[0] + r[1] * wb[1] + r[2] * wb[2]
                        })
                    };
                    let ideal = apply(g, m);
                    let actual = apply(qg, qm);
                    let precise_output = apply(pg, pm);
                    let f64_projection = crate::color::vector(
                        m,
                        std::array::from_fn(|c| f64::from(camera[c]) * f64::from(g[c])),
                    );
                    ensure!(
                        ideal
                            .into_iter()
                            .chain(actual)
                            .chain(precise_output)
                            .all(f32::is_finite),
                        "nonfinite WB audit"
                    );
                    cases.push(json!({"relative":exact,"quantized":quantized,"camera_rgb":camera,
                        "continuous_rec2020":ideal,"quantized_rec2020":actual,"precise_rec2020":precise_output,"precise_gains":precise,"f64_projection":f64_projection,
                        "unsaturated_positive_camera":camera.iter().all(|&v|(0.0..0.985).contains(&v))}));
                }
            }
        }
        cameras.push(
            json!({"camera":info.camera,"source_sha256":digest,"profile_hash":info.profile_hash,
            "cases":cases,"rejected_wb_pairs":rejected}),
        );
    }
    fs::OpenOptions::new().write(true).create_new(true).open(std::env::var("TR_COLOR_WB_OUTPUT")?)?
        .write_all(&serde_json::to_vec_pretty(&json!({"recipe":crate::RECIPE,"cameras":cameras,
            "schema":2,"legacy_gain_interpretation":"integer thousandths divided in fp32, then promoted to f64",
            "limits":"Synthetic constant camera colors and nine-by-nine near-half-step gain grid. Colors compared only when all three WB interpretations are accepted; other triplets recorded separately. WB/float precision only, not camera color accuracy or demosaic commutation."}))?)?;
    Ok(())
}
