//! A measured plateau can contradict extrapolation of the affine cell fit.
//! Recognise a continuous ramp-to-plateau from original surviving colours;
//! use its plateau only when every censored site in the target is on it.
use super::{Grid, Model, Reconstruction, SATURATED, SIDE};
use crate::color::{inverse, vector};

pub(super) fn plateau(grid: &Grid<'_>, tx: usize, ty: usize, r: Reconstruction) -> Option<Model> {
    if grid.cfa.period != 2 || r.extended || r.model.split != 0 || r.model.border != 0 {
        return None;
    }
    let refs = grid.data[ty * grid.columns + tx].valid;
    if refs.count_ones() < 2 {
        return None;
    }
    let (x0, y0) = (
        (tx * SIDE).min(grid.width - SIDE),
        (ty * SIDE).min(grid.height - SIDE),
    );
    let lifts = (ty * SIDE..((ty + 1) * SIDE).min(grid.height)).any(|y| {
        (tx * SIDE..((tx + 1) * SIDE).min(grid.width)).any(|x| {
            let i = y * grid.stride + x;
            grid.state[i] == SATURATED
                && r.model.predict(x - x0, y - y0) * r.ratio[grid.cfa.color(x, y)]
                    > 1.0001 * grid.values[i]
        })
    });
    if !lifts {
        return None;
    }
    let low = [x0.saturating_sub(SIDE), y0.saturating_sub(SIDE)];
    let high = [
        (x0 + 2 * SIDE).min(grid.width),
        (y0 + 2 * SIDE).min(grid.height),
    ];
    // Leave already coherent affine fits exactly unchanged. The tolerance is
    // the existing affine-demosaic admission policy, not a sensor noise model.
    let contradicts = (low[1]..high[1]).any(|y| {
        (low[0]..high[0]).any(|x| {
            let i = y * grid.stride + x;
            let c = grid.cfa.color(x, y);
            let p = r.model.plane;
            let expected = (p[0]
                + p[1] * (x as f32 - x0 as f32 - 2.5) / 2.5
                + p[2] * (y as f32 - y0 as f32 - 2.5) / 2.5)
                * r.ratio[c];
            grid.state[i] != SATURATED
                && (expected - grid.values[i]).abs()
                    > 0.001 * expected.abs().max(grid.values[i].abs()).max(0.01)
        })
    });
    if !contradicts {
        return None;
    }
    let mut best: Option<(Model, f64)> = None;
    for axis in 0..2 {
        if high[axis] - low[axis] < 9 {
            continue;
        }
        for cut in low[axis] + 4..high[axis] - 3 {
            for right in [false, true] {
                let on_plateau = |x: usize, y: usize| ([x, y][axis] >= cut) == right;
                if (ty * SIDE..((ty + 1) * SIDE).min(grid.height)).any(|y| {
                    (tx * SIDE..((tx + 1) * SIDE).min(grid.width))
                        .any(|x| grid.state[y * grid.stride + x] == SATURATED && !on_plateau(x, y))
                }) {
                    continue;
                }
                // A small residual alone cannot distinguish a plateau from
                // the top of a rounded light. Require directly observed flat
                // runs of each surviving reference colour along this axis.
                // Without a measured noise model, do not loosen this exact
                // equality for noisy or spatially varying calibration data.
                let mut flat_pairs = [0usize; 3];
                let flat = (low[1]..high[1]).all(|y| {
                    (low[0]..high[0]).all(|x| {
                        let c = grid.cfa.color(x, y);
                        let mut q = [x, y];
                        q[axis] += 2;
                        if refs & (1 << c) == 0
                            || q[axis] >= high[axis]
                            || !on_plateau(x, y)
                            || !on_plateau(q[0], q[1])
                        {
                            return true;
                        }
                        let (i, j) = (y * grid.stride + x, q[1] * grid.stride + q[0]);
                        if grid.state[i] == SATURATED || grid.state[j] == SATURATED {
                            return true;
                        }
                        flat_pairs[c] += 1;
                        grid.values[i].to_bits() == grid.values[j].to_bits()
                    })
                });
                if !flat || (0..3).any(|c| refs & (1 << c) != 0 && flat_pairs[c] < 6) {
                    continue;
                }
                let mut normal = [[[0.; 3]; 3]; 2];
                let mut rhs = [[0.; 3]; 2];
                let mut counts = [[0usize; 3]; 2];
                let mut minima = [[[usize::MAX; 2]; 3]; 2];
                let mut maxima = [[[0usize; 2]; 3]; 2];
                for y in low[1]..high[1] {
                    for x in low[0]..high[0] {
                        let i = y * grid.stride + x;
                        if grid.state[i] == SATURATED {
                            continue;
                        }
                        let c = grid.cfa.color(x, y);
                        let side = usize::from(on_plateau(x, y));
                        let mut v = [
                            1.,
                            (x as f64 - x0 as f64 - 2.5) / 2.5,
                            (y as f64 - y0 as f64 - 2.5) / 2.5,
                        ];
                        if side == 1 {
                            v[axis + 1] = 0.;
                        }
                        let ratio = f64::from(r.ratio[c]);
                        for j in 0..3 {
                            rhs[side][j] += v[j] * ratio * f64::from(grid.values[i]);
                            for k in 0..3 {
                                normal[side][j][k] += v[j] * v[k] * ratio * ratio;
                            }
                        }
                        counts[side][c] += 1;
                        for (a, coordinate) in [x, y].into_iter().enumerate() {
                            minima[side][c][a] = minima[side][c][a].min(coordinate);
                            maxima[side][c][a] = maxima[side][c][a].max(coordinate);
                        }
                    }
                }
                if (0..2).any(|s| {
                    (0..3).any(|c| {
                        refs & (1 << c) != 0
                            && (counts[s][c] < 6
                                || (0..2)
                                    .any(|a| maxima[s][c][a].saturating_sub(minima[s][c][a]) < 4))
                    })
                }) {
                    continue;
                }
                normal[1][axis + 1][axis + 1] = 1.;
                let (Ok(a), Ok(b)) = (inverse(normal[0]), inverse(normal[1])) else {
                    continue;
                };
                let planes = [vector(a, rhs[0]), vector(b, rhs[1])];
                let slope = planes[0][axis + 1];
                let scale = planes[1][0].abs().max(0.01);
                if !planes.iter().flatten().all(|v| v.is_finite())
                    || slope * if right { 1. } else { -1. } <= 0.001 * scale
                {
                    continue;
                }
                // This model is qualified for an axial shoulder. A moving
                // knee can pass the residual tolerance in a few cells and
                // create a partially corrected oblique boundary. Require
                // matching transverse slopes over the complete window.
                let transverse_span = (high[1 - axis] - low[1 - axis] - 1) as f64 / 2.5;
                if (planes[0][2 - axis] - planes[1][2 - axis]).abs() * transverse_span
                    > 0.001 * scale
                {
                    continue;
                }
                // The planes must meet between the two adjacent sampled
                // coordinates, with uncertainty implied by the fit tolerance.
                let continuous = [low[1 - axis], high[1 - axis] - 1].into_iter().all(|q| {
                    let cross = (q as f64 - [x0, y0][1 - axis] as f64 - 2.5) / 2.5;
                    let knee = (planes[1][0] - planes[0][0]
                        + (planes[1][2 - axis] - planes[0][2 - axis]) * cross)
                        / slope;
                    let coordinate = [x0, y0][axis] as f64 + 2.5 + 2.5 * knee;
                    let tolerance = 2.5 * 0.001 * scale / slope.abs();
                    coordinate >= cut as f64 - 1. - tolerance
                        && coordinate <= cut as f64 + tolerance
                });
                if !continuous {
                    continue;
                }
                let mut score = 0.;
                let valid = (low[1]..high[1]).all(|y| {
                    (low[0]..high[0]).all(|x| {
                        let i = y * grid.stride + x;
                        let p = planes[usize::from(on_plateau(x, y))];
                        let expected = (p[0]
                            + p[1] * (x as f64 - x0 as f64 - 2.5) / 2.5
                            + p[2] * (y as f64 - y0 as f64 - 2.5) / 2.5)
                            * f64::from(r.ratio[grid.cfa.color(x, y)]);
                        let measured = f64::from(grid.values[i]);
                        let tolerance = 0.001 * expected.abs().max(measured.abs()).max(0.01);
                        if grid.state[i] == SATURATED {
                            expected >= measured - tolerance && expected <= 4. * measured
                        } else {
                            score += (expected - measured).powi(2);
                            (expected - measured).abs() <= tolerance
                        }
                    })
                });
                if valid && best.is_none_or(|(_, previous)| score < previous) {
                    best = Some((
                        Model {
                            plane: planes[1].map(|v| v as f32),
                            ..r.model
                        },
                        score,
                    ));
                }
            }
        }
    }
    best.map(|(model, _)| model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfa::Cfa;

    #[test]
    fn sampled_shoulders_do_not_amplify_a_near_clipping_plateau() {
        let (w, h) = (96, 60);
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
            for axis in 0..2 {
                for reverse in [false, true] {
                    for knee in [27., 27.35, 28.7, 29., 30.2] {
                        let truth: Vec<_> = (0..w * h)
                            .map(|i| {
                                let p = [i % w, i / w][axis] as f32;
                                let p = if reverse {
                                    [w, h][axis] as f32 - 1. - p
                                } else {
                                    p
                                };
                                [0.65, 0.82, 1.][cfa.color(i % w, i / w)]
                                    * (1.00008 + 0.009 * (p - knee)).min(1.00008)
                            })
                            .collect();
                        let mut values: Vec<_> = truth
                            .iter()
                            .map(|v| (v * 16383.).round().min(16383.) / 16383.)
                            .collect();
                        let original = values.clone();
                        let state: Vec<_> = values
                            .iter()
                            .map(|v| {
                                if *v == 1. {
                                    SATURATED
                                } else if *v >= 0.98 {
                                    1
                                } else {
                                    0
                                }
                            })
                            .collect();
                        super::super::recover(&mut values, &state, w, w, h, cfa);
                        for (i, (&v, &expected)) in values.iter().zip(&truth).enumerate() {
                            if state[i] == SATURATED {
                                assert!(
                                    (v - expected).abs() < 0.00025,
                                    "phase={phase} axis={axis} reverse={reverse} knee={knee} site={i} got={v} truth={expected}"
                                );
                            } else {
                                assert_eq!(v.to_bits(), original[i].to_bits());
                            }
                        }
                    }
                }
            }
        }
    }
}
