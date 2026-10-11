//! Periodic RGB sensor layouts; coordinates are relative to the active area.
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cfa {
    pub period: usize,
    pub map: [usize; 36],
}

// X-Trans phase at the full X-T5 sensor origin, from its RAF metadata.
pub(crate) const XTRANS: [u8; 36] = [
    1, 1, 0, 1, 1, 2, 1, 1, 2, 1, 1, 0, 2, 0, 1, 0, 2, 1, 1, 1, 2, 1, 1, 0, 1, 1, 0, 1, 1, 2, 0, 2,
    1, 2, 0, 1,
];

impl Cfa {
    pub fn bayer(map: [usize; 4]) -> Self {
        let mut cfa = Self {
            period: 2,
            map: [0; 36],
        };
        cfa.map[..4].copy_from_slice(&map);
        cfa
    }
    pub fn new(period: usize, pattern: &[u8]) -> Result<Self> {
        ensure!(
            matches!(period, 2 | 6) && pattern.len() == period * period,
            "Experimental: periodo CFA non supportato"
        );
        if period == 2 {
            ensure!(
                [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]]
                    .iter()
                    .any(|p| p == pattern),
                "Experimental: pattern CFA non Bayer RGB"
            );
        } else {
            // Admit only translated/reflected/transposed X-Trans, not an
            // arbitrary 6x6 array with an accidentally correct colour count.
            let valid = (0..8).any(|transform| {
                (0..6).any(|dy| {
                    (0..6).any(|dx| {
                        (0..36).all(|i| {
                            let (mut x, mut y) = (i % 6, i / 6);
                            if transform & 1 != 0 {
                                std::mem::swap(&mut x, &mut y);
                            }
                            if transform & 2 != 0 {
                                x = 5 - x;
                            }
                            if transform & 4 != 0 {
                                y = 5 - y;
                            }
                            pattern[i] == XTRANS[((y + dy) % 6) * 6 + (x + dx) % 6]
                        })
                    })
                })
            });
            ensure!(valid, "Experimental: pattern CFA non X-Trans RGB");
        }
        let mut map = [0; 36];
        for (out, value) in map.iter_mut().zip(pattern) {
            *out = usize::from(*value);
        }
        Ok(Self { period, map })
    }
    pub fn color(self, x: usize, y: usize) -> usize {
        self.map[(y % self.period) * self.period + x % self.period]
    }
    pub fn shifted(self, left: usize, top: usize) -> Self {
        let mut out = self;
        for y in 0..self.period {
            for x in 0..self.period {
                out.map[y * self.period + x] = self.color(x + left, y + top);
            }
        }
        out
    }
}
