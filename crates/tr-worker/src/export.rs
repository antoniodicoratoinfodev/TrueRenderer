//! Encoders run in the same isolated worker as the decoder. No filesystem access.
//! This product includes DNG technology under license by Adobe.
use anyhow::{Result, ensure};
use image::ImageEncoder;
use std::io::{Cursor, Seek, SeekFrom, Write};
use tiff::{
    encoder::{Rational, SRational, TiffEncoder, colortype},
    tags::Tag,
};
use tr_core::{
    color::{self, LinearImage},
    export::{Compression, Format, Info, MAX_ENCODED, Options},
};

struct Bounded {
    data: Cursor<Vec<u8>>,
    limit: u64,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.data.position().saturating_add(bytes.len() as u64) > self.limit {
            return Err(std::io::Error::other("Export oltre quota"));
        }
        self.data.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Seek for Bounded {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let old = self.data.position();
        let next = self.data.seek(position)?;
        if next > self.limit {
            self.data.set_position(old);
            return Err(std::io::Error::other("Offset export oltre quota"));
        }
        Ok(next)
    }
}
fn encoded(pixel: color::Pixel) -> [f32; 4] {
    let alpha = pixel[3];
    let rgb = if alpha > 0. {
        [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha]
    } else {
        [0.; 3]
    };
    let rgb = color::rec2020_to_linear_srgb(rgb).map(color::linear_to_srgb);
    [rgb[0], rgb[1], rgb[2], alpha]
}
fn u16_sample(v: f32, clipped: &mut u64) -> u16 {
    if !(0. ..=1.).contains(&v) {
        *clipped += 1;
    }
    (v.clamp(0., 1.) * 65535.).round() as u16
}
fn rational(v: f32) -> Rational {
    Rational {
        n: (v as f64 * 1_000_000.).round() as u32,
        d: 1_000_000,
    }
}
fn srational(v: f32) -> SRational {
    SRational {
        n: (v as f64 * 1_000_000.).round() as i32,
        d: 1_000_000,
    }
}
fn tag(n: u16) -> Tag {
    Tag::from_u16_exhaustive(n)
}
fn profile(linear: bool) -> Result<Vec<u8>> {
    let mut profile = if linear {
        moxcms::ColorProfile::new_bt2020()
    } else {
        moxcms::ColorProfile::new_srgb()
    };
    profile.cicp = None; // Matrix/TRC RGB profile, not a video range/matrix declaration.
    profile.rendering_intent = moxcms::RenderingIntent::RelativeColorimetric;
    if linear {
        profile.red_trc = Some(moxcms::ToneReprCurve::Parametric(vec![1.]));
        profile.green_trc = profile.red_trc.clone();
        profile.blue_trc = profile.red_trc.clone();
        profile.description = None;
    }
    Ok(profile.encode()?)
}

fn dng_tags<W: Write + Seek>(
    dir: &mut tiff::encoder::DirectoryEncoder<'_, W, tiff::encoder::TiffKindStandard>,
    width: u32,
    height: u32,
    model: &str,
    matrix: [f32; 9],
    neutral: [f32; 3],
    orientation: u16,
) -> Result<()> {
    dir.write_tag(tag(50706), &[1u8, 1, 0, 0][..])?;
    dir.write_tag(tag(50707), &[1u8, 1, 0, 0][..])?;
    dir.write_tag(tag(50708), model)?;
    let (make, camera) = model.split_once(' ').unwrap_or(("TrueRenderer", model));
    dir.write_tag(Tag::Make, make)?;
    dir.write_tag(Tag::Model, camera)?;
    dir.write_tag(Tag::Software, "TrueRenderer")?;
    dir.write_tag(Tag::Orientation, orientation)?;
    dir.write_tag(tag(50721), &matrix.map(srational)[..])?;
    dir.write_tag(tag(50728), &neutral.map(rational)[..])?;
    dir.write_tag(tag(50778), 21u16)?; // D65 calibration
    dir.write_tag(tag(50718), &[rational(1.), rational(1.)][..])?;
    dir.write_tag(tag(50719), &[0u32, 0][..])?;
    dir.write_tag(tag(50720), &[width, height][..])?;
    dir.write_tag(Tag::NewSubfileType, 0u32)?;
    Ok(())
}

