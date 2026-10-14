//! Bounded CFA highlight estimates from locally consistent, unclipped colours.
//! No neutral assumption; valid measurements and unsupported highlights survive.
use crate::{
    cfa::Cfa,
    color::{Matrix, inverse, vector},
};

#[cfg(test)]
#[path = "../research/highlight_diagnostics.rs"]
mod diagnostics;
mod edge;
pub(crate) mod metadata;
mod occlusion;
#[cfg(test)]
mod tests;

const SIDE: usize = 6;
const RADIUS: usize = 32;
#[derive(Clone, Copy, Debug)]
struct Model {
    plane: [f32; 3],
    jump: f32,
    split: u8,
    split2: u8,
    side: u8,
    border: u8,
    correction: [f32; 2],
}

impl Model {
    fn predict(self, x: usize, y: usize) -> f32 {
        let side = usize::from(self.side);
        let v = edge::xy(x, y, side).map(|v| v as f32);
        let base = self.plane[0]
            + self.plane[1] * v[1]
            + self.plane[2] * v[2]
            + self.jump
                * ((edge::basis(self.split, x, y, side) + edge::basis(self.split2, x, y, side))
                    * 0.5) as f32;
        let cross = match self.border {
            1 if x == 0 => Some(v[2]),
            2 if x == side - 1 => Some(v[2]),
            3 if y == 0 => Some(v[1]),
            4 if y == side - 1 => Some(v[1]),
            _ => None,
        };
        if let Some(t) = cross {
            base + self.correction[0] + self.correction[1] * t
        } else {
            base
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Reconstruction {
    model: Model,
    ratio: [f32; 3],
    extended: bool,
}
type Estimate = Option<Reconstruction>;
pub(crate) const SATURATED: u8 = 2;

/// Classification in the linearized code domain, independent of normalization/WB.
pub(crate) fn classify(code: f64, black: f64, clipping: f64) -> u8 {
    if code == clipping {
        SATURATED
    } else if code >= black + 0.98 * (clipping - black) {
        1
    } else {
        0
    }
}

#[derive(Clone, Copy, Default)]
struct Cell {
    plane: [[f32; 3]; 3],
    jump: [f32; 3],
    split: u8,
    split2: u8,
    valid: u8,
    affine_valid: u8,
    clipped: u8,
    ready: bool,
}

struct Grid<'a> {
    data: Vec<Cell>,
    values: &'a [f32],
    state: &'a [u8],
    stride: usize,
    width: usize,
    height: usize,
    columns: usize,
    cfa: Cfa,
    designs: Vec<[(Matrix, usize); 3]>,
}

impl Grid<'_> {
    // Most photographs have few clipped sites. Fit only requested cells,
    // keeping the result independent of traversal order and source mutation.
    fn get(&mut self, x: usize, y: usize) -> Cell {
        let i = y * self.columns + x;
        if !self.data[i].ready {
            let (x0, y0) = (
                (x * SIDE).min(self.width - SIDE),
                (y * SIDE).min(self.height - SIDE),
            );
            let mut model = cell(
                self.values,
                self.state,
                self.stride,
                x0,
                y0,
                self.cfa.shifted(x0, y0),
                &self.designs[(y0 % self.cfa.period) * self.cfa.period + x0 % self.cfa.period],
            );
            model.clipped = self.data[i].clipped;
            model.ready = true;
            self.data[i] = model;
        }
        self.data[i]
    }
}

#[derive(Default, Debug)]
pub(crate) struct Evidence {
    pub saturated: usize,
    pub reconstructed: usize,
    pub supported_cells: usize,
}

pub(crate) fn scratch_bytes(width: usize, height: usize) -> usize {
    width * height
        + width.div_ceil(SIDE)
            * height.div_ceil(SIDE)
            * (size_of::<Cell>() + size_of::<Estimate>() + size_of::<usize>() + 3)
        + 16 * 1024
}

pub(crate) fn support_bytes(width: usize, height: usize) -> usize {
    width.div_ceil(SIDE) * height.div_ceil(SIDE)
}

fn coordinates(x: usize, y: usize) -> [f64; 3] {
    [1., (x as f64 - 2.5) / 2.5, (y as f64 - 2.5) / 2.5]
}

fn models(cfa: Cfa) -> [(Matrix, usize); 3] {
    let mut moments = [[[0.; 3]; 3]; 3];
    let mut counts = [0; 3];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let c = cfa.color(x, y);
            let v = coordinates(x, y);
            counts[c] += 1;
            for r in 0..3 {
                for k in 0..3 {
                    moments[c][r][k] += v[r] * v[k];
                }
            }
        }
    }
    std::array::from_fn(|c| (inverse(moments[c]).expect("complete CFA plane"), counts[c]))
}

