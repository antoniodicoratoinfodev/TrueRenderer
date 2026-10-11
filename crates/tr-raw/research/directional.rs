//! Own DR1 experiment; formulas in docs/demosaic-xtrans-experimental.md.
//! No third-party demosaic source. Test-only, not a product selection.
use crate::{cfa::Cfa, xtrans::CameraRgb};

pub fn reconstruct(mosaic: &[f32], width: usize, height: usize, cfa: Cfa) -> Vec<[f32; 3]> {
    let baseline = CameraRgb::new(mosaic, width, height, cfa);
    let mut green = vec![0.; mosaic.len()];
    let offset = |x: usize, y: usize, dx: isize, dy: isize| {
        let (xx, yy) = (x as isize + dx, y as isize + dy);
        (xx >= 0 && yy >= 0 && xx < width as isize && yy < height as isize)
            .then_some((xx as usize, yy as usize))
    };
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let color = cfa.color(x, y);
            if color == 1 {
                green[i] = mosaic[i];
                continue;
            }
            let mut sum = 0.;
            let mut weights = 0.;
            for (dx, dy) in [(1isize, 0isize), (0, 1), (1, 1), (1, -1)] {
                let pair = |channel, limit: isize| -> Option<[(f32, f32); 2]> {
                    let find = |sign: isize| {
                        (1..=limit).find_map(|distance| {
                            let (xx, yy) =
                                offset(x, y, dx * distance * sign, dy * distance * sign)?;
                            (cfa.color(xx, yy) == channel)
                                .then_some((mosaic[yy * width + xx], distance as f32))
                        })
                    };
                    Some([find(-1)?, find(1)?])
                };
                let (Some(g), Some(c)) = (pair(1, 3), pair(color, 6)) else {
                    continue;
                };
                let interpolate =
                    |v: [(f32, f32); 2]| (v[0].0 * v[1].1 + v[1].0 * v[0].1) / (v[0].1 + v[1].1);
                let curvature = mosaic[i] - interpolate(c);
                let value = interpolate(g) + curvature * (g[0].1 * g[1].1) / (c[0].1 * c[1].1);
                let uncertainty = 0.002
                    + (g[0].0 - g[1].0).abs() / (g[0].1 + g[1].1)
                    + curvature.abs() / (c[0].1 * c[1].1);
                let weight = 1. / (uncertainty * uncertainty);
                sum += weight * value;
                weights += weight;
            }
            green[i] = if weights > 0. {
                sum / weights
            } else {
                baseline.at(x, y)[1]
            };
        }
    }
    // This research implementation prioritizes readable equations; avoid
    // inferring production memory/speed from its materialized RGB output.
    let mut output = vec![[0.; 3]; mosaic.len()];
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            output[i][1] = green[i];
            for channel in [0, 2] {
                if cfa.color(x, y) == channel {
                    output[i][channel] = mosaic[i];
                    continue;
                }
                let (mut sum, mut weights) = (0., 0.);
                for dy in -3isize..=3 {
                    for dx in -3isize..=3 {
                        let d2 = dx * dx + dy * dy;
                        if d2 == 0 || d2 > 10 {
                            continue;
                        }
                        let Some((xx, yy)) = offset(x, y, dx, dy) else {
                            continue;
                        };
                        if cfa.color(xx, yy) != channel {
                            continue;
                        }
                        let k = yy * width + xx;
                        let weight = (1. / d2 as f32) / (1. + 64. * (green[i] - green[k]).abs());
                        sum += weight * (mosaic[k] - green[k]);
                        weights += weight;
                    }
                }
                output[i][channel] = green[i] + sum / weights;
            }
        }
    }
    output
}
