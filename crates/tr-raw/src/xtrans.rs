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

/// Camera RGB before the colour matrix and display orientation. Keeping the
/// per-pixel interface avoids an extra full RGB image in the product pipeline.
pub(crate) struct CameraRgb<'a> {
    mosaic: &'a [f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    green: Vec<f32>,
    neighbors: [[Vec<Neighbor>; 3]; 36],
}

fn neighbor_index(x: usize, y: usize, n: Neighbor, width: usize, height: usize) -> Option<usize> {
    let xx = x as isize + n.dx;
    let yy = y as isize + n.dy;
    (xx >= 0 && xx < width as isize && yy >= 0 && yy < height as isize)
        .then(|| yy as usize * width + xx as usize)
}

impl<'a> CameraRgb<'a> {
    pub(crate) fn new(mosaic: &'a [f32], width: usize, height: usize, cfa: Cfa) -> Self {
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
                    if let Some(k) = neighbor_index(x, y, n, width, height) {
                        sum += n.weight * mosaic[k];
                        weights += n.weight;
                    }
                }
                green[i] = sum / weights;
            }
        }
        Self {
            mosaic,
            width,
            height,
            cfa,
            green,
            neighbors,
        }
    }

    pub(crate) fn at(&self, x: usize, y: usize) -> [f32; 3] {
        let i = y * self.width + x;
        let mut rgb = [0., self.green[i], 0.];
        for c in [0, 2] {
            if self.cfa.color(x, y) == c {
                rgb[c] = self.mosaic[i];
                continue;
            }
            let (mut sum, mut weights) = (0., 0.);
            for &n in &self.neighbors[(y % 6) * 6 + x % 6][c] {
                if let Some(k) = neighbor_index(x, y, n, self.width, self.height) {
                    let weight = n.weight / (1. + 64. * (self.green[i] - self.green[k]).abs());
                    sum += weight * (self.mosaic[k] - self.green[k]);
                    weights += weight;
                }
            }
            rgb[c] = self.green[i] + sum / weights;
        }
        rgb
    }
}

#[cfg(test)]
pub(crate) fn baseline_develop(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    let camera_rgb = CameraRgb::new(mosaic, width, height, cfa);
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    // Historical E8 research reference retains its original matrix arithmetic.
    let matrix = matrix.map(|r| r.map(|v| v as f32));
    for cy in 0..h {
        for cx in 0..w {
            let (x, y) = (cx + left, cy + top);
            let rgb = camera_rgb.at(x, y);
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
pub(crate) fn develop(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    develop_supported(
        (mosaic, crate::render::Support::default()),
        width,
        height,
        cfa,
        matrix,
        crop,
        orientation,
    )
}

pub(crate) fn develop_supported(
    (mosaic, support): (&[f32], crate::render::Support<'_>),
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    crate::xtrans_dr6::bounded::reconstruct_with(
        mosaic,
        width,
        height,
        cfa,
        |m, w, h, c, x, y| {
            crate::xtrans_steps::reconstruct_supported(m, w, h, c, support.shifted(x, y))
        },
        || false,
        |x, y, rgb| {
            if x < left || y < top || x >= left + w || y >= top + h {
                return Ok(());
            }
            let (cx, cy) = (x - left, y - top);
            let rec = crate::color::project(matrix, rgb);
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
            Ok(())
        },
    )?;
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
