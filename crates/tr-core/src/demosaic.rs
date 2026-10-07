//! TR-directional-f32-v1: Bayer green interpolation with directional second
//! differences, then bilinear R-G/B-G differences. Independently implemented
//! from the mathematical construction described in docs/progetto-motori-raw.md.
//! Optional sensor-gated highlight neutralization precedes the colour matrix.
//! No sharpening, denoise, tone curve or working-space RGB clamp.
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
    develop_impl(mosaic, width, height, cfa, camera_to_srgb, flip, None)
}

/// Estimate neutral highlights in saturated sensor channels. `sensor_white`
/// is the calibrated sensor white after black subtraction, normalization and
/// the *effective* WB, indexed by CFA phase (not RGB channel). Thus an extended
/// RGB value caused by WB or the colour matrix is not itself clipping evidence.
/// Unclipped channels keep their demosaiced values, including negatives and
/// values above one. This does not reconstruct lost scene colour or texture.
pub fn develop_with_highlights(
    mosaic: &[f32],
    width: u32,
    height: u32,
    cfa: [u32; 4],
    camera_to_srgb: [f32; 9],
    flip: u32,
    sensor_white: [f32; 4],
) -> Result<LinearImage> {
    ensure!(
        sensor_white.iter().all(|v| v.is_finite() && *v > 0.),
        "Soglie di saturazione Bayer non valide"
    );
    develop_impl(
        mosaic,
        width,
        height,
        cfa,
        camera_to_srgb,
        flip,
        Some(sensor_white),
    )
}

