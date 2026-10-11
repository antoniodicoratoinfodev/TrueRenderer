use super::{hash, xtrans_e8};
use crate::{
    cfa::{Cfa, XTRANS},
    xtrans,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, f64::consts::TAU, fs, path::PathBuf};

#[path = "directional.rs"]
mod directional;

#[test]
fn camera_rgb_refactor_is_bit_exact_with_frozen_e8() {
    let (w, h) = (29, 23);
    let mosaic: Vec<f32> = (0..w * h)
        .map(|i| ((i * 127 + 83) % 997) as f32 / 431. - 0.17)
        .collect();
    let matrix = [
        [1.13, -0.22, 0.034],
        [-0.27, 1.21, 0.08],
        [0.09, -0.03, 0.97],
    ];
    for phase in 0..36 {
        let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
        let camera = xtrans::CameraRgb::new(&mosaic, w, h, cfa);
        for orientation in 1..=8 {
            for crop in [[0, 0, w, h], [2, 3, 19, 17]] {
                let a = xtrans_e8::develop(&mosaic, w, h, cfa, matrix, crop, orientation).unwrap();
                let b = xtrans::develop(&mosaic, w, h, cfa, matrix, crop, orientation).unwrap();
                assert_eq!((a.width, a.height), (b.width, b.height));
                for (a, b) in a.pixels.iter().zip(&b.pixels) {
                    assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let c = cfa.color(x, y);
                assert_eq!(camera.at(x, y)[c].to_bits(), mosaic[y * w + x].to_bits());
            }
        }
    }
}

#[test]
#[ignore = "offline research campaign; needs TR_FUJI_LAB_MANIFEST"]
fn photographic_refactor_research_campaign() -> Result<()> {
    let manifest = PathBuf::from(std::env::var("TR_FUJI_LAB_MANIFEST")?);
    let config: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    let bytes = fs::read(config["source"].as_str().context("source")?)?;
    ensure!(hash(&bytes) == config["source_sha256"], "source hash");
    let raw = crate::raf::Raf::parse(&bytes)?;
    let mut cases = vec![];
    for relative in [[1., 1., 1.], [1.2, 1., 0.8]] {
        let mut mosaic = raw.mosaic()?;
        let (gains, matrix) = mosaic.calibration.resolve(relative)?;
        for (i, v) in mosaic.values.iter_mut().enumerate() {
            *v *= gains[mosaic.cfa.color(i % mosaic.width, i / mosaic.width)];
        }
        let a = xtrans_e8::develop(
            &mosaic.values,
            mosaic.width,
            mosaic.height,
            mosaic.cfa,
            matrix,
            mosaic.crop,
            mosaic.orientation,
        )?;
        let b = xtrans::develop(
            &mosaic.values,
            mosaic.width,
            mosaic.height,
            mosaic.cfa,
            matrix,
            mosaic.crop,
            mosaic.orientation,
        )?;
        ensure!((a.width, a.height) == (b.width, b.height), "dimensions");
        for (a, b) in a.pixels.iter().zip(&b.pixels) {
            ensure!(
                a.map(f32::to_bits) == b.map(f32::to_bits),
                "photographic refactor changed a float bit"
            );
        }
        cases.push(json!({"relative_wb":relative,"width":a.width,"height":a.height,"rgba_float_components_compared":a.pixels.len()*4,"bit_mismatches":0}));
    }
    fs::write(
        manifest.with_file_name("photographic-refactor.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":true,"source_sha256":hash(&bytes),"reference":"frozen E8 xtrans source","cases":cases}),
        )?,
    )?;
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct Metric {
    sse: f64,
    count: u64,
}
impl Metric {
    fn add(&mut self, error: f64) {
        self.sse += error * error;
        self.count += 1;
    }
    fn merge(&mut self, other: Self) {
        self.sse += other.sse;
        self.count += other.count;
    }
    fn rmse(self) -> f64 {
        if self.count == 0 {
            0.
        } else {
            (self.sse / self.count as f64).sqrt()
        }
    }
    fn value(self) -> Value {
        json!({"sse":self.sse,"count":self.count,"rmse":self.rmse()})
    }
}
#[derive(Default, Clone)]
struct Metrics {
    missing: Metric,
    chroma: Metric,
    luma: Metric,
    gradient: Metric,
    border: Metric,
    measured: u64,
    mismatches: u64,
    nonfinite: u64,
}
impl Metrics {
    fn merge(&mut self, v: &Self) {
        self.missing.merge(v.missing);
        self.chroma.merge(v.chroma);
        self.luma.merge(v.luma);
        self.gradient.merge(v.gradient);
        self.border.merge(v.border);
        self.measured += v.measured;
        self.mismatches += v.mismatches;
        self.nonfinite += v.nonfinite;
    }
    fn value(&self) -> Value {
        json!({"missing_channel_rmse":self.missing.value(),"false_chroma_rmse":self.chroma.value(),
        "luma_rmse":self.luma.value(),"luma_gradient_rmse":self.gradient.value(),"border_rgb_rmse":self.border.value(),
        "measured_sites":self.measured,"measured_mismatches":self.mismatches,"nonfinite_values":self.nonfinite})
    }
}

