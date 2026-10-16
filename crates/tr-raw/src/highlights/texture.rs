//! Recover a missing Bayer green texture only when two independently fitted
//! relationships to original R/B agree. No assumed neutral or borrowed pixels.
use super::{Cell, Evidence, SATURATED, SIDE};
use crate::cfa::Cfa;

const TERMS: usize = 5;
const SPAN: usize = 192;
const HALO: usize = 24;
const TOLERANCE: f64 = 0.001;
const OFFSETS: [[isize; 4]; 3] = [[-3, -1, 1, 3], [1, 3, 5, 7], [-1, -3, -5, -7]];
const WEIGHTS: [[f64; 4]; 3] = [
    [-1. / 16., 9. / 16., 9. / 16., -1. / 16.],
    [35. / 16., -35. / 16., 21. / 16., -5. / 16.],
    [35. / 16., -35. / 16., 21. / 16., -5. / 16.],
];

fn kernel(p: usize, length: usize) -> usize {
    if p < 3 {
        1
    } else if p + 3 >= length {
        2
    } else {
        0
    }
}

// Normalized design, pivoting, explicit inverse condition check. No ridge
// prior that could silently pull an underdetermined colour towards neutral.
fn solve(normal: [[f64; TERMS]; TERMS], rhs: [f64; TERMS]) -> Option<[f64; TERMS]> {
    let mut a = [[0.; 2 * TERMS + 1]; TERMS];
    let norm = normal
        .iter()
        .map(|r| r.iter().map(|v| v.abs()).sum::<f64>())
        .fold(0., f64::max);
    for i in 0..TERMS {
        a[i][..TERMS].copy_from_slice(&normal[i]);
        a[i][TERMS + i] = 1.;
        a[i][2 * TERMS] = rhs[i];
    }
    for k in 0..TERMS {
        let pivot = (k..TERMS).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        a.swap(k, pivot);
        let scale = a[k][k];
        if !scale.is_finite() || scale.abs() <= 1e-12 * norm {
            return None;
        }
        for v in &mut a[k] {
            *v /= scale;
        }
        for i in 0..TERMS {
            if i == k {
                continue;
            }
            let scale = a[i][k];
            let pivot_row = a[k];
            for (v, p) in a[i].iter_mut().zip(pivot_row) {
                *v -= scale * p;
            }
        }
    }
    let inv_norm = a
        .iter()
        .map(|r| r[TERMS..2 * TERMS].iter().map(|v| v.abs()).sum::<f64>())
        .fold(0., f64::max);
    let result = std::array::from_fn(|i| a[i][2 * TERMS]);
    (norm * inv_norm <= 10000. && result.iter().all(|v| v.is_finite())).then_some(result)
}

#[derive(Clone, Copy)]
struct Fit {
    centre: [f64; 4],
    scale: [f64; 4],
    coefficients: [[f64; TERMS]; 2],
}

impl Fit {
    fn basis(&self, features: [f64; 4]) -> [f64; TERMS] {
        std::array::from_fn(|i| {
            if i == 0 {
                1.
            } else {
                (features[i - 1] - self.centre[i - 1]) / self.scale[i - 1]
            }
        })
    }
    fn predict(&self, features: [f64; 4]) -> [f64; 2] {
        let b = self.basis(features);
        self.coefficients
            .map(|p| p.into_iter().zip(b).map(|(p, b)| p * b).sum())
    }
}

struct Patch<'a> {
    values: &'a [f32],
    state: &'a [u8],
    size: [usize; 3],
    cfa: Cfa,
    bounds: [usize; 4],
}

