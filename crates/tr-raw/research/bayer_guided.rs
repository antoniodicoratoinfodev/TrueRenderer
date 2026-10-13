//! BR3/BR4, test-only RAW-domain RI/MLRI candidates. Project implementation
//! of local regression mathematics; no imported IPOL source. Exact choices
//! (including departures from published 8-bit implementations) are specified
//! in docs/demosaic-bayer-experimental.md.
use crate::cfa::Cfa;

const EPSILON: f64 = 1e-6;

fn reflect(x: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let p = 2 * (n as isize - 1);
    let x = x.rem_euclid(p);
    if x < n as isize {
        x as usize
    } else {
        (p - x) as usize
    }
}

#[derive(Clone, Copy)]
struct Grid {
    w: usize,
    h: usize,
}
impl Grid {
    fn at(self, x: isize, y: isize) -> usize {
        reflect(y, self.h) * self.w + reflect(x, self.w)
    }
    fn shift(self, i: usize, axis: usize, distance: isize) -> usize {
        self.at(
            (i % self.w) as isize + if axis == 0 { distance } else { 0 },
            (i / self.w) as isize + if axis == 1 { distance } else { 0 },
        )
    }
    fn box_mean(self, input: &[f64], radii: [usize; 2]) -> Vec<f64> {
        let mut values = input.to_vec();
        for (axis, radius) in radii.into_iter().enumerate() {
            if radius == 0 {
                continue;
            }
            let mut result = vec![0.; input.len()];
            let (lines, length, stride) = if axis == 0 {
                (self.h, self.w, 1)
            } else {
                (self.w, self.h, self.w)
            };
            for line in 0..lines {
                let start = if axis == 0 { line * self.w } else { line };
                let r = radius as isize;
                let get = |x| values[start + reflect(x, length) * stride];
                let mut sum: f64 = (-r..=r).map(get).sum();
                for x in 0..length {
                    result[start + x * stride] = sum / (2 * radius + 1) as f64;
                    sum += get(x as isize + r + 1) - get(x as isize - r);
                }
            }
            values = result;
        }
        values
    }
    fn laplace(self, input: &[f64], axis: Option<usize>) -> Vec<f64> {
        (0..input.len())
            .map(|i| {
                let axes: &[usize] = match axis {
                    Some(0) => &[0],
                    Some(1) => &[1],
                    _ => &[0, 1],
                };
                axes.iter()
                    .map(|&a| {
                        input[self.shift(i, a, -2)] + input[self.shift(i, a, 2)] - 2. * input[i]
                    })
                    .sum()
            })
            .collect()
    }
    fn gaussian(self, input: &[f64]) -> Vec<f64> {
        let weights: Vec<_> = (-4..=4)
            .map(|i: i32| (-0.5 * f64::from(i * i)).exp())
            .collect();
        let norm: f64 = weights.iter().sum();
        let mut values = input.to_vec();
        for axis in 0..2 {
            values = (0..input.len())
                .map(|i| {
                    weights
                        .iter()
                        .enumerate()
                        .map(|(j, &v)| v * values[self.shift(i, axis, j as isize - 4)])
                        .sum::<f64>()
                        / norm
                })
                .collect();
        }
        values
    }
}

fn guided(
    grid: Grid,
    guide: &[f64],
    target: &[f64],
    mask: &[f64],
    radii: [usize; 2],
    laplacian: bool,
    axis: Option<usize>,
) -> Vec<f64> {
    let n = guide.len();
    let counts = grid.box_mean(mask, radii);
    let mean = |values: Vec<f64>| -> Vec<f64> {
        let masked: Vec<_> = values.iter().zip(mask).map(|(v, m)| v * m).collect();
        grid.box_mean(&masked, radii)
            .iter()
            .zip(&counts)
            .map(|(v, n)| if *n > 0. { v / n } else { 0. })
            .collect()
    };
    let mg = mean(guide.to_vec());
    let mt = mean(target.to_vec());
    let (g, t) = if laplacian {
        (grid.laplace(guide, axis), grid.laplace(target, axis))
    } else {
        (guide.to_vec(), target.to_vec())
    };
    let gg = mean(g.iter().map(|v| v * v).collect());
    let gt = mean(g.iter().zip(&t).map(|(a, b)| a * b).collect());
    let a: Vec<_> = (0..n)
        .map(|i| {
            if laplacian {
                gt[i] / (gg[i] + EPSILON)
            } else {
                (gt[i] - mg[i] * mt[i]) / ((gg[i] - mg[i] * mg[i]).max(0.) + EPSILON)
            }
        })
        .collect();
    let b: Vec<_> = (0..n).map(|i| mt[i] - a[i] * mg[i]).collect();
    let a = grid.box_mean(&a, radii);
    let b = grid.box_mean(&b, radii);
    (0..n).map(|i| a[i] * guide[i] + b[i]).collect()
}

