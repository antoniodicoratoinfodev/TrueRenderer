//! Bounded X-T5 RAF container and sensor calibration. JPEG is used only for
//! its EXIF orientation, never as the image. No native calls or conversion.
use crate::{
    RawInfo, RawMemoryPlan,
    cfa::Cfa,
    color::{Calibration, ID},
    export::{self, Fields},
    render,
    tiff::Tiff,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tr_core::color::LinearImage;
#[path = "../../../third_party/fuji-codec/fuji.rs"]
mod codec;
#[path = "../../../third_party/fuji-codec/xt5.rs"]
mod data;
#[cfg(test)]
#[path = "../research/mod.rs"]
mod research;

const PROFILE: &str = "TRExp Fujifilm X-T5 D65 compatibility v1";
const COPYRIGHT: &str = "Matrix: LibRaw LLC 2019-2025; Dave Coffin 1997-2018; CDDL-1.0; upstream Adobe. Not a measured TrueRenderer profile.";
const WHITE: u32 = 16383;
pub(crate) struct Raf<'a> {
    pub width: usize,
    pub height: usize,
    codec: codec::Codec<'a>,
    active: [usize; 4],
    cfa: Cfa,
    black: u32,
    neutral: [(u32, u32); 3],
    calibration: Calibration,
    orientation: u32,
    profile_hash: String,
}
fn slice(b: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    b.get(at..at.checked_add(len).context("RAF: overflow intervallo")?)
        .context("RAF: file troncato")
}
fn be32(b: &[u8], at: usize) -> Result<usize> {
    Ok(u32::from_be_bytes(slice(b, at, 4)?.try_into().unwrap()) as usize)
}
fn pair(b: &[u8]) -> Result<[usize; 2]> {
    ensure!(b.len() == 4, "RAF: dimensioni metadato invalide");
    Ok([
        u16::from_be_bytes([b[0], b[1]]) as usize,
        u16::from_be_bytes([b[2], b[3]]) as usize,
    ])
}
fn orientation(jpeg: &[u8]) -> Result<u32> {
    ensure!(jpeg.starts_with(&[255, 216]), "RAF: EXIF preview assente");
    let mut at = 2;
    let mut found = None;
    for _ in 0..256 {
        let h = slice(jpeg, at, 4)?;
        ensure!(h[0] == 255, "RAF: marker preview invalido");
        if matches!(h[1], 0xda | 0xd9) {
            break;
        }
        let len = usize::from(u16::from_be_bytes([h[2], h[3]]));
        ensure!(len >= 2, "RAF: segmento preview invalido");
        let block = slice(jpeg, at + 4, len - 2)?;
        if h[1] == 0xe1 && block.starts_with(b"Exif\0\0") {
            ensure!(found.is_none(), "RAF: EXIF ambiguo");
            let tiff = Tiff::parse(&block[6..])?;
            ensure!(
                tiff.ifds[0].text(272)?.as_deref() == Some("X-T5"),
                "RAF: modello EXIF incoerente"
            );
            found = Some(tiff.ifds[0].integer(274, 0)?);
        }
        at += len + 2;
    }
    let value = found.context("RAF: orientamento EXIF assente")?;
    ensure!((1..=8).contains(&value), "RAF: orientamento invalido");
    Ok(value)
}
impl<'a> Raf<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= crate::MAX_SOURCE_BYTES,
            "RAF: sorgente oltre quota"
        );
        let h = slice(bytes, 0, 148)?;
        ensure!(
            &h[..20] == b"FUJIFILMCCD-RAW 0201" && &h[28..33] == b"X-T5\0" && &h[60..64] == b"0103",
            "Experimental: contenitore RAF non supportato"
        );
        let compression = be32(h, 108)?;
        ensure!(
            matches!(compression, 2 | 3),
            "Experimental: compressione RAF non supportata"
        );
        let jpeg = slice(bytes, be32(h, 84)?, be32(h, 88)?)?;
        let orientation = orientation(jpeg)?;
        let meta = slice(bytes, be32(h, 92)?, be32(h, 96)?)?;
        ensure!(meta.len() <= 1024 * 1024, "RAF: metadati oltre quota");
        let count = be32(meta, 0)?;
        ensure!(count <= 256, "RAF: troppi record");
        let mut at = 4;
        let mut tags = BTreeMap::new();
        for _ in 0..count {
            let b = slice(meta, at, 4)?;
            let tag = u16::from_be_bytes([b[0], b[1]]);
            let len = usize::from(u16::from_be_bytes([b[2], b[3]]));
            ensure!(
                tags.insert(tag, slice(meta, at + 4, len)?).is_none(),
                "RAF: record duplicato"
            );
            at += 4 + len;
        }
        ensure!(at == meta.len(), "RAF: lunghezza metadati incoerente");
        let tag = |t| {
            tags.get(&t)
                .copied()
                .context("RAF: record richiesto assente")
        };
        let [height, width] = pair(tag(0x100)?)?;
        let [top, left] = pair(tag(0x110)?)?;
        let [ah, aw] = pair(tag(0x111)?)?;
        ensure!(
            [width, height] == [7872, 5196] && [left, top, aw, ah] == [12, 21, 7728, 5152],
            "RAF: modalità/crop X-T5 non supportati"
        );
        ensure!(
            pair(tag(0x117)?)? == [0, 0]
                && pair(tag(0x118)?)? == [0, 0]
                && pair(tag(0x119)?)? == [ah, aw],
            "RAF: zoom RAW non supportato"
        );
        let pattern: Vec<_> = tag(0x131)?.iter().rev().copied().collect();
        let cfa = Cfa::new(6, &pattern)?;
        ensure!(
            pattern == crate::cfa::XTRANS,
            "RAF: fase CFA codec non supportata"
        );
        let tiff = Tiff::parse(slice(bytes, be32(h, 100)?, be32(h, 104)?)?)?;
        ensure!(
            tiff.le && tiff.ifds.len() == 1 && tiff.ifds[0].fields.len() == 1,
            "RAF: directory sensore ambigua"
        );
        let (raw, next, _) = tiff.directory(tiff.ifds[0].integer(0xf000, 0)? as usize)?;
        ensure!(
            next == 0
                && raw.integer(0xf001, 0)? == width as u32
                && raw.integer(0xf002, 0)? == height as u32
                && raw.integer(0xf003, 0)? == 14,
            "RAF: dimensioni/profondità incoerenti"
        );
        for t in [0xf004, 0xf005, 0xf006] {
            ensure!(
                raw.integer(t, u32::MAX)? == 0,
                "RAF: variante sensore non supportata"
            );
        }
        let codec = codec::Codec::parse(
            tiff.slice(
                raw.integer(0xf007, 0)? as usize,
                raw.integer(0xf008, 0)? as usize,
            )?,
            width,
            height,
        )?;
        ensure!(
            codec.lossy() == (compression == 3),
            "RAF: compressione incoerente"
        );
        let black = raw.required(0xf00a)?.integers(36)?;
        ensure!(
            black.len() == 36 && black.iter().all(|v| *v == black[0] && *v < WHITE),
            "RAF: nero periodico non uniforme non qualificato"
        );
        let black = black[0];
        let wb = raw.required(0xf00e)?.integers(3)?;
        ensure!(
            wb.len() == 3 && wb.iter().all(|v| (1..=65535).contains(v)),
            "RAF: WB invalido"
        );
        let neutral = [(wb[0], wb[1]), (1, 1), (wb[0], wb[2])];
        let matrix = std::array::from_fn(|r| {
            std::array::from_fn(|c| f64::from(data::MATRIX[r * 3 + c]) / 10000.)
        });
        let calibration = Calibration {
            cm: [matrix; 2],
            cc: [ID; 2],
            fm: [None; 2],
            temperatures: [6500.; 2],
            analog: [1.; 3],
            neutral: neutral.map(|(n, d)| f64::from(n) / f64::from(d)),
        };
        calibration.resolve([1.; 3])?;
        let mut hash = Sha256::new();
        hash.update(PROFILE.as_bytes());
        for v in data::MATRIX {
            hash.update(v.to_le_bytes());
        }
        for (n, d) in neutral {
            hash.update(n.to_le_bytes());
            hash.update(d.to_le_bytes());
        }
        Ok(Self {
            width,
            height,
            codec,
            active: [left, top, aw, ah],
            cfa,
            black,
            neutral,
            calibration,
            orientation,
            profile_hash: format!("{:x}", hash.finalize()),
        })
    }
    pub fn info(&self) -> Result<RawInfo> {
        let (width, height) = render::oriented_size(
            self.active[2] as u32,
            self.active[3] as u32,
            self.orientation,
        );
        let sensor_pixels = self.width * self.height;
        let output_pixels = self.active[2] * self.active[3];
        Ok(RawInfo {
            width,
            height,
            camera: "FUJIFILM X-T5".into(),
            format: if self.codec.lossy() {
                "RAF lossy"
            } else {
                "RAF lossless"
            },
            profile_source: "D65 compatibilità CDDL; non misurato",
            profile_hash: self.profile_hash.clone(),
            orientation: self.orientation,
            compression: if self.codec.lossy() { 3 } else { 2 },
            memory: RawMemoryPlan {
                sensor_pixels,
                output_pixels,
                scratch_peak_bytes: sensor_pixels * 8
                    + output_pixels * 16
                    + crate::highlights::support_bytes(self.width, self.height)
                    + 64 * 1024 * 1024
                    + crate::xtrans_dr6::bounded::SCRATCH_RESERVATION_BYTES,
            },
        })
    }
    pub fn validate_wb(&self, wb: [f64; 3]) -> Result<()> {
        self.calibration.resolve(wb)?;
        Ok(())
    }
    pub fn unpack(&self) -> Result<Vec<u16>> {
        self.codec.unpack(self.cfa.map)
    }
    pub fn develop(&self, wb: [f64; 3]) -> Result<LinearImage> {
        self.mosaic()?.develop(wb)
    }
    pub fn mosaic(&self) -> Result<crate::wb::Mosaic> {
        let raw = self.unpack()?;
        let [left, top, w, h] = self.active;
        let cfa = self.cfa.shifted(left, top);
        let scale = f64::from(WHITE - self.black).recip();
        let mut mosaic = Vec::with_capacity(w * h);
        let mut saturation = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                saturation.push(crate::highlights::classify(
                    f64::from(raw[(y + top) * self.width + x + left]),
                    f64::from(self.black),
                    f64::from(WHITE),
                ));
                mosaic.push(
                    ((f64::from(raw[(y + top) * self.width + x + left]) - f64::from(self.black))
                        * scale) as f32,
                );
            }
        }
        drop(raw);
        Ok(crate::wb::Mosaic {
            values: mosaic,
            saturation,
            width: w,
            active_width: w,
            height: h,
            cfa,
            crop: [0, 0, w, h],
            orientation: self.orientation,
            calibration: self.calibration.clone(),
        })
    }
    pub fn repack(&self, limit: usize) -> Result<(u32, u32, Vec<u8>)> {
        let mut f = Fields::new();
        let [left, top, w, h] = self.active;
        for (tag, v) in [
            (254, vec![0]),
            (256, vec![self.width as u32]),
            (257, vec![self.height as u32]),
            (273, vec![0]),
            (278, vec![self.height as u32]),
            (279, vec![(self.width * self.height * 2) as u32]),
            (50717, vec![WHITE]),
            (50719, vec![0, 0]),
            (50720, vec![w as u32, h as u32]),
            (
                50829,
                vec![top as u32, left as u32, (top + h) as u32, (left + w) as u32],
            ),
            (50941, vec![0]),
        ] {
            f.insert(
                tag,
                (
                    4,
                    v.len() as u32,
                    v.into_iter().flat_map(u32::to_le_bytes).collect(),
                ),
            );
        }
        for (tag, v) in [
            (258, vec![16]),
            (259, vec![1]),
            (262, vec![32803]),
            (274, vec![self.orientation as u16]),
            (277, vec![1]),
            (33421, vec![6, 6]),
            (50713, vec![6, 6]),
            (50714, vec![self.black as u16; 36]),
            (50778, vec![21]),
        ] {
            f.insert(
                tag,
                (
                    3,
                    v.len() as u32,
                    v.into_iter().flat_map(u16::to_le_bytes).collect(),
                ),
            );
        }
        let cfa = self.cfa.shifted(left, top);
        for (tag, v) in [
            (33422, cfa.map.map(|c| c as u8).to_vec()),
            (50706, vec![1, 4, 0, 0]),
            (50707, vec![1, 4, 0, 0]),
        ] {
            f.insert(tag, (1, v.len() as u32, v));
        }
        for (tag, value) in [
            (50708, "FUJIFILM X-T5"),
            (50936, PROFILE),
            (50942, COPYRIGHT),
            (
                270,
                "TrueRenderer Experimental: reconstructed RAF sensor codes; camera lossy quantization cannot be reversed. Black, WB, X-Trans and geometry retained. No WB/demosaic applied; lens corrections and EXIF/MakerNotes/preview omitted. Keep the original.",
            ),
        ] {
            let mut v = value.as_bytes().to_vec();
            v.push(0);
            f.insert(tag, (2, v.len() as u32, v));
        }
        f.insert(
            50721,
            (
                10,
                9,
                data::MATRIX
                    .into_iter()
                    .flat_map(|v| v.to_le_bytes().into_iter().chain(10000i32.to_le_bytes()))
                    .collect(),
            ),
        );
        f.insert(
            50728,
            (
                5,
                3,
                self.neutral
                    .into_iter()
                    .flat_map(|(n, d)| n.to_le_bytes().into_iter().chain(d.to_le_bytes()))
                    .collect(),
            ),
        );
        export::sensor(f, true, self.width, self.height, limit, || self.unpack())
    }
}
