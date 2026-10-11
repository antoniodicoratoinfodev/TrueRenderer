// SPDX-License-Identifier: CDDL-1.0
// Research candidate, NOT a clean-room or proprietary-origin claim.
// The format contract was learned from the attributed Fuji/LibRaw codec:
// Alexey Danilchenko (2016-2019), LibRaw adaptation by Alex Tutubalin.
// New streaming implementation: Antonio Dicorato / TrueRenderer, 2026.
// Retain third_party/fuji-codec/LICENSE.CDDL and provenance pending review.
// Laboratory only (unit tests and standalone tr-raw-lab); never selected by
// the production decoder.
use anyhow::{Context, Result, ensure};

const SENSOR_STRIPE: usize = 768;
const COMPONENT_WIDTH: usize = 512;
const LINE: usize = COMPONENT_WIDTH + 2;
const MAXIMUM: i32 = (1 << 14) - 1;

pub struct Codec<'a> {
    payloads: Vec<&'a [u8]>,
    steps: Option<&'a [u8]>,
    width: usize,
    groups: usize,
}

impl<'a> Codec<'a> {
    pub fn parse(bytes: &'a [u8], width: usize, height: usize) -> Result<Self> {
        let header = bytes.get(..16).context("Fuji candidate: short header")?;
        let stripes = width.div_ceil(SENSOR_STRIPE);
        ensure!(
            width >= SENSOR_STRIPE
                && width.is_multiple_of(24)
                && height >= 6
                && height.is_multiple_of(6)
                && stripes <= 16
                && width
                    .checked_mul(height)
                    .is_some_and(|n| n <= tr_core::color::MAX_PIXELS),
            "Fuji candidate: dimensions exceed contract"
        );
        let word = |i| usize::from(u16::from_be_bytes([header[i], header[i + 1]]));
        ensure!(
            &header[..2] == b"IS"
                && header[2] <= 1
                && header[3..5] == [16, 14]
                && word(5) == height
                && word(7) == stripes * SENSOR_STRIPE
                && word(9) == width
                && word(11) == SENSOR_STRIPE
                && usize::from(header[13]) == stripes
                && word(14) == height / 6,
            "Fuji candidate: unsupported header"
        );
        let sizes = bytes
            .get(16..16 + stripes * 4)
            .context("Fuji candidate: sizes")?;
        let groups = height / 6;
        let mut remaining = bytes
            .get(16 + (stripes * 4).next_multiple_of(16)..)
            .context("Fuji candidate: table padding")?;
        let steps = if header[2] == 0 {
            let len = groups.next_multiple_of(16) * stripes;
            ensure!(remaining.len() >= len, "Fuji candidate: quantizers");
            let (table, tail) = remaining.split_at(len);
            remaining = tail;
            Some(table)
        } else {
            None
        };
        let mut payloads = Vec::with_capacity(stripes);
        for bytes in sizes.as_chunks::<4>().0 {
            let len = u32::from_be_bytes(*bytes) as usize;
            ensure!(
                len > 0 && len <= SENSOR_STRIPE * height * 8 && len <= remaining.len(),
                "Fuji candidate: invalid stripe extent"
            );
            let (stripe, tail) = remaining.split_at(len);
            payloads.push(stripe);
            remaining = tail;
        }
        ensure!(remaining.is_empty(), "Fuji candidate: trailing payload");
        Ok(Self {
            payloads,
            steps,
            width,
            groups,
        })
    }