impl Patch<'_> {
    fn features(&self, x: usize, y: usize, k: usize) -> Option<[f64; 4]> {
        let [stride, width, height] = self.size;
        let mut f = [0., 0., x as f64 / width as f64, y as f64 / height as f64];
        for (channel, out) in [0, 2].into_iter().zip(&mut f[..2]) {
            let horizontal = self.cfa.color(x ^ 1, y) == channel;
            let kernel = if horizontal { k % 3 } else { k / 3 };
            for (d, w) in OFFSETS[kernel].into_iter().zip(WEIGHTS[kernel]) {
                let (xx, yy) = if horizontal {
                    (x.checked_add_signed(d)?, y)
                } else {
                    (x, y.checked_add_signed(d)?)
                };
                if xx >= width || yy >= height || self.state[yy * stride + xx] != 0 {
                    return None;
                }
                let value = f64::from(self.values[yy * stride + xx]);
                if !value.is_finite() || value <= 0.005 {
                    return None;
                }
                *out += w * value;
            }
        }
        f.iter().all(|v| v.is_finite()).then_some(f)
    }

    fn observations(
        &self,
        phase: usize,
        k: usize,
        training: bool,
        mut visit: impl FnMut(usize, usize, [f64; 4], f64) -> Option<()>,
    ) -> Option<()> {
        let [left, top, right, bottom] = self.bounds;
        for y in top..=bottom {
            if self.cfa.color(phase, y) != 1 {
                continue;
            }
            let start = left + ((left & 1) ^ phase);
            for x in (start..=right).step_by(2) {
                let i = y * self.size[0] + x;
                if self.state[i] == SATURATED || (training && self.state[i] != 0) {
                    continue;
                }
                if let Some(features) = self.features(x, y, k) {
                    visit(x, y, features, f64::from(self.values[i]))?;
                }
            }
        }
        Some(())
    }

    fn fit(&self, phase: usize, k: usize) -> Option<Fit> {
        let mut fit = Fit {
            centre: [0.; 4],
            scale: [0.; 4],
            coefficients: [[0.; TERMS]; 2],
        };
        let mut count = 0.;
        // Welford moments avoid subtracting nearly equal squared signals.
        self.observations(phase, k, true, |_, _, f, _| {
            count += 1.;
            for (j, v) in f.into_iter().enumerate() {
                let d = v - fit.centre[j];
                fit.centre[j] += d / count;
                fit.scale[j] += d * (v - fit.centre[j]);
            }
            Some(())
        })?;
        if count < 64. {
            return None;
        }
        for scale in &mut fit.scale {
            *scale = (*scale / count).sqrt();
            if !scale.is_finite() || *scale < 1e-6 {
                return None;
            }
        }
        let mut normal = [[[0.; TERMS]; TERMS]; 2];
        let mut rhs = [[0.; TERMS]; 2];
        let mut counts = [0; 2];
        self.observations(phase, k, true, |x, y, f, v| {
            let fold = (x / 2 + y / 2) % 2;
            counts[fold] += 1;
            let b = fit.basis(f);
            for j in 0..TERMS {
                rhs[fold][j] += b[j] * v;
                // The other triangle has the same products in the same sample
                // order. Copy it below instead of accumulating each twice.
                for q in j..TERMS {
                    normal[fold][j][q] += b[j] * b[q];
                }
            }
            Some(())
        })?;
        for fold in 0..2 {
            if counts[fold] < 32 {
                return None;
            }
            for j in 0..TERMS {
                let (previous, rows) = normal[fold].split_at_mut(j);
                for (value, row) in rows[0].iter_mut().zip(previous) {
                    *value = row[j];
                }
            }
            fit.coefficients[fold] = solve(normal[fold], rhs[fold])?;
            let c = fit.coefficients[fold];
            if c[1].abs() / fit.scale[0] + c[2].abs() / fit.scale[1] > 4. {
                return None;
            }
        }
        // A single disagreement rejects the whole component. No later sample
        // can reverse that decision, and no values have been written yet.
        self.observations(phase, k, false, |_, _, f, v| {
            fit.predict(f)
                .into_iter()
                .all(|p| p.is_finite() && v > 0.005 && (p - v).abs() <= TOLERANCE * v)
                .then_some(())
        })?;
        Some(fit)
    }

    fn component(&self, queue: &[usize], columns: usize) -> Option<[[Option<Fit>; 9]; 2]> {
        let [stride, width, height] = self.size;
        let mut fits = [[None; 9]; 2];
        for &cell in queue {
            let (tx, ty) = (cell % columns, cell / columns);
            for y in ty * SIDE..((ty + 1) * SIDE).min(height) {
                for x in tx * SIDE..((tx + 1) * SIDE).min(width) {
                    let i = y * stride + x;
                    if self.state[i] != SATURATED {
                        continue;
                    }
                    let phase = x % 2;
                    let k = kernel(x, width) + 3 * kernel(y, height);
                    if fits[phase][k].is_none() {
                        fits[phase][k] = Some(self.fit(phase, k)?);
                    }
                    let p = fits[phase][k]?.predict(self.features(x, y, k)?);
                    let v = f64::from(self.values[i]);
                    if !p
                        .iter()
                        .all(|p| p.is_finite() && *p >= (1. - TOLERANCE) * v && *p <= 1.5 * v)
                        || (p[0] - p[1]).abs() > TOLERANCE * v
                        || !(((p[0] + p[1]) * 0.5) as f32).is_finite()
                    {
                        return None;
                    }
                }
            }
        }
        Some(fits)
    }
}

