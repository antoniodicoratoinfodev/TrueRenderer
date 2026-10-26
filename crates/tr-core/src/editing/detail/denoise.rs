//! SpectralV1: overlapping, signal-adaptive DCT shrinkage in Y / R-Y / B-Y.
//! Independent implementation of the sliding-transform principle described by
//! Yu & Sapiro, IPOL 2011 (doi:10.5201/ipol.2011.ys-dct); no reference code used.
//! A robust frequency-dependent noise estimate accounts for demosaic correlation.
//! Coefficients use linear, extended Rec.2020, never quantized display pixels.
use super::input::{Input, extract};
use crate::color::{LinearImage, Pixel};
use anyhow::{Result, ensure};
use rayon::prelude::*;

const TILE: usize = 128;
const MODEL_SAMPLES: usize = 1024;

fn reflect(x: isize, length: usize) -> usize {
    if length <= 1 {
        return 0;
    }
    let period = 2 * (length - 1) as isize;
    let x = x.rem_euclid(period);
    x.min(period - x) as usize
}

fn opponent(p: Pixel) -> [f32; 3] {
    let rgb = super::straight(p);
    let y = super::luma(rgb);
    [y, rgb[0] - y, rgb[2] - y]
}

// Generate fixed sizes so the compiler inlines the complete factorization;
// dynamic recursion costs more than the arithmetic on these small patches.
#[inline(always)]
fn identity1<const N: usize>(values: [Pixel; 1], _: &[[f32; N]; 5]) -> [Pixel; 1] {
    values
}
macro_rules! butterfly {
    ($forward:ident, $inverse:ident, $previous_forward:ident, $previous_inverse:ident, $size:literal, $half:literal, $level:literal) => {
        #[inline(always)]
        fn $forward<const N: usize>(
            values: [Pixel; $size],
            factors: &[[f32; N]; 5],
        ) -> [Pixel; $size] {
            let even = std::array::from_fn(|x| {
                std::array::from_fn(|c| values[x][c] + values[$size - 1 - x][c])
            });
            let odd = std::array::from_fn(|x| {
                std::array::from_fn(|c| {
                    (values[x][c] - values[$size - 1 - x][c]) * factors[$level][x]
                })
            });
            let even = $previous_forward(even, factors);
            let odd = $previous_forward(odd, factors);
            let mut result = [[0.; 4]; $size];
            for k in 0..$half {
                for c in 0..4 {
                    result[2 * k][c] = even[k][c];
                    result[2 * k + 1][c] =
                        odd[k][c] + if k + 1 < $half { odd[k + 1][c] } else { 0. };
                }
            }
            result
        }
        #[inline(always)]
        fn $inverse<const N: usize>(
            values: [Pixel; $size],
            factors: &[[f32; N]; 5],
        ) -> [Pixel; $size] {
            let even = std::array::from_fn(|k| values[2 * k]);
            let odd = std::array::from_fn(|k| {
                std::array::from_fn(|c| {
                    values[2 * k + 1][c] + if k > 0 { values[2 * k - 1][c] } else { 0. }
                })
            });
            let even = $previous_inverse(even, factors);
            let odd = $previous_inverse(odd, factors);
            let mut result = [[0.; 4]; $size];
            for x in 0..$half {
                for c in 0..4 {
                    let delta = odd[x][c] * factors[$level][x];
                    result[x][c] = even[x][c] + delta;
                    result[$size - 1 - x][c] = even[x][c] - delta;
                }
            }
            result
        }
    };
}
butterfly!(forward2, inverse2, identity1, identity1, 2, 1, 1);
butterfly!(forward4, inverse4, forward2, inverse2, 4, 2, 2);
butterfly!(forward8, inverse8, forward4, inverse4, 8, 4, 3);
butterfly!(forward16, inverse16, forward8, inverse8, 16, 8, 4);

