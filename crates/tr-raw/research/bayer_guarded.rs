//! BR5: measured-data confidence blends frozen BR3 with the product baseline.
//! Laboratory only. No access to scene truth or quality metrics.
use super::bayer_guided;
use crate::cfa::Cfa;

// count, sums RGB, squares RGB, cross RG/RB/GB, green disagreement,
// same-CFA structure tensor xx/yy/xy, tensor count.
type Moment = [f64; 15];
const RADIUS: usize = 4;

fn observation(m: &[f32], w: usize, h: usize, cfa: Cfa, x: usize, y: usize) -> Moment {
    let mut result = [0.; 15];
    if x + 1 < w && y + 1 < h {
        let mut rgb = [0.; 3];
        let mut greens = [0.; 2];
        let mut ng = 0;
        for dy in 0..2 {
            for dx in 0..2 {
                let c = cfa.color(x + dx, y + dy);
                let v = f64::from(m[(y + dy) * w + x + dx]);
                rgb[c] += v;
                if c == 1 {
                    greens[ng] = v;
                    ng += 1;
                }
            }
        }
        rgb[1] *= 0.5;
        let [r, g, b] = rgb;
        result[..11].copy_from_slice(&[
            1.,
            r,
            g,
            b,
            r * r,
            g * g,
            b * b,
            r * g,
            r * b,
            g * b,
            (greens[0] - greens[1]).powi(2),
        ]);
    }
    if x >= 2 && y >= 2 && x + 2 < w && y + 2 < h {
        let gx = f64::from(m[y * w + x + 2]) - f64::from(m[y * w + x - 2]);
        let gy = f64::from(m[(y + 2) * w + x]) - f64::from(m[(y - 2) * w + x]);
        result[11..].copy_from_slice(&[gx * gx, gy * gy, gx * gy, 1.]);
    }
    result
}

fn smooth(value: f64, lower: f64, upper: f64) -> f64 {
    let t = ((value - lower) / (upper - lower)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn weight(m: Moment) -> f32 {
    let n = m[0];
    if n < 32. || m[14] < 16. {
        return 0.;
    }
    let variance: [f64; 3] = std::array::from_fn(|c| (m[4 + c] - m[1 + c] * m[1 + c] / n).max(0.));
    if variance.iter().any(|v| *v <= n * 1e-10) {
        return 0.;
    }
    let rho = [(0, 1, 7), (0, 2, 8), (1, 2, 9)].map(|(a, b, k)| {
        ((m[k] - m[1 + a] * m[1 + b] / n) / (variance[a] * variance[b]).sqrt()).clamp(-1., 1.)
    });
    let correlation = smooth(rho.into_iter().fold(1., f64::min), 0.70, 0.95);
    let energy = m[11] + m[12];
    if energy <= m[14] * 1e-10 || correlation == 0. {
        return 0.;
    }
    let coherence = (((m[11] - m[12]).powi(2) + 4. * m[13] * m[13]).sqrt() / energy).min(1.);
    let direction = smooth(coherence, 0.55, 0.85);
    let green_agreement = 1. - smooth(m[10] / (4. * variance[1]), 0.05, 0.25);
    (correlation * direction * green_agreement) as f32
}

fn confidence(mosaic: &[f32], w: usize, h: usize, cfa: Cfa) -> Vec<f32> {
    let stride = w + 1;
    let mut integral = vec![[0.; 15]; stride * (h + 1)];
    for y in 0..h {
        let mut row = [0.; 15];
        for x in 0..w {
            let v = observation(mosaic, w, h, cfa, x, y);
            for k in 0..15 {
                row[k] += v[k];
                integral[(y + 1) * stride + x + 1][k] = integral[y * stride + x + 1][k] + row[k];
            }
        }
    }
    (0..mosaic.len())
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let (l, r) = (x.saturating_sub(RADIUS), (x + RADIUS + 1).min(w));
            let (t, b) = (y.saturating_sub(RADIUS), (y + RADIUS + 1).min(h));
            weight(std::array::from_fn(|k| {
                integral[b * stride + r][k]
                    - integral[t * stride + r][k]
                    - integral[b * stride + l][k]
                    + integral[t * stride + l][k]
            }))
        })
        .collect()
}

