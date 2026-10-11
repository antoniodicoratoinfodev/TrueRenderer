//! Own X-Trans baseline: distance-weighted green, edge-weighted colour
//! differences. Measured sites and extended float values remain unchanged.
//! This is not Markesteijn or a claim of measured colour/aliasing superiority.
use crate::{cfa::Cfa, color::Matrix, render::oriented_size};
use anyhow::Result;
use tr_core::color::LinearImage;

#[derive(Clone, Copy)]
struct Neighbor {
    dx: isize,
    dy: isize,
    weight: f32,
}

pub(crate) fn develop(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    // Precompute same-channel neighbors from the actual CFA phase. Boundary
    // pixels omit out-of-frame sites; mirroring arbitrary phases is incorrect.
    let neighbors: [[Vec<Neighbor>; 3]; 36] = std::array::from_fn(|phase| {
        std::array::from_fn(|channel| {
            let mut list = vec![];
            for dy in -3isize..=3 {
                for dx in -3isize..=3 {
                    let distance = dx * dx + dy * dy;
                    if distance == 0 || distance > 10 {
                        continue;
                    }
                    let x = (phase % 6) as isize + dx + 6;
                    let y = (phase / 6) as isize + dy + 6;
                    if cfa.color(x as usize, y as usize) == channel {
                        list.push(Neighbor {
                            dx,
                            dy,
                            weight: 1. / distance as f32,
                        });
                    }
                }
            }
            list
        })
    });
    let index = |x: usize, y: usize, n: Neighbor| -> Option<usize> {
        let xx = x as isize + n.dx;
        let yy = y as isize + n.dy;
        (xx >= 0 && xx < width as isize && yy >= 0 && yy < height as isize)
            .then(|| yy as usize * width + xx as usize)
    };
    let mut green = vec![0.; mosaic.len()];
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            if cfa.color(x, y) == 1 {
                green[i] = mosaic[i];
                continue;
            }
            let (mut sum, mut weights) = (0., 0.);
            for &n in &neighbors[(y % 6) * 6 + x % 6][1] {
                if let Some(k) = index(x, y, n) {
                    sum += n.weight * mosaic[k];
                    weights += n.weight;
                }
            }
            green[i] = sum / weights;
        }
    }
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    let matrix = matrix.map(|r| r.map(|v| v as f32));
    for cy in 0..h {
        for cx in 0..w {
            let (x, y) = (cx + left, cy + top);
            let i = y * width + x;
            let mut rgb = [0., green[i], 0.];
            for c in [0, 2] {
                if cfa.color(x, y) == c {
                    rgb[c] = mosaic[i];
                    continue;
                }
                let (mut sum, mut weights) = (0., 0.);
                for &n in &neighbors[(y % 6) * 6 + x % 6][c] {
                    if let Some(k) = index(x, y, n) {
                        let weight = n.weight / (1. + 64. * (green[i] - green[k]).abs());
                        sum += weight * (mosaic[k] - green[k]);
                        weights += weight;
                    }
                }
                rgb[c] = green[i] + sum / weights;
            }
            let rec = matrix.map(|r| r[0] * rgb[0] + r[1] * rgb[1] + r[2] * rgb[2]);
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
    fn phases_borders_measured_sites_and_extended_flat_fields() {
        let (w, h) = (29, 23);
        for dy in 0..6 {
            for dx in 0..6 {
                let cfa = Cfa::new(6, &crate::cfa::XTRANS).unwrap().shifted(dx, dy);
                let colors = [-0.1, 0.37, 1.4];
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| colors[cfa.color(i % w, i / w)])
                    .collect();
                let image = develop(&mosaic, w, h, cfa, crate::color::ID, [0, 0, w, h], 1).unwrap();
                for (i, p) in image.pixels.iter().enumerate() {
                    assert_eq!(p[cfa.color(i % w, i / w)], mosaic[i]);
                    for c in 0..3 {
                        assert!((p[c] - colors[c]).abs() < 1e-6);
                    }
                }
            }
        }
    }
}
