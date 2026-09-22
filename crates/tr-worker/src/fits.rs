//! Read-only, bounded FITS IMAGE subset. Parses only the bytes granted by the broker.
//! No URLs, expressions, auxiliary files, compression, tables, cubes or implicit RGB.
use anyhow::{Context, Result, bail, ensure};
use std::collections::HashMap;
use tr_core::{
    color::LinearImage,
    protocol::RasterInfo,
    science::{Metadata, Sample},
};
pub fn recognizes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"SIMPLE  ")
}
struct Plane<'a> {
    bytes: &'a [u8],
    width: u32,
    height: u32,
    hdu: u16,
    bitpix: i16,
    scale: f64,
    zero: f64,
    blank: Option<i64>,
    unit: String,
}
fn value(card: &[u8]) -> Result<String> {
    ensure!(card.get(8..10) == Some(b"= "), "Carta FITS senza valore");
    let text = std::str::from_utf8(&card[10..])?.trim();
    if let Some(s) = text.strip_prefix('\'') {
        let mut chars = s.chars().peekable();
        let mut result = String::new();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    result.push('\'');
                } else {
                    return Ok(result.trim_end().into());
                }
            } else {
                result.push(c);
            }
        }
        bail!("Stringa FITS non terminata")
    }
    Ok(text.split('/').next().unwrap_or("").trim().into())
}
fn open(bytes: &[u8]) -> Result<Plane<'_>> {
    ensure!(recognizes(bytes), "Firma FITS SIMPLE assente");
    let mut offset = 0usize;
    let mut total_header = 0usize;
    for hdu in 0..256u16 {
        let start = offset;
        let mut fields = HashMap::new();
        loop {
            ensure!(
                offset - start < 1024 * 1024 && total_header < 4 * 1024 * 1024,
                "Header FITS oltre quota"
            );
            let card = bytes
                .get(offset..offset + 80)
                .context("Header FITS troncato")?;
            ensure!(
                card.iter().all(|c| (32..=126).contains(c)),
                "Header FITS non ASCII"
            );
            offset += 80;
            total_header += 80;
            let key = std::str::from_utf8(&card[..8])?.trim();
            if key == "END" {
                break;
            }
            if card.get(8..10) == Some(b"= ") {
                ensure!(
                    fields.insert(key.to_owned(), value(card)?).is_none(),
                    "Keyword FITS duplicata: {key}"
                );
            }
        }
        offset = start
            .checked_add((offset - start).div_ceil(2880) * 2880)
            .context("Offset FITS in overflow")?;
        ensure!(offset <= bytes.len(), "Padding FITS troncato");
        let integer = |key: &str, default: Option<i64>| -> Result<i64> {
            match fields.get(key) {
                Some(v) => v.parse().with_context(|| format!("{key} FITS non intero")),
                None => default.with_context(|| format!("{key} FITS assente")),
            }
        };
        let float = |key: &str, default: f64| -> Result<f64> {
            let v = fields
                .get(key)
                .map(|v| v.replace('D', "E").parse::<f64>())
                .transpose()?
                .unwrap_or(default);
            ensure!(v.is_finite(), "{key} FITS non finito");
            Ok(v)
        };
        if hdu == 0 {
            ensure!(
                fields.get("SIMPLE").is_some_and(|v| v == "T"),
                "FITS SIMPLE non vero"
            );
        } else {
            ensure!(
                fields.get("XTENSION").is_some_and(|v| v == "IMAGE"),
                "Solo estensioni FITS IMAGE non compresse"
            );
        }
        ensure!(
            integer("PCOUNT", Some(0))? == 0
                && integer("GCOUNT", Some(1))? == 1
                && !fields.get("GROUPS").is_some_and(|v| v == "T"),
            "Gruppi FITS non supportati"
        );
        let axes = integer("NAXIS", None)?;
        ensure!(
            axes == 0 || axes == 2,
            "FITS: solo immagini 2D; cubi e piani multicanale non supportati"
        );
        let bitpix = integer("BITPIX", None)?;
        ensure!(
            [8, 16, 32, -32].contains(&bitpix),
            "FITS BITPIX {bitpix}: supportati 8,16,32,-32; 64/-64 non qualificati"
        );
        if axes == 0 {
            ensure!(offset < bytes.len(), "FITS senza immagine 2D");
            continue;
        }
        let width = u32::try_from(integer("NAXIS1", None)?)?;
        let height = u32::try_from(integer("NAXIS2", None)?)?;
        let pixels = width as u64 * height as u64;
        ensure!(
            width > 0 && height > 0 && pixels <= tr_core::color::MAX_PIXELS as u64,
            "Piano FITS oltre quota"
        );
        let length = usize::try_from(pixels * bitpix.unsigned_abs() / 8)?;
        let end = offset
            .checked_add(length)
            .context("Piano FITS in overflow")?;
        ensure!(
            offset
                .checked_add(length.div_ceil(2880) * 2880)
                .is_some_and(|v| v <= bytes.len()),
            "Dati/padding FITS troncati"
        );
        let blank = fields.get("BLANK").map(|v| v.parse::<i64>()).transpose()?;
        ensure!(
            blank.is_none_or(|b| match bitpix {
                8 => (0..=255).contains(&b),
                16 => i16::try_from(b).is_ok(),
                32 => i32::try_from(b).is_ok(),
                _ => false,
            }),
            "BLANK FITS non valido per BITPIX"
        );
        return Ok(Plane {
            bytes: &bytes[offset..end],
            width,
            height,
            hdu,
            bitpix: bitpix as i16,
            scale: float("BSCALE", 1.)?,
            zero: float("BZERO", 0.)?,
            blank,
            unit: fields
                .get("BUNIT")
                .cloned()
                .unwrap_or_else(|| "unità non dichiarata".into()),
        });
    }
    bail!("Troppi HDU FITS o nessun piano 2D")
}
impl Plane<'_> {
    fn sample(&self, x: u32, y: u32) -> Result<Sample> {
        let (stored, physical, validity) = self.values(x, y)?;
        Ok(Sample {
            hdu: self.hdu,
            x,
            y,
            stored,
            physical,
            validity: validity.into(),
        })
    }
    fn values(&self, x: u32, y: u32) -> Result<(Option<f64>, Option<f64>, &'static str)> {
        ensure!(
            x < self.width && y < self.height,
            "Coordinate FITS fuori piano"
        );
        let index = (y as usize * self.width as usize + x as usize)
            * (self.bitpix.unsigned_abs() as usize / 8);
        let bytes = &self.bytes[index..];
        let v = match self.bitpix {
            8 => bytes[0] as f64,
            16 => i16::from_be_bytes(bytes[..2].try_into()?) as f64,
            32 => i32::from_be_bytes(bytes[..4].try_into()?) as f64,
            -32 => f32::from_be_bytes(bytes[..4].try_into()?) as f64,
            _ => unreachable!(),
        };
        let invalid = if v.is_nan() {
            Some("NaN")
        } else if v == f64::INFINITY {
            Some("+Inf")
        } else if v == f64::NEG_INFINITY {
            Some("-Inf")
        } else if self.bitpix > 0 && self.blank == Some(v as i64) {
            Some("BLANK")
        } else {
            None
        };
        let physical = self.zero + self.scale * v;
        let validity = invalid.unwrap_or(if physical.is_finite() {
            "valid"
        } else {
            "scaling overflow"
        });
        Ok((
            v.is_finite().then_some(v),
            (validity == "valid").then_some(physical),
            validity,
        ))
    }
    fn metadata(&self) -> Result<Metadata> {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut valid = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(v) = self.values(x, y)?.1 {
                    min = min.min(v);
                    max = max.max(v);
                    valid += 1;
                }
            }
        }
        let mut histogram = vec![0u64; 256];
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(v) = self.values(x, y)?.1 {
                    histogram[(normalize(v, min, max) * 255.).round() as usize] += 1;
                }
            }
        }
        let meta = Metadata {
            hdu: self.hdu,
            bitpix: self.bitpix,
            bscale: self.scale,
            bzero: self.zero,
            unit: self.unit.clone(),
            valid,
            invalid: self.width as u64 * self.height as u64 - valid,
            minimum: (valid > 0).then_some(min),
            maximum: (valid > 0).then_some(max),
            histogram,
        };
        meta.validate(self.width as u64 * self.height as u64)?;
        Ok(meta)
    }
}
fn normalize(v: f64, min: f64, max: f64) -> f64 {
    if min == max {
        0.5
    } else {
        // Avoid overflow of max-min for extreme finite scaled values.
        let scale = min.abs().max(max.abs()).max(f64::MIN_POSITIVE);
        ((v / scale - min / scale) / (max / scale - min / scale)).clamp(0., 1.)
    }
}
fn info(plane: &Plane<'_>, meta: Metadata) -> RasterInfo {
    RasterInfo {
        scientific: Some(Box::new(meta)),
        reference_mip: None,
        width: plane.width,
        height: plane.height,
        source_width: plane.width,
        source_height: plane.height,
        native_bits: plane.bitpix.unsigned_abs(),
        format: "FITS".into(),
        decoder: "TrueRenderer FITS IMAGE subset v1".into(),
        input_color: "Dati scientifici; proxy normalizzato, nessun ICC o significato RGB del dato"
            .into(),
        filter: tr_core::science::FILTER.into(),
        orientation: "Asse 1 orizzontale; asse 2 verso il basso; coordinate 0-based; nessuna WCS"
            .into(),
    }
}
pub fn probe(bytes: &[u8]) -> Result<RasterInfo> {
    let plane = open(bytes)?;
    let meta = plane.metadata()?;
    Ok(info(&plane, meta))
}
pub fn sample(bytes: &[u8], x: u32, y: u32) -> Result<Sample> {
    open(bytes)?.sample(x, y)
}
pub fn decode(bytes: &[u8]) -> Result<(RasterInfo, LinearImage)> {
    let plane = open(bytes)?;
    let meta = plane.metadata()?;
    let mut pixels = Vec::with_capacity(plane.width as usize * plane.height as usize);
    for y in 0..plane.height {
        for x in 0..plane.width {
            pixels.push(if let Some(v) = plane.values(x, y)?.1 {
                let v = normalize(v, meta.minimum.unwrap(), meta.maximum.unwrap()) as f32;
                [v, v, v, 1.]
            } else {
                [0.; 4]
            });
        }
    }
    let raster = LinearImage::new(plane.width, plane.height, pixels)?;
    Ok((info(&plane, meta), raster))
}