fn develop_impl(
    mosaic: &[f32],
    width: u32,
    height: u32,
    cfa: [u32; 4],
    camera_to_srgb: [f32; 9],
    flip: u32,
    sensor_white: Option<[f32; 4]>,
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
    let inverse_white = sensor_white.map(|white| white.map(f32::recip));
    let confidence = |i: usize| {
        let Some(inverse) = inverse_white else {
            return 0.;
        };
        let phase = (i / w % 2) * 2 + i % w % 2;
        // A narrow, continuous shoulder accommodates samples/noise just below
        // sensor white. Never infer clipping from demosaic overshoot. Bilinear
        // nonnegative weights below avoid ringing in the confidence itself.
        let t = ((mosaic[i] * inverse[phase] - 0.98) / 0.02).clamp(0., 1.);
        t * t * (3. - 2. * t)
    };
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
            let mut clipped = [0.; 3];
            if sensor_white.is_some() {
                clipped[1] = if color(x, y) == 1 {
                    confidence(i)
                } else {
                    [(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .iter()
                        .map(|(dx, dy)| confidence(at(x as isize + dx, y as isize + dy)))
                        .sum::<f32>()
                        * 0.25
                };
            }
            for c in [0, 2] {
                if color(x, y) == c {
                    rgb[c as usize] = mosaic[i];
                    clipped[c as usize] = confidence(i);
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
                if sensor_white.is_some() {
                    clipped[c as usize] = offsets
                        .iter()
                        .map(|(dx, dy)| confidence(at(x as isize + dx, y as isize + dy)))
                        .sum::<f32>()
                        / offsets.len() as f32;
                }
            }
            if clipped.iter().any(|v| *v > 0.) {
                // A clipped channel is a lower bound, not a colour measurement.
                // Lift only that channel toward the strongest balanced channel.
                // All clipped -> neutral; one clipped -> retain measured colour
                // in the others. No channel is lowered, no headroom is clamped,
                // and no colour is borrowed across objects or distant regions.
                let neutral = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
                for c in 0..3 {
                    if clipped[c] > 0. {
                        rgb[c] += clipped[c] * (neutral - rgb[c]);
                    }
                }
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
    fn sensor_highlights_keep_unclipped_extended_values_and_demosaic_overshoot_exact() {
        let (w, h) = (13, 11);
        for cfa in PATTERNS {
            let white = cfa.map(|c| [4., 1., 2.][c as usize]);
            let mosaic: Vec<_> = (0..w * h)
                .map(|i| {
                    let phase = (i / w % 2) * 2 + i % w % 2;
                    // Sharp texture provokes interpolation overshoot, while
                    // every actual sensor sample remains below the shoulder.
                    (((i * 137) % 997) as f32 / 1000. - 0.1) * white[phase as usize]
                })
                .collect();
            for flip in [0, 3, 5, 6] {
                let original = develop(&mosaic, w, h, cfa, ID, flip).unwrap();
                let corrected =
                    develop_with_highlights(&mosaic, w, h, cfa, ID, flip, white).unwrap();
                assert_eq!(original.pixels, corrected.pixels);
                assert!(corrected.pixels.iter().flatten().any(|v| *v > 1.));
                assert!(corrected.pixels.iter().flatten().any(|v| *v < 0.));
            }
        }
    }

    #[test]
    fn sensor_highlights_are_neutral_through_partial_and_complete_saturation() {
        let (w, h) = (9, 7);
        for gains in [[2.246, 1., 1.234], [0.25, 1., 4.], [1.; 3]] {
            for cfa in PATTERNS {
                let white = cfa.map(|c| gains[c as usize]);
                for exposure in [0.2_f32, 0.98, 0.99, 1., 1.2, 2., 3., 5.] {
                    let mosaic: Vec<_> = (0..w * h)
                        .map(|i| exposure.min(white[((i / w % 2) * 2 + i % w % 2) as usize]))
                        .collect();
                    for flip in [0, 3, 5, 6] {
                        let image =
                            develop_with_highlights(&mosaic, w, h, cfa, ID, flip, white).unwrap();
                        let expected = exposure.min(gains.into_iter().fold(0., f32::max));
                        let expected = linear_srgb_to_rec2020([expected; 3]);
                        for p in image.pixels {
                            for (value, expected) in p[..3].iter().zip(expected) {
                                assert!(
                                    (value - expected).abs() < 3e-6,
                                    "{gains:?}, {exposure}: {p:?}"
                                );
                            }
                            assert_eq!(p[3], 1.);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn sensor_highlights_preserve_measured_colours_and_only_lift_clipped_channels() {
        let gains = [2.246, 1., 1.234];
        for cfa in PATTERNS {
            let white = cfa.map(|c| gains[c as usize]);
            for rgb in [
                [2.246, 0.1, 0.2],
                [0.1, 1., 0.2],
                [0.1, 0.2, 1.234],
                [1.7, 1., 0.3],
                [0.4, 1., 1.234],
                [2.246, 1., 0.7],
            ] {
                let mosaic: Vec<_> = (0..80)
                    .map(|i| rgb[cfa[(i / 10 % 2) * 2 + i % 10 % 2] as usize])
                    .collect();
                let image = develop_with_highlights(&mosaic, 10, 8, cfa, ID, 0, white).unwrap();
                let neutral = rgb.into_iter().fold(0., f32::max);
                for p in image.pixels {
                    let actual = rec2020_to_linear_srgb([p[0], p[1], p[2]]);
                    for c in 0..3 {
                        let expected = if rgb[c] == gains[c] { neutral } else { rgb[c] };
                        assert!((actual[c] - expected).abs() < 4e-6, "{rgb:?}: {actual:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn sensor_highlight_mask_stays_local_at_bright_edges() {
        let (w, h) = (19, 17);
        for cfa in PATTERNS {
            let white = cfa.map(|c| [2.246, 1., 1.234][c as usize]);
            for diagonal in [false, true] {
                let coordinate = |x: u32, y: u32| if diagonal { x + y } else { x * 2 };
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| {
                        if coordinate(i % w, i / w) < 18 {
                            0.2
                        } else {
                            white[((i / w % 2) * 2 + i % w % 2) as usize]
                        }
                    })
                    .collect();
                let old = develop(&mosaic, w, h, cfa, ID, 0).unwrap();
                let new = develop_with_highlights(&mosaic, w, h, cfa, ID, 0, white).unwrap();
                for i in 0..w * h {
                    let distance = coordinate(i % w, i / w);
                    if distance < 12 {
                        assert_eq!(old.pixels[i as usize], new.pixels[i as usize]);
                    }
                    if distance >= 24 {
                        let p = new.pixels[i as usize];
                        let expected = linear_srgb_to_rec2020([2.246; 3]);
                        for c in 0..3 {
                            assert!((p[c] - expected[c]).abs() < 4e-6);
                        }
                    }
                }
                for flip in [3, 5, 6] {
                    let rotated =
                        develop_with_highlights(&mosaic, w, h, cfa, ID, flip, white).unwrap();
                    for y in 0..h {
                        for x in 0..w {
                            let (ox, oy) = match flip {
                                3 => (w - 1 - x, h - 1 - y),
                                5 => (y, w - 1 - x),
                                _ => (h - 1 - y, x),
                            };
                            assert_eq!(
                                new.pixels[(y * w + x) as usize],
                                rotated.pixels[(oy * rotated.width + ox) as usize]
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn sensor_highlights_shoulder_is_continuous_and_rejects_invalid_calibration() {
        let cfa = PATTERNS[0];
        let white = [2.246, 1., 1., 1.234];
        let sample = |green: f32| {
            let mosaic: Vec<_> = (0..64)
                .map(|i| [1.5, green, 1.1][cfa[(i / 8 % 2) * 2 + i % 2] as usize])
                .collect();
            develop_with_highlights(&mosaic, 8, 8, cfa, ID, 0, white)
                .unwrap()
                .pixels[0]
        };
        for boundary in [0.98, 1.] {
            let a = sample(boundary - 1e-6);
            let b = sample(boundary + 1e-6);
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 5e-6);
            }
        }
        for bad in [0., -1., f32::NAN, f32::INFINITY] {
            assert!(develop_with_highlights(&[0.; 64], 8, 8, cfa, ID, 0, [bad; 4]).is_err());
        }
    }

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
