//! A measurement-consistent two-region interpolant for sharp X-Trans edges.
//! Independent channel levels: no neutral or constant-hue assumption.
use crate::cfa::Cfa;

const RADIUS: usize = 4;

#[derive(Clone, Copy, Default)]
struct Moments {
    count: usize,
    sum: f64,
    square: f64,
}

impl Moments {
    fn add(&mut self, value: f32) {
        let value = f64::from(value);
        self.count += 1;
        self.sum += value;
        self.square += value * value;
    }

    fn subtract(self, rhs: Self) -> Self {
        Self {
            count: self.count - rhs.count,
            sum: self.sum - rhs.sum,
            square: self.square - rhs.square,
        }
    }
}

fn edge_sites(mosaic: &[f32], w: usize, h: usize) -> Vec<bool> {
    let mut sites = vec![false; mosaic.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            for j in [
                (x + 6 < w).then_some(i + 6),
                (y + 6 < h).then_some(i + 6 * w),
            ]
            .into_iter()
            .flatten()
            {
                let (a, b) = (mosaic[i], mosaic[j]);
                if (a - b).abs() > 0.1 * a.abs().max(b.abs()).max(0.05) {
                    sites[i] = true;
                    sites[j] = true;
                }
            }
        }
    }
    sites
}

fn interpolate(
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    x: usize,
    y: usize,
) -> Option<[f32; 3]> {
    let (x0, y0) = (x.saturating_sub(RADIUS), y.saturating_sub(RADIUS));
    let (x1, y1) = ((x + RADIUS + 1).min(w), (y + RADIUS + 1).min(h));
    let mut result: Option<[f32; 3]> = None;
    for axis in 0..2 {
        // Necessary condition for two constant regions: along the tangent,
        // same-phase pairs cannot differ beyond the model residual. Reject
        // ordinary texture/noise before building and solving every split.
        let tangent_is_flat = (y0..y1).all(|yy| {
            (x0..x1).all(|xx| {
                let (nx, ny) = if axis == 0 {
                    (xx, yy + 6)
                } else {
                    (xx + 6, yy)
                };
                if nx >= x1 || ny >= y1 {
                    return true;
                }
                let (a, b) = (mosaic[yy * w + xx], mosaic[ny * w + nx]);
                (a - b).abs() <= 0.002 * a.abs().max(b.abs()).max(0.01)
            })
        });
        if !tangent_is_flat {
            continue;
        }
        let length = if axis == 0 { x1 - x0 } else { y1 - y0 };
        let mut lines = [[Moments::default(); 3]; 2 * RADIUS + 1];
        let mut total = [Moments::default(); 3];
        for yy in y0..y1 {
            for xx in x0..x1 {
                let c = cfa.color(xx, yy);
                let value = mosaic[yy * w + xx];
                lines[if axis == 0 { xx - x0 } else { yy - y0 }][c].add(value);
                total[c].add(value);
            }
        }
        let mut left = [Moments::default(); 3];
        for cut in 1..length {
            for (c, item) in left.iter_mut().enumerate() {
                item.count += lines[cut - 1][c].count;
                item.sum += lines[cut - 1][c].sum;
                item.square += lines[cut - 1][c].square;
            }
            let regions = [left, std::array::from_fn(|c| total[c].subtract(left[c]))];
            if regions.iter().flatten().any(|m| m.count < 2) {
                continue;
            }
            let levels = regions.map(|r| r.map(|m| (m.sum / m.count as f64) as f32));
            let tolerance: [f32; 3] = std::array::from_fn(|c| {
                0.001 * levels[0][c].abs().max(levels[1][c].abs()).max(0.01)
            });
            if !(0..3).any(|c| (levels[0][c] - levels[1][c]).abs() > 100. * tolerance[c]) {
                continue;
            }
            if (0..2).any(|r| {
                (0..3).any(|c| {
                    let m = regions[r][c];
                    let variance =
                        (m.square - m.sum * m.sum / m.count as f64).max(0.) / m.count as f64;
                    variance > f64::from(tolerance[c]).powi(2)
                })
            }) {
                continue;
            }
            let mut valid = true;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let c = cfa.color(xx, yy);
                    let side = usize::from(if axis == 0 { xx - x0 } else { yy - y0 } >= cut);
                    valid &= (mosaic[yy * w + xx] - levels[side][c]).abs() <= tolerance[c];
                }
            }
            if !valid {
                continue;
            }
            let side = usize::from(if axis == 0 { x - x0 } else { y - y0 } >= cut);
            let prediction = levels[side];
            if let Some(previous) = result {
                // Multiple edges fitting the same samples must agree here.
                // Otherwise the missing channel is not identified by the CFA.
                if (0..3).any(|c| (previous[c] - prediction[c]).abs() > tolerance[c]) {
                    return None;
                }
            } else {
                result = Some(prediction);
            }
        }
    }
    result
}

