//! Bounded piecewise-affine luminance models, used only when the plane fails.
use super::{Cell, SIDE, coordinates};
use crate::cfa::Cfa;

/// Fit illumination using every surviving measurement, with the colour ratios
/// fixed by the independently selected source cells. Partly clipped channels
/// constrain the edge and bright valid channels reduce prediction variance.
pub(super) fn refine(
    values: &[f32],
    state: &[u8],
    stride: usize,
    window: [usize; 3],
    cfa: Cfa,
    ratio: [f32; 3],
    initial: Option<super::Model>,
) -> Option<super::Model> {
    let [x0, y0, side] = window;
    if let Some(model) = initial
        && validate(values, state, stride, window, cfa, model, ratio).is_some()
    {
        return Some(model);
    }
    let mut candidates = Vec::new();
    for split in 0..=(2 * (side - 1)) as u8 {
        let mut normal = [[0.; 4]; 4];
        let mut rhs = [0.; 4];
        let mut low = f64::INFINITY;
        for y in 0..side {
            for x in 0..side {
                let i = (y0 + y) * stride + x0 + x;
                if state[i] == super::SATURATED {
                    continue;
                }
                let c = cfa.color(x, y);
                let r = f64::from(ratio[c]);
                let [a, b, d] = xy(x, y, side);
                let v = [a, b, d, basis(split, x, y, side)];
                let measured = f64::from(values[i]);
                low = low.min(measured / r);
                for j in 0..4 {
                    rhs[j] += v[j] * r * measured;
                    for k in 0..4 {
                        normal[j][k] += v[j] * v[k] * r * r;
                    }
                }
            }
        }
        if split == 0 {
            normal[3][3] = 1.;
        }
        let Some(p) = solve(normal, rhs) else {
            continue;
        };
        if low <= 0.005 || p[0] <= 0.01 || p[1].abs() + p[2].abs() > 0.5 * p[0] || p[3].abs() > low
        {
            continue;
        }
        let model = super::Model {
            plane: [p[0] as f32, p[1] as f32, p[2] as f32],
            jump: p[3] as f32,
            split,
            split2: split,
            side: side as u8,
            border: 0,
            correction: [0.; 2],
        };
        if let Some(mut score) = validate(values, state, stride, window, cfa, model, ratio) {
            score += 0.0001 * (p[1].abs() + p[2].abs()).powi(2) / p[0].powi(2);
            if split == 0 {
                return Some(model);
            }
            candidates.push((model, score));
        }
    }
    let &(mut best, score) = candidates.iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    for (candidate, other) in candidates {
        if candidate.split != best.split
            && candidate.split.abs_diff(best.split) == 1
            && (candidate.split - 1) / (side as u8 - 1) == (best.split - 1) / (side as u8 - 1)
            && (other - score).abs() < 1e-12
            && (candidate.jump - best.jump).abs() < 1e-5
            && (1..3).all(|k| (candidate.plane[k] - best.plane[k]).abs() < 1e-5)
        {
            best.split2 = candidate.split;
            best.plane = std::array::from_fn(|k| (best.plane[k] + candidate.plane[k]) * 0.5);
            best.jump = (best.jump + candidate.jump) * 0.5;
            break;
        }
    }
    validate(values, state, stride, window, cfa, best, ratio).map(|_| best)
}

pub(super) fn with_halo(
    grid: &super::Grid<'_>,
    tx: usize,
    ty: usize,
    model: super::Model,
    ratio: [f32; 3],
) -> super::Model {
    if grid.cfa.period != 2 || model.split != 0 {
        return model;
    }
    let window = super::reconstruction_window(grid.width, grid.height, grid.cfa, tx, ty);
    let [hx, hy, side] = window;
    if side == SIDE {
        return model;
    }
    let Some(halo) = refine(
        grid.values,
        grid.state,
        grid.stride,
        window,
        grid.cfa.shifted(hx, hy),
        ratio,
        None,
    ) else {
        return model;
    };
    if halo.split == 0 {
        return model;
    }
    let axis = usize::from(halo.split - 1) / (side - 1);
    if usize::from(halo.split2 - 1) / (side - 1) != axis {
        return model;
    }
    let origin = [
        (tx * SIDE).min(grid.width - SIDE),
        (ty * SIDE).min(grid.height - SIDE),
    ];
    let halo_origin = [hx, hy];
    let cuts = [halo.split, halo.split2]
        .map(|s| halo_origin[axis] + (usize::from(s) - 1) % (side - 1) + 1);
    let border = if cuts
        .iter()
        .all(|c| *c >= origin[axis] && *c <= origin[axis] + 1)
    {
        1 + 2 * axis
    } else if cuts
        .iter()
        .all(|c| *c >= origin[axis] + SIDE - 1 && *c <= origin[axis] + SIDE)
    {
        2 + 2 * axis
    } else {
        return model;
    };
    let along = if border % 2 == 1 { 0 } else { SIDE - 1 };
    let delta = [0, SIDE - 1].map(|cross| {
        let [x, y] = if axis == 0 {
            [along, cross]
        } else {
            [cross, along]
        };
        halo.predict(origin[0] + x - hx, origin[1] + y - hy) - model.predict(x, y)
    });
    let candidate = super::Model {
        border: border as u8,
        correction: [(delta[0] + delta[1]) * 0.5, (delta[1] - delta[0]) * 0.5],
        ..model
    };
    if validate(
        grid.values,
        grid.state,
        grid.stride,
        [origin[0], origin[1], SIDE],
        grid.cfa.shifted(origin[0], origin[1]),
        candidate,
        ratio,
    )
    .is_some()
    {
        candidate
    } else {
        model
    }
}