pub(super) struct Transform<const N: usize> {
    #[cfg(test)]
    basis: [[f32; N]; N],
    factors: [[f32; N]; 5],
    norm: [f32; 2],
}
// Indexed row/column and butterfly coordinates are clearer than nested zips.
#[allow(clippy::needless_range_loop)]
impl<const N: usize> Transform<N> {
    pub(super) fn new() -> Self {
        Self {
            norm: [(1. / N as f32).sqrt(), (2. / N as f32).sqrt()],
            factors: std::array::from_fn(|level| {
                std::array::from_fn(|x| {
                    let size = 1_usize << level;
                    if x < size / 2 {
                        (0.5 / (std::f64::consts::PI / size as f64 * (x as f64 + 0.5)).cos()) as f32
                    } else {
                        0.
                    }
                })
            }),
            #[cfg(test)]
            basis: std::array::from_fn(|k| {
                std::array::from_fn(|x| {
                    let norm = if k == 0 {
                        (1. / N as f64).sqrt()
                    } else {
                        (2. / N as f64).sqrt()
                    };
                    (norm * (std::f64::consts::PI / N as f64 * (x as f64 + 0.5) * k as f64).cos())
                        as f32
                })
            }),
        }
    }
    // Recursive even/odd factorization of the DCT-II. The inverse is the
    // transpose of the same butterflies, with orthonormal scaling at the edges.
    // No FFT dependency or platform-specific arithmetic is required.
    fn forward_line(&self, source: &[Pixel; N]) -> [Pixel; N] {
        let mut target = [[0.; 4]; N];
        match N {
            4 => target[..4]
                .copy_from_slice(&forward4(source[..4].try_into().unwrap(), &self.factors)),
            8 => target[..8]
                .copy_from_slice(&forward8(source[..8].try_into().unwrap(), &self.factors)),
            16 => target[..16]
                .copy_from_slice(&forward16(source[..16].try_into().unwrap(), &self.factors)),
            _ => unreachable!("DCT sizes are statically selected"),
        }
        for (k, p) in target.iter_mut().enumerate() {
            for v in p {
                *v *= self.norm[usize::from(k != 0)];
            }
        }
        target
    }
    fn inverse_line(&self, source: &[Pixel; N]) -> [Pixel; N] {
        let mut scaled = *source;
        for (k, p) in scaled.iter_mut().enumerate() {
            for v in p {
                *v *= self.norm[usize::from(k != 0)];
            }
        }
        let mut target = [[0.; 4]; N];
        match N {
            4 => target[..4]
                .copy_from_slice(&inverse4(scaled[..4].try_into().unwrap(), &self.factors)),
            8 => target[..8]
                .copy_from_slice(&inverse8(scaled[..8].try_into().unwrap(), &self.factors)),
            16 => target[..16]
                .copy_from_slice(&inverse16(scaled[..16].try_into().unwrap(), &self.factors)),
            _ => unreachable!("DCT sizes are statically selected"),
        }
        target
    }
    fn forward(&self, block: &mut [[Pixel; N]; N]) {
        for row in block.iter_mut() {
            *row = self.forward_line(row);
        }
        for x in 0..N {
            let line = std::array::from_fn(|y| block[y][x]);
            let line = self.forward_line(&line);
            for y in 0..N {
                block[y][x] = line[y];
            }
        }
    }
    fn inverse(&self, block: &mut [[Pixel; N]; N]) {
        for row in block.iter_mut() {
            *row = self.inverse_line(row);
        }
        for x in 0..N {
            let line = std::array::from_fn(|y| block[y][x]);
            let line = self.inverse_line(&line);
            for y in 0..N {
                block[y][x] = line[y];
            }
        }
    }
    /// Subtract a patch constant before transforming: flat fields stay bit exact.
    pub(super) fn block(
        &self,
        image: &impl Input,
        x: isize,
        y: isize,
    ) -> Option<([[Pixel; N]; N], f32)> {
        let mut block = [[[0.; 4]; N]; N];
        let mut mean = [0_f64; 3];
        let xs: [usize; N] = std::array::from_fn(|dx| {
            if x >= 0 && x + N as isize <= image.width() as isize {
                x as usize + dx
            } else {
                reflect(x + dx as isize, image.width() as usize)
            }
        });
        for (dy, row) in block.iter_mut().enumerate() {
            let yy = if y >= 0 && y + N as isize <= image.height() as isize {
                y as usize + dy
            } else {
                reflect(y + dy as isize, image.height() as usize)
            };
            for (dx, target) in row.iter_mut().enumerate() {
                let xx = xs[dx];
                let p = image.pixel(xx, yy);
                if p[3] <= 0. {
                    return None;
                }
                target[..3].copy_from_slice(&opponent(p));
                if !target.iter().all(|v| v.is_finite()) {
                    return None;
                }
                for c in 0..3 {
                    mean[c] += f64::from(target[c]);
                }
            }
        }
        let mean = mean.map(|v| (v / (N * N) as f64) as f32);
        for row in &mut block {
            for p in row {
                for c in 0..3 {
                    p[c] -= mean[c];
                }
            }
        }
        self.forward(&mut block);
        Some((block, (mean[0].abs() + 0.003).sqrt()))
    }
}

