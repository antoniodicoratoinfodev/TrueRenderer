//! Exclusive adapter for the autonomous core. No legacy RAW adapter imports.
use anyhow::{Result, ensure};
use tr_core::{
    color::LinearImage,
    decoder::{ColorSource, Decoder, RawEngine, RawWhiteBalance},
    protocol::{self, RasterInfo},
};

pub(crate) struct Experimental;
#[cfg(test)]
#[path = "../../tr-raw/tests/support/mod.rs"]
mod fixture;

fn bitmap(bytes: &[u8]) -> Result<bool> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(b"\xff\xd8\xff") {
        return Ok(true);
    }
    if bytes.starts_with(b"BM")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
    {
        return Ok(true);
    }
    if matches!(bytes.get(..4), Some(b"II\x2a\x00") | Some(b"MM\x00\x2a")) {
        return tr_raw::is_bitmap_tiff(bytes);
    }
    Ok(false)
}

fn describe(raw: tr_raw::RawInfo) -> Result<(RasterInfo, ColorSource)> {
    // The broker reserves at least 48 bytes/output-pixel + 128 MiB for its full
    // decode slot. Admit this computed sensor plan before any raster allocation;
    // extreme sensor-to-crop ratios are refused until IPC has separate credits.
    ensure!(
        raw.memory.scratch_peak_bytes as u64
            <= raw.memory.output_pixels as u64 * 48 + 128 * 1024 * 1024,
        "Experimental: mosaico/crop oltre il budget di memoria del job"
    );
    let color = ColorSource::Developed(format!(
        "Rec.2020 fp32; profilo {} {}; mat2; alte luci HL7 Bayer/HL6 X-Trans + ripiego neutro sensor98-100; no clamp",
        raw.profile_source, raw.profile_hash
    ));
    let mut camera = raw.camera;
    camera.truncate(camera.floor_char_boundary(75));
    let info = RasterInfo {
        resolution: None,
        shooting: None,
        scientific: None,
        reference_mip: None,
        width: raw.width,
        height: raw.height,
        source_width: raw.width,
        source_height: raw.height,
        native_bits: 0,
        format: "RAW".into(),
        decoder: format!("{} · {camera}", tr_raw::RECIPE),
        input_color: color.provenance(),
        filter: "nessuno (campioni LOD 0)".into(),
        orientation: format!(
            "{} EXIF {} applicato una volta da Experimental",
            raw.format, raw.orientation
        ),
    };
    protocol::validate_info(&info)?;
    Ok((info, color))
}

pub fn probe_with_wb(bytes: &[u8], wb: RawWhiteBalance) -> Result<(RasterInfo, ColorSource)> {
    wb.validate_for(RawEngine::TrueRendererExperimental)?;
    let (mut info, mut color) = describe(tr_raw::probe_with_wb(bytes, wb.gains_f64())?)?;
    if !wb.is_as_shot() {
        let ColorSource::Developed(description) = color else {
            unreachable!()
        };
        color = ColorSource::Developed(format!(
            "{description}; WB relativo {:.6}/1/{:.6}",
            wb.gains_f64()[0],
            wb.gains_f64()[2]
        ));
        info.input_color = color.provenance();
    }
    protocol::validate_info(&info)?;
    Ok((info, color))
}

pub fn develop_with_wb(
    bytes: &[u8],
    max_edge: u32,
    wb: RawWhiteBalance,
) -> Result<(RasterInfo, ColorSource, LinearImage)> {
    let (mut info, color) = probe_with_wb(bytes, wb)?;
    let image = tr_raw::develop(bytes, wb.gains_f64())?;
    let image = if max_edge > 0 && image.width.max(image.height) > max_edge {
        image.reduced(max_edge)
    } else {
        image
    };
    info.width = image.width;
    info.height = image.height;
    if (info.width, info.height) != (info.source_width, info.source_height) {
        info.filter = tr_core::color::FILTER_VERSION.into();
    }
    Ok((info, color, image))
}

pub fn estimate_white_balance(
    bytes: &[u8],
    analysis: tr_core::raw_wb::Analysis,
) -> Result<RawWhiteBalance> {
    probe_with_wb(bytes, RawWhiteBalance::default())?;
    tr_raw::estimate_white_balance(bytes, analysis)
}