struct Scene {
    category: &'static str,
    name: String,
    truth: Vec<[f32; 3]>,
    sigma: f64,
}
fn scenes(protocol: &Value, split: &str, w: usize, h: usize) -> Vec<Scene> {
    let q = &protocol["quality"];
    let mut out = vec![];
    for angle in q[split]["angles_degrees"].as_array().unwrap() {
        let angle = angle.as_f64().unwrap();
        let radians = angle.to_radians();
        let coordinate = |x: usize, y: usize| {
            (x as f64 - w as f64 / 2.) * radians.cos() + (y as f64 - h as f64 / 2.) * radians.sin()
        };
        for frequency in q[split]["frequencies"].as_array().unwrap() {
            let frequency = frequency.as_f64().unwrap();
            for chromatic in [false, true] {
                let category = if chromatic {
                    "chromatic_texture"
                } else {
                    "achromatic_texture"
                };
                let truth = (0..w * h)
                    .map(|i| {
                        let t = TAU * frequency * coordinate(i % w, i / w);
                        if chromatic {
                            [
                                (0.45 + 0.25 * t.sin()) as f32,
                                (0.4 + 0.19 * (t + 0.9).sin()) as f32,
                                (0.5 + 0.22 * (t - 0.7).sin()) as f32,
                            ]
                        } else {
                            [(0.45 + 0.3 * t.sin()) as f32; 3]
                        }
                    })
                    .collect();
                out.push(Scene {
                    category,
                    name: format!("{category}-{angle}-{frequency}"),
                    truth,
                    sigma: 0.,
                });
            }
        }
        for chromatic in [false, true] {
            let truth = (0..w * h)
                .map(|i| {
                    // Linear pixel-integrated transition, fixed width one pixel.
                    let v = (coordinate(i % w, i / w) + 0.5).clamp(0., 1.) as f32;
                    if chromatic {
                        [0.13 + 0.61 * v, 0.69 - 0.45 * v, 0.22 + 0.25 * v]
                    } else {
                        [0.13 + 0.61 * v; 3]
                    }
                })
                .collect();
            out.push(Scene {
                category: "edge",
                name: format!("edge-{angle}-{chromatic}"),
                truth,
                sigma: 0.,
            });
        }
    }
    for (id, rgb) in [[-0.1, 0.37, 1.4], [0., 0., 0.], [1.4; 3]]
        .into_iter()
        .enumerate()
    {
        out.push(Scene {
            category: "flat_extended",
            name: format!("flat-{id}"),
            truth: vec![rgb; w * h],
            sigma: 0.,
        });
    }
    for sigma in q["noise_sigma"].as_array().unwrap() {
        let sigma = sigma.as_f64().unwrap();
        for (id, rgb) in [[0.37; 3], [0.21, 0.43, 0.68]].into_iter().enumerate() {
            out.push(Scene {
                category: "noise",
                name: format!("noise-{sigma}-{id}"),
                truth: vec![rgb; w * h],
                sigma,
            });
        }
    }
    out
}
fn random(seed: &mut u32) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (f64::from(*seed) + 0.5) / (f64::from(u32::MAX) + 1.)
}
fn evaluate(
    image: &[[f32; 3]],
    scene: &Scene,
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    border: usize,
) -> Metrics {
    let mut m = Metrics::default();
    let luma = |v: [f32; 3]| (f64::from(v[0]) + f64::from(v[1]) + f64::from(v[2])) / 3.;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let p = image[i];
            let truth = scene.truth[i];
            let measured = cfa.color(x, y);
            m.measured += 1;
            m.mismatches += u64::from(p[measured].to_bits() != mosaic[i].to_bits());
            m.nonfinite += p.iter().filter(|v| !v.is_finite()).count() as u64;
            if x < border || y < border || x >= w - border || y >= h - border {
                for c in 0..3 {
                    m.border.add(f64::from(p[c]) - f64::from(truth[c]));
                }
            } else {
                for c in 0..3 {
                    if c != measured {
                        m.missing.add(f64::from(p[c]) - f64::from(truth[c]));
                    }
                }
                m.luma.add(luma(p) - luma(truth));
                for k in [i + 1, i + w] {
                    m.gradient
                        .add((luma(image[k]) - luma(p)) - (luma(scene.truth[k]) - luma(truth)));
                }
                if scene.category == "achromatic_texture" {
                    m.chroma.add(f64::from(p[0]) - f64::from(p[1]));
                    m.chroma.add(f64::from(p[2]) - f64::from(p[1]));
                }
            }
        }
    }
    m
}

