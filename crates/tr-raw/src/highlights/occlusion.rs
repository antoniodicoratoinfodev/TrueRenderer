//! Continue a supported illumination plane up to a darker occluding object.
//! Only original samples constrain the fit; inferred cells never seed others.
use super::{Estimate, Grid, Model, Reconstruction, SATURATED, SIDE};
use crate::color::{inverse, vector};

pub(super) fn estimate(grid: &Grid<'_>, estimates: &[Estimate], tx: usize, ty: usize) -> Estimate {
    let i = ty * grid.columns + tx;
    if grid.data[i].clipped == 0 {
        return None;
    }
    let (x0, y0) = (
        (tx * SIDE).min(grid.width - SIDE),
        (ty * SIDE).min(grid.height - SIDE),
    );
    let mut accepted: Option<Reconstruction> = None;
    let mut count = 0;
    for ny in ty.saturating_sub(6)..=(ty + 6).min(grid.height.div_ceil(SIDE) - 1) {
        for nx in tx.saturating_sub(6)..=(tx + 6).min(grid.columns - 1) {
            let Some(mut reference) = estimates[ny * grid.columns + nx] else {
                continue;
            };
            if reference.extended || reference.model.split != 0 {
                continue;
            }
            let (rx, ry) = (
                (nx * SIDE).min(grid.width - SIDE),
                (ny * SIDE).min(grid.height - SIDE),
            );
            reference.model.plane[0] += reference.model.plane[1] * (x0 as f32 - rx as f32) / 2.5
                + reference.model.plane[2] * (y0 as f32 - ry as f32) / 2.5;
            reference.model.border = 0;
            let Some(candidate) = fit(grid, x0, y0, reference) else {
                continue;
            };
            if let Some(previous) = accepted {
                for y in 0..SIDE {
                    for x in 0..SIDE {
                        for c in 0..3 {
                            if !super::ratio_agrees(
                                previous.model.predict(x, y) * previous.ratio[c],
                                candidate.model.predict(x, y) * candidate.ratio[c],
                            ) {
                                return None;
                            }
                        }
                    }
                }
            } else {
                accepted = Some(candidate);
            }
            count += 1;
        }
    }
    if count >= 2 { accepted } else { None }
}

fn predict(r: Reconstruction, dx: f64, dy: f64, c: usize) -> f64 {
    let p = r.model.plane.map(f64::from);
    (p[0] + p[1] * dx + p[2] * dy) * f64::from(r.ratio[c])
}

pub(super) fn refit(grid: &Grid<'_>, tx: usize, ty: usize, reference: Reconstruction) -> Estimate {
    fit(
        grid,
        (tx * SIDE).min(grid.width - SIDE),
        (ty * SIDE).min(grid.height - SIDE),
        reference,
    )
}

fn fit(grid: &Grid<'_>, x0: usize, y0: usize, reference: Reconstruction) -> Estimate {
    let (left, top) = (x0.saturating_sub(6), y0.saturating_sub(6));
    let (right, bottom) = ((x0 + 12).min(grid.width), (y0 + 12).min(grid.height));
    let mut normal = [[0.; 3]; 3];
    let mut rhs = [0.; 3];
    let mut support = [0usize; 3];
    for y in top..bottom {
        for x in left..right {
            let i = y * grid.stride + x;
            if grid.state[i] == SATURATED {
                continue;
            }
            let c = grid.cfa.color(x, y);
            let xy = [
                1.,
                (x as f64 - x0 as f64 - 2.5) / 2.5,
                (y as f64 - y0 as f64 - 2.5) / 2.5,
            ];
            let expected = predict(reference, xy[1], xy[2], c);
            let value = f64::from(grid.values[i]);
            if expected <= 0.01 || value < 0. {
                return None;
            }
            if (value - expected).abs() <= 0.025 * expected {
                support[c] += 1;
                let r = f64::from(reference.ratio[c]);
                for j in 0..3 {
                    rhs[j] += xy[j] * r * value;
                    for k in 0..3 {
                        normal[j][k] += xy[j] * xy[k] * r * r;
                    }
                }
            } else if value >= 0.5 * expected {
                return None;
            }
        }
    }
    if support.iter().sum::<usize>() < 12 || support.iter().all(|&n| n < 6) {
        return None;
    }
    let plane = vector(inverse(normal).ok()?, rhs).map(|v| v as f32);
    if plane[0] <= 0.01 || plane[1].abs() + plane[2].abs() > 0.5 * plane[0] {
        return None;
    }
    let result = Reconstruction {
        model: Model {
            plane,
            border: 0,
            ..reference.model
        },
        extended: true,
        ..reference
    };
    let mut visible = [[false; 18]; 18];
    for y in top..bottom {
        for x in left..right {
            let i = y * grid.stride + x;
            let c = grid.cfa.color(x, y);
            let dx = (x as f64 - x0 as f64 - 2.5) / 2.5;
            let dy = (y as f64 - y0 as f64 - 2.5) / 2.5;
            let expected = predict(result, dx, dy, c);
            let value = f64::from(grid.values[i]);
            if !expected.is_finite() || expected <= 0.01 {
                return None;
            }
            if grid.state[i] == SATURATED {
                if expected < 0.98 * value || expected > 4. * value {
                    return None;
                }
                visible[y - top][x - left] = true;
            } else {
                let prior = predict(reference, dx, dy, c);
                if (value - prior).abs() <= 0.025 * prior {
                    if (value - expected).abs() > 0.025 * expected {
                        return None;
                    }
                    visible[y - top][x - left] = true;
                } else if value >= 0.5 * expected {
                    return None;
                }
            }
        }
    }
    monotone(&visible, right - left, bottom - top).then_some(result)
}

// A darker object must occupy one side of a coherent boundary, not alternate
// with CFA colours. Curvature is allowed; unobserved islands are not bridged.
fn monotone(mask: &[[bool; 18]; 18], width: usize, height: usize) -> bool {
    for axis in 0..2 {
        for increasing in [false, true] {
            let (lines, length) = if axis == 0 {
                (height, width)
            } else {
                (width, height)
            };
            let valid = (0..lines).all(|line| {
                (1..length).all(|p| {
                    let get = |k: usize| {
                        if axis == 0 {
                            mask[line][k]
                        } else {
                            mask[k][line]
                        }
                    };
                    if increasing {
                        !get(p - 1) || get(p)
                    } else {
                        get(p - 1) || !get(p)
                    }
                })
            });
            if valid {
                return true;
            }
        }
    }
    false
}
