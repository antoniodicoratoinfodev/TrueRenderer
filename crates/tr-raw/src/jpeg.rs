//! ISO/IEC 10918-1 lossless Huffman (SOF3), bounded to the declared DNG tile.
//! One interleaved scan, predictors 1..7, 1..4 components without subsampling.
use anyhow::{Context, Result, bail, ensure};

#[derive(Clone)]
struct Huffman {
    first: [u32; 17],
    count: [u32; 17],
    start: [usize; 17],
    symbols: Vec<u8>,
}
impl Huffman {
    fn parse(counts: &[u8], symbols: &[u8]) -> Result<Self> {
        let mut table = Self {
            first: [0; 17],
            count: [0; 17],
            start: [0; 17],
            symbols: symbols.to_vec(),
        };
        let (mut code, mut at) = (0u32, 0);
        for (index, count) in counts.iter().enumerate() {
            let length = index + 1;
            ensure!(
                code + u32::from(*count) < (1 << length),
                "JPEG lossless: Huffman sovraccarico/all-one"
            );
            table.first[length] = code;
            table.count[length] = u32::from(*count);
            table.start[length] = at;
            for _ in 0..*count {
                let symbol = symbols[at];
                at += 1;
                ensure!(symbol <= 16, "JPEG lossless: categoria non valida");
                code += 1;
            }
            code <<= 1;
        }
        ensure!(!symbols.is_empty(), "JPEG lossless: tabella vuota");
        Ok(table)
    }
    fn symbol(&self, bits: &mut Bits<'_>) -> Result<u8> {
        let mut code = 0;
        for length in 1..=16 {
            code = (code << 1) | bits.take(1)?;
            if code >= self.first[length] && code - self.first[length] < self.count[length] {
                return Ok(self.symbols[self.start[length] + (code - self.first[length]) as usize]);
            }
        }
        bail!("JPEG lossless: codice Huffman inesistente")
    }
}
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    byte: u8,
    remaining: u8,
}
impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            byte: 0,
            remaining: 0,
        }
    }
    fn take(&mut self, count: u8) -> Result<u32> {
        let mut value = 0;
        for _ in 0..count {
            if self.remaining == 0 {
                self.byte = *self
                    .data
                    .get(self.at)
                    .context("JPEG lossless: entropia troncata")?;
                self.at += 1;
                if self.byte == 255 {
                    ensure!(
                        self.data.get(self.at) == Some(&0),
                        "JPEG lossless: marker inatteso nell'entropia"
                    );
                    self.at += 1;
                }
                self.remaining = 8;
            }
            self.remaining -= 1;
            value = (value << 1) | u32::from((self.byte >> self.remaining) & 1);
        }
        Ok(value)
    }
    fn marker(&mut self, expected: u8) -> Result<()> {
        // Fill bits between entropy and marker must be ones (T.81 B.1.1.5).
        let mask = (1u16 << self.remaining) - 1;
        ensure!(
            u16::from(self.byte) & mask == mask,
            "JPEG lossless: padding non valido"
        );
        self.remaining = 0;
        ensure!(
            self.data.get(self.at) == Some(&255),
            "JPEG lossless: marker assente"
        );
        while self.data.get(self.at) == Some(&255) {
            self.at += 1;
        }
        ensure!(
            self.data.get(self.at) == Some(&expected),
            "JPEG lossless: sequenza marker non valida"
        );
        self.at += 1;
        Ok(())
    }
}
fn word(b: &[u8], at: usize) -> Result<usize> {
    Ok(u16::from_be_bytes(
        b.get(at..at + 2)
            .context("JPEG lossless: header troncato")?
            .try_into()
            .unwrap(),
    ) as usize)
}

