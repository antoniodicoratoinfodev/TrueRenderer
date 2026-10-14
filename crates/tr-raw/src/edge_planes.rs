//! Conservative two-surface interpolation from the measured camera samples.
//! Each colour has its own affine plane on each side; no neutral is assumed.
use crate::{
    cfa::Cfa,
    color::{inverse, vector},
};

const RADIUS: usize = 6;
const EDGE: usize = 2 * RADIUS + 1;
type Bounds = [[f32; 3]; 2];

#[derive(Clone, Copy, Default)]
struct Extremum {
    indices: [usize; EDGE],
    head: usize,
    len: usize,
}
impl Extremum {
    fn expire(&mut self, left: usize) {
        while self.len != 0 && self.indices[self.head] < left {
            self.head = (self.head + 1) % EDGE;
            self.len -= 1;
        }
    }
    fn push(&mut self, row: &[f32], index: usize, minimum: bool) {
        while self.len != 0 {
            let last = row[self.indices[(self.head + self.len - 1) % EDGE]];
            if if minimum {
                last < row[index]
            } else {
                last > row[index]
            } {
                break;
            }
            self.len -= 1;
        }
        debug_assert!(self.len < EDGE);
        self.indices[(self.head + self.len) % EDGE] = index;
        self.len += 1;
    }
    fn value(&self, row: &[f32], empty: f32) -> f32 {
        if self.len == 0 {
            empty
        } else {
            row[self.indices[self.head]]
        }
    }
}

/// Exact separable bounds: two 13-row blocks, with prefix/suffix extrema.
/// Each pixel enters a horizontal queue once; a window combines two bounds.
pub(crate) struct RangeRows {
    rows: [[Vec<Bounds>; 2 * EDGE]; 2],
    ids: [Option<usize>; 2],
    height: usize,
}
fn combine(a: Bounds, b: Bounds) -> Bounds {
    std::array::from_fn(|side| {
        std::array::from_fn(|c| {
            if side == 0 {
                a[side][c].min(b[side][c])
            } else {
                a[side][c].max(b[side][c])
            }
        })
    })
}
impl RangeRows {
    pub(crate) fn reservation_bytes(width: usize) -> usize {
        4 * EDGE * width * size_of::<Bounds>() + size_of::<Self>()
    }
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Self {
            rows: std::array::from_fn(|_| std::array::from_fn(|_| vec![[[0.; 3]; 2]; w])),
            ids: [None; 2],
            height: h,
        }
    }
    pub(crate) fn prepare(&mut self, mosaic: &[f32], w: usize, cfa: Cfa, y: usize) {
        let first = y.saturating_sub(RADIUS) / EDGE;
        let last = (y + RADIUS).min(self.height - 1) / EDGE;
        for block in first..=last {
            let slot = block % 2;
            if self.ids[slot] == Some(block) {
                continue;
            }
            let end = ((block + 1) * EDGE).min(self.height);
            let length = end - block * EDGE;
            let (prefix, suffix) = self.rows[slot].split_at_mut(EDGE);
            for yy in block * EDGE..end {
                let local = yy % EDGE;
                let row = &mosaic[yy * w..(yy + 1) * w];
                let mut queues = [[Extremum::default(); 3]; 2];
                let mut right = 0;
                for (x, bounds) in prefix[local].iter_mut().enumerate() {
                    let left = x.saturating_sub(RADIUS);
                    for q in queues.iter_mut().flatten() {
                        q.expire(left);
                    }
                    while right < (x + RADIUS + 1).min(w) {
                        let c = cfa.color(right, yy);
                        queues[0][c].push(row, right, true);
                        queues[1][c].push(row, right, false);
                        right += 1;
                    }
                    for c in 0..3 {
                        bounds[0][c] = queues[0][c].value(row, f32::INFINITY);
                        bounds[1][c] = queues[1][c].value(row, f32::NEG_INFINITY);
                    }
                }
                suffix[local].copy_from_slice(&prefix[local]);
                if local > 0 {
                    let (before, after) = prefix.split_at_mut(local);
                    for (v, previous) in after[0].iter_mut().zip(&before[local - 1]) {
                        *v = combine(*previous, *v);
                    }
                }
            }
            for local in (0..length - 1).rev() {
                let (before, after) = suffix.split_at_mut(local + 1);
                for (v, next) in before[local].iter_mut().zip(&after[0]) {
                    *v = combine(*v, *next);
                }
            }
            self.ids[slot] = Some(block);
        }
    }
    pub(crate) fn at(&self, x: usize, y: usize) -> Bounds {
        let first = y.saturating_sub(RADIUS);
        let last = (y + RADIUS).min(self.height - 1);
        let (a, b) = (first / EDGE, last / EDGE);
        debug_assert_eq!(self.ids[a % 2], Some(a));
        debug_assert_eq!(self.ids[b % 2], Some(b));
        let prefix = self.rows[b % 2][last % EDGE][x];
        let suffix = self.rows[a % 2][EDGE + first % EDGE][x];
        if a == b {
            if first.is_multiple_of(EDGE) {
                prefix
            } else {
                debug_assert_eq!(last, self.height - 1);
                suffix
            }
        } else {
            combine(suffix, prefix)
        }
    }
}