pub fn render(image: LinearImage, options: Options, limit: u64) -> Result<(Info, Vec<u8>)> {
    options.validate()?;
    ensure!(
        options.format != Format::DngRaw,
        "DNG RAW richiede il mosaico, non il render"
    );
    let image = if options.long_edge > 0 && image.width.max(image.height) > options.long_edge {
        image.reduced(options.long_edge)
    } else {
        image
    };
    let mut out = Bounded {
        data: Cursor::new(Vec::new()),
        limit: limit.min(MAX_ENCODED),
    };
    let mut clipped = 0;
    let description = match options.format {
        Format::Jpeg => {
            // JPEG has no alpha: composite onto explicitly declared white, in linear light.
            let mut pixels = Vec::with_capacity(image.pixels.len() * 3);
            for p in &image.pixels {
                let rgb = color::rec2020_to_linear_srgb([
                    p[0] + 1. - p[3],
                    p[1] + 1. - p[3],
                    p[2] + 1. - p[3],
                ]);
                for v in rgb.map(color::linear_to_srgb) {
                    pixels.push((u16_sample(v, &mut clipped) as f32 / 257.).round() as u8);
                }
            }
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, options.jpeg_quality);
            encoder.set_icc_profile(profile(false)?)?;
            // TIFF IFD0 -> Exif IFD, ColorSpace=1 (sRGB). No copied private EXIF/GPS.
            encoder.set_exif_metadata(vec![
                73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 105, 135, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0, 0, 0,
                0, 1, 0, 1, 160, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            ])?;
            encoder.encode(
                &pixels,
                image.width,
                image.height,
                image::ExtendedColorType::Rgb8,
            )?;
            "sRGB IEC 61966-2-1; JPEG 8 bit; alpha composto su bianco lineare; metadati privati non copiati"
        }
        Format::Png8 | Format::Png16 => {
            let sixteen = options.format == Format::Png16;
            let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(if sixteen {
                png::BitDepth::Sixteen
            } else {
                png::BitDepth::Eight
            });
            encoder.set_source_srgb(png::SrgbRenderingIntent::RelativeColorimetric);
            encoder.set_compression(match options.compression {
                Compression::Fast => png::Compression::Fast,
                Compression::Balanced => png::Compression::Balanced,
                Compression::Best => png::Compression::High,
            });
            let mut writer = encoder.write_header()?;
            let mut stream = writer.stream_writer()?;
            let mut row = Vec::with_capacity(image.width as usize * if sixteen { 8 } else { 4 });
            for pixels in image.pixels.chunks(image.width as usize) {
                row.clear();
                for pixel in pixels {
                    for (channel, value) in encoded(*pixel).into_iter().enumerate() {
                        let mut alpha_clipped = 0;
                        let v = u16_sample(
                            value,
                            if channel < 3 {
                                &mut clipped
                            } else {
                                &mut alpha_clipped
                            },
                        );
                        if sixteen {
                            row.extend_from_slice(&v.to_be_bytes());
                        } else {
                            row.push((v as f32 / 257.).round() as u8);
                        }
                    }
                }
                stream.write_all(&row)?;
            }
            stream.finish()?;
            writer.finish()?;
            "sRGB IEC 61966-2-1; alpha non associata; compressione senza perdita; gamut/estremi ritagliati"
        }
        Format::Tiff16 => {
            let mut encoder = TiffEncoder::new(&mut out)?;
            let mut output = encoder.new_image::<colortype::RGBA16>(image.width, image.height)?;
            output
                .encoder()
                .write_tag(Tag::IccProfile, &profile(false)?[..])?;
            output.encoder().write_tag(Tag::ExtraSamples, &[2u16][..])?;
            output.encoder().write_tag(Tag::Software, "TrueRenderer")?;
            output.rows_per_strip(1)?;
            let mut row = Vec::with_capacity(image.width as usize * 4);
            for pixels in image.pixels.chunks(image.width as usize) {
                row.clear();
                for pixel in pixels {
                    for (c, v) in encoded(*pixel).into_iter().enumerate() {
                        let mut ignored = 0;
                        row.push(u16_sample(
                            v,
                            if c == 3 { &mut ignored } else { &mut clipped },
                        ));
                    }
                }
                output.write_strip(&row)?;
            }
            output.finish()?;
            "TIFF sRGB ICC 16 bit; alpha non associata; non compresso; clamp [0,1]"
        }
        Format::TiffFloat32 => {
            let mut encoder = TiffEncoder::new(&mut out)?;
            let mut output =
                encoder.new_image::<colortype::RGBA32Float>(image.width, image.height)?;
            output
                .encoder()
                .write_tag(Tag::IccProfile, &profile(true)?[..])?;
            output.encoder().write_tag(Tag::ExtraSamples, &[1u16][..])?;
            output.encoder().write_tag(Tag::ImageDescription,"Extended linear Rec.2020 D65 RGBA float32; associated alpha in linear light; no RGB clamp. Developed image, not sensor RAW or scientific source data.")?;
            output.rows_per_strip(1)?;
            let mut row = Vec::with_capacity(image.width as usize * 4);
            for pixels in image.pixels.chunks(image.width as usize) {
                row.clear();
                for pixel in pixels {
                    row.extend_from_slice(pixel);
                }
                output.write_strip(&row)?;
            }
            output.finish()?;
            "TIFF Rec.2020 lineare ICC float32; alpha associata; negativi/oltre 1 conservati; non compresso; non RAW sensore"
        }
        Format::DngLinear16 => {
            ensure!(
                image.pixels.iter().all(|p| p[3] == 1.),
                "DNG lineare: trasparenza non supportata (usare PNG)"
            );
            let mut encoder = TiffEncoder::new(&mut out)?;
            let mut output = encoder.new_image::<colortype::RGB16>(image.width, image.height)?;
            dng_tags(
                output.encoder(),
                image.width,
                image.height,
                "TrueRenderer Linear Rec2020 v1",
                [
                    1.7166512,
                    -0.35567078,
                    -0.2533663,
                    -0.6666843,
                    1.6164812,
                    0.01576855,
                    0.01763986,
                    -0.04277061,
                    0.94210315,
                ],
                [1.; 3],
                1,
            )?;
            output
                .encoder()
                .write_tag(Tag::PhotometricInterpretation, 34892u16)?;
            output.encoder().write_tag(tag(50717), &[65535u32; 3][..])?;
            output.encoder().write_tag(Tag::ImageDescription, "Developed linear Rec.2020 D65 RGB; not sensor RAW. Clipped to [0,1], quantized to uint16. No private source metadata.")?;
            output.rows_per_strip(1)?;
            let mut row = Vec::with_capacity(image.width as usize * 3);
            for pixels in image.pixels.chunks(image.width as usize) {
                row.clear();
                for pixel in pixels {
                    for value in &pixel[..3] {
                        row.push(u16_sample(*value, &mut clipped));
                    }
                }
                output.write_strip(&row)?;
            }
            output.finish()?;
            "DNG lineare RGB Rec.2020 D65 16 bit; già sviluppato; non è il mosaico RAW; clamp [0,1]; senza metadati privati"
        }
        Format::DngRaw => unreachable!(),
    };
    let bytes = out.data.into_inner();
    let info = Info {
        format: options.format,
        width: image.width,
        height: image.height,
        bytes: bytes.len() as u64,
        clipped_channels: clipped,
        description: description.into(),
    };
    info.validate(options, limit)?;
    Ok((info, bytes))
}