    pub fn unpack(&self, cfa: [usize; 36]) -> Result<Vec<u16>> {
        ensure!(
            cfa == crate::cfa::XTRANS.map(usize::from),
            "Fuji candidate: CFA phase"
        );
        // Derive encoded even sites from the sensor lattice, instead of a
        // hardcoded 18-line decoder schedule. Odd sites always carry a code.
        let mut occupied = [[[false; 4]; 6]; 3];
        for y in 0..6 {
            for x in 0..6 {
                let color = cfa[y * 6 + x];
                let row = if color == 1 { y } else { y / 2 };
                occupied[color][row][[0, 1, 1, 2, 3, 3][x]] = true;
            }
        }
        let mut output = vec![0; self.width * self.groups * 6];
        for (stripe, payload) in self.payloads.iter().enumerate() {
            let mut reader = BitWindow::new(payload);
            let mut histories: [History; 3] = std::array::from_fn(|_| History::new());
            let mut models = Models::new();
            for group in 0..self.groups {
                let q = self.steps.map_or(0, |steps| {
                    steps[stripe * self.groups.next_multiple_of(16) + group]
                });
                models.begin_group(q);
                for phase in 0..6 {
                    let pair = if phase % 2 == 0 { [0, 1] } else { [1, 2] };
                    for color in pair {
                        histories[color].begin();
                    }
                    let masks = pair
                        .map(|color| occupied[color][if color == 1 { phase } else { phase / 2 }]);
                    let (lower, upper) = histories.split_at_mut(pair[1]);
                    let mut rows = [lower[pair[0]].writer(masks[0]), upper[0].writer(masks[1])];
                    // A four-even-site lead, followed by pairs (even,odd),
                    // then four odd sites. This is a bounded event stream.
                    for even in (0..8).step_by(2) {
                        reconstruct_pair::<false>(
                            &mut rows,
                            phase % 3,
                            even,
                            &mut models,
                            &mut reader,
                        )?;
                    }
                    for odd in (1..COMPONENT_WIDTH).step_by(2) {
                        let even = odd + 7;
                        if even < COMPONENT_WIDTH {
                            reconstruct_pair::<false>(
                                &mut rows,
                                phase % 3,
                                even,
                                &mut models,
                                &mut reader,
                            )?;
                        }
                        reconstruct_pair::<true>(
                            &mut rows,
                            3 + phase % 3,
                            odd,
                            &mut models,
                            &mut reader,
                        )?;
                    }
                    // Each component row can be scattered as soon as it is
                    // complete. Only two preceding rows survive for prediction.
                    for color in pair {
                        let sensor_y = if color == 1 { phase } else { phase / 2 * 2 };
                        let count = if color == 1 { 1 } else { 2 };
                        let row = histories[color].current();
                        for y in sensor_y..sensor_y + count {
                            let start = (group * 6 + y) * self.width + stripe * SENSOR_STRIPE;
                            let width = SENSOR_STRIPE.min(self.width - stripe * SENSOR_STRIPE);
                            for x in 0..width {
                                if cfa[y * 6 + x % 6] == color {
                                    let at = 4 * (x / 6) + [0, 1, 1, 2, 3, 3][x % 6];
                                    output[start + x] = row[at + 1];
                                }
                            }
                        }
                        histories[color].advance();
                    }
                }
            }
            ensure!(
                reader.remaining_bits().div_ceil(8) <= 16,
                "Fuji candidate: excessive stripe tail"
            );
        }
        Ok(output)
    }

    /// Includes owned image, row/model scratch and stripe slice table. Source
    /// bytes and caller copies are excluded, as in RawMemoryPlan. No concurrency.
    pub fn live_buffer_bound(&self) -> usize {
        self.width * self.groups * 6 * 2
            + 3 * 3 * LINE * size_of::<u16>()
            + size_of::<Models>()
            + size_of::<BitWindow<'_>>()
            + size_of::<[[[bool; 4]; 6]; 3]>()
            + size_of::<[usize; 36]>()
            + 3 * size_of::<History>()
            + 2 * size_of::<RowWriter<'_>>()
            + size_of::<Self>()
            + size_of::<Vec<u16>>()
            + self.payloads.capacity() * size_of::<&[u8]>()
    }
}