fn cell(
    values: &[f32],
    state: &[u8],
    stride: usize,
    x0: usize,
    y0: usize,
    cfa: Cfa,
    models: &[(Matrix, usize); 3],
) -> Cell {
    let mut sum = [[0.; 3]; 3];
    let mut square = [0.; 3];
    let mut low = [f64::INFINITY; 3];
    let mut unsafe_channels = 0;
    let mut clipped = 0;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let i = (y0 + y) * stride + x0 + x;
            let c = cfa.color(x, y);
            let value = f64::from(values[i]);
            let xy = coordinates(x, y);
            for (s, k) in sum[c].iter_mut().zip(xy) {
                *s += value * k;
            }
            square[c] += value * value;
            low[c] = low[c].min(value);
            if state[i] > 0 {
                unsafe_channels |= 1 << c;
            }
            if state[i] == SATURATED {
                clipped |= 1 << c;
            }
        }
    }
    let mut out = Cell {
        clipped,
        ..Cell::default()
    };
    for c in 0..3 {
        let p = vector(models[c].0, sum[c]);
        let residual = ((square[c] - p.into_iter().zip(sum[c]).map(|(a, b)| a * b).sum::<f64>())
            / models[c].1 as f64)
            .max(0.)
            .sqrt();
        out.plane[c] = p.map(|v| v as f32);
        if unsafe_channels & (1 << c) == 0
            && low[c] > 0.005
            && p[0] > 0.01
            && residual <= 0.025 * p[0]
            && p[1].abs() + p[2].abs() <= 0.5 * p[0]
        {
            out.valid |= 1 << c;
        }
    }
    out.affine_valid = out.valid;
    if out.valid != (!unsafe_channels & 7) {
        edge::fit(values, state, stride, x0, y0, cfa, out)
    } else {
        out
    }
}

fn ratio_agrees(a: f32, b: f32) -> bool {
    a > 0. && b > 0. && (a - b).abs() <= 0.04 * a.max(b)
}

// Bayer needs one measured-site halo on each side of a six-pixel cell.
// Otherwise an edge between the final sampled colour and the next cell can
// be extrapolated on the wrong side, despite a valid sample two pixels away.
fn reconstruction_window(
    width: usize,
    height: usize,
    cfa: Cfa,
    tx: usize,
    ty: usize,
) -> [usize; 3] {
    let side = if cfa.period == 2 && width >= 8 && height >= 8 {
        8
    } else {
        SIDE
    };
    let halo = (side - SIDE) / 2;
    [
        (tx * SIDE).saturating_sub(halo).min(width - side),
        (ty * SIDE).saturating_sub(halo).min(height - side),
        side,
    ]
}

