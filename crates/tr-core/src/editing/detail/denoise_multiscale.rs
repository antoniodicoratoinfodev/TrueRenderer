//! MultiscaleV2: brightness-conditioned spectral noise and a chroma pyramid.
//! All operations use extended linear Rec.2020. The pyramid replaces only
//! estimated low-frequency chroma; luminance detail never passes through it.
use super::{denoise, luma, straight};
use crate::color::{LinearImage, Pixel};
use anyhow::{Result, ensure};
use rayon::prelude::*;

struct Band<const N: usize> {
    level: f32,
    sigma: [[Pixel; N]; N],
}
struct Model<const N: usize> {
    bands: Vec<Band<N>>,
}

fn quantile(values: &mut [f32], q: f32) -> f32 {
    let i = ((values.len() - 1) as f32 * q) as usize;
    *values.select_nth_unstable_by(i, f32::total_cmp).1
}

impl<const N: usize> Model<N> {
    fn estimate(
        image: &LinearImage,
        transform: &denoise::Transform<N>,
        coarse: bool,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        let global = denoise::model(image, transform, cancelled)?;
        let (w, h) = (image.width as usize, image.height as usize);
        let (nx, ny) = (32.min(w.div_ceil(N)), 32.min(h.div_ceil(N)));
        let mut samples = Vec::with_capacity(nx * ny);
        for iy in 0..ny {
            ensure!(!cancelled(), "Editing annullato");
            for ix in 0..nx {
                let x = ix * w.saturating_sub(N) / nx.saturating_sub(1).max(1);
                let y = iy * h.saturating_sub(N) / ny.saturating_sub(1).max(1);
                if let Some((mut block, level)) = transform.block(image, x as isize, y as isize) {
                    for p in block.iter_mut().flatten() {
                        for v in p {
                            *v = v.abs() / level;
                        }
                    }
                    samples.push((level, block));
                }
            }
        }
        samples.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut bands = Vec::new();
        // At least 64 samples per brightness band, at most four bands. This
        // permits both shot-noise RAW and already tone-encoded JPEG responses;
        // no ISO/camera assumption or display-domain filtering is involved.
        let count = (samples.len() / 64).clamp(1, 4);
        for band in 0..count {
            ensure!(!cancelled(), "Editing annullato");
            let samples =
                &samples[band * samples.len() / count..(band + 1) * samples.len() / count];
            if samples.is_empty() {
                continue;
            }
            let mut sigma = [[[0.; 4]; N]; N];
            let mut values = Vec::with_capacity(samples.len().max(N * N));
            for (y, row) in sigma.iter_mut().enumerate() {
                for (x, p) in row.iter_mut().enumerate() {
                    for (c, v) in p[..3].iter_mut().enumerate() {
                        values.clear();
                        values.extend(samples.iter().map(|(_, b)| b[y][x][c]));
                        *v = quantile(&mut values, 0.3) / 0.385_32;
                    }
                }
            }
            let mut gain = [1.; 3];
            let mut cap = [0.; 3];
            // Keep the robust global spectral shape. Estimating each band's
            // low frequencies independently confuses saturated surfaces with
            // noise; only a pooled fine-frequency gain may vary by brightness.
            for c in 0..3 {
                values.clear();
                for (y, row) in sigma.iter().enumerate() {
                    for (x, p) in row.iter().enumerate() {
                        if (N / 2..=N + N / 2).contains(&(x + y)) && global[y][x][c] > 1e-8 {
                            values.push(p[c] / global[y][x][c]);
                        }
                    }
                }
                if !values.is_empty() {
                    gain[c] = quantile(&mut values, 0.5).clamp(0.25, 4.);
                }
                values.clear();
                for (y, row) in global.iter().enumerate() {
                    for (x, p) in row.iter().enumerate() {
                        if (N / 2..=N + N / 2).contains(&(x + y)) {
                            values.push(p[c]);
                        }
                    }
                }
                // On coarse levels real broad color transitions need a much
                // tighter cap than the native demosaic-frequency estimate.
                cap[c] = if coarse { 1.5 } else { 8. } * quantile(&mut values, 0.5);
            }
            for (y, row) in sigma.iter_mut().enumerate() {
                for (x, p) in row.iter_mut().enumerate() {
                    for c in 0..3 {
                        p[c] = global[y][x][c].min(cap[c]) * gain[c];
                    }
                }
            }
            sigma[0][0] = [0.; 4];
            bands.push(Band {
                level: samples[samples.len() / 2].0,
                sigma,
            });
        }
        Ok(Self { bands })
    }

