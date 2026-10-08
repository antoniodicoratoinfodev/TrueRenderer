//! Versioned point operators in straight extended linear Rec.2020.
//! Only selection weights are bounded; RGB and luminance are never clipped.
use super::{hue_chroma, hue_rgb, luma, range};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

fn version(v: u32) -> Result<()> {
    ensure!(v == 1, "Versione operatore cromatico non supportata");
    Ok(())
}

fn mix(a: [f32; 3], b: [f32; 3], amount: f32) -> [f32; 3] {
    if amount == 0. {
        a
    } else if amount == 100. {
        b
    } else {
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * amount / 100.)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradingZone {
    pub hue: f32,
    pub amount: f32,
    pub exposure: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grading {
    pub version: u32,
    /// Shadows, midtones, highlights, global. Guide is fixed at layer input.
    pub zones: [GradingZone; 4],
    pub balance: f32,
    pub overlap: f32,
}
impl Default for Grading {
    fn default() -> Self {
        Self {
            version: 1,
            zones: [GradingZone::default(); 4],
            balance: 0.,
            overlap: 50.,
        }
    }
}
impl Grading {
    pub fn is_neutral(&self) -> bool {
        self.zones
            .iter()
            .all(|z| z.amount == 0. && z.exposure == 0.)
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.balance, -100., 100., "Bilanciamento grading")?;
        range(self.overlap, 0., 100., "Sovrapposizione grading")?;
        for z in self.zones {
            range(z.hue, 0., 360., "Tonalità grading")?;
            range(z.amount, 0., 100., "Cromia grading")?;
            range(z.exposure, -2., 2., "Luminanza grading")?;
        }
        Ok(())
    }
    pub fn weights(&self, guide: [f32; 3]) -> [f32; 3] {
        let y = luma(guide).max(0.);
        let pivot = 0.18 * (-self.balance / 50.).exp2();
        let t = y / (y + pivot);
        let power = 4. - 3. * self.overlap / 100.;
        let weights = [(1. - t).powi(2), 2. * t * (1. - t), t * t].map(|w| w.powf(power));
        let sum: f32 = weights.iter().sum();
        weights.map(|w| w / sum)
    }
    pub(super) fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        let weights = self.weights(guide);
        let mut ev = 0.;
        let mut tint = [0.; 3];
        for (zone, weight) in self.zones.iter().zip(weights.into_iter().chain([1.])) {
            ev += zone.exposure * weight;
            if zone.amount != 0. {
                let unit = hue_rgb(zone.hue);
                let y = luma(unit);
                for c in 0..3 {
                    tint[c] += (unit[c] - y) * zone.amount / 100. * weight;
                }
            }
        }
        let y = luma(rgb).abs();
        let gain = ev.exp2();
        std::array::from_fn(|c| (rgb[c] + tint[c] * y) * gain)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlackAndWhite {
    pub version: u32,
    pub amount: f32,
    /// Relative family luminance, -100..100. Source chroma fades the effect on grays.
    pub bands: [f32; 8],
}
impl Default for BlackAndWhite {
    fn default() -> Self {
        Self {
            version: 1,
            amount: 0.,
            bands: [0.; 8],
        }
    }
}
impl BlackAndWhite {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.amount, 0., 100., "Quantità B&N")?;
        for value in self.bands {
            range(value, -100., 100., "Luminosità famiglia B&N")?;
        }
        Ok(())
    }
    pub(super) fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        let (hue, chroma) = hue_chroma(guide);
        let centers = [0., 30., 60., 120., 180., 240., 270., 300., 360.];
        let i = (0..8).find(|i| hue < centers[i + 1]).unwrap_or(7);
        let t = (hue - centers[i]) / (centers[i + 1] - centers[i]);
        let amount = self.bands[i] + (self.bands[(i + 1) % 8] - self.bands[i]) * t;
        let protection = chroma / (chroma + luma(guide).abs() + 1e-6);
        let gray = luma(rgb) * (amount / 100. * protection).exp2();
        mix(rgb, [gray; 3], self.amount)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorFilter {
    pub version: u32,
    pub hue: f32,
    pub saturation: f32,
    pub density: f32,
    pub preserve_luminance: bool,
}
impl Default for ColorFilter {
    fn default() -> Self {
        Self {
            version: 1,
            hue: 40.,
            saturation: 100.,
            density: 0.,
            preserve_luminance: true,
        }
    }
}
impl ColorFilter {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.hue, 0., 360., "Tonalità filtro")?;
        range(self.saturation, 0., 100., "Saturazione filtro")?;
        range(self.density, 0., 100., "Densità filtro")
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let color = hue_rgb(self.hue);
        let density = self.density / 100. * self.saturation / 100.;
        let filtered = std::array::from_fn(|c| rgb[c] * (1. + density * (color[c] - 1.)));
        let offset = if self.preserve_luminance {
            luma(rgb) - luma(filtered)
        } else {
            0.
        };
        filtered.map(|v| v + offset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    pub position: f32,
    pub color: [f32; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientMap {
    pub version: u32,
    pub amount: f32,
    pub input: [f32; 2],
    pub reverse: bool,
    pub stops: Vec<GradientStop>,
}
impl Default for GradientMap {
    fn default() -> Self {
        Self {
            version: 1,
            amount: 0.,
            input: [0., 1.],
            reverse: false,
            stops: vec![
                GradientStop {
                    position: 0.,
                    color: [0.; 3],
                },
                GradientStop {
                    position: 1.,
                    color: [1.; 3],
                },
            ],
        }
    }
}
impl GradientMap {
    pub const MAX_STOPS: usize = 16;
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        range(self.amount, 0., 100., "Quantità mappa gradiente")?;
        for v in self.input {
            range(v, -16., 16., "Intervallo mappa gradiente")?;
        }
        ensure!(
            self.input[1] - self.input[0] >= 0.001,
            "Intervallo gradiente troppo piccolo"
        );
        ensure!(
            (2..=Self::MAX_STOPS).contains(&self.stops.len()),
            "Numero punti gradiente non valido"
        );
        ensure!(
            self.stops[0].position == 0. && self.stops.last().unwrap().position == 1.,
            "Il gradiente richiede gli estremi 0 e 1"
        );
        for (i, stop) in self.stops.iter().enumerate() {
            range(stop.position, 0., 1., "Posizione gradiente")?;
            for v in stop.color {
                range(v, -16., 16., "Colore gradiente lineare")?;
            }
            if i > 0 {
                ensure!(
                    stop.position - self.stops[i - 1].position >= 0.001,
                    "Punti gradiente troppo vicini o fuori ordine"
                );
            }
        }
        Ok(())
    }
    /// Linear interpolation in working RGB, with linear continuation at both ends.
    pub fn color_at(&self, t: f32) -> [f32; 3] {
        let i = self
            .stops
            .windows(2)
            .position(|w| t < w[1].position)
            .unwrap_or(self.stops.len() - 2);
        let [a, b] = [self.stops[i], self.stops[i + 1]];
        let f = (t - a.position) / (b.position - a.position);
        std::array::from_fn(|c| a.color[c] + (b.color[c] - a.color[c]) * f)
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut t = (luma(rgb) - self.input[0]) / (self.input[1] - self.input[0]);
        if self.reverse {
            t = 1. - t;
        }
        mix(rgb, self.color_at(t), self.amount)
    }
}