struct History {
    rows: Box<[[u16; LINE]; 3]>,
    slot: usize,
}
impl History {
    fn new() -> Self {
        Self {
            rows: Box::new([[0; LINE]; 3]),
            slot: 0,
        }
    }
    fn begin(&mut self) {
        let above = (self.slot + 2) % 3;
        // Every interior even is written before a dependent odd can read it.
        // Previous contents are dead; only the two boundary sites need setup.
        self.rows[self.slot][0] = self.rows[above][1];
        self.rows[self.slot][LINE - 1] = self.rows[above][LINE - 2];
    }
    fn current(&self) -> &[u16; LINE] {
        &self.rows[self.slot]
    }
    fn advance(&mut self) {
        self.slot = (self.slot + 1) % 3;
    }
    fn writer(&mut self, occupied: [bool; 4]) -> RowWriter<'_> {
        // Resolve ring rotation and disjoint borrowing once per row, not once
        // per sample. No unsafe indexing and no copying of completed rows.
        let [a, b, c] = self.rows.as_mut();
        let (row, above, older) = match self.slot {
            0 => (a, c, b),
            1 => (b, a, c),
            _ => (c, b, a),
        };
        // Ring slots advance 0,1,2: the previous slot is two positions ahead.
        RowWriter {
            row,
            above,
            older,
            occupied,
        }
    }
}

struct RowWriter<'a> {
    row: &'a mut [u16; LINE],
    above: &'a [u16; LINE],
    older: &'a [u16; LINE],
    occupied: [bool; 4],
}
impl RowWriter<'_> {
    #[inline(always)]
    fn prediction<const ODD: bool>(&self, x: usize) -> (i32, i32, i32) {
        let above = self.above;
        let b = i32::from(above[x + 1]);
        let c = i32::from(above[x]);
        let d = i32::from(above[x + 2]);
        if !ODD {
            let f = i32::from(self.older[x + 1]);
            let deviations = [(c - b).abs(), (d - b).abs(), (f - b).abs()];
            let pair = if deviations[0] > deviations[1] && deviations[0] > deviations[2] {
                f + d
            } else if deviations[1] > deviations[0] && deviations[1] > deviations[2] {
                f + c
            } else {
                c + d
            };
            ((pair + 2 * b) / 4, b - f, c - b)
        } else {
            let row = &self.row;
            let a = i32::from(row[x]);
            let h = i32::from(row[x + 2]);
            let extreme = (b > c && b > d) || (b < c && b < d);
            (
                if extreme {
                    (a + h + 2 * b) / 4
                } else {
                    (a + h) / 2
                },
                b - c,
                c - a,
            )
        }
    }
}

#[inline(always)]
fn reconstruct_pair<const ODD: bool>(
    rows: &mut [RowWriter<'_>; 2],
    context: usize,
    x: usize,
    models: &mut Models,
    reader: &mut BitWindow<'_>,
) -> Result<()> {
    for row in rows {
        let (predicted, u, v) = row.prediction::<ODD>(x);
        let sample = if ODD || row.occupied[x % 4] {
            models.decode(reader, context, predicted, u, v)?
        } else {
            predicted as u16
        };
        row.row[x + 1] = sample;
    }
    Ok(())
}

struct BitWindow<'a> {
    bytes: &'a [u8],
    next: usize,
    register: u64,
    valid: u32,
    consumed: usize,
}
impl<'a> BitWindow<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            next: 0,
            register: 0,
            valid: 0,
            consumed: 0,
        }
    }
    fn fill(&mut self) {
        while self.valid <= 56 && self.next < self.bytes.len() {
            self.register |= u64::from(self.bytes[self.next]) << (56 - self.valid);
            self.next += 1;
            self.valid += 8;
        }
    }
    fn discard(&mut self, n: u32) {
        self.register = self.register.checked_shl(n).unwrap_or(0);
        self.valid -= n;
        self.consumed += n as usize;
    }
    fn take(&mut self, n: u32) -> Result<u32> {
        ensure!(n <= 16, "Fuji candidate: bit width");
        if n == 0 {
            return Ok(0);
        }
        if self.valid < n {
            self.fill();
        }
        ensure!(self.valid >= n, "Fuji candidate: truncated residual");
        let value = (self.register >> (64 - n)) as u32;
        self.discard(n);
        Ok(value)
    }
    fn unary(&mut self) -> Result<u32> {
        let mut total = 0;
        loop {
            if self.valid == 0 {
                self.fill();
            }
            ensure!(self.valid > 0, "Fuji candidate: unterminated prefix");
            let zeros = self.register.leading_zeros().min(self.valid);
            total += zeros;
            ensure!(total <= 56, "Fuji candidate: prefix exceeds limit");
            self.discard(zeros);
            if self.valid > 0 {
                self.discard(1);
                return Ok(total);
            }
        }
    }
    fn remaining_bits(&self) -> usize {
        self.bytes.len() * 8 - self.consumed
    }
}

