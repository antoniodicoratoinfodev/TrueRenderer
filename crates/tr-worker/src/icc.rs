//! Bounded Windows RGB matrix/TRC input conversion, entirely inside the worker.
use anyhow::{Context, Result, ensure};
use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Flags, Intent, PixelFormat, Profile,
    ProfileClassSignature, ThreadContext, ToneCurve, Transform,
};

pub const DESCRIPTION: &str =
    "ICC RGB matrice/TRC · Little CMS 2.19 · relativo senza BPC → Rec.2020 lineare";
const MAX_PROFILE: usize = 4 * 1024 * 1024;

// Context must outlive the native transform (fields drop in declaration order).
pub struct Input {
    transform: Transform<[f32; 4], [f32; 4], ThreadContext>,
    extended_linear: bool,
    _context: ThreadContext,
}

fn u32be(bytes: &[u8], offset: usize) -> usize {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

fn validate(bytes: &[u8]) -> Result<bool> {
    ensure!(
        (132..=MAX_PROFILE).contains(&bytes.len()),
        "ICC: dimensione fuori quota (4 MiB)"
    );
    ensure!(
        u32be(bytes, 0) == bytes.len() && &bytes[36..40] == b"acsp",
        "ICC: header o lunghezza invalidi"
    );
    ensure!(
        matches!(bytes[8], 2 | 4),
        "ICC: supportate soltanto versioni 2 e 4"
    );
    let count = u32be(bytes, 128);
    ensure!(
        count <= 1024 && 132 + 12 * count <= bytes.len(),
        "ICC: tabella tag invalida"
    );
    let mut tags = std::collections::BTreeMap::new();
    let mut ranges = Vec::new();
    for i in 0..count {
        let entry = 132 + 12 * i;
        let key: [u8; 4] = bytes[entry..entry + 4].try_into().unwrap();
        let start = u32be(bytes, entry + 4);
        let len = u32be(bytes, entry + 8);
        ensure!(
            start >= 132 + 12 * count
                && len >= 8
                && start.checked_add(len).is_some_and(|end| end <= bytes.len()),
            "ICC: tag {key:?} fuori limite (offset {start}, lunghezza {len})"
        );
        ensure!(
            tags.insert(key, &bytes[start..start + len]).is_none(),
            "ICC: tag duplicato"
        );
        ranges.push((start, start + len));
        // Never ignore a higher-priority LUT in a nominal matrix profile.
        ensure!(
            !matches!(&key[..3], b"A2B" | b"B2A" | b"D2B" | b"B2D") && &key != b"cicp",
            "ICC: LUT/cICP non ancora supportati; nessun ripiego matrice"
        );
    }
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        ensure!(
            pair[0] == pair[1] || pair[0].1 <= pair[1].0,
            "ICC: tag sovrapposti"
        );
    }
    // Only exact analytic identity curves have a qualified unbounded domain.
    // Sampled curves, even identity tables, can clamp outside [0,1].
    Ok([*b"rTRC", *b"gTRC", *b"bTRC"].iter().all(|key| {
        tags.get(key).is_some_and(|tag| {
            (tag.len() >= 16
                && &tag[..4] == b"para"
                && tag[8..12] == [0; 4]
                && tag[12..16] == [0, 1, 0, 0])
                || (tag.len() >= 12
                    && &tag[..4] == b"curv"
                    && (u32be(tag, 8) == 0
                        || (tag.len() >= 14 && u32be(tag, 8) == 1 && tag[12..14] == [1, 0])))
        })
    }))
}