pub fn reconstruct(
    mosaic: &[f32],
    w: usize,
    h: usize,
    cfa: Cfa,
    baseline: &[[f32; 3]],
) -> Vec<[f32; 3]> {
    let weights = confidence(mosaic, w, h, cfa);
    let mut candidate = if weights.iter().all(|&v| v == 0.) {
        baseline.to_vec()
    } else {
        bayer_guided::reconstruct(mosaic, w, h, cfa, false)
    };
    for (i, p) in candidate.iter_mut().enumerate() {
        let measured = cfa.color(i % w, i / w);
        for c in 0..3 {
            p[c] = if c == measured {
                mosaic[i]
            } else if weights[i] == 0. {
                baseline[i][c]
            } else if weights[i] == 1. {
                p[c]
            } else {
                baseline[i][c] + weights[i] * (p[c] - baseline[i][c])
            };
        }
    }
    candidate
}

#[test]
fn integral_confidence_matches_direct_windows() {
    let (w, h) = (23, 19);
    for phase in 0..4 {
        let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
        let m: Vec<_> = (0..w * h)
            .map(|i| 0.4 + 0.3 * (0.17 * (i % w) as f32 + 0.11 * (i / w) as f32).sin())
            .collect();
        let actual = confidence(&m, w, h, cfa);
        assert!(actual.iter().any(|&v| v > 0.5));
        for (i, a) in actual.into_iter().enumerate() {
            let (x, y) = (i % w, i / w);
            let mut sum = [0.; 15];
            for yy in y.saturating_sub(RADIUS)..=(y + RADIUS).min(h - 1) {
                for xx in x.saturating_sub(RADIUS)..=(x + RADIUS).min(w - 1) {
                    for (s, v) in sum.iter_mut().zip(observation(&m, w, h, cfa, xx, yy)) {
                        *s += v;
                    }
                }
            }
            assert!((a - weight(sum)).abs() < 2e-6);
            assert!((0. ..=1.).contains(&a));
        }
    }
}

#[test]
fn flat_and_ambiguous_confidence_preserve_baseline_and_measured_bits() {
    for (w, h) in [(2, 3), (7, 9), (31, 27)] {
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
            for rgb in [[-0.; 3], [-0.1, 0.4, 1.3], [1.7; 3]] {
                let m: Vec<_> = (0..w * h).map(|i| rgb[cfa.color(i % w, i / w)]).collect();
                let image =
                    crate::render::develop(&m, w, h, cfa, crate::color::ID, [0, 0, w, h], 1)
                        .unwrap();
                let base: Vec<_> = image.pixels.iter().map(|p| [p[0], p[1], p[2]]).collect();
                let out = reconstruct(&m, w, h, cfa, &base);
                assert!(confidence(&m, w, h, cfa).into_iter().all(|v| v == 0.));
                for (i, (a, b)) in out.iter().zip(&base).enumerate() {
                    let measured = cfa.color(i % w, i / w);
                    for c in 0..3 {
                        assert_eq!(
                            a[c].to_bits(),
                            if c == measured {
                                m[i].to_bits()
                            } else {
                                b[c].to_bits()
                            }
                        );
                    }
                }
            }
            let m: Vec<_> = (0..w * h)
                .map(|i| (i * 137 % 997) as f32 / 400. - 0.2)
                .collect();
            let image =
                crate::render::develop(&m, w, h, cfa, crate::color::ID, [0, 0, w, h], 1).unwrap();
            let base: Vec<_> = image.pixels.iter().map(|p| [p[0], p[1], p[2]]).collect();
            for (i, p) in reconstruct(&m, w, h, cfa, &base).iter().enumerate() {
                assert!(p.iter().all(|v| v.is_finite()));
                let c = cfa.color(i % w, i / w);
                assert_eq!(p[c].to_bits(), m[i].to_bits());
            }
        }
    }
}
