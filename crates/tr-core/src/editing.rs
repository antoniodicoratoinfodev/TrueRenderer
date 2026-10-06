//! First photographic process: deterministic CPU operations on straight RGB in
//! the extended linear Rec.2020 working space. Alpha remains coverage.
use crate::{
    color::{LinearImage, Pixel},
    decoder::RawEngine,
};
use anyhow::{Result, ensure};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
pub mod color;
pub mod detail;
pub mod geometry;
pub mod masks;
#[cfg(test)]
mod process3_tests;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advanced {
    pub geometry: geometry::Geometry,
    pub detail: detail::Detail,
    pub color: color::Color,
    pub masks: Vec<masks::Mask>,
}
impl Advanced {
    pub fn is_neutral(&self) -> bool {
        self.geometry == geometry::Geometry::default()
            && !self.detail.active()
            && self.color == color::Color::default()
            && !self.masks.iter().any(masks::Mask::active)
    }
    pub fn validate(&self) -> Result<()> {
        self.geometry.validate()?;
        self.detail.validate()?;
        self.color.validate()?;
        ensure!(self.masks.len() <= 16, "Massimo 16 maschere per ricetta");
        ensure!(
            self.masks.iter().map(|m| m.points.len()).sum::<usize>() <= 512,
            "Massimo 512 punti di pennello per ricetta"
        );
        for mask in &self.masks {
            mask.validate()?;
        }
        Ok(())
    }
}

fn range(v: f32, min: f32, max: f32, name: &str) -> Result<()> {
    ensure!(
        v.is_finite() && (min..=max).contains(&v),
        "{name}: valore fuori scala"
    );
    Ok(())
}

pub const SCHEMA_VERSION: u32 = 1;
pub const PROCESS_VERSION: u32 = 1;

// All pixel-independent stages share the application's bounded CPU pool.
// Each pixel keeps the scalar arithmetic order (including premultiplication).
pub(super) fn pixels_mut(pixels: &mut [Pixel], apply: impl Fn(usize, &mut Pixel) + Sync + Send) {
    if pixels.len() >= 32768 {
        crate::compute::install(|| {
            pixels
                .par_iter_mut()
                .enumerate()
                .for_each(|(i, p)| apply(i, p));
        });
    } else {
        for (i, p) in pixels.iter_mut().enumerate() {
            apply(i, p);
        }
    }
}

struct Parameters {
    exposure: f32,
    gains: [f32; 3],
    gamma: f32,
    exposure_only: bool,
}
impl Parameters {
    fn new(recipe: &EditRecipe) -> Self {
        let t = recipe.temperature / 100.;
        let tint = recipe.tint / 100.;
        Self {
            exposure: recipe.exposure_ev.exp2(),
            gains: [
                (0.18 * t + 0.07 * tint).exp(),
                (-0.14 * tint).exp(),
                (-0.18 * t + 0.07 * tint).exp(),
            ],
            gamma: (recipe.contrast / 100.).exp(),
            exposure_only: recipe.temperature == 0.
                && recipe.tint == 0.
                && recipe.brightness == 0.
                && recipe.contrast == 0.
                && recipe.highlights == 0.
                && recipe.shadows == 0.
                && recipe.whites == 0.
                && recipe.blacks == 0.
                && recipe.saturation == 0.
                && recipe.vibrance == 0.
                && recipe.curve.is_empty(),
        }
    }
}

/// Bounded picker estimate, not a camera-WB or statistical confidence estimate.
#[derive(Clone, Debug)]
pub struct RgbAreaSample {
    pub working: Pixel,
    pub valid: usize,
    pub total: usize,
    pub chroma_spread: f32,
}