    fn active(&self, amounts: [f32; 3]) -> bool {
        self.bands.iter().any(|b| {
            b.sigma
                .iter()
                .flatten()
                .any(|p| (0..3).any(|c| amounts[c] > 0. && p[c] > 1e-8))
        })
    }

    fn shrink(&self, block: &mut [[Pixel; N]; N], level: f32, amounts: [f32; 3], threshold: f32) {
        let hi = self
            .bands
            .partition_point(|b| b.level < level)
            .min(self.bands.len() - 1);
        let lo = hi.saturating_sub(1);
        let a = &self.bands[lo];
        let b = &self.bands[hi];
        let blend = ((level - a.level) / (b.level - a.level).max(1e-8)).clamp(0., 1.);
        let removed = |ratio: f32| {
            let t = ((ratio - 0.75) / 0.75).clamp(0., 1.);
            1. - t * t * (3. - 2. * t)
        };
        for (y, row) in block.iter_mut().enumerate() {
            for (x, p) in row.iter_mut().enumerate() {
                let sigma: [f32; 3] = std::array::from_fn(|c| {
                    (a.sigma[y][x][c] * (1. - blend) + b.sigma[y][x][c] * blend)
                        * level
                        * (1. + threshold)
                });
                p[0] *= if amounts[0] == 0. {
                    0.
                } else {
                    removed(p[0].abs() / (sigma[0] * amounts[0] * 3.).max(1e-12))
                };
                // Both opponent coefficients share their retention factor:
                // significant color edges keep their direction (hue).
                let strength = amounts[1] * 3.25;
                let ratio = ((p[1] * p[1] + p[2] * p[2])
                    / ((sigma[1] * sigma[1] + sigma[2] * sigma[2]) * strength * strength)
                        .max(1e-24))
                .sqrt();
                let factor = if amounts[1] == 0. { 0. } else { removed(ratio) };
                p[1] *= factor;
                p[2] *= factor;
            }
        }
    }
}

fn downsample(image: &LinearImage, cancelled: &(impl Fn() -> bool + Sync)) -> Result<LinearImage> {
    let (w, h) = (image.width.div_ceil(2), image.height.div_ceil(2));
    let mut pixels = vec![[0.; 4]; w as usize * h as usize];
    crate::compute::install(|| {
        pixels
            .par_chunks_mut(w as usize)
            .enumerate()
            .try_for_each(|(y, row)| -> Result<()> {
                ensure!(!cancelled(), "Editing annullato");
                for (x, out) in row.iter_mut().enumerate() {
                    let mut sum = [0_f64; 4];
                    let mut valid = true;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let xx = (x * 2 + dx).min(image.width as usize - 1);
                            let yy = (y * 2 + dy).min(image.height as usize - 1);
                            let p = image.pixels[yy * image.width as usize + xx];
                            valid &= p[3] > 0. && p.iter().all(|v| v.is_finite());
                            for c in 0..4 {
                                sum[c] += f64::from(p[c]);
                            }
                        }
                    }
                    if valid {
                        *out = sum.map(|v| (v * 0.25) as f32);
                    }
                }
                Ok(())
            })
    })?;
    LinearImage::new(w, h, pixels)
}