#[test]
#[ignore = "offline research campaign; needs TR_QUALITY_LAB_ROOT and TR_QUALITY_SPLIT"]
fn quality_research_campaign() -> Result<()> {
    let root = PathBuf::from(
        std::env::var("TR_QUALITY_LAB_ROOT").context("TR_QUALITY_LAB_ROOT required")?,
    );
    let split = std::env::var("TR_QUALITY_SPLIT").context("TR_QUALITY_SPLIT required")?;
    ensure!(
        matches!(split.as_str(), "development" | "validation"),
        "invalid split"
    );
    let text = include_str!("../tests/fixtures/research-v1.json");
    let protocol: Value = serde_json::from_str(text)?;
    let q = &protocol["quality"];
    let w = q["width"].as_u64().unwrap() as usize;
    let h = q["height"].as_u64().unwrap() as usize;
    let border = q["border"].as_u64().unwrap() as usize;
    let cases = scenes(&protocol, &split, w, h);
    let mut details = vec![];
    let mut totals: BTreeMap<&str, [Metrics; 2]> = BTreeMap::new();
    let mut all_passed = true;
    let mut worst_ratios: BTreeMap<String, f64> = BTreeMap::new();
    for phase in 0..36 {
        let cfa = Cfa::new(6, &XTRANS)?.shifted(phase % 6, phase / 6);
        let mut per_phase: BTreeMap<&str, [Metrics; 2]> = BTreeMap::new();
        for (case, scene) in cases.iter().enumerate() {
            let mut seed = q[&split]["seed"].as_u64().unwrap() as u32
                ^ (case as u32 * 1877 + phase as u32 * 8191);
            let mosaic: Vec<_> = scene
                .truth
                .iter()
                .enumerate()
                .map(|(i, rgb)| {
                    // Box-Muller from a fixed PRNG; identical source for both methods.
                    let noise = (-2. * random(&mut seed).ln()).sqrt()
                        * (TAU * random(&mut seed)).cos()
                        * scene.sigma;
                    (f64::from(rgb[cfa.color(i % w, i / w)]) + noise) as f32
                })
                .collect();
            let base = xtrans::CameraRgb::new(&mosaic, w, h, cfa);
            let a: Vec<_> = (0..w * h).map(|i| base.at(i % w, i / w)).collect();
            let b = directional::reconstruct(&mosaic, w, h, cfa);
            let metrics = [
                evaluate(&a, scene, &mosaic, w, h, cfa, border),
                evaluate(&b, scene, &mosaic, w, h, cfa, border),
            ];
            for (i, m) in metrics.iter().enumerate() {
                ensure!(
                    m.mismatches == 0 && m.nonfinite == 0,
                    "{} phase {phase}: measured/nonfinite",
                    scene.name
                );
                per_phase.entry(scene.category).or_default()[i].merge(m);
                totals.entry(scene.category).or_default()[i].merge(m);
            }
        }
        for (category, pair) in &per_phase {
            let gate = gate(pair, category, q);
            all_passed &= gate["passed"].as_bool().unwrap();
            for (name, ratio) in gate["ratios"].as_object().unwrap() {
                let key = format!("{category}/{name}");
                let value = ratio.as_f64().unwrap();
                worst_ratios
                    .entry(key)
                    .and_modify(|old| *old = old.max(value))
                    .or_insert(value);
            }
            details.push(json!({"phase":phase,"category":category,"baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate}));
        }
    }
    let categories:Vec<_>=totals.iter().map(|(category,pair)|json!({"category":category,"baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate(pair,category,q)})).collect();
    let report = json!({"protocol":"TRExp-research-v1","protocol_sha256":hash(text.as_bytes()),"candidate":"DR1","split":split,
        "scenes":cases.iter().map(|s|&s.name).collect::<Vec<_>>(),"scene_phase_cases":cases.len()*36,"width":w,"height":h,
        "synthetic_gates_passed":all_passed,"production_promoted":false,"photographic_ground_truth_available":false,
        "categories":categories,"worst_phase_ratios":worst_ratios,"per_phase":details});
    fs::write(
        root.join(format!("quality-{split}.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "{split}: {} cases, synthetic gates={all_passed}",
        cases.len() * 36
    );
    Ok(())
}
fn gate(pair: &[Metrics; 2], category: &str, q: &Value) -> Value {
    let floor = q["absolute_rmse_floor"].as_f64().unwrap();
    let limit = q["other_category_and_detail_rmse_max_ratio"]
        .as_f64()
        .unwrap();
    let ratio = |a: Metric, b: Metric| {
        if b.rmse() <= floor && a.rmse() <= floor {
            1.
        } else {
            b.rmse() / a.rmse().max(floor)
        }
    };
    let mut ratios = BTreeMap::from([
        ("missing", ratio(pair[0].missing, pair[1].missing)),
        ("luma", ratio(pair[0].luma, pair[1].luma)),
        ("gradient", ratio(pair[0].gradient, pair[1].gradient)),
        ("border", ratio(pair[0].border, pair[1].border)),
    ]);
    let mut passed = ratios.values().all(|v| *v <= limit);
    if category == "achromatic_texture" {
        let chroma = ratio(pair[0].chroma, pair[1].chroma);
        passed &= chroma <= q["false_chroma_rmse_max_ratio"].as_f64().unwrap();
        ratios.insert("chroma", chroma);
    }
    json!({"passed":passed,"ratios":ratios})
}
