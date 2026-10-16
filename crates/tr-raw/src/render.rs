//! Separate realization of TrueRenderer's directional green / colour-difference
//! construction. Measured sites are preserved. Crop follows interpolation.
use crate::{cfa::Cfa, color::Matrix};
use anyhow::Result;
use tr_core::color::LinearImage;

/// Six-site cells whose clipped colours lack an admitted highlight model.
/// A clipped code is an inequality, not a measured level for edge fitting.
/// Low three bits: unsupported colours. Bit 3: admitted Bayer texture recovery.
#[derive(Clone, Copy, Default)]
pub(crate) struct Support<'a> {
    cells: &'a [u8],
    columns: usize,
    origin: [usize; 2],
    original_clipping: &'a [u8],
    stride: usize,
    fallback: Option<&'a crate::highlight_render::Fallback>,
}
impl<'a> Support<'a> {
    pub(crate) fn new(cells: &'a [u8], columns: usize) -> Self {
        Self {
            cells,
            columns,
            origin: [0, 0],
            original_clipping: &[],
            stride: 0,
            fallback: None,
        }
    }
    pub(crate) fn with_highlight_fallback(
        mut self,
        fallback: &'a crate::highlight_render::Fallback,
    ) -> Self {
        self.fallback = Some(fallback);
        self
    }
    pub(crate) fn render_highlights(self, rgb: [f32; 3], x: usize, y: usize) -> [f32; 3] {
        self.fallback.map_or(rgb, |fallback| {
            fallback.apply(rgb, x + self.origin[0], y + self.origin[1])
        })
    }
    pub(crate) fn with_original_clipping(mut self, bits: &'a [u8], stride: usize) -> Self {
        self.original_clipping = bits;
        self.stride = stride;
        self
    }
    pub(crate) fn measured(self, x: usize, y: usize) -> bool {
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
    pub(crate) fn unsupported_colour(self, x: usize, y: usize, c: usize) -> bool {
        if self.cells.is_empty() {
            return false;
        }
        let (x, y) = (x + self.origin[0], y + self.origin[1]);
        self.cells[y / 6 * self.columns + x / 6] & (1 << c) != 0
    }
    pub(crate) fn texture(self, x: usize, y: usize) -> bool {
        if self.cells.is_empty() {
            return false;
        }
        let (x, y) = (x + self.origin[0], y + self.origin[1]);
        self.cells[y / 6 * self.columns + x / 6] & 8 != 0
    }
    pub(crate) fn allows(self, x: usize, y: usize) -> bool {
        if self.cells.is_empty() {
            return true;
        }
        let (x, y) = (x + self.origin[0], y + self.origin[1]);
        let rows = self.cells.len() / self.columns;
        (y.saturating_sub(6) / 6..=((y + 6) / 6).min(rows - 1)).all(|yy| {
            (x.saturating_sub(6) / 6..=((x + 6) / 6).min(self.columns - 1))
                .all(|xx| self.cells[yy * self.columns + xx] & 7 == 0)
        })
    }
}

#[cfg(test)]
#[path = "../research/bayer_dir2.rs"]
pub(crate) mod dir2_reference;
#[cfg(test)]
#[path = "../research/bayer_frame_reference.rs"]
pub(crate) mod frame_reference;
#[cfg(test)]
#[path = "../research/bayer_streaming.rs"]
mod streaming_lab;

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
        + crate::edge_planes::RangeRows::reservation_bytes(width)
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
        return crate::xtrans::develop_supported(
            (mosaic, support),
            width,
            height,
            cfa,
            matrix,
            crop,
            orientation,
        );
    }
    let mut rows = BayerRows::new(width, height, cfa);
    let mut ranges = crate::edge_planes::RangeRows::new(width, height);
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    let has_texture = support.cells.iter().any(|&mask| mask & 8 != 0);
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
            let near_edge = crate::edge_planes::near_edge(mosaic, width, height, x, y, 2);
            let supported = near_edge && support.allows(x, y);
            if supported
                && let Some(model) = crate::edge_planes::interpolate_with_bounds(
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
            if near_edge
                && !supported
                && let Some(prediction) = crate::bayer_clipped_edges::interpolate(
                    (mosaic, support),
                    width,
                    height,
                    cfa,
                    (x, y),
                    ranges.at(x, y),
                    (rgb, matrix),
                )
            {
                for c in 0..3 {
                    if c != measured {
                        rgb[c] = prediction[c];
                    }
                }
            }
            // Texture interpolation is defined only at the physical margin.
            // Reject interior pixels before looking up the per-cell support.
            if has_texture
                && (x < 3 || x + 3 >= width || y < 3 || y + 3 >= height)
                && support.texture(x, y)
                && let Some(prediction) = crate::bayer_texture_edges::interpolate(
                    (mosaic, support),
                    width,
                    height,
                    cfa,
                    (x, y),
                    rgb,
                )
            {
                rgb = prediction;
            }
            let rec = crate::color::project(matrix, support.render_highlights(rgb, x, y));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_clipping_bits_keep_partial_bytes_stride_and_shifted_coordinates() {
        for length in 1usize..=41 {
            let state: Vec<_> = (0..length).map(|i| (i % 3) as u8).collect();
            let bits = original_clipping_bits(&state);
            assert!(bits.len() <= length.div_ceil(8));
            assert_eq!(bits.capacity(), bits.len());
            let support = Support::default().with_original_clipping(&bits, length);
            for (i, &s) in state.iter().enumerate() {
                assert_eq!(support.measured(i, 0), s != crate::highlights::SATURATED);
            }
        }
        let (w, h) = (19, 17);
        let mut state = vec![0; w * h];
        for x in 0..w {
            state[8 * w + x] = crate::highlights::SATURATED;
        }
        let bits = original_clipping_bits(&state);
        let support = Support::default().with_original_clipping(&bits, w);
        assert!(!support.axis_identified(8, 8, w, h, 1, 8));
        assert!(!support.axis_identified(8, 8, w, h, 1, 9));
        assert!(support.axis_identified(8, 8, w, h, 0, 8));
        for y in 4..h {
            for x in 3..w {
                assert_eq!(
                    support.measured(x, y),
                    support.shifted(3, 4).measured(x - 3, y - 4)
                );
            }
        }
    }

    #[test]
    fn identified_bayer_axis_edges_preserve_sites_and_reject_clipped_boundary_evidence() {
        let (w, h) = (31, 29);
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
            for axis in 0..2 {
                for cut in [7, 14, 15, 23] {
                    let scene = |x: usize, y: usize| {
                        if [x, y][axis] < cut {
                            [0.13, 0.41, 0.27]
                        } else {
                            [1.21, 0.73, 0.99]
                        }
                    };
                    let mosaic: Vec<_> = (0..w * h)
                        .map(|i| scene(i % w, i / w)[cfa.color(i % w, i / w)])
                        .collect();
                    let reference = dir2_reference::develop(
                        &mosaic,
                        w,
                        h,
                        cfa,
                        crate::color::ID,
                        [0, 0, w, h],
                        1,
                    )
                    .unwrap();
                    let actual =
                        develop(&mosaic, w, h, cfa, crate::color::ID, [0, 0, w, h], 1).unwrap();
                    for (i, pixel) in actual.pixels.iter().enumerate() {
                        assert!(pixel.iter().all(|v| v.is_finite()));
                        let c = cfa.color(i % w, i / w);
                        assert_eq!(pixel[c].to_bits(), mosaic[i].to_bits());
                    }
                    let mut old_error = 0f32;
                    for y in 3..h - 3 {
                        for x in 3..w - 3 {
                            let i = y * w + x;
                            let measured = cfa.color(x, y);
                            assert_eq!(actual.pixels[i][measured].to_bits(), mosaic[i].to_bits());
                            for c in 0..3 {
                                assert!((actual.pixels[i][c] - scene(x, y)[c]).abs() < 2e-6);
                                old_error =
                                    old_error.max((reference.pixels[i][c] - scene(x, y)[c]).abs());
                            }
                        }
                    }
                    assert!(old_error > 0.01);
                    let state: Vec<_> = (0..w * h)
                        .map(|i| {
                            if [i % w, i / w][axis] == cut {
                                crate::highlights::SATURATED
                            } else {
                                0
                            }
                        })
                        .collect();
                    let bits = original_clipping_bits(&state);
                    let blocked = develop_supported(
                        (&mosaic, Support::default().with_original_clipping(&bits, w)),
                        w,
                        h,
                        cfa,
                        crate::color::ID,
                        [0, 0, w, h],
                        1,
                    )
                    .unwrap();
                    assert_eq!(blocked.pixels, reference.pixels);
                }
            }
        }
    }

    #[test]
    fn unsupported_clipping_excludes_the_entire_edge_fit_support_across_tiles() {
        let (columns, rows) = (25, 24);
        let mut cells = vec![0; columns * rows];
        cells[21 * columns + 22] = 1;
        let support = Support::new(&cells, columns);
        for y in 0..rows * 6 {
            for x in 0..columns * 6 {
                let intersects = x + 6 >= 132
                    && x.saturating_sub(6) < 138
                    && y + 6 >= 126
                    && y.saturating_sub(6) < 132;
                assert_eq!(support.allows(x, y), !intersects);
                if x >= 119 && y >= 119 {
                    assert_eq!(
                        support.shifted(119, 119).allows(x - 119, y - 119),
                        !intersects
                    );
                }
            }
        }
    }
    #[test]
    fn directional_construction_matches_legacy_with_controlled_matrix() {
        let (w, h) = (17, 13);
        // Reconstruct camera RGB, then use the legacy colour conversion to
        // isolate the demosaic comparison from the new matrix precision.
        let mosaic: Vec<_> = (0..w * h)
            .map(|i| (i * 137 % 997) as f32 / 400. - 0.1)
            .collect();
        for cfa in [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]] {
            let old = tr_core::demosaic::develop(
                &mosaic,
                w as u32,
                h as u32,
                cfa.map(|c| c as u32),
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                0,
            )
            .unwrap();
            let camera = develop(
                &mosaic,
                w,
                h,
                Cfa::bayer(cfa),
                crate::color::ID,
                [0, 0, w, h],
                1,
            )
            .unwrap();
            for i in 0..w * h {
                let p = camera.pixels[i];
                let legacy_rgb = tr_core::color::linear_srgb_to_rec2020([p[0], p[1], p[2]]);
                assert_eq!(
                    old.pixels[i],
                    [legacy_rgb[0], legacy_rgb[1], legacy_rgb[2], 1.]
                );
                let c = cfa[(i / w % 2) * 2 + i % w % 2];
                assert_eq!(camera.pixels[i][c], mosaic[i]);
            }
        }
    }
}