fn quantile(values: &mut [f32], q: f32) -> f32 {
    if values.is_empty() {
        return 0.;
    }
    let i = ((values.len() - 1) as f32 * q) as usize;
    *values.select_nth_unstable_by(i, f32::total_cmp).1
}

pub(super) fn model<const N: usize>(
    image: &impl Input,
    transform: &Transform<N>,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<[[Pixel; N]; N]> {
    Ok(model_from_samples(&model_samples(
        image, transform, cancelled,
    )?))
}
pub(super) fn model_samples<const N: usize>(
    image: &impl Input,
    transform: &Transform<N>,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<Vec<(f32, [[Pixel; N]; N])>> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let nx = 32.min(w.div_ceil(N));
    let ny = 32.min(h.div_ceil(N));
    ensure!(nx * ny <= MODEL_SAMPLES, "Stima rumore oltre quota");
    let samples: Result<Vec<_>> = crate::compute::install(|| {
        (0..nx * ny)
            .into_par_iter()
            .map(|i| {
                ensure!(!cancelled(), "Editing annullato");
                let (ix, iy) = (i % nx, i / nx);
                let x = ix * w.saturating_sub(N) / nx.saturating_sub(1).max(1);
                let y = iy * h.saturating_sub(N) / ny.saturating_sub(1).max(1);
                if let Some((mut b, scale)) = transform.block(image, x as isize, y as isize) {
                    for row in &mut b {
                        for p in row {
                            for v in p {
                                *v = v.abs() / scale;
                            }
                        }
                    }
                    Ok(Some((scale, b)))
                } else {
                    Ok(None)
                }
            })
            .collect()
    });
    Ok(samples?.into_iter().flatten().collect())
}
pub(super) fn model_from_samples<const N: usize>(
    samples: &[(f32, [[Pixel; N]; N])],
) -> [[Pixel; N]; N] {
    let mut result = sample_quantiles(&samples.iter().collect::<Vec<_>>());
    let mut values = Vec::with_capacity(N * N);
    cap_model(&mut result, &mut values);
    result
}

pub(super) fn sample_quantiles<const N: usize>(
    samples: &[&(f32, [[Pixel; N]; N])],
) -> [[Pixel; N]; N] {
    let mut result = [[[0.; 4]; N]; N];
    if samples.is_empty() {
        return result;
    }
    // The 30th percentile of |N(0,1)| is 0.38532. Using a lower quantile
    // reduces contamination by sparse real edges and texture coefficients.
    crate::compute::install(|| {
        result.par_iter_mut().enumerate().for_each(|(y, row)| {
            // Avoid repeatedly scanning coefficients at a 4096-byte stride.
            // Small patch groups are transposed once into contiguous planes.
            let count = samples.len();
            let mut coefficients = vec![0.; count * N * 3];
            for (group, samples) in samples.chunks(8).enumerate() {
                let mut rows = [[[0.; 4]; N]; 8];
                for (row, (_, sample)) in rows.iter_mut().zip(samples) {
                    *row = sample[y];
                }
                for x in 0..N {
                    for c in 0..3 {
                        let begin = (x * 3 + c) * count + group * 8;
                        for (out, row) in coefficients[begin..begin + samples.len()]
                            .iter_mut()
                            .zip(&rows)
                        {
                            *out = row[x][c];
                        }
                    }
                }
            }
            for (x, p) in row.iter_mut().enumerate() {
                for (c, value) in p[..3].iter_mut().enumerate() {
                    let begin = (x * 3 + c) * count;
                    *value = quantile(&mut coefficients[begin..begin + count], 0.3) / 0.385_32;
                }
            }
        })
    });
    result
}

fn cap_model<const N: usize>(result: &mut [[Pixel; N]; N], values: &mut Vec<f32>) {
    // Bound low-frequency estimates by the mid/high-frequency floor. A clean
    // gradient or repeated pattern must not be interpreted as unlimited noise.
    let mut cap = [0.; 3];
    for c in 0..3 {
        values.clear();
        for (y, row) in result.iter().enumerate() {
            for (x, p) in row.iter().enumerate() {
                if (N / 2..=N + N / 2).contains(&(x + y)) {
                    values.push(p[c]);
                }
            }
        }
        cap[c] = 8. * quantile(values, 0.5);
    }
    let original = *result;
    for y in 0..N {
        for x in 0..N {
            for (c, limit) in cap.iter().enumerate() {
                result[y][x][c] = ((original[y][x][c] + original[x][y][c]) * 0.5).min(*limit);
            }
        }
    }
    result[0][0] = [0.; 4];
}

fn run<const N: usize>(
    image: &mut LinearImage,
    amounts: [f32; 3],
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<()> {
    let transform = Transform::<N>::new();
    let noise = model(image, &transform, cancelled)?;
    if noise.iter().flatten().flatten().all(|v| *v <= 1e-8) {
        return Ok(());
    }
    image.pixels = filter(image, &transform, cancelled, &|block, level| {
        for y in 0..N {
            for x in 0..N {
                for (c, amount) in amounts.iter().enumerate() {
                    let threshold = noise[y][x][c] * level * amount * 3.;
                    let t = (block[y][x][c].abs() / threshold.max(1e-12) - 0.75) / 0.75;
                    let t = t.clamp(0., 1.);
                    let keep = t * t * (3. - 2. * t);
                    block[y][x][c] *= if *amount == 0. { 0. } else { 1. - keep };
                }
            }
        }
    })?;
    Ok(())
}

/// Transactional shared transform/aggregation kernel; versioned coefficient
/// rules stay separate so historical recipes keep their floating-point result.
pub(super) fn filter<const N: usize>(
    image: &LinearImage,
    transform: &Transform<N>,
    cancelled: &(impl Fn() -> bool + Sync),
    shrink: &(impl Fn(&mut [[Pixel; N]; N], f32) + Sync),
) -> Result<Vec<Pixel>> {
    filter_region(
        image,
        transform,
        cancelled,
        shrink,
        super::super::regional::Rect::full([image.width, image.height]),
    )
}
pub(super) fn filter_region<const N: usize>(
    image: &impl Input,
    transform: &Transform<N>,
    cancelled: &(impl Fn() -> bool + Sync),
    shrink: &(impl Fn(&mut [[Pixel; N]; N], f32) + Sync),
    rect: super::super::regional::Rect,
) -> Result<Vec<Pixel>> {
    if image.cache_region() {
        let support = rect.expand(N as u32, [image.width(), image.height()]);
        let window = super::input::Window {
            source: image,
            rect: support,
            image: extract(image, support)?,
        };
        filter_region_inner(&window, transform, cancelled, shrink, rect)
    } else {
        filter_region_inner(image, transform, cancelled, shrink, rect)
    }
}
fn filter_region_inner<const N: usize>(
    image: &impl Input,
    transform: &Transform<N>,
    cancelled: &(impl Fn() -> bool + Sync),
    shrink: &(impl Fn(&mut [[Pixel; N]; N], f32) + Sync),
    rect: super::super::regional::Rect,
) -> Result<Vec<Pixel>> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let [rx, ry] = rect.origin.map(|v| v as usize);
    let [rw, rh] = rect.size.map(|v| v as usize);
    ensure!(
        rx + rw <= w && ry + rh <= h && rw > 0 && rh > 0,
        "Regione denoise non valida"
    );
    let mut output = vec![[0.; 4]; rw * rh];
    // Disjoint mutable row slices allow independent 2D tiles without unsafe
    // writes or a second output raster. Larger tiles avoid repeating boundary
    // transforms; horizontal tasks keep all cores busy on short viewports.
    let columns = rw.div_ceil(TILE);
    let mut tiles: Vec<Vec<&mut [Pixel]>> = (0..columns * rh.div_ceil(TILE))
        .map(|i| Vec::with_capacity((rh - (i / columns) * TILE).min(TILE)))
        .collect();
    for (y, row) in output.chunks_mut(rw).enumerate() {
        for (x, chunk) in row.chunks_mut(TILE).enumerate() {
            tiles[y / TILE * columns + x].push(chunk);
        }
    }
    crate::compute::install(|| {
        tiles
            .into_par_iter()
            .enumerate()
            .try_for_each(|(tile, mut rows)| -> Result<()> {
                let y0 = ry + (tile / columns) * TILE;
                let y1 = (y0 + TILE).min(ry + rh);
                let x0 = rx + (tile % columns) * TILE;
                ensure!(!cancelled(), "Editing annullato");
                let x1 = (x0 + TILE).min(rx + rw);
                let mut sums = [[0_f32; 4]; TILE * TILE];
                let step = (N / 4).max(1) as isize;
                let first = |v: usize| ((v as isize - N as isize).div_euclid(step) + 1) * step;
                for py in (first(y0)..y1 as isize).step_by(step as usize) {
                    ensure!(!cancelled(), "Editing annullato");
                    // Stagger consecutive patch rows: the checkerboard lattice
                    // retains eight estimates per pixel at native scale and
                    // 90-degree symmetry, with half the transform work.
                    let start = first(x0);
                    let start = start + (start / step + py / step).rem_euclid(2) * step;
                    for px in (start..x1 as isize).step_by(2 * step as usize) {
                        let Some((mut block, level)) = transform.block(image, px, py) else {
                            continue;
                        };
                        shrink(&mut block, level);
                        block[0][0] = [0.; 4];
                        transform.inverse(&mut block);
                        for yy in
                            y0.max(py.max(0) as usize)..y1.min((py + N as isize).max(0) as usize)
                        {
                            for xx in x0.max(px.max(0) as usize)
                                ..x1.min((px + N as isize).max(0) as usize)
                            {
                                let delta =
                                    block[(yy as isize - py) as usize][(xx as isize - px) as usize];
                                let acc = &mut sums[(yy - y0) * TILE + xx - x0];
                                for c in 0..3 {
                                    acc[c] += delta[c];
                                }
                                acc[3] += 1.;
                            }
                        }
                    }
                }
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let p = image.pixel(xx, yy);
                        let sum = sums[(yy - y0) * TILE + xx - x0];
                        let target = &mut rows[yy - y0][xx - x0];
                        *target = p;
                        if sum[3] > 0. {
                            let [dy, dr, db] = [sum[0], sum[1], sum[2]].map(|v| v / sum[3]);
                            target[0] -= (dy + dr) * p[3];
                            target[1] -= (dy - (0.2627 * dr + 0.0593 * db) / 0.6780) * p[3];
                            target[2] -= (dy + db) * p[3];
                        }
                    }
                }
                Ok(())
            })
    })?;
    ensure!(!cancelled(), "Editing annullato");
    Ok(output)
}

