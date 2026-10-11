//! DR5: the frozen DR4 confidence gates the three-pass reconstruction.
//! Own research formulation; never compiled in the production library.
use super::residual_refined as residual;
use crate::{cfa::Cfa, xtrans::CameraRgb};

// count, sum a, sum b, sum a², sum b², sum ab, per acquired color.
type Moments = [[f64; 6]; 3];
const DIRECTIONS: [(isize, isize); 4] = [(3, 0), (0, 3), (3, 3), (-3, 3)];
const RADIUS: usize = 6;

fn pair(
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
    width: usize,
    height: usize,
) -> Option<(usize, usize)> {
    let ax = x as isize - dx;
    let ay = y as isize - dy;
    let bx = x as isize + dx;
    let by = y as isize + dy;
    if [ax, bx].iter().any(|&v| v < 0 || v >= width as isize)
        || [ay, by].iter().any(|&v| v < 0 || v >= height as isize)
    {
        None
    } else {
        Some((
            ay as usize * width + ax as usize,
            by as usize * width + bx as usize,
        ))
    }
}

fn observation(a: f32, b: f32) -> [f64; 6] {
    let (a, b) = (f64::from(a), f64::from(b));
    [1., a, b, a * a, b * b, a * b]
}

fn correlation(moments: Moments) -> f64 {
    let (mut count, mut va, mut vb, mut cov) = (0., 0., 0., 0.);
    for [n, a, b, aa, bb, ab] in moments {
        if n >= 8. {
            count += n;
            va += (aa - a * a / n).max(0.);
            vb += (bb - b * b / n).max(0.);
            cov += ab - a * b / n;
        }
    }
    if count < 32. || va <= count * 1e-12 || vb <= count * 1e-12 {
        0.
    } else {
        (cov.abs() / (va * vb).sqrt()).min(1.)
    }
}

fn weight(rho: f64) -> f32 {
    let t = ((rho - 0.35) / 0.30).clamp(0., 1.);
    (t * t * (3. - 2. * t)) as f32
}

fn correlation_confidence(mosaic: &[f32], width: usize, height: usize, cfa: Cfa) -> Vec<f32> {
    let mut rho = vec![0f64; mosaic.len()];
    let stride = width + 1;
    let mut integral = vec![[[0.; 6]; 3]; stride * (height + 1)];
    for (dx, dy) in DIRECTIONS {
        // Row zero and column zero remain zero; overwrite every interior cell.
        for y in 0..height {
            let mut row: Moments = [[0.; 6]; 3];
            for x in 0..width {
                if let Some((a, b)) = pair(x, y, dx, dy, width, height) {
                    let color = cfa.color(a % width, a / width);
                    let sample = observation(mosaic[a], mosaic[b]);
                    for (sum, v) in row[color].iter_mut().zip(sample) {
                        *sum += v;
                    }
                }
                let above = integral[y * stride + x + 1];
                integral[(y + 1) * stride + x + 1] =
                    std::array::from_fn(|c| std::array::from_fn(|k| above[c][k] + row[c][k]));
            }
        }
        for y in 0..height {
            let (top, bottom) = (y.saturating_sub(RADIUS), (y + RADIUS + 1).min(height));
            for x in 0..width {
                let (left, right) = (x.saturating_sub(RADIUS), (x + RADIUS + 1).min(width));
                let moments = std::array::from_fn(|c| {
                    std::array::from_fn(|k| {
                        integral[bottom * stride + right][c][k]
                            - integral[top * stride + right][c][k]
                            - integral[bottom * stride + left][c][k]
                            + integral[top * stride + left][c][k]
                    })
                });
                let i = y * width + x;
                rho[i] = rho[i].max(correlation(moments));
            }
        }
    }
    rho.into_iter().map(weight).collect()
}

fn directional_weight([xx, yy, xy, n]: [f64; 4]) -> f32 {
    if n < 32. || xx + yy <= n * 1e-12 {
        return 0.;
    }
    let coherence = (((xx - yy).powi(2) + 4. * xy * xy).sqrt() / (xx + yy)).min(1.);
    let t = ((coherence - 0.55) / 0.30).clamp(0., 1.);
    (t * t * (3. - 2. * t)) as f32
}

fn directional_confidence(mosaic: &[f32], width: usize, height: usize) -> Vec<f32> {
    let stride = width + 1;
    let mut integral = vec![[0.; 4]; stride * (height + 1)];
    for y in 0..height {
        let mut row = [0.; 4];
        for x in 0..width {
            if let (Some((a, b)), Some((c, d))) = (
                pair(x, y, 3, 0, width, height),
                pair(x, y, 0, 3, width, height),
            ) {
                let dx = f64::from(mosaic[b]) - f64::from(mosaic[a]);
                let dy = f64::from(mosaic[d]) - f64::from(mosaic[c]);
                for (sum, v) in row.iter_mut().zip([dx * dx, dy * dy, dx * dy, 1.]) {
                    *sum += v;
                }
            }
            let above = integral[y * stride + x + 1];
            integral[(y + 1) * stride + x + 1] = std::array::from_fn(|k| above[k] + row[k]);
        }
    }
    (0..mosaic.len())
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let (l, r) = (x.saturating_sub(RADIUS), (x + RADIUS + 1).min(width));
            let (t, b) = (y.saturating_sub(RADIUS), (y + RADIUS + 1).min(height));
            directional_weight(std::array::from_fn(|k| {
                integral[b * stride + r][k]
                    - integral[t * stride + r][k]
                    - integral[b * stride + l][k]
                    + integral[t * stride + l][k]
            }))
        })
        .collect()
}