#[derive(Clone, Copy)]
struct Adaptation {
    sum: u32,
    count: u32,
}
struct Model {
    q: i32,
    cardinality: u32,
    escape_bits: u32,
    contexts: [[Adaptation; 41]; 6],
}
impl Model {
    fn new(q: u8) -> Self {
        let q = i32::from(q);
        let cardinality = ((MAXIMUM + 2 * q) / (2 * q + 1) + 1) as u32;
        Self {
            q,
            cardinality,
            escape_bits: 32 - (cardinality - 1).leading_zeros(),
            contexts: [[Adaptation {
                sum: 2.max((cardinality + 32) / 64),
                count: 1,
            }; 41]; 6],
        }
    }
    #[inline(always)]
    fn bucket(&self, v: i32) -> i32 {
        let a = v.abs();
        let magnitude = if a <= self.q {
            0
        } else if a < 3 * self.q + 18 {
            1
        } else if a < 5 * self.q + 67 {
            2
        } else if a < 7 * self.q + 276 {
            3
        } else {
            4
        };
        magnitude * v.signum()
    }
}
struct Models {
    main_q: Option<u8>,
    bank: [Model; 4],
}
impl Models {
    fn new() -> Self {
        Self {
            main_q: None,
            bank: [Model::new(0), Model::new(0), Model::new(1), Model::new(2)],
        }
    }
    fn begin_group(&mut self, q: u8) {
        if self.main_q != Some(q) {
            self.bank[0] = Model::new(q);
            self.main_q = Some(q);
        }
    }
    #[inline(always)]
    fn decode(
        &mut self,
        reader: &mut BitWindow<'_>,
        context: usize,
        predicted: i32,
        u: i32,
        v: i32,
    ) -> Result<u16> {
        let activity = u.abs() + v.abs();
        let main_q = self.bank[0].q;
        let secondary = if main_q >= 1 && activity <= 5 {
            1
        } else if main_q >= 2 && activity <= 6 {
            2
        } else if main_q >= 3 && activity <= 7 {
            3
        } else {
            0
        };
        let model = &mut self.bank[secondary];
        let signed_context =
            (if secondary == 0 { 9 } else { 3 }) * model.bucket(u) + model.bucket(v);
        let state = &mut model.contexts[context][signed_context.unsigned_abs() as usize];
        let unary = reader.unary()?;
        let symbol = if unary >= 55 - model.escape_bits {
            reader.take(model.escape_bits)? + 1
        } else {
            let mut k = 0;
            if state.sum > state.count {
                k = state.count.leading_zeros() - state.sum.leading_zeros();
                if state.count << k < state.sum {
                    k += 1;
                }
            }
            ensure!(k <= 14, "Fuji candidate: adaptive width");
            (unary << k) | reader.take(k)?
        };
        ensure!(
            symbol < model.cardinality,
            "Fuji candidate: residual domain"
        );
        let magnitude = symbol.div_ceil(2);
        let delta = if symbol & 1 == 0 {
            magnitude as i32
        } else {
            -(magnitude as i32)
        };
        state.sum += magnitude;
        if state.count == 64 {
            state.sum /= 2;
            state.count /= 2;
        }
        state.count += 1;
        let step = 2 * model.q + 1;
        let mut reconstructed = predicted
            + if signed_context < 0 {
                -delta * step
            } else {
                delta * step
            };
        let period = model.cardinality as i32 * step;
        if reconstructed < -model.q {
            reconstructed += period;
        } else if reconstructed > MAXIMUM + model.q {
            reconstructed -= period;
        }
        Ok(reconstructed.clamp(0, MAXIMUM) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn codeword(zeros: usize, suffix: u32, bits: usize) -> Vec<u8> {
        let mut stream = vec![false; zeros];
        stream.push(true);
        stream.extend((0..bits).rev().map(|i| suffix & (1 << i) != 0));
        let mut bytes = vec![0; stream.len().div_ceil(8)];
        for (i, value) in stream.into_iter().enumerate() {
            bytes[i / 8] |= u8::from(value) << (7 - i % 8);
        }
        bytes
    }
    #[test]
    fn hand_solved_residual_sign_wrap_escape_and_quantization_boundaries() {
        // q=0: initial k=8; prefix 1 + eight-bit zigzag value.
        for (symbol, predicted, gradient, expected) in [
            (2, 100, 0, 101),
            (2, 100, -30, 99),
            (1, 0, 0, 16383),
            (2, 16383, 0, 0),
        ] {
            let bytes = codeword(0, symbol, 8);
            let mut models = Models::new();
            models.begin_group(0);
            assert_eq!(
                models
                    .decode(&mut BitWindow::new(&bytes), 0, predicted, gradient, 0)
                    .unwrap(),
                expected
            );
        }
        // Lossless escape starts at z=41, L=14. The largest valid zigzag
        // symbol is 16383; escape suffix 16383 instead means invalid 16384.
        for zeros in [41, 56] {
            let bytes = codeword(zeros, 16382, 14);
            let mut models = Models::new();
            models.begin_group(0);
            assert_eq!(
                models
                    .decode(&mut BitWindow::new(&bytes), 0, 8191, 0, 0)
                    .unwrap(),
                16383
            );
        }
        for (zeros, suffix) in [(41, 16383), (57, 0)] {
            let bytes = codeword(zeros, suffix, 14);
            assert!(
                Models::new()
                    .decode(&mut BitWindow::new(&bytes), 0, 0, 0, 0)
                    .is_err()
            );
        }
        // q=1: S=3,T=5462,L=13. Activity 50 selects the main model.
        // These straddle -q and M+q; saturating before wrapping is wrong.
        for (symbol, predicted, expected) in
            [(1, 0, 16383), (1, 1, 16383), (1, 2, 0), (2, 16383, 0)]
        {
            let bytes = codeword(42, symbol - 1, 13);
            let mut models = Models::new();
            models.begin_group(1);
            assert_eq!(
                models
                    .decode(&mut BitWindow::new(&bytes), 0, predicted, 50, 0)
                    .unwrap(),
                expected
            );
        }
        let mut models = Models::new();
        models.begin_group(1);
        // Smooth context chooses secondary q=0, with k=8, and persists across
        // a main-q change; the main state is reset independently.
        let bytes = codeword(0, 2, 8);
        assert_eq!(
            models
                .decode(&mut BitWindow::new(&bytes), 0, 100, 0, 0)
                .unwrap(),
            101
        );
        assert_eq!(models.bank[1].contexts[0][0].count, 2);
        models.begin_group(255);
        assert_eq!(models.bank[1].contexts[0][0].count, 2);
        assert_eq!(models.bank[0].contexts[0][0].count, 1);
    }
    #[test]
    fn bit_window_matches_bit_definition_at_every_boundary() {
        let bytes: Vec<_> = (0..47).map(|i| (i * 197 + 13) as u8).collect();
        for offset in 0..bytes.len() * 8 {
            for n in 0..=16 {
                let mut b = BitWindow::new(&bytes);
                for _ in 0..offset {
                    b.take(1).unwrap();
                }
                if offset + n > bytes.len() * 8 {
                    assert!(b.take(n as u32).is_err());
                    continue;
                }
                let expected = (offset..offset + n).fold(0, |v, i| {
                    v * 2 + u32::from((bytes[i / 8] >> (7 - i % 8)) & 1)
                });
                assert_eq!(b.take(n as u32).unwrap(), expected);
                assert_eq!(b.remaining_bits(), bytes.len() * 8 - offset - n);
            }
        }
        for zeros in 0..=64 {
            let mut bytes = [0; 9];
            bytes[zeros / 8] |= 1 << (7 - zeros % 8);
            let result = BitWindow::new(&bytes).unary();
            if zeros <= 56 {
                assert_eq!(result.unwrap(), zeros as u32);
            } else {
                assert!(result.is_err());
            }
        }
        assert!(BitWindow::new(&[0; 7]).unary().is_err());
    }
}