fn estimate(grid: &mut Grid<'_>, tx: usize, ty: usize) -> Estimate {
    let (columns, rows) = (grid.columns, grid.height.div_ceil(SIDE));
    if grid.data[ty * columns + tx].clipped == 0 {
        return None;
    }
    let target = grid.get(tx, ty);
    if target.clipped == 0 || target.valid == 0 {
        return None;
    }
    let anchor = (0..3)
        .filter(|c| target.valid & (1 << c) != 0)
        // Prefer headroom: the brightest surviving channel may itself clip
        // just beyond the target, preventing a consistent path to a seed.
        .min_by(|a, b| target.plane[*a][0].total_cmp(&target.plane[*b][0]))?;
    let mut candidates = Vec::with_capacity(16);
    for (dx, dy) in [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ] {
        let mut previous = target;
        let mut found = 0;
        for distance in 1..=RADIUS {
            let x = tx as isize + dx * distance as isize;
            let y = ty as isize + dy * distance as isize;
            if x < 0 || y < 0 || x >= columns as isize || y >= rows as isize {
                break;
            }
            let current = grid.get(x as usize, y as usize);
            // A path ends at an obstacle. It never jumps an unsupported
            // region to borrow a colour on the far side.
            if current.valid & (1 << anchor) == 0 {
                break;
            }
            // A large jump in the surviving channel blocks propagation.
            let a = previous.plane[anchor][0];
            let b = current.plane[anchor][0];
            if a > 2. * b || b > 2. * a {
                break;
            }
            let consistent = (0..3)
                .filter(|c| target.valid & current.valid & (1 << c) != 0)
                .all(|c| {
                    ratio_agrees(
                        target.plane[c][0] / target.plane[anchor][0],
                        current.plane[c][0] / current.plane[anchor][0],
                    )
                });
            if !consistent {
                break;
            }
            if current.valid == 7 {
                let ratio = current.plane.map(|p| p[0] / current.plane[anchor][0]);
                candidates.push(ratio);
                found += 1;
                if found == 2 {
                    break;
                }
            }
            previous = current;
        }
    }
    if candidates.len() < 2 {
        return None;
    }
    let first = candidates[0];
    if candidates
        .iter()
        .any(|p| (0..3).any(|c| target.clipped & (1 << c) != 0 && !ratio_agrees(first[c], p[c])))
    {
        return None;
    }
    let ratio = std::array::from_fn(|c| {
        candidates.iter().map(|p| p[c]).sum::<f32>() / candidates.len() as f32
    });
    let (x0, y0) = (
        (tx * SIDE).min(grid.width - SIDE),
        (ty * SIDE).min(grid.height - SIDE),
    );
    // DR6's chroma interpolation reacts differently to noise correlation.
    // Preserve its qualified affine reference when compatible with the data;
    // the shared model remains available for the newly supported discontinuities.
    let initial = (grid.cfa.period == 6 && target.split == 0).then_some(Model {
        plane: target.plane[anchor],
        jump: 0.,
        split: 0,
        split2: 0,
        side: SIDE as u8,
        border: 0,
        correction: [0.; 2],
    });
    let model = edge::refine(
        grid.values,
        grid.state,
        grid.stride,
        [x0, y0, SIDE],
        grid.cfa.shifted(x0, y0),
        ratio,
        initial,
    )?;
    Some(Reconstruction {
        model,
        ratio,
        extended: false,
    })
}