pub(super) fn recover(
    values: &mut [f32],
    state: &[u8],
    size: [usize; 3],
    cfa: Cfa,
    cells: &[Cell],
    unsupported: &mut [u8],
) -> Evidence {
    let [stride, width, height] = size;
    let columns = width.div_ceil(SIDE);
    let rows = height.div_ceil(SIDE);
    let mut evidence = Evidence::default();
    if cfa.period != 2 || width < 8 || height < 8 || !unsupported.iter().any(|&m| m != 0) {
        return evidence;
    }
    let mut queue = Vec::with_capacity(cells.len());
    for start in 0..cells.len() {
        if cells[start].clipped == 0 || unsupported[start] & 128 != 0 {
            continue;
        }
        queue.clear();
        queue.push(start);
        unsupported[start] |= 128;
        let (mut cursor, mut untouched) = (0, true);
        let mut bounds = [width, height, 0, 0];
        while cursor < queue.len() {
            let i = queue[cursor];
            cursor += 1;
            untouched &= cells[i].clipped == 2
                && unsupported[i] & 7 == 2
                && queue.len() <= (SPAN / SIDE + 2).pow(2);
            let (tx, ty) = (i % columns, i / columns);
            if untouched {
                for y in ty * SIDE..((ty + 1) * SIDE).min(height) {
                    for x in tx * SIDE..((tx + 1) * SIDE).min(width) {
                        if state[y * stride + x] == SATURATED {
                            bounds[0] = bounds[0].min(x);
                            bounds[1] = bounds[1].min(y);
                            bounds[2] = bounds[2].max(x);
                            bounds[3] = bounds[3].max(y);
                        }
                    }
                }
            }
            for y in ty.saturating_sub(1)..=(ty + 1).min(rows - 1) {
                for x in tx.saturating_sub(1)..=(tx + 1).min(columns - 1) {
                    let j = y * columns + x;
                    if cells[j].clipped != 0 && unsupported[j] & 128 == 0 {
                        unsupported[j] |= 128;
                        queue.push(j);
                    }
                }
            }
        }
        let [left, top, right, bottom] = bounds;
        if !untouched || right - left >= SPAN || bottom - top >= SPAN {
            continue;
        }
        let bounds = [
            left.saturating_sub(HALO),
            top.saturating_sub(HALO),
            (right + HALO).min(width - 1),
            (bottom + HALO).min(height - 1),
        ];
        let patch = Patch {
            values,
            state,
            size,
            cfa,
            bounds,
        };
        let Some(fits) = patch.component(&queue, columns) else {
            continue;
        };
        // Commit only a complete component, after checking every clipped site.
        // Original R/B are never changed, so application order is irrelevant.
        for &cell in &queue {
            unsupported[cell] = (unsupported[cell] & 128) | 8;
            evidence.supported_cells += 1;
            let (tx, ty) = (cell % columns, cell / columns);
            for y in ty * SIDE..((ty + 1) * SIDE).min(height) {
                for x in tx * SIDE..((tx + 1) * SIDE).min(width) {
                    let i = y * stride + x;
                    if state[i] != SATURATED {
                        continue;
                    }
                    let k = kernel(x, width) + 3 * kernel(y, height);
                    let f = (Patch {
                        values,
                        state,
                        size,
                        cfa,
                        bounds,
                    })
                    .features(x, y, k)
                    .expect("validated original features");
                    let p = fits[x % 2][k].expect("complete component").predict(f);
                    let p = ((p[0] + p[1]) * 0.5) as f32;
                    if p > values[i] {
                        values[i] = p;
                        evidence.reconstructed += 1;
                    }
                }
            }
        }
    }
    for mask in unsupported {
        *mask &= 15;
    }
    evidence
}

#[cfg(test)]
#[path = "texture_tests.rs"]
mod tests;
