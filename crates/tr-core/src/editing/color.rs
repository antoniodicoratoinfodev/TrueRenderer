//! Extended RGB hue mixer, process 3. Hue comes from max/min differences;
//! luminance and the signed offset are retained, including negative RGB.
use super::detail::luma;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grade {
    pub hue: f32,
    pub amount: f32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Color {
    pub bands: [Band; 8],
    pub grading: [Grade; 3],
    pub monochrome: bool,
    pub rgb_midtones: [f32; 3],
}
impl Color {
    pub fn validate(&self) -> Result<()> {
        for band in self.bands {
            for v in [band.hue, band.saturation, band.luminance] {
                super::range(v, -100., 100., "Mixer HSL")?;
            }
        }
        for g in self.grading {
            super::range(g.hue, 0., 360., "Tonalità grading")?;
            super::range(g.amount, 0., 100., "Intensità grading")?;
        }
        for v in self.rgb_midtones {
            super::range(v, -100., 100., "Curva RGB")?;
        }
        Ok(())
    }
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if *self == Self::default() {
            return rgb;
        }
        self.apply_active(rgb)
    }
    pub(super) fn apply_active(&self, mut rgb: [f32; 3]) -> [f32; 3] {
        let y = luma(rgb);
        let (h, chroma) = hue_chroma(rgb);
        if chroma > 1e-8 {
            let centers = [0., 30., 60., 120., 180., 240., 270., 300., 360.];
            let index = (0..8).find(|i| h < centers[i + 1]).unwrap_or(7);
            let t = (h - centers[index]) / (centers[index + 1] - centers[index]);
            let a = self.bands[index];
            let b = self.bands[(index + 1) % 8];
            let mix = |a: f32, b: f32| a + (b - a) * t;
            let h = h + mix(a.hue, b.hue) * 0.3;
            let c = chroma * (1. + mix(a.saturation, b.saturation) / 100.);
            let unit = hue_rgb(h);
            let uy = luma(unit);
            let gain = (mix(a.luminance, b.luminance) / 100.).exp2();
            rgb = std::array::from_fn(|i| (y + (unit[i] - uy) * c) * gain);
        }
        if self.monochrome {
            rgb = [luma(rgb); 3];
        }
        let y = luma(rgb);
        let t = y.clamp(0., 1.);
        let weights = [(1. - t).powi(2), 2. * t * (1. - t), t * t];
        for (grade, weight) in self.grading.iter().zip(weights) {
            if grade.amount == 0. {
                continue;
            }
            let unit = hue_rgb(grade.hue);
            let uy = luma(unit);
            for c in 0..3 {
                rgb[c] += (unit[c] - uy) * grade.amount / 100. * weight * y.abs();
            }
        }
        for (v, mid) in rgb.iter_mut().zip(self.rgb_midtones) {
            // Endpoints fixed, sign/extended values preserved outside [0,1].
            if (0. ..=1.).contains(v) {
                *v += mid / 400. * 4. * *v * (1. - *v);
            }
        }
        rgb
    }
}
pub(super) fn hue_chroma(rgb: [f32; 3]) -> (f32, f32) {
    let max = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let min = rgb.into_iter().fold(f32::INFINITY, f32::min);
    let c = max - min;
    if c <= 1e-8 {
        return (0., 0.);
    }
    let h = if max == rgb[0] {
        ((rgb[1] - rgb[2]) / c).rem_euclid(6.)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / c + 2.
    } else {
        (rgb[0] - rgb[1]) / c + 4.
    };
    (h * 60., c)
}
fn hue_rgb(h: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.) / 60.;
    let x = 1. - (h.rem_euclid(2.) - 1.).abs();
    match h as u32 {
        0 => [1., x, 0.],
        1 => [x, 1., 0.],
        2 => [0., 1., x],
        3 => [0., x, 1.],
        4 => [x, 0., 1.],
        _ => [1., 0., x],
    }
}