fn validate(
    values: &[f32],
    state: &[u8],
    stride: usize,
    window: [usize; 3],
    cfa: Cfa,
    model: super::Model,
    ratio: [f32; 3],
) -> Option<f64> {
    let [x0, y0, side] = window;
    let mut squared = [0.; 3];
    let mut scale = [0.; 3];
    for y in 0..side {
        for x in 0..side {
            let i = (y0 + y) * stride + x0 + x;
            let c = cfa.color(x, y);
            let expected = model.predict(x, y) * ratio[c];
            let measured = values[i];
            if !expected.is_finite() {
                return None;
            }
            if state[i] == super::SATURATED {
                if expected < 0.98 * measured || expected > 4. * measured {
                    return None;
                }
            } else {
                if !super::ratio_agrees(expected, measured) {
                    return None;
                }
                squared[c] += f64::from(expected - measured).powi(2);
                scale[c] += f64::from(measured).powi(2);
            }
        }
    }
    let mut score = 0.;
    for c in 0..3 {
        if scale[c] > 0. {
            let residual = squared[c] / scale[c];
            if residual > 0.025_f64.powi(2) {
                return None;
            }
            score += residual;
        }
    }
    Some(score)
}

pub(super) fn xy(x: usize, y: usize, side: usize) -> [f64; 3] {
    let centre = (side - 1) as f64 * 0.5;
    [
        1.,
        (x as f64 - centre) / centre,
        (y as f64 - centre) / centre,
    ]
}

pub(super) fn basis(split: u8, x: usize, y: usize, side: usize) -> f64 {
    if split == 0 {
        return 0.;
    }
    let k = (usize::from(split) - 1) % (side - 1) + 1;
    let p = if usize::from(split) < side { x } else { y };
    f64::from(p >= k) - f64::from((side - 1) as f64 * 0.5 >= k as f64)
}

fn solve(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for k in 0..4 {
        let pivot = (k..4).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[pivot][k].abs() < 1e-8 {
            return None;
        }
        a.swap(k, pivot);
        b.swap(k, pivot);
        let d = a[k][k];
        for v in &mut a[k][k..] {
            *v /= d;
        }
        b[k] /= d;
        for i in 0..4 {
            if i == k {
                continue;
            }
            let f = a[i][k];
            let pivot_row = a[k];
            for (v, p) in a[i][k..].iter_mut().zip(&pivot_row[k..]) {
                *v -= f * p;
            }
            b[i] -= f * b[k];
        }
    }
    b.iter().all(|v| v.is_finite()).then_some(b)
}

