use crate::{
    RawInfo, RawMemoryPlan,
    color::{self, Calibration, Matrix},
    jpeg, render,
    tiff::{Field, Ifd, Tiff},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use tr_core::color::{LinearImage, MAX_PIXELS};

pub(crate) struct Dng<'a> {
    tiff: Tiff<'a>,
    pub width: usize,
    pub height: usize,
    bits: u32,
    compression: u32,
    chunks: Vec<(usize, usize)>,
    tile: [usize; 2],
    tiled: bool,
    active: [usize; 4], // left, top, width, height
    crop: [usize; 4],   // relative to ActiveArea
    cfa: [usize; 4],    // origin is ActiveArea, per DNG 1.7.1 p22
    black_repeat: [usize; 2],
    black: Vec<f64>,
    delta_h: Vec<f64>,
    delta_v: Vec<f64>,
    white: f64,
    maximum_black: f64,
    lut: Option<Vec<u16>>,
    calibration: Calibration,
    camera: String,
    profile_hash: String,
    orientation: u32,
}

fn exact(field: Field<'_>, count: usize, types: &[u16]) -> Result<Vec<f64>> {
    ensure!(
        field.count == count && types.contains(&field.kind),
        "DNG: tipo/conteggio tag invalido"
    );
    field.numbers(count)
}
fn values(ifd: &Ifd<'_>, tag: u16, defaults: &[f64], types: &[u16]) -> Result<Vec<f64>> {
    if let Some(f) = ifd.get(tag) {
        exact(f, defaults.len(), types)
    } else {
        Ok(defaults.to_vec())
    }
}
fn sizes(values: Vec<f64>) -> Result<Vec<usize>> {
    values
        .into_iter()
        .map(|v| {
            ensure!(
                (0. ..=65536.).contains(&v) && v.fract() == 0.,
                "DNG: geometria frazionaria/fuori quota"
            );
            Ok(v as usize)
        })
        .collect()
}
fn matrix(field: Field<'_>) -> Result<Matrix> {
    let v = exact(field, 9, &[10])?;
    let m = std::array::from_fn(|r| std::array::from_fn(|c| v[r * 3 + c]));
    color::inverse(m)?;
    Ok(m)
}

