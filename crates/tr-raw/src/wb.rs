//! Robust neutral/grey-world estimates on calibrated CFA sites, before WB,
//! demosaic or colour conversion. No scene-adaptive exposure is applied.
use crate::{cfa::Cfa, color::Calibration, render};
use anyhow::{Result, ensure};
use tr_core::{
    color::LinearImage,
    decoder::{RawEngine, RawWhiteBalance},
    raw_wb::Analysis,
};

#[cfg(test)]
#[path = "../research/wb_spatial.rs"]
mod spatial_lab;
#[cfg(test)]
mod tests;

pub(crate) struct Mosaic {
    pub values: Vec<f32>,
    pub saturation: Vec<u8>,
    pub highlight_confidence: Vec<u8>,
    pub width: usize,
    pub active_width: usize,
    pub height: usize,
    pub cfa: Cfa,
    pub crop: [usize; 4],
    pub orientation: u32,
    pub calibration: Calibration,
}

#[derive(Clone, Copy)]
struct Site {
    x: f64,
    y: f64,
    value: f64,
}

fn fit_plane(sites: &[Site], weights: &[f64]) -> Option<[f64; 3]> {
    let weight: f64 = weights.iter().sum();
    let mut mean = [0.; 3];
    for (s, w) in sites.iter().zip(weights) {
        mean[0] += w * s.x / weight;
        mean[1] += w * s.y / weight;
        mean[2] += w * s.value / weight;
    }
    let (mut xx, mut xy, mut yy, mut xv, mut yv) = (0., 0., 0., 0., 0.);
    for (s, w) in sites.iter().zip(weights) {
        let (x, y, v) = (s.x - mean[0], s.y - mean[1], s.value - mean[2]);
        xx += w * x * x;
        xy += w * x * y;
        yy += w * y * y;
        xv += w * x * v;
        yv += w * y * v;
    }
    let determinant = xx * yy - xy * xy;
    // Normalized coordinates; reject a line/point or a poorly supported
    // plane instead of falling back to the spatially biased mean.
    if determinant <= 1e-8 * (xx + yy).powi(2) {
        return None;
    }
    let dx = (xv * yy - yv * xy) / determinant;
    let dy = (yv * xx - xv * xy) / determinant;
    let centre = mean[2] - dx * mean[0] - dy * mean[1];
    [centre, dx, dy]
        .iter()
        .all(|v| v.is_finite())
        .then_some([centre, dx, dy])
}

// Fit v_c(x,y) = colour_c * (a + b*x + d*y). All channels see the same
// illumination surface, with green's colour fixed to 1 to remove the scale
// ambiguity. Independent means sample different positions; independent planes
// give a single outlier excessive leverage in a 5x5 patch. The common surface
// uses every valid CFA site, without inventing RGB pixels or a camera profile.
fn region_ratios(channels: &[Vec<Site>; 3]) -> Option<[f64; 2]> {
    let means = channels
        .each_ref()
        .map(|v| v.iter().map(|s| s.value).sum::<f64>() / v.len() as f64);
    let mut colour = means.map(|v| v / means[1]);
    let sites: Vec<_> = channels.iter().flatten().copied().collect();
    let labels: Vec<_> = channels
        .iter()
        .enumerate()
        .flat_map(|(c, v)| std::iter::repeat_n(c, v.len()))
        .collect();
    let mut weights = vec![1.; sites.len()];
    let mut residuals = vec![0.; sites.len()];
    let mut normalized = sites.clone();
    let mut plane_weights = weights.clone();
    for _ in 0..32 {
        for (i, &c) in labels.iter().enumerate() {
            normalized[i].value = sites[i].value / colour[c];
            plane_weights[i] = weights[i] * colour[c] * colour[c];
        }
        let mut plane = fit_plane(&normalized, &plane_weights)?;
        let light = |s: &Site, p: [f64; 3]| p[0] + p[1] * s.x + p[2] * s.y;
        let mut numerator = [0.; 3];
        let mut denominator = [0.; 3];
        for (i, &c) in labels.iter().enumerate() {
            let l = light(&sites[i], plane);
            if !l.is_finite() || l <= 0. {
                return None;
            }
            numerator[c] += weights[i] * l * sites[i].value;
            denominator[c] += weights[i] * l * l;
        }
        let updated: [f64; 3] = std::array::from_fn(|c| numerator[c] / denominator[c]);
        let next = updated.map(|v| v / updated[1]);
        plane.iter_mut().for_each(|v| *v *= updated[1]);
        let change = (0..3)
            .map(|c| (next[c] / colour[c] - 1.).abs())
            .fold(0., f64::max);
        colour = next;
        for (i, &c) in labels.iter().enumerate() {
            residuals[i] = (sites[i].value / colour[c] - light(&sites[i], plane)).abs();
        }
        residuals.sort_by(f64::total_cmp);
        let middle = residuals.len() / 2;
        let median = (residuals[(residuals.len() - 1) / 2] + residuals[middle]) * 0.5;
        // This is an outlier cutoff, not a calibrated sensor noise estimate.
        // The floor is below one 16-bit code but above fp32 roundoff here.
        let cutoff = (2.5 * median).max(1e-7);
        let mut weight_change = 0_f64;
        for (i, &c) in labels.iter().enumerate() {
            let residual = (sites[i].value / colour[c] - light(&sites[i], plane)).abs();
            let w = if residual <= cutoff {
                1.
            } else {
                cutoff / residual
            };
            weight_change = weight_change.max((w - weights[i]).abs());
            weights[i] = w;
        }
        if change < 1e-8 && weight_change < 1e-6 {
            return colour
                .iter()
                .all(|c| (0.003..0.985).contains(&(c * plane[0])))
                .then_some([(colour[0]).ln(), (colour[2]).ln()]);
        }
    }
    // Do not accept an unstable estimate or silently switch estimators.
    None
}

