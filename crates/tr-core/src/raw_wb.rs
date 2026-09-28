//! Bounded native-WB parameter fitting. The callback must develop RAW without
//! creative edits at native resolution. Only the resolved parameters persist.
use crate::{
    color::LinearImage,
    decoder::{RawEngine, RawWhiteBalance},
    editing::sample_rgb_area,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Analysis {
    Auto,
    Patch { x: u32, y: u32, side: u32 },
}

fn measure(image: &LinearImage, analysis: Analysis) -> Result<[f32; 2]> {
    if let Analysis::Patch { x, y, side } = analysis {
        let p = sample_rgb_area(image, x, y, side)?.working;
        return Ok([(p[0] / p[1]).ln(), (p[2] / p[1]).ln()]);
    }
    let nx = image.width.min(32);
    let ny = image.height.min(32);
    let mut values = [Vec::new(), Vec::new()];
    for j in 0..ny {
        for i in 0..nx {
            let x = ((2 * i as u64 + 1) * image.width as u64 / (2 * nx as u64)) as usize;
            let y = ((2 * j as u64 + 1) * image.height as u64 / (2 * ny as u64)) as usize;
            let p = image.pixels[y * image.width as usize + x];
            if !(0.95..=1.).contains(&p[3]) {
                continue;
            }
            let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
            if !rgb
                .iter()
                .all(|v| v.is_finite() && (0.02..0.95).contains(v))
            {
                continue;
            }
            values[0].push((rgb[0] / rgb[1]).ln());
            values[1].push((rgb[2] / rgb[1]).ln());
        }
    }
    let n = values[0].len();
    ensure!(
        n >= 16 && n * 4 >= (nx * ny) as usize,
        "WB RAW: campioni validi insufficienti"
    );
    let mut result = [0.; 2];
    for (v, out) in values.iter_mut().zip(&mut result) {
        v.sort_by(f32::total_cmp);
        let trim = n / 5;
        *out = v[trim..n - trim].iter().sum::<f32>() / (n - 2 * trim) as f32;
    }
    Ok(result)
}
fn parameters(wb: RawWhiteBalance, engine: RawEngine) -> [f32; 2] {
    if engine == RawEngine::Apple {
        [
            (wb.apple_temperature as f32 / 6500.).ln(),
            wb.apple_tint as f32 / 100.,
        ]
    } else {
        [(wb.red as f32 / 1000.).ln(), (wb.blue as f32 / 1000.).ln()]
    }
}
fn resolve(p: [f32; 2], engine: RawEngine) -> Result<RawWhiteBalance> {
    ensure!(p.iter().all(|v| v.is_finite()), "WB RAW: stima non finita");
    let wb = if engine == RawEngine::Apple {
        let k = 6500. * p[0].exp();
        let tint = p[1] * 100.;
        ensure!(
            (2000. ..=50000.).contains(&k) && (-150. ..=150.).contains(&tint),
            "WB RAW: stima oltre scala"
        );
        RawWhiteBalance {
            apple_temperature: k.round() as u16,
            apple_tint: tint.round() as i16,
            ..Default::default()
        }
    } else {
        let r = 1000. * p[0].exp();
        let b = 1000. * p[1].exp();
        ensure!(
            (250. ..=4000.).contains(&r) && (250. ..=4000.).contains(&b),
            "WB RAW: stima oltre scala"
        );
        RawWhiteBalance {
            red: r.round() as u16,
            blue: b.round() as u16,
            ..Default::default()
        }
    };
    wb.validate_for(engine)?;
    Ok(wb)
}

/// At most 16 serial developments, each raster dropped before the next.
/// No clamped answer: convergence and capability failures leave the recipe intact.
pub fn estimate(
    engine: RawEngine,
    initial: RawWhiteBalance,
    analysis: Analysis,
    mut develop: impl FnMut(RawWhiteBalance) -> Result<LinearImage>,
) -> Result<RawWhiteBalance> {
    initial.validate_for(engine)?;
    if let Analysis::Patch { side, .. } = analysis {
        ensure!(matches!(side, 5 | 11), "WB RAW: area richiesta 5×5 o 11×11");
    }
    let start = if engine == RawEngine::Apple && initial.is_as_shot() {
        RawWhiteBalance {
            apple_temperature: 6500,
            ..Default::default()
        }
    } else {
        initial
    };
    let mut p = parameters(start, engine);
    for iteration in 0..6 {
        let wb = resolve(p, engine)?;
        p = parameters(wb, engine);
        let residual = measure(&develop(wb)?, analysis)?;
        let norm = residual[0].abs().max(residual[1].abs());
        if norm <= 0.008 {
            return Ok(wb);
        }
        ensure!(iteration < 5, "WB RAW: stima non convergente");
        let mut columns = [[0.; 2]; 2];
        for c in 0..2 {
            let mut q = p;
            q[c] += 0.04;
            let perturbed = match resolve(q, engine) {
                Ok(wb) => wb,
                Err(_) => {
                    q[c] = p[c] - 0.04;
                    resolve(q, engine)?
                }
            };
            let delta = parameters(perturbed, engine)[c] - p[c];
            let r = measure(&develop(perturbed)?, analysis)?;
            columns[c] = [(r[0] - residual[0]) / delta, (r[1] - residual[1]) / delta];
        }
        let [a, b] = columns;
        let determinant = a[0] * b[1] - b[0] * a[1];
        ensure!(
            determinant.is_finite() && determinant.abs() > 0.001,
            "WB RAW: risposta cromatica instabile"
        );
        let step = [
            (b[1] * residual[0] - b[0] * residual[1]) / determinant,
            (-a[1] * residual[0] + a[0] * residual[1]) / determinant,
        ];
        for c in 0..2 {
            p[c] -= step[c].clamp(-0.35, 0.35);
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_fit_recovers_gains_and_rejects_unresponsive_or_bad_samples() {
        for analysis in [
            Analysis::Auto,
            Analysis::Patch {
                x: 8,
                y: 8,
                side: 5,
            },
        ] {
            let mut calls = 0;
            let wb = estimate(
                RawEngine::TrueRenderer,
                Default::default(),
                analysis,
                |wb| {
                    calls += 1;
                    let g = wb.gains();
                    LinearImage::new(16, 16, vec![[0.3 * g[0], 0.4, 0.5 * g[2], 1.]; 256])
                },
            )
            .unwrap();
            assert!((i32::from(wb.red) - 1333).abs() <= 4);
            assert!((i32::from(wb.blue) - 800).abs() <= 4);
            assert!(calls <= 16);
        }
        for pixel in [[0.3, 0.4, 0.5, 1.], [0.; 4]] {
            assert!(
                estimate(
                    RawEngine::TrueRenderer,
                    Default::default(),
                    Analysis::Auto,
                    |_| LinearImage::new(16, 16, vec![pixel; 256])
                )
                .is_err()
            );
        }
        assert!(
            estimate(
                RawEngine::TrueRenderer,
                Default::default(),
                Analysis::Patch {
                    x: 0,
                    y: 0,
                    side: 5
                },
                |_| LinearImage::new(16, 16, vec![[0.4; 4]; 256])
            )
            .is_err()
        );
    }
}
