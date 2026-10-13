//! Bounded Nikon D750/D800 FX lossless NEF. See ADR 0012 E7/E8.
//! The codec is direct; neither a DNG conversion nor a legacy decoder is used.
use crate::{
    RawInfo, RawMemoryPlan,
    color::{Calibration, ID},
    export::{self, Fields},
    render,
    tiff::{Field, Tiff},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use tr_core::color::LinearImage;

// Constant data have their own CDDL license and provenance, kept separate from
// implementation. They do not introduce a library dependency or decoder calls.
#[path = "../../../third_party/nikon-reference-data/nikon_d750.rs"]
mod data;

const CFA: [usize; 4] = [0, 1, 1, 2];
const COPYRIGHT: &str = "Matrix data: LibRaw LLC 2019-2025; Dave Coffin 1997-2018; CDDL-1.0. Upstream source: Adobe DNG Converter. Not a measured TrueRenderer profile.";

struct Mode {
    camera: &'static str,
    width: usize,
    height: usize,
    active_width: usize,
    crop: [usize; 4],
    bits: u32,
    white: u32,
    matrix: [i32; 9],
    profile: &'static str,
}

pub(crate) struct Nef<'a> {
    pub width: usize,
    pub height: usize,
    mode: Mode,
    strip: &'a [u8],
    seeds: [u16; 4],
    black: [u16; 4],
    neutral: [(u32, u32); 3],
    calibration: Calibration,
    orientation: u32,
    profile_hash: String,
}