pub(super) fn regional(
    image: &impl Input,
    scale: f32,
    luminance: f32,
    chroma: f32,
    rect: super::super::regional::Rect,
    timings: &mut [u64; 4],
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<LinearImage> {
    fn run<const N: usize>(
        image: &impl Input,
        amounts: [f32; 3],
        rect: super::super::regional::Rect,
        timings: &mut [u64; 4],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        let transform = Transform::<N>::new();
        let started = std::time::Instant::now();
        let noise = model(image, &transform, cancelled)?;
        timings[0] += started.elapsed().as_micros() as u64;
        if noise.iter().flatten().flatten().all(|v| *v <= 1e-8) {
            return extract(image, rect);
        }
        let started = std::time::Instant::now();
        let pixels = filter_region(
            image,
            &transform,
            cancelled,
            &|block, level| {
                for y in 0..N {
                    for x in 0..N {
                        for (c, amount) in amounts.iter().enumerate() {
                            let threshold = noise[y][x][c] * level * amount * 3.;
                            let t = ((block[y][x][c].abs() / threshold.max(1e-12) - 0.75) / 0.75)
                                .clamp(0., 1.);
                            let keep = t * t * (3. - 2. * t);
                            block[y][x][c] *= if *amount == 0. { 0. } else { 1. - keep };
                        }
                    }
                }
            },
            rect,
        )?;
        timings[1] += started.elapsed().as_micros() as u64;
        LinearImage::new(rect.size[0], rect.size[1], pixels)
    }
    let amounts = [luminance, chroma, chroma];
    if scale <= 0.3 {
        run::<4>(image, amounts, rect, timings, cancelled)
    } else if scale <= 0.6 {
        run::<8>(image, amounts, rect, timings, cancelled)
    } else {
        run::<16>(image, amounts, rect, timings, cancelled)
    }
}

pub(super) fn apply(
    image: &mut LinearImage,
    scale: f32,
    luminance: f32,
    chroma: f32,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<()> {
    ensure!(!cancelled(), "Editing annullato");
    let amounts = [luminance, chroma, chroma];
    if scale <= 0.3 {
        run::<4>(image, amounts, cancelled)
    } else if scale <= 0.6 {
        run::<8>(image, amounts, cancelled)
    } else {
        run::<16>(image, amounts, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_model_samples_preserve_the_historical_grid_order_and_bits() {
        let image = LinearImage::new(
            533,
            487,
            (0..533 * 487)
                .map(|i| {
                    [
                        (i % 113) as f32 / 113.,
                        (i % 71) as f32 / 71.,
                        (i % 47) as f32 / 47.,
                        if i == 12345 { 0. } else { 1. },
                    ]
                })
                .collect(),
        )
        .unwrap();
        let transform = Transform::<16>::new();
        let mut expected = Vec::new();
        let (w, h) = (image.width as usize, image.height as usize);
        let (nx, ny) = (32.min(w.div_ceil(16)), 32.min(h.div_ceil(16)));
        for iy in 0..ny {
            for ix in 0..nx {
                let x = ix * w.saturating_sub(16) / nx.saturating_sub(1).max(1);
                let y = iy * h.saturating_sub(16) / ny.saturating_sub(1).max(1);
                if let Some((mut block, level)) = transform.block(&image, x as isize, y as isize) {
                    for p in block.iter_mut().flatten() {
                        for v in p {
                            *v = v.abs() / level;
                        }
                    }
                    expected.push((level, block));
                }
            }
        }
        let actual = model_samples(&image, &transform, &|| false).unwrap();
        assert!(!actual.is_empty());
        assert_eq!(actual.len(), expected.len());
        for ((level, block), (reference_level, reference)) in actual.iter().zip(&expected) {
            assert_eq!(level.to_bits(), reference_level.to_bits());
            for (a, b) in block
                .iter()
                .flatten()
                .flatten()
                .zip(reference.iter().flatten().flatten())
            {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }
        let sigma = sample_quantiles(&actual.iter().collect::<Vec<_>>());
        for (y, row) in sigma.iter().enumerate() {
            for (x, pixel) in row.iter().enumerate() {
                for c in 0..3 {
                    let mut values: Vec<_> =
                        expected.iter().map(|(_, block)| block[y][x][c]).collect();
                    values.sort_by(f32::total_cmp);
                    let expected = values[((values.len() - 1) as f32 * 0.3) as usize] / 0.385_32;
                    assert_eq!(pixel[c].to_bits(), expected.to_bits());
                }
            }
        }
    }
    #[test]
    fn symmetric_transform_roundtrips_and_matches_direct_cosines() {
        let transform = Transform::<16>::new();
        let mut block = std::array::from_fn(|y| {
            std::array::from_fn(|x| {
                [
                    ((y * 137 + x * 91) % 397) as f32 / 397.,
                    (x * y) as f32 / 256.,
                    (x + y) as f32 / 32.,
                    0.,
                ]
            })
        });
        let original = block;
        transform.forward(&mut block);
        for (ky, row) in block.iter().enumerate() {
            for (kx, p) in row.iter().enumerate() {
                for (c, v) in p.iter().enumerate() {
                    let mut direct = 0_f64;
                    for (y, row) in original.iter().enumerate() {
                        for (x, p) in row.iter().enumerate() {
                            direct += f64::from(p[c])
                                * f64::from(transform.basis[ky][y])
                                * f64::from(transform.basis[kx][x]);
                        }
                    }
                    assert!((direct - f64::from(*v)).abs() < 3e-6);
                }
            }
        }
        transform.inverse(&mut block);
        for (a, b) in block
            .iter()
            .flatten()
            .flatten()
            .zip(original.iter().flatten().flatten())
        {
            assert!((a - b).abs() < 1e-6);
        }
    }
    #[test]
    fn tiled_accumulation_matches_independent_pixel_reference_at_seams_and_borders() {
        let mut state = 48271_u32;
        let mut image = LinearImage::new(
            133,
            131,
            (0..133 * 131)
                .map(|i| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    let v = 0.2
                        + (state as f64 / u32::MAX as f64 * 0.04) as f32
                        + if i % 133 >= 64 { 0.2 } else { 0. };
                    [v, v * 0.8, v * 0.6, 1.]
                })
                .collect(),
        )
        .unwrap();
        let transform = Transform::<16>::new();
        let noise = model(&image, &transform, &|| false).unwrap();
        let source = image.clone();
        run::<16>(&mut image, [0.8; 3], &|| false).unwrap();
        for y in [0, 1, 63, 64, 65, 127, 128, 130] {
            for x in [0, 1, 63, 64, 65, 127, 128, 132] {
                let mut sum = [0.; 3];
                let mut count = 0.;
                for py in (y as isize - 15..=y as isize).filter(|v| v.rem_euclid(4) == 0) {
                    for px in (x as isize - 15..=x as isize).filter(|v| v.rem_euclid(4) == 0) {
                        if (px / 4 + py / 4).rem_euclid(2) != 0 {
                            continue;
                        }
                        let (mut b, level) = transform.block(&source, px, py).unwrap();
                        for yy in 0..16 {
                            for xx in 0..16 {
                                for c in 0..3 {
                                    let t = ((b[yy][xx][c].abs()
                                        / (noise[yy][xx][c] * level * 0.8 * 3.).max(1e-12)
                                        - 0.75)
                                        / 0.75)
                                        .clamp(0., 1.);
                                    b[yy][xx][c] *= 1. - t * t * (3. - 2. * t);
                                }
                            }
                        }
                        b[0][0] = [0.; 4];
                        transform.inverse(&mut b);
                        for (c, v) in sum.iter_mut().enumerate() {
                            *v += b[(y as isize - py) as usize][(x as isize - px) as usize][c];
                        }
                        count += 1.;
                    }
                }
                let [dy, dr, db] = sum.map(|v| v / count);
                let p = source.pixels[y * 133 + x];
                let expected = [
                    p[0] - dy - dr,
                    p[1] - dy + (0.2627 * dr + 0.0593 * db) / 0.678,
                    p[2] - dy - db,
                    1.,
                ];
                for (a, b) in image.pixels[y * 133 + x].iter().zip(expected) {
                    assert!((a - b).abs() < 1e-7, "seam {x},{y}");
                }
            }
        }
    }
}
