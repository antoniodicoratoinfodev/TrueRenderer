//! Separate realization of TrueRenderer's directional green / colour-difference
//! construction. Measured sites are preserved. Crop follows interpolation.
use crate::color::Matrix;
use anyhow::Result;
use tr_core::color::LinearImage;

fn reflect(i: isize, length: usize) -> usize {
    let period = 2 * (length as isize - 1);
    let i = i.rem_euclid(period);
    if i < length as isize {
        i as usize
    } else {
        (period - i) as usize
    }
}

pub(crate) fn oriented_size(w: u32, h: u32, orientation: u32) -> (u32, u32) {
    if orientation >= 5 { (h, w) } else { (w, h) }
}

pub(crate) fn develop(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: [usize; 4],
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    let at = |x: isize, y: isize| reflect(y, height) * width + reflect(x, width);
    let color = |x: usize, y: usize| cfa[(y % 2) * 2 + x % 2];
    let mut green = vec![0.; mosaic.len()];
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            if color(x, y) == 1 {
                green[i] = mosaic[i];
                continue;
            }
            let (x, y) = (x as isize, y as isize);
            let (l, r, u, d) = (
                mosaic[at(x - 1, y)],
                mosaic[at(x + 1, y)],
                mosaic[at(x, y - 1)],
                mosaic[at(x, y + 1)],
            );
            let dh = 2. * mosaic[i] - mosaic[at(x - 2, y)] - mosaic[at(x + 2, y)];
            let dv = 2. * mosaic[i] - mosaic[at(x, y - 2)] - mosaic[at(x, y + 2)];
            let gh = (l + r) * 0.5 + dh * 0.25;
            let gv = (u + d) * 0.5 + dv * 0.25;
            let horizontal = (l - r).abs() + dh.abs();
            let vertical = (u - d).abs() + dv.abs();
            green[i] = if horizontal < vertical {
                gh
            } else if vertical < horizontal {
                gv
            } else {
                (gh + gv) * 0.5
            };
        }
    }
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    let m = matrix.map(|row| row.map(|v| v as f32));
    for cy in 0..h {
        for cx in 0..w {
            let (x, y) = (cx + left, cy + top);
            let i = y * width + x;
            let mut rgb = [0., green[i], 0.];
            for c in [0, 2] {
                if color(x, y) == c {
                    rgb[c] = mosaic[i];
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
                rgb[c] = green[i] + delta / offsets.len() as f32;
            }
            let rec = m.map(|r| r[0] * rgb[0] + r[1] * rgb[1] + r[2] * rgb[2]);
            let (ox, oy) = match orientation {
                2 => (w - 1 - cx, cy),
                3 => (w - 1 - cx, h - 1 - cy),
                4 => (cx, h - 1 - cy),
                5 => (cy, cx),
                6 => (h - 1 - cy, cx),
                7 => (h - 1 - cy, w - 1 - cx),
                8 => (cy, w - 1 - cx),
                _ => (cx, cy),
            };
            pixels[oy * ow as usize + ox] = [rec[0], rec[1], rec[2], 1.];
        }
    }
    LinearImage::new(ow, oh, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directional_construction_matches_legacy_with_controlled_matrix() {
        let (w, h) = (17, 13);
        // Same f32 operation order as the old two-matrix path with identity
        // camera->sRGB; compare all samples including negative/high headroom.
        let rec = [
            [0.627404, 0.329282, 0.0433136],
            [0.069097, 0.91954, 0.0113612],
            [0.0163916, 0.0880132, 0.895595],
        ];
        let mosaic: Vec<_> = (0..w * h)
            .map(|i| (i * 137 % 997) as f32 / 400. - 0.1)
            .collect();
        for cfa in [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]] {
            let old = tr_core::demosaic::develop(
                &mosaic,
                w as u32,
                h as u32,
                cfa.map(|c| c as u32),
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                0,
            )
            .unwrap();
            let new = develop(&mosaic, w, h, cfa, rec, [0, 0, w, h], 1).unwrap();
            assert_eq!(old.pixels, new.pixels);
            let camera = develop(&mosaic, w, h, cfa, crate::color::ID, [0, 0, w, h], 1).unwrap();
            for i in 0..w * h {
                let c = cfa[(i / w % 2) * 2 + i % w % 2];
                assert_eq!(camera.pixels[i][c], mosaic[i]);
            }
        }
    }
}