impl Decoder for Experimental {
    fn name(&self) -> &'static str {
        tr_raw::RECIPE
    }
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
        if bitmap(bytes)? {
            #[cfg(target_os = "macos")]
            {
                let result = super::Apple.probe(bytes)?;
                ensure!(
                    result.0.format != "RAW",
                    "Experimental: contenitore RAW fuori contratto"
                );
                return Ok(result);
            }
            #[cfg(windows)]
            {
                let (info, color, _) = super::portable_decode(bytes, 0, false)?;
                return Ok((info, color));
            }
        }
        probe_with_wb(bytes, RawWhiteBalance::default())
    }
    fn decode(
        &self,
        bytes: &[u8],
        max_edge: u32,
    ) -> Result<(RasterInfo, ColorSource, LinearImage)> {
        if bitmap(bytes)? {
            #[cfg(target_os = "macos")]
            {
                let info = super::Apple.probe(bytes)?.0;
                ensure!(
                    info.format != "RAW",
                    "Experimental: contenitore RAW fuori contratto"
                );
                return super::Apple.decode(bytes, max_edge);
            }
            #[cfg(windows)]
            {
                let (info, color, image) = super::portable_decode(bytes, max_edge, true)?;
                return Ok((info, color, image.expect("requested raster")));
            }
        }
        develop_with_wb(bytes, max_edge, RawWhiteBalance::default())
    }
}

pub fn export_raw(
    source: &[u8],
    options: tr_core::export::Options,
    limit: u64,
) -> Result<(tr_core::export::Info, Vec<u8>)> {
    use tr_core::export::{Format, Info, MAX_ENCODED};
    options.validate()?;
    ensure!(
        options.format == Format::DngRaw,
        "Experimental: richiesto export mosaico"
    );
    probe_with_wb(source, RawWhiteBalance::default())?;
    let (width, height, bytes) = tr_raw::repack_dng(source, limit.min(MAX_ENCODED) as usize)?;
    let info=Info {format:Format::DngRaw,width,height,bytes:bytes.len() as u64,clipped_channels:0,
        description:"Experimental: campioni sensore decodificati uint16; calibrazione, LUT, nero, margini e crop conservati. La quantizzazione lossy in camera non è reversibile. WB/demosaic non applicati; EXIF/MakerNotes/preview esclusi. Conservare l'originale.".into()};
    info.validate(options, limit)?;
    Ok((info, bytes))
}