fn typed(field: Field<'_>, kind: u16, count: usize) -> Result<Field<'_>> {
    ensure!(
        field.kind == kind && field.count == count,
        "NEF: tipo/conteggio metadato non supportato"
    );
    Ok(field)
}

impl<'a> Nef<'a> {
    pub fn parse(tiff: Tiff<'a>) -> Result<Self> {
        let root = &tiff.ifds[0];
        ensure!(
            root.text(271)?.as_deref() == Some("NIKON CORPORATION"),
            "Experimental: formato RAW non supportato"
        );
        let model = root.text(272)?.context("NEF: modello assente")?;
        ensure!(
            matches!(model.as_str(), "NIKON D750" | "NIKON D800"),
            "Experimental: modello NEF non supportato"
        );
        let d750 = model == "NIKON D750";
        let mut raws = Vec::new();
        for ifd in &tiff.ifds {
            if ifd.integer(254, 0)? == 0 && ifd.integer(262, 0)? == 32803 {
                raws.push(ifd);
            }
        }
        ensure!(raws.len() == 1, "NEF: mosaico principale assente o ambiguo");
        let raw = raws[0];
        let bits = raw.integer(258, 0)?;
        ensure!(
            (d750 && bits == 14) || (!d750 && matches!(bits, 12 | 14)),
            "NEF: profondità non supportata"
        );
        let mode = if d750 {
            Mode {
                camera: "NIKON D750",
                width: 6032,
                height: 4032,
                active_width: 6032,
                crop: [8, 8, 6016, 4016],
                bits,
                white: 15520,
                matrix: data::D750_MATRIX,
                profile: "TRExp Nikon D750 D65 compatibility v1",
            }
        } else {
            Mode {
                camera: "NIKON D800",
                width: 7424,
                height: 4924,
                active_width: 7378,
                crop: [8, 6, 7360, 4912],
                bits,
                white: 15520 >> (14 - bits),
                matrix: data::D800_MATRIX,
                profile: "TRExp Nikon D800 D65 compatibility v1",
            }
        };
        for (tag, expected) in [
            (256, mode.width as u32),
            (257, mode.height as u32),
            (258, mode.bits),
            (259, 34713),
            (277, 1),
            (278, mode.height as u32),
        ] {
            ensure!(
                raw.integer(tag, 0)? == expected,
                "NEF: modalità sensore non supportata (tag {tag})"
            );
        }
        ensure!(
            raw.integer(284, 1)? == 1
                && raw.integer(266, 1)? == 1
                && raw.integer(339, 1)? == 1
                && raw.integer(317, 1)? == 1
                && raw.get(324).is_none()
                && raw.get(325).is_none(),
            "NEF: layout sensore non supportato"
        );
        ensure!(
            typed(raw.required(33421)?, 3, 2)?.integers(2)? == [2, 2]
                && typed(raw.required(33422)?, 1, 4)?.data == [0, 1, 1, 2],
            "NEF: CFA non RGGB"
        );
        let offset = raw.integer(273, 0)? as usize;
        let length = raw.integer(279, 0)? as usize;
        // Minimum code length is two bits/pixel; maximum is 8+14 bits/pixel.
        ensure!(
            length >= mode.width * mode.height / 4
                && length <= (mode.width * mode.height * (8 + bits as usize)).div_ceil(8) + 16,
            "NEF: dimensione strip fuori quota"
        );
        let strip = tiff.slice(offset, length)?;
        let orientation = root.integer(274, 1)?;
        ensure!((1..=8).contains(&orientation), "NEF: orientamento invalido");
        let (exif, next, _) = tiff.directory(root.integer(34665, 0)? as usize)?;
        ensure!(next == 0, "NEF: catena EXIF non supportata");
        let maker = exif.required(37500)?;
        ensure!(
            maker.kind == 7
                && maker.data.get(..10)
                    == Some(if d750 {
                        b"Nikon\0\x02\x11\0\0"
                    } else {
                        b"Nikon\0\x02\x10\0\0"
                    }),
            "NEF: MakerNote non supportato"
        );
        let maker = Tiff::maker_directory(&maker.data[10..])?;
        ensure!(
            typed(maker.required(1)?, 7, 4)?.data == if d750 { b"0211" } else { b"0210" }
                && maker.integer(0x93, 0)? == 3,
            "NEF: versione/compressione Nikon non supportata"
        );
        ensure!(
            typed(maker.required(0x1b)?, 3, 7)?.integers(7)?
                == [
                    if d750 { 11 } else { 0 },
                    mode.width as u32,
                    mode.height as u32,
                    mode.width as u32,
                    mode.height as u32,
                    0,
                    0
                ],
            "NEF: crop in camera non supportato"
        );
        let codec = typed(maker.required(0x96)?, 7, 46)?;
        ensure!(
            codec.data[..2] == [0x46, 0x30],
            "NEF: header codec non supportato"
        );
        let seeds = std::array::from_fn(|i| {
            let b = codec.data[2 + i * 2..4 + i * 2].try_into().unwrap();
            if codec.le {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        });
        ensure!(
            seeds.iter().all(|v| u32::from(*v) < (1 << bits)),
            "NEF: predittori iniziali fuori precisione"
        );
        let levels = if d750 {
            typed(maker.required(0x3d)?, 3, 4)?.integers(4)?
        } else {
            ensure!(maker.get(0x3d).is_none(), "NEF D800: livello nero inatteso");
            vec![0; 4]
        };
        ensure!(
            levels.iter().all(|v| *v < mode.white),
            "NEF: nero fuori scala"
        );
        let black = std::array::from_fn(|i| levels[i] as u16);
        let analog = if d750 {
            typed(maker.required(0x3b)?, 5, 4)?.numbers(4)?
        } else {
            ensure!(
                maker.get(0x3b).is_none(),
                "NEF D800: guadagni analogici inattesi"
            );
            vec![1.; 4]
        };
        ensure!(
            analog == [1.; 4],
            "NEF: AnalogBalance non unitario non supportato"
        );
        let wb = typed(maker.required(0x0c)?, 5, 4)?;
        let gains = wb.numbers(4)?;
        ensure!(
            gains[2..] == [1., 1.] && gains[..2].iter().all(|g| (0.01..=100.).contains(g)),
            "NEF: WB R/B/G/G non supportato"
        );
        // Reciprocal rationals preserve the exact representation on DNG export.
        let reciprocal = |i: usize| {
            let integer = |b: &[u8]| {
                if wb.le {
                    u32::from_le_bytes(b.try_into().unwrap())
                } else {
                    u32::from_be_bytes(b.try_into().unwrap())
                }
            };
            (
                integer(&wb.data[i * 8 + 4..i * 8 + 8]),
                integer(&wb.data[i * 8..i * 8 + 4]),
            )
        };
        let neutral = [reciprocal(0), (1, 1), reciprocal(1)];
        let matrix = std::array::from_fn(|r| {
            std::array::from_fn(|c| f64::from(mode.matrix[r * 3 + c]) / 10000.)
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
        hash.update(mode.profile.as_bytes());
        for v in mode.matrix {
            hash.update(v.to_le_bytes());
        }
        for (n, d) in neutral {
            hash.update(n.to_le_bytes());
            hash.update(d.to_le_bytes());
        }
        Ok(Self {
            width: mode.width,
            height: mode.height,
            mode,
            strip,
            seeds,
            black,
            neutral,
            calibration,
            orientation,
            profile_hash: format!("{:x}", hash.finalize()),
        })
    }

    pub fn info(&self) -> Result<RawInfo> {
        let (width, height) = render::oriented_size(
            self.mode.crop[2] as u32,
            self.mode.crop[3] as u32,
            self.orientation,
        );
        let sensor_pixels = self.width * self.height;
        let output_pixels = self.mode.crop[2] * self.mode.crop[3];
        Ok(RawInfo {
            width,
            height,
            camera: self.mode.camera.into(),
            format: "NEF",
            profile_source: "D65 compatibilità CDDL; non misurato",
            profile_hash: self.profile_hash.clone(),
            orientation: self.orientation,
            compression: 34713,
            memory: RawMemoryPlan {
                sensor_pixels,
                output_pixels,
                // Unpack/calibration owns u16 + f32 sensor samples; development
                // owns the f32 mosaic, three green rows and final RGBA. Export's
                // two u16 rasters fit below the calibration peak.
                scratch_peak_bytes: (sensor_pixels * 6).max(
                    sensor_pixels * 4
                        + output_pixels * 16
                        + render::bayer_scratch_reservation_bytes(self.width),
                ) + 64 * 1024 * 1024,
            },
        })
    }
    pub fn validate_wb(&self, relative: [f64; 3]) -> Result<()> {
        self.calibration.resolve(relative)?;
        Ok(())
    }
    pub fn unpack(&self) -> Result<Vec<u16>> {
        lossless(
            self.strip,
            self.width,
            self.height,
            self.seeds,
            self.mode.bits,
        )
    }
    pub fn develop(&self, relative: [f64; 3]) -> Result<LinearImage> {
        self.mosaic()?.develop(relative)
    }
    pub fn mosaic(&self) -> Result<crate::wb::Mosaic> {
        let raw = self.unpack()?;
        let scale =
            (f64::from(self.mode.white) - f64::from(*self.black.iter().max().unwrap())).recip();
        let values = raw
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let phase = ((i / self.width) % 2) * 2 + i % 2;
                ((f64::from(*v) - f64::from(self.black[phase])) * scale) as f32
            })
            .collect();
        Ok(crate::wb::Mosaic {
            values,
            width: self.width,
            height: self.height,
            cfa: crate::cfa::Cfa::bayer(CFA),
            crop: self.mode.crop,
            orientation: self.orientation,
            calibration: self.calibration.clone(),
        })
    }
    pub fn repack(&self, limit: usize) -> Result<(u32, u32, Vec<u8>)> {
        let mut f = Fields::new();
        for (tag, values) in [
            (254, vec![0]),
            (256, vec![self.width as u32]),
            (257, vec![self.height as u32]),
            (273, vec![0]),
            (278, vec![self.height as u32]),
            (279, vec![(self.width * self.height * 2) as u32]),
            (50717, vec![self.mode.white]),
            (
                50719,
                self.mode.crop[..2].iter().map(|v| *v as u32).collect(),
            ),
            (
                50720,
                self.mode.crop[2..].iter().map(|v| *v as u32).collect(),
            ),
            (
                50829,
                vec![0, 0, self.height as u32, self.mode.active_width as u32],
            ),
            (50941, vec![0]),
        ] {
            f.insert(
                tag,
                (
                    4,
                    values.len() as u32,
                    values.into_iter().flat_map(u32::to_le_bytes).collect(),
                ),
            );
        }
        for (tag, values) in [
            (258, vec![16]),
            (259, vec![1]),
            (262, vec![32803]),
            (274, vec![self.orientation as u16]),
            (277, vec![1]),
            (33421, vec![2, 2]),
            (
                50713,
                if self.mode.camera == "NIKON D800" {
                    vec![1, 1]
                } else {
                    vec![2, 2]
                },
            ),
            (
                50714,
                if self.mode.camera == "NIKON D800" {
                    vec![0]
                } else {
                    self.black.to_vec()
                },
            ),
            (50778, vec![21]),
        ] {
            f.insert(
                tag,
                (
                    3,
                    values.len() as u32,
                    values.into_iter().flat_map(u16::to_le_bytes).collect(),
                ),
            );
        }
        for (tag, values) in [
            (33422, vec![0, 1, 1, 2]),
            (50706, vec![1, 4, 0, 0]),
            (50707, vec![1, 4, 0, 0]),
        ] {
            f.insert(tag, (1, 4, values));
        }
        for (tag, value) in [
            (50708, self.mode.camera),
            (50936, self.mode.profile),
            (50942, COPYRIGHT),
            (
                270,
                "TrueRenderer Experimental: NEF sensor codes, black, WB and geometry; D65 compatibility profile. No WB/demosaic applied. EXIF/MakerNotes/previews omitted. Keep the original.",
            ),
        ] {
            let mut bytes = value.as_bytes().to_vec();
            bytes.push(0);
            f.insert(tag, (2, bytes.len() as u32, bytes));
        }
        f.insert(
            50721,
            (
                10,
                9,
                self.mode
                    .matrix
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

// Reads at most 14 bits. Peeking may pad the last byte, consuming never does.
struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl Bits<'_> {
    fn peek(&self, n: usize) -> u32 {
        let at = self.position / 8;
        let word = (0..3).fold(0u32, |v, i| {
            (v << 8) | u32::from(self.bytes.get(at + i).copied().unwrap_or(0))
        });
        ((word << (self.position % 8)) & 0xffffff) >> (24 - n)
    }
    fn take(&mut self, n: usize) -> Result<u32> {
        ensure!(
            self.position + n <= self.bytes.len() * 8,
            "NEF: flusso lossless troncato"
        );
        let value = self.peek(n);
        self.position += n;
        Ok(value)
    }
}

fn lossless(
    bytes: &[u8],
    width: usize,
    height: usize,
    seeds: [u16; 4],
    precision: u32,
) -> Result<Vec<u16>> {
    ensure!(
        matches!(precision, 12 | 14),
        "NEF: precisione codec non supportata"
    );
    ensure!(
        width >= 2
            && width.is_multiple_of(2)
            && height >= 2
            && width
                .checked_mul(height)
                .context("NEF: overflow dimensioni")?
                <= tr_core::color::MAX_PIXELS,
        "NEF: dimensioni codec fuori quota"
    );
    let mut lookup = [(0usize, 0usize); 256];
    let (mut code, mut symbol) = (0usize, 0usize);
    let (counts, categories): (&[u8], &[u8]) = if precision == 14 {
        (&data::LOSSLESS14_COUNTS, &data::LOSSLESS14_CATEGORIES)
    } else {
        (&data::LOSSLESS12_COUNTS, &data::LOSSLESS12_CATEGORIES)
    };
    for (i, count) in counts.iter().copied().enumerate() {
        let length = i + 1;
        for _ in 0..count {
            let start = code << (8 - length);
            lookup[start..start + (1 << (8 - length))].fill((categories[symbol] as usize, length));
            symbol += 1;
            code += 1;
        }
        code <<= 1;
    }
    let mut bits = Bits { bytes, position: 0 };
    let mut samples: Vec<u16> = Vec::with_capacity(width * height);
    for i in 0..width * height {
        let (category, length) = lookup[bits.peek(8) as usize];
        ensure!(length != 0, "NEF: codice Huffman invalido");
        bits.take(length)?;
        let magnitude = bits.take(category)? as i32;
        let delta = if category > 0 && magnitude < (1 << (category - 1)) {
            magnitude - ((1 << category) - 1)
        } else {
            magnitude
        };
        let (x, y) = (i % width, i / width);
        let predicted = if x >= 2 {
            samples[i - 2]
        } else if y >= 2 {
            samples[i - 2 * width]
        } else {
            seeds[y * 2 + x]
        };
        let value = i32::from(predicted) + delta;
        ensure!(
            (0..1 << precision).contains(&value),
            "NEF: campione ricostruito fuori precisione"
        );
        samples.push(value as u16);
    }
    ensure!(
        (bytes.len() * 8 - bits.position).div_ceil(8) <= 16,
        "NEF: coda lossless inattesa"
    );
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_golden_domains_predictors_and_truncation() {
        // Own independent bit-string encoder, fixed prefixes; seeds differ to
        // check both row phases and both column predictors. Not a photo.
        let bytes = [
            241, 255, 255, 252, 23, 254, 255, 255, 240, 0, 62, 102, 255, 191, 190, 213, 1, 231,
            255, 126, 254, 253, 249, 255, 255, 248,
        ];
        let expected = [
            0, 16383, 8191, 8192, 500, 0, 501, 1, 2, 16000, 4096, 16001, 501, 0, 500, 16383,
        ];
        let seeds = [2048, 1000, 500, 200];
        assert_eq!(lossless(&bytes, 4, 4, seeds, 14).unwrap(), expected);
        for end in 0..bytes.len() {
            assert!(lossless(&bytes[..end], 4, 4, seeds, 14).is_err());
        }
        assert!(lossless(&bytes, usize::MAX, 4, seeds, 14).is_err());
        assert!(lossless(&bytes, 4, 4, [16383; 4], 14).is_err());
        for i in 0..2048 {
            let mut changed = bytes;
            changed[i % bytes.len()] ^= (i / bytes.len()) as u8;
            let _ = lossless(&changed, 4, 4, seeds, 14);
        }
    }
}