#[cfg(test)]
pub fn fixture(bitpix: i16, cards: &[&str], samples: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for card in [
        "SIMPLE  =                    T".to_owned(),
        format!("BITPIX  = {bitpix:20}"),
        "NAXIS   =                    2".into(),
        "NAXIS1  =                    4".into(),
        "NAXIS2  =                    1".into(),
    ]
    .into_iter()
    .chain(cards.iter().map(|s| s.to_string()))
    .chain(["END".into()])
    {
        bytes.extend_from_slice(card.as_bytes());
        bytes.resize(bytes.len().div_ceil(80) * 80, b' ');
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, b' ');
    bytes.extend_from_slice(samples);
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer_values_survive_beyond_fp32_and_scale_once() {
        let values = [16_777_217i32, 16_777_219, -1, 3];
        let bytes = fixture(
            32,
            &[
                "BSCALE  =                    2",
                "BZERO   =                    7",
                "BLANK   =                   -1",
                "BUNIT   = 'ADU'",
            ],
            &values
                .into_iter()
                .flat_map(i32::to_be_bytes)
                .collect::<Vec<_>>(),
        );
        assert_eq!(sample(&bytes, 0, 0).unwrap().stored, Some(16_777_217.));
        assert_eq!(sample(&bytes, 0, 0).unwrap().physical, Some(33_554_441.));
        assert_eq!(sample(&bytes, 2, 0).unwrap().validity, "BLANK");
        let (info, preview) = decode(&bytes).unwrap();
        let meta = info.scientific.unwrap();
        assert_eq!(
            (meta.valid, meta.invalid, meta.unit.as_str()),
            (3, 1, "ADU")
        );
        assert_eq!(preview.pixels[2], [0.; 4]);
    }
    #[test]
    fn float_specials_are_not_silently_zeroed() {
        let bytes = fixture(
            -32,
            &[],
            &[
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::from_bits(1),
            ]
            .into_iter()
            .flat_map(f32::to_be_bytes)
            .collect::<Vec<_>>(),
        );
        assert_eq!(sample(&bytes, 0, 0).unwrap().validity, "NaN");
        assert_eq!(sample(&bytes, 1, 0).unwrap().validity, "+Inf");
        assert_eq!(sample(&bytes, 2, 0).unwrap().validity, "-Inf");
        assert_eq!(
            sample(&bytes, 3, 0).unwrap().physical,
            Some(f32::from_bits(1) as f64)
        );
        assert_eq!(probe(&bytes).unwrap().scientific.unwrap().valid, 1);
        assert!(probe(&bytes[..3000]).is_err());
        assert!(sample(&bytes, 4, 0).is_err());
    }
    #[test]
    fn unsigned16_convention_and_rejections() {
        let bytes = fixture(
            16,
            &["BZERO   =                32768"],
            &[-32768i16, -1, 0, 32767]
                .into_iter()
                .flat_map(i16::to_be_bytes)
                .collect::<Vec<_>>(),
        );
        assert_eq!(sample(&bytes, 3, 0).unwrap().physical, Some(65535.));
        assert!(probe(&fixture(-64, &[], &[0; 32])).is_err());
        assert!(probe(&fixture(8, &["NAXIS1  = 999999999999999999"], &[0; 4])).is_err());
    }
    #[test]
    fn empty_primary_image_extension_and_negative_scale() {
        let mut bytes = Vec::new();
        for card in [
            "SIMPLE  =                    T",
            "BITPIX  =                    8",
            "NAXIS   =                    0",
            "EXTEND  =                    T",
            "END",
        ] {
            bytes.extend_from_slice(card.as_bytes());
            bytes.resize(bytes.len().div_ceil(80) * 80, b' ');
        }
        bytes.resize(2880, b' ');
        let mut extension = fixture(
            8,
            &[
                "BSCALE  =                   -2",
                "BZERO   =                   10",
            ],
            &[0, 1, 2, 3],
        );
        extension[..80].fill(b' ');
        let card = b"XTENSION= 'IMAGE   '";
        extension[..card.len()].copy_from_slice(card);
        bytes.extend(extension);
        let info = probe(&bytes).unwrap();
        let meta = info.scientific.unwrap();
        assert_eq!(
            (meta.hdu, meta.minimum, meta.maximum),
            (1, Some(4.), Some(10.))
        );
        assert_eq!(sample(&bytes, 3, 0).unwrap().physical, Some(4.));
    }
    #[test]
    fn all_invalid_and_bounded_hostile_headers() {
        let bytes = fixture(8, &["BLANK   =                    7"], &[7; 4]);
        let (info, image) = decode(&bytes).unwrap();
        let meta = info.scientific.unwrap();
        assert_eq!(
            (meta.valid, meta.invalid, meta.minimum, meta.maximum),
            (0, 4, None, None)
        );
        assert!(image.pixels.iter().all(|p| *p == [0.; 4]));
        let mut missing_end = vec![b' '; 1024 * 1024 + 80];
        missing_end[..8].copy_from_slice(b"SIMPLE  ");
        assert!(probe(&missing_end).is_err());
        let mut giant_axis = fixture(8, &[], &[0; 4]);
        giant_axis[3 * 80..4 * 80].fill(b' ');
        let card = b"NAXIS1  =           4294967295";
        giant_axis[3 * 80..3 * 80 + card.len()].copy_from_slice(card);
        assert!(probe(&giant_axis).is_err());
    }
}
