//! Analytic zero fields. Independent bit emitter, no decoder imports. Expected
//! sensor samples are zero by induction: zero prediction and zero residual.
//! Format knowledge has the provenance recorded in the factual contract.

struct Writer {
    bytes: Vec<u8>,
    bits: usize,
}
impl Writer {
    fn push(&mut self, value: u32, n: usize) {
        for bit in (0..n).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let last = self.bytes.last_mut().unwrap();
            *last |= (((value >> bit) & 1) as u8) << (7 - self.bits % 8);
            self.bits += 1;
        }
    }
}

pub fn zero_payload(width: usize, groups: usize, quantizers: Option<&[u8]>) -> Vec<u8> {
    let stripes = width.div_ceil(768);
    let mut writer = Writer {
        bytes: vec![],
        bits: 0,
    };
    let initial = [(256u32, 1u32); 6];
    let mut main = initial;
    let mut smooth = initial;
    let mut previous = None;
    // Occupancy from the factual schedule, intentionally independent of the
    // candidate's CFA-derived masks and component ring implementation.
    let masks = [[0u8, 3], [3, 0], [2, 0], [3, 1], [1, 3], [0, 2]];
    for group in 0..groups {
        let q = quantizers.map_or(0, |v| v[group]);
        if previous != Some(q) {
            main = initial;
            previous = Some(q);
        }
        let states = if q == 0 { &mut main } else { &mut smooth };
        for (phase, mask) in masks.iter().enumerate() {
            let events = (0..4).map(|n| (2 * n, false)).chain((0..256).flat_map(|n| {
                let even = 8 + 2 * n;
                [(even, false), (1 + 2 * n, true)]
                    .into_iter()
                    .filter(|(x, _)| *x < 512)
            }));
            for (position, odd) in events {
                for m in mask {
                    if !odd && (m >> (position % 4 / 2)) & 1 == 0 {
                        continue;
                    }
                    let (sum, count) = &mut states[phase % 3 + if odd { 3 } else { 0 }];
                    let mut k = 0;
                    while (*count << k) < *sum {
                        k += 1;
                    }
                    writer.push(1, 1);
                    writer.push(0, k);
                    if *count == 64 {
                        *sum /= 2;
                        *count /= 2;
                    }
                    *count += 1;
                }
            }
        }
    }
    writer
        .bytes
        .resize(writer.bytes.len().next_multiple_of(16), 0);
    let mut bytes = vec![b'I', b'S', if quantizers.is_some() { 0 } else { 1 }, 16, 14];
    for value in [groups * 6, stripes * 768, width, 768] {
        bytes.extend((value as u16).to_be_bytes());
    }
    bytes.push(stripes as u8);
    bytes.extend((groups as u16).to_be_bytes());
    for _ in 0..stripes {
        bytes.extend((writer.bytes.len() as u32).to_be_bytes());
    }
    bytes.resize(16 + (stripes * 4).next_multiple_of(16), 0);
    if let Some(q) = quantizers {
        assert_eq!(q.len(), groups);
        for _ in 0..stripes {
            bytes.extend(q);
            bytes.resize(bytes.len() + groups.next_multiple_of(16) - groups, 0);
        }
    }
    for _ in 0..stripes {
        bytes.extend(&writer.bytes);
    }
    bytes
}