#[cfg(any(windows, target_os = "macos"))]
pub fn raw(source: &[u8], options: Options, limit: u64) -> Result<(Info, Vec<u8>)> {
    options.validate()?;
    ensure!(
        options.format == Format::DngRaw,
        "Operazione mosaico non richiesta"
    );
    let mosaic = super::mosaic::export_mosaic(source)?;
    let mut out = Bounded {
        data: Cursor::new(Vec::new()),
        limit: limit.min(MAX_ENCODED),
    };
    let mut encoder = TiffEncoder::new(&mut out)?;
    let mut output = encoder.new_image::<colortype::Gray16>(mosaic.width, mosaic.height)?;
    dng_tags(
        output.encoder(),
        mosaic.width,
        mosaic.height,
        &mosaic.camera,
        mosaic.matrix,
        mosaic.neutral,
        mosaic.orientation,
    )?;
    let dir = output.encoder();
    dir.write_tag(Tag::PhotometricInterpretation, 32803u16)?;
    dir.write_tag(tag(33421), &[2u16, 2][..])?;
    dir.write_tag(tag(33422), &mosaic.cfa[..])?;
    dir.write_tag(tag(50713), &[2u16, 2][..])?;
    dir.write_tag(tag(50714), &mosaic.black.map(rational)[..])?;
    dir.write_tag(tag(50717), mosaic.white)?;
    dir.write_tag(Tag::ImageDescription, "Unscaled active-area sensor mosaic. LibRaw D65 camera-table calibration. Not an archival copy: optical margins, MakerNotes, EXIF/GPS and original compressed RAW are not included. Keep the original.")?;
    output.write_data(&mosaic.samples)?;
    let bytes = out.data.into_inner();
    let info = Info {format: Format::DngRaw, width: mosaic.width, height: mosaic.height, bytes: bytes.len() as u64, clipped_channels: 0,
        description: "Mosaico area attiva uint16 originale; CFA/nero/bianco/WB/orientamento; matrice D65 LibRaw; esclusi margini ottici, MakerNotes, EXIF/GPS e RAW compresso. Conservare l'originale.".into()};
    info.validate(options, limit)?;
    Ok((info, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn linear_dng_interoperates_with_libraw_without_demosaic() {
        let image = LinearImage::new(64, 32, vec![[0.2, 0.4, 0.6, 1.]; 2048]).unwrap();
        let (_, bytes) = render(
            image,
            Options {
                format: Format::DngLinear16,
                ..Options::default()
            },
            MAX_ENCODED,
        )
        .unwrap();
        for engine in [
            tr_core::decoder::RawEngine::LibRawBilinear,
            tr_core::decoder::RawEngine::LibRawAhd,
        ] {
            let (info, _, image) = crate::libraw::develop_with_engine(&bytes, 0, engine).unwrap();
            assert_eq!((info.width, info.height), (64, 32));
            assert!(info.decoder.contains("nessun demosaic"));
            for (value, expected) in image.pixels[100][..3].iter().zip([0.2, 0.4, 0.6]) {
                assert!(
                    (value - expected).abs() < 0.01,
                    "{engine:?}: {:?}",
                    image.pixels[100]
                );
            }
        }
    }
    #[test]
    fn float_tiff_preserves_extended_premultiplied_bits_and_icc() {
        let samples = vec![[-0.125, 0.25, 2.5, 0.5], [1e-20, 1000., -2., 1.]];
        let source = LinearImage::new(2, 1, samples.clone()).unwrap();
        let (info, bytes) = render(
            source,
            Options {
                format: Format::TiffFloat32,
                ..Options::default()
            },
            MAX_ENCODED,
        )
        .unwrap();
        assert_eq!(info.clipped_channels, 0);
        let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).unwrap();
        assert_eq!(decoder.get_tag_u16_vec(Tag::ExtraSamples).unwrap(), [1]);
        let profile = decoder
            .get_tag(Tag::IccProfile)
            .unwrap()
            .into_u32_vec()
            .unwrap()
            .into_iter()
            .map(|v| v as u8)
            .collect::<Vec<_>>();
        let profile = moxcms::ColorProfile::new_from_slice(&profile).unwrap();
        assert!(
            matches!(profile.red_trc,Some(moxcms::ToneReprCurve::Parametric(ref v)) if v==&vec![1.])
        );
        let tiff::decoder::DecodingResult::F32(decoded) = decoder.read_image().unwrap() else {
            panic!("not fp32")
        };
        assert_eq!(
            decoded.into_iter().map(f32::to_bits).collect::<Vec<_>>(),
            samples
                .into_iter()
                .flatten()
                .map(f32::to_bits)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn jpeg_quality_and_png_compression_are_different_controls() {
        let source = LinearImage::new(
            128,
            64,
            (0..8192)
                .map(|i| {
                    color::from_encoded_srgb([
                        (i % 251) as f32 / 250.,
                        (i % 199) as f32 / 198.,
                        (i % 97) as f32 / 96.,
                        1.,
                    ])
                })
                .collect(),
        )
        .unwrap();
        let (_, low) = render(
            source.clone(),
            Options {
                jpeg_quality: 25,
                ..Options::default()
            },
            MAX_ENCODED,
        )
        .unwrap();
        let (_, high) = render(
            source.clone(),
            Options {
                jpeg_quality: 95,
                ..Options::default()
            },
            MAX_ENCODED,
        )
        .unwrap();
        assert!(high.len() > low.len());
        let mut reference = None;
        for compression in [Compression::Fast, Compression::Balanced, Compression::Best] {
            let (_, bytes) = render(
                source.clone(),
                Options {
                    format: Format::Png16,
                    compression,
                    ..Options::default()
                },
                MAX_ENCODED,
            )
            .unwrap();
            let pixels = image::load_from_memory(&bytes)
                .unwrap()
                .to_rgba16()
                .into_raw();
            if let Some(reference) = &reference {
                assert_eq!(&pixels, reference);
            } else {
                reference = Some(pixels);
            }
        }
    }
    #[test]
    fn png16_retains_more_than_256_values_and_straight_alpha() {
        let image = LinearImage::new(
            1024,
            1,
            (0..1024)
                .map(|i| color::from_encoded_srgb([i as f32 / 1023., 0.5, 0.2, 0.5]))
                .collect(),
        )
        .unwrap();
        let (info, bytes) = render(
            image,
            Options {
                format: Format::Png16,
                ..Options::default()
            },
            MAX_ENCODED,
        )
        .unwrap();
        assert_eq!(info.width, 1024);
        let image = image::load_from_memory(&bytes).unwrap().to_rgba16();
        let distinct = image
            .pixels()
            .map(|p| p[0])
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct.len() > 1000);
        assert_eq!(image.get_pixel(500, 0)[3], 32768);
    }
    #[test]
    fn linear_dng_is_linear_rgb_not_cfa() {
        let source = LinearImage::new(
            2,
            1,
            vec![[-0.1, 0.5, 1.2, 1.], [0.12345, 0.23456, 0.34567, 1.]],
        )
        .unwrap();
        let (info, mut bytes) = render(
            source,
            Options {
                format: Format::DngLinear16,
                ..Options::default()
            },
            MAX_ENCODED,
        )
        .unwrap();
        assert_eq!(info.clipped_channels, 2);
        // Generic TIFF intentionally cannot interpret LinearRaw. Check the DNG
        // tag independently, then change only that tag in a private test copy
        // so the independent TIFF codec can verify actual uint16 sample storage.
        assert_eq!(&bytes[..4], b"II\x2a\0");
        let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
        let entry = (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|i| u16::from_le_bytes(bytes[*i..*i + 2].try_into().unwrap()) == 262)
            .unwrap();
        assert_eq!(
            u16::from_le_bytes(bytes[entry + 8..entry + 10].try_into().unwrap()),
            34892
        );
        bytes[entry + 8..entry + 10].copy_from_slice(&2u16.to_le_bytes());
        let mut reader = tiff::decoder::Decoder::new(Cursor::new(bytes)).unwrap();
        assert_eq!(
            reader.get_tag_u16_vec(Tag::BitsPerSample).unwrap(),
            [16, 16, 16]
        );
        assert_eq!(reader.get_tag_u32_vec(tag(50706)).unwrap(), [1, 1, 0, 0]);
        assert!(reader.find_tag(tag(33422)).unwrap().is_none());
        let tiff::decoder::DecodingResult::U16(samples) = reader.read_image().unwrap() else {
            panic!("expected uint16")
        };
        assert_eq!(samples, [0, 32768, 65535, 8090, 15372, 22653]);
    }
    #[test]
    fn output_quota_is_enforced_during_encoding() {
        let source = LinearImage::new(10, 10, vec![[0.5, 0.5, 0.5, 1.]; 100]).unwrap();
        assert!(render(source, Options::default(), 32).is_err());
    }
}
