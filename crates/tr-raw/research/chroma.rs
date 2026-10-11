//! Own DR3 experiment. Frozen formulas in docs/demosaic-xtrans-experimental.md.
//! Test-only; no new production recipe or external demosaic implementation.
use crate::{cfa::Cfa, xtrans::CameraRgb};

fn median(values: &mut [f32]) -> f32 {
    values.sort_unstable_by(f32::total_cmp);
    let n = values.len();
    if n.is_multiple_of(2) {
        values[n / 2 - 1] * 0.5 + values[n / 2] * 0.5
    } else {
        values[n / 2]
    }
}

pub fn reconstruct(mosaic: &[f32], width: usize, height: usize, cfa: Cfa) -> Vec<[f32; 3]> {
    let baseline = CameraRgb::new(mosaic, width, height, cfa);
    let mut previous: Vec<_> = (0..mosaic.len())
        .map(|i| baseline.at(i % width, i / width))
        .collect();
    drop(baseline);
    let mut next = vec![[0.; 3]; mosaic.len()];
    for _ in 0..2 {
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let mut red = [0.; 9];
                let mut blue = [0.; 9];
                let mut count = 0;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        let p = previous[yy * width + xx];
                        red[count] = p[0] - p[1];
                        blue[count] = p[2] - p[1];
                        count += 1;
                    }
                }
                let dr = median(&mut red[..count]);
                let db = median(&mut blue[..count]);
                let measured = cfa.color(x, y);
                let green = previous[i][1];
                let hypothesis = [green + dr, green, green + db];
                next[i] = std::array::from_fn(|c| {
                    if c == measured {
                        mosaic[i]
                    } else if c == 1 {
                        green
                    } else {
                        previous[i][c] * 0.25 + hypothesis[c] * 0.75
                    }
                });
            }
        }
        std::mem::swap(&mut previous, &mut next);
    }
    previous
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfa::XTRANS;

    #[test]
    fn extended_flats_and_measured_sites_across_phases_and_borders() {
        for (w, h) in [(6, 6), (7, 11), (29, 23)] {
            for phase in 0..36 {
                let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
                for rgb in [[-0.17, 0.37, 1.4], [0.; 3], [1.7; 3]] {
                    let mosaic: Vec<_> = (0..w * h).map(|i| rgb[cfa.color(i % w, i / w)]).collect();
                    let output = reconstruct(&mosaic, w, h, cfa);
                    for (i, p) in output.iter().enumerate() {
                        let c = cfa.color(i % w, i / w);
                        assert_eq!(p[c].to_bits(), mosaic[i].to_bits());
                        for channel in 0..3 {
                            assert!((p[channel] - rgb[channel]).abs() < 1e-6);
                        }
                    }
                }
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| ((i * 127 + 83) % 997) as f32 / 431. - 0.17)
                    .collect();
                let output = reconstruct(&mosaic, w, h, cfa);
                let baseline = CameraRgb::new(&mosaic, w, h, cfa);
                for (i, p) in output.iter().enumerate() {
                    assert!(p.iter().all(|v| v.is_finite()));
                    assert_eq!(p[1].to_bits(), baseline.at(i % w, i / w)[1].to_bits());
                    let c = cfa.color(i % w, i / w);
                    assert_eq!(p[c].to_bits(), mosaic[i].to_bits());
                }
            }
        }
    }
}