#[cfg(test)]
#[path = "../research/edge_planes_frozen.rs"]
pub(crate) mod frozen;

pub(crate) fn near_edge(
    mosaic: &[f32],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    period: usize,
) -> bool {
    let a = mosaic[y * w + x];
    [
        x.checked_sub(period).map(|xx| y * w + xx),
        (x + period < w).then_some(y * w + x + period),
        y.checked_sub(period).map(|yy| yy * w + x),
        (y + period < h).then_some((y + period) * w + x),
    ]
    .into_iter()
    .flatten()
    .any(|j| {
        let b = mosaic[j];
        (a - b).abs() > 0.1 * a.abs().max(b.abs()).max(0.05)
    })
}

#[cfg(test)]
pub(crate) fn interpolate(
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    x: usize,
    y: usize,
) -> Option<[f32; 3]> {
    let (x0, y0) = (x.saturating_sub(RADIUS), y.saturating_sub(RADIUS));
    let (x1, y1) = ((x + RADIUS + 1).min(w), (y + RADIUS + 1).min(h));
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let v = mosaic[yy * w + xx];
            bounds[0][c] = bounds[0][c].min(v);
            bounds[1][c] = bounds[1][c].max(v);
        }
    }
    interpolate_with_bounds(mosaic, w, h, cfa, x, y, bounds)
}

