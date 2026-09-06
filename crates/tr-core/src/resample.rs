//! CPU sampling in linear premultiplied light. Pixel coordinates refer to edges;
//! sample centers are at n + 0.5. A final buffer maps 1:1 to backing pixels.
use crate::color::{LinearImage, MAX_PIXELS, Pixel};
use anyhow::{Result, ensure};
use std::{
    f64::consts::PI,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub const VERSION: &str = "cpu-pyramid-lanczos3-guard10-mitchell-alpha-area-triangle-v1";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub size: [u32; 2],
    pub origin: [f64; 2],
    pub step: [f64; 2],
}
impl Region {
    pub fn fitted(source: [u32; 2], size: [u32; 2]) -> Self {
        Self {
            size,
            origin: [0.; 2],
            step: [
                source[0] as f64 / size[0] as f64,
                source[1] as f64 / size[1] as f64,
            ],
        }
    }
}
pub struct Pyramid {
    id: u64,
    levels: Vec<LinearImage>,
    opaque: bool,
}
impl Pyramid {
    pub fn new(source: LinearImage) -> Result<Self> {
        let opaque = source.pixels.iter().all(|p| p[3] == 1.0);
        let mut levels = vec![source];
        while levels.last().is_some_and(|p| p.width > 1 || p.height > 1) {
            let last = levels.last().unwrap();
            let size = [last.width.div_ceil(2), last.height.div_ceil(2)];
            levels.push(filter(
                last,
                Region::fitted([last.width, last.height], size),
                opaque,
            )?);
        }
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            levels,
            opaque,
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn source(&self) -> &LinearImage {
        &self.levels[0]
    }
    pub fn byte_len(&self) -> usize {
        self.levels
            .iter()
            .map(|p| p.pixels.len() * std::mem::size_of::<Pixel>())
            .sum()
    }
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }
    pub fn render(&self, region: Region) -> Result<LinearImage> {
        ensure!(
            (region.size[0] as u64) * (region.size[1] as u64)
                <= crate::color::MAX_PRESENTATION_PIXELS as u64,
            "Viewport oltre quota di 8 Mi pixel"
        );
        let source = self.source();
        let mut level = 0;
        while self.levels.get(level + 1).is_some() {
            let current = &self.levels[level];
            if region.step[0] * current.width as f64 / source.width as f64 <= 2.
                && region.step[1] * current.height as f64 / source.height as f64 <= 2.
            {
                break;
            }
            level += 1;
        }
        let selected = &self.levels[level];
        let ratio = [
            selected.width as f64 / source.width as f64,
            selected.height as f64 / source.height as f64,
        ];
        filter(
            selected,
            Region {
                size: region.size,
                origin: [region.origin[0] * ratio[0], region.origin[1] * ratio[1]],
                step: [region.step[0] * ratio[0], region.step[1] * ratio[1]],
            },
            self.opaque,
        )
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.
    } else {
        (PI * x).sin() / (PI * x)
    }
}
fn lanczos(x: f64) -> f64 {
    if x.abs() < 3. {
        sinc(x) * sinc(x / 3.)
    } else {
        0.
    }
}
fn mitchell(x: f64) -> f64 {
    let x = x.abs();
    if x < 1. {
        (7. * x * x * x - 12. * x * x + 16. / 3.) / 6.
    } else if x < 2. {
        ((-7. / 3.) * x * x * x + 12. * x * x - 20. * x + 32. / 3.) / 6.
    } else {
        0.
    }
}
type Taps = Vec<(usize, f64)>;
fn axis(length: u32, count: u32, origin: f64, step: f64, opaque: bool) -> Vec<Taps> {
    (0..count)
        .map(|i| {
            let center = origin + (i as f64 + 0.5) * step - 0.5;
            // Unit scale aligned to the source centers is a direct LOD-0 read.
            if (step - 1.).abs() < 1e-12 && (center - center.round()).abs() < 1e-10 {
                return vec![(center.round().clamp(0., length as f64 - 1.) as usize, 1.)];
            }
            let down = step > 1.;
            // Smoothly reserve up to 10% bandwidth at 2x decimation. This reduces
            // alias already folded into pyramid levels; versioned, never sharpening.
            let cutoff_scale = step * (1. + 0.1 * (step - 1.).clamp(0., 1.));
            let support = if down {
                if opaque {
                    3. * cutoff_scale
                } else {
                    step / 2. + 0.5
                }
            } else if opaque {
                2.
            } else {
                1.
            };
            let mut taps = Vec::new();
            for j in (center - support).floor() as i64..=(center + support).ceil() as i64 {
                let weight = if down && !opaque {
                    let left = origin + i as f64 * step;
                    let right = left + step;
                    (right.min(j as f64 + 1.) - left.max(j as f64)).max(0.)
                } else if down {
                    lanczos((j as f64 - center) / cutoff_scale)
                } else if opaque {
                    mitchell(j as f64 - center)
                } else {
                    (1. - (j as f64 - center).abs()).max(0.)
                };
                if weight.abs() > 1e-16 {
                    taps.push((j.clamp(0, length as i64 - 1) as usize, weight));
                }
            }
            let sum: f64 = taps.iter().map(|(_, w)| *w).sum();
            for (_, w) in &mut taps {
                *w /= sum;
            }
            taps
        })
        .collect()
}

