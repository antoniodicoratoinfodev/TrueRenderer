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

#[path = "residual.rs"]
mod residual;

#[path = "chroma.rs"]
mod chroma;

#[path = "confidence.rs"]
mod confidence;

#[path = "photographic.rs"]
mod photographic;

#[path = "frame.rs"]
mod frame;

#[path = "refined.rs"]
mod refined;

#[path = "bounded.rs"]
mod bounded;

#[path = "bounded_constrained.rs"]
mod bounded_constrained;
#[path = "constrained.rs"]
mod constrained;
#[path = "residual_constrained.rs"]
mod residual_constrained;
#[path = "residual_refined.rs"]
mod residual_refined;

use crate::xtrans_dr6::bounded as bounded_fast;
#[path = "bayer.rs"]
mod bayer;
#[path = "bayer_guarded.rs"]
mod bayer_guarded;
#[path = "bayer_guided.rs"]
mod bayer_guided;
#[path = "frame_fast.rs"]
mod frame_fast;

#[test]
fn integrated_dr6_matches_frozen_rgb_through_matrix_crop_and_all_orientations() {
    let (w, h) = (41, 35);
    let matrix = [
        [1.13, -0.22, 0.034],
        [-0.27, 1.21, 0.08],
        [0.09, -0.03, 0.97],
    ];
    for phase in 0..36 {
        let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
        for gains in [[1., 1., 1.], [1.2, 1., 0.8]] {
            let mosaic: Vec<_> = (0..w * h)
                .map(|i| {
                    let x = (i % w) as f32;
                    let y = (i / w) as f32;
                    (-0.2 + 0.8 * (x * 0.2 + y * 0.1).sin() + cfa.color(i % w, i / w) as f32 * 0.5)
                        * gains[cfa.color(i % w, i / w)]
                })
                .collect();
            let rgb = constrained::reconstruct(&mosaic, w, h, cfa);
            let m = matrix.map(|r| r.map(|v| v as f32));
            for [left, top, cw, ch] in [[0, 0, w, h], [3, 2, 29, 27]] {
                for orientation in 1..=8 {
                    let image = xtrans::develop(
                        &mosaic,
                        w,
                        h,
                        cfa,
                        matrix,
                        [left, top, cw, ch],
                        orientation,
                    )
                    .unwrap();
                    let expected_size = if orientation < 5 { (cw, ch) } else { (ch, cw) };
                    assert_eq!((image.width as usize, image.height as usize), expected_size);
                    for y in 0..ch {
                        for x in 0..cw {
                            let (ox, oy) = match orientation {
                                1 => (x, y),
                                2 => (cw - 1 - x, y),
                                3 => (cw - 1 - x, ch - 1 - y),
                                4 => (x, ch - 1 - y),
                                5 => (y, x),
                                6 => (ch - 1 - y, x),
                                7 => (ch - 1 - y, cw - 1 - x),
                                8 => (y, cw - 1 - x),
                                _ => unreachable!(),
                            };
                            let p = rgb[(top + y) * w + left + x];
                            let expected = m.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2]);
                            let actual = image.pixels[oy * expected_size.0 + ox];
                            assert_eq!(actual[3], 1.);
                            for c in 0..3 {
                                assert!((actual[c] - expected[c]).abs() <= 4e-6);
                            }
                        }
                    }
                }
            }
        }
    }
}

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
                let b = xtrans::baseline_develop(&mosaic, w, h, cfa, matrix, crop, orientation)
                    .unwrap();
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
        let b = xtrans::baseline_develop(
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

#[derive(Clone)]
struct Scene {
    category: &'static str,
    name: String,
    truth: Vec<[f32; 3]>,
    sigma: f64,
    noise_correlation: f64,
}
fn scenes(protocol: &Value, split: &str, w: usize, h: usize) -> Vec<Scene> {
    let q = &protocol["quality"];
    let settings = &q[split];
    let wave_phase = settings["wave_phase_radians"].as_f64().unwrap_or(0.);
    let edge_offset = settings["edge_offset_pixels"].as_f64().unwrap_or(0.);
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
                        let t = TAU * frequency * coordinate(i % w, i / w) + wave_phase;
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
                    noise_correlation: 0.,
                });
            }
        }
        for chromatic in [false, true] {
            let truth = (0..w * h)
                .map(|i| {
                    // Linear pixel-integrated transition, fixed width one pixel.
                    let v = (coordinate(i % w, i / w) + 0.5 + edge_offset).clamp(0., 1.) as f32;
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
                noise_correlation: 0.,
            });
        }
    }
    let flats: [[f32; 3]; 3] = settings
        .get("flat_rgb")
        .map_or([[-0.1, 0.37, 1.4], [0.; 3], [1.4; 3]], |v| {
            serde_json::from_value(v.clone()).expect("flat_rgb")
        });
    let noise_colors: [[f32; 3]; 2] = settings
        .get("noise_rgb")
        .map_or([[0.37; 3], [0.21, 0.43, 0.68]], |v| {
            serde_json::from_value(v.clone()).expect("noise_rgb")
        });
    for (id, rgb) in flats.into_iter().enumerate() {
        out.push(Scene {
            category: "flat_extended",
            name: format!("flat-{id}"),
            truth: vec![rgb; w * h],
            sigma: 0.,
            noise_correlation: 0.,
        });
    }
    for sigma in q["noise_sigma"].as_array().unwrap() {
        let sigma = sigma.as_f64().unwrap();
        for (id, rgb) in noise_colors.into_iter().enumerate() {
            out.push(Scene {
                category: "noise",
                name: format!("noise-{sigma}-{id}"),
                truth: vec![rgb; w * h],
                sigma,
                noise_correlation: 0.,
            });
        }
    }
    if let Some(mixed) = q["mixed_noise"].as_array() {
        let structured: Vec<_> = out
            .iter()
            .filter(|s| {
                matches!(
                    s.category,
                    "achromatic_texture" | "chromatic_texture" | "edge"
                )
            })
            .cloned()
            .collect();
        for scene in &structured {
            for params in mixed {
                let scale = params["contrast_scale"].as_f64().unwrap() as f32;
                let sigma = params["sigma"].as_f64().unwrap();
                out.push(Scene {
                    category: match scene.category {
                        "achromatic_texture" => "noisy_achromatic_texture",
                        "chromatic_texture" => "noisy_chromatic_texture",
                        _ => "noisy_edge",
                    },
                    name: format!("noisy-{}-scale{scale}-sigma{sigma}", scene.name),
                    truth: scene
                        .truth
                        .iter()
                        .map(|p| p.map(|v| 0.4 + scale * (v - 0.4)))
                        .collect(),
                    sigma,
                    noise_correlation: 0.,
                });
            }
        }
    }
    if q["spatial_stress"].as_bool().unwrap_or(false) {
        for (id, frequency) in [
            q[split]["frequencies"][1].as_f64().unwrap(),
            q[split]["frequencies"][3].as_f64().unwrap(),
        ]
        .into_iter()
        .enumerate()
        {
            for chromatic in [false, true] {
                out.push(Scene {
                    category: "cross_texture",
                    name: format!("cross-{id}-{chromatic}"),
                    truth: (0..w * h)
                        .map(|i| {
                            let a = (TAU * frequency * (i % w) as f64 + wave_phase).sin() as f32;
                            let b = (TAU * frequency * (i / w) as f64 - wave_phase).cos() as f32;
                            if chromatic {
                                [
                                    0.4 + 0.16 * a + 0.13 * b,
                                    0.4 + 0.12 * a - 0.15 * b,
                                    0.4 - 0.17 * a + 0.11 * b,
                                ]
                            } else {
                                [0.4 + 0.16 * a + 0.13 * b; 3]
                            }
                        })
                        .collect(),
                    sigma: 0.,
                    noise_correlation: 0.,
                });
            }
        }
        for sigma in [0.012, 0.04] {
            for (id, rgb) in noise_colors.into_iter().enumerate() {
                out.push(Scene {
                    category: "correlated_noise",
                    name: format!("correlated-{sigma}-{id}"),
                    truth: vec![rgb; w * h],
                    sigma,
                    noise_correlation: 0.6,
                });
            }
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
    run_quality_campaign("DR1", include_str!("../tests/fixtures/research-v1.json"))
}

#[test]
#[ignore = "offline DR2 research campaign; needs TR_QUALITY_LAB_ROOT and TR_QUALITY_SPLIT"]
fn residual_quality_research_campaign() -> Result<()> {
    run_quality_campaign("DR2", include_str!("../tests/fixtures/research-v2.json"))
}

#[test]
#[ignore = "offline DR3 research campaign; needs TR_QUALITY_LAB_ROOT and TR_QUALITY_SPLIT"]
fn chroma_quality_research_campaign() -> Result<()> {
    run_quality_campaign("DR3", include_str!("../tests/fixtures/research-v3.json"))
}

#[test]
#[ignore = "offline DR4 research campaign; needs TR_QUALITY_LAB_ROOT and TR_QUALITY_SPLIT"]
fn confidence_quality_research_campaign() -> Result<()> {
    run_quality_campaign("DR4", include_str!("../tests/fixtures/research-v4.json"))
}

#[test]
#[ignore = "offline DR5 research campaign; needs TR_QUALITY_LAB_ROOT and TR_QUALITY_SPLIT"]
fn refined_quality_research_campaign() -> Result<()> {
    run_quality_campaign("DR5", include_str!("../tests/fixtures/research-v5.json"))
}

#[test]
#[ignore = "DR5 regression on consumed v4 data; not an independent research campaign"]
fn refined_previous_research_campaign() -> Result<()> {
    run_quality_campaign("DR5", include_str!("../tests/fixtures/research-v4.json"))
}

#[test]
#[ignore = "offline DR6 research campaign; needs TR_QUALITY_LAB_ROOT and TR_QUALITY_SPLIT"]
fn constrained_quality_research_campaign() -> Result<()> {
    run_quality_campaign("DR6", include_str!("../tests/fixtures/research-v6.json"))
}

#[test]
#[ignore = "DR6 regression on consumed v5 data, not independent validation"]
fn constrained_previous_research_campaign() -> Result<()> {
    run_quality_campaign("DR6", include_str!("../tests/fixtures/research-v5.json"))
}

#[test]
#[ignore = "Bayer BR6 regression on observed v3 signals"]
fn edge_planes_bayer_quality_research_campaign() -> Result<()> {
    run_quality_campaign(
        "BR6",
        include_str!("../tests/fixtures/research-br6-regression.json"),
    )
}

#[test]
#[ignore = "DR7 regression on observed v6 signals; not new validation"]
fn steps_quality_research_campaign() -> Result<()> {
    run_quality_campaign(
        "DR7",
        include_str!("../tests/fixtures/research-dr7-regression.json"),
    )
}

#[test]
#[ignore = "offline Bayer research campaign; needs TR_QUALITY_LAB_ROOT, TR_QUALITY_SPLIT and TR_BAYER_CANDIDATE"]
fn bayer_quality_research_campaign() -> Result<()> {
    let candidate = std::env::var("TR_BAYER_CANDIDATE")?;
    ensure!(
        matches!(candidate.as_str(), "BR1" | "BR2" | "BR3" | "BR4" | "BR5"),
        "Bayer candidate"
    );
    run_quality_campaign(
        &candidate,
        if candidate == "BR5" {
            include_str!("../tests/fixtures/bayer-research-v3.json")
        } else if matches!(candidate.as_str(), "BR3" | "BR4") {
            include_str!("../tests/fixtures/bayer-research-v2.json")
        } else {
            include_str!("../tests/fixtures/bayer-research-v1.json")
        },
    )
}

fn run_quality_campaign(candidate: &str, text: &str) -> Result<()> {
    let root = PathBuf::from(
        std::env::var("TR_QUALITY_LAB_ROOT").context("TR_QUALITY_LAB_ROOT required")?,
    );
    let split = std::env::var("TR_QUALITY_SPLIT").context("TR_QUALITY_SPLIT required")?;
    ensure!(
        matches!(split.as_str(), "development" | "validation"),
        "invalid split"
    );
    let protocol: Value = serde_json::from_str(text)?;
    let q = &protocol["quality"];
    let w = q[&split]["width"]
        .as_u64()
        .unwrap_or(q["width"].as_u64().unwrap()) as usize;
    let h = q[&split]["height"]
        .as_u64()
        .unwrap_or(q["height"].as_u64().unwrap()) as usize;
    let border = q["border"].as_u64().unwrap() as usize;
    let cases = scenes(&protocol, &split, w, h);
    let mut details = vec![];
    let mut totals: BTreeMap<&str, [Metrics; 2]> = BTreeMap::new();
    let mut all_passed = true;
    let mut worst_ratios: BTreeMap<String, f64> = BTreeMap::new();
    let mut worst_cases: BTreeMap<String, Value> = BTreeMap::new();
    let is_bayer = matches!(candidate, "BR1" | "BR2" | "BR3" | "BR4" | "BR5" | "BR6");
    let phases = if is_bayer { 4 } else { 36 };
    for phase in 0..phases {
        let cfa = if is_bayer {
            Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2)
        } else {
            Cfa::new(6, &XTRANS)?.shifted(phase % 6, phase / 6)
        };
        let mut per_phase: BTreeMap<&str, [Metrics; 2]> = BTreeMap::new();
        for (case, scene) in cases.iter().enumerate() {
            let mut seed = q[&split]["seed"].as_u64().unwrap() as u32
                ^ (case as u32 * 1877 + phase as u32 * 8191);
            let noise: Vec<_> = (0..w * h)
                .map(|_| {
                    (-2. * random(&mut seed).ln()).sqrt()
                        * (TAU * random(&mut seed)).cos()
                        * scene.sigma
                })
                .collect();
            let mosaic: Vec<_> = scene
                .truth
                .iter()
                .enumerate()
                .map(|(i, rgb)| {
                    // Same measured sample for both methods. Optional fixed
                    // six-site correlation is a stress input, never an algorithm parameter.
                    let value = if scene.noise_correlation == 0. {
                        noise[i]
                    } else {
                        let previous = i / w * w + (i % w + w - 6) % w;
                        (noise[i] + scene.noise_correlation * noise[previous])
                            / (1. + scene.noise_correlation.powi(2)).sqrt()
                    };
                    (f64::from(rgb[cfa.color(i % w, i / w)]) + value) as f32
                })
                .collect();
            let a: Vec<_> = if candidate == "DR7" {
                crate::xtrans_dr6::reconstruct(&mosaic, w, h, cfa)
            } else if is_bayer {
                crate::render::frame_reference::develop(
                    &mosaic,
                    w,
                    h,
                    cfa,
                    crate::color::ID,
                    [0, 0, w, h],
                    1,
                )?
                .pixels
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect()
            } else {
                let base = xtrans::CameraRgb::new(&mosaic, w, h, cfa);
                (0..w * h).map(|i| base.at(i % w, i / w)).collect()
            };
            let b = match candidate {
                "DR1" => directional::reconstruct(&mosaic, w, h, cfa),
                "DR2" => residual::reconstruct(&mosaic, w, h, cfa),
                "DR3" => chroma::reconstruct(&mosaic, w, h, cfa),
                "DR4" => confidence::reconstruct(&mosaic, w, h, cfa),
                "DR5" => refined::reconstruct(&mosaic, w, h, cfa),
                "DR6" => constrained::reconstruct(&mosaic, w, h, cfa),
                "DR7" => crate::xtrans_steps::reconstruct(&mosaic, w, h, cfa),
                "BR6" => {
                    crate::render::develop(&mosaic, w, h, cfa, crate::color::ID, [0, 0, w, h], 1)?
                        .pixels
                        .into_iter()
                        .map(|p| [p[0], p[1], p[2]])
                        .collect()
                }
                "BR1" | "BR2" => bayer::reconstruct(&mosaic, w, h, cfa, &a, candidate == "BR2"),
                "BR3" | "BR4" => bayer_guided::reconstruct(&mosaic, w, h, cfa, candidate == "BR4"),
                "BR5" => bayer_guarded::reconstruct(&mosaic, w, h, cfa, &a),
                _ => anyhow::bail!("unknown candidate"),
            };
            let metrics = [
                evaluate(&a, scene, &mosaic, w, h, cfa, border),
                evaluate(&b, scene, &mosaic, w, h, cfa, border),
            ];
            for m in &metrics {
                ensure!(
                    m.mismatches == 0 && m.nonfinite == 0,
                    "{} phase {phase}: measured/nonfinite",
                    scene.name
                );
            }
            let case_gate = gate(&metrics, scene.category, q);
            for (name, ratio) in case_gate["ratios"].as_object().unwrap() {
                let key = format!("{}/{name}", scene.category);
                let value = ratio.as_f64().unwrap();
                if worst_cases
                    .get(&key)
                    .is_none_or(|old| value > old["ratio"].as_f64().unwrap())
                {
                    worst_cases.insert(
                        key,
                        json!({"ratio":value,"scene":scene.name,"phase":phase,
                        "baseline":metrics[0].value(),"candidate":metrics[1].value()}),
                    );
                }
            }
            for (i, m) in metrics.iter().enumerate() {
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
        if matches!(candidate, "DR4" | "DR5" | "DR6") && (phase + 1) % 6 == 0 {
            println!("{split}: {}/36 phases measured", phase + 1);
        }
        if is_bayer {
            println!("{candidate} {split}: {}/4 phases measured", phase + 1);
        }
    }
    let categories:Vec<_>=totals.iter().map(|(category,pair)|json!({"category":category,"baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate(pair,category,q)})).collect();
    let candidate_source = match candidate {
        "BR1" | "BR2" => Some(include_bytes!("bayer.rs").as_slice()),
        "BR3" | "BR4" => Some(include_bytes!("bayer_guided.rs").as_slice()),
        "BR5" => Some(include_bytes!("bayer_guarded.rs").as_slice()),
        _ => None,
    };
    let report = json!({"protocol":protocol["protocol"],"protocol_sha256":hash(text.as_bytes()),"candidate":candidate,"candidate_source_sha256":candidate_source.map(hash),"split":split,
        "baseline":if candidate=="DR7" {"DR6"} else if is_bayer {"Bayer dir1"} else {"E8 camera RGB"},
        "edge_planes_sha256":hash(include_bytes!("../src/edge_planes.rs")),
        "xtrans_steps_sha256":hash(include_bytes!("../src/xtrans_steps.rs")),
        "scenes":cases.iter().map(|s|&s.name).collect::<Vec<_>>(),"scene_phase_cases":cases.len()*phases,"width":w,"height":h,
        "synthetic_gates_passed":all_passed,"production_promoted":false,"photographic_ground_truth_available":false,
        "categories":categories,"worst_phase_ratios":worst_ratios,"worst_scene_phase_cases":worst_cases,"per_phase":details});
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(format!("quality-{split}.json")))?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{split}: {} cases, synthetic gates={all_passed}",
        cases.len() * phases
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

#[test]
fn quality_gates_reject_chroma_and_detail_regressions_independently() {
    let protocol: Value =
        serde_json::from_str(include_str!("../tests/fixtures/research-v2.json")).unwrap();
    let q = &protocol["quality"];
    let unit = Metric {
        sse: 100.,
        count: 100,
    };
    let baseline = Metrics {
        missing: unit,
        chroma: unit,
        luma: unit,
        gradient: unit,
        border: unit,
        ..Default::default()
    };
    let mut candidate = baseline.clone();
    candidate.chroma.sse = 49.;
    assert_eq!(
        gate(
            &[baseline.clone(), candidate.clone()],
            "achromatic_texture",
            q
        )["passed"],
        true
    );
    candidate.gradient.sse = 121.;
    assert_eq!(
        gate(
            &[baseline.clone(), candidate.clone()],
            "achromatic_texture",
            q
        )["passed"],
        false
    );
    candidate.gradient = unit;
    candidate.chroma.sse = 81.;
    assert_eq!(
        gate(&[baseline, candidate], "achromatic_texture", q)["passed"],
        false
    );
    assert_eq!(
        gate(
            &[Metrics::default(), Metrics::default()],
            "flat_extended",
            q
        )["passed"],
        true
    );
}

#[test]
fn new_holdout_changes_every_scene_and_preserves_development_and_thresholds() {
    let protocols: [Value; 6] = [
        include_str!("../tests/fixtures/research-v1.json"),
        include_str!("../tests/fixtures/research-v2.json"),
        include_str!("../tests/fixtures/research-v3.json"),
        include_str!("../tests/fixtures/research-v4.json"),
        include_str!("../tests/fixtures/research-v5.json"),
        include_str!("../tests/fixtures/research-v6.json"),
    ]
    .map(|text| serde_json::from_str(text).unwrap());
    for (a, b) in [
        (0, 1),
        (0, 2),
        (1, 2),
        (0, 3),
        (1, 3),
        (2, 3),
        (0, 4),
        (1, 4),
        (2, 4),
        (3, 4),
        (0, 5),
        (1, 5),
        (2, 5),
        (3, 5),
        (4, 5),
    ] {
        check_distinct_holdout(&protocols[a], &protocols[b]);
    }
}

fn check_distinct_holdout(old: &Value, new: &Value) {
    for key in [
        "false_chroma_rmse_max_ratio",
        "other_category_and_detail_rmse_max_ratio",
        "absolute_rmse_floor",
    ] {
        assert_eq!(old["quality"][key], new["quality"][key]);
    }
    for split in ["development", "validation"] {
        // Equal geometry here deliberately tests that scene content changes,
        // rather than relying only on the new holdout's dimensions.
        let a = scenes(old, split, 192, 144);
        let b = scenes(new, split, 192, 144);
        assert!(a.len() >= 57 && b.len() >= 57);
        for (a, b) in a.iter().zip(&b) {
            if split == "development" {
                assert_eq!(a.truth, b.truth);
                assert_eq!(a.sigma, b.sigma);
            } else {
                assert_ne!(a.truth, b.truth, "reused scene {}", a.name);
            }
        }
        if split == "validation" {
            for new in &b {
                assert!(
                    a.iter().all(|old| old.truth != new.truth),
                    "reused held-out truth {}",
                    new.name
                );
            }
        }
    }
}

#[test]
fn bayer_v2_uses_four_phases_and_an_unconsumed_holdout() {
    let old: Value =
        serde_json::from_str(include_str!("../tests/fixtures/bayer-research-v1.json")).unwrap();
    let new: Value =
        serde_json::from_str(include_str!("../tests/fixtures/bayer-research-v2.json")).unwrap();
    assert_eq!(new["quality"]["phases"], 4);
    assert_eq!(old["quality"]["development"], new["quality"]["development"]);
    check_distinct_holdout(&old, &new);
}

#[test]
fn noisy_structure_and_spatial_stress_do_not_dilute_the_original_categories() {
    let protocol: Value =
        serde_json::from_str(include_str!("../tests/fixtures/research-v4.json")).unwrap();
    for split in ["development", "validation"] {
        let cases = scenes(&protocol, split, 192, 144);
        assert_eq!(cases.len(), 209);
        let mut counts = BTreeMap::new();
        for s in cases {
            *counts.entry(s.category).or_insert(0) += 1;
        }
        assert_eq!(
            counts,
            BTreeMap::from([
                ("achromatic_texture", 20),
                ("chromatic_texture", 20),
                ("edge", 8),
                ("flat_extended", 3),
                ("noise", 6),
                ("noisy_achromatic_texture", 60),
                ("noisy_chromatic_texture", 60),
                ("noisy_edge", 24),
                ("cross_texture", 4),
                ("correlated_noise", 4)
            ])
        );
    }
}

#[test]
fn stratified_protocols_expand_validation_without_changing_development_gates() {
    for text in [
        include_str!("../tests/fixtures/research-v5.json"),
        include_str!("../tests/fixtures/research-v6.json"),
    ] {
        let p: Value = serde_json::from_str(text).unwrap();
        assert_eq!(scenes(&p, "development", 192, 144).len(), 209);
        assert_eq!(scenes(&p, "validation", 216, 168).len(), 369);
        assert_eq!(
            p["quality"]["validation"]["frequencies"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
    }
}

#[test]
fn optimized_execution_matches_frozen_dr6_on_structures_and_noise() {
    for (w, h) in [(6, 7), (17, 21), (79, 65)] {
        for phase in 0..36 {
            let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
            for mode in 0..3 {
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| {
                        let noise = ((i * 7919 + phase * 31) % 997) as f32 / 997. - 0.5;
                        let signal = ((i % w) as f32 * 0.21 + (i / w) as f32 * 0.13).sin();
                        match mode {
                            0 => 0.4 + 0.03 * noise,
                            1 => {
                                -0.3 + 0.8 * signal
                                    + 0.01 * noise
                                    + cfa.color(i % w, i / w) as f32 * 0.6
                            }
                            _ => (if i % w < w / 2 { -0.2 } else { 1.7 }) + 0.02 * noise,
                        }
                    })
                    .collect();
                let expected = constrained::reconstruct(&mosaic, w, h, cfa);
                let actual = crate::xtrans_dr6::reconstruct(&mosaic, w, h, cfa);
                for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
                    let channel = cfa.color(i % w, i / w);
                    assert_eq!(a[channel].to_bits(), mosaic[i].to_bits());
                    for c in 0..3 {
                        assert!((a[c] - b[c]).abs() <= 2e-6, "{phase} {mode} {i} {c}");
                    }
                }
            }
        }
    }
}

#[test]
fn bayer_v3_keeps_gates_and_changes_both_consumed_holdouts() {
    let new: Value =
        serde_json::from_str(include_str!("../tests/fixtures/bayer-research-v3.json")).unwrap();
    for text in [
        include_str!("../tests/fixtures/bayer-research-v1.json"),
        include_str!("../tests/fixtures/bayer-research-v2.json"),
    ] {
        let old: Value = serde_json::from_str(text).unwrap();
        check_distinct_holdout(&old, &new);
    }
}
