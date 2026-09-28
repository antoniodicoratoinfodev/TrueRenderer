//! Small read-only EXIF presentation profile, always inside the decoder worker.
//! The parser sees at most 4 MiB of the original container, never a preview JPEG.
//! This is not the qualified catalog/XMP metadata subsystem of architecture §13.4.
use exif::{Exif, In, Tag, Value};
use std::io::Cursor;
use tr_core::protocol::{ShootingInfo, ShootingValue};

const MAX_BYTES: usize = 4 * 1024 * 1024;

pub fn read(bytes: &[u8]) -> Option<Box<ShootingInfo>> {
    // Limit the supported container profile, independently of the parser's
    // broader capabilities. BigTIFF and proprietary MakerNotes are not inferred.
    if !(super::container::is_tiff_container(bytes)
        || bytes.starts_with(b"\xff\xd8")
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
    {
        return None;
    }
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(&bytes[..bytes.len().min(MAX_BYTES)]))
        .ok()?;
    if exif.fields().count() > 512 {
        return None;
    }
    let result = ShootingInfo {
        camera: equipment(&exif, Tag::Make, Tag::Model),
        lens: equipment(&exif, Tag::LensMake, Tag::LensModel).or_else(|| lens_specification(&exif)),
        exposure: rational(&exif, Tag::ExposureTime).map(|n| {
            let reciprocal = 1.0 / n;
            let text = if n < 1.0 && (reciprocal - reciprocal.round()).abs() < 0.001 {
                format!("1/{:.0} s", reciprocal)
            } else {
                format!("{} s", decimal(n))
            };
            fact(text, "ExposureTime")
        }),
        aperture: rational(&exif, Tag::FNumber)
            .map(|n| fact(format!("f/{}", decimal(n)), "FNumber")),
        iso: iso(&exif),
        focal_length: rational(&exif, Tag::FocalLength)
            .map(|n| fact(format!("{} mm", decimal(n)), "FocalLength")),
    };
    (result.values().iter().any(Option::is_some) && result.validate().is_ok())
        .then(|| Box::new(result))
}

fn fact(text: String, tag: &str) -> ShootingValue {
    ShootingValue {
        text,
        source: format!("EXIF primary · {tag} · kamadak-exif 0.6.1 · TR shooting v1"),
    }
}

fn value(exif: &Exif, tag: Tag) -> Option<&Value> {
    // Conflicting duplicates are not silently selected, nor is IFD1 (thumbnail).
    let mut fields = exif
        .fields()
        .filter(|f| f.tag == tag && f.ifd_num == In::PRIMARY);
    let field = fields.next()?;
    fields.next().is_none().then_some(&field.value)
}

fn ascii(exif: &Exif, tag: Tag) -> Option<String> {
    let Value::Ascii(parts) = value(exif, tag)? else {
        return None;
    };
    if parts.len() != 1 || parts[0].len() > 128 {
        return None;
    }
    let text = std::str::from_utf8(&parts[0]).ok()?.trim().to_owned();
    (!text.is_empty() && !text.chars().any(char::is_control)).then_some(text)
}

fn equipment(exif: &Exif, make: Tag, model: Tag) -> Option<ShootingValue> {
    let brand = ascii(exif, make);
    let name = ascii(exif, model);
    let (text, tags) = match (brand, name) {
        (Some(a), Some(b)) => {
            let text = if b.to_lowercase().starts_with(&a.to_lowercase()) {
                b
            } else {
                format!("{a} {b}")
            };
            (text, format!("{make}/{model}"))
        }
        (Some(a), None) if make == Tag::Make => (a, make.to_string()),
        (_, Some(b)) => (b, model.to_string()),
        _ => return None,
    };
    Some(fact(text, &tags))
}

