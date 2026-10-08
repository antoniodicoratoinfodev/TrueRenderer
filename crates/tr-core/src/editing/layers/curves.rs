//! Additional versioned curve mappings; historical curves keep their evaluator.
use super::{luma, range};
use crate::editing::{CurvePoint, EditRecipe, curve_has_adjusted_endpoints, tone_curve_value};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum CurveSpace {
    Rgb,
    #[default]
    Luminance,
}
impl CurveSpace {
    fn apply(self, rgb: [f32; 3], map: impl Fn(f32) -> f32) -> [f32; 3] {
        match self {
            Self::Rgb => rgb.map(map),
            Self::Luminance => {
                let y = luma(rgb);
                let offset = map(y) - y;
                rgb.map(|v| v + offset)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LuminanceCurve {
    pub version: u32,
    pub points: Vec<CurvePoint>,
}
impl Default for LuminanceCurve {
    fn default() -> Self {
        Self {
            version: 1,
            points: Vec::new(),
        }
    }
}
impl LuminanceCurve {
    pub fn is_neutral(&self) -> bool {
        !curve_has_adjusted_endpoints(&self.points) && self.points.iter().all(|p| p.x == p.y)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Versione curva luminanza non supportata");
        let mut r = EditRecipe::neutral(Default::default());
        r.process_version = 4;
        r.curve = self.points.clone();
        r.validate()
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        CurveSpace::Luminance.apply(rgb, |y| tone_curve_value(y, &self.points))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParametricCurve {
    pub version: u32,
    pub space: CurveSpace,
    pub boundaries: [f32; 3],
    /// Shadows, dark tones, light tones, highlights. Positive raises the zone.
    pub amounts: [f32; 4],
}
impl Default for ParametricCurve {
    fn default() -> Self {
        Self {
            version: 1,
            space: CurveSpace::Luminance,
            boundaries: [0.25, 0.5, 0.75],
            amounts: [0.; 4],
        }
    }
}
impl ParametricCurve {
    pub fn is_neutral(&self) -> bool {
        self.amounts == [0.; 4]
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Versione curva parametrica non supportata"
        );
        for v in self.amounts {
            range(v, -100., 100., "Zona curva parametrica")?;
        }
        let mut previous = 0.;
        for b in self.boundaries.into_iter().chain([1.]) {
            range(b, 0., 1., "Confine curva parametrica")?;
            ensure!(
                b - previous >= 0.02 - f32::EPSILON,
                "Zone parametriche troppo strette"
            );
            previous = b;
        }
        Ok(())
    }
    /// Each zone has a bounded quartic lift, zero value/slope change at its
    /// boundaries. The derivative stays positive for the entire allowed range.
    pub fn value(&self, x: f32) -> f32 {
        if x <= 0. || x >= 1. || self.is_neutral() {
            return x;
        }
        let edges = [
            0.,
            self.boundaries[0],
            self.boundaries[1],
            self.boundaries[2],
            1.,
        ];
        let i = self.boundaries.partition_point(|b| *b < x);
        if self.amounts[i] == 0. {
            return x;
        }
        let width = edges[i + 1] - edges[i];
        let t = (x - edges[i]) / width;
        x + 4. * self.amounts[i] / 100. * width * t.powi(2) * (1. - t).powi(2)
    }
    pub(super) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.space.apply(rgb, |x| self.value(x))
    }
}
