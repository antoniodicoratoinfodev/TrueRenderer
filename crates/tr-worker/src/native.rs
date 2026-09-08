//! macOS system decoders. External entry is enabled only by the sandboxed XPC host.
use anyhow::{Result, ensure};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
};
use tr_core::{
    color::{LinearImage, MAX_PIXELS},
    protocol::RasterInfo,
};
#[repr(C)]
struct Info {
    width: u32,
    height: u32,
    bits: u32,
    orientation: u32,
    frames: u32,
    raw: u32,
    format: [c_char; 32],
    decoder: [c_char; 128],
    input_color: [c_char; 256],
}
unsafe extern "C" {
    fn tr_image_probe(
        bytes: *const u8,
        length: usize,
        max_pixels: u64,
        info: *mut Info,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn tr_image_open(
        bytes: *const u8,
        length: usize,
        max_pixels: u64,
        info: *mut Info,
        error: *mut c_char,
        error_size: usize,
    ) -> *mut c_void;
    fn tr_image_render_backend(
        handle: *mut c_void,
        pixels: *mut f32,
        count: usize,
        metal: u32,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn tr_image_close(handle: *mut c_void);
}
pub fn probe(bytes: &[u8]) -> Result<RasterInfo> {
    let mut info: Info = unsafe { std::mem::zeroed() };
    let mut error = [0; 512];
    let status = unsafe {
        tr_image_probe(
            bytes.as_ptr(),
            bytes.len(),
            MAX_PIXELS as u64,
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, "{}", text(&error));
    let result = RasterInfo {
        width: info.width,
        height: info.height,
        source_width: info.width,
        source_height: info.height,
        native_bits: info.bits as u16,
        format: text(&info.format),
        decoder: text(&info.decoder),
        input_color: "Metadati · nessun raster".into(),
        filter: "nessuno · probe".into(),
        orientation: format!("EXIF {}", info.orientation),
    };
    tr_core::protocol::validate_info(&result)?;
    Ok(result)
}
struct Handle(NonNull<c_void>);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            tr_image_close(self.0.as_ptr());
        }
    }
}
fn text(bytes: &[c_char]) -> String {
    // Native snprintf writes to zero-initialized fixed-size arrays.
    unsafe { CStr::from_ptr(bytes.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
pub fn decode(bytes: &[u8], edge: u32) -> Result<(RasterInfo, LinearImage)> {
    decode_backend(bytes, edge, false)
}
fn decode_backend(bytes: &[u8], edge: u32, metal: bool) -> Result<(RasterInfo, LinearImage)> {
    let mut info: Info = unsafe { std::mem::zeroed() };
    let mut error = [0; 512];
    // The native handle borrows bytes only until it is dropped before this call returns.
    let raw = unsafe {
        tr_image_open(
            bytes.as_ptr(),
            bytes.len(),
            MAX_PIXELS as u64,
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    let handle = Handle(NonNull::new(raw).ok_or_else(|| anyhow::anyhow!("{}", text(&error)))?);
    let count = (info.width as usize)
        .checked_mul(info.height as usize)
        .ok_or_else(|| anyhow::anyhow!("Dimensioni native in overflow"))?;
    ensure!(
        count > 0 && count <= MAX_PIXELS,
        "Dimensioni native fuori quota"
    );
    let mut pixels = vec![[f32::NAN; 4]; count];
    // A NaN sentinel detects a native failure that returns without writing the entire bitmap.
    let status = unsafe {
        tr_image_render_backend(
            handle.0.as_ptr(),
            pixels.as_mut_ptr().cast(),
            count,
            u32::from(metal),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, "{}", text(&error));
    drop(handle);
    let image = LinearImage::new(info.width, info.height, pixels)?;
    let image = if edge > 0 && info.width.max(info.height) > edge {
        image.reduced(edge)
    } else {
        image
    };
    let frame = if info.frames > 1 {
        format!(" · pagina/fotogramma 1/{}", info.frames)
    } else {
        String::new()
    };
    let mut color = text(&info.input_color);
    color.push_str(&frame);
    color.truncate(color.floor_char_boundary(256));
    Ok((
        RasterInfo {
            width: image.width,
            height: image.height,
            source_width: info.width,
            source_height: info.height,
            native_bits: info.bits as u16,
            format: text(&info.format),
            decoder: text(&info.decoder),
            input_color: color,
            filter: if image.width == info.width && image.height == info.height {
                "nessuno (campioni LOD 0)".into()
            } else {
                tr_core::color::FILTER_VERSION.into()
            },
            orientation: format!("EXIF {} · applicato una volta", info.orientation),
        },
        image,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Core Image requires a native macOS session outside a terminal sandbox"]
    fn native_bitmap_preserves_orientation_alpha_and_sixteen_bit_precision() {
        let source = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
            2,
            2,
            vec![
                32768, 32768, 32768, 65535, 32769, 32769, 32769, 65535, 65535, 0, 0, 65535, 0,
                65535, 0, 0,
            ],
        )
        .unwrap();
        let mut bytes = std::io::Cursor::new(vec![]);
        image::DynamicImage::ImageRgba16(source)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let (info, raster) = decode(bytes.get_ref(), 0).unwrap();
        assert_eq!((info.width, info.height, info.native_bits), (2, 2, 16));
        assert!(
            (raster.pixels[0][0] - tr_core::color::srgb_to_linear(32768. / 65535.)).abs() < 0.005,
            "{:?}",
            raster.pixels
        );
        assert_ne!(raster.pixels[0][0], raster.pixels[1][0]);
        assert_eq!(raster.pixels[3], [0.; 4]);
    }
    #[test]
    #[ignore = "Core Image/Metal qualification requires the native macOS session"]
    fn persistent_context_and_metal_backend_comparison() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let folder = root.join("var/format-fixtures");
        let mut cases = Vec::new();
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file()
                || !matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("jpg" | "jpeg" | "png" | "tif" | "tiff" | "dng" | "heic" | "webp")
                )
            {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let start = std::time::Instant::now();
            let Ok((info, cpu)) = decode_backend(&bytes, 0, false) else {
                continue;
            };
            let first = start.elapsed().as_secs_f64();
            let start = std::time::Instant::now();
            let (_, repeat) = decode_backend(&bytes, 0, false).unwrap();
            let reused = start.elapsed().as_secs_f64();
            assert_eq!(
                cpu.pixels, repeat.pixels,
                "Persistent CPU context changed pixels"
            );
            let start = std::time::Instant::now();
            let metal = decode_backend(&bytes, 0, true);
            let seconds = start.elapsed().as_secs_f64();
            let report = match metal {
                Ok((_, gpu)) => {
                    assert_eq!([cpu.width, cpu.height], [gpu.width, gpu.height]);
                    let mut failures = 0;
                    let mut maximum = 0.0_f32;
                    for (a, b) in gpu.pixels.iter().flatten().zip(cpu.pixels.iter().flatten()) {
                        let error = (a - b).abs();
                        maximum = maximum.max(error);
                        if error > 1e-5 + 1e-4 * b.abs() {
                            failures += 1;
                        }
                    }
                    serde_json::json!({"failed_channels":failures,"maximum_absolute_error":maximum,"passed":failures==0})
                }
                Err(e) => serde_json::json!({"passed":false,"error":format!("{e:#}")}),
            };
            cases.push(serde_json::json!({"fixture":path.file_name().unwrap().to_string_lossy(),"size":[info.width,info.height],"cpu_first_seconds":first,"cpu_reused_seconds":reused,"metal_seconds":seconds,"cpu_reuse_bit_exact":true,"metal":report}));
        }
        assert!(
            cases.len() >= 3,
            "Generate format fixtures before native qualification"
        );
        let report = serde_json::json!({"scope":"Generated fixtures only, CPU persistent context bit-exact and explicit Metal experiment; no camera-wide or RSS qualification. Timings are single trials, not p95.","threshold":"1e-5 + 1e-4 * abs(cpu)","metal_enabled_for_decode":false,"cases":cases});
        std::fs::write(
            root.join("reports/preview-native-compute-macos.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn native_rejects_truncated_and_unknown_formats() {
        for bytes in [
            b"not an image".as_slice(),
            b"\xff\xd8\xff".as_slice(),
            b"II*\0".as_slice(),
        ] {
            assert!(decode(bytes, 0).is_err());
        }
    }
}
