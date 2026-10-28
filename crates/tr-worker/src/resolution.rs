//! Physical density from the original container, inside the isolated decoder.
//! No fabricated default for absent/invalid metadata or unitless aspect ratios.
use exif::{In, Tag, Value};
use std::io::Cursor;
use tr_core::protocol::Resolution;
const MAX_METADATA: usize = 4 * 1024 * 1024;
pub fn read(bytes: &[u8]) -> Option<Resolution> {
    let mut orientation = 1;
    let mut resolution = None;
    if (super::container::is_tiff_container(bytes)
        || bytes.starts_with(b"\xff\xd8")
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        && let Ok(exif) = exif::Reader::new()
            .read_from_container(&mut Cursor::new(&bytes[..bytes.len().min(MAX_METADATA)]))
        && exif.fields().count() <= 512
    {
        let value = |tag| {
            let mut fields = exif
                .fields()
                .filter(|f| f.tag == tag && f.ifd_num == In::PRIMARY);
            let field = fields.next()?;
            fields.next().is_none().then_some(&field.value)
        };
        let rational = |tag| match value(tag)? {
            Value::Rational(v) if v.len() == 1 && v[0].denom > 0 => Some(v[0].to_f64()),
            _ => None,
        };
        orientation = value(Tag::Orientation)
            .and_then(|v| v.get_uint(0))
            .unwrap_or(1);
        if let Some(unit) = value(Tag::ResolutionUnit).and_then(|v| v.get_uint(0))
            && let (Some(x), Some(y)) = (rational(Tag::XResolution), rational(Tag::YResolution))
        {
            let factor = match unit {
                2 => Some(1.),
                3 => Some(2.54),
                _ => None,
            };
            resolution = factor
                .map(|f| Resolution { x: x * f, y: y * f })
                .filter(|r| r.validate().is_ok());
        }
    }
    let mut r = resolution.or_else(|| container_density(bytes))?;
    if (5..=8).contains(&orientation) {
        std::mem::swap(&mut r.x, &mut r.y);
    }
    r.validate().ok()?;
    Some(r)
}
fn container_density(bytes: &[u8]) -> Option<Resolution> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_limits(png::Limits {
            bytes: MAX_METADATA,
        });
        let reader = decoder.read_info().ok()?;
        let dims = reader.info().pixel_dims?;
        return (dims.unit == png::Unit::Meter).then_some(Resolution {
            x: f64::from(dims.xppu) * 0.0254,
            y: f64::from(dims.yppu) * 0.0254,
        });
    }
    if !bytes.starts_with(b"\xff\xd8") {
        return None;
    }
    let mut i = 2;
    while i + 4 <= bytes.len().min(MAX_METADATA) {
        if bytes[i] != 0xff {
            return None;
        }
        while bytes.get(i) == Some(&0xff) {
            i += 1;
        }
        let marker = *bytes.get(i)?;
        i += 1;
        if matches!(marker, 0xda | 0xd9) {
            break;
        }
        let length = u16::from_be_bytes(bytes.get(i..i + 2)?.try_into().ok()?) as usize;
        if length < 2 {
            return None;
        }
        let segment = bytes.get(i + 2..i + length)?;
        if marker == 0xe0 && segment.len() >= 12 && segment.starts_with(b"JFIF\0") {
            let factor = match segment[7] {
                1 => 1.,
                2 => 2.54,
                _ => return None,
            };
            let x = u16::from_be_bytes([segment[8], segment[9]]) as f64;
            let y = u16::from_be_bytes([segment[10], segment[11]]) as f64;
            return Some(Resolution {
                x: x * factor,
                y: y * factor,
            });
        }
        i += length;
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    use exif::{Field, Rational, experimental::Writer};
    #[test]
    fn tiff_raw_primary_resolution_preserves_axes_units_and_orientation() {
        for unit in [1, 2, 3] {
            for orientation in [1, 6] {
                let fields = [
                    Field {
                        tag: Tag::XResolution,
                        ifd_num: In::PRIMARY,
                        value: Value::Rational(vec![Rational { num: 300, denom: 1 }]),
                    },
                    Field {
                        tag: Tag::YResolution,
                        ifd_num: In::PRIMARY,
                        value: Value::Rational(vec![Rational { num: 150, denom: 1 }]),
                    },
                    Field {
                        tag: Tag::ResolutionUnit,
                        ifd_num: In::PRIMARY,
                        value: Value::Short(vec![unit]),
                    },
                    Field {
                        tag: Tag::Orientation,
                        ifd_num: In::PRIMARY,
                        value: Value::Short(vec![orientation]),
                    },
                ];
                let mut writer = Writer::new();
                for f in &fields {
                    writer.push_field(f);
                }
                let mut bytes = Cursor::new(vec![]);
                writer.write(&mut bytes, true).unwrap();
                let actual = read(bytes.get_ref());
                if unit == 1 {
                    assert_eq!(actual, None);
                } else {
                    let factor = if unit == 2 { 1. } else { 2.54 };
                    let [x, y] = if orientation == 1 {
                        [300., 150.]
                    } else {
                        [150., 300.]
                    };
                    assert_eq!(
                        actual,
                        Some(Resolution {
                            x: x * factor,
                            y: y * factor
                        })
                    );
                }
            }
        }
    }
    #[test]
    fn jpeg_jfif_and_png_phys_unknown_remains_absent() {
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        jpeg.extend_from_slice(b"JFIF\0\x01\x02\x01\x01\x2c\x00\x96\0\0");
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        assert_eq!(read(&jpeg), Some(Resolution { x: 300., y: 150. }));
        jpeg[13] = 0;
        assert_eq!(read(&jpeg), None);
        for unit in [png::Unit::Meter, png::Unit::Unspecified] {
            let mut bytes = vec![];
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_pixel_dims(Some(png::PixelDimensions {
                xppu: 11811,
                yppu: 5906,
                unit,
            }));
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0]).unwrap();
            writer.finish().unwrap();
            let r = read(&bytes);
            if unit == png::Unit::Meter {
                let r = r.unwrap();
                assert!((r.x - 300.).abs() < 0.02);
                assert!((r.y - 150.).abs() < 0.02);
            } else {
                assert_eq!(r, None);
            }
        }
        assert_eq!(read(b"missing"), None);
    }
}
