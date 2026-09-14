//! LibRaw under explicit bilinear or AHD recipes, through the shim in
//! `native/libraw`. Mirrors the shape of the macOS adapter: a narrow C surface,
//! statically linked, with the recipe visible in one file rather than spread
//! across call sites.
//!
//! This is the second stage of §7's two-stage render, the one that develops the
//! sensor mosaic. Its samples are the file's own data, so unlike the embedded
//! preview it carries a colour space the decoder can state.
use anyhow::{Result, bail, ensure};
use std::ffi::{CStr, c_char};
use tr_core::{
    color::{self, LinearImage, MAX_PIXELS, Pixel},
    decoder::{ColorSource, RawEngine},
    protocol::RasterInfo,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Info {
    width: u32,
    height: u32,
    bits: u32,
    non_bayer: u32,
    version: [c_char; 64],
    camera: [c_char; 128],
    color: [c_char; 64],
}

impl Default for Info {
    fn default() -> Self {
        // Every field is written by the shim before it reports success; zeroing
        // means a failure path can never be read as a plausible description.
        Self {
            width: 0,
            height: 0,
            bits: 0,
            non_bayer: 0,
            version: [0; 64],
            camera: [0; 128],
            color: [0; 64],
        }
    }
}

unsafe extern "C" {
    fn tr_libraw_probe(
        bytes: *const u8,
        length: usize,
        variant: u32,
        info: *mut Info,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn tr_libraw_develop(
        bytes: *const u8,
        length: usize,
        variant: u32,
        samples: *mut u16,
        count: usize,
        info: *mut Info,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
}

pub(crate) fn text(field: &[c_char]) -> String {
    // SAFETY: the shim writes NUL-terminated ASCII into a fixed array.
    unsafe { CStr::from_ptr(field.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

fn failure(error: &[c_char; 512]) -> anyhow::Error {
    anyhow::anyhow!("LibRaw: {}", text(error))
}

/// Metadata only. The mosaic is never unpacked here.
fn describe(bytes: &[u8], engine: RawEngine) -> Result<Info> {
    let mut info = Info::default();
    let mut error = [0 as c_char; 512];
    // SAFETY: the buffer outlives the call, and both outputs are owned locals
    // sized as the shim's header declares.
    let status = unsafe {
        tr_libraw_probe(
            bytes.as_ptr(),
            bytes.len(),
            u32::from(engine == RawEngine::LibRawAhd),
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, failure(&error));
    ensure!(
        info.width > 0
            && info.height > 0
            && (info.width as u64 * info.height as u64) <= MAX_PIXELS as u64,
        "Dimensioni RAW fuori quota"
    );
    Ok(info)
}

fn provenance(info: &Info, filter: &str, engine: RawEngine) -> RasterInfo {
    RasterInfo {
        width: info.width,
        height: info.height,
        source_width: info.width,
        source_height: info.height,
        native_bits: info.bits as u16,
        // The same label macOS uses, because it means the same thing: the
        // mosaic was developed. Only the preview fallback qualifies it, so a
        // caller can test the stage without knowing which decoder ran.
        format: "RAW".into(),
        decoder: format!(
            "LibRaw {} · {} · {}",
            text(&info.version),
            engine.recipe(),
            text(&info.camera)
        ),
        input_color: String::new(),
        filter: filter.into(),
        // LibRaw applies the recorded orientation itself, under `user_flip` of
        // -1, so the samples arrive upright and nothing is applied twice.
        orientation: "Applicato da LibRaw una volta".into(),
    }
}

/// What the developed samples are, said as a declaration rather than a guess.
///
/// The recipe asks LibRaw for a named output space and linear gamma, and the
/// camera's own white balance and colour matrix. That is the file describing
/// itself through its metadata, not sRGB assumed for want of anything better,
/// which is why this is `Declared` where the embedded preview is `Assumed`.
fn declared(info: &Info, engine: RawEngine) -> ColorSource {
    ColorSource::Developed(format!("{}; {}", text(&info.color), engine.recipe()))
}

pub fn probe_with_engine(bytes: &[u8], engine: RawEngine) -> Result<(RasterInfo, ColorSource)> {
    let info = describe(bytes, engine)?;
    ensure!(
        info.non_bayer == 0,
        "CFA non Bayer: il demosaicing di questa ricetta non è qualificato"
    );
    let mut raster = provenance(&info, "nessuno · probe", engine);
    let color = declared(&info, engine);
    raster.input_color = color.provenance();
    Ok((raster, color))
}

pub fn develop_with_engine(
    bytes: &[u8],
    max_edge: u32,
    engine: RawEngine,
) -> Result<(RasterInfo, ColorSource, LinearImage)> {
    let probed = describe(bytes, engine)?;
    if probed.non_bayer != 0 {
        bail!("CFA non Bayer: il demosaicing di questa ricetta non è qualificato");
    }
    let count = probed.width as usize * probed.height as usize * 3;
    let mut samples = vec![0u16; count];
    let mut info = Info::default();
    let mut error = [0 as c_char; 512];
    // SAFETY: `samples` is sized from the probe of the same bytes, and the shim
    // refuses rather than truncates if its own result disagrees.
    let status = unsafe {
        tr_libraw_develop(
            bytes.as_ptr(),
            bytes.len(),
            u32::from(engine == RawEngine::LibRawAhd),
            samples.as_mut_ptr(),
            count,
            &mut info,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    ensure!(status == 0, failure(&error));
    ensure!(
        [info.width, info.height] == [probed.width, probed.height],
        "Dimensioni diverse fra probe e sviluppo"
    );

    // 16-bit linear samples in the named output space, so only the matrix into
    // the working space applies. Nothing here undoes a transfer curve, because
    // the recipe asked for none.
    const FULL: f32 = 65535.;
    let (triples, rest) = samples.as_chunks::<3>();
    ensure!(rest.is_empty(), "Campioni non multipli di tre canali");
    let pixels: Vec<Pixel> = triples
        .iter()
        .map(|rgb| {
            let channels = [
                f32::from(rgb[0]) / FULL,
                f32::from(rgb[1]) / FULL,
                f32::from(rgb[2]) / FULL,
            ];
            let linear = if engine == RawEngine::LibRawAhd {
                channels
            } else {
                color::linear_srgb_to_rec2020(channels)
            };
            // Opaque, so premultiplication by an alpha of one is the identity.
            [linear[0], linear[1], linear[2], 1.]
        })
        .collect();
    drop(samples);

    let working = LinearImage::new(info.width, info.height, pixels)?;
    let reduced = if max_edge == 0 || info.width.max(info.height) <= max_edge {
        working
    } else {
        working.reduced(max_edge)
    };
    let filter = if (info.width, info.height) == (reduced.width, reduced.height) {
        "nessuno (campioni LOD 0)"
    } else {
        color::FILTER_VERSION
    };
    let mut raster = provenance(&info, filter, engine);
    raster.width = reduced.width;
    raster.height = reduced.height;
    let color = declared(&info, engine);
    raster.input_color = color.provenance();
    Ok((raster, color, reduced))
}