/// Estimate chromaticity using 20%-trimmed means of log(R/G), log(B/G).
/// Brightness variation does not weight the estimate. Reject incomplete areas,
/// too many unusable pixels and chromatically heterogeneous regions.
pub fn sample_rgb_area(image: &LinearImage, x: u32, y: u32, side: u32) -> Result<RgbAreaSample> {
    ensure!(matches!(side, 5 | 11), "Area RGB non supportata");
    let radius = side / 2;
    ensure!(
        x >= radius
            && y >= radius
            && x.checked_add(radius).is_some_and(|v| v < image.width)
            && y.checked_add(radius).is_some_and(|v| v < image.height),
        "Area RGB incompleta al bordo: campionare più all'interno"
    );
    let total = (side * side) as usize;
    let mut ratios = [Vec::with_capacity(total), Vec::with_capacity(total)];
    for row in y - radius..=y + radius {
        for col in x - radius..=x + radius {
            let pixel = image.pixels[row as usize * image.width as usize + col as usize];
            if let Ok([r, g, b]) = picker_rgb(pixel) {
                ratios[0].push((r / g).ln());
                ratios[1].push((b / g).ln());
            }
        }
    }
    let valid = ratios[0].len();
    ensure!(
        valid * 5 >= total * 4,
        "Area RGB: meno dell'80% di pixel validi"
    );
    let mut means = [0.; 2];
    let mut chroma_spread = 0_f32;
    for (values, mean) in ratios.iter_mut().zip(&mut means) {
        values.sort_by(f32::total_cmp);
        let tail = valid / 10;
        chroma_spread = chroma_spread.max(values[valid - 1 - tail] - values[tail]);
        let trim = valid / 5;
        *mean = values[trim..valid - trim].iter().sum::<f32>() / (valid - 2 * trim) as f32;
    }
    ensure!(
        chroma_spread <= 0.08,
        "Area RGB cromaticamente disomogenea: scegliere una zona uniforme"
    );
    // An equivalent opaque pixel carries only the estimated chromaticity into
    // the existing gain solver; it is not reported as a measured linear mean.
    let rgb = [means[0].exp(), 1., means[1].exp()];
    let scale = 0.5 / rgb.into_iter().fold(1_f32, f32::max);
    Ok(RgbAreaSample {
        working: [rgb[0] * scale, scale, rgb[2] * scale, 1.],
        valid,
        total,
        chroma_spread,
    })
}

fn picker_rgb(pixel: Pixel) -> Result<[f32; 3]> {
    let alpha = pixel[3];
    ensure!(
        alpha.is_finite() && (0.95..=1.).contains(&alpha),
        "Campione RGB non opaco"
    );
    let rgb = [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha];
    ensure!(
        rgb.into_iter()
            .all(|v| v.is_finite() && (0.02..0.95).contains(&v)),
        "Campione RGB troppo scuro, saturo o fuori dominio"
    );
    Ok(rgb)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRecipe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced: Option<Box<Advanced>>,
    #[serde(
        default,
        skip_serializing_if = "crate::decoder::RawWhiteBalance::is_as_shot"
    )]
    pub raw_wb: crate::decoder::RawWhiteBalance,
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
    #[serde(default, skip_serializing_if = "is_zero")]
    pub vibrance: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub protect_warm: bool,
    pub curve: Vec<CurvePoint>,
}

impl EditRecipe {
    pub fn neutral(raw_engine: RawEngine) -> Self {
        Self {
            advanced: None,
            raw_wb: Default::default(),
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
            vibrance: 0.,
            protect_warm: false,
            curve: vec![],
        }
    }

    pub fn is_neutral(&self) -> bool {
        let mut neutral = Self::neutral(self.raw_engine);
        neutral.curve = self.curve.clone();
        neutral.process_version = self.process_version;
        neutral.protect_warm = self.protect_warm;
        neutral.advanced = self.advanced.clone();
        self == &neutral
            && self.curve.iter().all(|p| p.x == p.y)
            && self.advanced.as_ref().is_none_or(|a| a.is_neutral())
    }

