//! First photographic process: deterministic CPU operations on straight RGB in
//! the extended linear Rec.2020 working space. Alpha remains coverage.
use crate::{
    color::{LinearImage, Pixel},
    decoder::RawEngine,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const PROCESS_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRecipe {
    pub schema_version: u32,
    pub process_version: u32,
    pub raw_engine: RawEngine,
    pub exposure_ev: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    /// Relative correction on developed RGB; these values are not kelvin.
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub curve: Vec<CurvePoint>,
}

impl EditRecipe {
    pub fn neutral(raw_engine: RawEngine) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            process_version: PROCESS_VERSION,
            raw_engine,
            exposure_ev: 0.,
            brightness: 0.,
            contrast: 0.,
            highlights: 0.,
            shadows: 0.,
            whites: 0.,
            blacks: 0.,
            temperature: 0.,
            tint: 0.,
            saturation: 0.,
            curve: vec![],
        }
    }

    pub fn is_neutral(&self) -> bool {
        let mut neutral = Self::neutral(self.raw_engine);
        neutral.curve = self.curve.clone();
        self == &neutral && self.curve.iter().all(|p| p.x == p.y)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == SCHEMA_VERSION && self.process_version == PROCESS_VERSION,
            "Versione della ricetta fotografica non eseguibile"
        );
        ensure!(
            self.exposure_ev.is_finite() && (-10. ..=10.).contains(&self.exposure_ev),
            "Esposizione fuori scala"
        );
        for (name, value) in [
            ("Luminosità", self.brightness),
            ("Contrasto", self.contrast),
            ("Alte luci", self.highlights),
            ("Ombre", self.shadows),
            ("Bianchi", self.whites),
            ("Neri", self.blacks),
            ("Temperatura RGB", self.temperature),
            ("Tinta RGB", self.tint),
            ("Saturazione", self.saturation),
        ] {
            ensure!(
                value.is_finite() && (-100. ..=100.).contains(&value),
                "{name} fuori scala"
            );
        }
        ensure!(self.curve.len() <= 32, "Curva: massimo 32 punti");
        if !self.curve.is_empty() {
            ensure!(
                self.curve.len() >= 2
                    && self.curve[0] == CurvePoint { x: 0., y: 0. }
                    && self.curve[self.curve.len() - 1] == CurvePoint { x: 1., y: 1. },
                "Curva: ancoraggi 0 e 1 richiesti"
            );
            for pair in self.curve.windows(2) {
                ensure!(
                    pair[0].x.is_finite()
                        && pair[0].y.is_finite()
                        && pair[1].x.is_finite()
                        && pair[1].y.is_finite()
                        && pair[0].x < pair[1].x
                        && pair[0].y <= pair[1].y
                        && (0. ..=1.).contains(&pair[0].y)
                        && (0. ..=1.).contains(&pair[1].y),
                    "Curva tonale non monotona o fuori dominio"
                );
            }
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 1024 * 1024,
            "Ricetta oltre quota"
        );
        Ok(())
    }

    /// Apply in place. The caller owns a private raster and must validate the
    /// recipe before doing expensive work. Identity never rounds existing bits.
    pub fn apply(&self, image: &mut LinearImage) -> Result<()> {
        self.validate()?;
        if self.is_neutral() {
            return Ok(());
        }
        for pixel in &mut image.pixels {
            *pixel = self.transform_pixel(*pixel);
        }
        ensure!(
            image.pixels.iter().all(|p| p.iter().all(|v| v.is_finite())),
            "Regolazione fotografica: risultato non finito"
        );
        Ok(())
    }

    pub fn apply_pixel(&self, pixel: Pixel) -> Pixel {
        if self.is_neutral() {
            return pixel;
        }
        self.transform_pixel(pixel)
    }

    /// Choose relative RGB gains from a rendered, premultiplied pixel. This is
    /// not camera white balance: it cannot recover clipped source channels.
    pub fn neutralize_render_sample(&mut self, pixel: Pixel) -> Result<()> {
        self.validate()?;
        ensure!(
            self.saturation == 0.,
            "Azzerare la saturazione prima del contagocce RGB"
        );
        let alpha = pixel[3];
        ensure!(
            alpha.is_finite() && (0.95..=1.).contains(&alpha),
            "Campione RGB non opaco"
        );
        let [r, g, b] = [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha];
        ensure!(
            [r, g, b]
                .into_iter()
                .all(|v| v.is_finite() && (0.02..0.95).contains(&v)),
            "Campione RGB troppo scuro, saturo o fuori dominio"
        );
        let temperature = self.temperature - 100. * (r / b).ln() / 0.36;
        let tint = self.tint + 100. * (g / (r * b).sqrt()).ln() / 0.21;
        ensure!(
            (-100. ..=100.).contains(&temperature) && (-100. ..=100.).contains(&tint),
            "Correzione RGB richiesta oltre scala"
        );
        self.temperature = temperature;
        self.tint = tint;
        self.validate()
    }

    fn transform_pixel(&self, pixel: Pixel) -> Pixel {
        let alpha = pixel[3];
        if alpha == 0. {
            return pixel;
        }
        if self.temperature == 0.
            && self.tint == 0.
            && self.brightness == 0.
            && self.contrast == 0.
            && self.highlights == 0.
            && self.shadows == 0.
            && self.whites == 0.
            && self.blacks == 0.
            && self.saturation == 0.
            && self.curve.is_empty()
        {
            let factor = self.exposure_ev.exp2();
            return [
                pixel[0] * factor,
                pixel[1] * factor,
                pixel[2] * factor,
                alpha,
            ];
        }
        let mut rgb = [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha];
        let ev = self.exposure_ev.exp2();
        let t = self.temperature / 100.;
        let tint = self.tint / 100.;
        let gains = [
            (0.18 * t + 0.07 * tint).exp(),
            (-0.14 * tint).exp(),
            (-0.18 * t + 0.07 * tint).exp(),
        ];
        for c in 0..3 {
            rgb[c] *= ev * gains[c];
        }
        let luminance = |rgb: [f32; 3]| rgb[0] * 0.2627 + rgb[1] * 0.6780 + rgb[2] * 0.0593;
        let old_y = luminance(rgb);
        if old_y > 1e-8
            && (self.brightness != 0.
                || self.contrast != 0.
                || self.highlights != 0.
                || self.shadows != 0.
                || self.whites != 0.
                || self.blacks != 0.
                || !self.curve.is_empty())
        {
            let mut y = old_y;
            if y <= 1. {
                // Each bounded smoothstep has fixed endpoints and no viewport dependence.
                let tone = |y: f32, amount: f32, weight: f32| y + amount * y * (1. - y) * weight;
                y = tone(y, self.brightness / 200., 1.);
                y = tone(y, self.shadows / 200., (1. - y).powi(2));
                y = tone(y, self.highlights / 200., y.powi(2));
                y = tone(y, self.blacks / 200., (1. - y).powi(4));
                y = tone(y, self.whites / 200., y.powi(4));
            }
            let gamma = (self.contrast / 100.).exp();
            y = 0.18 * (y / 0.18).powf(gamma);
            if !self.curve.is_empty() {
                y = curve(y, &self.curve);
            }
            let ratio = y / old_y;
            for value in &mut rgb {
                *value *= ratio;
            }
        }
        let y = luminance(rgb);
        if self.saturation != 0. {
            let saturation = 1. + self.saturation / 100.;
            for value in &mut rgb {
                *value = y + (*value - y) * saturation;
            }
        }
        [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
    }
}

