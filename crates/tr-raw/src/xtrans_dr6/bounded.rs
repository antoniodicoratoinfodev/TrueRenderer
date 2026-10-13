//! Bounded execution of the frozen DR6 equations.
use super as refined;
use crate::cfa::Cfa;
use anyhow::{Result, ensure};

pub const CORE_EDGE: usize = 128;
pub const HALO: usize = 9;
pub const SCRATCH_RESERVATION_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Statistics {
    pub blocks: usize,
    pub pixels: usize,
    pub largest_tile_pixels: usize,
}

pub fn reconstruct_into(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    mut cancelled: impl FnMut() -> bool,
    mut emit: impl FnMut(usize, usize, [f32; 3]) -> Result<()>,
) -> Result<Statistics> {
    ensure!(
        width >= 6
            && height >= 6
            && cfa.period == 6
            && width.checked_mul(height) == Some(mosaic.len()),
        "DR6 geometry"
    );
    ensure!(mosaic.iter().all(|v| v.is_finite()), "DR6 nonfinite input");
    let mut stats = Statistics::default();
    for top in (0..height).step_by(CORE_EDGE) {
        for left in (0..width).step_by(CORE_EDGE) {
            ensure!(!cancelled(), "DR6 cancelled between blocks");
            let (right, bottom) = (
                left.saturating_add(CORE_EDGE).min(width),
                top.saturating_add(CORE_EDGE).min(height),
            );
            let (x0, y0) = (left.saturating_sub(HALO), top.saturating_sub(HALO));
            let (x1, y1) = (
                right.saturating_add(HALO).min(width),
                bottom.saturating_add(HALO).min(height),
            );
            let (w, h) = (x1 - x0, y1 - y0);
            let mut tile = Vec::with_capacity(w * h);
            for y in y0..y1 {
                tile.extend_from_slice(&mosaic[y * width + x0..y * width + x1]);
            }
            let rgb = refined::reconstruct(&tile, w, h, cfa.shifted(x0, y0));
            for y in top..bottom {
                for x in left..right {
                    emit(x, y, rgb[(y - y0) * w + x - x0])?;
                    stats.pixels += 1;
                }
            }
            stats.blocks += 1;
            stats.largest_tile_pixels = stats.largest_tile_pixels.max(w * h);
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfa::XTRANS;
    use std::cell::Cell;

    #[test]
    fn phases_seams_odd_sizes_and_measured_samples_match_scalar() {
        let mut maximum = 0f32;
        for (w, h) in [(6, 7), (17, 21), (269, 141)] {
            for phase in 0..36 {
                let cfa = Cfa::new(6, &XTRANS).unwrap().shifted(phase % 6, phase / 6);
                let mosaic: Vec<_> = (0..w * h)
                    .map(|i| {
                        let x = (i % w) as f32;
                        let y = (i / w) as f32;
                        -0.1 + 0.72 * (0.19 * x + 0.06 * y).sin()
                            + 0.3 * (0.071 * x - 0.11 * y).cos()
                            + cfa.color(i % w, i / w) as f32 * 0.47
                    })
                    .collect();
                let reference = refined::reconstruct(&mosaic, w, h, cfa);
                let mut visits = vec![0u8; w * h];
                let stats = reconstruct_into(
                    &mosaic,
                    w,
                    h,
                    cfa,
                    || false,
                    |x, y, p| {
                        let i = y * w + x;
                        visits[i] += 1;
                        assert_eq!(p[cfa.color(x, y)].to_bits(), mosaic[i].to_bits());
                        for c in 0..3 {
                            assert!(p[c].is_finite());
                            maximum = maximum.max((p[c] - reference[i][c]).abs());
                            assert!(
                                (p[c] - reference[i][c]).abs() <= 2e-6,
                                "phase {phase} ({x},{y}) channel {c}"
                            );
                        }
                        Ok(())
                    },
                )
                .unwrap();
                assert!(visits.into_iter().all(|v| v == 1));
                assert_eq!(stats.pixels, w * h);
                assert_eq!(stats.blocks, w.div_ceil(CORE_EDGE) * h.div_ceil(CORE_EDGE));
                assert!(stats.largest_tile_pixels <= (CORE_EDGE + 2 * HALO).pow(2));
            }
        }
        println!("DR6 bounded/scalar max abs camera RGB error {maximum}");
    }

    #[test]
    fn bounded_buffers_cancellation_and_sink_errors_are_explicit() {
        // Deliberately over-count buffers from stages that do not overlap.
        // The separately owned input/output and sink storage are excluded.
        let side = CORE_EDGE + 2 * HALO;
        let buffers = (side + 1).pow(2) * (18 + 4) * size_of::<f64>()
            + side.pow(2) * (size_of::<f64>() + 5 * size_of::<f32>() + 3 * size_of::<[f32; 3]>())
            + 256 * 1024; // neighbor vectors and metadata, conservative
        assert!(buffers <= SCRATCH_RESERVATION_BYTES);
        let (w, h) = (257, 133);
        let mosaic = vec![0.4; w * h];
        let cfa = Cfa::new(6, &XTRANS).unwrap();
        let emitted = Cell::new(0usize);
        let result = reconstruct_into(
            &mosaic,
            w,
            h,
            cfa,
            || emitted.get() > 0,
            |_, _, _| {
                emitted.set(emitted.get() + 1);
                Ok(())
            },
        );
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(emitted.get(), CORE_EDGE * CORE_EDGE);
        assert!(
            reconstruct_into(
                &mosaic,
                w,
                h,
                cfa,
                || false,
                |_, _, _| anyhow::bail!("sink failed")
            )
            .unwrap_err()
            .to_string()
            .contains("sink failed")
        );
        let mut touched = false;
        assert!(
            reconstruct_into(
                &[f32::NAN; 36],
                6,
                6,
                cfa,
                || false,
                |_, _, _| {
                    touched = true;
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(!touched);
    }
}
