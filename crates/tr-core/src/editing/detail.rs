//! Bounded CPU filters, process 3. Two scratch rasters at most; no per-pixel
//! allocation. Native-pixel radii are scaled for explicitly provisional mips.
use crate::color::{LinearImage, Pixel};
use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detail {
    pub texture: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub sharpen: f32,
    pub radius: f32,
    pub threshold: f32,
    pub luminance_noise: f32,
    pub chroma_noise: f32,
}
impl Default for Detail {
    fn default() -> Self {
        Self {
            texture: 0.,
            clarity: 0.,
            dehaze: 0.,
            sharpen: 0.,
            radius: 1.,
            threshold: 0.,
            luminance_noise: 0.,
            chroma_noise: 0.,
        }
    }
}
impl Detail {
    pub fn active(&self) -> bool {
        [
            self.texture,
            self.clarity,
            self.dehaze,
            self.sharpen,
            self.luminance_noise,
            self.chroma_noise,
        ]
        .iter()
        .any(|v| *v != 0.)
    }
    pub fn validate(&self) -> Result<()> {
        for v in [self.texture, self.clarity, self.dehaze] {
            super::range(v, -100., 100., "Presenza")?;
        }
        super::range(self.sharpen, 0., 150., "Nitidezza")?;
        super::range(self.radius, 0.3, 3., "Raggio")?;
        for v in [self.threshold, self.luminance_noise, self.chroma_noise] {
            super::range(v, 0., 100., "Dettaglio")?;
        }
        Ok(())
    }
    pub fn apply(&self, image: &mut LinearImage, native: [u32; 2]) {
        self.apply_cancellable(image, native, &|| false).unwrap();
    }
    pub(super) fn apply_cancellable(
        &self,
        image: &mut LinearImage,
        native: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        if !self.active() {
            return Ok(());
        }
        let scale = image.width.max(image.height) as f32 / native[0].max(native[1]) as f32;
        if self.luminance_noise > 0. || self.chroma_noise > 0. {
            let blurred = blur(image, (2. * scale).round().max(1.) as u32);
            super::pixels_mut(&mut image.pixels, |i, p| {
                let b = blurred[i];
                if p[3] <= 0. || b[3] <= 0. {
                    return;
                }
                let rgb = straight(*p);
                let avg = straight(b);
                let y = luma(rgb);
                let by = luma(avg);
                let edge = 1.
                    / (1.
                        + ((y - by)
                            / (0.01 + 0.12 * self.luminance_noise.max(self.chroma_noise) / 100.))
                            .powi(2));
                let dy = (by - y) * self.luminance_noise / 100. * edge;
                for c in 0..3 {
                    p[c] = (rgb[c]
                        + dy
                        + ((avg[c] - by) - (rgb[c] - y)) * self.chroma_noise / 100. * edge)
                        * p[3];
                }
            });
        }
        // Independent spatial scales and halo limiting; no implicit sharpening.
        for (node, (amount, radius, midtones)) in [
            (
                self.texture / 100.,
                (2. * scale).round().max(1.) as u32,
                false,
            ),
            (
                self.clarity / 100.,
                (native[0].max(native[1]) as f32 * 0.008 * scale)
                    .round()
                    .clamp(1., 512.) as u32,
                true,
            ),
            (
                self.sharpen / 100.,
                (self.radius * scale).round().max(1.) as u32,
                false,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            anyhow::ensure!(!cancelled(), "Editing annullato");
            if amount == 0. {
                continue;
            }
            let blurred = if node == 2 {
                gaussian(image, (self.radius * scale).max(0.15))
            } else {
                blur(image, radius)
            };
            super::pixels_mut(&mut image.pixels, |i, p| {
                let b = blurred[i];
                if p[3] <= 0. || b[3] <= 0. {
                    return;
                }
                let rgb = straight(*p);
                let y = luma(rgb);
                let low = luma(straight(b));
                let difference = y - low;
                let threshold = self.threshold / 100. * 0.02;
                let detail = difference.signum() * (difference.abs() - threshold).max(0.);
                let protection = if midtones {
                    (4. * y.clamp(0., 1.) * (1. - y.clamp(0., 1.))).sqrt()
                } else {
                    1.
                };
                let limit = 0.05 + y.abs() * 0.25;
                let delta = (amount * detail * protection).clamp(-limit, limit);
                for c in 0..3 {
                    p[c] = (rgb[c] + delta) * p[3];
                }
            });
        }
        if self.dehaze != 0. {
            anyhow::ensure!(!cancelled(), "Editing annullato");
            // Explicit local veil model. Robust scalar airlight avoids invented
            // per-channel illuminants; no claim of physical scene reconstruction.
            let step = (image.pixels.len() / 4096).max(1);
            let mut samples: Vec<f32> = image
                .pixels
                .iter()
                .step_by(step)
                .filter(|p| p[3] > 0.95)
                .map(|p| luma(straight(*p)))
                .filter(|y| *y > 0.)
                .collect();
            samples.sort_by(f32::total_cmp);
            let air = samples
                .get(samples.len().saturating_sub(1) * 99 / 100)
                .copied()
                .unwrap_or(1.)
                .max(0.01);
            let blurred = blur(image, (image.width.max(image.height) / 64).clamp(1, 128));
            super::pixels_mut(&mut image.pixels, |i, p| {
                let b = blurred[i];
                if p[3] <= 0. || b[3] <= 0. {
                    return;
                }
                let avg = straight(b);
                let dark = avg.into_iter().fold(f32::INFINITY, f32::min).max(0.);
                let amount = self.dehaze / 100.;
                let transmission = (1. - amount.abs() * 0.8 * (dark / air).clamp(0., 1.)).max(0.2);
                for c in 0..3 {
                    let v = p[c] / p[3];
                    p[c] = (if amount > 0. {
                        (v - air) / transmission + air
                    } else {
                        v * transmission + air * (1. - transmission)
                    }) * p[3];
                }
            });
        }
        Ok(())
    }
}
pub(super) fn luma(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}
pub(super) fn straight(p: Pixel) -> [f32; 3] {
    [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
}

pub(super) fn defringe(image: &mut LinearImage, amount: f32) {
    if amount == 0. {
        return;
    }
    let blurred = blur(image, 1);
    super::pixels_mut(&mut image.pixels, |i, p| {
        let b = blurred[i];
        if p[3] <= 0. || b[3] <= 0. {
            return;
        }
        let rgb = straight(*p);
        let y = luma(rgb);
        let by = luma(straight(b));
        let edge = ((y - by).abs() / (0.03 + y.abs())).clamp(0., 1.);
        let [r, g, b] = rgb;
        let purple = (r.min(b) - g).max(0.);
        let green = (g - r.max(b)).max(0.);
        let f = amount / 100. * edge * p[3];
        p[0] -= f * purple;
        p[2] -= f * purple;
        p[1] -= f * green;
    });
}

fn rows_mut(pixels: &mut [Pixel], width: usize, apply: impl Fn(usize, &mut [Pixel]) + Send + Sync) {
    if pixels.len() >= 32768 {
        crate::compute::install(|| {
            pixels
                .par_chunks_mut(width)
                .enumerate()
                .for_each(|(i, row)| apply(i, row))
        });
    } else {
        for (i, row) in pixels.chunks_mut(width).enumerate() {
            apply(i, row);
        }
    }
}

fn blur(image: &LinearImage, radius: u32) -> Vec<Pixel> {
    let (w, h) = (image.width as usize, image.height as usize);
    let r = radius as isize;
    let count = f64::from(2 * radius + 1);
    let mut tmp = vec![[0.; 4]; w * h];
    let mut output = vec![[0.; 4]; w * h];
    // f64 sliding sums preserve flat fields, including at image borders.
    rows_mut(&mut tmp, w, |y, row| {
        let get = |x: isize| image.pixels[y * w + x.clamp(0, w as isize - 1) as usize];
        let mut sum = [0_f64; 4];
        for x in -r..=r {
            for (s, v) in sum.iter_mut().zip(get(x)) {
                *s += v as f64;
            }
        }
        for (x, pixel) in row.iter_mut().enumerate() {
            *pixel = sum.map(|s| (s / count) as f32);
            for (c, value) in sum.iter_mut().enumerate() {
                *value += get(x as isize + r + 1)[c] as f64 - get(x as isize - r)[c] as f64;
            }
        }
    });
    // Independent columns retain the original f64 sliding-sum order. Write
    // transposed output, then reuse tmp for the final layout: two scratch frames.
    rows_mut(&mut output, h, |x, column| {
        let get = |y: isize| tmp[y.clamp(0, h as isize - 1) as usize * w + x];
        let mut sum = [0_f64; 4];
        for y in -r..=r {
            for (s, v) in sum.iter_mut().zip(get(y)) {
                *s += v as f64;
            }
        }
        for (y, pixel) in column.iter_mut().enumerate() {
            *pixel = sum.map(|s| (s / count) as f32);
            for (c, value) in sum.iter_mut().enumerate() {
                *value += get(y as isize + r + 1)[c] as f64 - get(y as isize - r)[c] as f64;
            }
        }
    });
    super::pixels_mut(&mut tmp, |i, p| *p = output[(i % w) * h + i / w]);
    tmp
}

fn gaussian(image: &LinearImage, sigma: f32) -> Vec<Pixel> {
    let radius = (sigma * 2.).ceil() as i32;
    let mut kernel: Vec<f64> = (-radius..=radius)
        .map(|x| (-0.5 * (x as f64 / sigma as f64).powi(2)).exp())
        .collect();
    let norm: f64 = kernel.iter().sum();
    for k in &mut kernel {
        *k /= norm;
    }
    let (w, h) = (image.width as i32, image.height as i32);
    let mut tmp = vec![[0.; 4]; image.pixels.len()];
    let mut out = vec![[0.; 4]; image.pixels.len()];
    super::pixels_mut(&mut tmp, |i, target| {
        let (x, y) = (i as i32 % w, i as i32 / w);
        let mut sum = [0_f64; 4];
        for (offset, k) in (-radius..=radius).zip(&kernel) {
            let p = image.pixels[(y * w + (x + offset).clamp(0, w - 1)) as usize];
            for c in 0..4 {
                sum[c] += p[c] as f64 * k;
            }
        }
        *target = sum.map(|v| v as f32);
    });
    super::pixels_mut(&mut out, |i, target| {
        let (x, y) = (i as i32 % w, i as i32 / w);
        let mut sum = [0_f64; 4];
        for (offset, k) in (-radius..=radius).zip(&kernel) {
            let p = tmp[((y + offset).clamp(0, h - 1) * w + x) as usize];
            for c in 0..4 {
                sum[c] += p[c] as f64 * k;
            }
        }
        *target = sum.map(|v| v as f32);
    });
    out
}
