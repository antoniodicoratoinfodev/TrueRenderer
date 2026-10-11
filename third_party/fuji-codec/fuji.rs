// SPDX-License-Identifier: CDDL-1.0
// Copyright 2016-2019 Alexey Danilchenko.
// LibRaw adaptation by Alex Tutubalin, lexa@lexa.ru.
// Rust adaptation Copyright 2026 Antonio Dicorato / TrueRenderer.
// Derived from the documented Fuji bitstream and LibRaw 0.22.2's codec.
// This file is CDDL source, not covered by the proprietary project license.
// Changes: memory-only bounded Rust, X-Trans/14-bit subset, table-driven line
// schedule, unified residual reconstruction, no FFI, I/O or LibRaw dependency.
// See manifest.json and LICENSE.CDDL, distributed together with this source.
use anyhow::{Context, Result, ensure};

const STRIPE: usize = 768;
const N: usize = STRIPE * 2 / 3;
const STRIDE: usize = N + 2;
const MAX: i32 = 16383;

pub(crate) struct Codec<'a> {
    strips: Vec<&'a [u8]>,
    quantizers: Option<&'a [u8]>,
    width: usize,
    height: usize,
    lines: usize,
}

fn slice(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(at..at.checked_add(len).context("RAF: overflow codec")?)
        .context("RAF: codec troncato")
}

impl<'a> Codec<'a> {
    pub fn parse(bytes: &'a [u8], width: usize, height: usize) -> Result<Self> {
        let h = slice(bytes, 0, 16)?;
        let word = |i| u16::from_be_bytes([h[i], h[i + 1]]) as usize;
        let blocks = width.div_ceil(STRIPE);
        ensure!(
            width >= STRIPE
                && width.is_multiple_of(24)
                && height >= 6
                && height.is_multiple_of(6)
                && blocks <= 16
                && width
                    .checked_mul(height)
                    .is_some_and(|n| n <= tr_core::color::MAX_PIXELS),
            "RAF: dimensioni codec fuori quota"
        );
        ensure!(
            &h[..2] == b"IS"
                && h[2] <= 1
                && h[3] == 16
                && h[4] == 14
                && word(5) == height
                && word(7) == blocks * STRIPE
                && word(9) == width
                && word(11) == STRIPE
                && usize::from(h[13]) == blocks
                && word(14) == height / 6,
            "RAF: modalità codec non supportata"
        );
        let lines = height / 6;
        let table = slice(bytes, 16, blocks * 4)?;
        let mut at = 16 + (blocks * 4).next_multiple_of(16);
        let quantizers = if h[2] == 0 {
            let q = slice(bytes, at, blocks * lines.next_multiple_of(16))?;
            at += q.len();
            Some(q)
        } else {
            None
        };
        let mut strips = Vec::with_capacity(blocks);
        for chunk in table.as_chunks::<4>().0 {
            let len = u32::from_be_bytes(*chunk) as usize;
            ensure!(
                len > 0 && len <= STRIPE * height * 8,
                "RAF: strip fuori quota"
            );
            strips.push(slice(bytes, at, len)?);
            at += len;
        }
        ensure!(at == bytes.len(), "RAF: lunghezze strip incoerenti");
        Ok(Self {
            strips,
            quantizers,
            width,
            height,
            lines,
        })
    }
    pub fn lossy(&self) -> bool {
        self.quantizers.is_some()
    }
    pub fn unpack(&self, pattern: [usize; 36]) -> Result<Vec<u16>> {
        let mut out = vec![0; self.width * self.height];
        for (stripe, bytes) in self.strips.iter().enumerate() {
            let mut bits = Bits { bytes, at: 0 };
            let mut rows = vec![0i32; 18 * STRIDE];
            let mut coding = Coding::new();
            let mut last_q = None;
            for group in 0..self.lines {
                let q = self
                    .quantizers
                    .map_or(0, |q| q[stripe * self.lines.next_multiple_of(16) + group]);
                if last_q != Some(q) {
                    coding.reset_main(i32::from(q));
                    last_q = Some(q);
                }
                for (stage, pair) in [[2, 7], [8, 15], [3, 9], [10, 16], [4, 11], [12, 17]]
                    .into_iter()
                    .enumerate()
                {
                    let (mut even, mut odd) = (0, 1);
                    while even < N || odd < N {
                        if even < N {
                            for (component, row) in pair.into_iter().enumerate() {
                                let encoded = match (stage, component) {
                                    (0, 0) | (1, 1) | (2, 1) | (5, 0) => false,
                                    (2, 0) | (5, 1) => even % 4 == 2,
                                    (3, 1) | (4, 0) => even % 4 == 0,
                                    _ => true,
                                };
                                sample(
                                    &mut rows,
                                    row,
                                    even,
                                    encoded,
                                    stage % 3,
                                    &mut coding,
                                    &mut bits,
                                )?;
                            }
                            even += 2;
                        }
                        if even > 8 && odd < N {
                            for row in pair {
                                sample(
                                    &mut rows,
                                    row,
                                    odd,
                                    true,
                                    3 + stage % 3,
                                    &mut coding,
                                    &mut bits,
                                )?;
                            }
                            odd += 2;
                        }
                    }
                    for row in pair {
                        let range = if row < 5 {
                            2..5
                        } else if row < 13 {
                            7..13
                        } else {
                            15..18
                        };
                        for r in range {
                            rows[r * STRIDE] = rows[(r - 1) * STRIDE + 1];
                            rows[r * STRIDE + N + 1] = rows[(r - 1) * STRIDE + N];
                        }
                    }
                }
                project(&rows, &mut out, self.width, stripe, group, pattern);
                for (start, count) in [(0, 3), (5, 6), (13, 3)] {
                    rows.copy_within(
                        (start + count) * STRIDE..(start + count + 2) * STRIDE,
                        start * STRIDE,
                    );
                    rows[(start + 2) * STRIDE..(start + 2 + count) * STRIDE].fill(0);
                    rows[(start + 2) * STRIDE] = rows[(start + 1) * STRIDE + 1];
                    rows[(start + 2) * STRIDE + N + 1] = rows[(start + 1) * STRIDE + N];
                }
            }
            // Camera strips are padded to a 16-byte boundary. No reads are
            // permitted past a strip, including on truncated unary codes.
            ensure!(
                (bytes.len() * 8 - bits.at).div_ceil(8) <= 16,
                "RAF: coda codec inattesa"
            );
        }
        Ok(out)
    }
}

