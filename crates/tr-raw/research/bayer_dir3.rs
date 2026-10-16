//! Frozen dir3 from the qualified HL5 bundle, test-only comparator.
//! Separate realization of TrueRenderer's directional green / colour-difference
//! construction. Measured sites are preserved. Crop follows interpolation.
use crate::{cfa::Cfa, color::Matrix};
use anyhow::Result;
use tr_core::color::LinearImage;

/// Six-site cells whose clipped colours lack an admitted highlight model.
/// A clipped code is an inequality, not a measured level for edge fitting.
#[derive(Clone, Copy, Default)]
pub(crate) struct Support<'a> {
    cells: &'a [u8],
    columns: usize,
    origin: [usize; 2],
    original_clipping: &'a [u8],
    stride: usize,
}
impl<'a> Support<'a> {
    pub(crate) fn new(cells: &'a [u8], columns: usize) -> Self {
        Self {
            cells,
            columns,
            origin: [0, 0],
            original_clipping: &[],
            stride: 0,
        }
    }
    pub(crate) fn with_original_clipping(mut self, bits: &'a [u8], stride: usize) -> Self {
        self.original_clipping = bits;
        self.stride = stride;
        self
    }
    fn measured(self, x: usize, y: usize) -> bool {
        let i = (y + self.origin[1]) * self.stride + x + self.origin[0];
        self.original_clipping.is_empty() || self.original_clipping[i / 8] & (1 << (i % 8)) == 0
    }
    fn axis_identified(
        self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        axis: usize,
        cut: usize,
    ) -> bool {
        // The two adjacent rows/columns at the fitted boundary must each
        // contain an original measurement. Otherwise clipping can hide which
        // side an intervening CFA row/column belongs to.
        [cut - 1, cut].into_iter().all(|coordinate| {
            if axis == 0 {
                (y.saturating_sub(6)..=(y + 6).min(h - 1)).any(|yy| self.measured(coordinate, yy))
            } else {
                (x.saturating_sub(6)..=(x + 6).min(w - 1)).any(|xx| self.measured(xx, coordinate))
            }
        })
    }
    pub(crate) fn shifted(self, x: usize, y: usize) -> Self {
        Self {
            origin: [self.origin[0] + x, self.origin[1] + y],
            ..self
        }
    }
    pub(crate) fn allows(self, x: usize, y: usize) -> bool {
        if self.cells.is_empty() {
            return true;
        }
        let (x, y) = (x + self.origin[0], y + self.origin[1]);
        let rows = self.cells.len() / self.columns;
        (y.saturating_sub(6) / 6..=((y + 6) / 6).min(rows - 1)).all(|yy| {
            (x.saturating_sub(6) / 6..=((x + 6) / 6).min(self.columns - 1))
                .all(|xx| self.cells[yy * self.columns + xx] == 0)
        })
    }
}

fn reflect(i: isize, length: usize) -> usize {
    let period = 2 * (length as isize - 1);
    let i = i.rem_euclid(period);
    if i < length as isize {
        i as usize
    } else {
        (period - i) as usize
    }
}

pub(crate) fn oriented_size(w: u32, h: u32, orientation: u32) -> (u32, u32) {
    if orientation >= 5 { (h, w) } else { (w, h) }
}

/// One provenance bit per sensor site; no clipping is inferred after WB.
pub(crate) fn original_clipping_bits(state: &[u8]) -> Vec<u8> {
    if !state.contains(&crate::highlights::SATURATED) {
        return Vec::new();
    }
    state
        .chunks(8)
        .map(|chunk| {
            chunk.iter().enumerate().fold(0, |bits, (i, &s)| {
                bits | (u8::from(s == crate::highlights::SATURATED) << i)
            })
        })
        .collect()
}

pub(crate) fn bayer_scratch_reservation_bytes(width: usize, height: usize) -> usize {
    width * (3 * size_of::<f32>() + size_of::<[usize; 4]>())
        + crate::edge_planes_dir6::RangeRows::reservation_bytes(width)
        + (width * height).div_ceil(8)
        + 8 * 1024
}

struct BayerRows {
    width: usize,
    height: usize,
    pattern: [[usize; 2]; 2],
    columns: Vec<[usize; 4]>,
    green: [Vec<f32>; 3],
    row_ids: [Option<usize>; 3],
}

impl BayerRows {
    fn new(width: usize, height: usize, cfa: Cfa) -> Self {
        Self {
            width,
            height,
            pattern: std::array::from_fn(|y| std::array::from_fn(|x| cfa.color(x, y))),
            columns: (0..width)
                .map(|x| [-2, -1, 1, 2].map(|dx| reflect(x as isize + dx, width)))
                .collect(),
            green: std::array::from_fn(|_| vec![0.; width]),
            row_ids: [None; 3],
        }
    }

