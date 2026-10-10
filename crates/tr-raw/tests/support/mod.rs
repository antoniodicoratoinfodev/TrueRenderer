use std::collections::BTreeMap;

pub struct Fixture {
    pub le: bool,
    pub width: usize,
    pub height: usize,
    pub tags: BTreeMap<u16, (u16, u32, Vec<u8>)>,
    pub pixels: Vec<u16>,
    pub encoded: Option<Vec<u8>>,
}
impl Fixture {
    pub fn new(le: bool) -> Self {
        let mut f = Self {
            le,
            width: 12,
            height: 10,
            tags: BTreeMap::new(),
            pixels: vec![512; 120],
            encoded: None,
        };
        for (tag, value) in [
            (256, 12),
            (257, 10),
            (258, 16),
            (259, 1),
            (262, 32803),
            (274, 1),
            (277, 1),
            (278, 10),
            (50778, 21),
        ] {
            f.ints(
                tag,
                if [256, 257, 278].contains(&tag) { 4 } else { 3 },
                &[value],
            );
        }
        f.ints(50706, 1, &[1, 4, 0, 0]);
        f.ints(50707, 1, &[1, 1, 0, 0]);
        f.ints(33421, 3, &[2, 2]);
        f.ints(33422, 1, &[0, 1, 1, 2]);
        f.text(50708, "TrueRenderer Synthetic");
        f.rational(50721, true, &[1., 0., 0., 0., 1., 0., 0., 0., 1.]);
        f.rational(50728, false, &[0.9504559, 1., 1.0890578]);
        f.ints(50717, 4, &[1024]);
        f
    }
    pub fn u16(&self, v: u16) -> [u8; 2] {
        if self.le {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
    pub fn u32(&self, v: u32) -> [u8; 4] {
        if self.le {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
    pub fn ints(&mut self, tag: u16, kind: u16, values: &[u32]) {
        let bytes = values
            .iter()
            .flat_map(|v| match kind {
                1 | 7 => vec![*v as u8],
                3 => self.u16(*v as u16).to_vec(),
                4 => self.u32(*v).to_vec(),
                _ => panic!(),
            })
            .collect();
        self.tags.insert(tag, (kind, values.len() as u32, bytes));
    }
    pub fn rational(&mut self, tag: u16, signed: bool, values: &[f64]) {
        let bytes = values
            .iter()
            .flat_map(|v| {
                self.u32((v * 1e7).round() as i32 as u32)
                    .into_iter()
                    .chain(self.u32(10_000_000))
            })
            .collect();
        self.tags.insert(
            tag,
            (if signed { 10 } else { 5 }, values.len() as u32, bytes),
        );
    }
    pub fn text(&mut self, tag: u16, text: &str) {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        self.tags.insert(tag, (2, bytes.len() as u32, bytes));
    }
    pub fn bytes(&mut self) -> Vec<u8> {
        let bits = self.tags[&258].2.clone();
        let bits = if self.le {
            u16::from_le_bytes(bits.try_into().unwrap())
        } else {
            u16::from_be_bytes(bits.try_into().unwrap())
        } as usize;
        let data = self.encoded.clone().unwrap_or_else(|| {
            if bits == 16 {
                self.pixels.iter().flat_map(|v| self.u16(*v)).collect()
            } else {
                let stride = (self.width * bits).div_ceil(8);
                let mut data = vec![0; stride * self.height];
                for (i, v) in self.pixels.iter().enumerate() {
                    for b in 0..bits {
                        let bit = (i % self.width) * bits + b;
                        data[(i / self.width) * stride + bit / 8] |=
                            (((v >> (bits - 1 - b)) & 1) as u8) << (7 - bit % 8);
                    }
                }
                data
            }
        });
        self.ints(273, 4, &[0]);
        self.ints(279, 4, &[data.len() as u32]);
        let dir_end = 8 + 2 + self.tags.len() * 12 + 4;
        let mut tail = Vec::new();
        let mut output = if self.le {
            b"II\x2a\0".to_vec()
        } else {
            b"MM\0\x2a".to_vec()
        };
        output.extend(self.u32(8));
        output.extend(self.u16(self.tags.len() as u16));
        let mut strip_pointer = 0;
        for (tag, (kind, count, bytes)) in &self.tags {
            output.extend(self.u16(*tag));
            output.extend(self.u16(*kind));
            output.extend(self.u32(*count));
            if *tag == 273 {
                strip_pointer = output.len();
            }
            if bytes.len() <= 4 {
                output.extend(bytes);
                output.resize(output.len() + 4 - bytes.len(), 0);
            } else {
                output.extend(self.u32((dir_end + tail.len()) as u32));
                tail.extend(bytes);
                if tail.len() % 2 != 0 {
                    tail.push(0);
                }
            }
        }
        output.extend(self.u32(0));
        output.extend(tail);
        let position = self.u32(output.len() as u32);
        output[strip_pointer..strip_pointer + 4].copy_from_slice(&position);
        output.extend(data);
        output
    }
}
