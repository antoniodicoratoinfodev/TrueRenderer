//! Reproducible analytical comparison, with known linear RGB ground truth.
//! Optional argument: JSON output path. No third-party images are used.
use tr_core::{color::rec2020_to_linear_srgb, demosaic};

fn main() -> anyhow::Result<()> {
    let (w, h) = (128usize, 96usize);
    let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    let mut cases = vec![];
    for pattern in [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]] {
        for scene in 0..8 {
            let truth: Vec<[f32; 3]> = (0..w * h)
                .map(|i| {
                    let x = (i % w) as f32;
                    let y = (i / w) as f32;
                    let wave = match scene {
                        0 => 0.2 + 0.6 * x / (w - 1) as f32,
                        1 => 0.5 + 0.25 * (x * 0.8).sin(),
                        2 => 0.5 + 0.25 * (y * 0.8).sin(),
                        3 => 0.5 + 0.25 * ((x + y) * 0.55).sin(),
                        4 => {
                            if x > 0.63 * y + 35. {
                                0.75
                            } else {
                                0.2
                            }
                        }
                        5 => 0.5 + 0.2 * (x * x * 0.012 + y * y * 0.009).sin(),
                        6 => 0.5 + 0.25 * (x * 0.8).sin(),
                        _ => 0.5 + 0.15 * (x * 0.5).sin() + 0.15 * (y * 0.4).cos(),
                    };
                    if scene == 7 {
                        [
                            wave,
                            0.5 + 0.25 * (y * 0.8).sin(),
                            0.5 + 0.25 * (x * 0.9).cos(),
                        ]
                    } else {
                        [wave * 0.9 + 0.02, wave, wave * 0.75 + 0.06]
                    }
                })
                .collect();
            let mosaic: Vec<f32> = truth
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let noise = if scene == 6 {
                        ((((i as u32).wrapping_mul(1664525).wrapping_add(1013904223)) >> 16) as f32
                            / 65535.
                            - 0.5)
                            * 0.04
                    } else {
                        0.
                    };
                    p[pattern[(i / w % 2) * 2 + i % w % 2] as usize] + noise
                })
                .collect();
            let result = demosaic::develop(&mosaic, w as u32, h as u32, pattern, identity, 0)?;
            let mut errors = [0f64; 2];
            let mut max_error = [0f32; 2];
            let mut count = 0;
            // Three-pixel border excluded from quality metrics; border contracts
            // are checked separately by unit tests, not hidden in these numbers.
            for y in 3..h - 3 {
                for x in 3..w - 3 {
                    let i = y * w + x;
                    let p = result.pixels[i];
                    let rgb = rec2020_to_linear_srgb([p[0], p[1], p[2]]);
                    for c in 0..3 {
                        let linear = if pattern[(y % 2) * 2 + x % 2] as usize == c {
                            mosaic[i]
                        } else {
                            let mut sum = 0.;
                            let mut n = 0;
                            for yy in y - 1..=y + 1 {
                                for xx in x - 1..=x + 1 {
                                    if pattern[(yy % 2) * 2 + xx % 2] as usize == c {
                                        sum += mosaic[yy * w + xx];
                                        n += 1;
                                    }
                                }
                            }
                            sum / n as f32
                        };
                        for (engine, v) in [linear, rgb[c]].into_iter().enumerate() {
                            let e = (v - truth[i][c]).abs();
                            errors[engine] += f64::from(e).powi(2);
                            max_error[engine] = max_error[engine].max(e);
                        }
                        count += 1;
                    }
                }
            }
            let mse = errors.map(|e| e / count as f64);
            cases.push(serde_json::json!({"scene":(["ramp","horizontal-frequency","vertical-frequency","diagonal-frequency","slanted-edge","chirp","uniform-noise","independent-chroma"][scene]),"cfa":pattern,"bilinear_mse":mse[0],"directional_mse":mse[1],"max_error":max_error,"directional_lower_mse":mse[1]<mse[0]}));
        }
    }
    let report = serde_json::json!({"recipe":"TR-directional-f32-v1","dimensions":[w,h],"cases":cases,"scope":"Own analytical linear RGB scenes remosaiced as Bayer; unit camera matrix and WB. Quality metrics exclude a 3-pixel border. Bilinear comparator is an independent mathematical reference, not LibRaw. No camera colour, Apple comparison, sensor noise qualification or universal quality verdict."});
    let output = serde_json::to_string_pretty(&report)? + "\n";
    if let Some(path) = std::env::args_os().nth(1) {
        std::fs::write(path, output)?;
    } else {
        println!("{output}");
    }
    Ok(())
}
