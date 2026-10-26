//! Full-domain noise models, canonical patch lattice, regional reconstruction.
//! Coarse guides remain global; only the filter footprints are reconstructed.
use super::*;
use crate::editing::{detail::input::extract, regional::Rect};

#[allow(clippy::too_many_arguments)]
pub(in crate::editing::detail) fn apply(
    source: &impl Input,
    scale: f32,
    luminance: f32,
    chroma: f32,
    threshold: f32,
    rect: Rect,
    timings: &mut [u64; 4],
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<LinearImage> {
    let amounts = [luminance, chroma, chroma];
    if scale <= 0.3 {
        run::<4>(
            source, amounts, threshold, 1, false, rect, timings, cancelled,
        )
    } else if scale <= 0.6 {
        run::<8>(
            source, amounts, threshold, 2, false, rect, timings, cancelled,
        )
    } else {
        run::<16>(
            source, amounts, threshold, 3, false, rect, timings, cancelled,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn run<const N: usize>(
    source: &impl Input,
    amounts: [f32; 3],
    threshold: f32,
    levels: u32,
    coarse_level: bool,
    rect: Rect,
    timings: &mut [u64; 4],
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<LinearImage> {
    ensure!(!cancelled(), "Editing annullato");
    let coarse = amounts[1] > 0. && levels > 0 && source.width().min(source.height()) >= 32;
    let coarse_full = [source.width().div_ceil(2), source.height().div_ceil(2)];
    let coarse_origin = rect
        .origin
        .map(|v| ((v as f32 * 0.5 - 0.25).floor() as i64).max(0) as u32);
    let coarse_end: [u32; 2] = std::array::from_fn(|a| {
        (((rect.origin[a] + rect.size[a] - 1) as f32 * 0.5 - 0.25).floor() as i64 + 2)
            .clamp(1, i64::from(coarse_full[a])) as u32
    });
    let coarse_rect = Rect {
        origin: coarse_origin,
        size: std::array::from_fn(|a| coarse_end[a] - coarse_origin[a]),
    };
    let footprint = if coarse {
        Rect {
            origin: coarse_origin.map(|v| v * 2),
            size: std::array::from_fn(|a| {
                (coarse_end[a] * 2).min([source.width(), source.height()][a]) - coarse_origin[a] * 2
            }),
        }
    } else {
        rect
    };
    // Fine reconstruction and the chroma pyramid have no dependency until the
    // merge. Share the existing bounded CPU pool instead of serializing their
    // model/guide barriers. Each pixel retains its canonical arithmetic order.
    let (fine, pyramid) = crate::compute::install(|| {
        rayon::join(
            || -> Result<_> {
                let transform = denoise::Transform::<N>::new();
                let started = std::time::Instant::now();
                let model = Model::estimate(source, &transform, coarse_level, cancelled)?;
                let model_us = started.elapsed().as_micros() as u64;
                let started = std::time::Instant::now();
                let pixels = if model.active(amounts) {
                    denoise::filter_region(
                        source,
                        &transform,
                        cancelled,
                        &|block, level| model.shrink(block, level, amounts, threshold),
                        footprint,
                    )?
                } else {
                    extract(source, footprint)?.pixels
                };
                let filtered = LinearImage::new(footprint.size[0], footprint.size[1], pixels)?;
                Ok((filtered, model_us, started.elapsed().as_micros() as u64))
            },
            || -> Result<_> {
                if !coarse {
                    return Ok(None);
                }
                let started = std::time::Instant::now();
                let guide = downsample(source, cancelled)?;
                let mut times = [0, 0, started.elapsed().as_micros() as u64, 0];
                let reconstructed = run::<N>(
                    &guide,
                    [0., amounts[1], amounts[1]],
                    threshold,
                    levels - 1,
                    true,
                    coarse_rect,
                    &mut times,
                    cancelled,
                )?;
                Ok(Some((guide, reconstructed, times)))
            },
        )
    });
    let (filtered, model_us, filter_us) = fine?;
    ensure!(!cancelled(), "Editing annullato");
    timings[0] += model_us;
    timings[1] += filter_us;
    let Some((guide, reconstructed, times)) = pyramid? else {
        return Ok(filtered);
    };
    for (total, time) in timings.iter_mut().zip(times) {
        *total += time;
    }
    let started = std::time::Instant::now();
    let mut delta = downsample(&filtered, cancelled)?;
    ensure!(
        [delta.width, delta.height] == coarse_rect.size,
        "Supporto chroma incoerente"
    );
    for (p, q) in delta.pixels.iter_mut().zip(&reconstructed.pixels) {
        if p[3] > 0. && q[3] > 0. {
            let a = straight(*p);
            let b = straight(*q);
            let dy = luma(b) - luma(a);
            *p = [b[0] - a[0] - dy, 0., b[2] - a[2] - dy, 1.];
        } else {
            *p = [0.; 4];
        }
    }
    let mut output = extract(
        &filtered,
        Rect {
            origin: std::array::from_fn(|a| rect.origin[a] - footprint.origin[a]),
            size: rect.size,
        },
    )?;
    let width = output.width as usize;
    crate::editing::regional::visit_mut(&mut output.pixels, cancelled, |i, p| {
        if p[3] <= 0. {
            return;
        }
        let (x, y) = (
            rect.origin[0] as usize + i % width,
            rect.origin[1] as usize + i / width,
        );
        let (fx, fy) = (x as f32 * 0.5 - 0.25, y as f32 * 0.5 - 0.25);
        let (ix, iy) = (fx.floor() as isize, fy.floor() as isize);
        let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
        let rgb = straight(*p);
        let y0 = luma(rgb);
        let mut sum = [0.; 3];
        for (dy, wy) in [(0, 1. - ty), (1, ty)] {
            for (dx, wx) in [(0, 1. - tx), (1, tx)] {
                let xx = (ix + dx).clamp(0, guide.width as isize - 1) as usize;
                let yy = (iy + dy).clamp(0, guide.height as isize - 1) as usize;
                let d = delta.pixels[(yy - coarse_origin[1] as usize) * delta.width as usize + xx
                    - coarse_origin[0] as usize];
                let g = guide.pixels[yy * guide.width as usize + xx];
                if d[3] <= 0. || g[3] <= 0. {
                    continue;
                }
                let g = straight(g);
                let gy = luma(g);
                let range_y = 0.02 + 0.2 * y0.abs();
                let range_c = 0.015 + 0.08 * (y0.abs() + 0.003).sqrt();
                let distance = ((y0 - gy) / range_y).powi(2)
                    + ((rgb[0] - y0 - g[0] + gy) / range_c).powi(2)
                    + ((rgb[2] - y0 - g[2] + gy) / range_c).powi(2);
                let weight = wx * wy / (1. + distance).powi(2);
                sum[0] += weight * d[0];
                sum[1] += weight * d[2];
                sum[2] += weight;
            }
        }
        if sum[2] > 1e-12 {
            let confidence = (sum[2] * 2.).min(1.);
            let (dr, db) = (sum[0] / sum[2] * confidence, sum[1] / sum[2] * confidence);
            p[0] += dr * p[3];
            p[1] -= (0.2627 * dr + 0.0593 * db) / 0.678 * p[3];
            p[2] += db * p[3];
        }
    })?;
    timings[3] += started.elapsed().as_micros() as u64;
    ensure!(!cancelled(), "Editing annullato");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiscale_regions_match_full_filter_at_odd_edges_and_patch_boundaries() {
        let size = [265, 199];
        for alpha in [false, true] {
            let source = LinearImage::new(
                size[0],
                size[1],
                (0..size[0] * size[1])
                    .map(|i| {
                        let a = if alpha && i % 7 == 0 { 0. } else { 1. };
                        [
                            ((i * 17) % 113) as f32 / 113. * a,
                            ((i * 31) % 71) as f32 / 71. * a,
                            (i % 3) as f32 * 0.5 * a,
                            a,
                        ]
                    })
                    .collect(),
            )
            .unwrap();
            for scale in [0.25, 0.5, 1.] {
                let mut expected = source.clone();
                super::super::apply(&mut expected, scale, 0.3, 0.7, 0.2, &|| false).unwrap();
                for rect in [
                    Rect {
                        origin: [61, 63],
                        size: [67, 65],
                    },
                    Rect {
                        origin: [0, 0],
                        size: [33, 35],
                    },
                    Rect {
                        origin: [233, 170],
                        size: [32, 29],
                    },
                ] {
                    let actual =
                        apply(&source, scale, 0.3, 0.7, 0.2, rect, &mut [0; 4], &|| false).unwrap();
                    let expected = extract(&expected, rect).unwrap();
                    for (a, b) in actual
                        .pixels
                        .iter()
                        .flatten()
                        .zip(expected.pixels.iter().flatten())
                    {
                        assert_eq!(
                            a.to_bits(),
                            b.to_bits(),
                            "scale={scale}, alpha={alpha}, rect={rect:?}: {a} != {b}"
                        );
                    }
                }
            }
        }
    }
}
