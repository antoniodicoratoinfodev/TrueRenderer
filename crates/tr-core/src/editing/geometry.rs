//! Process 3 inverse geometry. Coordinates are edges of the oriented source,
//! normalized to [0,1]; interpolation is linear on premultiplied samples.
use crate::color::{LinearImage, Pixel};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub crop: [f32; 4],
    pub quarter_turns: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub angle: f32,
    pub perspective: [f32; 2],
    pub scale: f32,
    pub distortion: f32,
    pub moustache: f32,
    pub vignette: f32,
    pub ca: [f32; 2],
    pub defringe: f32,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            crop: [0., 0., 1., 1.],
            quarter_turns: 0,
            flip_horizontal: false,
            flip_vertical: false,
            angle: 0.,
            perspective: [0.; 2],
            scale: 1.,
            distortion: 0.,
            moustache: 0.,
            vignette: 0.,
            ca: [0.; 2],
            defringe: 0.,
        }
    }
}
impl Geometry {
    /// Rotate the existing framing in the oriented source domain, without
    /// reinterpreting previously saved recipes or using a cropped raster.
    pub fn rotate_crop(&mut self, turns: u8) {
        for _ in 0..turns % 4 {
            let [l, t, r, b] = self.crop;
            self.crop = [1. - b, l, 1. - t, r];
            self.perspective = [-self.perspective[1], self.perspective[0]];
            self.quarter_turns = (self.quarter_turns + 1) % 4;
        }
    }
    /// Reflect the output frame, preserving its selected source area.
    pub fn reflect_crop(&mut self, horizontal: bool) {
        let axis = usize::from(!horizontal);
        let previous = self.crop;
        self.crop[axis] = 1. - previous[axis + 2];
        self.crop[axis + 2] = 1. - previous[axis];
        self.perspective[axis] = -self.perspective[axis];
        self.angle = -self.angle;
        if horizontal != (self.quarter_turns % 2 == 1) {
            self.flip_horizontal = !self.flip_horizontal;
        } else {
            self.flip_vertical = !self.flip_vertical;
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.crop
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                // A 1% interval such as 0.50..0.51 rounds just below 0.01.
                && self.crop[2] - self.crop[0] >= 0.01 - f32::EPSILON
                && self.crop[3] - self.crop[1] >= 0.01 - f32::EPSILON,
            "Ritaglio non valido (minimo 1% per asse)"
        );
        ensure!(self.quarter_turns < 4, "Rotazione non valida");
        super::range(self.angle, -45., 45., "Raddrizzamento")?;
        super::range(self.scale, 1., 4., "Scala")?;
        for v in self.perspective {
            super::range(v, -100., 100., "Prospettiva")?;
        }
        for v in [self.distortion, self.moustache, self.vignette] {
            super::range(v, -100., 100., "Ottica")?;
        }
        for v in self.ca {
            super::range(v, -10., 10., "Aberrazione laterale")?;
        }
        super::range(self.defringe, 0., 100., "Defringe")
    }
    pub fn output_size(&self, source: [u32; 2]) -> [u32; 2] {
        let [w, h] = self.rotated_size(source);
        [
            ((self.crop[2] - self.crop[0]) as f64 * w as f64)
                .round()
                .max(1.) as u32,
            ((self.crop[3] - self.crop[1]) as f64 * h as f64)
                .round()
                .max(1.) as u32,
        ]
    }
    fn rotated_size(&self, source: [u32; 2]) -> [u32; 2] {
        if self.quarter_turns % 2 == 1 {
            [source[1], source[0]]
        } else {
            source
        }
    }
    /// Inverse map from the output rectangle to the pre-geometry source.
    pub fn source_point(&self, p: [f64; 2], source: [u32; 2]) -> Option<[f64; 2]> {
        self.source_point_with_rotation(p, source, (self.angle as f64).to_radians().sin_cos())
    }
    fn source_point_with_rotation(
        &self,
        p: [f64; 2],
        source: [u32; 2],
        (sin, cos): (f64, f64),
    ) -> Option<[f64; 2]> {
        let [w, h] = self.rotated_size(source).map(f64::from);
        let unit = w.max(h) / 2.;
        let mut x = ((self.crop[0] as f64 + p[0] * (self.crop[2] - self.crop[0]) as f64) * w
            - w / 2.)
            / unit;
        let mut y = ((self.crop[1] as f64 + p[1] * (self.crop[3] - self.crop[1]) as f64) * h
            - h / 2.)
            / unit;
        x /= self.scale as f64;
        y /= self.scale as f64;
        let divisor =
            1. + self.perspective[0] as f64 * 0.003 * x + self.perspective[1] as f64 * 0.003 * y;
        if divisor <= 0.05 {
            return None;
        }
        x /= divisor;
        y /= divisor;
        (x, y) = (cos * x + sin * y, -sin * x + cos * y);
        let (u, v) = (x * unit / w + 0.5, y * unit / h + 0.5);
        let (mut u, mut v) = match self.quarter_turns {
            1 => (v, 1. - u),
            2 => (1. - u, 1. - v),
            3 => (1. - v, u),
            _ => (u, v),
        };
        if self.flip_horizontal {
            u = 1. - u;
        }
        if self.flip_vertical {
            v = 1. - v;
        }
        let unit = f64::from(source[0].max(source[1])) / 2.;
        let x = (u - 0.5) * source[0] as f64 / unit;
        let y = (v - 0.5) * source[1] as f64 / unit;
        let r2 = x * x + y * y;
        // Saturating radial terms keep the radial derivative positive across
        // the entire admitted parameter range, including points beyond frame.
        let factor = 1.
            + self.distortion as f64 * 0.003 * r2 / (1. + r2)
            + self.moustache as f64 * 0.0015 * r2 * r2 / (1. + r2 * r2);
        Some([
            0.5 + x * factor * unit / source[0] as f64,
            0.5 + y * factor * unit / source[1] as f64,
        ])
    }
    pub fn has_mapping(&self) -> bool {
        let baseline = Self {
            vignette: self.vignette,
            defringe: self.defringe,
            ..Default::default()
        };
        *self != baseline
    }
    pub fn apply(&self, image: &mut LinearImage, native: [u32; 2], base: u32) -> Result<[u32; 2]> {
        let output = self.output_size(native);
        if self.has_mapping() {
            let divisor = 1_u32
                .checked_shl(base)
                .ok_or_else(|| anyhow::anyhow!("Livello non valido"))?;
            let [w, h] = output.map(|v| v.div_ceil(divisor));
            let mut pixels = vec![[0.; 4]; w as usize * h as usize];
            let rotation = (self.angle as f64).to_radians().sin_cos();
            super::pixels_mut(&mut pixels, |i, target| {
                let (x, y) = (i % w as usize, i / w as usize);
                let Some(p) = self.source_point_with_rotation(
                    [(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64],
                    native,
                    rotation,
                ) else {
                    return;
                };
                *target = sample(image, p);
                // Lateral CA samples each colour in source space. Intersect
                // coverages so a missing channel never becomes a valid pixel.
                if self.ca != [0.; 2] && target[3] > 0. {
                    let mut colors = [*target; 3];
                    for (c, amount) in [(0, self.ca[0]), (2, self.ca[1])] {
                        // Tiny inputs must not collapse or reverse a channel
                        // at offsets larger than their own half-width.
                        let gain =
                            (1. + 2. * amount as f64 / native[0].max(native[1]) as f64).max(0.05);
                        colors[c] = sample(image, p.map(|v| 0.5 + (v - 0.5) * gain));
                    }
                    let a = colors.iter().map(|p| p[3]).fold(1_f32, f32::min);
                    for c in 0..3 {
                        target[c] = if colors[c][3] > 0. {
                            colors[c][c] / colors[c][3] * a
                        } else {
                            0.
                        };
                    }
                    target[3] = a;
                }
            });
            *image = LinearImage::new(w, h, pixels)?;
        }
        Ok(output)
    }
    /// Sensor-space radial gain and colour fringe attenuation before resampling.
    pub fn optical_color(&self, p: &mut Pixel, x: f32, y: f32) {
        if p[3] <= 0. {
            return;
        }
        let r2 = (x - 0.5).powi(2) + (y - 0.5).powi(2);
        let gain = (self.vignette / 100. * r2 * 4.).exp2();
        for v in &mut p[..3] {
            *v *= gain;
        }
    }
}

fn sample(image: &LinearImage, p: [f64; 2]) -> Pixel {
    if p.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v)) {
        return [0.; 4];
    }
    let x = (p[0] * image.width as f64 - 0.5).clamp(0., image.width as f64 - 1.);
    let y = (p[1] * image.height as f64 - 0.5).clamp(0., image.height as f64 - 1.);
    let (ix, iy) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = ((x - ix as f64) as f32, (y - iy as f64) as f32);
    let get = |x: u32, y: u32| {
        image.pixels[(y.min(image.height - 1) * image.width + x.min(image.width - 1)) as usize]
    };
    let (a, b, c, d) = (
        get(ix, iy),
        get(ix + 1, iy),
        get(ix, iy + 1),
        get(ix + 1, iy + 1),
    );
    std::array::from_fn(|i| {
        (a[i] + (b[i] - a[i]) * fx) * (1. - fy) + (c[i] + (d[i] - c[i]) * fx) * fy
    })
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    #[test]
    fn rotating_and_reflecting_off_center_crops_preserves_source_samples() {
        for quarter_turns in 0..4 {
            for flips in 0..4 {
                let original = Geometry {
                    crop: [0.1, 0.2, 0.7, 0.9],
                    quarter_turns,
                    flip_horizontal: flips & 1 != 0,
                    flip_vertical: flips & 2 != 0,
                    angle: 13.,
                    perspective: [12., -8.],
                    distortion: 10.,
                    ..Default::default()
                };
                for action in 0..3 {
                    let mut transformed = original.clone();
                    match action {
                        0 => transformed.rotate_crop(1),
                        1 => transformed.reflect_crop(true),
                        _ => transformed.reflect_crop(false),
                    }
                    transformed.validate().unwrap();
                    for p in [[0.1, 0.2], [0.4, 0.5], [0.9, 0.8]] {
                        let old = match action {
                            0 => [p[1], 1. - p[0]],
                            1 => [1. - p[0], p[1]],
                            _ => [p[0], 1. - p[1]],
                        };
                        let a = original.source_point(old, [800, 600]).unwrap();
                        let b = transformed.source_point(p, [800, 600]).unwrap();
                        assert!(
                            (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
                            "q={quarter_turns} flips={flips} action={action}: {a:?} {b:?}"
                        );
                    }
                }
            }
        }
    }
}
