//! BR1/BR2: masked local residual probes with baseline green held fixed.
//! Equations and limits in docs/demosaic-bayer-experimental.md; test-only.
use crate::cfa::Cfa;

pub fn reconstruct(
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    baseline: &[[f32; 3]],
    laplacian: bool,
) -> Vec<[f32; 3]> {
    let mut output = baseline.to_vec();
    for channel in [0, 2] {
        let mut predicted = vec![0.; mosaic.len()];
        for y in 0..h {
            for x in 0..w {
                let (mut n, mut g, mut c, mut gg, mut gc) = (0., 0., 0., 0., 0.);
                let (mut ln, mut lgg, mut lgc) = (0., 0., 0.);
                for yy in y.saturating_sub(3)..=(y + 3).min(h - 1) {
                    for xx in x.saturating_sub(3)..=(x + 3).min(w - 1) {
                        if cfa.color(xx, yy) != channel {
                            continue;
                        }
                        let k = yy * w + xx;
                        let gv = f64::from(baseline[k][1]);
                        let cv = f64::from(mosaic[k]);
                        n += 1.;
                        g += gv;
                        c += cv;
                        gg += gv * gv;
                        gc += gv * cv;
                        if laplacian && xx >= 2 && yy >= 2 && xx + 2 < w && yy + 2 < h {
                            let neighbors = [k - 2, k + 2, k - 2 * w, k + 2 * w];
                            let lg = 4. * gv
                                - neighbors
                                    .iter()
                                    .map(|&i| f64::from(baseline[i][1]))
                                    .sum::<f64>();
                            let lc = 4. * cv
                                - neighbors.iter().map(|&i| f64::from(mosaic[i])).sum::<f64>();
                            ln += 1.;
                            lgg += lg * lg;
                            lgc += lg * lc;
                        }
                    }
                }
                let a = if ln > 0. {
                    (lgc / ln) / (lgg / ln + 1e-6)
                } else {
                    (gc / n - g * c / (n * n)) / ((gg / n - g * g / (n * n)).max(0.) + 1e-6)
                };
                let b = c / n - a * g / n;
                predicted[y * w + x] = (a * f64::from(baseline[y * w + x][1]) + b) as f32;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if cfa.color(x, y) == channel {
                    output[i][channel] = mosaic[i];
                    continue;
                }
                let offsets: &[(isize, isize)] = if cfa.color(x, y) != 1 {
                    &[(-1, -1), (1, -1), (-1, 1), (1, 1)]
                } else if cfa.color(x ^ 1, y) == channel {
                    &[(-1, 0), (1, 0)]
                } else {
                    &[(0, -1), (0, 1)]
                };
                let (mut sum, mut count) = (0., 0.);
                for &(dx, dy) in offsets {
                    let (xx, yy) = (x as isize + dx, y as isize + dy);
                    if xx >= 0 && yy >= 0 && xx < w as isize && yy < h as isize {
                        let k = yy as usize * w + xx as usize;
                        sum += mosaic[k] - predicted[k];
                        count += 1.;
                    }
                }
                output[i][channel] = predicted[i] + sum / count;
            }
        }
    }
    output
}

#[test]
fn bayer_residual_probes_preserve_flats_and_measured_sites() {
    for pattern in [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]] {
        let cfa = Cfa::bayer(pattern);
        for (w, h) in [(7, 9), (31, 27)] {
            for flat in [true, false] {
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| {
                        if flat {
                            [-0.1, 0.4, 1.3][cfa.color(i % w, i / w)]
                        } else {
                            (i * 127 % 997) as f32 / 400. - 0.2
                        }
                    })
                    .collect();
                let image =
                    crate::render::develop(&mosaic, w, h, cfa, crate::color::ID, [0, 0, w, h], 1)
                        .unwrap();
                let baseline: Vec<_> = image.pixels.iter().map(|p| [p[0], p[1], p[2]]).collect();
                for lap in [false, true] {
                    for (i, p) in reconstruct(&mosaic, w, h, cfa, &baseline, lap)
                        .iter()
                        .enumerate()
                    {
                        let c = cfa.color(i % w, i / w);
                        assert_eq!(p[c].to_bits(), mosaic[i].to_bits());
                        assert!(p.iter().all(|v| v.is_finite()));
                        if flat {
                            for (a, b) in p.iter().zip([-0.1, 0.4, 1.3]) {
                                assert!((a - b).abs() < 1e-6);
                            }
                        }
                    }
                }
            }
        }
    }
}