pub(crate) fn interpolate_with_bounds(
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    x: usize,
    y: usize,
    [low, high]: Bounds,
) -> Option<[f32; 3]> {
    let (x0, y0) = (x.saturating_sub(RADIUS), y.saturating_sub(RADIUS));
    let (x1, y1) = ((x + RADIUS + 1).min(w), (y + RADIUS + 1).min(h));
    let mut counts = [0usize; 3];
    let thresholds: [f32; 3] = std::array::from_fn(|c| low[c] + 0.5 * (high[c] - low[c]));
    let mut tolerance = [0.; 3];
    let mut lower_max = [f32::NEG_INFINITY; 3];
    let mut upper_min = [f32::INFINITY; 3];
    let mut upper_count = [0usize; 3];
    // A subset can only overestimate the gap. Rejecting an already too small
    // upper bound is exact, not a texture heuristic or an admission change.
    for gy in 0..4 {
        for gx in 0..4 {
            let (xx, yy) = (x0 + gx * (x1 - x0 - 1) / 3, y0 + gy * (y1 - y0 - 1) / 3);
            let c = cfa.color(xx, yy);
            let value = mosaic[yy * w + xx];
            if value > thresholds[c] {
                upper_min[c] = upper_min[c].min(value);
            } else {
                lower_max[c] = lower_max[c].max(value);
            }
            if upper_min[c] - lower_max[c] <= 0.5 * (high[c] - low[c]) {
                return None;
            }
        }
    }
    let mut mask = [[false; EDGE]; EDGE];
    // A unique gap larger than half the range contains its midpoint. This
    // partition is equivalent to sorting for an admissible gap, in linear
    // time and without per-pixel sample arrays.
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let value = mosaic[yy * w + xx];
            counts[c] += 1;
            let upper = value > thresholds[c];
            mask[yy - y0][xx - x0] = upper;
            if upper {
                upper_min[c] = upper_min[c].min(value);
                upper_count[c] += 1;
            } else {
                lower_max[c] = lower_max[c].max(value);
            }
        }
    }
    for c in 0..3 {
        if counts[c] < 6 {
            return None;
        }
        let gap = upper_min[c] - lower_max[c];
        let scale = low[c].abs().max(high[c].abs()).max(0.01);
        if upper_count[c] < 3
            || counts[c] - upper_count[c] < 3
            || gap < 0.1 * scale
            || gap <= 0.5 * (high[c] - low[c])
        {
            return None;
        }
        tolerance[c] = 0.001 * scale;
    }
    // All measured colours must describe one common, non-alternating boundary.
    // The directional Bayer construction already treats axis-aligned edges.
    // This extra model is for boundaries crossing both axes. In particular,
    // do not refit an unresolved highlight step from its inferred samples.
    if cfa.period == 2
        && ((1..y1 - y0).all(|yy| mask[yy][..x1 - x0] == mask[0][..x1 - x0])
            || (0..y1 - y0).all(|yy| (1..x1 - x0).all(|xx| mask[yy][xx] == mask[yy][0])))
    {
        return None;
    }
    let monotone = (0..2).any(|axis| {
        [false, true].into_iter().any(|increasing| {
            let (lines, length) = if axis == 0 {
                (y1 - y0, x1 - x0)
            } else {
                (x1 - x0, y1 - y0)
            };
            (0..lines).all(|line| {
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
            })
        })
    });
    if !monotone {
        return None;
    }
    let mut normal = [[[[0.; 3]; 3]; 3]; 2];
    let mut rhs = [[[0.; 3]; 3]; 2];
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let side = usize::from(mask[yy - y0][xx - x0]);
            let xy = [
                1.,
                (xx as f64 - x as f64) / RADIUS as f64,
                (yy as f64 - y as f64) / RADIUS as f64,
            ];
            let value = f64::from(mosaic[yy * w + xx]);
            for j in 0..3 {
                rhs[side][c][j] += xy[j] * value;
                for k in 0..3 {
                    normal[side][c][j][k] += xy[j] * xy[k];
                }
            }
        }
    }
    let mut planes = [[[0.; 3]; 3]; 2];
    for side in 0..2 {
        for c in 0..3 {
            planes[side][c] = vector(inverse(normal[side][c]).ok()?, rhs[side][c]);
        }
    }
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let side = usize::from(mask[yy - y0][xx - x0]);
            let p = planes[side][c];
            let predicted = p[0]
                + p[1] * (xx as f64 - x as f64) / RADIUS as f64
                + p[2] * (yy as f64 - y as f64) / RADIUS as f64;
            if (predicted - f64::from(mosaic[yy * w + xx])).abs() > f64::from(tolerance[c]) {
                return None;
            }
        }
    }
    let side = usize::from(mask[y - y0][x - x0]);
    let result = planes[side].map(|p| p[0] as f32);
    result.iter().all(|v| v.is_finite()).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfa::XTRANS;

    #[test]
    fn cached_ranges_and_early_rejection_match_frozen_equations_exactly() {
        for base in [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS).unwrap()] {
            for phase in 0..base.period * base.period {
                let cfa = base.shifted(phase % base.period, phase / base.period);
                for (w, h) in [(6, 7), (17, 19), (37, 31)] {
                    for scene in 0..4 {
                        let mosaic: Vec<_> = (0..w * h)
                            .map(|i| {
                                let (x, y) = ((i % w) as f32, (i / w) as f32);
                                let c = cfa.color(i % w, i / w);
                                let side = x < 0.53 * y + 4. + (y * 0.17).sin() * 2.;
                                match scene {
                                    0 => {
                                        if side {
                                            [-0.1, 0.13, 0.08][c] + 0.001 * x
                                        } else {
                                            [1.2, 0.62, 0.83][c] + 0.003 * y
                                        }
                                    }
                                    1 => [0.3, 0.6, 0.9][c] + 0.17 * (x * 0.8 + y * 0.53).sin(),
                                    2 => {
                                        if x < w as f32 / 2. {
                                            [-0.0, -0.05, 0.01][c]
                                        } else {
                                            [1.7, 0.3, 0.07][c]
                                        }
                                    }
                                    _ => {
                                        ((i.wrapping_mul(1664525).wrapping_add(1013904223)) % 65521)
                                            as f32
                                            / 32000.
                                            - 1.
                                    }
                                }
                            })
                            .collect();
                        let mut ranges = RangeRows::new(w, h);
                        for y in 0..h {
                            ranges.prepare(&mosaic, w, cfa, y);
                            for x in 0..w {
                                let mut literal = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
                                for yy in y.saturating_sub(6)..=(y + 6).min(h - 1) {
                                    for xx in x.saturating_sub(6)..=(x + 6).min(w - 1) {
                                        let c = cfa.color(xx, yy);
                                        let v = mosaic[yy * w + xx];
                                        literal[0][c] = literal[0][c].min(v);
                                        literal[1][c] = literal[1][c].max(v);
                                    }
                                }
                                let cached = ranges.at(x, y);
                                assert_eq!(cached, literal);
                                let old = frozen::interpolate(&mosaic, w, h, cfa, x, y)
                                    .map(|v| v.map(f32::to_bits));
                                let new = interpolate_with_bounds(&mosaic, w, h, cfa, x, y, cached)
                                    .map(|v| v.map(f32::to_bits));
                                assert_eq!(
                                    old, new,
                                    "phase {phase}, scene {scene}, {w}×{h}, ({x},{y})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn curved_coloured_surfaces_and_affine_detail_all_cfa_phases() {
        let (w, h) = (37, 31);
        for base in [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS).unwrap()] {
            for phase in 0..base.period * base.period {
                let cfa = base.shifted(phase % base.period, phase / base.period);
                let scene = |x: usize, y: usize| {
                    let dark = (x as f32) < 0.53 * y as f32 + 8. + 2. * (y as f32 * 0.17).sin();
                    if dark {
                        [0.05 + 0.001 * x as f32, 0.13 + 0.001 * y as f32, 0.08]
                    } else {
                        [
                            1.2 + 0.005 * x as f32,
                            0.62 + 0.003 * y as f32,
                            0.83 + 0.002 * x as f32,
                        ]
                    }
                };
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| scene(i % w, i / w)[cfa.color(i % w, i / w)])
                    .collect();
                let mut accepted = 0;
                for y in 0..h {
                    for x in 0..w {
                        if let Some(rgb) = interpolate(&mosaic, w, h, cfa, x, y) {
                            accepted += 1;
                            for (c, value) in rgb.iter().enumerate() {
                                assert!(
                                    (value - scene(x, y)[c]).abs() < 2e-5,
                                    "{phase} {x} {y} {c}"
                                );
                            }
                        }
                    }
                }
                assert!(accepted > 100, "phase {phase}: {accepted}");
            }
        }
    }

    #[test]
    fn alternating_texture_and_degenerate_support_are_not_two_surfaces() {
        let cfa = Cfa::new(6, &XTRANS).unwrap();
        let (w, h) = (25, 23);
        let mosaic: Vec<_> = (0..w * h)
            .map(|i| {
                let c = cfa.color(i % w, i / w);
                [0.3, 0.6, 0.9][c] + 0.17 * ((i % w) as f32 * 0.8 + (i / w) as f32 * 0.53).sin()
            })
            .collect();
        for y in 0..h {
            for x in 0..w {
                assert!(interpolate(&mosaic, w, h, cfa, x, y).is_none());
            }
        }
        assert!(interpolate(&[0.4; 36], 6, 6, cfa, 0, 0).is_none());
    }
}