// A supported island inside an unsupported clipped region creates a false
// intensity edge. Admit a connected clipped region only when every cell has
// evidence, independently for each channel. Eight-neighbour connectivity is
// deliberately conservative at diagonals and object boundaries.
fn complete_regions(
    cells: &[Cell],
    estimates: &[Estimate],
    columns: usize,
    rows: usize,
) -> Vec<u8> {
    let mut allowed: Vec<_> = cells
        .iter()
        .zip(estimates)
        .map(|(c, e)| if e.is_some() { c.clipped } else { 0 })
        .collect();
    let mut visited = vec![0u8; cells.len()];
    let mut queue = Vec::with_capacity(cells.len());
    for channel in 0..3 {
        let bit = 1 << channel;
        for start in 0..cells.len() {
            if cells[start].clipped & bit == 0 || visited[start] & bit != 0 {
                continue;
            }
            queue.clear();
            queue.push(start);
            visited[start] |= bit;
            let mut complete = true;
            let mut needs_edge = false;
            let mut cursor = 0;
            while cursor < queue.len() {
                let i = queue[cursor];
                cursor += 1;
                complete &= allowed[i] & bit != 0;
                // This component could not be recovered by the affine policy:
                // at least one clipped cell had no valid affine anchor at all.
                // Keep components already supported by that policy intact.
                needs_edge |= cells[i].affine_valid == 0 && cells[i].valid != 0;
                let (x, y) = (i % columns, i / columns);
                for yy in y.saturating_sub(1)..=(y + 1).min(rows - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(columns - 1) {
                        let j = yy * columns + xx;
                        if cells[j].clipped & bit != 0 && visited[j] & bit == 0 {
                            visited[j] |= bit;
                            queue.push(j);
                        }
                    }
                }
            }
            if !complete {
                for &i in &queue {
                    allowed[i] &= !bit;
                }
            } else if needs_edge {
                for &i in &queue {
                    allowed[i] |= bit << 3;
                }
            }
        }
    }
    if !estimates.iter().flatten().any(|e| e.extended) {
        return allowed;
    }
    // Extending a region must not recover only one clipped colour while a
    // connected colour remains unsupported. Such partial progress can improve
    // RGB energy yet rotate hue. Preserve the old admission for old regions;
    // require joint completeness for every region using the new continuation.
    for start in 0..cells.len() {
        if cells[start].clipped == 0 || visited[start] & 128 != 0 {
            continue;
        }
        queue.clear();
        queue.push(start);
        visited[start] |= 128;
        let (mut complete, mut extended, mut cursor) = (true, false, 0);
        while cursor < queue.len() {
            let i = queue[cursor];
            cursor += 1;
            complete &= allowed[i] & 7 == cells[i].clipped;
            extended |= estimates[i].is_some_and(|e| e.extended);
            let (x, y) = (i % columns, i / columns);
            for yy in y.saturating_sub(1)..=(y + 1).min(rows - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(columns - 1) {
                    let j = yy * columns + xx;
                    if cells[j].clipped != 0 && visited[j] & 128 == 0 {
                        visited[j] |= 128;
                        queue.push(j);
                    }
                }
            }
        }
        if extended && !complete {
            for &i in &queue {
                allowed[i] = 0;
            }
        } else if extended {
            for &i in &queue {
                allowed[i] |= 64;
            }
        }
    }
    allowed
}

#[cfg(test)]
pub(crate) fn recover(
    values: &mut [f32],
    state: &[u8],
    stride: usize,
    active_width: usize,
    height: usize,
    cfa: Cfa,
) -> Evidence {
    recover_with_support(values, state, stride, active_width, height, cfa, None)
}

