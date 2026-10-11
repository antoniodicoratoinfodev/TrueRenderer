//! Robust neutral/grey-world estimates on calibrated CFA sites, before WB,
//! demosaic or colour conversion. No scene-adaptive exposure is applied.
use crate::{cfa::Cfa, color::Calibration, render};
use anyhow::{Result, ensure};
use tr_core::{
    color::LinearImage,
    decoder::{RawEngine, RawWhiteBalance},
    raw_wb::Analysis,
};

pub(crate) struct Mosaic {
    pub values: Vec<f32>,
    pub width: usize,
    pub height: usize,
    pub cfa: Cfa,
    pub crop: [usize; 4],
    pub orientation: u32,
    pub calibration: Calibration,
}
impl Mosaic {
    pub fn develop(mut self, relative: [f64; 3]) -> Result<LinearImage> {
        let (gains, matrix) = self.calibration.resolve(relative)?;
        for (i, v) in self.values.iter_mut().enumerate() {
            *v *= gains[self.cfa.color(i % self.width, i / self.width)];
        }
        render::develop(
            &self.values,
            self.width,
            self.height,
            self.cfa,
            matrix,
            self.crop,
            self.orientation,
        )
    }
    fn region(&self, rect: [usize; 4]) -> Option<[f64; 2]> {
        let [left, top, w, h] = rect;
        let mut sites: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
        let mut total = [0usize; 3];
        for y in top..top + h {
            for x in left..left + w {
                let c = self.cfa.color(x, y);
                total[c] += 1;
                let v = f64::from(self.values[y * self.width + x]);
                // Fixed sensor-relative rejection, independent of selected WB.
                if v.is_finite() && (0.003..0.985).contains(&v) {
                    sites[c].push(v);
                }
            }
        }
        if (0..3).any(|c| sites[c].len() < 3 || sites[c].len() * 4 < total[c] * 3) {
            return None;
        }
        let means: [f64; 3] = sites.map(|mut v| {
            v.sort_by(f64::total_cmp);
            let trim = v.len() / 5;
            v[trim..v.len() - trim].iter().sum::<f64>() / (v.len() - 2 * trim) as f64
        });
        Some([(means[0] / means[1]).ln(), (means[2] / means[1]).ln()])
    }
    pub fn estimate(&self, analysis: Analysis) -> Result<RawWhiteBalance> {
        let [left, top, w, h] = self.crop;
        let mut ratios = [Vec::new(), Vec::new()];
        match analysis {
            Analysis::Patch { x, y, side } => {
                ensure!(matches!(side, 5 | 11), "WB CFA: area richiesta 5×5 o 11×11");
                let (ow, oh) = render::oriented_size(w as u32, h as u32, self.orientation);
                ensure!(x < ow && y < oh, "WB CFA: punto fuori immagine");
                let (x, y) = (x as usize, y as usize);
                let (cx, cy) = match self.orientation {
                    2 => (w - 1 - x, y),
                    3 => (w - 1 - x, h - 1 - y),
                    4 => (x, h - 1 - y),
                    5 => (y, x),
                    6 => (y, h - 1 - x),
                    7 => (w - 1 - y, h - 1 - x),
                    8 => (w - 1 - y, x),
                    _ => (x, y),
                };
                let radius = side as usize / 2;
                let (x0, y0) = (cx.saturating_sub(radius), cy.saturating_sub(radius));
                let (x1, y1) = ((cx + radius + 1).min(w), (cy + radius + 1).min(h));
                if let Some(v) = self.region([left + x0, top + y0, x1 - x0, y1 - y0]) {
                    ratios[0].push(v[0]);
                    ratios[1].push(v[1]);
                }
                ensure!(
                    !ratios[0].is_empty(),
                    "WB CFA: area senza campioni validi sufficienti; evitare nero e saturi"
                );
            }
            Analysis::Auto => {
                let (nx, ny) = (32.min(w / 12), 32.min(h / 12));
                for y in 0..ny {
                    for x in 0..nx {
                        let cx = ((2 * x + 1) * w / (2 * nx)).clamp(6, w - 6);
                        let cy = ((2 * y + 1) * h / (2 * ny)).clamp(6, h - 6);
                        if let Some(v) = self.region([left + cx - 6, top + cy - 6, 12, 12]) {
                            ratios[0].push(v[0]);
                            ratios[1].push(v[1]);
                        }
                    }
                }
                ensure!(
                    ratios[0].len() >= 16 && ratios[0].len() * 4 >= nx * ny,
                    "WB CFA: regioni valide insufficienti"
                );
            }
        }
        let means = ratios.map(|mut v| {
            v.sort_by(f64::total_cmp);
            let trim = v.len() / 5;
            v[trim..v.len() - trim].iter().sum::<f64>() / (v.len() - 2 * trim) as f64
        });
        let neutral = self.calibration.neutral;
        let gains = [
            neutral[0] / neutral[1] / means[0].exp(),
            1.,
            neutral[2] / neutral[1] / means[1].exp(),
        ];
        ensure!(
            gains
                .iter()
                .all(|g| g.is_finite() && (0.25..=4.).contains(g)),
            "WB CFA: guadagni stimati oltre scala"
        );
        let wb = RawWhiteBalance {
            red: (1000. * gains[0]).round() as u16,
            blue: (1000. * gains[2]).round() as u16,
            ..Default::default()
        };
        wb.validate_for(RawEngine::TrueRendererExperimental)?;
        self.calibration.resolve(wb.gains().map(f64::from))?;
        Ok(wb)
    }
}