    fn prepare(&mut self, mosaic: &[f32], y: usize) {
        let slot = y % 3;
        if self.row_ids[slot] == Some(y) {
            return;
        }
        let width = self.width;
        let [u2, u1, d1, d2] =
            [-2, -1, 1, 2].map(|dy| reflect(y as isize + dy, self.height) * width);
        let row = y * width;
        for (x, &[l2, l1, r1, r2]) in self.columns.iter().enumerate() {
            let i = row + x;
            if self.pattern[y & 1][x & 1] == 1 {
                self.green[slot][x] = mosaic[i];
                continue;
            }
            let (l, r, u, d) = (
                mosaic[row + l1],
                mosaic[row + r1],
                mosaic[u1 + x],
                mosaic[d1 + x],
            );
            let dh = 2. * mosaic[i] - mosaic[row + l2] - mosaic[row + r2];
            let dv = 2. * mosaic[i] - mosaic[u2 + x] - mosaic[d2 + x];
            let gh = (l + r) * 0.5 + dh * 0.25;
            let gv = (u + d) * 0.5 + dv * 0.25;
            let horizontal = (l - r).abs() + dh.abs();
            let vertical = (u - d).abs() + dv.abs();
            self.green[slot][x] = if horizontal < vertical {
                gh
            } else if vertical < horizontal {
                gv
            } else {
                (gh + gv) * 0.5
            };
        }
        self.row_ids[slot] = Some(y);
    }

    fn row(&self, y: usize) -> &[f32] {
        debug_assert_eq!(self.row_ids[y % 3], Some(y));
        &self.green[y % 3]
    }
}

#[cfg(test)]
pub(crate) fn develop(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    develop_supported(
        (mosaic, Support::default()),
        width,
        height,
        cfa,
        matrix,
        crop,
        orientation,
    )
}

pub(crate) fn develop_supported(
    (mosaic, support): (&[f32], Support<'_>),
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    if cfa.period == 6 {
        return crate::xtrans_dir6::develop_supported(
            (
                mosaic,
                crate::render::Support::new(support.cells, support.columns)
                    .with_original_clipping(support.original_clipping, support.stride)
                    .shifted(support.origin[0], support.origin[1]),
            ),
            width,
            height,
            cfa,
            matrix,
            crop,
            orientation,
        );
    }
    let mut rows = BayerRows::new(width, height, cfa);
    let mut ranges = crate::edge_planes_dir6::RangeRows::new(width, height);
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    for cy in 0..h {
        let y = cy + top;
        ranges.prepare(mosaic, width, cfa, y);
        let (above, below) = (
            reflect(y as isize - 1, height),
            reflect(y as isize + 1, height),
        );
        for yy in [above, y, below] {
            rows.prepare(mosaic, yy);
        }
        let (up, green, down) = (rows.row(above), rows.row(y), rows.row(below));
        let (up_start, row_start, down_start) = (above * width, y * width, below * width);
        for cx in 0..w {
            let x = cx + left;
            let i = row_start + x;
            let [_, l, r, _] = rows.columns[x];
            let measured = rows.pattern[y & 1][x & 1];
            let mut rgb = [0., green[x], 0.];
            if measured == 1 {
                let horizontal = rows.pattern[y & 1][(x & 1) ^ 1];
                let dh: f32 = [
                    mosaic[row_start + l] - green[l],
                    mosaic[row_start + r] - green[r],
                ]
                .into_iter()
                .sum();
                let dv: f32 = [
                    mosaic[up_start + x] - up[x],
                    mosaic[down_start + x] - down[x],
                ]
                .into_iter()
                .sum();
                rgb[horizontal] = green[x] + dh / 2.;
                rgb[2 - horizontal] = green[x] + dv / 2.;
            } else {
                rgb[measured] = mosaic[i];
                let delta: f32 = [
                    mosaic[up_start + l] - up[l],
                    mosaic[up_start + r] - up[r],
                    mosaic[down_start + l] - down[l],
                    mosaic[down_start + r] - down[r],
                ]
                .into_iter()
                .sum();
                rgb[2 - measured] = green[x] + delta / 4.;
            }
            if crate::edge_planes_dir6::near_edge(mosaic, width, height, x, y, 2)
                && support.allows(x, y)
                && let Some(model) = crate::edge_planes_dir6::interpolate_with_bounds(
                    mosaic,
                    width,
                    height,
                    cfa,
                    (x, y),
                    ranges.at(x, y),
                    true,
                )
            {
                let bounds = ranges.at(x, y);
                let significant = model.bayer_axis.is_none()
                    || (0..3).any(|c| {
                        c != measured
                            && (model.rgb[c] - rgb[c]).abs()
                                > 0.001 * bounds[0][c].abs().max(bounds[1][c].abs()).max(0.01)
                    });
                let identified = model.bayer_axis.is_none_or(|(axis, cut)| {
                    support.axis_identified(x, y, width, height, axis, cut)
                });
                if identified && significant {
                    for (c, value) in rgb.iter_mut().enumerate() {
                        if c != measured {
                            *value = model.rgb[c];
                        }
                    }
                }
            }
            let rec = crate::color::project(matrix, rgb);
            let (ox, oy) = match orientation {
                2 => (w - 1 - cx, cy),
                3 => (w - 1 - cx, h - 1 - cy),
                4 => (cx, h - 1 - cy),
                5 => (cy, cx),
                6 => (h - 1 - cy, cx),
                7 => (h - 1 - cy, w - 1 - cx),
                8 => (cy, w - 1 - cx),
                _ => (cx, cy),
            };
            pixels[oy * ow as usize + ox] = [rec[0], rec[1], rec[2], 1.];
        }
    }
    LinearImage::new(ow, oh, pixels)
}
