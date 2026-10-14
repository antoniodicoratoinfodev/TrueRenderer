//! Rejected HL1 research probe, compiled only for tests.
//! Explicit neutral estimate for sensor sites at the declared white level.
//! The mask is computed before WB and interpolation, never from output RGB.
//! This cannot recover the original colour or texture of clipped measurements.
use crate::cfa::Cfa;

struct Neighbor {
    dx: isize,
    dy: isize,
    weight: f32,
}

pub(crate) struct Map {
    clipped: Vec<u8>,
    width: usize,
    height: usize,
    cfa: Cfa,
    neighbors: Vec<[Vec<Neighbor>; 3]>,
    tiles: Vec<bool>,
    columns: usize,
}

impl Map {
    pub fn new(clipped: Vec<u8>, width: usize, height: usize, cfa: Cfa) -> Option<Self> {
        assert_eq!(clipped.len(), width * height);
        if !clipped.contains(&1) {
            return None;
        }
        let columns = width.div_ceil(32);
        let mut tiles = vec![false; columns * height.div_ceil(32)];
        for (i, &value) in clipped.iter().enumerate() {
            if value == 0 {
                continue;
            }
            let (x, y) = (i % width, i / width);
            for ty in y.saturating_sub(3) / 32..=(y + 3).min(height - 1) / 32 {
                for tx in x.saturating_sub(3) / 32..=(x + 3).min(width - 1) / 32 {
                    tiles[ty * columns + tx] = true;
                }
            }
        }
        let neighbors = (0..cfa.period * cfa.period)
            .map(|phase| {
                std::array::from_fn(|channel| {
                    let x = phase % cfa.period;
                    let y = phase / cfa.period;
                    if cfa.color(x, y) == channel {
                        return vec![Neighbor {
                            dx: 0,
                            dy: 0,
                            weight: 1.,
                        }];
                    }
                    let radius: isize = if cfa.period == 2 { 1 } else { 3 };
                    let mut entries = vec![];
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            let distance = dx * dx + dy * dy;
                            if distance == 0 || distance > 10 {
                                continue;
                            }
                            let xx = (x as isize + dx + 6) as usize;
                            let yy = (y as isize + dy + 6) as usize;
                            if cfa.color(xx, yy) == channel {
                                entries.push(Neighbor {
                                    dx,
                                    dy,
                                    weight: 1. / distance as f32,
                                });
                            }
                        }
                    }
                    entries
                })
            })
            .collect();
        Some(Self {
            clipped,
            width,
            height,
            cfa,
            neighbors,
            tiles,
            columns,
        })
    }

    pub fn apply(&self, mut rgb: [f32; 3], x: usize, y: usize) -> [f32; 3] {
        if !self.tiles[(y / 32) * self.columns + x / 32] {
            return rgb;
        }
        let phase = (y % self.cfa.period) * self.cfa.period + x % self.cfa.period;
        let peak = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
        for (channel, v) in rgb.iter_mut().enumerate() {
            let (mut clipped, mut weight) = (0., 0.);
            for n in &self.neighbors[phase][channel] {
                let mut xx = x as isize + n.dx;
                let mut yy = y as isize + n.dy;
                if self.cfa.period == 2 {
                    // Reflect without repeating the outer site, keeping Bayer parity.
                    let reflect = |i: isize, len: usize| {
                        let period = 2 * (len as isize - 1);
                        let i = i.rem_euclid(period);
                        if i < len as isize { i } else { period - i }
                    };
                    xx = reflect(xx, self.width);
                    yy = reflect(yy, self.height);
                } else if xx < 0
                    || yy < 0
                    || xx >= self.width as isize
                    || yy >= self.height as isize
                {
                    continue;
                }
                weight += n.weight;
                clipped +=
                    n.weight * f32::from(self.clipped[yy as usize * self.width + xx as usize]);
            }
            if clipped > 0. {
                *v += (clipped / weight).min(1.) * (peak - *v);
            }
        }
        rgb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_channels_with_sensor_evidence_change_in_every_phase_and_at_borders() {
        for base in [
            Cfa::bayer([0, 1, 1, 2]),
            Cfa::new(6, &crate::cfa::XTRANS).unwrap(),
        ] {
            for phase in 0..base.period * base.period {
                let cfa = base.shifted(phase % base.period, phase / base.period);
                let (w, h) = (23, 19);
                for clipped_channel in 0..3 {
                    let mask = (0..w * h)
                        .map(|i| u8::from(cfa.color(i % w, i / w) == clipped_channel))
                        .collect();
                    let map = Map::new(mask, w, h, cfa).unwrap();
                    for y in 0..h {
                        for x in 0..w {
                            let rgb = [-0.125, 2., 0.75];
                            let actual = map.apply(rgb, x, y);
                            for c in 0..3 {
                                assert_eq!(
                                    actual[c],
                                    if c == clipped_channel { 2. } else { rgb[c] }
                                );
                            }
                        }
                    }
                }
                assert!(Map::new(vec![0; w * h], w, h, cfa).is_none());
            }
        }
    }
    #[test]
    fn isolated_clipping_has_bounded_support_and_keeps_valid_measured_sites_exact() {
        for cfa in [
            Cfa::bayer([0, 1, 1, 2]),
            Cfa::new(6, &crate::cfa::XTRANS).unwrap(),
        ] {
            let (w, h) = (31, 29);
            let (sx, sy) = (15, 14);
            let c = cfa.color(sx, sy);
            let mut mask = vec![0; w * h];
            mask[sy * w + sx] = 1;
            let map = Map::new(mask, w, h, cfa).unwrap();
            let rgb = std::array::from_fn(|i| if i == c { -0. } else { 1.5 });
            for y in 0..h {
                for x in 0..w {
                    let actual = map.apply(rgb, x, y);
                    assert!(actual[c] >= 0. && actual[c] <= 1.5);
                    if x.abs_diff(sx) > 3
                        || y.abs_diff(sy) > 3
                        || (cfa.color(x, y) == c && (x, y) != (sx, sy))
                    {
                        assert_eq!(actual.map(f32::to_bits), rgb.map(f32::to_bits));
                    }
                }
            }
        }
    }
}
