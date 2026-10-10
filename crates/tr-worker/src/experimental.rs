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
        "Rec.2020 fp32; profilo DNG {}; matrici/WB v1; saturi conservati; no clamp",
        raw.profile_hash
    ));
    let mut camera = raw.camera;
    camera.truncate(camera.floor_char_boundary(75));
    let info = RasterInfo {
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
            "DNG {} applicato una volta da Experimental",
            raw.orientation
        ),
    };
    protocol::validate_info(&info)?;
    Ok((info, color))
}

pub fn probe_with_wb(bytes: &[u8], wb: RawWhiteBalance) -> Result<(RasterInfo, ColorSource)> {
    wb.validate_for(RawEngine::TrueRendererExperimental)?;
    let (mut info, mut color) = describe(tr_raw::probe_with_wb(bytes, wb.gains().map(f64::from))?)?;
    if !wb.is_as_shot() {
        let ColorSource::Developed(description) = color else {
            unreachable!()
        };
        color = ColorSource::Developed(format!(
            "{description}; WB relativo {:.3}/1/{:.3}",
            wb.gains()[0],
            wb.gains()[2]
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
    let image = tr_raw::develop(bytes, wb.gains().map(f64::from))?;
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
        description:"Experimental: campioni sensore originali uint16; calibrazione, LUT, nero, margini e crop conservati. WB/demosaic non applicati; EXIF/MakerNotes/preview esclusi. Conservare l'originale.".into()};
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
    }
}
