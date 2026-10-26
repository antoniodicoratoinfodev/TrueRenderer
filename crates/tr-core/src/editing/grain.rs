//! Creative grain v1, in extended linear Rec.2020 after geometry and before
//! output resizing. Counter-based randomness is independent of worker/thread
//! scheduling. No raster scratch; reduced previews are explicitly provisional.
use crate::color::LinearImage;
use anyhow::{Result, ensure};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Distribution {
    #[default]
    Uniform,
    Gaussian,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grain {
    pub enabled: bool,
    pub distribution: Distribution,
    pub monochrome: bool,
    pub amount: f32,
    /// Spacing in native output pixels, independent of viewport zoom.
    pub size: f32,
    pub opacity: f32,
    pub seed: u32,
}
impl Default for Grain {
    fn default() -> Self {
        Self {
            enabled: false,
            distribution: Distribution::Uniform,
            monochrome: true,
            amount: 20.,
            size: 1.,
            opacity: 100.,
            seed: 0,
        }
    }
}
impl Grain {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn active(&self) -> bool {
        self.enabled && self.amount > 0. && self.opacity > 0.
    }
    pub fn validate(&self) -> Result<()> {
        super::range(self.amount, 0., 100., "Quantità grana")?;
        super::range(self.size, 1., 20., "Dimensione grana")?;
        super::range(self.opacity, 0., 100., "Opacità grana")
    }
    pub(super) fn apply_cancellable(
        &self,
        image: &mut LinearImage,
        native: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        self.apply_region(
            image,
            native,
            [0; 2],
            [image.width, image.height],
            cancelled,
        )
    }
    pub(super) fn apply_region(
        &self,
        image: &mut LinearImage,
        native: [u32; 2],
        origin: [u32; 2],
        dimensions: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        if !self.active() {
            return Ok(());
        }
        let width = image.width as usize;
        let sx = native[0] as f32 / dimensions[0] as f32;
        let sy = native[1] as f32 / dimensions[1] as f32;
        // Approximate unresolved grain energy on provisional mips. The final
        // rendering always evaluates native pixels before the canonical resize.
        let attenuation = (self.size / (sx * sy).sqrt()).min(1.);
        let amplitude = 0.15 * (self.amount / 100.) * (self.opacity / 100.) * attenuation;
        crate::compute::install(|| {
            image
                .pixels
                .par_chunks_mut(width * 16)
                .enumerate()
                .try_for_each(|(block, pixels)| {
                    ensure!(!cancelled(), "Editing annullato");
                    for (i, p) in pixels.iter_mut().enumerate() {
                        if p[3] <= 0. {
                            continue;
                        }
                        let x = (((origin[0] as usize + i % width) as f32 + 0.5) * sx - 0.5)
                            / self.size;
                        let y = (((origin[1] as usize + block * 16 + i / width) as f32 + 0.5) * sy
                            - 0.5)
                            / self.size;
                        let mono = self.sample(x, y, 0);
                        for c in 0..3 {
                            let noise = if self.monochrome || c == 0 {
                                mono
                            } else {
                                self.sample(x, y, c as u32)
                            };
                            // Add straight linear RGB noise, premultiplied by coverage.
                            // No clipping of shadows, highlights or Gaussian tails.
                            p[c] += noise * amplitude * p[3];
                        }
                    }
                    Ok(())
                })
        })
    }

    fn sample(&self, x: f32, y: f32, channel: u32) -> f32 {
        let ix = x.floor() as u32;
        let iy = y.floor() as u32;
        let smooth = |v: f32| v * v * (3. - 2. * v);
        let tx = smooth(x.fract());
        let ty = smooth(y.fract());
        let weights = [
            (1. - tx) * (1. - ty),
            tx * (1. - ty),
            (1. - tx) * ty,
            tx * ty,
        ];
        let points = [(ix, iy), (ix + 1, iy), (ix, iy + 1), (ix + 1, iy + 1)];
        let mut sum = 0.;
        let mut energy = 0.;
        for ((x, y), weight) in points.into_iter().zip(weights) {
            if weight == 0. {
                continue;
            }
            sum += self.random(x, y, channel) * weight;
            energy += weight * weight;
        }
        sum / energy.sqrt()
    }

    fn random(&self, x: u32, y: u32, channel: u32) -> f32 {
        let counter = (u64::from(y) << 32) | u64::from(x);
        let salt = mix(u64::from(self.seed) | (u64::from(channel) << 32));
        let bits = mix(counter ^ salt);
        let unit = |v: u64| ((v >> 41) as f32 + 0.5) / 8_388_608.;
        let u = unit(bits);
        match self.distribution {
            Distribution::Uniform => (2. * u - 1.) * 3_f32.sqrt(),
            Distribution::Gaussian => {
                let v = unit(mix(bits));
                (-2. * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
            }
        }
    }
}

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests;