fn curve(x: f32, points: &[CurvePoint]) -> f32 {
    if x <= 0. {
        return x;
    }
    if x >= 1. {
        return x;
    }
    let right = points
        .partition_point(|p| p.x < x)
        .min(points.len() - 1)
        .max(1);
    let (a, b) = (points[right - 1], points[right]);
    a.y + (x - a.x) * (b.y - a.y) / (b.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_exposure_alpha_and_signed_values() {
        let mut image =
            LinearImage::new(2, 1, vec![[0.2, -0.3, 2., 0.5], [0., 0., 0., 0.]]).unwrap();
        EditRecipe::neutral(RawEngine::default())
            .apply(&mut image)
            .unwrap();
        assert_eq!(image.pixels[0], [0.2, -0.3, 2., 0.5]);
        let mut recipe = EditRecipe::neutral(RawEngine::default());
        recipe.exposure_ev = 1.;
        recipe.apply(&mut image).unwrap();
        assert_eq!(image.pixels[0], [0.4, -0.6, 4., 0.5]);
        assert_eq!(image.pixels[1], [0.; 4]);
    }
    #[test]
    fn curve_and_invalid_parameters() {
        let mut recipe = EditRecipe::neutral(RawEngine::default());
        recipe.curve = vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 0.5, y: 0.3 },
            CurvePoint { x: 1., y: 1. },
        ];
        recipe.validate().unwrap();
        assert!((curve(0.5, &recipe.curve) - 0.3).abs() < 1e-6);
        recipe.curve[1].y = 2.;
        assert!(recipe.validate().is_err());
        recipe.curve.clear();
        recipe.exposure_ev = f32::NAN;
        assert!(recipe.validate().is_err());
    }
    #[test]
    fn tone_ramp_is_monotone_in_single_controls() {
        for amount in [-100., -50., 50., 100.] {
            for set in [0, 1, 2, 3, 4, 5] {
                let mut recipe = EditRecipe::neutral(RawEngine::default());
                match set {
                    0 => recipe.brightness = amount,
                    1 => recipe.contrast = amount,
                    2 => recipe.shadows = amount,
                    3 => recipe.highlights = amount,
                    4 => recipe.blacks = amount,
                    _ => recipe.whites = amount,
                }
                let mut previous = -1.;
                for i in 0..=1000 {
                    let v = i as f32 / 1000.;
                    let out = recipe.apply_pixel([v, v, v, 1.])[0];
                    assert!(
                        out >= previous - 1e-6,
                        "{set} {amount} {i}: {out} < {previous}"
                    );
                    previous = out;
                }
            }
        }
    }
    #[test]
    fn rendered_rgb_neutral_picker_is_reversible_and_rejects_bad_samples() {
        let mut recipe = EditRecipe::neutral(RawEngine::default());
        recipe.temperature = 24.;
        recipe.tint = -15.;
        let source = [0.4, 0.4, 0.4, 1.];
        let rendered = recipe.apply_pixel(source);
        recipe.neutralize_render_sample(rendered).unwrap();
        assert!(recipe.temperature.abs() < 1e-4);
        assert!(recipe.tint.abs() < 1e-4);
        let neutral = recipe.apply_pixel(source);
        assert!((neutral[0] - neutral[1]).abs() < 1e-6);
        assert!((neutral[1] - neutral[2]).abs() < 1e-6);
        let before = recipe.clone();
        for bad in [[0.; 4], [0.4, 0.4, 0.4, 0.5], [1., 1., 1., 1.]] {
            assert!(recipe.neutralize_render_sample(bad).is_err());
            assert_eq!(recipe, before);
        }
        recipe.saturation = 20.;
        assert!(recipe.neutralize_render_sample(source).is_err());
    }
}
