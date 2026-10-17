//! Small, in-memory sensor-exposure diagnostics. Analytical camera RGB is
//! known before quantization/censoring; no photograph or physical colour truth
//! is inferred from the generated scenes. Only numerical summaries are saved.
use crate::{cfa::Cfa, color, dng::Dng, highlights, test_support::Fixture};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
struct Scene {
    kind: usize,
    phase: usize,
    period: usize,
    shift: [usize; 2],
    exposure: f64,
    code_offset: i32,
    white: u32,
    wb: [f64; 3],
    seed: u64,
}

const W: usize = 96;
const H: usize = 72;

fn scene(s: Scene) -> (Vec<u8>, Vec<[f32; 3]>) {
    // Frozen mixing; independent of changes to a library RNG implementation.
    let random = |key: u64| {
        let mut v = s.seed.wrapping_add(key.wrapping_mul(0x9e3779b97f4a7c15));
        v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((v ^ (v >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let (slope, centre, sigma, peak, hue) = if s.seed == 0 {
        (0.012, 48., 14., 1.001, [0.64, 1., 0.42])
    } else {
        let mut hue = [
            0.30 + 0.4 * random(1),
            0.30 + 0.4 * random(2),
            0.30 + 0.4 * random(3),
        ];
        hue[(s.seed % 3) as usize] = 1.;
        (
            0.007 + 0.01 * random(4),
            43. + 10. * random(5),
            10. + 12. * random(6),
            1.0002 + 0.0018 * random(7),
            hue,
        )
    };
    let cfa = if s.period == 2 {
        Cfa::bayer([0, 1, 1, 2])
    } else {
        Cfa::new(6, &crate::cfa::XTRANS).unwrap()
    }
    .shifted(s.phase % s.period, s.phase / s.period);
    let mut f = Fixture::new(true);
    f.width = W;
    f.height = H;
    f.ints(256, 4, &[W as u32]);
    f.ints(257, 4, &[H as u32]);
    f.ints(278, 4, &[H as u32]);
    f.ints(33421, 3, &[s.period as u32; 2]);
    f.ints(
        33422,
        1,
        &(0..s.period * s.period)
            .map(|p| cfa.color(p % s.period, p / s.period) as u32)
            .collect::<Vec<_>>(),
    );
    f.ints(50717, 4, &[s.white]);
    let black = if s.seed == 0 {
        128.
    } else {
        64. + (320. * random(8)).round()
    };
    f.rational(50714, false, &[black]);
    let camera: Vec<[f32; 3]> = (0..W * H)
        .map(|i| {
            let x = (i % W) as f64 - s.shift[0] as f64;
            let y = (i / W) as f64 - s.shift[1] as f64;
            let plane = 1. + slope * (x - centre - 1.) + 0.0007 * (y - 36.);
            let level = match s.kind {
                0 => plane,
                1 => plane.min(1.00008),
                2 => 0.965 + (peak - 0.965) * (-((x - centre) / sigma).powi(2)).exp(),
                3 => 0.99,
                4 => {
                    if x < 48. {
                        0.94
                    } else {
                        1.08
                    }
                }
                _ => 1.4,
            };
            let hue = if s.kind == 5 { [1., 1., 1.] } else { hue };
            hue.map(|v| (v * level * s.exposure.exp2()) as f32)
        })
        .collect();
    f.pixels = camera
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = (i % W) as i64 - s.shift[0] as i64;
            let y = (i / W) as i64 - s.shift[1] as i64;
            // Bounded stress noise, not a calibrated sensor noise model.
            let noise = if s.seed != 0 && s.kind == 4 {
                0.001 * (2. * random((x + 512 + 1024 * (y + 512)) as u64) - 1.)
            } else {
                0.
            };
            ((black
                + (f64::from(s.white) - black) * (f64::from(v[cfa.color(i % W, i / W)]) + noise))
                .round() as i32
                + s.code_offset)
                .clamp(0, s.white as i32) as u16
        })
        .collect();
    (f.bytes(), camera)
}

fn errors(actual: &[[f32; 4]], truth: &[[f32; 3]], shift: [usize; 2]) -> serde_json::Value {
    let (mut sum, mut maximum, mut chroma, mut gradient) = (0., 0_f64, 0_f64, 0_f64);
    let mut count = 0;
    for y in 18..54 {
        for x in 24..72 {
            let i = (y + shift[1]) * W + x + shift[0];
            let a = actual[i];
            let t = truth[i];
            let an = a[..3].iter().sum::<f32>();
            let tn = t.iter().sum::<f32>();
            for c in 0..3 {
                let e = f64::from(a[c]) - f64::from(t[c]);
                sum += e * e;
                maximum = maximum.max(e.abs());
                if an > 0.01 && tn > 0.01 {
                    chroma = chroma.max(f64::from(a[c] / an - t[c] / tn).abs());
                }
                for d in [1, W] {
                    gradient = gradient.max(
                        (f64::from(a[c] - actual[i + d][c]) - f64::from(t[c] - truth[i + d][c]))
                            .abs(),
                    );
                }
                count += 1;
            }
        }
    }
    json!({"rmse":(sum / f64::from(count)).sqrt(),"max":maximum,
        "chroma_max":chroma,"gradient_max":gradient,"channel_samples":count})
}

fn run(s: Scene) -> (serde_json::Value, Vec<[f32; 4]>) {
    let (bytes, camera) = scene(s);
    let raw = Dng::parse(&bytes).unwrap().mosaic().unwrap();
    let (gains, matrix) = raw.calibration.resolve(s.wb).unwrap();
    let truth: Vec<_> = camera
        .iter()
        .map(|p| color::project(matrix, std::array::from_fn(|c| p[c] * gains[c])))
        .collect();
    let mut recovered = raw.values.clone();
    let evidence = highlights::recover(&mut recovered, &raw.saturation, W, W, H, raw.cfa);
    let mut sensor_max = [0_f64; 2];
    let mut sensor_squared = [0_f64; 2];
    let mut preserved = 0;
    for (i, &v) in recovered.iter().enumerate() {
        if raw.saturation[i] != highlights::SATURATED {
            assert_eq!(v.to_bits(), raw.values[i].to_bits());
            preserved += 1;
        }
        let target = f64::from(camera[i][raw.cfa.color(i % W, i / W)]);
        for (k, value) in [raw.values[i], v].into_iter().enumerate() {
            let e = f64::from(value) - target;
            sensor_max[k] = sensor_max[k].max(e.abs());
            sensor_squared[k] += e * e;
        }
    }
    let mut before = Dng::parse(&bytes).unwrap().mosaic().unwrap();
    before.highlight_confidence.fill(0);
    let before = before.develop(s.wb).unwrap();
    let after = raw.develop(s.wb).unwrap();
    let changed = before
        .pixels
        .iter()
        .zip(&after.pixels)
        .filter(|(a, b)| a != b)
        .count();
    let mut row = json!({"kind":s.kind,"phase":s.phase,"period":s.period,"shift":s.shift,
        "exposure_ev":s.exposure,"code_offset":s.code_offset,"white":s.white,"wb":s.wb,"seed":s.seed,
        "input_sha256":format!("{:x}",Sha256::digest(&bytes)),
        "saturated":evidence.saturated,"reconstructed":evidence.reconstructed,
        "valid_preserved":preserved,"fallback_pixels":changed,
        "sensor_max":sensor_max,"sensor_rmse":sensor_squared.map(|v|(v/(W*H) as f64).sqrt()),
        "without_fallback":errors(&before.pixels,&truth,s.shift),
        "rendered":errors(&after.pixels,&truth,s.shift)});
    if s.seed == 0 && s.period == 2 && s.phase == 0 && s.kind == 2 {
        row["profile"] = json!((24..72).map(|x| {
            let i = (36+s.shift[1])*W+x+s.shift[0];
            json!({"x":x,"rendered":after.pixels[i],"truth":truth[i],"without_fallback":before.pixels[i]})
        }).collect::<Vec<_>>());
    }
    (row, after.pixels)
}

#[test]
fn localized_coloured_light_keeps_its_colour_when_translated() {
    for phase in 0..4 {
        let base = Scene {
            kind: 2,
            phase,
            period: 2,
            shift: [0, 0],
            exposure: 0.,
            code_offset: 0,
            white: 16383,
            wb: [1.8, 1., 1.3],
            seed: 0,
        };
        let (_, reference) = run(base);
        for shift in [[0, 0], [2, 2], [4, 4]] {
            let (row, image) = run(Scene { shift, ..base });
            assert!(
                row["rendered"]["max"].as_f64().unwrap() < 0.005,
                "{phase} {shift:?}: {row}"
            );
            for y in 18..54 {
                for x in 24..72 {
                    for c in 0..3 {
                        assert!(
                            (image[(y + shift[1]) * W + x + shift[0]][c] - reference[y * W + x][c])
                                .abs()
                                < 0.005
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn oblique_shoulders_do_not_trade_lower_average_error_for_colour_or_edge_peaks() {
    // Independent camera-RGB truth exposes partial axial correction of an
    // oblique knee. These maxima were preserved by HL6; an earlier HL7
    // candidate lowered RMSE but exceeded both bounds.
    for (seed, phase, metric, limit) in [
        (0, 2, "chroma_max", 0.0030413),
        (101703, 0, "gradient_max", 0.0031377),
    ] {
        let (row, _) = run(Scene {
            kind: 1,
            phase,
            period: 2,
            shift: [0, 0],
            exposure: 0.,
            code_offset: 0,
            white: if seed == 0 { 16383 } else { 4095 },
            wb: [0.75, 1., 2.],
            seed,
        });
        assert!(
            row["rendered"][metric].as_f64().unwrap() <= limit,
            "seed={seed} phase={phase} {metric}: {row}"
        );
    }
}

#[test]
fn rounded_light_is_not_mistaken_for_a_measured_plateau() {
    for phase in 0..4 {
        let (row, _) = run(Scene {
            kind: 2,
            phase,
            period: 2,
            shift: [2, 2],
            exposure: 0.,
            code_offset: 0,
            white: 16383,
            wb: [1.8, 1., 1.3],
            seed: 101719,
        });
        assert!(
            row["sensor_max"][1].as_f64().unwrap() < 0.000655,
            "phase={phase}: {row}"
        );
    }
}

#[test]
fn diagnostics_measure_the_same_aligned_region_and_find_a_single_outlier() {
    let truth = vec![[1.; 3]; W * H];
    let mut actual = vec![[1.; 4]; W * H];
    actual[23 * W + 44][1] = 1.25;
    let metrics = errors(&actual, &truth, [4, 3]);
    assert_eq!(metrics["max"], 0.25);
    assert_eq!(metrics["gradient_max"], 0.25);
    assert_eq!(metrics["channel_samples"], 48 * 36 * 3);
    assert!(
        (metrics["rmse"].as_f64().unwrap() - (0.0625_f64 / (48. * 36. * 3.)).sqrt()).abs() < 1e-12
    );
    actual[0][0] = 1000.; // Outside the common evaluation region.
    assert_eq!(errors(&actual, &truth, [4, 3]), metrics);
}

#[test]
#[ignore = "explicit small numerical sensor-exposure campaign"]
fn research_campaign_highlight_stability() {
    let out = std::env::var("TR_HL_STABILITY_REPORT").unwrap();
    assert!(!std::path::Path::new(&out).exists());
    let seed: u64 = std::env::var("TR_HL_STABILITY_SEED").map_or(0, |v| v.parse().unwrap());
    let colour_wb = std::env::var_os("TR_HL_STABILITY_COLOURED_WB").is_some();
    let mut rows = vec![];
    for period in [2, 6] {
        for phase in 0..period * period {
            for kind in 0..6 {
                let base = Scene {
                    kind,
                    phase,
                    period,
                    shift: [0, 0],
                    exposure: 0.,
                    code_offset: 0,
                    white: if seed == 0 {
                        16383
                    } else {
                        [4095, 16383, 65535][(seed % 3) as usize]
                    },
                    wb: if colour_wb {
                        [[0.75, 1., 2.], [1.8, 1., 1.3], [2.4, 1., 0.8]][(seed % 3) as usize]
                    } else {
                        [1.8, 1., 1.3]
                    },
                    seed,
                };
                let (mut row, reference) = run(base);
                row["variant"] = json!("base");
                rows.push(row);
                for (variant, s) in [
                    (
                        "code+1",
                        Scene {
                            code_offset: 1,
                            ..base
                        },
                    ),
                    (
                        "code-1",
                        Scene {
                            code_offset: -1,
                            ..base
                        },
                    ),
                    (
                        "ev-0.02",
                        Scene {
                            exposure: -0.02,
                            ..base
                        },
                    ),
                    (
                        "ev+0.02",
                        Scene {
                            exposure: 0.02,
                            ..base
                        },
                    ),
                    (
                        "wb+0.001",
                        Scene {
                            wb: [base.wb[0] + 0.001, base.wb[1], base.wb[2]],
                            ..base
                        },
                    ),
                    (
                        "translate",
                        Scene {
                            shift: [period, period],
                            ..base
                        },
                    ),
                ] {
                    let (mut row, pixels) = run(s);
                    let mut max = 0_f64;
                    for y in 18..54 {
                        for x in 24..72 {
                            for c in 0..3 {
                                max = max.max(
                                    (f64::from(pixels[(y + s.shift[1]) * W + x + s.shift[0]][c])
                                        / s.exposure.exp2()
                                        - f64::from(reference[y * W + x][c]))
                                    .abs(),
                                );
                            }
                        }
                    }
                    row["variant"] = json!(variant);
                    row["aligned_change_max"] = json!(max);
                    rows.push(row);
                }
            }
        }
    }
    let report = json!({"schema":1,"protocol":"TR-sensor-stability-v1",
        "recipe":crate::RECIPE,"size":[W,H],"region":[24,18,48,36],
        "scope":"Analytical camera RGB; quantization and sensor clipping before development. No physical colour qualification. Translation includes the whole pipeline, so differences alone do not establish a grid defect.","cases":rows});
    std::fs::write(out, serde_json::to_vec(&report).unwrap()).unwrap();
}
