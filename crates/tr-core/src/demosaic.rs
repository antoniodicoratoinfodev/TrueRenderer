//! TR-directional-f32-v1: Bayer green interpolation with directional second
//! differences, then bilinear R-G/B-G differences. Independently implemented
//! from the mathematical construction described in docs/progetto-motori-raw.md.
//! No sharpening, denoise, tone curve, highlight reconstruction or RGB clamp.
use crate::color::{LinearImage, MAX_PIXELS, Pixel, linear_srgb_to_rec2020};
use anyhow::{Result, ensure};

/// Reflect around the outer sample, preserving CFA parity (including odd sizes).
fn reflect(index: isize, length: usize) -> usize {
    let period = 2 * (length as isize - 1);
    let i = index.rem_euclid(period);
    if i < length as isize {
        i as usize
    } else {
        (period - i) as usize
    }
}

pub fn oriented_size(width: u32, height: u32, flip: u32) -> (u32, u32) {
    if flip == 5 || flip == 6 {
        (height, width)
    } else {
        (width, height)
    }
}

/// Input is black-subtracted, white-normalized and as-shot balanced camera RGB
/// samples. Matrix maps that camera RGB to linear sRGB, with unrestricted float
/// values: sRGB primaries here do NOT impose the integer gamut boundary.
pub fn develop(
    mosaic: &[f32],
    width: u32,
    height: u32,
    cfa: [u32; 4],
    camera_to_srgb: [f32; 9],
    flip: u32,
) -> Result<LinearImage> {
    let (w, h) = (width as usize, height as usize);
    ensure!(
        w >= 4
            && h >= 4
            && (width as u64 * height as u64) <= MAX_PIXELS as u64
            && mosaic.len() == w * h,
        "Dimensioni Bayer non valide"
    );
    ensure!(
        [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]].contains(&cfa),
        "CFA non Bayer RGB"
    );
    ensure!(
        [0, 3, 5, 6].contains(&flip),
        "Orientamento Bayer non supportato"
    );
    ensure!(
        mosaic
            .iter()
            .chain(camera_to_srgb.iter())
            .all(|v| v.is_finite()),
        "Calibrazione/campioni non finiti"
    );
    let color = |x: usize, y: usize| cfa[(y % 2) * 2 + x % 2];
    let at = |x: isize, y: isize| reflect(y, h) * w + reflect(x, w);
    let mut green = vec![0.; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if color(x, y) == 1 {
                green[i] = mosaic[i];
                continue;
            }
            let (x, y) = (x as isize, y as isize);
            let left = mosaic[at(x - 1, y)];
            let right = mosaic[at(x + 1, y)];
            let up = mosaic[at(x, y - 1)];
            let down = mosaic[at(x, y + 1)];
            let dh = 2. * mosaic[i] - mosaic[at(x - 2, y)] - mosaic[at(x + 2, y)];
            let dv = 2. * mosaic[i] - mosaic[at(x, y - 2)] - mosaic[at(x, y + 2)];
            let gh = (left + right) * 0.5 + dh * 0.25;
            let gv = (up + down) * 0.5 + dv * 0.25;
            let horizontal = (left - right).abs() + dh.abs();
            let vertical = (up - down).abs() + dv.abs();
            green[i] = if horizontal < vertical {
                gh
            } else if vertical < horizontal {
                gv
            } else {
                (gh + gv) * 0.5
            };
        }
    }
    let (ow, oh) = oriented_size(width, height, flip);
    let mut pixels: Vec<Pixel> = vec![[0.; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut rgb = [0., green[i], 0.];
            for c in [0, 2] {
                if color(x, y) == c {
                    rgb[c as usize] = mosaic[i];
                    continue;
                }
                let offsets: &[(isize, isize)] = if color(x, y) != 1 {
                    &[(-1, -1), (1, -1), (-1, 1), (1, 1)]
                } else if color(x ^ 1, y) == c {
                    &[(-1, 0), (1, 0)]
                } else {
                    &[(0, -1), (0, 1)]
                };
                let delta: f32 = offsets
                    .iter()
                    .map(|(dx, dy)| {
                        let k = at(x as isize + dx, y as isize + dy);
                        mosaic[k] - green[k]
                    })
                    .sum();
                rgb[c as usize] = green[i] + delta / offsets.len() as f32;
            }
            let srgb = std::array::from_fn(|row| {
                (0..3).map(|c| camera_to_srgb[row * 3 + c] * rgb[c]).sum()
            });
            let rec = linear_srgb_to_rec2020(srgb);
            let (ox, oy) = match flip {
                3 => (w - 1 - x, h - 1 - y),
                5 => (y, w - 1 - x),
                6 => (h - 1 - y, x),
                _ => (x, y),
            };
            pixels[oy * ow as usize + ox] = [rec[0], rec[1], rec[2], 1.];
        }
    }
    LinearImage::new(ow, oh, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rec2020_to_linear_srgb;
    const ID: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    const PATTERNS: [[u32; 4]; 4] = [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]];

    #[test]
    fn constant_color_borders_all_phases_and_unbounded_samples() {
        for cfa in PATTERNS {
            for (w, h) in [(8, 10), (9, 7)] {
                let expected = [-0.1, 0.4, 1.7];
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| expected[cfa[(i / w % 2) * 2 + i % w % 2] as usize])
                    .collect();
                for flip in [0, 3, 5, 6] {
                    let image = develop(&mosaic, w as u32, h as u32, cfa, ID, flip).unwrap();
                    assert_eq!(
                        (image.width, image.height),
                        oriented_size(w as u32, h as u32, flip)
                    );
                    for pixel in image.pixels {
                        let rgb = rec2020_to_linear_srgb([pixel[0], pixel[1], pixel[2]]);
                        for c in 0..3 {
                            assert!((rgb[c] - expected[c]).abs() < 3e-6);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn sampled_channels_and_rotation_are_preserved() {
        let (w, h) = (13, 11);
        let mosaic: Vec<_> = (0..w * h)
            .map(|i| ((i * 137) % 997) as f32 / 500. - 0.2)
            .collect();
        for cfa in PATTERNS {
            let image = develop(&mosaic, w, h, cfa, ID, 0).unwrap();
            for (i, p) in image.pixels.iter().enumerate() {
                let c = cfa[(i / w as usize % 2) * 2 + i % w as usize % 2] as usize;
                let rgb = rec2020_to_linear_srgb([p[0], p[1], p[2]]);
                assert!((rgb[c] - mosaic[i]).abs() < 5e-6);
            }
            for flip in [3, 5, 6] {
                let rotated = develop(&mosaic, w, h, cfa, ID, flip).unwrap();
                for y in 0..h {
                    for x in 0..w {
                        let (ox, oy) = match flip {
                            3 => (w - 1 - x, h - 1 - y),
                            5 => (y, w - 1 - x),
                            _ => (h - 1 - y, x),
                        };
                        assert_eq!(
                            image.pixels[(y * w + x) as usize],
                            rotated.pixels[(oy * rotated.width + ox) as usize]
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_invalid_inputs_before_processing() {
        assert!(develop(&[0.; 16], 4, 4, [0, 1, 2, 3], ID, 0).is_err());
        assert!(develop(&[0.; 15], 4, 4, PATTERNS[0], ID, 0).is_err());
        assert!(develop(&[f32::NAN; 16], 4, 4, PATTERNS[0], ID, 0).is_err());
    }
}