    pub fn validate(&self) -> Result<()> {
        self.raw_wb.validate_for(self.raw_engine)?;
        ensure!(
            self.schema_version == SCHEMA_VERSION
                && matches!(self.process_version, 1..=3)
                && (self.process_version >= 2 || (self.vibrance == 0. && !self.protect_warm))
                && (self.advanced.is_none() || self.process_version == 3),
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
            ("Vividezza", self.vibrance),
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
        if let Some(advanced) = &self.advanced {
            advanced.validate()?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 48 * 1024,
            "Ricetta oltre quota"
        );
        Ok(())
    }

    /// Apply in place. The caller owns a private raster and must validate the
    /// recipe before doing expensive work. Identity never rounds existing bits.
    pub fn apply(&self, image: &mut LinearImage) -> Result<()> {
        self.apply_preview(image, [image.width, image.height], 0)
            .map(|_| ())
    }

    /// Full-frame process or explicitly provisional mip, with canonical output
    /// geometry. The result reports the edited native dimensions, not the input.
    pub fn apply_preview(
        &self,
        image: &mut LinearImage,
        native: [u32; 2],
        base: u32,
    ) -> Result<[u32; 2]> {
        self.apply_preview_cancellable(image, native, base, &|| false)
    }

    /// Cancellation is for superseded background refinements. The caller must
    /// discard the private raster on error; no partially edited image is valid.
    pub fn apply_preview_cancellable(
        &self,
        image: &mut LinearImage,
        native: [u32; 2],
        base: u32,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<[u32; 2]> {
        ensure!(!cancelled(), "Editing annullato");
        self.validate()?;
        ensure!(
            native[0] > 0 && native[1] > 0 && base < 32,
            "Dimensioni editing non valide"
        );
        let divisor = 1_u32 << base;
        ensure!(
            [image.width, image.height] == native.map(|v| v.div_ceil(divisor)),
            "Scala editing incoerente"
        );
        if self.is_neutral() {
            return Ok(native);
        }
        let (width, height) = (image.width, image.height);
        let parameters = Parameters::new(self);
        pixels_mut(&mut image.pixels, |i, pixel| {
            if let Some(a) = &self.advanced
                && a.geometry.vignette != 0.
            {
                a.geometry.optical_color(
                    pixel,
                    ((i % width as usize) as f32 + 0.5) / width as f32,
                    ((i / width as usize) as f32 + 0.5) / height as f32,
                );
            }
            *pixel = self.transform_prepared(*pixel, &parameters);
        });
        let mut output = native;
        ensure!(!cancelled(), "Editing annullato");
        if let Some(a) = &self.advanced {
            a.detail.apply_cancellable(image, native, cancelled)?;
            detail::defringe(image, a.geometry.defringe);
            ensure!(!cancelled(), "Editing annullato");
            let masks: Vec<_> = a
                .masks
                .iter()
                .filter(|m| m.active())
                .map(|m| masks::Prepared::new(m, native[1] as f32 / native[0] as f32))
                .collect();
            let (w, h) = (image.width, image.height);
            let color_active = a.color != color::Color::default();
            pixels_mut(&mut image.pixels, |i, p| {
                if p[3] <= 0. {
                    return;
                }
                if !color_active && masks.is_empty() && p[3] == 1. {
                    return;
                }
                let xy = [
                    ((i % w as usize) as f32 + 0.5) / w as f32,
                    ((i / w as usize) as f32 + 0.5) / h as f32,
                ];
                let guide = detail::straight(*p);
                let mut rgb = if color_active {
                    a.color.apply_active(guide)
                } else {
                    guide
                };
                for mask in &masks {
                    mask.apply(&mut rgb, guide, xy);
                }
                for c in 0..3 {
                    p[c] = rgb[c] * p[3];
                }
            });
            ensure!(!cancelled(), "Editing annullato");
            output = a.geometry.apply(image, native, base)?;
        }
        ensure!(!cancelled(), "Editing annullato");
        ensure!(
            image.pixels.iter().all(|p| p.iter().all(|v| v.is_finite())),
            "Regolazione fotografica: risultato non finito"
        );
        Ok(output)
    }

    pub fn scratch_bytes(&self, width: u32, height: u32) -> u64 {
        self.advanced.as_ref().map_or(0, |a| {
            let frames = if a.detail.active() || a.geometry.defringe != 0. {
                2
            } else {
                u64::from(a.geometry.has_mapping())
            };
            u64::from(width) * u64::from(height) * 16 * frames + 1024 * 1024
        })
    }

    pub fn apply_pixel(&self, pixel: Pixel) -> Pixel {
        if self.is_neutral() {
            return pixel;
        }
        self.transform_pixel(pixel)
    }

    /// Fixed native-frame grid, at most 4096 samples, independent of viewport.
    /// Call only with the source developed under this recipe's RAW WB.
    fn analysis_samples(&self, image: &LinearImage) -> Result<Vec<Pixel>> {
        self.validate()?;
        ensure!(
            self.advanced.as_ref().is_none_or(|a| a.is_neutral()),
            "Auto: ripristinare prima geometria, dettaglio, colore avanzato e maschere"
        );
        let mut samples = Vec::with_capacity(4096);
        let nx = image.width.min(64);
        let ny = image.height.min(64);
        for j in 0..ny {
            for i in 0..nx {
                let x = ((2 * i as u64 + 1) * image.width as u64 / (2 * nx as u64)) as usize;
                let y = ((2 * j as u64 + 1) * image.height as u64 / (2 * ny as u64)) as usize;
                samples.push(self.apply_pixel(image.pixels[y * image.width as usize + x]));
            }
        }
        Ok(samples)
    }

    /// Conservative exposure suggestion: median -> 0.18, capped by p99 -> 0.9.
    /// Freeze only the resolved EV; preserve every other control.
    pub fn auto_exposure(&mut self, image: &LinearImage) -> Result<()> {
        let samples = self.analysis_samples(image)?;
        let mut luminances: Vec<f32> = samples
            .iter()
            .filter_map(|p| {
                if !(0.95..=1.).contains(&p[3]) {
                    return None;
                }
                let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
                if !rgb.iter().all(|v| v.is_finite() && *v >= 0.) {
                    return None;
                }
                let y = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
                (y > 1e-6).then_some(y)
            })
            .collect();
        ensure!(
            luminances.len() >= 16 && luminances.len() * 4 >= samples.len(),
            "Auto: campioni validi insufficienti"
        );
        luminances.sort_by(f32::total_cmp);
        let median = luminances[luminances.len() / 2];
        let high = luminances[(luminances.len() - 1) * 99 / 100];
        let delta = (0.18 / median)
            .log2()
            .min((0.9 / high).log2())
            .clamp(-3., 3.);
        self.exposure_ev = (self.exposure_ev + delta).clamp(-10., 10.);
        Ok(())
    }

    /// Grey-world RGB estimate, not a camera/illuminant recognition algorithm.
    /// Freeze the existing relative RGB controls; do not request decode-time Auto.
    pub fn auto_rgb(&mut self, image: &LinearImage) -> Result<()> {
        ensure!(
            self.saturation == 0. && self.vibrance == 0.,
            "Azzerare saturazione e vividezza prima del contagocce RGB"
        );
        let samples = self.analysis_samples(image)?;
        let mut ratios = [Vec::new(), Vec::new()];
        for p in &samples {
            if let Ok([r, g, b]) = picker_rgb(*p) {
                ratios[0].push((r / g).ln());
                ratios[1].push((b / g).ln());
            }
        }
        let n = ratios[0].len();
        ensure!(
            n >= 16 && n * 4 >= samples.len(),
            "Auto: campioni validi insufficienti"
        );
        let mut mean = [0.; 2];
        for (values, m) in ratios.iter_mut().zip(&mut mean) {
            values.sort_by(f32::total_cmp);
            let trim = n / 5;
            *m = values[trim..n - trim].iter().sum::<f32>() / (n - 2 * trim) as f32;
        }
        let rgb = [mean[0].exp(), 1., mean[1].exp()];
        let scale = 0.5 / rgb.into_iter().fold(1_f32, f32::max);
        self.neutralize_render_sample([rgb[0] * scale, scale, rgb[2] * scale, 1.])
    }

    /// Choose relative RGB gains from a rendered, premultiplied pixel. This is
    /// not camera white balance: it cannot recover clipped source channels.
    pub fn neutralize_render_sample(&mut self, pixel: Pixel) -> Result<()> {
        self.validate()?;
        ensure!(
            self.advanced.as_ref().is_none_or(|a| a.is_neutral()),
            "Contagocce RGB: ripristinare prima gli strumenti avanzati"
        );
        ensure!(
            self.saturation == 0. && self.vibrance == 0.,
            "Azzerare saturazione e vividezza prima del contagocce RGB"
        );
        let [r, g, b] = picker_rgb(pixel)?;
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
        self.transform_prepared(pixel, &Parameters::new(self))
    }
    fn transform_prepared(&self, pixel: Pixel, parameters: &Parameters) -> Pixel {
        let alpha = pixel[3];
        if alpha == 0. {
            return pixel;
        }
        if parameters.exposure_only {
            let factor = parameters.exposure;
            return [
                pixel[0] * factor,
                pixel[1] * factor,
                pixel[2] * factor,
                alpha,
            ];
        }
        let mut rgb = [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha];
        let ev = parameters.exposure;
        let gains = parameters.gains;
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
            let gamma = parameters.gamma;
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
        if self.vibrance != 0. && rgb.iter().all(|v| *v >= 0. && v.is_finite()) {
            let max = rgb.into_iter().fold(0_f32, f32::max);
            let min = rgb.into_iter().fold(f32::INFINITY, f32::min);
            let chroma = max - min;
            if max > 1e-8 && chroma > 0. {
                let saturation = chroma / max;
                let mut weight = (1. - saturation).powi(2);
                if self.protect_warm {
                    // Triangular hue mask 0..70 degrees, peak at 35. This is
                    // a warm-colour heuristic, never semantic skin detection.
                    let hue = if max == rgb[0] {
                        ((rgb[1] - rgb[2]) / chroma).rem_euclid(6.)
                    } else if max == rgb[1] {
                        (rgb[2] - rgb[0]) / chroma + 2.
                    } else {
                        (rgb[0] - rgb[1]) / chroma + 4.
                    } * 60.;
                    weight *= 1. - 0.75 * (1. - (hue - 35.).abs() / 35.).max(0.);
                }
                let factor = 1. + self.vibrance / 100. * weight;
                let y = luminance(rgb);
                for value in &mut rgb {
                    *value = y + (*value - y) * factor;
                }
            }
        }
        [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
    }
}

fn is_zero(value: &f32) -> bool {
    *value == 0.
}
fn is_false(value: &bool) -> bool {
    !*value
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
    fn vibrance_versioning_preserves_old_pixels_and_rejects_implicit_upgrade() {
        let mut old = EditRecipe::neutral(RawEngine::default());
        old.exposure_ev = 0.7;
        old.saturation = 12.;
        let json = serde_json::to_string(&old).unwrap();
        assert!(!json.contains("vibrance") && !json.contains("protect_warm"));
        let mut upgraded: EditRecipe = serde_json::from_str(&json).unwrap();
        upgraded.process_version = 2;
        for pixel in [[-0.2, 0.3, 1.8, 1.], [0.1, 0.2, 0.3, 0.5], [0.; 4]] {
            assert_eq!(old.apply_pixel(pixel), upgraded.apply_pixel(pixel));
        }
        old.vibrance = 50.;
        assert!(old.validate().is_err());
        upgraded.vibrance = f32::NAN;
        assert!(upgraded.validate().is_err());
    }

    #[test]
    fn vibrance_preserves_luminance_alpha_and_protects_warm_hues() {
        let mut recipe = EditRecipe::neutral(RawEngine::default());
        recipe.process_version = 2;
        recipe.vibrance = 100.;
        let pixel = [0.4, 0.3, 0.2, 1.];
        let result = recipe.apply_pixel(pixel);
        // saturation=.5; weight=.25; factor=1.25.
        let y = 0.2627 * pixel[0] + 0.6780 * pixel[1] + 0.0593 * pixel[2];
        for c in 0..3 {
            assert!((result[c] - (y + (pixel[c] - y) * 1.25)).abs() < 1e-6);
        }
        let new_y = 0.2627 * result[0] + 0.6780 * result[1] + 0.0593 * result[2];
        assert!((new_y - y).abs() < 1e-6);
        for alpha in [0.01, 0.5, 1.] {
            let out =
                recipe.apply_pixel([pixel[0] * alpha, pixel[1] * alpha, pixel[2] * alpha, alpha]);
            assert_eq!(out[3], alpha);
            for c in 0..3 {
                assert!((out[c] / alpha - result[c]).abs() < 1e-6);
            }
        }
        recipe.protect_warm = true;
        let protected = recipe.apply_pixel(pixel);
        assert!((protected[0] - y).abs() < (result[0] - y).abs());
        recipe.vibrance = -100.;
        let reduced = recipe.apply_pixel(pixel);
        assert!((reduced[0] - y).abs() < (pixel[0] - y).abs());
        for pixel in [[0.4; 4], [1., 0., 0., 1.], [-0.2, 0.3, 1.5, 1.], [0.; 4]] {
            assert_eq!(recipe.apply_pixel(pixel), pixel);
        }
        assert!(
            recipe
                .neutralize_render_sample([0.4, 0.4, 0.4, 1.])
                .is_err()
        );
    }

    #[test]
    fn area_picker_recovers_cast_despite_brightness_noise_and_outliers() {
        for side in [5, 11] {
            let mut recipe = EditRecipe::neutral(RawEngine::default());
            recipe.temperature = 24.;
            recipe.tint = -15.;
            let mut pixels: Vec<_> = (0..side * side)
                .map(|i| {
                    let value = 0.2 + (i % 7) as f32 * 0.06;
                    let alpha = if i % 2 == 0 { 0.96 } else { 1. };
                    recipe.apply_pixel([value * alpha, value * alpha, value * alpha, alpha])
                })
                .collect();
            // One valid colour outlier and one invalid clipped pixel.
            pixels[0] = [0.8, 0.1, 0.2, 1.];
            pixels[1] = [1., 1., 1., 1.];
            let image = LinearImage::new(side, side, pixels).unwrap();
            let original = image.pixels.clone();
            let area = sample_rgb_area(&image, side / 2, side / 2, side).unwrap();
            assert_eq!(area.valid, (side * side - 1) as usize);
            assert!(area.chroma_spread < 1e-5);
            recipe.neutralize_render_sample(area.working).unwrap();
            assert!(recipe.temperature.abs() < 1e-3);
            assert!(recipe.tint.abs() < 1e-3);
            assert_eq!(image.pixels, original);
        }
    }

    #[test]
    fn area_picker_rejects_edges_invalid_pixels_and_mixed_colours() {
        let mut image = LinearImage::new(11, 11, vec![[0.4, 0.4, 0.4, 1.]; 121]).unwrap();
        for (x, y, side) in [
            (0, 5, 5),
            (5, 0, 5),
            (10, 5, 5),
            (5, 10, 5),
            (u32::MAX, 5, 11),
            (5, u32::MAX, 11),
            (5, 5, 3),
        ] {
            assert!(sample_rgb_area(&image, x, y, side).is_err());
        }
        for bad in [
            [0.; 4],
            [0.2, 0.2, 0.2, 0.5],
            [0.01, 0.1, 0.1, 1.],
            [1., 0.2, 0.2, 1.],
            [f32::NAN, 0.2, 0.2, 1.],
        ] {
            image.pixels[..25].fill(bad);
            assert!(sample_rgb_area(&image, 5, 5, 11).is_err());
        }
        image.pixels.fill([0.4, 0.4, 0.4, 1.]);
        image.pixels[..60].fill([0.5, 0.3, 0.4, 1.]);
        assert!(sample_rgb_area(&image, 5, 5, 11).is_err());
        image.pixels.fill([0.4, 0.4, 0.4, 1.]);
        image.pixels[..24].fill([0.; 4]);
        assert_eq!(sample_rgb_area(&image, 5, 5, 11).unwrap().valid, 97);
    }

    #[test]
    fn auto_actions_freeze_parameters_and_preserve_other_controls() {
        let image = LinearImage::new(64, 64, vec![[0.09, 0.09, 0.09, 1.]; 4096]).unwrap();
        let mut r = EditRecipe::neutral(RawEngine::TrueRenderer);
        r.auto_exposure(&image).unwrap();
        assert!((r.exposure_ev - 1.).abs() < 1e-5);
        r.auto_exposure(&image).unwrap();
        assert!((r.exposure_ev - 1.).abs() < 1e-5);
        r.temperature = 24.;
        r.tint = -15.;
        r.auto_rgb(&image).unwrap();
        assert!(r.temperature.abs() < 0.001 && r.tint.abs() < 0.001);
        assert_eq!(r.exposure_ev, 1.);
        let wire = serde_json::to_vec(&r).unwrap();
        let restored: EditRecipe = serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored, r);
        assert_eq!(image.pixels[0], [0.09, 0.09, 0.09, 1.]);
    }
    #[test]
    fn auto_refuses_invalid_samples_and_limits_exposure() {
        let mut r = EditRecipe::neutral(RawEngine::TrueRenderer);
        for p in [[0.; 4], [0.1, 0.1, 0.1, 0.5], [-1., -1., -1., 1.]] {
            let image = LinearImage::new(8, 8, vec![p; 64]).unwrap();
            assert!(r.auto_exposure(&image).is_err());
            assert!(r.auto_rgb(&image).is_err());
            assert!(r.is_neutral());
        }
        let image = LinearImage::new(8, 8, vec![[0.0001, 0.0001, 0.0001, 1.]; 64]).unwrap();
        r.auto_exposure(&image).unwrap();
        assert_eq!(r.exposure_ev, 3.);
        r.saturation = 1.;
        let before = r.clone();
        assert!(r.auto_rgb(&image).is_err());
        assert_eq!(r, before);
        let mut pixels = vec![[0.05, 0.05, 0.05, 1.]; 4096];
        pixels[..100].fill([0.9, 0.9, 0.9, 1.]);
        let bright = LinearImage::new(64, 64, pixels).unwrap();
        r = EditRecipe::neutral(RawEngine::TrueRenderer);
        r.auto_exposure(&bright).unwrap();
        assert!(r.exposure_ev.abs() < 1e-5);
    }
    #[test]
    fn raw_wb_is_serialized_validated_and_not_applied_again_to_working_rgb() {
        let mut r = EditRecipe::neutral(RawEngine::TrueRenderer);
        let old = serde_json::to_string(&r).unwrap();
        assert!(!old.contains("raw_wb"));
        r.raw_wb = crate::decoder::RawWhiteBalance {
            red: 1800,
            blue: 800,
            ..Default::default()
        };
        let pixel = [-0.1, 0.2, 1.5, 0.5];
        assert_eq!(r.apply_pixel(pixel), pixel);
        r.validate().unwrap();
        assert_eq!(
            serde_json::from_str::<EditRecipe>(&serde_json::to_string(&r).unwrap()).unwrap(),
            r
        );
        assert!(
            serde_json::from_str::<EditRecipe>(&old)
                .unwrap()
                .raw_wb
                .is_as_shot()
        );
        r.raw_wb.red = 0;
        assert!(r.validate().is_err());
        r.raw_wb.red = 1800;
        r.raw_engine = RawEngine::Apple;
        assert!(r.validate().is_err());
    }

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