#[cfg(test)]
pub(crate) mod legacy_guard {
    use std::cell::Cell;
    thread_local! {static FORBIDDEN: Cell<bool> = const {Cell::new(false)};}
    pub(crate) fn check() {
        assert!(
            !FORBIDDEN.get(),
            "Experimental called a legacy LibRaw boundary"
        );
    }
    pub struct Guard;
    impl Guard {
        pub fn new() -> Self {
            assert!(!FORBIDDEN.replace(true));
            Self
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            FORBIDDEN.set(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raf_recognition_and_truncated_codec_never_call_libraw() {
        let _guard = legacy_guard::Guard::new();
        let bytes = fixture::raf::bytes();
        Experimental.probe(&bytes).unwrap();
        assert!(Experimental.decode(&bytes, 32).is_err());
        assert!(estimate_white_balance(&bytes, tr_core::raw_wb::Analysis::Auto).is_err());
        let options = tr_core::export::Options {
            format: tr_core::export::Format::DngRaw,
            ..Default::default()
        };
        assert!(export_raw(&bytes, options, 128 * 1024 * 1024).is_err());
    }
    #[test]
    fn direct_nef_all_entry_points_never_call_libraw() {
        let _guard = legacy_guard::Guard::new();
        let engine = RawEngine::TrueRendererExperimental;
        assert_eq!(engine.label(), "trueRendererExperimental 32fp");
        let decoder =
            super::super::select_engine(tr_core::decoder::Trust::External, engine).unwrap();
        let mut fixture = fixture::nef::NefFixture::new();
        let bytes = fixture.bytes();
        decoder.probe(&bytes).unwrap();
        let wb = RawWhiteBalance {
            red: 1200,
            blue: 900,
            ..Default::default()
        };
        let balanced = super::super::WhiteBalanced {
            base: decoder,
            engine,
            wb,
        };
        let (probe, _) = balanced.probe(&bytes).unwrap();
        let (info, _, raster) = balanced.decode(&bytes, 8).unwrap();
        assert_eq!(probe.input_color, info.input_color);
        assert_eq!((raster.width, raster.height), (8, 5));
        let options = tr_core::export::Options {
            format: tr_core::export::Format::DngRaw,
            ..Default::default()
        };
        let (_, dng) = export_raw(&bytes, options, 128 * 1024 * 1024).unwrap();
        assert_eq!(balanced.decode(&dng, 8).unwrap().2.pixels, raster.pixels);
        // A brighter constant plane gives Auto sufficient valid neutral samples.
        for i in 0..4 {
            fixture.maker.tags.get_mut(&0x96).unwrap().2[2 + i * 2..4 + i * 2]
                .copy_from_slice(&4000u16.to_le_bytes());
        }
        let neutral = fixture.bytes();
        for analysis in [
            tr_core::raw_wb::Analysis::Auto,
            tr_core::raw_wb::Analysis::Patch {
                x: 32,
                y: 32,
                side: 5,
            },
        ] {
            let direct = estimate_white_balance(&neutral, analysis).unwrap();
            assert_eq!((direct.red, direct.blue), (500, 667));
            let wb =
                tr_core::raw_wb::estimate(engine, RawWhiteBalance::default(), analysis, |wb| {
                    Ok(super::super::WhiteBalanced {
                        base: decoder,
                        engine,
                        wb,
                    }
                    .decode(&neutral, 0)?
                    .2)
                })
                .unwrap();
            wb.validate_for(engine).unwrap();
        }
        fixture.root.text(272, "NIKON D800");
        let unsupported = fixture.bytes();
        assert!(decoder.probe(&unsupported).is_err());
        assert!(decoder.decode(&unsupported, 8).is_err());
        assert!(export_raw(&unsupported, options, 128 * 1024 * 1024).is_err());
    }
    #[test]
    fn explicit_engine_probe_wb_render_refusal_and_export_never_call_libraw() {
        let _guard = legacy_guard::Guard::new();
        let engine = RawEngine::TrueRendererExperimental;
        assert_eq!(engine.recipe(), tr_raw::RECIPE);
        let decoder =
            super::super::select_engine(tr_core::decoder::Trust::External, engine).unwrap();
        let mut f = fixture::Fixture::new(true);
        let bytes = f.bytes();
        decoder.probe(&bytes).unwrap();
        for wb in [
            RawWhiteBalance::default(),
            RawWhiteBalance {
                red: 1200,
                blue: 800,
                ..Default::default()
            },
        ] {
            let balanced = super::super::WhiteBalanced {
                base: decoder,
                engine,
                wb,
            };
            let (info, _) = balanced.probe(&bytes).unwrap();
            let (out, _, image) = balanced.decode(&bytes, 8).unwrap();
            assert_eq!(info.input_color, out.input_color);
            assert_eq!((image.width, image.height), (8, 7));
            protocol::validate_info(&out).unwrap();
        }
        let options = tr_core::export::Options {
            format: tr_core::export::Format::DngRaw,
            ..Default::default()
        };
        let (_, copy) = export_raw(&bytes, options, 1 << 20).unwrap();
        assert_eq!(tr_raw::unpack(&copy).unwrap().2, f.pixels);
        let cfa = [0, 1, 1, 2];
        for (i, p) in f.pixels.iter_mut().enumerate() {
            *p = [487, 512, 558][cfa[(i / 12 % 2) * 2 + i % 12 % 2]];
        }
        let neutral = f.bytes();
        for analysis in [
            tr_core::raw_wb::Analysis::Auto,
            tr_core::raw_wb::Analysis::Patch {
                x: 6,
                y: 5,
                side: 5,
            },
        ] {
            let wb = tr_core::raw_wb::estimate(
                engine,
                RawWhiteBalance {
                    red: 1150,
                    blue: 900,
                    ..Default::default()
                },
                analysis,
                |wb| {
                    Ok(super::super::WhiteBalanced {
                        base: decoder,
                        engine,
                        wb,
                    }
                    .decode(&neutral, 0)?
                    .2)
                },
            )
            .unwrap();
            assert!(
                (i32::from(wb.red) - 1000).abs() < 15 && (i32::from(wb.blue) - 1000).abs() < 15,
                "{wb:?}"
            );
        }
        for bad in [
            &b"FUJIFILMCCD-RAW unsupported"[..],
            &b"II\x2a\0\xff\xff\xff\xff"[..],
            &b"not raw"[..],
        ] {
            assert!(decoder.probe(bad).is_err());
            assert!(decoder.decode(bad, 8).is_err());
            assert!(export_raw(bad, options, 1 << 20).is_err());
        }
        for compressed in [
            include_bytes!("../../tr-raw/tests/fixtures/jpeg-modern16.dng").as_slice(),
            include_bytes!("../../tr-raw/tests/fixtures/jpeg-dng10-16.dng").as_slice(),
            include_bytes!("../../tr-raw/tests/fixtures/jpeg-predictor4-12.dng").as_slice(),
        ] {
            let (info, _) = decoder.probe(compressed).unwrap();
            let (_, _, image) = decoder.decode(compressed, 0).unwrap();
            assert_eq!((info.width, info.height), (image.width, image.height));
            let (_, copy) = export_raw(compressed, options, 1 << 20).unwrap();
            assert_eq!(decoder.decode(&copy, 0).unwrap().2.pixels, image.pixels);
        }
    }
}
