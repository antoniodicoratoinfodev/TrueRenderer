// Frozen pre-optimization model; reference source SHA-256 69f02cf93b2d55682850374a78bdfceac0619e8e29f7021ef6105966dce95362
//! Conservative two-surface interpolation from the measured camera samples.
//! Each colour has its own affine plane on each side; no neutral is assumed.
use crate::{
    cfa::Cfa,
    color::{inverse, vector},
};

const RADIUS: usize = 6;
const EDGE: usize = 2 * RADIUS + 1;

#[allow(dead_code)] // Frozen scalar reference; callers only compare interpolation.
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
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    let mut counts = [0usize; 3];
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            low[c] = low[c].min(mosaic[yy * w + xx]);
            high[c] = high[c].max(mosaic[yy * w + xx]);
            counts[c] += 1;
        }
    }
    let thresholds: [f32; 3] = std::array::from_fn(|c| low[c] + 0.5 * (high[c] - low[c]));
    let mut tolerance = [0.; 3];
    let mut lower_max = [f32::NEG_INFINITY; 3];
    let mut upper_min = [f32::INFINITY; 3];
    let mut upper_count = [0usize; 3];
    let mut mask = [[false; EDGE]; EDGE];
    // A unique gap larger than half the range contains its midpoint. This
    // partition is equivalent to sorting for an admissible gap, in linear
    // time and without per-pixel sample arrays.
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let value = mosaic[yy * w + xx];
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