pub fn reconstruct(mosaic: &[f32], w: usize, h: usize, cfa: Cfa, laplacian: bool) -> Vec<[f32; 3]> {
    assert!(w >= 2 && h >= 2 && mosaic.len() == w * h && cfa.period == 2);
    let grid = Grid { w, h };
    let n = mosaic.len();
    let sites: Vec<_> = (0..n)
        .map(|i| match cfa.color(i % w, i / w) {
            0 => 0,
            2 => 3,
            _ => {
                if cfa.color((i % w) ^ 1, i / w) == 0 {
                    1
                } else {
                    2
                }
            }
        })
        .collect();
    let mask: [Vec<f64>; 4] =
        std::array::from_fn(|c| sites.iter().map(|&v| f64::from(v == c)).collect());
    let sparse: [Vec<f64>; 4] = std::array::from_fn(|c| {
        mosaic
            .iter()
            .zip(&mask[c])
            .map(|(&v, m)| f64::from(v) * m)
            .collect()
    });
    let mut differences: [Vec<f64>; 2] = std::array::from_fn(|_| vec![0.; n]);
    for (axis, difference) in differences.iter_mut().enumerate() {
        let partner = if axis == 0 {
            [1, 0, 3, 2]
        } else {
            [2, 3, 0, 1]
        };
        let initial: [Vec<f64>; 4] = std::array::from_fn(|c| {
            (0..n)
                .map(|i| {
                    sparse[c][i]
                        + 0.5
                            * (sparse[c][grid.shift(i, axis, -1)]
                                + sparse[c][grid.shift(i, axis, 1)])
                })
                .collect()
        });
        let radii = if laplacian {
            [3, 3]
        } else if axis == 0 {
            [5, 0]
        } else {
            [0, 5]
        };
        for c in 0..4 {
            let predicted = guided(
                grid,
                &initial[partner[c]],
                &initial[c],
                &mask[c],
                radii,
                laplacian,
                Some(axis),
            );
            let residual: Vec<_> = (0..n)
                .map(|i| mask[c][i] * (sparse[c][i] - predicted[i]))
                .collect();
            for i in 0..n {
                if sites[i] != partner[c] {
                    continue;
                }
                let estimate = predicted[i]
                    + 0.5 * (residual[grid.shift(i, axis, -1)] + residual[grid.shift(i, axis, 1)]);
                difference[i] = if c == 1 || c == 2 {
                    estimate - f64::from(mosaic[i])
                } else {
                    f64::from(mosaic[i]) - estimate
                };
            }
        }
    }
    let gradients: [Vec<f64>; 2] = std::array::from_fn(|a| {
        grid.gaussian(
            &(0..n)
                .map(|i| {
                    (differences[a][grid.shift(i, a, 1)] - differences[a][grid.shift(i, a, -1)])
                        .abs()
                })
                .collect::<Vec<_>>(),
        )
    });
    let mut green = vec![0.; n];
    for i in 0..n {
        if cfa.color(i % w, i / w) == 1 {
            green[i] = f64::from(mosaic[i]);
            continue;
        }
        let (mut value, mut weight) = (0., 0.);
        for axis in 0..2 {
            for sign in [-1, 1] {
                let g = gradients[axis][grid.shift(i, axis, sign * if laplacian { 1 } else { 2 })];
                let factor = 1. / (g * g + EPSILON);
                let estimate: f64 = [0.26, 0.24, 0.21, 0.17, 0.12]
                    .iter()
                    .enumerate()
                    .map(|(j, v)| v * differences[axis][grid.shift(i, axis, sign * j as isize)])
                    .sum();
                value += factor * estimate;
                weight += factor;
            }
        }
        green[i] = f64::from(mosaic[i]) + value / weight;
    }
    let mut output: Vec<_> = green.iter().map(|&g| [0., g as f32, 0.]).collect();
    for (channel, plane) in [(0, 0), (2, 3)] {
        let prediction = guided(
            grid,
            &green,
            &sparse[plane],
            &mask[plane],
            [5, 5],
            laplacian,
            None,
        );
        for i in 0..n {
            if sites[i] == plane {
                output[i][channel] = mosaic[i];
                continue;
            }
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            let mut residual = 0.;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let k = grid.at(x + dx, y + dy);
                    if sites[k] != plane {
                        continue;
                    }
                    let weight = if dx == 0 { 1. } else { 0.5 } * if dy == 0 { 1. } else { 0.5 };
                    residual += weight * (f64::from(mosaic[k]) - prediction[k]);
                }
            }
            output[i][channel] = (prediction[i] + residual) as f32;
        }
    }
    output
}

