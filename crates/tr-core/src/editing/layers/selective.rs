//! Version-1 creative RGB and tonal operators. Straight, extended linear Rec.2020.
//! Selection weights use the fixed layer input; only weights, never RGB, clamp.
use super::{hue_chroma, hue_rgb, luma, range, smooth};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

fn version(v: u32) -> Result<()> {
    ensure!(v == 1, "Versione operatore colore/tono non supportata");
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum SelectiveMethod {
    #[default]
    Relative,
    Absolute,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectiveColor {
    pub version: u32,
    pub method: SelectiveMethod,
    /// R, Y, G, C, B, M, whites, neutrals, blacks; each contains C/M/Y/K percent.
    pub adjustments: [[f32; 4]; 9],
}
impl Default for SelectiveColor {
    fn default() -> Self {
        Self {
            version: 1,
            method: SelectiveMethod::Relative,
            adjustments: [[0.; 4]; 9],
        }
    }
}
impl SelectiveColor {
    pub fn is_neutral(&self) -> bool {
        self.adjustments.iter().flatten().all(|v| *v == 0.)
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        for v in self.adjustments.iter().flatten() {
            range(*v, -100., 100., "Componente colore selettivo")?;
        }
        Ok(())
    }
    pub fn weights(&self, guide: [f32; 3]) -> [f32; 9] {
        // Normalize before hue extraction, then fade chromatic selection near
        // black. This avoids an arbitrary hue discontinuity at tiny RGB values.
        let scale = guide.into_iter().map(f32::abs).fold(0., f32::max);
        let (hue, chroma) = if scale > 0. {
            hue_chroma(guide.map(|v| v / scale))
        } else {
            (0., 0.)
        };
        let strength = chroma.min(1.) * smooth(scale / 1e-4);
        let t = luma(guide).clamp(0., 1.).sqrt();
        let white = t.powi(4);
        let black = (1. - t).powi(4);
        std::array::from_fn(|i| match i {
            0..=5 => {
                let delta = (hue - i as f32 * 60. + 180.).rem_euclid(360.) - 180.;
                strength * smooth(1. - delta.abs() / 60.)
            }
            6 => (1. - strength) * white,
            7 => (1. - strength) * (1. - white - black),
            _ => (1. - strength) * black,
        })
    }
    pub(super) fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        let weights = self.weights(guide);
        let mut components = [0.; 4];
        for (family, weight) in self.adjustments.iter().zip(weights) {
            for c in 0..4 {
                components[c] += family[c] * (weight / 100.);
            }
        }
        // Creative RGB complements, not a characterised CMYK separation.
        std::array::from_fn(|c| {
            let adjustment = components[c] + components[3];
            match self.method {
                SelectiveMethod::Relative => rgb[c] * (1. - adjustment),
                SelectiveMethod::Absolute => rgb[c] - adjustment,
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colorize {
    pub version: u32,
    pub hue: f32,
    pub chroma: f32,
    pub amount: f32,
    pub exposure: f32,
}
impl Default for Colorize {
    fn default() -> Self {
        Self {
            version: 1,
            hue: 30.,
            chroma: 50.,
            amount: 0.,
            exposure: 0.,
        }
    }
}
impl Colorize {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.hue, 0., 360., "Tonalità Colorizza")?;
        range(self.chroma, 0., 100., "Cromia Colorizza")?;
        range(self.amount, 0., 100., "Quantità Colorizza")?;
        range(self.exposure, -2., 2., "Luminosità Colorizza")
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let y = luma(rgb);
        let unit = hue_rgb(self.hue);
        let uy = luma(unit);
        let target =
            unit.map(|v| (y + (v - uy) * y.abs() * self.chroma / 100.) * self.exposure.exp2());
        if self.amount == 100. {
            target
        } else {
            std::array::from_fn(|c| rgb[c] + (target[c] - rgb[c]) * self.amount / 100.)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TonalAdjustments {
    pub version: u32,
    pub exposure: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub shadows: f32,
    pub highlights: f32,
    pub blacks: f32,
    pub whites: f32,
    pub pivot: f32,
}
impl Default for TonalAdjustments {
    fn default() -> Self {
        Self {
            version: 1,
            exposure: 0.,
            brightness: 0.,
            contrast: 0.,
            shadows: 0.,
            highlights: 0.,
            blacks: 0.,
            whites: 0.,
            pivot: 0.18,
        }
    }
}
impl TonalAdjustments {
    fn neutral_tones(&self) -> bool {
        [
            self.brightness,
            self.contrast,
            self.shadows,
            self.highlights,
            self.blacks,
            self.whites,
        ]
        .into_iter()
        .all(|v| v == 0.)
    }
    pub fn is_neutral(&self) -> bool {
        self.exposure == 0. && self.neutral_tones()
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.exposure, -10., 10., "Esposizione tonale")?;
        range(self.pivot, 0.01, 4., "Pivot contrasto")?;
        for v in [
            self.brightness,
            self.contrast,
            self.shadows,
            self.highlights,
            self.blacks,
            self.whites,
        ] {
            range(v, -100., 100., "Regolazione tonale")?;
        }
        Ok(())
    }
    pub(super) fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        let exposed = rgb.map(|v| v * self.exposure.exp2());
        if self.neutral_tones() {
            return exposed;
        }
        let y = luma(exposed);
        let contrast_y = if self.contrast == 0. {
            y
        } else {
            y.signum() * self.pivot * (y.abs() / self.pivot).powf((self.contrast / 100.).exp())
        };
        let guide_y = luma(guide).abs();
        let t = guide_y / (guide_y + self.pivot);
        let dark = 1. - t;
        let ev = (self.brightness * 4. * t * dark
            + self.shadows * dark.powi(2)
            + self.highlights * t * t
            + self.blacks * dark.powi(4)
            + self.whites * t.powi(4))
            / 100.;
        let offset = contrast_y * ev.exp2() - y;
        exposed.map(|v| v + offset)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureGamma {
    pub version: u32,
    pub exposure: f32,
    pub offset: f32,
    pub gamma: f32,
}
impl Default for ExposureGamma {
    fn default() -> Self {
        Self {
            version: 1,
            exposure: 0.,
            offset: 0.,
            gamma: 1.,
        }
    }
}
impl ExposureGamma {
    pub fn is_neutral(&self) -> bool {
        self.exposure == 0. && self.offset == 0. && self.gamma == 1.
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.exposure, -10., 10., "Esposizione tecnica")?;
        range(self.offset, -4., 4., "Offset tecnico")?;
        range(self.gamma, 0.25, 4., "Gamma tecnica")
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        rgb.map(|v| {
            let x = v * self.exposure.exp2() + self.offset;
            if self.gamma == 1. {
                x
            } else {
                x.signum() * x.abs().powf(1. / self.gamma)
            }
        })
    }
}