#[cfg(test)]
pub(crate) fn reconstruct(mosaic: &[f32], w: usize, h: usize, cfa: Cfa) -> Vec<[f32; 3]> {
    reconstruct_supported(mosaic, w, h, cfa, crate::render::Support::default())
}

pub(crate) fn reconstruct_supported(
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    support: crate::render::Support<'_>,
) -> Vec<[f32; 3]> {
    let mut rgb = crate::xtrans_dr6::reconstruct(mosaic, w, h, cfa);
    let edges = edge_sites(mosaic, w, h);
    let mut ranges = crate::edge_planes::RangeRows::new(w, h);
    for y in 0..h {
        ranges.prepare(mosaic, w, cfa, y);
        for x in 0..w {
            if support.allows(x, y)
                && (y.saturating_sub(1)..=(y + 1).min(h - 1)).any(|yy| {
                    (x.saturating_sub(1)..=(x + 1).min(w - 1)).any(|xx| edges[yy * w + xx])
                })
                && let Some(model) = interpolate(mosaic, w, h, cfa, x, y).or_else(|| {
                    crate::edge_planes::interpolate_with_bounds(
                        mosaic,
                        w,
                        h,
                        cfa,
                        x,
                        y,
                        ranges.at(x, y),
                    )
                })
            {
                let i = y * w + x;
                let measured = cfa.color(x, y);
                for c in 0..3 {
                    if c != measured {
                        rgb[i][c] = model[c];
                    }
                }
            }
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfa::XTRANS;

    #[test]
    fn sharp_coloured_edges_all_phases_preserve_samples_and_reduce_peak_error() {
        let (w, h) = (43, 35);
        for phase in 0..36 {
            for axis in 0..2 {
                let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
                let scene = |x, y| {
                    if (if axis == 0 { x } else { y }) < 17 {
                        [0.22, 0.48, 0.13]
                    } else {
                        [1.43, 0.57, 0.97]
                    }
                };
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| scene(i % w, i / w)[cfa.color(i % w, i / w)])
                    .collect();
                let old = crate::xtrans_dr6::reconstruct(&mosaic, w, h, cfa);
                let new = reconstruct(&mosaic, w, h, cfa);
                let mut maximum = [0f32; 2];
                for i in 0..w * h {
                    let c = cfa.color(i % w, i / w);
                    assert_eq!(new[i][c].to_bits(), mosaic[i].to_bits());
                    for (k, rgb) in [old[i], new[i]].into_iter().enumerate() {
                        for (a, b) in rgb.into_iter().zip(scene(i % w, i / w)) {
                            maximum[k] = maximum[k].max((a - b).abs());
                        }
                    }
                }
                assert!(maximum[1] < 0.1 * maximum[0], "{phase} {axis}: {maximum:?}");
            }
        }
    }

    #[test]
    fn bounded_edges_match_whole_frame_with_extended_and_signed_samples() {
        let (w, h) = (139, 137);
        for phase in 0..36 {
            let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
            let mosaic: Vec<_> = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    let rgb = if x < 127 && y < 129 {
                        [-0.0, 0.41, -0.2]
                    } else {
                        [1.7, 0.76, 1.03]
                    };
                    rgb[cfa.color(x, y)]
                })
                .collect();
            let expected = reconstruct(&mosaic, w, h, cfa);
            crate::xtrans_dr6::bounded::reconstruct_with(
                &mosaic,
                w,
                h,
                cfa,
                |m, w, h, c, _, _| reconstruct(m, w, h, c),
                || false,
                |x, y, rgb| {
                    let i = y * w + x;
                    assert_eq!(rgb[cfa.color(x, y)].to_bits(), mosaic[i].to_bits());
                    for c in 0..3 {
                        assert!(
                            (rgb[c] - expected[i][c]).abs() < 2e-6,
                            "{phase} {x} {y} {c}"
                        );
                    }
                    Ok(())
                },
            )
            .unwrap();
        }
    }
}