impl Input {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let extended_linear = validate(bytes)?;
        let context = ThreadContext::new();
        let source = Profile::new_icc_context(&context, bytes).context("ICC non valido")?;
        ensure!(
            source.color_space() == ColorSpaceSignature::RgbData,
            "ICC: richiesto profilo RGB; Gray/CMYK non ancora supportati"
        );
        ensure!(
            matches!(
                source.device_class(),
                ProfileClassSignature::InputClass
                    | ProfileClassSignature::DisplayClass
                    | ProfileClassSignature::OutputClass
                    | ProfileClassSignature::ColorSpaceClass
            ) && source.pcs() == ColorSpaceSignature::XYZData
                && source.is_matrix_shaper(),
            "ICC: richiesto profilo RGB matrice/TRC con PCS XYZ"
        );
        let xy = |x, y| CIExyY { x, y, Y: 1. };
        let linear = ToneCurve::new(1.);
        let working = Profile::new_rgb_context(
            &context,
            &xy(0.3127, 0.3290),
            &CIExyYTRIPLE {
                Red: xy(0.708, 0.292),
                Green: xy(0.170, 0.797),
                Blue: xy(0.131, 0.046),
            },
            &[&linear, &linear, &linear],
        )?;
        let transform = Transform::new_flags_context(
            &context,
            &source,
            PixelFormat::RGBA_FLT,
            &working,
            PixelFormat::RGBA_FLT,
            Intent::RelativeColorimetric,
            Flags::NO_OPTIMIZE | Flags::COPY_ALPHA,
        )?;
        Ok(Self {
            transform,
            extended_linear,
            _context: context,
        })
    }

    /// Straight, encoded RGBA in; premultiplied linear Rec.2020 out.
    pub fn apply(&self, pixels: &mut [[f32; 4]]) -> Result<()> {
        for p in pixels.iter_mut() {
            if p[3] == 0. {
                *p = [0.; 4];
            }
            ensure!(
                p.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&p[3]),
                "ICC: campione non finito o alpha invalida"
            );
            ensure!(
                self.extended_linear || p[..3].iter().all(|v| (0.0..=1.0).contains(v)),
                "ICC: campioni fuori [0,1] richiedono TRC lineare analitica; nessun clamp implicito"
            );
        }
        self.transform.transform_in_place(pixels);
        for p in pixels {
            for c in 0..3 {
                p[c] *= p[3];
            }
            ensure!(
                p.iter().all(|v| v.is_finite()),
                "ICC: trasformata non finita"
            );
        }
        Ok(())
    }
}