fn project(
    rows: &[i32],
    out: &mut [u16],
    width: usize,
    stripe: usize,
    group: usize,
    pattern: [usize; 36],
) {
    let count = STRIPE.min(width - stripe * STRIPE);
    for y in 0..6 {
        for x in 0..count {
            let color = pattern[y * 6 + x % 6];
            let row = match color {
                0 => 2 + y / 2,
                1 => 7 + y,
                2 => 15 + y / 2,
                _ => unreachable!(),
            };
            let pos = x / 6 * 4 + [0, 1, 1, 2, 3, 3][x % 6];
            out[(group * 6 + y) * width + stripe * STRIPE + x] =
                rows[row * STRIDE + pos + 1] as u16;
        }
    }
}

struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn read(&mut self, n: usize) -> Result<i32> {
        ensure!(
            n <= 16 && self.at + n <= self.bytes.len() * 8,
            "RAF: residuo troncato"
        );
        let word = (0..3).fold(0u32, |w, i| {
            (w << 8) | u32::from(self.bytes.get(self.at / 8 + i).copied().unwrap_or(0))
        });
        let value = ((word << (self.at % 8)) & 0xffffff) >> (24 - n);
        self.at += n;
        Ok(value as i32)
    }
}
#[derive(Clone, Copy)]
struct Stats {
    sum: i32,
    count: i32,
}
#[derive(Clone, Copy)]
struct Quant {
    q: i32,
    total: i32,
    bits: usize,
    thresholds: [i32; 3],
    multiplier: i32,
}
impl Quant {
    fn new(q: i32, multiplier: i32) -> Self {
        let total = (MAX + 2 * q) / (2 * q + 1) + 1;
        Self {
            q,
            total,
            bits: (32 - (total - 1).leading_zeros()) as usize,
            thresholds: [3 * q + 18, 5 * q + 67, 7 * q + 276],
            multiplier,
        }
    }
    fn bucket(self, value: i32) -> i32 {
        let magnitude = value.abs();
        let b = if magnitude <= self.q {
            0
        } else if magnitude < self.thresholds[0] {
            1
        } else if magnitude < self.thresholds[1] {
            2
        } else if magnitude < self.thresholds[2] {
            3
        } else {
            4
        };
        b * value.signum()
    }
}
struct Coding {
    tables: [Quant; 4],
    stats: [[[Stats; 41]; 6]; 4],
}
impl Coding {
    fn new() -> Self {
        let tables = [
            Quant::new(0, 9),
            Quant::new(0, 3),
            Quant::new(1, 3),
            Quant::new(2, 3),
        ];
        let stats = tables.map(|q| {
            [[Stats {
                sum: 2.max((q.total + 32) >> 6),
                count: 1,
            }; 41]; 6]
        });
        Self { tables, stats }
    }
    fn reset_main(&mut self, q: i32) {
        self.tables[0] = Quant::new(q, 9);
        self.stats[0] = [[Stats {
            sum: 2.max((self.tables[0].total + 32) >> 6),
            count: 1,
        }; 41]; 6];
    }
    fn residual(
        &mut self,
        bits: &mut Bits<'_>,
        context: usize,
        a: i32,
        b: i32,
        predictor: i32,
    ) -> Result<i32> {
        let table = (1..=3)
            .find(|i| self.tables[0].q >= *i as i32 && a.abs() + b.abs() <= *i as i32 + 4)
            .unwrap_or(0);
        let q = self.tables[table];
        let gradient = q.multiplier * q.bucket(a) + q.bucket(b);
        let s = &mut self.stats[table][context][gradient.unsigned_abs() as usize];
        let mut zeros = 0;
        while bits.read(1)? == 0 {
            zeros += 1;
            ensure!(zeros <= 56, "RAF: prefisso residuo fuori quota");
        }
        let code = if zeros < 56 - q.bits - 1 {
            let mut n = 0;
            while (s.count << n) < s.sum {
                n += 1;
            }
            ensure!(n <= 14, "RAF: stato adattivo invalido");
            bits.read(n)? + ((zeros as i32) << n)
        } else {
            bits.read(q.bits)? + 1
        };
        ensure!(code < q.total, "RAF: residuo fuori dominio");
        let delta = if code & 1 == 0 {
            code / 2
        } else {
            -1 - code / 2
        };
        s.sum += delta.abs();
        if s.count == 64 {
            s.sum >>= 1;
            s.count >>= 1;
        }
        s.count += 1;
        let mut value = predictor + if gradient < 0 { -delta } else { delta } * (2 * q.q + 1);
        let range = q.total * (2 * q.q + 1);
        if value < -q.q {
            value += range;
        } else if value > MAX + q.q {
            value -= range;
        }
        // This clamp is part of the lossy sensor codec, not the fp32 pipeline.
        Ok(value.clamp(0, MAX))
    }
}
fn sample(
    rows: &mut [i32],
    row: usize,
    pos: usize,
    encoded: bool,
    context: usize,
    coding: &mut Coding,
    bits: &mut Bits<'_>,
) -> Result<()> {
    let i = row * STRIDE + pos + 1;
    let (b, c, d) = (rows[i - STRIDE], rows[i - STRIDE - 1], rows[i - STRIDE + 1]);
    let (predictor, a, e) = if pos.is_multiple_of(2) {
        let f = rows[i - 2 * STRIDE];
        let (dc, df, dd) = ((c - b).abs(), (f - b).abs(), (d - b).abs());
        let p = if dc > df && dc > dd {
            f + d + 2 * b
        } else if dd > dc && dd > df {
            f + c + 2 * b
        } else {
            d + c + 2 * b
        };
        (p >> 2, b - f, c - b)
    } else {
        let (a, g) = (rows[i - 1], rows[i + 1]);
        let p = if (b > c && b > d) || (b < c && b < d) {
            (a + g + 2 * b) >> 2
        } else {
            (a + g) >> 1
        };
        (p, b - c, c - a)
    };
    rows[i] = if encoded {
        coding.residual(bits, context, a, e, predictor)?
    } else {
        predictor
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plane_projection_does_not_duplicate_green_column_four() {
        let rows: Vec<_> = (0..18)
            .flat_map(|r| (0..STRIDE).map(move |x| r * 100 + x as i32 - 1))
            .collect();
        let mut out = vec![0; 36];
        let pattern = crate::cfa::XTRANS.map(usize::from);
        project(&rows, &mut out, 6, 0, 0, pattern);
        assert_eq!(
            out,
            [
                700, 701, 201, 702, 703, 1503, 800, 801, 1501, 802, 803, 203, 1600, 301, 901, 302,
                1603, 903, 1000, 1001, 1601, 1002, 1003, 303, 1100, 1101, 401, 1102, 1103, 1703,
                400, 1701, 1201, 1702, 403, 1203
            ]
        );
    }
    #[test]
    fn bounded_headers_and_residuals() {
        for len in 0..16 {
            assert!(Codec::parse(&vec![0; len], 768, 6).is_err());
        }
        assert!(Codec::parse(&[0; 100], usize::MAX, 6).is_err());
        let mut coding = Coding::new();
        coding.reset_main(0);
        // Initial k=8, unary=0, zigzag=2 => +1.
        let mut bits = Bits {
            bytes: &[0x81, 0],
            at: 0,
        };
        assert_eq!(coding.residual(&mut bits, 0, 0, 0, 100).unwrap(), 101);
        // No terminator: deterministic error instead of reading the next strip.
        let mut bits = Bits {
            bytes: &[0; 16],
            at: 0,
        };
        assert!(coding.residual(&mut bits, 0, 0, 0, 100).is_err());
    }
}