fn filter(source: &LinearImage, region: Region, opaque: bool) -> Result<LinearImage> {
    let [width, height] = region.size;
    let count = (width as usize).saturating_mul(height as usize);
    ensure!(
        width > 0 && height > 0 && width <= 16384 && height <= 16384 && count <= MAX_PIXELS,
        "Raster di presentazione fuori quota"
    );
    ensure!(
        region
            .origin
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1e9)
            && region
                .step
                .iter()
                .all(|v| v.is_finite() && *v > 0. && *v <= 2.000001),
        "Geometria o rapporto del filtro fuori quota"
    );
    let xs = axis(
        source.width,
        width,
        region.origin[0],
        region.step[0],
        opaque,
    );
    let ys = axis(
        source.height,
        height,
        region.origin[1],
        region.step[1],
        opaque,
    );
    let min_y = ys.iter().flatten().map(|(y, _)| *y).min().unwrap();
    let max_y = ys.iter().flatten().map(|(y, _)| *y).max().unwrap();
    let intermediate_count = (max_y - min_y + 1).saturating_mul(width as usize);
    ensure!(
        intermediate_count <= 2 * MAX_PIXELS + 65536,
        "Intermedio del filtro fuori quota"
    );
    let mut horizontal = vec![[0.; 4]; intermediate_count];
    for y in min_y..=max_y {
        for (x, taps) in xs.iter().enumerate() {
            let mut p = [0.0f64; 4];
            for &(sx, weight) in taps {
                for (c, value) in p.iter_mut().enumerate() {
                    *value += source.pixels[y * source.width as usize + sx][c] as f64 * weight;
                }
            }
            horizontal[(y - min_y) * width as usize + x] = p.map(|v| v as f32);
        }
    }
    let mut pixels = Vec::with_capacity(count);
    for taps in &ys {
        for x in 0..width as usize {
            let mut p = [0.0f64; 4];
            for &(sy, weight) in taps {
                for (c, value) in p.iter_mut().enumerate() {
                    *value += horizontal[(sy - min_y) * width as usize + x][c] as f64 * weight;
                }
            }
            if opaque {
                p[3] = 1.;
            }
            pixels.push(p.map(|v| v as f32));
        }
    }
    LinearImage::new(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligned_identity_and_crop_preserve_original_samples() {
        let source = LinearImage::new(
            13,
            9,
            (0..117)
                .map(|i| [i as f32 / 50. - 0.5, 0.2, 0.3, 1.])
                .collect(),
        )
        .unwrap();
        let pyramid = Pyramid::new(source.clone()).unwrap();
        assert_eq!(
            pyramid
                .render(Region::fitted([13, 9], [13, 9]))
                .unwrap()
                .pixels,
            source.pixels
        );
        let crop = pyramid
            .render(Region {
                size: [5, 3],
                origin: [3., 2.],
                step: [1.; 2],
            })
            .unwrap();
        for y in 0..3 {
            for x in 0..5 {
                assert_eq!(crop.pixels[y * 5 + x], source.pixels[(y + 2) * 13 + x + 3]);
            }
        }
    }
    #[test]
    fn odd_shapes_constant_and_alpha_remain_valid() {
        for [w, h] in [[1, 33], [37, 1], [37, 23]] {
            let p = [0.01, 0.2, 0.3, 0.4];
            let pyramid =
                Pyramid::new(LinearImage::new(w, h, vec![p; (w * h) as usize]).unwrap()).unwrap();
            for size in [[1, 1], [9, 7], [77, 53]] {
                let rendered = pyramid.render(Region::fitted([w, h], size)).unwrap();
                for pixel in rendered.pixels {
                    for (a, b) in pixel.into_iter().zip(p) {
                        assert!((a - b).abs() < 1e-6);
                    }
                }
            }
        }
    }
    #[test]
    fn minification_suppresses_nyquist_pattern_in_linear_light() {
        let source = LinearImage::new(
            512,
            64,
            (0..512 * 64)
                .map(|i| {
                    let v = (i % 2) as f32;
                    [v, v, v, 1.]
                })
                .collect(),
        )
        .unwrap();
        let pyramid = Pyramid::new(source).unwrap();
        let image = pyramid.render(Region::fitted([512, 64], [73, 9])).unwrap();
        for y in 1..8 {
            for x in 4..69 {
                assert!((image.pixels[y * 73 + x][0] - 0.5).abs() < 0.002);
            }
        }
        assert_eq!(
            crate::color::display_pixel(image.pixels[4 * 73 + 36], 119. / 255.),
            [188, 188, 188, 255]
        );
    }
    #[test]
    fn transparent_hidden_colors_and_output_quota() {
        let source = LinearImage::new(2, 1, vec![[0., 0., 0., 0.], [0., 0., 1., 1.]]).unwrap();
        let pyramid = Pyramid::new(source).unwrap();
        let reduced = pyramid.render(Region::fitted([2, 1], [1, 1])).unwrap();
        assert_eq!(reduced.pixels[0], [0., 0., 0.5, 0.5]);
        assert!(
            pyramid
                .render(Region::fitted([2, 1], [u32::MAX, 2]))
                .is_err()
        );
        assert!(
            pyramid
                .render(Region {
                    size: [1, 1],
                    origin: [f64::NAN, 0.],
                    step: [1.; 2]
                })
                .is_err()
        );
    }
}