fn merge_chroma(
    fine: &mut LinearImage,
    guide: &LinearImage,
    coarse: &LinearImage,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<()> {
    let mut delta = downsample(fine, cancelled)?;
    for (p, q) in delta.pixels.iter_mut().zip(&coarse.pixels) {
        if p[3] > 0. && q[3] > 0. {
            let a = straight(*p);
            let b = straight(*q);
            let dy = luma(b) - luma(a);
            *p = [b[0] - a[0] - dy, 0., b[2] - a[2] - dy, 1.];
        } else {
            *p = [0.; 4];
        }
    }
    let w = fine.width as usize;
    crate::compute::install(|| {
        fine.pixels.par_chunks_mut(w * 64).enumerate().try_for_each(
            |(stripe, rows)| -> Result<()> {
                ensure!(!cancelled(), "Editing annullato");
                for (i, p) in rows.iter_mut().enumerate() {
                    if p[3] <= 0. {
                        continue;
                    }
                    let (x, y) = (i % w, stripe * 64 + i / w);
                    let (fx, fy) = (x as f32 * 0.5 - 0.25, y as f32 * 0.5 - 0.25);
                    let (ix, iy) = (fx.floor() as isize, fy.floor() as isize);
                    let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
                    let rgb = straight(*p);
                    let y0 = luma(rgb);
                    let mut sum = [0.; 3];
                    for (dy, wy) in [(0, 1. - ty), (1, ty)] {
                        for (dx, wx) in [(0, 1. - tx), (1, tx)] {
                            let xx = (ix + dx).clamp(0, coarse.width as isize - 1) as usize;
                            let yy = (iy + dy).clamp(0, coarse.height as isize - 1) as usize;
                            let j = yy * coarse.width as usize + xx;
                            let d = delta.pixels[j];
                            let g = guide.pixels[j];
                            if d[3] <= 0. || g[3] <= 0. {
                                continue;
                            }
                            let g = straight(g);
                            let gy = luma(g);
                            let range_y = 0.02 + 0.2 * y0.abs();
                            let range_c = 0.015 + 0.08 * (y0.abs() + 0.003).sqrt();
                            let distance = ((y0 - gy) / range_y).powi(2)
                                + ((rgb[0] - y0 - g[0] + gy) / range_c).powi(2)
                                + ((rgb[2] - y0 - g[2] + gy) / range_c).powi(2);
                            let weight = wx * wy / (1. + distance).powi(2);
                            sum[0] += weight * d[0];
                            sum[1] += weight * d[2];
                            sum[2] += weight;
                        }
                    }
                    if sum[2] > 1e-12 {
                        // If none of the coarse neighbors resembles the fine
                        // pixel, fade the correction instead of normalizing a
                        // tiny weight into a full-strength edge/color bleed.
                        let confidence = (sum[2] * 2.).min(1.);
                        let (dr, db) = (sum[0] / sum[2] * confidence, sum[1] / sum[2] * confidence);
                        p[0] += dr * p[3];
                        p[1] -= (0.2627 * dr + 0.0593 * db) / 0.678 * p[3];
                        p[2] += db * p[3];
                    }
                }
                Ok(())
            },
        )
    })
}

fn run<const N: usize>(
    source: &LinearImage,
    amounts: [f32; 3],
    threshold: f32,
    levels: u32,
    coarse_level: bool,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<LinearImage> {
    let transform = denoise::Transform::<N>::new();
    let model = Model::estimate(source, &transform, coarse_level, cancelled)?;
    let pixels = if model.active(amounts) {
        denoise::filter(source, &transform, cancelled, &|block, level| {
            model.shrink(block, level, amounts, threshold)
        })?
    } else {
        source.pixels.clone()
    };
    let mut output = LinearImage::new(source.width, source.height, pixels)?;
    if amounts[1] > 0. && levels > 0 && source.width.min(source.height) >= 32 {
        let guide = downsample(source, cancelled)?;
        let coarse = run::<N>(
            &guide,
            [0., amounts[1], amounts[1]],
            threshold,
            levels - 1,
            true,
            cancelled,
        )?;
        merge_chroma(&mut output, &guide, &coarse, cancelled)?;
    }
    ensure!(!cancelled(), "Editing annullato");
    Ok(output)
}

pub(super) fn apply(
    image: &mut LinearImage,
    scale: f32,
    luminance: f32,
    chroma: f32,
    threshold: f32,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<()> {
    ensure!(!cancelled(), "Editing annullato");
    let amounts = [luminance, chroma, chroma];
    let output = if scale <= 0.3 {
        run::<4>(image, amounts, threshold, 1, false, cancelled)?
    } else if scale <= 0.6 {
        run::<8>(image, amounts, threshold, 2, false, cancelled)?
    } else {
        run::<16>(image, amounts, threshold, 3, false, cancelled)?
    };
    image.pixels = output.pixels;
    Ok(())
}
