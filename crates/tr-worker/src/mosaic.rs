//! Narrow LibRaw extraction boundary; TrueRenderer owns all later arithmetic.
use super::libraw::text;
use anyhow::{Result, ensure};
use std::ffi::c_char;
use tr_core::{
    color::{LinearImage, MAX_PIXELS},
    decoder::{ColorSource, RawEngine},
    demosaic,
    protocol::RasterInfo,
};

#[repr(C)]
struct Info {
    width: u32,
    height: u32,
    flip: u32,
    cfa: [u32; 4],
    black: [f32; 4],
    white: f32,
    wb: [f32; 4],
    matrix: [f32; 9],
    camera: [c_char; 128],
}
impl Default for Info {
    fn default() -> Self {
        // SAFETY: all fields are integers/floats/arrays; every zero bit pattern is valid.
        unsafe { std::mem::zeroed() }
    }
}
unsafe extern "C" {
    fn tr_mosaic_probe(
        bytes: *const u8,
        length: usize,
        info: *mut Info,
        error: *mut c_char,
        size: usize,
    ) -> i32;
    fn tr_mosaic_read(
        bytes: *const u8,
        length: usize,
        samples: *mut u16,
        count: usize,
        info: *mut Info,
        error: *mut c_char,
        size: usize,
    ) -> i32;
}
fn describe(bytes: &[u8]) -> Result<Info> {
    let mut info = Info::default();
    let mut error = [0 as c_char; 512];
    // SAFETY: live input, correctly sized and exclusively owned output buffers.
    let status = unsafe {
        tr_mosaic_probe(
            bytes.as_ptr(),
            bytes.len(),
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, "{}", text(&error));
    ensure!(
        info.width >= 8
            && info.height >= 8
            && info.width as u64 * info.height as u64 <= MAX_PIXELS as u64,
        "Mosaico fuori quota"
    );
    Ok(info)
}
fn provenance(info: &Info) -> (RasterInfo, ColorSource) {
    let (width, height) = demosaic::oriented_size(info.width, info.height, info.flip);
    let color = ColorSource::Developed(format!(
        "Rec.2020 fp32; WB as-shot {:.3}/{:.3}/{:.3}/{:.3}; nero {:.0}/{:.0}/{:.0}/{:.0}, bianco {:.0}; matrice LibRaw; nessun clamp RGB",
        info.wb[0],
        info.wb[1],
        info.wb[2],
        info.wb[3],
        info.black[0],
        info.black[1],
        info.black[2],
        info.black[3],
        info.white
    ));
    (
        RasterInfo {
            scientific: None,
            reference_mip: None,
            width,
            height,
            source_width: width,
            source_height: height,
            native_bits: 0, // unpacked storage is uint16; sensor precision is not inferred
            format: "RAW".into(),
            decoder: format!(
                "{} · {}",
                RawEngine::TrueRenderer.recipe(),
                text(&info.camera)
            ),
            input_color: color.provenance(),
            filter: "nessuno (campioni LOD 0)".into(),
            orientation: format!(
                "LibRaw flip {} applicato una volta da TrueRenderer",
                info.flip
            ),
        },
        color,
    )
}
pub fn probe(bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
    Ok(provenance(&describe(bytes)?))
}
pub struct ExportMosaic {
    pub width: u32,
    pub height: u32,
    pub orientation: u16,
    pub cfa: [u8; 4],
    pub black: [f32; 4],
    pub white: u32,
    pub neutral: [f32; 3],
    pub matrix: [f32; 9],
    pub camera: String,
    pub samples: Vec<u16>,
}
pub fn export_mosaic(bytes: &[u8]) -> Result<ExportMosaic> {
    unsafe extern "C" {
        fn tr_mosaic_color_matrix(
            bytes: *const u8,
            len: usize,
            matrix: *mut f32,
            error: *mut c_char,
            size: usize,
        ) -> i32;
    }
    let mut info = describe(bytes)?;
    let mut samples = vec![0u16; info.width as usize * info.height as usize];
    let mut error = [0 as c_char; 512];
    // SAFETY: bounded exclusive buffers; native side rechecks geometry and model.
    let status = unsafe {
        tr_mosaic_read(
            bytes.as_ptr(),
            bytes.len(),
            samples.as_mut_ptr(),
            samples.len(),
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, "{}", text(&error));
    let mut matrix = [0.; 9];
    // SAFETY: the output holds exactly the 9 coefficients required by the ABI.
    let status = unsafe {
        tr_mosaic_color_matrix(
            bytes.as_ptr(),
            bytes.len(),
            matrix.as_mut_ptr(),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, "Matrice camera DNG: {}", text(&error));
    let m = matrix;
    let determinant = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    ensure!(
        determinant.is_finite() && determinant.abs() > 1e-6,
        "Matrice camera DNG assente o singolare"
    );
    let mut neutral = [0.; 3];
    for phase in 0..4 {
        let channel = info.cfa[phase] as usize;
        let value = 1. / info.wb[phase];
        ensure!(
            neutral[channel] == 0. || (neutral[channel] - value).abs() < 1e-5,
            "DNG RAW: verdi con WB differenti non supportati"
        );
        neutral[channel] = value;
    }
    Ok(ExportMosaic {
        width: info.width,
        height: info.height,
        orientation: match info.flip {
            0 => 1,
            3 => 3,
            5 => 8,
            6 => 6,
            _ => unreachable!(),
        },
        cfa: info.cfa.map(|v| v as u8),
        black: info.black,
        white: info.white as u32,
        neutral,
        matrix,
        camera: text(&info.camera),
        samples,
    })
}
pub fn develop(bytes: &[u8], max_edge: u32) -> Result<(RasterInfo, ColorSource, LinearImage)> {
    let probed = describe(bytes)?;
    let mut samples = vec![0u16; probed.width as usize * probed.height as usize];
    let mut info = Info::default();
    let mut error = [0 as c_char; 512];
    // SAFETY: allocation is bounded by the same input's probe. The shim checks count.
    let status = unsafe {
        tr_mosaic_read(
            bytes.as_ptr(),
            bytes.len(),
            samples.as_mut_ptr(),
            samples.len(),
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, "{}", text(&error));
    ensure!(
        (info.width, info.height, info.flip) == (probed.width, probed.height, probed.flip),
        "Geometria mosaico cambiata dopo unpack"
    );
    // Re-read calibration after unpack: camera decoders may refine black/white levels.
    let mosaic: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let phase = (i / info.width as usize % 2) * 2 + i % info.width as usize % 2;
            (f32::from(*v) - info.black[phase]) / (info.white - info.black[phase]) * info.wb[phase]
        })
        .collect();
    drop(samples);
    let working = demosaic::develop(
        &mosaic,
        info.width,
        info.height,
        info.cfa,
        info.matrix,
        info.flip,
    )?;
    drop(mosaic);
    let image = if max_edge > 0 && working.width.max(working.height) > max_edge {
        working.reduced(max_edge)
    } else {
        working
    };
    let (mut raster, color) = provenance(&info);
    raster.width = image.width;
    raster.height = image.height;
    if image.width != raster.source_width || image.height != raster.source_height {
        raster.filter = tr_core::color::FILTER_VERSION.into();
    }
    Ok((raster, color, image))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires TR_RAW_SAMPLE pointing to an authorized read-only Nikon sample"]
    fn real_raw_export_preserves_every_active_sample() {
        let path = std::env::var_os("TR_RAW_SAMPLE").expect("TR_RAW_SAMPLE");
        let bytes = std::fs::read(&path).unwrap();
        let before = export_mosaic(&bytes).unwrap();
        let (_, mut exported) = crate::export::raw(
            &bytes,
            tr_core::export::Options {
                format: tr_core::export::Format::DngRaw,
                ..Default::default()
            },
            tr_core::export::MAX_ENCODED,
        )
        .unwrap();
        // Independent TIFF codec reads the uncompressed sample storage. Only
        // the photometric tag of our private output copy changes for that reader.
        let ifd = u32::from_le_bytes(exported[4..8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(exported[ifd..ifd + 2].try_into().unwrap()) as usize;
        let entry = (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|i| u16::from_le_bytes(exported[*i..*i + 2].try_into().unwrap()) == 262)
            .unwrap();
        exported[entry + 8..entry + 10].copy_from_slice(&1u16.to_le_bytes());
        let mut reader = tiff::decoder::Decoder::new(std::io::Cursor::new(exported)).unwrap();
        assert_eq!(reader.dimensions().unwrap(), (before.width, before.height));
        let tiff::decoder::DecodingResult::U16(actual) = reader.read_image().unwrap() else {
            panic!("expected u16")
        };
        assert_eq!(actual, before.samples);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn raw_dng_roundtrip_preserves_active_sensor_samples() {
        let mut bytes = fixture(1923, true, "Synthetic DNG", 6);
        // Distinct samples catch phase shifts, crops and accidental processing;
        // include below-black and above-white values as well.
        let start = bytes.len() - 32 * 24 * 2;
        for index in 0..32 * 24 {
            bytes[start + index * 2..start + index * 2 + 2]
                .copy_from_slice(&((index * 17) as u16).to_le_bytes());
        }
        let before = export_mosaic(&bytes).unwrap();
        let (_, output) = crate::export::raw(
            &bytes,
            tr_core::export::Options {
                format: tr_core::export::Format::DngRaw,
                ..Default::default()
            },
            tr_core::export::MAX_ENCODED,
        )
        .unwrap();
        let after = export_mosaic(&output).unwrap();
        assert_eq!(before.samples, after.samples);
        assert_eq!(before.cfa, after.cfa);
        assert_eq!(before.black, after.black);
        assert_eq!(before.white, after.white);
        assert_eq!(before.orientation, after.orientation);
        assert_eq!(before.neutral, after.neutral);
        for (a, b) in before.matrix.iter().zip(after.matrix) {
            assert!((a - b).abs() < 1e-5);
        }
    }
    /// Own minimal, uncompressed Bayer DNG. No downloaded/reference code or photo.
    fn fixture(value: u16, wb: bool, camera: &str, orientation: u16) -> Vec<u8> {
        let shorts = |v: &[u16]| v.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>();
        let longs = |v: &[u32]| v.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>();
        let rational = |v: &[i32]| {
            v.iter()
                .flat_map(|n| [n.to_le_bytes(), 1i32.to_le_bytes()].concat())
                .collect::<Vec<_>>()
        };
        let ascii = |s: &str| [s.as_bytes(), &[0]].concat();
        let mut tags: Vec<(u16, u16, u32, Vec<u8>)> = vec![
            (254, 4, 1, longs(&[0])),
            (256, 4, 1, longs(&[32])),
            (257, 4, 1, longs(&[24])),
            (258, 3, 1, shorts(&[16])),
            (259, 3, 1, shorts(&[1])),
            (262, 3, 1, shorts(&[32803])),
            (271, 2, 13, ascii("TrueRenderer")),
            (272, 2, camera.len() as u32 + 1, ascii(camera)),
            (273, 4, 1, longs(&[0])),
            (274, 3, 1, shorts(&[orientation])),
            (277, 3, 1, shorts(&[1])),
            (278, 4, 1, longs(&[24])),
            (279, 4, 1, longs(&[32 * 24 * 2])),
            (284, 3, 1, shorts(&[1])),
            (33421, 3, 2, shorts(&[2, 2])),
            (33422, 1, 4, vec![0, 1, 1, 2]),
            (50706, 1, 4, vec![1, 4, 0, 0]),
            (50707, 1, 4, vec![1, 1, 0, 0]),
            (50708, 2, 27, ascii("TrueRenderer Synthetic DNG")),
            (50710, 1, 3, vec![0, 1, 2]),
            (50711, 3, 1, shorts(&[1])),
            (50713, 3, 2, shorts(&[1, 1])),
            (50714, 5, 1, rational(&[512])),
            (50717, 4, 1, longs(&[4095])),
            (50721, 10, 9, rational(&[1, 0, 0, 0, 1, 0, 0, 0, 1])),
            (50778, 3, 1, shorts(&[21])),
        ];
        if wb {
            tags.push((50728, 5, 3, rational(&[1, 1, 1])));
        }
        // ASCII counts are derived, including the terminator.
        for (_, kind, count, data) in &mut tags {
            if *kind == 2 {
                *count = data.len() as u32;
            }
        }
        tags.sort_by_key(|t| t.0);
        let extra_at = 8 + 2 + 12 * tags.len() + 4;
        let mut entries = vec![];
        let mut extra = vec![];
        for (tag, kind, count, data) in &tags {
            let entry_value = if data.len() <= 4 {
                let mut v = [0; 4];
                v[..data.len()].copy_from_slice(data);
                v
            } else {
                let offset = (extra_at + extra.len()) as u32;
                extra.extend(data);
                if extra.len() % 2 != 0 {
                    extra.push(0);
                }
                offset.to_le_bytes()
            };
            entries.extend(tag.to_le_bytes());
            entries.extend(kind.to_le_bytes());
            entries.extend(count.to_le_bytes());
            entries.extend(entry_value);
        }
        let image_at = (extra_at + extra.len()) as u32;
        let strip_index = tags.iter().position(|t| t.0 == 273).unwrap() * 12 + 8;
        entries[strip_index..strip_index + 4].copy_from_slice(&image_at.to_le_bytes());
        let mut bytes = b"II*\0\x08\0\0\0".to_vec();
        bytes.extend((tags.len() as u16).to_le_bytes());
        bytes.extend(entries);
        bytes.extend([0; 4]);
        bytes.extend(extra);
        for _ in 0..32 * 24 {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn native_extraction_preserves_black_white_headroom_and_orientation() {
        for (value, orientation) in [(511, 1), (1512, 6), (8000, 8)] {
            let bytes = fixture(value, true, "Synthetic DNG", orientation);
            let info = describe(&bytes).unwrap();
            assert_eq!(info.black, [512.; 4]);
            assert_eq!(info.white, 4095.);
            assert_eq!(info.wb, [1.; 4]);
            let (meta, _, image) = develop(&bytes, 0).unwrap();
            tr_core::protocol::validate_info(&meta).unwrap();
            assert_eq!(
                (image.width, image.height),
                if orientation == 1 { (32, 24) } else { (24, 32) }
            );
            let expected = (f32::from(value) - 512.) / (4095. - 512.);
            for p in image.pixels {
                for c in &p[..3] {
                    assert!((*c - expected).abs() < 2e-5, "{c} != {expected}");
                }
            }
        }
    }
    #[test]
    fn missing_wb_and_unsupported_models_do_not_fall_back_to_another_engine() {
        use tr_core::decoder::Trust;
        for (wb, model) in [(false, "Synthetic DNG"), (true, "Unsupported Camera")] {
            let bytes = fixture(1512, wb, model, 1);
            let selected = crate::select_engine(Trust::External, RawEngine::TrueRenderer).unwrap();
            assert!(selected.probe(&bytes).is_err());
            assert!(selected.decode(&bytes, 0).is_err());
        }
        let selected = crate::select_engine(Trust::External, RawEngine::LibRawAhd).unwrap();
        assert!(
            selected
                .probe(&fixture(1512, false, "Synthetic DNG", 1))
                .is_err()
        );
    }
}