fn positive(n: exif::Rational) -> Option<f64> {
    (n.num > 0 && n.denom > 0).then(|| n.to_f64())
}
fn rational(exif: &Exif, tag: Tag) -> Option<f64> {
    let Value::Rational(values) = value(exif, tag)? else {
        return None;
    };
    if values.len() != 1 {
        return None;
    }
    positive(values[0])
}
fn decimal(n: f64) -> String {
    if n < 0.000001 {
        return format!("{n:e}");
    }
    let text = format!("{n:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}
fn iso(exif: &Exif) -> Option<ShootingValue> {
    let integer = |tag| match value(exif, tag)? {
        Value::Short(v) if v.len() == 1 => Some(u32::from(v[0])),
        Value::Long(v) if v.len() == 1 => Some(v[0]),
        _ => None,
    };
    let n = integer(Tag::PhotographicSensitivity);
    if let Some(n @ 1..=65534) = n {
        return Some(fact(n.to_string(), "PhotographicSensitivity"));
    }
    // 65535 is the EXIF overflow sentinel, not an ISO value. ISOSpeed is
    // unambiguous; neither exposure index nor SOS is relabelled as ISO speed.
    let n = integer(Tag::ISOSpeed)?;
    (n > 0).then(|| fact(n.to_string(), "ISOSpeed"))
}
fn lens_specification(exif: &Exif) -> Option<ShootingValue> {
    let Value::Rational(values) = value(exif, Tag::LensSpecification)? else {
        return None;
    };
    if values.len() != 4 {
        return None;
    }
    let min = positive(values[0])?;
    let max = positive(values[1])?;
    if max < min {
        return None;
    }
    let range = |a, b| {
        if a == b {
            decimal(a)
        } else {
            format!("{}–{}", decimal(a), decimal(b))
        }
    };
    let mut text = format!("{} mm", range(min, max));
    if let (Some(a), Some(b)) = (positive(values[2]), positive(values[3])) {
        text.push_str(&format!(" f/{}", range(a, b)));
    }
    Some(fact(text, "LensSpecification"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use exif::{Field, Rational, experimental::Writer};

    fn field(tag: Tag, value: Value) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value,
        }
    }
    fn rat(num: u32, denom: u32) -> Value {
        Value::Rational(vec![Rational { num, denom }])
    }
    fn fields() -> Vec<Field> {
        vec![
            field(Tag::Make, Value::Ascii(vec![b"Example".to_vec()])),
            field(Tag::Model, Value::Ascii(vec![b"Example Camera".to_vec()])),
            field(Tag::LensModel, Value::Ascii(vec![b"24-70 mm".to_vec()])),
            field(Tag::ExposureTime, rat(10, 2500)),
            field(Tag::FNumber, rat(28, 10)),
            field(Tag::PhotographicSensitivity, Value::Short(vec![800])),
            field(Tag::FocalLength, rat(50, 1)),
        ]
    }
    fn tiff(fields: &[Field], little: bool) -> Vec<u8> {
        let mut writer = Writer::new();
        for field in fields {
            writer.push_field(field);
        }
        let mut data = Cursor::new(vec![]);
        writer.write(&mut data, little).unwrap();
        data.into_inner()
    }
    fn texts(info: &ShootingInfo) -> Vec<&str> {
        info.values()
            .into_iter()
            .flatten()
            .map(|v| v.text.as_str())
            .collect()
    }
    #[test]
    fn standard_tags_both_endians_and_containers() {
        for little in [false, true] {
            let exif = tiff(&fields(), little);
            let expected = read(&exif).unwrap();
            assert_eq!(
                texts(&expected),
                [
                    "Example Camera",
                    "24-70 mm",
                    "1/250 s",
                    "f/2.8",
                    "800",
                    "50 mm"
                ]
            );
            let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
            jpeg.extend_from_slice(&((exif.len() + 8) as u16).to_be_bytes());
            jpeg.extend_from_slice(b"Exif\0\0");
            jpeg.extend_from_slice(&exif);
            jpeg.extend_from_slice(&[0xff, 0xd9]);
            assert_eq!(read(&jpeg), Some(expected.clone()));
            let mut png = vec![];
            let encoder = png::Encoder::new(&mut png, 1, 1);
            let mut writer = encoder.write_header().unwrap();
            writer.write_chunk(png::chunk::eXIf, &exif).unwrap();
            writer.write_image_data(&[0]).unwrap();
            drop(writer);
            assert_eq!(read(&png), Some(expected));
        }
    }
    #[test]
    fn absent_invalid_thumbnail_and_ambiguous_tags_are_not_invented() {
        assert!(read(b"invalid").is_none());
        let mut fields = fields();
        fields[2].ifd_num = In::THUMBNAIL;
        fields[3].value = rat(1, 0);
        fields[4].value = rat(0, 1);
        fields[5].value = Value::Short(vec![65535]);
        let info = read(&tiff(&fields, true)).unwrap();
        assert!(
            info.lens.is_none()
                && info.exposure.is_none()
                && info.aperture.is_none()
                && info.iso.is_none()
        );
        fields.push(field(Tag::ISOSpeed, Value::Long(vec![102400])));
        fields[3].value = rat(16, 10);
        let info = read(&tiff(&fields, false)).unwrap();
        assert_eq!(info.iso.unwrap().text, "102400");
        assert_eq!(info.exposure.unwrap().text, "1.6 s");
        fields.push(field(Tag::Model, Value::Ascii(vec![b"conflict".to_vec()])));
        assert_eq!(
            read(&tiff(&fields, true)).unwrap().camera.unwrap().text,
            "Example"
        );
    }
    #[test]
    fn limits_offsets_and_lens_specification() {
        let mut fields = fields();
        fields[2] = field(
            Tag::LensSpecification,
            Value::Rational(vec![
                (24, 1).into(),
                (70, 1).into(),
                (28, 10).into(),
                (28, 10).into(),
            ]),
        );
        assert_eq!(
            read(&tiff(&fields, true)).unwrap().lens.unwrap().text,
            "24–70 mm f/2.8"
        );
        fields[2] = field(Tag::LensModel, Value::Ascii(vec![vec![b'x'; 129]]));
        assert!(read(&tiff(&fields, true)).unwrap().lens.is_none());
        let mut bytes = tiff(&fields, true);
        for length in 0..bytes.len() {
            if let Some(info) = read(&bytes[..length]) {
                info.validate().unwrap();
            }
        }
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&bytes).is_none());
        bytes.resize(MAX_BYTES + 1024, 0);
        bytes[4..8].copy_from_slice(&(MAX_BYTES as u32).to_le_bytes());
        assert!(read(&bytes).is_none());
    }
    #[test]
    #[cfg(any(target_os = "macos", windows))]
    #[ignore = "requires the native image backend outside the terminal sandbox"]
    fn worker_transports_original_tags_for_probe_and_raster() {
        use tr_core::protocol::{self, DecodeIntent, DecodeRequest, RasterInfo};
        let mut source = vec![];
        let encoder = png::Encoder::new(&mut source, 8, 4);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_chunk(png::chunk::eXIf, &tiff(&fields(), true))
            .unwrap();
        writer.write_image_data(&[0; 32]).unwrap();
        drop(writer);
        let mut requests = vec![];
        for (id, intent) in [
            DecodeIntent::Probe,
            DecodeIntent::LegacyRaster,
            DecodeIntent::ReferenceMip { cpu_threads: 1 },
        ]
        .into_iter()
        .enumerate()
        {
            protocol::write_control(
                &mut requests,
                protocol::REQUEST,
                id as u64,
                &DecodeRequest {
                    raw_wb: Default::default(),
                    raw_engine: Default::default(),
                    source_len: source.len(),
                    max_edge: 4,
                    intent,
                    edit: None,
                    maximum_output_bytes: 1024,
                },
            )
            .unwrap();
            requests.extend_from_slice(&source);
        }
        let mut responses = vec![];
        // In-process test of the already-authorized worker entry point. Production
        // external authorization still requires the OS-isolated broker transport.
        assert!(
            super::super::serve_with_policy(requests.as_slice(), &mut responses, true).is_err()
        );
        let mut reader = responses.as_slice();
        for id in 0..3 {
            let (kind, actual_id, control) = protocol::read_control(&mut reader).unwrap();
            assert_eq!(
                (kind, actual_id),
                (protocol::RESPONSE, id),
                "{}",
                String::from_utf8_lossy(&control)
            );
            let info: RasterInfo = protocol::parse(&control).unwrap();
            protocol::validate_info(&info).unwrap();
            assert_eq!(info.shooting, read(&source));
            if id > 0 {
                protocol::read_raster(&mut reader, &info).unwrap();
            }
        }
        assert!(reader.is_empty());
    }
}