pub(crate) fn recover_with_support(
    values: &mut [f32],
    state: &[u8],
    stride: usize,
    active_width: usize,
    height: usize,
    cfa: Cfa,
    unsupported: Option<&mut Vec<u8>>,
) -> Evidence {
    assert_eq!(values.len(), stride * height);
    assert_eq!(values.len(), state.len());
    assert!(active_width <= stride);
    let mut evidence = Evidence {
        saturated: state.iter().filter(|&&s| s == SATURATED).count(),
        ..Evidence::default()
    };
    if evidence.saturated == 0 {
        return evidence;
    }
    if active_width < SIDE || height < SIDE {
        if let Some(unsupported) = unsupported {
            let columns = active_width.div_ceil(SIDE);
            unsupported.resize(columns * height.div_ceil(SIDE), 0);
            for y in 0..height {
                for x in 0..active_width {
                    if state[y * stride + x] == SATURATED {
                        unsupported[y / SIDE * columns + x / SIDE] |= 1 << cfa.color(x, y);
                    }
                }
            }
        }
        return evidence;
    }
    let (columns, rows) = (active_width.div_ceil(SIDE), height.div_ceil(SIDE));
    let designs: Vec<_> = (0..cfa.period * cfa.period)
        .map(|p| models(cfa.shifted(p % cfa.period, p / cfa.period)))
        .collect();
    let mut grid = Grid {
        data: vec![Cell::default(); columns * rows],
        values,
        state,
        stride,
        width: active_width,
        height,
        columns,
        cfa,
        designs,
    };
    for y in 0..height {
        for x in 0..active_width {
            if state[y * stride + x] == SATURATED {
                grid.data[(y / SIDE) * columns + x / SIDE].clipped |= 1 << cfa.color(x, y);
            }
        }
    }
    // An exact-size iterator reserves one estimate per cell. Flattening rows
    // loses that bound and Vec grows geometrically beyond scratch_bytes().
    let mut estimates: Vec<_> = (0..columns * rows)
        .map(|i| estimate(&mut grid, i % columns, i / columns))
        .collect();
    for i in 0..estimates.len() {
        if estimates[i].is_none() {
            estimates[i] = occlusion::estimate(&grid, &estimates, i % columns, i / columns);
        }
    }
    let allowed = complete_regions(&grid.data, &estimates, columns, rows);
    if let Some(unsupported) = unsupported {
        unsupported.clear();
        if grid
            .data
            .iter()
            .zip(&allowed)
            .any(|(cell, &mask)| cell.clipped & !mask != 0)
        {
            unsupported.extend(
                grid.data
                    .iter()
                    .zip(&allowed)
                    .map(|(cell, &mask)| cell.clipped & !mask),
            );
        }
    }
    // Preserve established affine components. Compute the extra boundary
    // support for piecewise components entirely from original measurements.
    for (i, estimate) in estimates.iter_mut().enumerate() {
        if allowed[i] & 64 != 0
            && let Some(r) = estimate
            && r.model.split == 0
            && !r.extended
        {
            // A newly admitted component uses the same measured support on
            // both sides of its cell boundaries. Fit estimates, never denoise
            // the surviving RAW samples or borrow inferred samples.
            if let Some(refitted) = occlusion::refit(&grid, i % columns, i / columns, *r) {
                *r = refitted;
            }
        }
        if allowed[i] & 56 != 0
            && let Some(r) = estimate
        {
            r.model = edge::with_halo(&grid, i % columns, i / columns, r.model, r.ratio);
        }
    }
    drop(grid);
    for ty in 0..rows {
        for tx in 0..columns {
            if allowed[ty * columns + tx] == 0 {
                continue;
            }
            let Some(Reconstruction { model, ratio, .. }) = estimates[ty * columns + tx] else {
                continue;
            };
            evidence.supported_cells += 1;
            let (x0, y0) = (
                (tx * SIDE).min(active_width - SIDE),
                (ty * SIDE).min(height - SIDE),
            );
            for y in ty * SIDE..((ty + 1) * SIDE).min(height) {
                for x in tx * SIDE..((tx + 1) * SIDE).min(active_width) {
                    let i = y * stride + x;
                    if state[i] != SATURATED {
                        continue;
                    }
                    let c = cfa.color(x, y);
                    if allowed[ty * columns + tx] & (1 << c) == 0 {
                        continue;
                    }
                    let local_model = if allowed[ty * columns + tx] & (1 << (c + 3)) != 0 {
                        model
                    } else {
                        Model { border: 0, ..model }
                    };
                    let predicted = local_model.predict(x - x0, y - y0) * ratio[c];
                    if predicted.is_finite()
                        && predicted > 1.0001 * values[i]
                        && predicted <= 4. * values[i]
                    {
                        values[i] = predicted;
                        evidence.reconstructed += 1;
                    }
                }
            }
        }
    }
    evidence
}
