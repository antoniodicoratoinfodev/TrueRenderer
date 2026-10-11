//! Own synthetic full-frame NEF container. No camera data or photographs.
use super::Fixture;

pub struct NefFixture {
    pub root: Fixture,
    pub raw: Fixture,
    pub maker: Fixture,
}

fn empty(le: bool) -> Fixture {
    let mut f = Fixture::new(le);
    f.tags.clear();
    f
}

pub fn directory(bytes: &mut Vec<u8>, f: &Fixture) -> u32 {
    let at = bytes.len();
    bytes.extend(f.u16(f.tags.len() as u16));
    bytes.resize(at + 2 + 12 * f.tags.len() + 4, 0);
    for (i, (tag, (kind, count, payload))) in f.tags.iter().enumerate() {
        let e = at + 2 + i * 12;
        bytes[e..e + 2].copy_from_slice(&f.u16(*tag));
        bytes[e + 2..e + 4].copy_from_slice(&f.u16(*kind));
        bytes[e + 4..e + 8].copy_from_slice(&f.u32(*count));
        if payload.len() <= 4 {
            bytes[e + 8..e + 8 + payload.len()].copy_from_slice(payload);
        } else {
            let offset = bytes.len() as u32;
            bytes[e + 8..e + 12].copy_from_slice(&f.u32(offset));
            bytes.extend(payload);
            if !bytes.len().is_multiple_of(2) {
                bytes.push(0);
            }
        }
    }
    at as u32
}

impl NefFixture {
    pub fn new() -> Self {
        Self::camera(true, 14, true)
    }
    pub fn d800(bits: u32, le: bool) -> Self {
        Self::camera(false, bits, le)
    }
    fn camera(d750: bool, bits: u32, le: bool) -> Self {
        let (mut root, mut raw, mut maker) = (empty(le), empty(le), empty(le));
        let (w, h) = if d750 { (6032, 4032) } else { (7424, 4924) };
        root.text(271, "NIKON CORPORATION");
        root.text(272, if d750 { "NIKON D750" } else { "NIKON D800" });
        root.ints(274, 3, &[1]);
        root.ints(254, 4, &[1]);
        for (tag, kind, values) in [
            (254, 4, vec![0]),
            (256, 4, vec![w]),
            (257, 4, vec![h]),
            (258, 3, vec![bits]),
            (259, 3, vec![34713]),
            (262, 3, vec![32803]),
            (277, 3, vec![1]),
            (278, 4, vec![h]),
            (33421, 3, vec![2, 2]),
            (33422, 1, vec![0, 1, 1, 2]),
        ] {
            raw.ints(tag, kind, &values);
        }
        maker.ints(1, 7, &[48, 50, 49, if d750 { 49 } else { 48 }]);
        maker.ints(0x93, 3, &[3]);
        maker.ints(0x1b, 3, &[if d750 { 11 } else { 0 }, w, h, w, h, 0, 0]);
        if d750 {
            maker.ints(0x3d, 3, &[600; 4]);
            maker.rational(0x3b, false, &[1.; 4]);
        }
        maker.rational(0x0c, false, &[2., 1.5, 1., 1.]);
        let mut header = vec![0x46, 0x30];
        for _ in 0..4 {
            header.extend(maker.u16(1000));
        }
        header.resize(46, 0);
        maker.tags.insert(0x96, (7, 46, header));
        Self { root, raw, maker }
    }
    pub fn bytes(&mut self) -> Vec<u8> {
        let mut maker = if self.maker.le {
            b"II*\0\x08\0\0\0"
        } else {
            b"MM\0*\0\0\0\x08"
        }
        .to_vec();
        directory(&mut maker, &self.maker);
        let d750 = self.maker.tags[&1].2[3] == b'1';
        let mut payload = if d750 {
            b"Nikon\0\x02\x11\0\0"
        } else {
            b"Nikon\0\x02\x10\0\0"
        }
        .to_vec();
        payload.extend(maker);
        let mut exif = empty(self.root.le);
        exif.tags.insert(37500, (7, payload.len() as u32, payload));
        let mut bytes = if self.root.le {
            b"II*\0\0\0\0\0"
        } else {
            b"MM\0*\0\0\0\0"
        }
        .to_vec();
        let exif_at = directory(&mut bytes, &exif);
        // Independent fixed encoder for a constant plane: category zero is
        // the six-bit prefix 111110; four samples occupy FB EF BE.
        let strip_at = bytes.len() as u32;
        let pixels = if d750 { 6032 * 4032 } else { 7424 * 4924 };
        let bits = self.raw.tags[&258].2[if self.raw.le { 0 } else { 1 }];
        let (groups, code): (usize, &[u8]) = if bits == 12 {
            (pixels / 8, &[0xf7, 0xbd, 0xef, 0x7b, 0xde])
        } else {
            (pixels / 4, &[0xfb, 0xef, 0xbe])
        };
        bytes.reserve(groups * code.len());
        for _ in 0..groups {
            bytes.extend(code);
        }
        self.raw.ints(273, 4, &[strip_at]);
        self.raw.ints(279, 4, &[(groups * code.len()) as u32]);
        let raw_at = directory(&mut bytes, &self.raw);
        self.root.ints(330, 4, &[raw_at]);
        self.root.ints(34665, 4, &[exif_at]);
        let root_at = directory(&mut bytes, &self.root);
        bytes[4..8].copy_from_slice(&self.root.u32(root_at));
        bytes
    }
}
