//! Scientific values are not photographic RGB. Preview proxies are explicitly derived.
use crate::{color::LinearImage, resample::Region};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
pub const FILTER: &str = "fits-valid-area-v1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub hdu: u16,
    pub bitpix: i16,
    pub bscale: f64,
    pub bzero: f64,
    pub unit: String,
    pub valid: u64,
    pub invalid: u64,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    /// Full-plane physical-value histogram, not the stretched display histogram.
    pub histogram: Vec<u64>,
}
impl Metadata {
    pub fn validate(&self, pixels: u64) -> Result<()> {
        ensure!(
            self.hdu < 256
                && [8, 16, 32, -32].contains(&self.bitpix)
                && self.bscale.is_finite()
                && self.bzero.is_finite()
                && self.unit.len() <= 68
                && self.valid <= pixels
                && self.invalid == pixels - self.valid
                && self.histogram.len() == 256
                && self
                    .histogram
                    .iter()
                    .try_fold(0u64, |s, v| s.checked_add(*v))
                    == Some(self.valid),
            "Metadati FITS incoerenti"
        );
        ensure!(
            if self.valid == 0 {
                self.minimum.is_none() && self.maximum.is_none()
            } else {
                self.minimum
                    .zip(self.maximum)
                    .is_some_and(|(min, max)| min.is_finite() && max.is_finite() && min <= max)
            },
            "Estremi FITS incoerenti"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub hdu: u16,
    pub x: u32,
    pub y: u32,
    pub stored: Option<f64>,
    pub physical: Option<f64>,
    pub validity: String,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stretch {
    pub black: f32,
    pub white: f32,
    pub asinh: bool,
}
impl Default for Stretch {
    fn default() -> Self {
        Self {
            black: 0.,
            white: 1.,
            asinh: false,
        }
    }
}
impl Stretch {
    pub fn apply(self, image: &mut LinearImage) {
        for p in &mut image.pixels {
            let alpha = p[3];
            let v = if alpha > 0. { p[0] / alpha } else { 0. };
            let mut v = ((v - self.black) / (self.white - self.black).max(1e-6)).clamp(0., 1.);
            if self.asinh {
                v = (10. * v).asinh() / 10f32.asinh();
            }
            *p = crate::color::from_encoded_srgb([v, v, v, alpha]);
        }
    }
}
/// Nonnegative box integration of proxy numerator and valid coverage. No ringing
/// across invalids. Values remain proxies, never a source of scientific measurements.
pub fn area(source: &LinearImage, region: Region) -> Result<LinearImage> {
    let count = region.size[0] as u64 * region.size[1] as u64;
    ensure!(
        count > 0
            && count <= crate::color::MAX_PIXELS as u64
            && region.origin.iter().all(|v| v.is_finite())
            && region
                .step
                .iter()
                .all(|v| v.is_finite() && *v > 0. && *v <= 65536.)
            && region.origin.iter().all(|v| v.abs() <= 131072.),
        "Regione scientifica fuori quota"
    );
    ensure!(
        count as f64 * (region.step[0].ceil() + 1.) * (region.step[1].ceil() + 1.)
            <= crate::color::MAX_PIXELS as f64 * 16.,
        "Lavoro scientifico oltre quota"
    );
    let mut output = Vec::with_capacity(count as usize);
    for y in 0..region.size[1] {
        for x in 0..region.size[0] {
            let left = region.origin[0] + x as f64 * region.step[0];
            let top = region.origin[1] + y as f64 * region.step[1];
            let right = left + region.step[0];
            let bottom = top + region.step[1];
            let mut sum = [0f64; 4];
            for sy in top.floor() as i64..bottom.ceil() as i64 {
                for sx in left.floor() as i64..right.ceil() as i64 {
                    let weight = (((sx + 1) as f64).min(right) - left.max(sx as f64))
                        * (((sy + 1) as f64).min(bottom) - top.max(sy as f64));
                    let p = source.pixels[sy.clamp(0, source.height as i64 - 1) as usize
                        * source.width as usize
                        + sx.clamp(0, source.width as i64 - 1) as usize];
                    for c in 0..4 {
                        sum[c] += p[c] as f64 * weight;
                    }
                }
            }
            let p = sum.map(|v| (v / (region.step[0] * region.step[1])) as f32);
            output.push([p[0], p[1], p[2], p[3].clamp(0., 1.)]);
        }
    }
    LinearImage::new(region.size[0], region.size[1], output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_coverage_does_not_become_a_zero_measurement() {
        let source = LinearImage::new(2, 1, vec![[1., 1., 1., 1.], [0.; 4]]).unwrap();
        let mut reduced = area(&source, Region::fitted([2, 1], [1, 1])).unwrap();
        assert_eq!(reduced.pixels[0], [0.5; 4]);
        Stretch::default().apply(&mut reduced);
        assert!((reduced.pixels[0][0] - 0.5).abs() < 1e-5);
        assert_eq!(source.pixels[1], [0.; 4]);
        let levels = crate::provider::ImageLevels::from_scientific_mip(source, [2, 1], 0).unwrap();
        assert!(levels.scientific());
        assert_eq!(levels.levels()[1].pixels[0], [0.5; 4]);
    }
}