#[test]
fn sliding_boxes_match_direct_reflected_sums() {
    for (w, h) in [(1, 1), (2, 3), (17, 13)] {
        let grid = Grid { w, h };
        let input: Vec<_> = (0..w * h)
            .map(|i| (i * 127 % 997) as f64 / 400. - 0.2)
            .collect();
        for radii in [[0, 5], [5, 0], [3, 3], [5, 5]] {
            let result = grid.box_mean(&input, radii);
            for (i, &actual) in result.iter().enumerate() {
                let mut sum = 0.;
                for dy in -(radii[1] as isize)..=radii[1] as isize {
                    for dx in -(radii[0] as isize)..=radii[0] as isize {
                        sum += input[grid.at((i % w) as isize + dx, (i / w) as isize + dy)];
                    }
                }
                assert!(
                    (actual - sum / ((2 * radii[0] + 1) * (2 * radii[1] + 1)) as f64).abs() < 1e-12
                );
            }
        }
    }
}

#[test]
fn reconstruction_commutes_with_transpose_and_red_blue_exchange() {
    let (w, h) = (31, 27);
    for phase in 0..4 {
        let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
        let transposed = Cfa::bayer([
            cfa.color(0, 0),
            cfa.color(0, 1),
            cfa.color(1, 0),
            cfa.color(1, 1),
        ]);
        let exchanged = Cfa::bayer(
            [
                cfa.color(0, 0),
                cfa.color(1, 0),
                cfa.color(0, 1),
                cfa.color(1, 1),
            ]
            .map(|c| 2 - c),
        );
        let mosaic: Vec<_> = (0..w * h)
            .map(|i| 0.4 + 0.7 * ((i % w) as f32 * 0.31 + (i / w) as f32 * 0.27).sin())
            .collect();
        let rotated: Vec<_> = (0..w * h).map(|i| mosaic[(i % h) * w + i / h]).collect();
        for lap in [false, true] {
            let a = reconstruct(&mosaic, w, h, cfa, lap);
            let b = reconstruct(&rotated, h, w, transposed, lap);
            let c = reconstruct(&mosaic, w, h, exchanged, lap);
            for i in 0..w * h {
                for channel in 0..3 {
                    assert!((a[i][channel] - b[(i % w) * h + i / w][channel]).abs() < 2e-6);
                    assert!((a[i][channel] - c[i][2 - channel]).abs() < 2e-6);
                }
            }
        }
    }
}

#[test]
fn both_stages_preserve_extended_flats_and_sites_in_all_phases() {
    for (w, h) in [(2, 3), (7, 9), (31, 27)] {
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
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
                for lap in [false, true] {
                    let rgb = reconstruct(&mosaic, w, h, cfa, lap);
                    for (i, p) in rgb.iter().enumerate() {
                        let c = cfa.color(i % w, i / w);
                        assert_eq!(p[c].to_bits(), mosaic[i].to_bits());
                        assert!(p.iter().all(|v| v.is_finite()));
                        if flat {
                            for (v, reference) in p.iter().zip([-0.1, 0.4, 1.3]) {
                                assert!(
                                    (v - reference).abs() < 1e-6,
                                    "phase={phase} lap={lap} pixel={i}: {p:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
