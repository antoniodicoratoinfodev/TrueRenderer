//! Deterministic sensor-space WB experiment. No measured camera/target claim.
use super::{Mosaic, tests::mosaic};
use crate::cfa::{Cfa, XTRANS};
use anyhow::{Result, ensure};
use serde_json::json;

fn previous_region(raw: &Mosaic) -> Option<[f64; 2]> {
    let mut sites: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
    let mut total = [0; 3];
    for (i, v) in raw.values.iter().enumerate() {
        let c = raw.cfa.color(i % raw.width, i / raw.width);
        total[c] += 1;
        let v = f64::from(*v);
        if v.is_finite() && (0.003..0.985).contains(&v) {
            sites[c].push(v);
        }
    }
    if (0..3).any(|c| sites[c].len() < 3 || sites[c].len() * 4 < total[c] * 3) {
        return None;
    }
    let means = sites.map(|mut v| {
        v.sort_by(f64::total_cmp);
        let trim = v.len() / 5;
        v[trim..v.len() - trim].iter().sum::<f64>() / (v.len() - 2 * trim) as f64
    });
    Some([means[0] / means[1], means[2] / means[1]])
}

#[derive(Default)]
struct Stats {
    errors: Vec<f64>,
    rejected: usize,
}
impl Stats {
    fn add(&mut self, ratios: Option<[f64; 2]>) {
        if let Some(ratios) = ratios {
            for (ratio, neutral) in ratios.into_iter().zip([0.5, 0.75]) {
                self.errors.push(neutral / ratio - 1.);
            }
        } else {
            self.rejected += 1;
        }
    }
    fn report(&mut self) -> serde_json::Value {
        let count = self.errors.len();
        if count == 0 {
            return json!({"accepted_regions":0,"rejected_regions":self.rejected});
        }
        let bias = self.errors.iter().sum::<f64>() / count as f64;
        let rms = (self.errors.iter().map(|e| e * e).sum::<f64>() / count as f64).sqrt();
        self.errors.iter_mut().for_each(|e| *e = e.abs());
        self.errors.sort_by(f64::total_cmp);
        json!({"accepted_regions":count/2,"rejected_regions":self.rejected,
            "relative_gain_bias":bias,"relative_gain_rms":rms,
            "absolute_relative_gain_p95":self.errors[(95*count).div_ceil(100)-1],
            "absolute_relative_gain_max":self.errors[count-1]})
    }
}

fn normal(state: &mut u64) -> f64 {
    let mut uniform = || {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*state >> 11) as f64 + 0.5) / ((1_u64 << 53) as f64)
    };
    (-2. * uniform().ln()).sqrt() * (std::f64::consts::TAU * uniform()).cos()
}

#[test]
#[ignore = "explicit synthetic WB precision campaign; TR_WB_SPATIAL_OUTPUT must be new"]
fn wb_spatial_research_campaign() -> Result<()> {
    let validation = std::env::var("TR_WB_SPATIAL_VALIDATION").is_ok_and(|v| v == "1");
    let repetitions = if validation { 48 } else { 32 };
    let output = std::path::PathBuf::from(std::env::var("TR_WB_SPATIAL_OUTPUT")?);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut results = Vec::new();
    let mut cases = 0;
    for cfa in [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS)?] {
        for size in [5, 11, 12] {
            for scene in ["flat", "plane", "curved", "fine_texture"] {
                for disturbance in ["none", "noise", "one_outlier", "sensor_rejection"] {
                    let mut before = Stats::default();
                    let mut before_paired = Stats::default();
                    let mut after = Stats::default();
                    for dy in 0..cfa.period {
                        for dx in 0..cfa.period {
                            for repetition in 0..repetitions {
                                let mut raw = mosaic(cfa.shifted(dx, dy), size, size);
                                let mut state = if validation { 932_813 } else { 73_013 }
                                    + repetition * 7937
                                    + (dx + dy * cfa.period) as u64 * 1693;
                                let angle = std::f64::consts::TAU * repetition as f64
                                    / repetitions as f64
                                    + if validation { 0.37 } else { 0. };
                                for y in 0..size {
                                    for x in 0..size {
                                        let (xx, yy) = (
                                            (x as f64 - (size - 1) as f64 / 2.) / size as f64,
                                            (y as f64 - (size - 1) as f64 / 2.) / size as f64,
                                        );
                                        let light = match scene {
                                            "flat" => 0.4,
                                            "plane" => {
                                                0.4 + 0.35 * (xx * angle.cos() + yy * angle.sin())
                                            }
                                            "curved" => {
                                                0.4 + 0.25 * (xx * angle.cos() + yy * angle.sin())
                                                    + 0.2 * (xx * xx + yy * yy)
                                            }
                                            _ => {
                                                0.4 + 0.08
                                                    * (1.7 * x as f64 + 0.9 * y as f64 + angle)
                                                        .sin()
                                            }
                                        };
                                        let signal = light * [0.5, 1., 0.75][raw.cfa.color(x, y)];
                                        let noise = if disturbance == "noise" {
                                            ((if validation { 4e-6 } else { 1e-6 }) + 9e-6 * signal)
                                                .sqrt()
                                                * normal(&mut state)
                                        } else {
                                            0.
                                        };
                                        raw.values[y * size + x] = (signal + noise) as f32;
                                    }
                                }
                                let at = (repetition as usize * 17
                                    + if validation { 7 } else { 0 })
                                    % raw.values.len();
                                if disturbance == "one_outlier" {
                                    raw.values[at] = 0.9;
                                }
                                if disturbance == "sensor_rejection" {
                                    raw.values[at] = 1.;
                                }
                                let old = previous_region(&raw);
                                let new = raw.region([0, 0, size, size]).map(|r| r.map(f64::exp));
                                before.add(old);
                                before_paired.add(new.and(old));
                                after.add(new);
                                cases += 1;
                            }
                        }
                    }
                    let before = before.report();
                    let after = after.report();
                    if matches!(scene, "flat" | "plane") && disturbance == "none" {
                        ensure!(
                            after["absolute_relative_gain_max"].as_f64().unwrap() < 3e-7,
                            "affine illumination invariance failed"
                        );
                        ensure!(
                            after["rejected_regions"] == 0,
                            "unexpected affine rejection"
                        );
                    }
                    results.push(json!({"period":cfa.period,"size":size,"scene":scene,
                        "disturbance":disturbance,"before":before,"before_paired":before_paired.report(),"after":after}));
                }
            }
        }
    }
    serde_json::to_writer_pretty(
        &mut file,
        &json!({"protocol":"TRExp-CFA-WB-spatial-v2","validation":validation,
        "cases":cases,"repetitions_per_phase":repetitions,"results":results,
        "signal":"Common achromatic illumination times sensor neutral [0.5,1,0.75]; fp32 input",
        "noise":"Deterministic Gaussian, variance read_variance + 9e-6 * signal; not camera-characterized",
        "read_variance":if validation {4e-6} else {1e-6},
        "metric":"Relative error of recovered R/B gain, before integer gain quantization",
        "limits":"Synthetic sampling/noise study only. Curved and fine-texture scenes deliberately violate the affine model. Not a target measurement, illuminant estimate or camera profile qualification."}),
    )?;
    Ok(())
}
