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
    pub curve: Vec<CurvePoint>,
}

impl EditRecipe {
    pub fn neutral(raw_engine: RawEngine) -> Self {
        Self {
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
            curve: vec![],
        }
    }

    pub fn is_neutral(&self) -> bool {
        let mut neutral = Self::neutral(self.raw_engine);
        neutral.curve = self.curve.clone();
        self == &neutral && self.curve.iter().all(|p| p.x == p.y)
    }

    pub fn validate(&self) -> Result<()> {
        self.raw_wb.validate_for(self.raw_engine)?;
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

    /// Fixed native-frame grid, at most 4096 samples, independent of viewport.
    /// Call only with the source developed under this recipe's RAW WB.
    fn analysis_samples(&self, image: &LinearImage) -> Result<Vec<Pixel>> {
        self.validate()?;
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
            self.saturation == 0.,
            "Azzerare la saturazione prima del contagocce RGB"
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
            self.saturation == 0.,
            "Azzerare la saturazione prima del contagocce RGB"
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