pub(crate) fn decode(
    data: &[u8],
    width: usize,
    height: usize,
    bits_per_sample: u32,
) -> Result<Vec<u16>> {
    ensure!(
        data.get(..2) == Some(&[255, 216]),
        "JPEG lossless: SOI assente"
    );
    ensure!(
        width
            .checked_mul(height)
            .is_some_and(|n| n <= tr_core::color::MAX_PIXELS),
        "JPEG lossless: tile oltre quota"
    );
    let mut at = 2;
    let mut tables: [Option<Huffman>; 4] = std::array::from_fn(|_| None);
    let mut frame: Option<(usize, usize, u8, Vec<u8>)> = None;
    let mut restart = 0;
    loop {
        ensure!(
            data.get(at) == Some(&255),
            "JPEG lossless: marker non valido"
        );
        while data.get(at) == Some(&255) {
            at += 1;
        }
        let marker = *data.get(at).context("JPEG lossless: marker troncato")?;
        at += 1;
        let length = word(data, at)?;
        ensure!(length >= 2, "JPEG lossless: lunghezza marker non valida");
        let payload = data
            .get(at + 2..at.checked_add(length).context("JPEG lossless: overflow")?)
            .context("JPEG lossless: segmento troncato")?;
        at += length;
        match marker {
            0xc3 => {
                ensure!(
                    frame.is_none() && payload.len() >= 6,
                    "JPEG lossless: SOF3 invalido/duplicato"
                );
                let precision = payload[0];
                let h = word(payload, 1)?;
                let w = word(payload, 3)?;
                let components = usize::from(payload[5]);
                ensure!(
                    (2..=16).contains(&precision)
                        && u32::from(precision) == bits_per_sample
                        && (1..=4).contains(&components),
                    "JPEG lossless: precisione/componenti fuori contratto"
                );
                ensure!(
                    w > 0
                        && h > 0
                        && w * h * components == width * height
                        && payload.len() == 6 + 3 * components,
                    "JPEG lossless: geometria non corrispondente al tile"
                );
                let mut ids = Vec::new();
                for item in payload[6..].as_chunks::<3>().0 {
                    ensure!(
                        item[1] == 0x11 && item[2] == 0 && !ids.contains(&item[0]),
                        "JPEG lossless: subsampling/componenti non supportati"
                    );
                    ids.push(item[0]);
                }
                frame = Some((w, h, precision, ids));
            }
            0xc4 => {
                let mut k = 0;
                while k < payload.len() {
                    let id = usize::from(payload[k]);
                    k += 1;
                    ensure!(id < 4, "JPEG lossless: tabella AC/id non valido");
                    let counts = payload
                        .get(k..k + 16)
                        .context("JPEG lossless: DHT troncato")?;
                    k += 16;
                    let count: usize = counts.iter().map(|v| usize::from(*v)).sum();
                    ensure!(count <= 256, "JPEG lossless: DHT oltre quota");
                    let symbols = payload
                        .get(k..k + count)
                        .context("JPEG lossless: DHT troncato")?;
                    k += count;
                    tables[id] = Some(Huffman::parse(counts, symbols)?);
                }
            }
            0xdd => {
                ensure!(payload.len() == 2, "JPEG lossless: DRI invalido");
                restart = word(payload, 0)?;
            }
            0xda => {
                let (w, h, precision, ids) = frame.context("JPEG lossless: SOS senza SOF3")?;
                let n = ids.len();
                let stride = w * n;
                ensure!(
                    restart == 0 || restart % w == 0,
                    "JPEG lossless: restart non allineato alla riga"
                );
                ensure!(
                    payload.len() == 1 + 2 * n + 3 && usize::from(payload[0]) == n,
                    "JPEG lossless: scan non interleaved/incompleto"
                );
                let mut selectors = Vec::new();
                for (c, item) in payload[1..1 + 2 * n].as_chunks::<2>().0.iter().enumerate() {
                    ensure!(
                        item[0] == ids[c] && item[1] & 15 == 0,
                        "JPEG lossless: ordine componenti non supportato"
                    );
                    let table = usize::from(item[1] >> 4);
                    selectors.push(
                        tables
                            .get(table)
                            .and_then(Option::as_ref)
                            .context("JPEG lossless: DHT mancante")?,
                    );
                }
                let predictor = payload[1 + 2 * n];
                let end = payload[2 + 2 * n];
                let point = payload[3 + 2 * n];
                ensure!(
                    (1..=7).contains(&predictor) && end == 0 && point < precision,
                    "JPEG lossless: parametri scan invalidi"
                );
                let effective = precision - point;
                let initial = 1i32 << (effective - 1);
                let mask = (1i32 << effective) - 1;
                let mut output = vec![0u16; width * height];
                let mut stream = Bits::new(&data[at..]);
                let mut restart_index = 0;
                for mcu in 0..w * h {
                    let reset = mcu == 0 || (restart != 0 && mcu % restart == 0);
                    if mcu != 0 && reset {
                        stream.marker(0xd0 + restart_index)?;
                        restart_index = (restart_index + 1) % 8;
                    }
                    let (x, y) = (mcu % w, mcu / w);
                    for (c, selector) in selectors.iter().enumerate() {
                        let i = mcu * n + c;
                        let first_row = if restart == 0 {
                            y == 0
                        } else {
                            mcu % restart < w
                        };
                        let predicted = if reset {
                            initial
                        } else if first_row {
                            i32::from(output[i - n])
                        } else if x == 0 {
                            i32::from(output[i - stride])
                        } else {
                            let (a, b, d) = (
                                i32::from(output[i - n]),
                                i32::from(output[i - stride]),
                                i32::from(output[i - stride - n]),
                            );
                            match predictor {
                                1 => a,
                                2 => b,
                                3 => d,
                                4 => a + b - d,
                                5 => a + ((b - d) >> 1),
                                6 => b + ((a - d) >> 1),
                                7 => (a + b) >> 1,
                                _ => unreachable!(),
                            }
                        };
                        let category = selector.symbol(&mut stream)?;
                        ensure!(
                            category <= effective,
                            "JPEG lossless: categoria oltre precisione"
                        );
                        // T.81 H.1.2.2: category 16 represents -32768 with no extra bits.
                        let delta = if category == 0 {
                            0
                        } else if category == 16 {
                            -32768
                        } else {
                            let value = stream.take(category)? as i32;
                            if value < (1 << (category - 1)) {
                                value - ((1 << category) - 1)
                            } else {
                                value
                            }
                        };
                        output[i] = ((predicted + delta) & mask) as u16;
                    }
                }
                stream.marker(0xd9)?;
                ensure!(stream.at == data.len() - at, "JPEG lossless: dati dopo EOI");
                if point != 0 {
                    for value in &mut output {
                        *value <<= point;
                    }
                }
                return Ok(output);
            }
            0xe0..=0xef | 0xfe => {}
            _ => bail!("JPEG lossless: marker {marker:02x} non supportato"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Fixed five-bit canonical codes for categories 0..16. Vectors below are
    // hand-derived T.81 Annex H examples, not produced by another predictor.
    #[allow(clippy::too_many_arguments)] // Direct correspondence to SOF/SOS/DRI fields.
    fn stream(
        w: u16,
        h: u16,
        n: u8,
        p: u8,
        point: u8,
        predictor: u8,
        interval: u16,
        deltas: &[i32],
    ) -> Vec<u8> {
        fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
            out.extend([255, marker]);
            out.extend(((payload.len() + 2) as u16).to_be_bytes());
            out.extend(payload);
        }
        let mut out = vec![255, 216];
        let mut dht = vec![0; 17];
        dht[5] = 17;
        dht.extend(0..=16);
        segment(&mut out, 0xc4, &dht);
        let mut sof = vec![p];
        sof.extend(h.to_be_bytes());
        sof.extend(w.to_be_bytes());
        sof.push(n);
        for c in 0..n {
            sof.extend([c + 1, 0x11, 0]);
        }
        segment(&mut out, 0xc3, &sof);
        if interval != 0 {
            segment(&mut out, 0xdd, &interval.to_be_bytes());
        }
        let mut sos = vec![n];
        for c in 0..n {
            sos.extend([c + 1, 0]);
        }
        sos.extend([predictor, 0, point]);
        segment(&mut out, 0xda, &sos);
        let mut bits = Vec::new();
        let flush = |bits: &mut Vec<u8>, out: &mut Vec<u8>| {
            while !bits.len().is_multiple_of(8) {
                bits.push(1);
            }
            for chunk in bits.as_chunks::<8>().0 {
                let b = chunk.iter().fold(0, |a, b| (a << 1) | b);
                out.push(b);
                if b == 255 {
                    out.push(0);
                }
            }
            bits.clear();
        };
        let mut rest = 0;
        for (i, delta) in deltas.iter().copied().enumerate() {
            if i > 0 && interval != 0 && i % (usize::from(interval) * usize::from(n)) == 0 {
                flush(&mut bits, &mut out);
                out.extend([255, 0xd0 + rest]);
                rest = (rest + 1) % 8;
            }
            let category = if delta == -32768 {
                16
            } else {
                32 - delta.unsigned_abs().leading_zeros()
            };
            for b in (0..5).rev() {
                bits.push(((category >> b) & 1) as u8);
            }
            if category != 16 {
                let value = if delta < 0 {
                    delta + (1 << category) - 1
                } else {
                    delta
                };
                for b in (0..category).rev() {
                    bits.push(((value >> b) & 1) as u8);
                }
            }
        }
        flush(&mut bits, &mut out);
        out.extend([255, 217]);
        out
    }
    #[test]
    fn predictors_restarts_point_transform_and_components() {
        for (p, last) in [(1, 2), (2, 6), (3, 11), (4, -3), (5, 0), (6, 2), (7, 4)] {
            let bytes = stream(2, 2, 1, 8, 0, p, 0, &[-28, 5, 9, last]);
            assert_eq!(decode(&bytes, 2, 2, 8).unwrap(), [100, 105, 109, 111]);
            for end in 0..bytes.len() {
                assert!(decode(&bytes[..end], 2, 2, 8).is_err());
            }
        }
        let bytes = stream(2, 2, 1, 12, 4, 4, 0, &[-28, 5, 9, -3]);
        assert_eq!(decode(&bytes, 2, 2, 12).unwrap(), [1600, 1680, 1744, 1776]);
        let bytes = stream(2, 2, 1, 8, 0, 4, 2, &[-28, 5, -19, 2]);
        assert_eq!(decode(&bytes, 2, 2, 8).unwrap(), [100, 105, 109, 111]);
        let bytes = stream(2, 2, 2, 8, 0, 4, 0, &[-28, -78, 5, 5, 9, 10, -3, -3]);
        assert_eq!(
            decode(&bytes, 4, 2, 8).unwrap(),
            [100, 50, 105, 55, 109, 60, 111, 62]
        );
        // DNG permits any JPEG geometry with the same flattened sample count.
        assert_eq!(
            decode(&bytes, 2, 4, 8).unwrap(),
            [100, 50, 105, 55, 109, 60, 111, 62]
        );
        let bytes = stream(2, 2, 1, 16, 0, 1, 0, &[-32768, -1, -2, -1]);
        assert_eq!(decode(&bytes, 2, 2, 16).unwrap(), [0, 65535, 65534, 65533]);
    }
    #[test]
    fn dng_lossless_integrates_without_a_native_decoder() {
        let mut fixture = crate::test_support::Fixture::new(true);
        let mut deltas = vec![0; 120];
        deltas[0] = -32256;
        fixture.ints(259, 3, &[7]);
        fixture.encoded = Some(stream(12, 10, 1, 16, 0, 1, 0, &deltas));
        let bytes = fixture.bytes();
        assert_eq!(crate::unpack(&bytes).unwrap().2, vec![512; 120]);
        assert_eq!(crate::develop(&bytes, [1.; 3]).unwrap().pixels.len(), 120);
    }
}