impl Mosaic {
    pub fn develop(mut self, relative: [f64; 3]) -> Result<LinearImage> {
        let (gains, matrix) = self.calibration.resolve(relative)?;
        let mut unsupported = Vec::new();
        crate::highlights::recover_with_support(
            &mut self.values,
            &self.saturation,
            self.width,
            self.active_width,
            self.height,
            self.cfa,
            Some(&mut unsupported),
        );
        // Bayer's axis-edge admission needs original clipping provenance,
        // even when HL has supplied a valid estimate. Keep one bit per site;
        // X-Trans uses only the existing cell support contract.
        let clipped = if self.cfa.period == 2 {
            render::original_clipping_bits(&self.saturation)
        } else {
            Vec::new()
        };
        let fallback = crate::highlight_render::Fallback::new(
            self.highlight_confidence,
            &self.saturation,
            render::Support::new(&unsupported, self.active_width.div_ceil(6)),
            self.width,
            self.active_width,
            self.height,
            self.cfa,
        );
        drop(self.saturation);
        for (i, v) in self.values.iter_mut().enumerate() {
            *v *= gains[self.cfa.color(i % self.width, i / self.width)];
        }
        render::develop_supported(
            (
                &self.values,
                render::Support::new(&unsupported, self.active_width.div_ceil(6))
                    .with_original_clipping(&clipped, self.width)
                    .with_highlight_fallback(&fallback),
            ),
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
        if w < 2 || h < 2 {
            return None;
        }
        let mut sites: [Vec<Site>; 3] = std::array::from_fn(|_| Vec::new());
        let mut total = [0usize; 3];
        for y in top..top + h {
            for x in left..left + w {
                let c = self.cfa.color(x, y);
                total[c] += 1;
                let v = f64::from(self.values[y * self.width + x]);
                // Fixed sensor-relative rejection, independent of selected WB.
                if v.is_finite() && (0.003..0.985).contains(&v) {
                    sites[c].push(Site {
                        x: 2. * (x - left) as f64 / (w - 1) as f64 - 1.,
                        y: 2. * (y - top) as f64 / (h - 1) as f64 - 1.,
                        value: v,
                    });
                }
            }
        }
        if (0..3).any(|c| sites[c].len() < 3 || sites[c].len() * 4 < total[c] * 3) {
            return None;
        }
        // Complete odd Bayer windows are centrally symmetric in every colour:
        // their historical trimmed means already evaluate an affine signal at
        // the same centre. Preserve that estimator (also on curved shading).
        // Rejected sites break the symmetry and require the common model.
        if self.cfa.period == 2
            && w % 2 == 1
            && h % 2 == 1
            && (0..3).all(|c| sites[c].len() == total[c])
        {
            let means = sites.map(|s| {
                let mut v: Vec<_> = s.into_iter().map(|s| s.value).collect();
                v.sort_by(f64::total_cmp);
                let trim = v.len() / 5;
                v[trim..v.len() - trim].iter().sum::<f64>() / (v.len() - 2 * trim) as f64
            });
            return Some([(means[0] / means[1]).ln(), (means[2] / means[1]).ln()]);
        }
        region_ratios(&sites)
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
        let wb = RawWhiteBalance::precise_sensor_gains(gains)?;
        wb.validate_for(RawEngine::TrueRendererExperimental)?;
        self.calibration.resolve(wb.gains_f64())?;
        Ok(wb)
    }
}