/// The codec conflates broken ICC chunk sequences with absent profiles. Assemble
/// APP2 explicitly, and check native components before any CMYK conversion.
pub fn jpeg_profile(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut at = 2;
    let mut components = None;
    let mut chunks = std::collections::BTreeMap::new();
    let mut total = None;
    let mut size = 0;
    while at + 1 < bytes.len() {
        ensure!(bytes[at] == 0xff, "JPEG ICC: marker invalido");
        while at < bytes.len() && bytes[at] == 0xff {
            at += 1;
        }
        let marker = *bytes.get(at).context("JPEG ICC troncato")?;
        at += 1;
        if matches!(marker, 0xda | 0xd9) {
            break;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        ensure!(at + 2 <= bytes.len(), "JPEG ICC troncato");
        let len = u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
        ensure!(
            len >= 2 && at + len <= bytes.len(),
            "JPEG ICC: segmento invalido"
        );
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            ensure!(len >= 8, "JPEG ICC: SOF troncato");
            components = Some(bytes[at + 7]);
        }
        let payload = &bytes[at + 2..at + len];
        if marker == 0xe2 && payload.starts_with(b"ICC_PROFILE\0") {
            ensure!(payload.len() > 14, "JPEG ICC: APP2 troncato/vuoto");
            let (seq, count) = (payload[12], payload[13]);
            ensure!(seq > 0 && seq <= count, "JPEG ICC: sequenza invalida");
            ensure!(
                total.is_none_or(|n| n == count),
                "JPEG ICC: conteggi discordanti"
            );
            total = Some(count);
            size += payload.len() - 14;
            ensure!(size <= MAX_PROFILE, "JPEG ICC: profilo oltre 4 MiB");
            ensure!(
                chunks.insert(seq, &payload[14..]).is_none(),
                "JPEG ICC: segmento duplicato"
            );
        }
        at += len;
    }
    let Some(total) = total else {
        return Ok(None);
    };
    ensure!(
        chunks.len() == usize::from(total),
        "JPEG ICC: segmenti mancanti"
    );
    ensure!(
        components == Some(3),
        "JPEG ICC: richiesti tre componenti RGB/YCbCr; Gray/CMYK non supportati"
    );
    Ok(Some(
        chunks.values().flat_map(|v| v.iter().copied()).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn xy(x: f64, y: f64) -> CIExyY {
        CIExyY { x, y, Y: 1. }
    }
    fn png_profile(profile: Vec<u8>, srgb: bool, cicp: bool) -> Vec<u8> {
        let mut info = png::Info::with_size(2, 1);
        info.color_type = png::ColorType::Rgba;
        info.bit_depth = png::BitDepth::Sixteen;
        info.icc_profile = Some(Cow::Owned(profile));
        let mut bytes = vec![];
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        // The encoder deliberately omits iCCP when info.srgb is set. Write
        // the conflicting chunk explicitly to actually test both declarations.
        if srgb {
            writer.write_chunk(png::chunk::sRGB, &[1]).unwrap();
        }
        // png 0.18 parses cICP but does not serialize it from Info.
        if cicp {
            writer
                .write_chunk(png::chunk::cICP, &[1, 13, 0, 1])
                .unwrap();
        }
        writer
            .write_image_data(&[
                0x80, 0, 0x40, 0, 0x20, 0, 0x80, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0,
            ])
            .unwrap();
        writer.finish().unwrap();
        bytes
    }

    #[test]
    fn png_icc_applies_profile_to_16bit_alpha_and_rejects_conflicting_declarations() {
        let profile = Profile::new_srgb().icc().unwrap();
        let bytes = png_profile(profile.clone(), false, false);
        let (probe, declared, _) = crate::portable_decode(&bytes, 0, false).unwrap();
        assert!(matches!(
            declared,
            tr_core::decoder::ColorSource::Declared(_)
        ));
        let (info, _, raster) = crate::portable_decode(&bytes, 0, true).unwrap();
        assert_eq!(info.input_color, probe.input_color);
        let expected = tr_core::color::from_encoded_srgb([
            32768. / 65535.,
            16384. / 65535.,
            8192. / 65535.,
            32768. / 65535.,
        ]);
        let raster = raster.unwrap();
        for (a, b) in raster.pixels[0].iter().zip(expected) {
            assert!((a - b).abs() < 0.0001);
        }
        assert_eq!(raster.pixels[1], [0.; 4]);
        assert_eq!(
            crate::portable_decode(&bytes, 1, true)
                .unwrap()
                .2
                .unwrap()
                .width,
            1
        );
        for (srgb, cicp) in [(true, false), (false, true)] {
            let bytes = png_profile(profile.clone(), srgb, cicp);
            for raster in [false, true] {
                assert!(
                    crate::portable_decode(&bytes, 0, raster).is_err(),
                    "srgb={srgb}, cicp={cicp}, raster={raster}"
                );
            }
        }
    }

    #[test]
    fn rgb_spaces_v2_v4_match_independent_chromaticity_matrices_and_curves() {
        // Analytic primaries/Bradford reference, independent of Little CMS.
        // Profiles cover sRGB primaries, Adobe RGB, Display P3 and ProPhoto.
        use moxcms::{ColorProfile, Matrix3d, XyY};
        let xyz = |p: CIExyY| {
            XyY {
                x: p.x,
                y: p.y,
                yb: 1.,
            }
            .to_xyzd()
        };
        let matrix = |w: CIExyY, p: CIExyYTRIPLE| {
            let [r, g, b] = [p.Red, p.Green, p.Blue].map(xyz);
            ColorProfile::rgb_to_xyz_d(
                Matrix3d {
                    v: [[r.x, g.x, b.x], [r.y, g.y, b.y], [r.z, g.z, b.z]],
                },
                xyz(w),
            )
        };
        let d65 = xy(0.3127, 0.3290);
        let working = matrix(
            d65,
            CIExyYTRIPLE {
                Red: xy(0.708, 0.292),
                Green: xy(0.170, 0.797),
                Blue: xy(0.131, 0.046),
            },
        );
        for (white, primaries, gamma) in [
            (d65, [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]], 2.2),
            (d65, [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]], 563. / 256.),
            (d65, [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]], 0.),
            (
                xy(0.3457, 0.3585),
                [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]],
                1.8,
            ),
        ] {
            let p = CIExyYTRIPLE {
                Red: xy(primaries[0][0], primaries[0][1]),
                Green: xy(primaries[1][0], primaries[1][1]),
                Blue: xy(primaries[2][0], primaries[2][1]),
            };
            let curve = if gamma == 0. {
                ToneCurve::new_parametric(4, &[2.4, 1. / 1.055, 0.055 / 1.055, 1. / 12.92, 0.04045])
                    .unwrap()
            } else {
                ToneCurve::new(gamma)
            };
            for version in [2.1, 4.3] {
                let mut profile = Profile::new_rgb(&white, &p, &[&curve, &curve, &curve]).unwrap();
                profile.set_version(version);
                let encoded = profile.icc().unwrap();
                let transform = Input::new(&encoded).unwrap();
                // Compare the serialized profile, including v2 curve/table
                // quantization, via an independent parser and evaluator.
                let tag = |name: &[u8]| {
                    let entry = (0..u32be(&encoded, 128))
                        .map(|i| 132 + 12 * i)
                        .find(|&at| &encoded[at..at + 4] == name)
                        .unwrap();
                    &encoded[u32be(&encoded, entry + 4)..][..u32be(&encoded, entry + 8)]
                };
                let fixed =
                    |bytes: &[u8]| i32::from_be_bytes(bytes.try_into().unwrap()) as f64 / 65536.;
                let colorants = [b"rXYZ", b"gXYZ", b"bXYZ"].map(|name| {
                    let data = tag(name);
                    [8, 12, 16].map(|at| fixed(&data[at..at + 4]))
                });
                let source_matrix = Matrix3d {
                    v: [0, 1, 2].map(|row| [0, 1, 2].map(|col| colorants[col][row])),
                };
                let trc = tag(b"rTRC");
                let curve = |v: f32| -> f64 {
                    let v = f64::from(v);
                    if &trc[..4] == b"curv" {
                        let n = u32be(trc, 8);
                        let value = |i: usize| {
                            u16::from_be_bytes(trc[12 + 2 * i..14 + 2 * i].try_into().unwrap())
                                as f64
                        };
                        if n == 0 {
                            v
                        } else if n == 1 {
                            v.powf(value(0) / 256.)
                        } else {
                            let pos = v * (n - 1) as f64;
                            let lo = (pos.floor() as usize).min(n - 1);
                            let hi = (lo + 1).min(n - 1);
                            (value(lo) + (value(hi) - value(lo)) * (pos - lo as f64)) / 65535.
                        }
                    } else {
                        let g = fixed(&trc[12..16]);
                        if trc[8..10] == [0, 0] {
                            v.powf(g)
                        } else {
                            let [a, b, c, d] = [16, 20, 24, 28].map(|at| fixed(&trc[at..at + 4]));
                            if v >= d { (a * v + b).powf(g) } else { c * v }
                        }
                    }
                };
                let pcs = moxcms::Xyzd {
                    x: 0.9642,
                    y: 1.,
                    z: 0.8249,
                };
                let expected_matrix = working
                    .inverse()
                    .mat_mul_const(moxcms::adaption_matrix_d(pcs.to_xyz(), xyz(d65).to_xyz()))
                    .mat_mul_const(source_matrix)
                    .v;
                let samples: Vec<_> = (0..17 * 17 * 17)
                    .map(|i| {
                        [
                            (i % 17) as f32 / 16.,
                            ((i / 17) % 17) as f32 / 16.,
                            (i / 289) as f32 / 16.,
                            0.7,
                        ]
                    })
                    .collect();
                let mut actual = samples.clone();
                transform.apply(&mut actual).unwrap();
                for (a, b) in actual.iter().zip(samples) {
                    let rgb = [b[0], b[1], b[2]].map(curve);
                    for c in 0..3 {
                        let expected = (0..3).map(|i| expected_matrix[c][i] * rgb[i]).sum::<f64>()
                            * f64::from(b[3]);
                        assert!(
                            (f64::from(a[c]) - expected).abs() < 0.00025,
                            "v{version}, gamma {gamma}: {a:?} vs {expected}"
                        );
                    }
                    assert_eq!(a[3], b[3]);
                }
            }
        }
    }

    #[test]
    fn malformed_or_unsupported_icc_is_never_assumed_srgb() {
        let good = Profile::new_srgb().icc().unwrap();
        let mut cases = vec![vec![], vec![0; MAX_PROFILE + 1], good[..100].to_vec()];
        for (offset, value) in [
            (0, u32::MAX),
            (128, u32::MAX),
            (136, u32::MAX),
            (140, u32::MAX),
        ] {
            let mut bad = good.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            cases.push(bad);
        }
        for (offset, replacement) in [(16, *b"CMYK"), (12, *b"link"), (132, *b"A2B0")] {
            let mut bad = good.clone();
            bad[offset..offset + 4].copy_from_slice(&replacement);
            cases.push(bad);
        }
        let mut duplicate = good.clone();
        duplicate[144..148].copy_from_slice(&good[132..136]);
        cases.push(duplicate);
        cases.push(
            Profile::new_gray(&xy(0.3457, 0.3585), &ToneCurve::new(2.2))
                .unwrap()
                .icc()
                .unwrap(),
        );
        for bytes in cases {
            assert!(Input::new(&bytes).is_err());
            let png = png_profile(bytes, false, false);
            for raster in [false, true] {
                assert!(crate::portable_decode(&png, 0, raster).is_err());
            }
        }
    }

    #[test]
    fn jpeg_icc_chunks_are_complete_unique_and_match_native_channels() {
        use crate::export;
        use tr_core::export::{Format, Options};
        let image = tr_core::color::LinearImage::new(1, 1, vec![[0.2, 0.3, 0.4, 1.]]).unwrap();
        let (_, good) = export::render(
            image,
            Options {
                format: Format::Jpeg,
                ..Options::default()
            },
            1024 * 1024,
        )
        .unwrap();
        let signature = good
            .windows(12)
            .position(|v| v == b"ICC_PROFILE\0")
            .unwrap();
        let start = signature - 4;
        let len = u16::from_be_bytes(good[start + 2..start + 4].try_into().unwrap()) as usize;
        let profile = jpeg_profile(&good).unwrap().unwrap();
        Input::new(&profile).unwrap();
        let mut cases = vec![];
        for (offset, value) in [(12, 0), (13, 2)] {
            let mut bad = good.clone();
            bad[signature + offset] = value;
            cases.push(bad);
        }
        let mut duplicate = good.clone();
        duplicate.splice(start..start, good[start..start + 2 + len].iter().copied());
        cases.push(duplicate);
        let mut truncated = good.clone();
        truncated.drain(signature + 14..start + 2 + len);
        truncated[start + 2..start + 4].copy_from_slice(&16u16.to_be_bytes());
        cases.push(truncated);
        for bad in cases {
            assert!(jpeg_profile(&bad).is_err());
            for raster in [false, true] {
                assert!(crate::portable_decode(&bad, 0, raster).is_err());
            }
        }
        // APP2 ordering is not significant; sequence numbers determine order.
        let half = profile.len() / 2;
        let mut split = good.clone();
        let mut segments = vec![];
        for (seq, part) in [(2u8, &profile[half..]), (1, &profile[..half])] {
            segments.extend_from_slice(&[0xff, 0xe2]);
            segments.extend_from_slice(&((part.len() + 16) as u16).to_be_bytes());
            segments.extend_from_slice(b"ICC_PROFILE\0");
            segments.extend_from_slice(&[seq, 2]);
            segments.extend_from_slice(part);
        }
        split.splice(start..start + len + 2, segments);
        assert_eq!(jpeg_profile(&split).unwrap().unwrap(), profile);
        crate::portable_decode(&split, 0, true).unwrap();
        // A native four-component JPEG must not be accepted using an RGB ICC.
        let sof = good
            .windows(2)
            .position(|v| matches!(v, [0xff, 0xc0..=0xc2]))
            .unwrap();
        let mut cmyk = good;
        cmyk[sof + 9] = 4;
        assert!(jpeg_profile(&cmyk).is_err());
    }

    #[test]
    fn tiff_icc_preserves_alpha_semantics_and_metadata_errors() {
        use tiff::{
            encoder::{TiffEncoder, colortype::RGBA16},
            tags::Tag,
        };
        let profile = Profile::new_srgb().icc().unwrap();
        for associated in [1u16, 2] {
            let mut bytes = vec![];
            let samples: Vec<u16> = [0u16, 1, 32768, 65534, 65535]
                .into_iter()
                .flat_map(|alpha| {
                    let rgb = if associated == 1 { alpha } else { 65535 };
                    [rgb, rgb, rgb, alpha]
                })
                .collect();
            {
                let mut encoder = TiffEncoder::new(std::io::Cursor::new(&mut bytes)).unwrap();
                let mut image = encoder.new_image::<RGBA16>(5, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::ExtraSamples, &[associated][..])
                    .unwrap();
                image
                    .encoder()
                    .write_tag(Tag::IccProfile, profile.as_slice())
                    .unwrap();
                image.write_data(&samples).unwrap();
            }
            let (_, _, image) = crate::portable_decode(&bytes, 0, true).unwrap();
            for (pixel, alpha) in image
                .unwrap()
                .pixels
                .iter()
                .zip([0u16, 1, 32768, 65534, 65535])
            {
                let alpha = f32::from(alpha) / 65535.;
                for channel in pixel {
                    assert!((channel - alpha).abs() < 0.0002);
                }
            }
            let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
            let entry = (0..count)
                .map(|i| ifd + 2 + 12 * i)
                .find(|&at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) == 34675)
                .unwrap();
            bytes[entry + 8..entry + 12].copy_from_slice(&u32::MAX.to_le_bytes());
            for raster in [false, true] {
                assert!(crate::portable_decode(&bytes, 0, raster).is_err());
            }
        }
    }

    #[test]
    fn nonlinear_and_sampled_curves_reject_extended_input_without_clipping() {
        let transform = Input::new(&Profile::new_srgb().icc().unwrap()).unwrap();
        for pixel in [
            [-0.01, 0.2, 0.3, 1.],
            [1.01, 0.2, 0.3, 1.],
            [f32::NAN, 0., 0., 1.],
            [0., 0., 0., 2.],
        ] {
            assert!(transform.apply(&mut [pixel]).is_err());
        }
        // Associated versus straight alpha is tested at the container boundary;
        // the ICC operation itself never transforms the alpha channel.
        let mut pixels = [[2., -1., 3., 0.]];
        transform.apply(&mut pixels).unwrap();
        assert_eq!(pixels, [[0.; 4]]);
        let mut profile = Profile::new_srgb();
        let table = ToneCurve::new_tabulated(&[0, 32768, 65535]);
        for tag in [
            lcms2::TagSignature::RedTRCTag,
            lcms2::TagSignature::GreenTRCTag,
            lcms2::TagSignature::BlueTRCTag,
        ] {
            assert!(profile.write_tag(tag, lcms2::Tag::ToneCurve(&table)));
        }
        let transform = Input::new(&profile.icc().unwrap()).unwrap();
        assert!(transform.apply(&mut [[-0.1, 0.2, 1.2, 1.]]).is_err());
    }
}