pub(super) fn fit(
    values: &[f32],
    state: &[u8],
    stride: usize,
    x0: usize,
    y0: usize,
    cfa: Cfa,
    previous: Cell,
) -> Cell {
    let mut best = previous;
    let mut best_score = f64::INFINITY;
    let mut alternatives = Vec::new();
    for split in 1..=10 {
        let mut normal = [[[0.; 4]; 4]; 3];
        let mut rhs = [[0.; 4]; 3];
        let mut unsafe_channels = 0u8;
        let mut low = [f64::INFINITY; 3];
        for y in 0..SIDE {
            for x in 0..SIDE {
                let c = cfa.color(x, y);
                let i = (y0 + y) * stride + x0 + x;
                let value = f64::from(values[i]);
                let [a, b, d] = coordinates(x, y);
                let v = [a, b, d, basis(split, x, y, SIDE)];
                for r in 0..4 {
                    rhs[c][r] += value * v[r];
                    for k in 0..4 {
                        normal[c][r][k] += v[r] * v[k];
                    }
                }
                low[c] = low[c].min(value);
                if state[i] > 0 {
                    unsafe_channels |= 1 << c;
                }
            }
        }
        let safe = !unsafe_channels & 7;
        if safe == 0 {
            return previous;
        }
        let mut fits = [[0.; 4]; 3];
        let mut good = true;
        for c in 0..3 {
            if let Some(p) = solve(normal[c], rhs[c]) {
                fits[c] = p;
            } else if safe & (1 << c) != 0 {
                // A channel may sample only the centre's side of a border
                // step. Its existing affine fit still identifies that side;
                // do not turn the whole cell into an artificial path barrier.
                if previous.valid & (1 << c) != 0 {
                    let p = previous.plane[c].map(f64::from);
                    fits[c] = [p[0], p[1], p[2], 0.];
                } else {
                    good = false;
                }
            }
        }
        if !good {
            continue;
        }
        let mut residuals = [0.; 3];
        let mut counts = [0.; 3];
        for y in 0..SIDE {
            for x in 0..SIDE {
                let c = cfa.color(x, y);
                let p = fits[c];
                let [_, dx, dy] = coordinates(x, y);
                let prediction = p[0] + p[1] * dx + p[2] * dy + p[3] * basis(split, x, y, SIDE);
                residuals[c] +=
                    (f64::from(values[(y0 + y) * stride + x0 + x]) - prediction).powi(2);
                counts[c] += 1.;
            }
        }
        let mut score = 0.;
        for c in 0..3 {
            if safe & (1 << c) == 0 {
                continue;
            }
            let p = fits[c];
            let residual = (residuals[c] / counts[c]).sqrt();
            if low[c] <= 0.005
                || p[0] <= 0.01
                || residual > 0.025 * p[0]
                || p[1].abs() + p[2].abs() > 0.5 * p[0]
                || p[3].abs() > low[c]
            {
                good = false;
            }
            score += (residual / p[0]).powi(2)
                + 0.0001 * (p[1].abs() + p[2].abs()).powi(2) / p[0].powi(2);
        }
        // Partly clipped channels still contain real samples. They constrain
        // the common edge position even when they cannot fit their own plane.
        // Ignoring them can shift an edge by one CFA site.
        let anchor = (0..3)
            .filter(|c| safe & (1 << c) != 0)
            .min_by(|a, b| fits[*a][0].total_cmp(&fits[*b][0]))
            .unwrap();
        let mut aa = [0.; 3];
        let mut av = [0.; 3];
        let mut vv = [0.; 3];
        let mut observed = [0; 3];
        for y in 0..SIDE {
            for x in 0..SIDE {
                let i = (y0 + y) * stride + x0 + x;
                if state[i] == super::SATURATED {
                    continue;
                }
                let c = cfa.color(x, y);
                let p = fits[anchor];
                let [_, dx, dy] = coordinates(x, y);
                let a = p[0] + p[1] * dx + p[2] * dy + p[3] * basis(split, x, y, SIDE);
                let v = f64::from(values[i]);
                aa[c] += a * a;
                av[c] += a * v;
                vv[c] += v * v;
                observed[c] += 1;
            }
        }
        for c in 0..3 {
            if observed[c] < 2 {
                continue;
            }
            if aa[c] <= 0. || vv[c] <= 0. {
                good = false;
                continue;
            }
            let relative = (vv[c] - av[c] * av[c] / aa[c]).max(0.) / vv[c];
            if relative > 0.025_f64.powi(2) {
                good = false;
            }
            score += relative;
        }
        if good {
            let mut candidate = previous;
            candidate.valid = safe;
            candidate.split = split;
            candidate.split2 = split;
            for (c, p) in fits.iter().enumerate() {
                if safe & (1 << c) != 0 {
                    candidate.plane[c] = [p[0] as f32, p[1] as f32, p[2] as f32];
                    candidate.jump[c] = p[3] as f32;
                }
            }
            if score < best_score {
                best = candidate;
                best_score = score;
            }
            alternatives.push((candidate, score));
        }
    }
    // When the CFA cannot locate a step between two adjacent sites, use the
    // midpoint of the equivalent models rather than arbitrarily shifting it.
    for (candidate, score) in alternatives {
        if candidate.split != best.split
            && candidate.split.abs_diff(best.split) == 1
            && (candidate.split - 1) / 5 == (best.split - 1) / 5
            && (score - best_score).abs() < 1e-12
            && (0..3).filter(|c| best.valid & (1 << c) != 0).all(|c| {
                (candidate.jump[c] - best.jump[c]).abs() < 1e-5
                    && (1..3).all(|k| (candidate.plane[c][k] - best.plane[c][k]).abs() < 1e-5)
            })
        {
            best.split2 = candidate.split;
            for c in 0..3 {
                for k in 0..3 {
                    best.plane[c][k] = (best.plane[c][k] + candidate.plane[c][k]) * 0.5;
                }
            }
            break;
        }
    }
    best
}