fn confidence(mosaic: &[f32], width: usize, height: usize, cfa: Cfa) -> Vec<f32> {
    let correlation = correlation_confidence(mosaic, width, height, cfa);
    let direction = directional_confidence(mosaic, width, height);
    correlation
        .into_iter()
        .zip(direction)
        .map(|(a, b)| a * b)
        .collect()
}

pub fn reconstruct(mosaic: &[f32], width: usize, height: usize, cfa: Cfa) -> Vec<[f32; 3]> {
    let weights = confidence(mosaic, width, height, cfa);
    let mut candidate = residual::reconstruct(mosaic, width, height, cfa);
    let baseline = CameraRgb::new(mosaic, width, height, cfa);
    for (i, pixel) in candidate.iter_mut().enumerate() {
        let (x, y) = (i % width, i / width);
        let base = baseline.at(x, y);
        let measured = cfa.color(x, y);
        for c in 0..3 {
            pixel[c] = if c == measured {
                mosaic[i]
            } else if weights[i] == 0. {
                base[c]
            } else if weights[i] == 1. {
                pixel[c]
            } else {
                base[c] + weights[i] * (pixel[c] - base[c])
            };
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfa::XTRANS;

    #[test]
    fn integral_confidence_matches_direct_pairs_at_all_phases_and_borders() {
        let (w, h) = (25, 23);
        let mosaic: Vec<_> = (0..w * h)
            .map(|i| (0.17 * (i % w) as f32 + 0.11 * (i / w) as f32).sin() - 0.15)
            .collect();
        for phase in 0..36 {
            let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
            let actual = correlation_confidence(&mosaic, w, h, cfa);
            for (i, &value) in actual.iter().enumerate() {
                let (x, y) = (i % w, i / w);
                let mut best = 0f64;
                for (dx, dy) in DIRECTIONS {
                    let mut moments = [[0.; 6]; 3];
                    for yy in y.saturating_sub(RADIUS)..=(y + RADIUS).min(h - 1) {
                        for xx in x.saturating_sub(RADIUS)..=(x + RADIUS).min(w - 1) {
                            if let Some((a, b)) = pair(xx, yy, dx, dy, w, h) {
                                assert_eq!(cfa.color(a % w, a / w), cfa.color(b % w, b / w));
                                let color = cfa.color(a % w, a / w);
                                for (sum, v) in moments[color]
                                    .iter_mut()
                                    .zip(observation(mosaic[a], mosaic[b]))
                                {
                                    *sum += v;
                                }
                            }
                        }
                    }
                    best = best.max(correlation(moments));
                }
                assert!(
                    (value - weight(best)).abs() < 1e-6,
                    "phase {phase}, ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn directional_tensor_matches_direct_same_color_quadruples() {
        let (w, h) = (25, 23);
        for phase in 0..36 {
            let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
            let mosaic: Vec<_> = (0..w * h)
                .map(|i| {
                    (0.13 * (i % w) as f32).sin()
                        + 0.27 * (0.3 * (i / w) as f32).cos()
                        + cfa.color(i % w, i / w) as f32 * 0.25
                })
                .collect();
            let actual = directional_confidence(&mosaic, w, h);
            for (i, value) in actual.into_iter().enumerate() {
                let (x, y) = (i % w, i / w);
                let mut sum = [0.; 4];
                for yy in y.saturating_sub(RADIUS)..=(y + RADIUS).min(h - 1) {
                    for xx in x.saturating_sub(RADIUS)..=(x + RADIUS).min(w - 1) {
                        if let (Some((a, b)), Some((c, d))) =
                            (pair(xx, yy, 3, 0, w, h), pair(xx, yy, 0, 3, w, h))
                        {
                            assert!(
                                [b, c, d]
                                    .into_iter()
                                    .all(|k| cfa.color(k % w, k / w) == cfa.color(a % w, a / w))
                            );
                            let dx = f64::from(mosaic[b]) - f64::from(mosaic[a]);
                            let dy = f64::from(mosaic[d]) - f64::from(mosaic[c]);
                            for (s, v) in sum.iter_mut().zip([dx * dx, dy * dy, dx * dy, 1.]) {
                                *s += v;
                            }
                        }
                    }
                }
                assert!((value - directional_weight(sum)).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn measured_sites_signed_zero_flats_and_small_images_are_preserved() {
        for (w, h) in [(6, 6), (7, 11), (29, 23)] {
            for phase in 0..36 {
                let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
                for rgb in [[-0.17, 0.37, 1.4], [-0.; 3], [1.7; 3]] {
                    let mosaic: Vec<_> = (0..w * h).map(|i| rgb[cfa.color(i % w, i / w)]).collect();
                    let actual = reconstruct(&mosaic, w, h, cfa);
                    let baseline = CameraRgb::new(&mosaic, w, h, cfa);
                    for (i, p) in actual.iter().enumerate() {
                        assert_eq!(
                            p.map(f32::to_bits),
                            baseline.at(i % w, i / w).map(f32::to_bits)
                        );
                        assert_eq!(p[cfa.color(i % w, i / w)].to_bits(), mosaic[i].to_bits());
                    }
                }
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| ((i * 127 + 83) % 997) as f32 / 431. - 0.17)
                    .collect();
                for (i, p) in reconstruct(&mosaic, w, h, cfa).iter().enumerate() {
                    assert!(p.iter().all(|v| v.is_finite()));
                    assert_eq!(p[cfa.color(i % w, i / w)].to_bits(), mosaic[i].to_bits());
                }
            }
        }
    }
}
