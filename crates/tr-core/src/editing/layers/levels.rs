//! Frozen level analyses. Statistics are bounded and independent of display RGB.
use super::{Operator, TonalLevel, luma};
use crate::{color::LinearImage, editing::geometry::Geometry};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_LEVEL_SAMPLES: usize = 32768;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LevelsAction {
    AutoComposite,
    AutoChannels,
    Black,
    Gray,
    White,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelAnalysis {
    pub action: LevelsAction,
    pub channel: usize,
    pub source_digest: String,
    pub input_recipe_hash: String,
    pub geometry: Geometry,
    pub image_size: [u32; 2],
    pub valid_samples: usize,
    pub considered_samples: usize,
    pub sample_xy: Option<[u32; 2]>,
    pub sample_rgb: Option<[f32; 3]>,
}
impl LevelAnalysis {
    pub fn validate(&self) -> Result<()> {
        for hash in [&self.source_digest, &self.input_recipe_hash] {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "Riferimento analisi livelli non valido"
            );
        }
        self.geometry.validate()?;
        ensure!(
            self.channel <= 3 && self.image_size.iter().all(|v| *v > 0),
            "Ambito analisi livelli non valido"
        );
        ensure!(
            self.valid_samples > 0
                && self.valid_samples <= self.considered_samples
                && self.considered_samples <= MAX_LEVEL_SAMPLES,
            "Conteggio analisi livelli non valido"
        );
        match self.action {
            LevelsAction::AutoComposite | LevelsAction::AutoChannels => {
                ensure!(
                    self.channel == 0
                        && self.sample_xy.is_none()
                        && self.sample_rgb.is_none()
                        && self.valid_samples >= 32
                        && self.valid_samples * 4 >= self.considered_samples,
                    "Analisi Auto livelli non valida"
                );
            }
            _ => {
                let xy = self
                    .sample_xy
                    .ok_or_else(|| anyhow::anyhow!("Posizione campione mancante"))?;
                let rgb = self
                    .sample_rgb
                    .ok_or_else(|| anyhow::anyhow!("Campione livelli mancante"))?;
                ensure!(
                    self.valid_samples == 1
                        && self.considered_samples == 1
                        && xy[0] < self.image_size[0]
                        && xy[1] < self.image_size[1]
                        && rgb.iter().all(|v| v.is_finite()),
                    "Campione livelli non valido"
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelAdjustments {
    pub version: u32,
    pub channels: [TonalLevel; 4],
    pub analysis: Option<LevelAnalysis>,
}
impl Default for LevelAdjustments {
    fn default() -> Self {
        Self {
            version: 1,
            channels: [TonalLevel::default(); 4],
            analysis: None,
        }
    }
}
impl LevelAdjustments {
    pub fn is_neutral(&self) -> bool {
        self.channels == [TonalLevel::default(); 4]
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Versione livelli tonali non supportata");
        Operator::Levels(self.channels).validate()?;
        if let Some(analysis) = &self.analysis {
            analysis.validate()?;
        }
        Ok(())
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| self.channels[c + 1].apply(self.channels[0].apply(rgb[c])))
    }
    /// Freeze only the selected parameters; any error leaves the node intact.
    pub fn auto(&mut self, stats: &LevelStatistics, mut record: LevelAnalysis) -> Result<()> {
        self.validate()?;
        ensure!(
            matches!(
                record.action,
                LevelsAction::AutoComposite | LevelsAction::AutoChannels
            ),
            "Azione Auto livelli non valida"
        );
        ensure!(
            stats.composite == self.channels[0],
            "Analisi livelli superata"
        );
        ensure!(
            record.image_size == stats.image_size,
            "Ambito analisi livelli non valido"
        );
        let mut next = self.clone();
        let indices = if record.action == LevelsAction::AutoComposite {
            0..1
        } else {
            1..4
        };
        for i in indices {
            let [lo, hi] = stats.bounds[i];
            ensure!(
                lo.is_finite() && hi.is_finite() && hi - lo >= 0.001 && lo >= -16. && hi <= 16.,
                "Auto livelli: intervallo utile insufficiente o fuori scala"
            );
            next.channels[i].black = lo;
            next.channels[i].white = hi;
        }
        record.valid_samples = stats.valid;
        record.considered_samples = stats.considered;
        next.analysis = Some(record);
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn pick(&mut self, record: LevelAnalysis) -> Result<()> {
        self.validate()?;
        record.validate()?;
        let rgb = record
            .sample_rgb
            .ok_or_else(|| anyhow::anyhow!("Campione livelli mancante"))?;
        let mut next = self.clone();
        match record.action {
            LevelsAction::Black | LevelsAction::White => {
                let value = if record.channel == 0 {
                    luma(rgb)
                } else {
                    self.channels[0].apply(rgb[record.channel - 1])
                };
                let target = &mut next.channels[record.channel];
                if record.action == LevelsAction::Black {
                    target.black = value;
                } else {
                    target.white = value;
                }
            }
            LevelsAction::Gray => {
                let y = luma(self.apply(rgb));
                let channels = if record.channel == 0 {
                    1..4
                } else {
                    record.channel..record.channel + 1
                };
                for i in channels {
                    let c = &mut next.channels[i];
                    let x = (self.channels[0].apply(rgb[i - 1]) - c.black) / (c.white - c.black);
                    let target = (y - c.output[0]) / (c.output[1] - c.output[0]);
                    ensure!(
                        (1e-6..1. - 1e-6).contains(&x) && (1e-6..1. - 1e-6).contains(&target),
                        "Grigio livelli: campione fuori dai mezzitoni utili"
                    );
                    c.gamma = x.ln() / target.ln();
                }
            }
            _ => anyhow::bail!("Azione contagocce livelli non valida"),
        }
        next.analysis = Some(record);
        next.validate()?;
        *self = next;
        Ok(())
    }
}

/// The composite histogram uses input Y; channel histograms/Auto use the
/// composite mapping first, exactly as the independent channel levels do.
#[derive(Clone, Debug)]
pub struct LevelStatistics {
    pub image_size: [u32; 2],
    pub histogram: [[u32; 64]; 4],
    pub bounds: [[f32; 2]; 4],
    pub valid: usize,
    pub considered: usize,
    pub composite: TonalLevel,
}
impl LevelStatistics {
    pub fn from_image(image: &LinearImage, composite: TonalLevel) -> Result<Self> {
        Operator::Levels([
            composite,
            TonalLevel::default(),
            TonalLevel::default(),
            TonalLevel::default(),
        ])
        .validate()?;
        let count = image.pixels.len().min(MAX_LEVEL_SAMPLES);
        let mut values: [Vec<f32>; 4] = std::array::from_fn(|_| Vec::with_capacity(count));
        let mut histogram = [[0; 64]; 4];
        for i in 0..count {
            let p = image.pixels[i * image.pixels.len() / count];
            if !(0.95..=1.).contains(&p[3]) {
                continue;
            }
            let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
            let mapped = rgb.map(|v| composite.apply(v));
            let sample = [luma(rgb), mapped[0], mapped[1], mapped[2]];
            if !sample.iter().all(|v| v.is_finite()) {
                continue;
            }
            for c in 0..4 {
                values[c].push(sample[c]);
                histogram[c][(sample[c].clamp(0., 1.) * 63.) as usize] += 1;
            }
        }
        let valid = values[0].len();
        ensure!(
            valid >= 32 && valid * 4 >= count,
            "Auto livelli: campioni opachi insufficienti"
        );
        let bounds = values.map(|mut values| {
            values.sort_by(f32::total_cmp);
            [values[(valid - 1) / 100], values[(valid - 1) * 99 / 100]]
        });
        Ok(Self {
            image_size: [image.width, image.height],
            histogram,
            bounds,
            valid,
            considered: count,
            composite,
        })
    }
}