impl<'a> Dng<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let tiff = Tiff::parse(bytes)?;
        let root = &tiff.ifds[0];
        let version = root
            .required(50706)
            .context("Experimental: richiesto DNG; decoder NEF/RAF non ancora disponibile")?;
        let version = exact(version, 4, &[1])?;
        ensure!(
            version[0] == 1. && version[1] <= 7.,
            "Experimental: versione DNG non supportata"
        );
        let backward = values(root, 50707, &[version[0], version[1], 0., 0.], &[1])?;
        ensure!(
            backward.as_slice() <= [1., 7., 1., 0.].as_slice(),
            "DNG: compatibilità richiede una versione futura"
        );
        ensure!(
            root.integer(50879, 0)? == 0,
            "Experimental: DNG output-referred non supportato"
        );
        let mut candidates = Vec::new();
        for (i, ifd) in tiff.ifds.iter().enumerate() {
            if ifd.integer(254, 0)? == 0 && matches!(ifd.integer(262, 0)?, 32803 | 34892) {
                candidates.push(i);
            }
        }
        ensure!(
            candidates.len() == 1,
            "Experimental: DNG richiede un solo fotogramma RAW primario"
        );
        let raw = &tiff.ifds[candidates[0]];
        ensure!(
            raw.integer(262, 0)? == 32803,
            "Experimental: DNG LinearRaw non ancora supportato"
        );
        let width = raw.integer(256, 0)? as usize;
        let height = raw.integer(257, 0)? as usize;
        ensure!(
            (4..=65536).contains(&width)
                && (4..=65536).contains(&height)
                && width.checked_mul(height).is_some_and(|n| n <= MAX_PIXELS),
            "DNG: dimensioni oltre quota"
        );
        let bits = raw.integer(258, 0)?;
        let compression = raw.integer(259, 1)?;
        ensure!(
            (8..=16).contains(&bits) && [1, 7].contains(&compression),
            "Experimental: codifica DNG non supportata"
        );
        for (tag, default) in [
            (277, 1),
            (284, 1),
            (339, 1),
            (266, 1),
            (317, 1),
            (50711, 1),
            (50975, 1),
            (52547, 1),
        ] {
            ensure!(
                raw.integer(tag, default)? == default,
                "Experimental: DNG tag {tag} fuori contratto"
            );
        }
        ensure!(
            exact(raw.required(33421)?, 2, &[3])? == [2., 2.],
            "Experimental: CFA DNG non Bayer 2x2"
        );
        ensure!(
            values(raw, 50710, &[0., 1., 2.], &[1])? == [0., 1., 2.],
            "Experimental: piani CFA non RGB"
        );
        let pattern = exact(raw.required(33422)?, 4, &[1])?;
        let cfa: [usize; 4] = pattern
            .iter()
            .map(|v| *v as usize)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        ensure!(
            [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]].contains(&cfa),
            "Experimental: pattern CFA non Bayer RGB"
        );
        let area = sizes(values(
            raw,
            50829,
            &[0., 0., height as f64, width as f64],
            &[3, 4],
        )?)?;
        let (top, left, bottom, right) = (area[0], area[1], area[2], area[3]);
        ensure!(
            left < right
                && top < bottom
                && right <= width
                && bottom <= height
                && right - left >= 4
                && bottom - top >= 4,
            "DNG: ActiveArea invalida"
        );
        let active = [left, top, right - left, bottom - top];
        let origin = sizes(values(raw, 50719, &[0., 0.], &[3, 4, 5])?)?;
        let size = sizes(values(
            raw,
            50720,
            &[active[2] as f64, active[3] as f64],
            &[3, 4, 5],
        )?)?;
        let crop = [origin[0], origin[1], size[0], size[1]];
        ensure!(
            size[0] > 0
                && size[1] > 0
                && origin[0] + size[0] <= active[2]
                && origin[1] + size[1] <= active[3],
            "DNG: DefaultCrop invalido"
        );
        ensure!(
            values(raw, 50718, &[1., 1.], &[5])? == [1., 1.],
            "Experimental: DefaultScale non unitario"
        );
        ensure!(
            values(raw, 50974, &[1., 1.], &[3, 4])? == [1., 1.],
            "Experimental: SubTileBlockSize non supportato"
        );
        ensure!(
            values(raw, 51125, &[0., 0., 1., 1.], &[5])? == [0., 0., 1., 1.],
            "Experimental: DefaultUserCrop non supportato"
        );
        let orientation = root.integer(274, 1)?;
        ensure!((1..=8).contains(&orientation), "DNG: orientamento invalido");
        if let Some(field) = raw.get(274) {
            ensure!(
                field.scalar()? == f64::from(orientation),
                "DNG: orientamenti discordanti"
            );
        }

        let tiled = raw.get(324).is_some();
        ensure!(
            !(tiled && raw.get(273).is_some()),
            "DNG: strip e tile ambigui"
        );
        let tile = if tiled {
            [raw.integer(322, 0)? as usize, raw.integer(323, 0)? as usize]
        } else {
            [width, raw.integer(278, height as u32)? as usize]
        };
        ensure!(
            tile.iter().all(|v| (1..=65536).contains(v))
                && tile[0]
                    .checked_mul(tile[1])
                    .is_some_and(|v| v <= MAX_PIXELS),
            "DNG: geometria tile fuori quota"
        );
        let count = width.div_ceil(tile[0]) * height.div_ceil(tile[1]);
        ensure!(count <= 65536, "DNG: troppi tile/strip");
        let offsets = raw
            .required(if tiled { 324 } else { 273 })?
            .integers(count)?;
        let lengths = raw
            .required(if tiled { 325 } else { 279 })?
            .integers(count)?;
        ensure!(
            offsets.len() == count && lengths.len() == count,
            "DNG: conteggio tile/strip invalido"
        );
        let chunks: Vec<_> = offsets
            .into_iter()
            .zip(lengths)
            .map(|(a, b)| (a as usize, b as usize))
            .collect();
        for (at, len) in &chunks {
            ensure!(*len > 0, "DNG: tile vuoto");
            tiff.slice(*at, *len)?;
        }
        if compression == 1 {
            let columns = width.div_ceil(tile[0]);
            for (i, (_, length)) in chunks.iter().enumerate() {
                let rows = if tiled {
                    tile[1]
                } else {
                    tile[1].min(height - (i / columns) * tile[1])
                };
                ensure!(
                    *length == (tile[0] * bits as usize).div_ceil(8) * rows,
                    "DNG: dimensioni e lunghezza strip/tile incoerenti"
                );
            }
        }
        let mut ranges = chunks.clone();
        ranges.sort_unstable();
        ensure!(
            ranges.windows(2).all(|v| v[0].0 + v[0].1 <= v[1].0),
            "DNG: tile sovrapposti"
        );

        let repeat = sizes(values(raw, 50713, &[1., 1.], &[3])?)?;
        ensure!(
            repeat.iter().all(|v| (1..=64).contains(v))
                && repeat[0] <= active[3]
                && repeat[1] <= active[2],
            "DNG: pattern nero oltre quota"
        );
        let black = values(raw, 50714, &vec![0.; repeat[0] * repeat[1]], &[3, 4, 5])?;
        let delta_h = values(raw, 50715, &vec![0.; active[2]], &[10])?;
        let delta_v = values(raw, 50716, &vec![0.; active[3]], &[10])?;
        let white = values(raw, 50717, &[f64::from((1u32 << bits) - 1)], &[3, 4])?[0];
        ensure!((1. ..=65535.).contains(&white), "DNG: WhiteLevel invalido");
        // Exact maximum over ActiveArea: independent row/column deltas, grouped
        // by BlackLevelRepeatDim. DNG normalization uses one scale per plane.
        let mut maximum_black = f64::NEG_INFINITY;
        for row in 0..repeat[0] {
            for col in 0..repeat[1] {
                let h = delta_h
                    .iter()
                    .skip(col)
                    .step_by(repeat[1])
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                let v = delta_v
                    .iter()
                    .skip(row)
                    .step_by(repeat[0])
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                maximum_black = maximum_black.max(black[row * repeat[1] + col] + h + v);
            }
        }
        ensure!(
            maximum_black.is_finite()
                && maximum_black < white
                && maximum_black.abs() < 65536.
                && black
                    .iter()
                    .chain(&delta_h)
                    .chain(&delta_v)
                    .all(|v| v.abs() < 65536.),
            "DNG: calibrazione nero/bianco invalida"
        );
        let lut = raw
            .get(50712)
            .map(|f| -> Result<Vec<u16>> {
                ensure!(
                    f.kind == 3 && f.count > 0 && f.count <= 65536,
                    "DNG: LinearizationTable invalida"
                );
                Ok(f.integers(65536)?.into_iter().map(|v| v as u16).collect())
            })
            .transpose()?;
        // No silent omission of sensor or profile transforms. Optional opcodes
        // are also rejected by this strict baseline until individually supported.
        for ifd in [root, raw] {
            for tag in [
                34675, 50831, 50832, 50833, 50834, 50937, 50938, 50939, 52525, 52529, 52530, 52531,
                52532, 52533, 52534, 52535, 52536, 52537, 52538, 52543, 52544,
            ] {
                ensure!(
                    ifd.get(tag).is_none(),
                    "Experimental: trasformata DNG {tag} non supportata"
                );
            }
            for tag in [51008, 51009, 51022] {
                if let Some(f) = ifd.get(tag) {
                    ensure!(
                        f.data == [0, 0, 0, 0],
                        "Experimental: opcode DNG da implementare"
                    );
                }
            }
        }
        let cm1 = matrix(root.required(50721)?)?;
        let second = root.get(50722).map(matrix).transpose()?;
        ensure!(
            second.is_some() == root.get(50779).is_some(),
            "DNG: seconda calibrazione incompleta"
        );
        let temp1 = color::illuminant(root.integer(50778, 0)?)?;
        let temp2 = if second.is_some() {
            color::illuminant(root.integer(50779, 0)?)?
        } else {
            temp1
        };
        ensure!(
            second.is_none() || temp1 != temp2,
            "DNG: illuminanti duplicati"
        );
        let signature = |tag| -> Result<String> {
            Ok(root
                .get(tag)
                .map(Field::signature)
                .transpose()?
                .unwrap_or_default())
        };
        let cc_matches = signature(50931)? == signature(50932)?;
        let cc1 = root
            .get(50723)
            .map(matrix)
            .transpose()?
            .unwrap_or(color::ID);
        let cc2 = root
            .get(50724)
            .map(matrix)
            .transpose()?
            .unwrap_or(color::ID);
        let fm1 = root.get(50964).map(matrix).transpose()?;
        let fm2 = root.get(50965).map(matrix).transpose()?;
        ensure!(
            second.is_none() || fm1.is_some() == fm2.is_some(),
            "DNG: ForwardMatrix incompleta"
        );
        ensure!(
            second.is_some() || (root.get(50724).is_none() && fm2.is_none()),
            "DNG: calibrazione orfana"
        );
        let mut calibration = Calibration {
            cm: [cm1, second.unwrap_or(cm1)],
            cc: if cc_matches {
                [cc1, if second.is_some() { cc2 } else { cc1 }]
            } else {
                [color::ID; 2]
            },
            fm: [fm1, if second.is_some() { fm2 } else { fm1 }],
            temperatures: [temp1, temp2],
            analog: values(root, 50727, &[1., 1., 1.], &[5])?
                .try_into()
                .unwrap(),
            neutral: [1.; 3],
        };
        let neutral = root.get(50728);
        let white_xy = root.get(50729);
        ensure!(
            neutral.is_some() != white_xy.is_some(),
            "DNG: AsShotNeutral/AsShotWhiteXY assente o ambiguo"
        );
        calibration.neutral = if let Some(n) = neutral {
            exact(n, 3, &[3, 5])?.try_into().unwrap()
        } else {
            calibration.neutral_from_xy(exact(white_xy.unwrap(), 2, &[5])?.try_into().unwrap())?
        };
        calibration.resolve([1.; 3])?;
        let camera = root
            .text(50708)?
            .context("DNG: UniqueCameraModel assente")?;
        ensure!(
            !camera.is_empty() && camera.len() <= 128,
            "DNG: nome camera fuori quota"
        );
        let mut hash = Sha256::new();
        hash.update(b"TRExp-embedded-profile-v1");
        for tag in [
            50721u16, 50722, 50723, 50724, 50727, 50728, 50729, 50778, 50779, 50931, 50932, 50964,
            50965,
        ] {
            hash.update(tag.to_le_bytes());
            if let Some(f) = root.get(tag) {
                hash.update(f.kind.to_le_bytes());
                hash.update((f.count as u64).to_le_bytes());
                hash.update(f.data);
            }
        }
        let profile_hash = format!("{:x}", hash.finalize());
        Ok(Self {
            tiff,
            width,
            height,
            bits,
            compression,
            chunks,
            tile,
            tiled,
            active,
            crop,
            cfa,
            black_repeat: [repeat[0], repeat[1]],
            black,
            delta_h,
            delta_v,
            white,
            maximum_black,
            lut,
            calibration,
            camera,
            profile_hash,
            orientation,
        })
    }

    pub fn info(&self) -> Result<RawInfo> {
        let (_, _, w, h) = (self.crop[0], self.crop[1], self.crop[2], self.crop[3]);
        let (width, height) = render::oriented_size(w as u32, h as u32, self.orientation);
        let sensor_pixels = self.width * self.height;
        let active_pixels = self.active[2] * self.active[3];
        let output_pixels = w * h;
        let scratch_peak_bytes = (sensor_pixels * 2 + active_pixels * 4)
            .max(sensor_pixels * 4) // sensor plus repacked DNG output
            .max(sensor_pixels * 2 + self.tile[0] * self.tile[1] * 2)
            .max(active_pixels * 8 + output_pixels * 16)
            // Export can hold copied profile fields, a metadata payload and
            // its final output copy together (each <=16 MiB). The remaining
            // 16 MiB cover bounded directories, calibration vectors and LUT.
            + 64 * 1024 * 1024;
        Ok(RawInfo {
            width,
            height,
            camera: self.camera.clone(),
            profile_hash: self.profile_hash.clone(),
            orientation: self.orientation,
            compression: self.compression,
            memory: RawMemoryPlan {
                sensor_pixels,
                output_pixels,
                scratch_peak_bytes,
            },
        })
    }

    pub fn unpack(&self) -> Result<Vec<u16>> {
        let mut output = vec![0; self.width * self.height];
        let columns = self.width.div_ceil(self.tile[0]);
        for (index, (at, len)) in self.chunks.iter().enumerate() {
            let (x, y) = (
                (index % columns) * self.tile[0],
                (index / columns) * self.tile[1],
            );
            let (w, h) = (
                self.tile[0],
                if self.tiled {
                    self.tile[1]
                } else {
                    self.tile[1].min(self.height - y)
                },
            );
            let data = self.tiff.slice(*at, *len)?;
            let decoded = if self.compression == 7 {
                jpeg::decode(data, w, h, self.bits)?
            } else {
                self.unpacked_tile(data, w, h)?
            };
            for row in 0..h.min(self.height - y) {
                let count = w.min(self.width - x);
                output[(y + row) * self.width + x..(y + row) * self.width + x + count]
                    .copy_from_slice(&decoded[row * w..row * w + count]);
            }
        }
        Ok(output)
    }

    fn unpacked_tile(&self, bytes: &[u8], w: usize, h: usize) -> Result<Vec<u16>> {
        let stride = (w * self.bits as usize).div_ceil(8);
        ensure!(
            bytes.len() == stride * h,
            "DNG: lunghezza strip/tile incoerente"
        );
        let mut output = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                let row = &bytes[y * stride..(y + 1) * stride];
                output[y * w + x] = if self.bits == 16 {
                    let b = row[x * 2..x * 2 + 2].try_into().unwrap();
                    if self.tiff.le {
                        u16::from_le_bytes(b)
                    } else {
                        u16::from_be_bytes(b)
                    }
                } else if self.bits == 8 {
                    u16::from(row[x])
                } else {
                    let mut v = 0;
                    for bit in 0..self.bits as usize {
                        let at = x * self.bits as usize + bit;
                        v = (v << 1) | u16::from((row[at / 8] >> (7 - at % 8)) & 1);
                    }
                    v
                };
            }
        }
        Ok(output)
    }

    pub fn develop(&self, relative: [f64; 3]) -> Result<LinearImage> {
        let (gains, matrix) = self.calibration.resolve(relative)?;
        let raw = self.unpack()?;
        let [left, top, w, h] = self.active;
        let mut mosaic = vec![0.; w * h];
        let scale = (self.white - self.maximum_black).recip();
        for y in 0..h {
            for x in 0..w {
                let code = raw[(y + top) * self.width + x + left];
                let value = if let Some(lut) = &self.lut {
                    lut[usize::from(code).min(lut.len() - 1)]
                } else {
                    code
                };
                let black = self.black
                    [(y % self.black_repeat[0]) * self.black_repeat[1] + x % self.black_repeat[1]]
                    + self.delta_h[x]
                    + self.delta_v[y];
                let c = self.cfa[(y % 2) * 2 + x % 2];
                mosaic[y * w + x] = ((f64::from(value) - black) * scale) as f32 * gains[c];
            }
        }
        drop(raw);
        render::develop(&mosaic, w, h, self.cfa, matrix, self.crop, self.orientation)
    }

    pub fn validate_wb(&self, relative: [f64; 3]) -> Result<()> {
        self.calibration.resolve(relative)?;
        Ok(())
    }

    pub fn repack(&self, limit: usize) -> Result<(u32, u32, Vec<u8>)> {
        use std::collections::BTreeMap;
        let root = &self.tiff.ifds[0];
        let raw = self
            .tiff
            .ifds
            .iter()
            .find(|f| f.integer(254, 0).ok() == Some(0) && f.integer(262, 0).ok() == Some(32803))
            .unwrap();
        let short = |v: u16| {
            if self.tiff.le {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let long = |v: u32| {
            if self.tiff.le {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut fields: BTreeMap<u16, (u16, u32, Vec<u8>)> = BTreeMap::new();
        for tag in [
            50708, 50721, 50722, 50723, 50724, 50727, 50728, 50729, 50778, 50779, 50931, 50932,
            50936, 50941, 50942, 50964, 50965,
        ] {
            if let Some(f) = root.get(tag) {
                fields.insert(tag, (f.kind, f.count as u32, f.data.to_vec()));
            }
        }
        for tag in [
            33421, 33422, 50710, 50711, 50712, 50713, 50714, 50715, 50716, 50718, 50719, 50720,
            50829,
        ] {
            if let Some(f) = raw.get(tag) {
                fields.insert(tag, (f.kind, f.count as u32, f.data.to_vec()));
            }
        }
        for (tag, value) in [
            (254, 0),
            (256, self.width as u32),
            (257, self.height as u32),
            (273, 0),
            (278, self.height as u32),
            (279, (self.width * self.height * 2) as u32),
            (50717, self.white as u32),
        ] {
            fields.insert(tag, (4, 1, long(value).to_vec()));
        }
        for (tag, value) in [
            (258, 16),
            (259, 1),
            (262, 32803),
            (274, self.orientation as u16),
            (277, 1),
        ] {
            fields.insert(tag, (3, 1, short(value).to_vec()));
        }
        for (tag, value) in [(50706, vec![1, 4, 0, 0]), (50707, vec![1, 4, 0, 0])] {
            fields.insert(tag, (1, 4, value));
        }
        let description=b"TrueRenderer Experimental: stored sensor codes repacked losslessly to uint16. Full supported matrix calibration, LUT, black maps, active area and default crop retained. No WB/demosaic applied. EXIF, MakerNotes, previews and original compressed bytes omitted. Keep the original.\0";
        fields.insert(270, (2, description.len() as u32, description.to_vec()));
        let directory_end = 8 + 2 + fields.len() * 12 + 4;
        let payload_bytes: usize = fields
            .values()
            .filter(|(_, _, v)| v.len() > 4)
            .map(|(_, _, v)| (v.len() + 1) & !1)
            .sum();
        let sample_start = directory_end + payload_bytes;
        let total = sample_start + self.width * self.height * 2;
        ensure!(
            total <= limit.min(crate::MAX_SOURCE_BYTES),
            "DNG export oltre quota"
        );
        fields.insert(273, (4, 1, long(sample_start as u32).to_vec()));
        let samples = self.unpack()?;
        let mut output = Vec::with_capacity(total);
        output.extend(if self.tiff.le {
            b"II\x2a\0"
        } else {
            b"MM\0\x2a"
        });
        output.extend(long(8));
        output.extend(short(fields.len() as u16));
        let mut payload = Vec::with_capacity(payload_bytes);
        for (tag, (kind, count, bytes)) in fields {
            output.extend(short(tag));
            output.extend(short(kind));
            output.extend(long(count));
            if bytes.len() <= 4 {
                output.extend(&bytes);
                output.resize(output.len() + 4 - bytes.len(), 0);
            } else {
                output.extend(long((directory_end + payload.len()) as u32));
                payload.extend(bytes);
                if payload.len() % 2 != 0 {
                    payload.push(0);
                }
            }
        }
        output.extend(long(0));
        output.extend(payload);
        for sample in samples {
            output.extend(short(sample));
        }
        Ok((self.width as u32, self.height as u32, output))
    }
}
