//! Non-reflected boundary interpolation for a qualified recovered texture.
//! All measured CFA sites remain exact; only missing channels are estimated.
use crate::{cfa::Cfa, render::Support};

fn taps(p: usize, phase: usize, length: usize) -> [(usize, f64); 4] {
    if p % 2 == phase {
        return [(p, 1.), (p, 0.), (p, 0.), (p, 0.)];
    }
    let start = (p.saturating_sub(phase) / 2)
        .saturating_sub(1)
        .min((length - 1 - phase) / 2 - 3);
    let positions: [usize; 4] = std::array::from_fn(|i| phase + 2 * (start + i));
    positions.map(|q| {
        (
            q,
            positions
                .iter()
                .filter(|&&r| r != q)
                .map(|&r| (p as f64 - r as f64) / (q as f64 - r as f64))
                .product(),
        )
    })
}

pub(crate) fn interpolate(
    source: (&[f32], Support<'_>),
    width: usize,
    height: usize,
    cfa: Cfa,
    position: (usize, usize),
    mut rgb: [f32; 3],
) -> Option<[f32; 3]> {
    let (mosaic, support) = source;
    let (x, y) = position;
    let horizontal_border = x < 3 || x + 3 >= width;
    let vertical_border = y < 3 || y + 3 >= height;
    if width < 8 || height < 8 || (!horizontal_border && !vertical_border) {
        return None;
    }
    let measured = cfa.color(x, y);
    for c in [0, 2] {
        if c == measured {
            continue;
        }
        let p = (0..4).find(|&p| cfa.color(p % 2, p / 2) == c)?;
        let (xs, ys) = (taps(x, p % 2, width), taps(y, p / 2, height));
        let mut value = 0.;
        for (yy, wy) in ys.into_iter().filter(|(_, w)| *w != 0.) {
            for (xx, wx) in xs.into_iter().filter(|(_, w)| *w != 0.) {
                if !support.measured(xx, yy) {
                    return None;
                }
                value += wx * wy * f64::from(mosaic[yy * width + xx]);
            }
        }
        rgb[c] = value as f32;
    }
    if measured != 1 {
        let mut sum = 0.;
        let mut count = 0;
        // Interpolate along the frame boundary, where both sides are measured.
        // At a corner, average the two one-sided estimates; no reflection.
        if vertical_border {
            let mut value = 0.;
            for (xx, w) in taps(x, 1 - x % 2, width)
                .into_iter()
                .filter(|(_, w)| *w != 0.)
            {
                if !support.measured(xx, y) && !support.texture(xx, y) {
                    return None;
                }
                value += w * f64::from(mosaic[y * width + xx]);
            }
            sum += value;
            count += 1;
        }
        if horizontal_border {
            let mut value = 0.;
            for (yy, w) in taps(y, 1 - y % 2, height)
                .into_iter()
                .filter(|(_, w)| *w != 0.)
            {
                if !support.measured(x, yy) && !support.texture(x, yy) {
                    return None;
                }
                value += w * f64::from(mosaic[yy * width + x]);
            }
            sum += value;
            count += 1;
        }
        rgb[1] = (sum / f64::from(count)) as f32;
    }
    rgb.iter().all(|v| v.is_finite()).then_some(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(x: usize, y: usize, c: usize) -> f32 {
        let (x, y) = (x as f64 / 23., y as f64 / 21.);
        (0.2 + 0.07 * c as f64 + 0.11 * x + 0.08 * y + 0.05 * x * x * y - 0.03 * y * y * y) as f32
    }

    #[test]
    fn boundary_reproduces_independent_cubic_surfaces_and_measured_sites() {
        for (w, h) in [(24, 22), (23, 21), (8, 9)] {
            for phase in 0..4 {
                let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
                let v: Vec<_> = (0..w * h)
                    .map(|i| scene(i % w, i / w, cfa.color(i % w, i / w)))
                    .collect();
                let columns = w.div_ceil(6);
                let masks = vec![8; columns * h.div_ceil(6)];
                let support = Support::new(&masks, columns);
                for y in 0..h {
                    for x in 0..w {
                        let c = cfa.color(x, y);
                        let mut input = [f32::NAN; 3];
                        input[c] = v[y * w + x];
                        let result = interpolate((&v, support), w, h, cfa, (x, y), input);
                        if x >= 3 && x + 3 < w && y >= 3 && y + 3 < h {
                            assert!(result.is_none());
                            continue;
                        }
                        let result = result.unwrap();
                        assert_eq!(result[c].to_bits(), input[c].to_bits());
                        for (c, value) in result.into_iter().enumerate() {
                            assert!((value - scene(x, y, c)).abs() < 3e-7);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn texture_provenance_is_not_original_red_or_blue_support() {
        let (w, h) = (24_usize, 22_usize);
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let values = vec![0.7; w * h];
        let masks = vec![8; w.div_ceil(6) * h.div_ceil(6)];
        let mut clipping = vec![0; (w * h).div_ceil(8)];
        // Blue at (1,1) is needed to interpolate the corner's missing blue.
        let i = w + 1;
        clipping[i / 8] |= 1 << (i % 8);
        let support = Support::new(&masks, w.div_ceil(6)).with_original_clipping(&clipping, w);
        assert!(support.allows(0, 0));
        assert!(interpolate((&values, support), w, h, cfa, (0, 0), [0.7; 3]).is_none());
        let bad = vec![10; masks.len()];
        assert!(!Support::new(&bad, w.div_ceil(6)).allows(0, 0));
    }
}
