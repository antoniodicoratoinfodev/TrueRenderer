//! Separate realization of TrueRenderer's directional green / colour-difference
//! construction. Measured sites are preserved. Crop follows interpolation.
use crate::{cfa::Cfa, color::Matrix};
use anyhow::Result;
use tr_core::color::LinearImage;

#[cfg(test)]
#[path = "../research/bayer_frame_reference.rs"]
mod frame_reference;
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

pub(crate) fn bayer_scratch_reservation_bytes(width: usize) -> usize {
    width * (3 * size_of::<f32>() + size_of::<[usize; 4]>()) + 1024
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

pub(crate) fn develop(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    matrix: Matrix,
    crop: [usize; 4],
    orientation: u32,
) -> Result<LinearImage> {
    if cfa.period == 6 {
        return crate::xtrans::develop(mosaic, width, height, cfa, matrix, crop, orientation);
    }
    let mut rows = BayerRows::new(width, height, cfa);
    let [left, top, w, h] = crop;
    let (ow, oh) = oriented_size(w as u32, h as u32, orientation);
    let mut pixels = vec![[0.; 4]; w * h];
    let m = matrix.map(|row| row.map(|v| v as f32));
    for cy in 0..h {
        let y = cy + top;
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
            let rec = m.map(|r| r[0] * rgb[0] + r[1] * rgb[1] + r[2] * rgb[2]);
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
    fn directional_construction_matches_legacy_with_controlled_matrix() {
        let (w, h) = (17, 13);
        // Same f32 operation order as the old two-matrix path with identity
        // camera->sRGB; compare all samples including negative/high headroom.
        let rec = [
            [0.627404, 0.329282, 0.0433136],
            [0.069097, 0.91954, 0.0113612],
            [0.0163916, 0.0880132, 0.895595],
        ];
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
            let new = develop(&mosaic, w, h, Cfa::bayer(cfa), rec, [0, 0, w, h], 1).unwrap();
            assert_eq!(old.pixels, new.pixels);
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
                let c = cfa[(i / w % 2) * 2 + i % w % 2];
                assert_eq!(camera.pixels[i][c], mosaic[i]);
            }
        }
    }
}
