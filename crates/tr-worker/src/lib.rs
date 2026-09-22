mod container;
mod export;
mod fits;
#[cfg(any(windows, target_os = "macos"))]
mod libraw;
#[cfg(windows)]
#[path = "../../../native/windows/lpac.rs"]
mod lpac;
#[cfg(any(windows, target_os = "macos"))]
mod mosaic;
#[cfg(target_os = "macos")]
mod native;
use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageDecoder, Limits, codecs::png::PngDecoder};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use tr_core::{
    color::{self, LinearImage, MAX_PIXELS},
    decoder::{ColorSource, Decoder, RawEngine, Trust},
    protocol::{self, DecodeRequest, MAX_SOURCE, RasterInfo},
};

fn decode(bytes: &[u8], max_edge: u32) -> Result<(RasterInfo, LinearImage)> {
    ensure!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "R0 decodifica soltanto PNG del corpus controllato; JPEG/TIFF sono previsti in R1"
    );
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut decoder = PngDecoder::with_limits(Cursor::new(bytes), limits)?;
    let (width, height) = decoder.dimensions();
    ensure!(
        (width as u64 * height as u64) <= MAX_PIXELS as u64,
        "Limite: 64 Mi pixel per immagine"
    );
    ensure!(
        decoder.icc_profile()?.is_none(),
        "ICC incorporato: trasformazione Little CMS ancora da qualificare in R1"
    );
    let color_type = decoder.color_type();
    let bits = color_type.bits_per_pixel() / color_type.channel_count() as u16;
    ensure!([8, 16].contains(&bits), "Profondità non supportata in R0");
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let (width, height) = (image.width(), image.height());
    let samples = image.to_rgba32f();
    let working = LinearImage::new(
        width,
        height,
        samples
            .pixels()
            .map(|p| color::from_encoded_srgb(p.0))
            .collect(),
    )?;
    let reduced = if max_edge == 0 || width.max(height) <= max_edge {
        working
    } else {
        working.reduced(max_edge)
    };
    let filter = if (width, height) == (reduced.width, reduced.height) {
        "nessuno (campioni LOD 0)"
    } else {
        color::FILTER_VERSION
    };
    let info = RasterInfo {
        scientific: None,
        reference_mip: None,
        width: reduced.width,
        height: reduced.height,
        source_width: width,
        source_height: height,
        native_bits: bits,
        format: "PNG".into(),
        decoder: "image 0.25.10 / png 0.18.1".into(),
        input_color: "sRGB · corpus generato".into(),
        filter: filter.into(),
        orientation: format!("{orientation:?} · applicato una volta"),
    };
    Ok((info, reduced))
}

fn probe(bytes: &[u8]) -> Result<RasterInfo> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut decoder = PngDecoder::with_limits(Cursor::new(bytes), limits)?;
    let (mut width, mut height) = decoder.dimensions();
    let orientation = decoder.orientation()?;
    use image::metadata::Orientation;
    if matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    ) {
        std::mem::swap(&mut width, &mut height);
    }
    let info = RasterInfo {
        scientific: None,
        reference_mip: None,
        width,
        height,
        source_width: width,
        source_height: height,
        native_bits: decoder.color_type().bits_per_pixel()
            / decoder.color_type().channel_count() as u16,
        format: "PNG".into(),
        decoder: "image metadata probe".into(),
        input_color: "Metadati · nessun raster".into(),
        filter: "nessuno · probe".into(),
        orientation: format!("{orientation:?}"),
    };
    protocol::validate_info(&info)?;
    Ok(info)
}

/// gAMA value the PNG specification pairs with sRGB, scaled by 100000.
const SRGB_GAMA: u32 = 45455;

/// Colour contract of a PNG, read from its chunks instead of assumed.
///
/// `iCCP` and `cHRM` describe spaces this build cannot apply, so a file
/// carrying them is refused: rendering it as sRGB would change the meaning of
/// its pixels, which is exactly the failure this port exists to prevent. An
/// `sRGB` chunk is a statement the pipeline honours as written.
///
/// A lone `gAMA` is weaker than it looks. It fixes the transfer curve and says
/// nothing about the primaries, so even at 45455 it leaves primaries assumed
/// while applying the declared power curve; other values this build does not
/// apply, and the file is refused.
fn png_color_source(bytes: &[u8]) -> Result<ColorSource> {
    Ok(png_color_contract(bytes)?.0)
}

// The optional exponent applies only to a lone supported gAMA declaration;
// sRGB/cICP retain their exact piecewise transfer. Primaries stay separate.
fn png_color_contract(bytes: &[u8]) -> Result<(ColorSource, Option<f32>)> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 4 * 1024 * 1024,
    });
    // Ancillary chunks are only parsed once the reader has reached IDAT;
    // `read_header_info` stops at IHDR and would report every file as silent.
    let reader = decoder.read_info()?;
    // The raw chunk fields, not the resolved accessors: `chromaticities()` and
    // `gamma()` substitute the sRGB values whenever an sRGB chunk is present,
    // which would hide what the file actually states.
    let info = reader.info();
    if let Some(cicp) = info.coding_independent_code_points {
        // PNG Third Edition §11.3.2.6. Recognize the declaration even when
        // unsupported: neither an sRGB fallback chunk nor absent ICC may hide it.
        ensure!(
            cicp.color_primaries == 1
                && cicp.transfer_function == 13
                && cicp.matrix_coefficients == 0
                && cicp.is_video_full_range_image,
            "cICP dichiara una codifica non supportata ({cicp:?}); nessun ripiego sRGB"
        );
        ensure!(
            info.icc_profile.is_none(),
            "cICP insieme a iCCP: confronto dei profili non disponibile, file ambiguo"
        );
        ensure!(
            info.gama_chunk
                .is_none_or(|gamma| gamma.into_scaled() == SRGB_GAMA)
                && info.chrm_chunk.is_none_or(|chrm| chrm
                    == png::SourceChromaticities::new(
                        (0.3127, 0.3290),
                        (0.6400, 0.3300),
                        (0.3000, 0.6000),
                        (0.1500, 0.0600)
                    )),
            "cICP sRGB in conflitto con gAMA/cHRM"
        );
        return Ok((ColorSource::Declared("sRGB · cICP".into()), None));
    }
    ensure!(
        info.icc_profile.is_none(),
        "iCCP dichiara un profilo incorporato: trasformazione Little CMS ancora da qualificare in R1"
    );
    // The specification tells decoders without colour management to honour an
    // sRGB chunk and ignore any gAMA and cHRM beside it.
    if info.srgb.is_some() {
        return Ok((ColorSource::Declared("sRGB".into()), None));
    }
    ensure!(
        info.chrm_chunk.is_none(),
        "cHRM dichiara primarie che questa build non sa applicare; assumere sRGB cambierebbe i colori"
    );
    match info.gama_chunk {
        Some(gamma) => {
            let scaled = gamma.into_scaled();
            ensure!(
                scaled == SRGB_GAMA,
                "gAMA dichiara una curva ({}) che questa build non sa applicare",
                f64::from(scaled) / 100_000.
            );
            Ok((
                ColorSource::AssumedPrimaries("gAMA 0.45455 dichiarato e applicato".into()),
                Some(100_000. / scaled as f32),
            ))
        }
        None => Ok((ColorSource::Assumed("nessun chunk di colore".into()), None)),
    }
}

/// Portable implementation. Compiled into every build and the only one that
/// exists off macOS, which is why it never accepts arbitrary sources.
struct CorpusPng;
struct FitsDecoder;
impl Decoder for FitsDecoder {
    fn name(&self) -> &'static str {
        "FITS IMAGE v1"
    }
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
        Ok((fits::probe(bytes)?, ColorSource::Scientific))
    }
    fn decode(
        &self,
        bytes: &[u8],
        max_edge: u32,
    ) -> Result<(RasterInfo, ColorSource, LinearImage)> {
        let (mut info, image) = fits::decode(bytes)?;
        let image = if max_edge > 0 && image.width.max(image.height) > max_edge {
            image.reduced(max_edge)
        } else {
            image
        };
        info.width = image.width;
        info.height = image.height;
        Ok((info, ColorSource::Scientific, image))
    }
}
impl Decoder for CorpusPng {
    fn name(&self) -> &'static str {
        "corpus-png"
    }
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
        let color = png_color_source(bytes)?;
        Ok((probe(bytes)?, color))
    }
    fn decode(
        &self,
        bytes: &[u8],
        max_edge: u32,
    ) -> Result<(RasterInfo, ColorSource, LinearImage)> {
        let color = png_color_source(bytes)?;
        let (info, image) = decode(bytes, max_edge)?;
        Ok((info, color, image))
    }
}

/// Resolve the primary TIFF image's stored alpha semantics before colour conversion.
#[cfg(any(windows, test))]
fn tiff_associated_alpha(bytes: &[u8], has_alpha: bool) -> Result<bool> {
    // Reuse the exact TIFF reader used by image, including BigTIFF/endian
    // support and bounded IFD values. No raster is allocated by this query.
    let mut limits = tiff::decoder::Limits::default();
    limits.ifd_value_size = 64 * 1024;
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes))?.with_limits(limits);
    // TIFF 6.0 section 20: absence of ICC does not imply absence of a
    // declared transfer/primaries. Until this adapter can apply these tags,
    // refuse them before any sRGB assumption (also for partial descriptions).
    for (number, name) in [
        (301, "TransferFunction"),
        (318, "WhitePoint"),
        (319, "PrimaryChromaticities"),
        (342, "TransferRange"),
        (532, "ReferenceBlackWhite"),
    ] {
        ensure!(
            decoder
                .find_tag(tiff::tags::Tag::from_u16_exhaustive(number))?
                .is_none(),
            "Colorimetria TIFF {name}: trasformazione non supportata; nessun ripiego sRGB"
        );
    }
    let extras = decoder
        .find_tag_unsigned_vec::<u16>(tiff::tags::Tag::ExtraSamples)?
        .unwrap_or_default();
    match extras.as_slice() {
        [] if !has_alpha => Ok(false),
        [1] if has_alpha => Ok(true),
        [2] if has_alpha => Ok(false),
        _ => anyhow::bail!("TIFF ExtraSamples: alpha ambigua o non supportata ({extras:?})"),
    }
}

/// Decode an ordinary bitmap or an explicitly identified embedded RAW preview.
/// A preview describes the camera-developed JPEG, never an undeclared substitute
/// for the sensor mosaic; `format` and `decoder` retain that distinction.
#[cfg(any(windows, test))]
fn portable_decode(
    bytes: &[u8],
    max_edge: u32,
    raster: bool,
) -> Result<(RasterInfo, ColorSource, Option<LinearImage>)> {
    // Ordinary TIFF thumbnails never substitute for the primary image.
    let preview = container::declares_raw(bytes)
        .then(|| container::embedded_preview(bytes).ok())
        .flatten();
    let (payload, container_stage, orientation) = match preview {
        Some(preview) => (
            &bytes[preview.offset..preview.offset + preview.length],
            true,
            preview.orientation,
        ),
        None => (bytes, false, None),
    };

    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let format = image::guess_format(payload).context("Formato non riconosciuto")?;
    let mut reader = image::ImageReader::new(Cursor::new(payload));
    reader.set_format(format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    ensure!(
        (width as u64 * height as u64) <= MAX_PIXELS as u64,
        "Limite: 64 Mi pixel per immagine"
    );
    // Same rule as the PNG path: a declared space this build cannot apply is
    // refused, never quietly rendered as sRGB.
    ensure!(
        decoder.icc_profile()?.is_none(),
        "Profilo ICC incorporato: trasformazione Little CMS ancora da qualificare in R1"
    );
    let associated_alpha = if format == image::ImageFormat::Tiff {
        tiff_associated_alpha(payload, decoder.color_type().has_alpha())?
    } else {
        false
    };
    let embedded = decoder.orientation()?;
    let (color, gamma_exponent) = if format == image::ImageFormat::Png {
        png_color_contract(payload)?
    } else {
        (
            ColorSource::Assumed(if container_stage {
                "anteprima della fotocamera, nessuno spazio dichiarato".into()
            } else {
                "nessun profilo incorporato".into()
            }),
            None,
        )
    };
    // `format` is capped at 32 bytes by the protocol, so it carries the stage
    // as a label and `decoder` carries the sentence that explains it.
    let (stage, provenance) = if container_stage {
        (
            "RAW · anteprima".to_string(),
            format!(
                "image 0.25.10 · {format:?} incorporato nel contenitore; mosaico non sviluppato"
            ),
        )
    } else {
        (
            format!("{format:?}"),
            format!(
                "image 0.25.10 · {format:?} · {}",
                tr_core::decoder::BITMAP_RECIPE
            ),
        )
    };

    let mut info = RasterInfo {
        scientific: None,
        reference_mip: None,
        width,
        height,
        source_width: width,
        source_height: height,
        native_bits: decoder.color_type().bits_per_pixel()
            / u16::from(decoder.color_type().channel_count()),
        format: stage,
        decoder: provenance,
        input_color: color.provenance(),
        filter: "nessuno · probe".into(),
        orientation: format!("{embedded:?}"),
    };
    // The selected IFD (or primary fallback) wins, including explicit 1.
    // Only absent container metadata delegates to the JPEG's own EXIF.
    let orientation = match orientation.and_then(|exif| u8::try_from(exif).ok()) {
        Some(exif) => image::metadata::Orientation::from_exif(exif).unwrap_or(embedded),
        _ => embedded,
    };
    if matches!(
        orientation,
        image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH
    ) {
        std::mem::swap(&mut info.width, &mut info.height);
        std::mem::swap(&mut info.source_width, &mut info.source_height);
    }
    info.orientation = format!("{orientation:?} · applicato una volta");
    if !raster {
        protocol::validate_info(&info)?;
        return Ok((info, color, None));
    }

    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let (width, height) = (image.width(), image.height());
    let samples = image.to_rgba32f();
    let working = LinearImage::new(
        width,
        height,
        samples
            .pixels()
            .map(|p| {
                let mut rgba = p.0;
                if associated_alpha {
                    // TIFF associates the stored colour samples. Undo that
                    // before the nonlinear input transfer, then premultiply
                    // exactly once in linear working space. Hidden RGB at zero
                    // alpha has no contribution, and must never divide by zero.
                    if rgba[3] == 0.0 {
                        return [0.0; 4];
                    }
                    for channel in 0..3 {
                        rgba[channel] /= rgba[3];
                    }
                }
                if let Some(exponent) = gamma_exponent {
                    let linear = color::linear_srgb_to_rec2020([
                        rgba[0].powf(exponent),
                        rgba[1].powf(exponent),
                        rgba[2].powf(exponent),
                    ]);
                    [
                        linear[0] * rgba[3],
                        linear[1] * rgba[3],
                        linear[2] * rgba[3],
                        rgba[3],
                    ]
                } else {
                    color::from_encoded_srgb(rgba)
                }
            })
            .collect(),
    )?;
    let reduced = if max_edge == 0 || width.max(height) <= max_edge {
        working
    } else {
        working.reduced(max_edge)
    };
    info.width = reduced.width;
    info.height = reduced.height;
    info.source_width = width;
    info.source_height = height;
    info.filter = if (width, height) == (reduced.width, reduced.height) {
        "nessuno (campioni LOD 0)".into()
    } else {
        color::FILTER_VERSION.into()
    };
    Ok((info, color, Some(reduced)))
}

/// Windows external decoder: LibRaw for the mosaic, `image` for everything
/// else, and the embedded preview only where the mosaic cannot be developed.
///
/// The order is the point. §7 names LibRaw as the baseline, so a container goes
/// there first and the preview is a declared fallback, never a silent
/// substitute. The two are told apart by the provenance without exception:
/// development says `RAW · sviluppo` and states its colour space, the preview
/// says `RAW · anteprima` and states that sRGB was assumed.
#[cfg(any(windows, test))]
struct PortablePreview;
#[cfg(windows)]
fn portable_preview_fallback(
    bytes: &[u8],
    max_edge: u32,
    raster: bool,
    refusal: &anyhow::Error,
) -> Result<(RasterInfo, ColorSource, Option<LinearImage>)> {
    ensure!(
        container::declares_raw(bytes),
        "Sviluppo rifiutato: {refusal:#}"
    );
    container::embedded_preview(bytes)?;
    let (mut info, color, pixels) = portable_decode(bytes, max_edge, raster)?;
    info.decoder = format!("Sviluppo rifiutato: {refusal:#}");
    info.decoder.truncate(info.decoder.floor_char_boundary(128));
    Ok((info, color, pixels))
}
#[cfg(any(windows, test))]
impl Decoder for PortablePreview {
    fn name(&self) -> &'static str {
        "libraw-e-anteprima"
    }
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
        #[cfg(windows)]
        match libraw::probe_with_engine(bytes, RawEngine::LibRawBilinear) {
            Ok(developed) => return Ok(developed),
            Err(refusal) if container::declares_raw(bytes) => {
                let (info, color, _) = portable_preview_fallback(bytes, 0, false, &refusal)?;
                return Ok((info, color));
            }
            Err(_) => {}
        }
        let (info, color, _) = portable_decode(bytes, 0, false)?;
        Ok((info, color))
    }
    fn decode(
        &self,
        bytes: &[u8],
        max_edge: u32,
    ) -> Result<(RasterInfo, ColorSource, LinearImage)> {
        #[cfg(windows)]
        match libraw::probe_with_engine(bytes, RawEngine::LibRawBilinear) {
            // Choose the same stage as probe, without developing a full RAW
            // during metadata reads. A later unpack/process failure remains
            // an explicit error, never a different stage behind the probe.
            Ok(_) => {
                return libraw::develop_with_engine(bytes, max_edge, RawEngine::LibRawBilinear);
            }
            Err(refusal) if container::declares_raw(bytes) => {
                let (info, color, raster) =
                    portable_preview_fallback(bytes, max_edge, true, &refusal)?;
                return Ok((info, color, raster.context("Raster mancante")?));
            }
            Err(_) => {}
        }
        let (info, color, raster) = portable_decode(bytes, max_edge, true)?;
        Ok((info, color, raster.context("Raster mancante")?))
    }
}

/// Classify what the Apple decoder reported about the input colour space.
///
/// ImageIO writes a profile name when the file carries one and says so
/// explicitly when it does not; CIRAWFilter develops from the camera's own
/// white balance and matrices under a development recipe, not an embedded ICC.
/// This reads a string the native adapter builds, so it is a bridge: the
/// distinction belongs in `TRImageInfo` as a flag, alongside the text.
#[cfg(target_os = "macos")]
fn apple_color_source(reported: &str) -> ColorSource {
    if reported.starts_with("RAW:") {
        ColorSource::Developed(reported.into())
    } else if reported.starts_with("Profilo ImageIO: ") {
        ColorSource::Declared(reported.into())
    } else {
        ColorSource::Assumed(reported.into())
    }
}

/// Apple adapter. ImageIO and ColorSync for bitmaps, CIRAWFilter for RAW.
#[cfg(target_os = "macos")]
struct Apple;
#[cfg(target_os = "macos")]
impl Decoder for Apple {
    fn name(&self) -> &'static str {
        "apple-imageio-ciraw"
    }
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
        let info = native::probe(bytes)?;
        let color = apple_color_source(&info.input_color);
        Ok((info, color))
    }
    fn decode(
        &self,
        bytes: &[u8],
        max_edge: u32,
    ) -> Result<(RasterInfo, ColorSource, LinearImage)> {
        let (info, image) = native::decode(bytes, max_edge)?;
        let color = apple_color_source(&info.input_color);
        Ok((info, color, image))
    }
}

/// The single platform decision of the decode path. A port for a new platform
/// is published here; nothing upstream of this function names an operating
/// system. Returning `None` is a supported state, not a failure: it means this
/// build can still serve the controlled corpus.
pub fn external_decoder() -> Option<&'static dyn Decoder> {
    #[cfg(target_os = "macos")]
    {
        Some(&Apple)
    }
    #[cfg(windows)]
    {
        Some(&PortablePreview)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(test)]
fn select(trust: Trust) -> Result<&'static dyn Decoder> {
    select_engine(trust, RawEngine::default())
}

fn select_engine(trust: Trust, engine: RawEngine) -> Result<&'static dyn Decoder> {
    if trust == Trust::Controlled {
        return Ok(&CorpusPng);
    }
    ensure!(
        engine.available(),
        "Motore RAW non disponibile su questa piattaforma"
    );
    #[cfg(any(windows, target_os = "macos"))]
    match engine {
        RawEngine::LibRawAhd => return Ok(&EngineDecoder(RawEngine::LibRawAhd)),
        RawEngine::TrueRenderer => return Ok(&EngineDecoder(RawEngine::TrueRenderer)),
        RawEngine::LibRawBilinear => {
            #[cfg(windows)]
            return Ok(&PortablePreview);
            #[cfg(target_os = "macos")]
            return Ok(&EngineDecoder(RawEngine::LibRawBilinear));
        }
        _ => {}
    }
    external_decoder()
        .context("Decoder esterno non disponibile per questa piattaforma: solo corpus")
}

/// Explicit RAW selection never silently substitutes another development or a JPEG.
#[cfg(any(windows, target_os = "macos"))]
struct EngineDecoder(RawEngine);
#[cfg(any(windows, target_os = "macos"))]
impl EngineDecoder {
    fn is_raw(&self, bytes: &[u8]) -> bool {
        // LibRaw is the authority for all of its containers, including non-TIFF.
        if libraw::probe_with_engine(bytes, RawEngine::LibRawBilinear).is_ok()
            || container::declares_raw(bytes)
        {
            return true;
        }
        // ImageIO may know a RAW that this LibRaw release cannot identify.
        // Selecting LibRaw must then refuse it, never develop it with Apple.
        #[cfg(target_os = "macos")]
        if native::probe(bytes).is_ok_and(|info| info.format == "RAW") {
            return true;
        }
        false
    }
    fn bitmap(&self) -> &'static dyn Decoder {
        #[cfg(target_os = "macos")]
        {
            &Apple
        }
        #[cfg(windows)]
        {
            &PortablePreview
        }
    }
}
#[cfg(any(windows, target_os = "macos"))]
impl Decoder for EngineDecoder {
    fn name(&self) -> &'static str {
        self.0.recipe()
    }
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)> {
        if self.is_raw(bytes) {
            if self.0 == RawEngine::TrueRenderer {
                mosaic::probe(bytes)
            } else {
                libraw::probe_with_engine(bytes, self.0)
            }
        } else {
            self.bitmap().probe(bytes)
        }
    }
    fn decode(
        &self,
        bytes: &[u8],
        max_edge: u32,
    ) -> Result<(RasterInfo, ColorSource, LinearImage)> {
        if self.is_raw(bytes) {
            if self.0 == RawEngine::TrueRenderer {
                mosaic::develop(bytes, max_edge)
            } else {
                libraw::develop_with_engine(bytes, max_edge, self.0)
            }
        } else {
            self.bitmap().decode(bytes, max_edge)
        }
    }
}

pub fn serve<R: Read, W: Write>(input: R, output: W) -> Result<()> {
    serve_with_policy(input, output, false)
}

/// Serve arbitrary sources, the way the macOS XPC entry point does.
///
/// The broker asks for this only after confining the process, but a caller's
/// word is not the reason to widen what may be decoded. The kernel is asked
/// first, and a process that is not actually inside a job with a memory
/// ceiling keeps the corpus gate instead of being trusted on the strength of
/// an argument on its own command line.
pub fn serve_confined<R: Read, W: Write>(input: R, output: W) -> Result<()> {
    ensure!(
        confinement::verified(),
        "Fiducia esterna richiesta ma il kernel non conferma il confinamento"
    );
    serve_with_policy(input, output, true)
}

/// Does this process actually run under the limits the broker says it set?
mod confinement {
    /// Windows: inside a job whose limits include a committed-memory ceiling
    /// and a cap on live processes. Being in some job is not enough, because
    /// shells, schedulers and containers place processes in jobs of their own.
    #[cfg(windows)]
    pub fn verified() -> bool {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_GROUPS, TOKEN_QUERY, TokenCapabilities, TokenIsAppContainer,
        };
        use windows_sys::Win32::System::{
            JobObjects::{
                IsProcessInJob, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                QueryInformationJobObject,
            },
            Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut handle = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
            return false;
        }
        let token = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut app_container = 0u32;
        let mut returned = 0u32;
        let mut capabilities = TOKEN_GROUPS::default();
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenIsAppContainer,
                (&raw mut app_container).cast(),
                size_of::<u32>() as u32,
                &mut returned,
            )
        } == 0
            || app_container != 1
            || unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    TokenCapabilities,
                    (&raw mut capabilities).cast(),
                    size_of::<TOKEN_GROUPS>() as u32,
                    &mut returned,
                )
            } == 0
            || capabilities.GroupCount != 0
        {
            return false;
        }
        if !crate::lpac::verified(token.as_raw_handle()) {
            return false;
        }
        let mut inside = 0;
        // A null job handle asks about the job this process already belongs to.
        if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut inside) } == 0
            || inside == 0
        {
            return false;
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let queried = unsafe {
            QueryInformationJobObject(
                std::ptr::null_mut(),
                JobObjectExtendedLimitInformation,
                (&raw mut limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        let flags = limits.BasicLimitInformation.LimitFlags;
        queried != 0
            && flags & JOB_OBJECT_LIMIT_JOB_MEMORY != 0
            && flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS != 0
            && flags & JOB_OBJECT_LIMIT_PROCESS_MEMORY != 0
            && flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0
            && limits.BasicLimitInformation.ActiveProcessLimit == 1
            && limits.JobMemoryLimit > 0
            && limits.ProcessMemoryLimit > 0
            && limits.ProcessMemoryLimit <= limits.JobMemoryLimit
    }

    /// Elsewhere the confined entry point does not exist yet, so nothing can
    /// corroborate the claim and nothing is granted.
    #[cfg(not(windows))]
    pub fn verified() -> bool {
        false
    }
}
fn serve_with_policy<R: Read, W: Write>(input: R, output: W, external: bool) -> Result<()> {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    let policy = tr_core::corpus::CorpusPolicy::default();
    loop {
        let (kind, id, data) = protocol::read_control(&mut input).context("Lettura richiesta")?;
        ensure!(kind == protocol::REQUEST, "Messaggio non richiesta");
        let request: DecodeRequest = protocol::parse(&data)?;
        let reference = matches!(request.intent, protocol::DecodeIntent::ReferenceMip { .. });
        ensure!(
            request.source_len > 0
                && request.source_len <= MAX_SOURCE
                && request.max_edge <= if reference { 8192 } else { 2048 },
            "Richiesta fuori quota"
        );
        let mut bytes = vec![0; request.source_len];
        input.read_exact(&mut bytes)?;
        if let protocol::DecodeIntent::ScientificSample { x, y } = request.intent {
            let result = (|| -> Result<_> {
                ensure!(
                    policy.approves_bytes(&bytes) || external,
                    "FITS esterno richiede isolamento OS"
                );
                ensure!(
                    request.max_edge == 0 && request.maximum_output_bytes == 0,
                    "Richiesta campione incoerente"
                );
                fits::sample(&bytes, x, y)
            })();
            match result {
                Ok(sample) => {
                    protocol::write_control(&mut output, protocol::RESPONSE, id, &sample)?
                }
                Err(error) => protocol::write_control(
                    &mut output,
                    protocol::ERROR,
                    id,
                    &format!("{error:#}").chars().take(2048).collect::<String>(),
                )?,
            }
            continue;
        }
        if let protocol::DecodeIntent::Export(options) = request.intent {
            let encoded = (|| -> Result<_> {
                let controlled = policy.approves_bytes(&bytes);
                ensure!(
                    controlled || external,
                    "Export esterno richiede isolamento OS"
                );
                ensure!(
                    !fits::recognizes(&bytes),
                    "Export fotografico FITS non disponibile: non convertire implicitamente dati scientifici in RGB"
                );
                options.validate()?;
                ensure!(request.max_edge == 0, "Export richiede sviluppo nativo");
                if options.format == tr_core::export::Format::DngRaw {
                    #[cfg(any(windows, target_os = "macos"))]
                    return export::raw(&bytes, options, request.maximum_output_bytes);
                    #[cfg(not(any(windows, target_os = "macos")))]
                    anyhow::bail!("Mosaico RAW non disponibile su questa piattaforma");
                }
                let decoder = select_engine(
                    if controlled {
                        Trust::Controlled
                    } else {
                        Trust::External
                    },
                    request.raw_engine,
                )?;
                let (_, _, raster) = decoder.decode(&bytes, 0)?;
                export::render(raster, options, request.maximum_output_bytes)
            })();
            drop(bytes);
            match encoded {
                Ok((info, bytes)) => {
                    protocol::write_control(&mut output, protocol::RESPONSE, id, &info)?;
                    output.write_all(&bytes)?;
                    output.flush()?;
                }
                Err(error) => protocol::write_control(
                    &mut output,
                    protocol::ERROR,
                    id,
                    &format!("{error:#}").chars().take(2048).collect::<String>(),
                )?,
            }
            continue;
        }
        let decoded = (|| -> Result<(RasterInfo, Option<LinearImage>)> {
            let controlled = policy.approves_bytes(&bytes);
            ensure!(
                controlled || external,
                "Sorgente rifiutata: non appartiene al corpus R0 compilato nel decoder"
            );
            let decoder = if fits::recognizes(&bytes) {
                &FitsDecoder as &dyn Decoder
            } else {
                select_engine(
                    if controlled {
                        Trust::Controlled
                    } else {
                        Trust::External
                    },
                    request.raw_engine,
                )?
            };
            // The colour contract, not the decoder's own prose, is what the
            // render panel shows. An assumption stays visible as an assumption.
            let (mut metadata, color) = decoder.probe(&bytes)?;
            metadata.input_color = color.provenance();
            if request.intent == protocol::DecodeIntent::Probe {
                return Ok((metadata, None));
            }
            ensure!(
                request.intent != protocol::DecodeIntent::FullSource || request.max_edge == 0,
                "Intent Full con scala ridotta"
            );
            if let protocol::DecodeIntent::ReferenceMip { cpu_threads } = request.intent {
                ensure!(
                    request.max_edge > 0 && (1..=256).contains(&cpu_threads),
                    "Richiesta mip fuori quota"
                );
                tr_core::compute::configure(cpu_threads)?;
            }
            let output_size = if reference {
                protocol::mip_geometry([metadata.width, metadata.height], request.max_edge).0
            } else {
                [metadata.width, metadata.height]
            };
            // Native development is still full-frame and bounded by MAX_PIXELS.
            // Only its canonical mip crosses IPC on the new explicit intent.
            ensure!(
                output_size[0] as u64 * output_size[1] as u64 * 16 <= request.maximum_output_bytes,
                "Raster oltre prenotazione"
            );
            let (mut info, color, mut raster) =
                decoder.decode(&bytes, if reference { 0 } else { request.max_edge })?;
            info.input_color = color.provenance();
            ensure!(
                [info.source_width, info.source_height]
                    == [metadata.source_width, metadata.source_height],
                "Dimensioni diverse dal probe"
            );
            if reference {
                ensure!(
                    [raster.width, raster.height] == [metadata.width, metadata.height],
                    "Mip senza sorgente completa"
                );
                let (reduced, base, opaque) = if info.scientific.is_some() {
                    tr_core::provider::reduce_scientific_mip(raster, request.max_edge)?
                } else {
                    tr_core::provider::reduce_reference_mip(raster, request.max_edge)?
                };
                raster = reduced;
                info.width = raster.width;
                info.height = raster.height;
                info.filter = if info.scientific.is_some() {
                    tr_core::science::FILTER
                } else {
                    tr_core::resample::VERSION
                }
                .into();
                info.reference_mip = Some(protocol::ReferenceMip { base, opaque });
            }
            Ok((info, Some(raster)))
        })();
        // Native handles borrowing these bytes have already been closed. Do not
        // retain compressed input while the host receives the full fp32 raster.
        drop(bytes);
        match decoded {
            Ok((info, raster)) => {
                protocol::write_control(&mut output, protocol::RESPONSE, id, &info)?;
                if let Some(raster) = raster {
                    protocol::write_raster(&mut output, &raster)?;
                }
            }
            Err(error) => {
                let error: String = format!("{error:#}").chars().take(2048).collect();
                protocol::write_control(&mut output, protocol::ERROR, id, &error)?;
            }
        }
    }
}
/// C bridge owns and transfers two distinct pipe descriptors to Rust.
///
/// # Safety
/// Both descriptors must be valid, uniquely owned descriptors with the declared direction.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tr_worker_serve_fds(input: i32, output: i32) -> i32 {
    use std::{fs::File, os::fd::FromRawFd};
    if input < 0 || output < 0 || input == output {
        return 2;
    }
    // SAFETY: ownership is transferred by the native XPC adapter after type/mode validation.
    let input = unsafe { File::from_raw_fd(input) };
    // SAFETY: the second descriptor is distinct and exclusively transferred as above.
    let output = unsafe { File::from_raw_fd(output) };
    match std::panic::catch_unwind(|| serve_with_policy(input, output, true)) {
        Ok(Ok(())) => 0,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::decoder::RECIPE;

    fn png_with_cicp(codes: [u8; 4], srgb: bool, conflicting_gamma: bool) -> Vec<u8> {
        let mut bytes = vec![];
        {
            let mut info = png::Info::with_size(1, 1);
            info.color_type = png::ColorType::Rgb;
            info.bit_depth = png::BitDepth::Eight;
            let mut writer = png::Encoder::with_info(&mut bytes, info)
                .unwrap()
                .write_header()
                .unwrap();
            // png 0.18.1 parses cICP but does not emit Info's cICP field.
            // Write real chunks so the test exercises the on-disk declaration.
            writer.write_chunk(png::chunk::cICP, &codes).unwrap();
            if srgb {
                writer.write_chunk(png::chunk::sRGB, &[0]).unwrap();
            }
            if conflicting_gamma {
                writer
                    .write_chunk(png::chunk::gAMA, &100_000u32.to_be_bytes())
                    .unwrap();
            }
            writer.write_image_data(&[128, 64, 32]).unwrap();
        }
        bytes
    }

    #[test]
    fn lone_png_gamma_uses_power_transfer_without_transforming_alpha() {
        for bits in [8, 16] {
            for priority in [0, 1, 2] {
                let mut bytes = Vec::new();
                let values = if bits == 8 {
                    [16u16, 128, 64, 128]
                } else {
                    [4096, 32768, 16384, 32768]
                };
                {
                    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(if bits == 8 {
                        png::BitDepth::Eight
                    } else {
                        png::BitDepth::Sixteen
                    });
                    let mut writer = encoder.write_header().unwrap();
                    writer
                        .write_chunk(png::chunk::gAMA, &45455u32.to_be_bytes())
                        .unwrap();
                    if priority == 1 {
                        writer.write_chunk(png::chunk::sRGB, &[0]).unwrap();
                    }
                    if priority == 2 {
                        writer
                            .write_chunk(png::chunk::cICP, &[1, 13, 0, 1])
                            .unwrap();
                    }
                    let samples: Vec<u8> = values
                        .iter()
                        .flat_map(|v| {
                            if bits == 8 {
                                vec![*v as u8]
                            } else {
                                v.to_be_bytes().to_vec()
                            }
                        })
                        .collect();
                    writer.write_image_data(&samples).unwrap();
                }
                let (_, source, image) = portable_decode(&bytes, 0, true).unwrap();
                let full = if bits == 8 { 255. } else { 65535. };
                let alpha = f64::from(values[3]) / full;
                let expected = if priority == 0 {
                    let linear = color::linear_srgb_to_rec2020(std::array::from_fn(|c| {
                        (f64::from(values[c]) / full).powf(100000. / 45455.) as f32
                    }));
                    assert!(
                        source
                            .provenance()
                            .contains("0.45455 dichiarato e applicato")
                    );
                    assert!(!source.faithful());
                    assert!(!source.provenance().starts_with("sRGB assunto"));
                    [
                        linear[0] * alpha as f32,
                        linear[1] * alpha as f32,
                        linear[2] * alpha as f32,
                        alpha as f32,
                    ]
                } else {
                    assert!(matches!(source, ColorSource::Declared(_)));
                    color::from_encoded_srgb(values.map(|v| (f64::from(v) / full) as f32))
                };
                for (got, want) in image.unwrap().pixels[0].into_iter().zip(expected) {
                    assert!(
                        (got - want).abs() < 1e-6,
                        "{bits} bit, priority {priority}: {got} != {want}"
                    );
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn secondary_ifd_cannot_rotate_primary_preview_and_absence_uses_jpeg_exif() {
        let first = include_bytes!("../tests/fixtures/second-ifd-orientation-1.dng");
        let second = include_bytes!("../tests/fixtures/second-ifd-orientation-6.dng");
        let a = PortablePreview.decode(first, 0).unwrap().2;
        let b = PortablePreview.decode(second, 0).unwrap().2;
        assert_eq!((a.width, a.height), (8, 4));
        assert_eq!(a.pixels, b.pixels);
        // Give the embedded JPEG its own EXIF orientation 6, without touching
        // either directory's image data. Test absence versus explicit 1/6.
        let preview = container::embedded_preview(second).unwrap();
        let jpeg = &second[preview.offset..preview.offset + preview.length];
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut oriented = jpeg[..2].to_vec();
        oriented.extend([0xff, 0xe1]);
        oriented.extend(((exif.len() + 2) as u16).to_be_bytes());
        oriented.extend(exif);
        oriented.extend(&jpeg[2..]);
        for (orientation, dimensions, label) in [
            (None, (4, 8), "Rotate90"),
            (Some(1u16), (8, 4), "NoTransforms"),
            (Some(6), (4, 8), "Rotate90"),
        ] {
            let mut bytes = second.to_vec();
            let offset = bytes.len() as u32;
            let count = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
            for i in 0..count {
                let entry = 10 + i * 12;
                match u16::from_le_bytes(bytes[entry..entry + 2].try_into().unwrap()) {
                    274 => {
                        if let Some(value) = orientation {
                            bytes[entry + 8..entry + 10].copy_from_slice(&value.to_le_bytes());
                        } else {
                            bytes[entry..entry + 2].copy_from_slice(&65000u16.to_le_bytes());
                        }
                    }
                    513 => bytes[entry + 8..entry + 12].copy_from_slice(&offset.to_le_bytes()),
                    514 => bytes[entry + 8..entry + 12]
                        .copy_from_slice(&(oriented.len() as u32).to_le_bytes()),
                    _ => {}
                }
            }
            bytes.extend(&oriented);
            let (probe, _) = PortablePreview.probe(&bytes).unwrap();
            let (info, _, raster) = PortablePreview.decode(&bytes, 0).unwrap();
            assert_eq!((probe.width, probe.height), dimensions);
            assert_eq!((raster.width, raster.height), dimensions);
            assert!(info.orientation.starts_with(label));
        }
    }

    #[test]
    fn cicp_is_declared_or_refused_before_any_srgb_fallback() {
        // PNG Third Edition §11.3.2.6: exact sRGB/full-range is supported;
        // linear, P3, PQ/HLG and limited range must never become assumed sRGB.
        let supported = png_with_cicp([1, 13, 0, 1], false, false);
        let (info, color, image) = portable_decode(&supported, 0, true).unwrap();
        assert!(matches!(color, ColorSource::Declared(_)));
        assert!(info.input_color.contains("cICP"));
        let expected = color::from_encoded_srgb([128. / 255., 64. / 255., 32. / 255., 1.]);
        assert_eq!(image.unwrap().pixels[0], expected);
        for codes in [
            [1, 8, 0, 1],
            [12, 13, 0, 1],
            [9, 16, 0, 1],
            [9, 18, 0, 1],
            [1, 13, 0, 0],
        ] {
            for srgb in [false, true] {
                let bytes = png_with_cicp(codes, srgb, false);
                for raster in [false, true] {
                    let error = portable_decode(&bytes, 0, raster).unwrap_err();
                    assert!(format!("{error:#}").contains("cICP"));
                }
                #[cfg(windows)]
                for engine in RawEngine::choices() {
                    let decoder = select_engine(Trust::External, engine).unwrap();
                    assert!(format!("{:#}", decoder.probe(&bytes).unwrap_err()).contains("cICP"));
                    assert!(
                        format!("{:#}", decoder.decode(&bytes, 0).unwrap_err()).contains("cICP")
                    );
                }
            }
        }
        let conflict = png_with_cicp([1, 13, 0, 1], false, true);
        assert!(format!("{:#}", png_color_source(&conflict).unwrap_err()).contains("conflitto"));
    }

    fn alpha_tiff(associated: u16, bits: u16, big: bool, gray: bool, pixel: [u16; 4]) -> Vec<u8> {
        let short = |n: u16| {
            if big {
                n.to_be_bytes()
            } else {
                n.to_le_bytes()
            }
        };
        let long = |n: u32| {
            if big {
                n.to_be_bytes()
            } else {
                n.to_le_bytes()
            }
        };
        let channels = if gray { 2u32 } else { 4 };
        let tags = 11u16;
        let bits_at = 8 + 2 + 12 * u32::from(tags) + 4;
        let pixels_at = bits_at + 2 * channels;
        let mut bytes = if big { b"MM".to_vec() } else { b"II".to_vec() };
        bytes.extend(short(42));
        bytes.extend(long(8));
        bytes.extend(short(tags));
        for (tag, kind, count, value) in [
            (256u16, 4u16, 1u32, 1u32),
            (257, 4, 1, 1),
            (258, 3, channels, bits_at),
            (259, 3, 1, 1),
            (262, 3, 1, if gray { 1 } else { 2 }),
            (273, 4, 1, pixels_at),
            (277, 3, 1, channels),
            (278, 4, 1, 1),
            (279, 4, 1, channels * u32::from(bits) / 8),
            (284, 3, 1, 1),
            (338, 3, 1, u32::from(associated)),
        ] {
            bytes.extend(short(tag));
            bytes.extend(short(kind));
            bytes.extend(long(count));
            if tag == 258 && gray {
                bytes.extend(short(bits));
                bytes.extend(short(bits));
            } else if kind == 3 && count == 1 {
                bytes.extend(short(value as u16));
                bytes.extend([0; 2]);
            } else {
                bytes.extend(long(value));
            }
        }
        bytes.extend(long(0));
        for _ in 0..channels {
            bytes.extend(short(bits));
        }
        let samples = if gray {
            vec![pixel[0], pixel[3]]
        } else {
            pixel.to_vec()
        };
        for sample in samples {
            if bits == 8 {
                bytes.push(sample as u8);
            } else {
                bytes.extend(short(sample));
            }
        }
        bytes
    }

    #[test]
    fn declared_tiff_colour_is_refused_before_srgb_assumption() {
        for (bytes, reason) in [
            (
                include_bytes!("../tests/fixtures/linear-srgb.tif").as_slice(),
                "TransferFunction",
            ),
            (
                include_bytes!("../tests/fixtures/p3-primaries.tif").as_slice(),
                "WhitePoint",
            ),
        ] {
            for raster in [false, true] {
                assert!(
                    format!("{:#}", portable_decode(bytes, 0, raster).unwrap_err())
                        .contains(reason)
                );
            }
            #[cfg(windows)]
            for engine in RawEngine::choices() {
                let decoder = select_engine(Trust::External, engine).unwrap();
                assert!(format!("{:#}", decoder.probe(bytes).unwrap_err()).contains(reason));
                assert!(format!("{:#}", decoder.decode(bytes, 0).unwrap_err()).contains(reason));
            }
        }
        let (_, colour, raster) =
            portable_decode(include_bytes!("../tests/fixtures/plain.tif"), 0, true).unwrap();
        assert!(matches!(colour, ColorSource::Assumed(_)));
        let expected = color::from_encoded_srgb([128. / 255., 128. / 255., 128. / 255., 1.]);
        assert_eq!(raster.unwrap().pixels[0], expected);
    }

    #[cfg(windows)]
    #[test]
    fn non_bayer_preview_has_the_same_stage_and_dimensions_in_probe_and_decode() {
        let bytes = include_bytes!("../tests/fixtures/non-bayer-preview.dng");
        assert!(container::declares_raw(bytes));
        let (probed, colour) = PortablePreview.probe(bytes).unwrap();
        assert_eq!((probed.width, probed.height), (8, 8));
        assert_eq!(probed.format, "RAW · anteprima");
        assert!(probed.decoder.contains("CFA non Bayer"));
        assert!(matches!(colour, ColorSource::Assumed(_)));
        for edge in [0, 4] {
            let (info, _, raster) = PortablePreview.decode(bytes, edge).unwrap();
            assert_eq!((info.source_width, info.source_height), (8, 8));
            assert_eq!(info.format, probed.format);
            assert_eq!(info.decoder, probed.decoder);
            assert_eq!(raster.width, if edge == 0 { 8 } else { 4 });
        }
        // AHD and the own mosaic recipe still refuse instead of substituting.
        for engine in [RawEngine::LibRawAhd, RawEngine::TrueRenderer] {
            assert!(
                select_engine(Trust::External, engine)
                    .unwrap()
                    .decode(bytes, 0)
                    .is_err()
            );
        }
    }

    #[test]
    fn tiff_associated_and_straight_alpha_produce_the_same_linear_white() {
        for bits in [8, 16] {
            let white = if bits == 8 { 255 } else { 65535 };
            let alpha = if bits == 8 { 128 } else { 32768 };
            for big in [false, true] {
                for gray in [false, true] {
                    let associated = alpha_tiff(1, bits, big, gray, [alpha; 4]);
                    let straight = alpha_tiff(2, bits, big, gray, [white, white, white, alpha]);
                    if gray {
                        // tiff 0.11.3 exposes gray+alpha as Multiband, which the
                        // image adapter does not support. Keep that explicit
                        // refusal rather than treating the alpha as gray RGB.
                        for bytes in [&associated, &straight] {
                            assert!(portable_decode(bytes, 0, false).is_err());
                            assert!(portable_decode(bytes, 0, true).is_err());
                        }
                        continue;
                    }
                    let (meta, _, a) = portable_decode(&associated, 0, true).unwrap();
                    let (_, _, b) = portable_decode(&straight, 0, true).unwrap();
                    assert_eq!(meta.native_bits, bits);
                    assert_eq!(a.unwrap().pixels, b.unwrap().pixels);
                    #[cfg(windows)]
                    for engine in RawEngine::choices() {
                        let decoder = select_engine(Trust::External, engine).unwrap();
                        assert_eq!(
                            decoder.decode(&associated, 0).unwrap().2.pixels,
                            decoder.decode(&straight, 0).unwrap().2.pixels
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn tiff_alpha_is_unassociated_before_transfer_and_zero_alpha_is_finite() {
        let bytes = alpha_tiff(1, 8, false, false, [64, 32, 0, 128]);
        let (_, _, raster) = portable_decode(&bytes, 0, true).unwrap();
        let expected = color::from_encoded_srgb([0.5, 0.25, 0., 128. / 255.]);
        for (actual, expected) in raster.unwrap().pixels[0].iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
        let bytes = alpha_tiff(1, 8, true, false, [128, 64, 32, 0]);
        assert_eq!(
            portable_decode(&bytes, 0, true).unwrap().2.unwrap().pixels[0],
            [0.; 4]
        );
        let ambiguous = alpha_tiff(0, 8, false, false, [128; 4]);
        assert!(portable_decode(&ambiguous, 0, false).is_err());
        assert!(portable_decode(&ambiguous, 0, true).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn external_png_colour_contract_applies_to_every_engine() {
        for engine in [
            RawEngine::LibRawBilinear,
            RawEngine::LibRawAhd,
            RawEngine::TrueRenderer,
        ] {
            let decoder = select_engine(Trust::External, engine).unwrap();
            for (bytes, reason) in [
                (png_with(Some(1.0), false), "gAMA"),
                (png_with(None, true), "cHRM"),
            ] {
                assert!(format!("{:#}", decoder.probe(&bytes).unwrap_err()).contains(reason));
                assert!(format!("{:#}", decoder.decode(&bytes, 0).unwrap_err()).contains(reason));
            }
            let bytes = include_bytes!("../../../corpus/05_Trasparenza.png");
            assert!(matches!(
                decoder.decode(bytes, 32).unwrap().1,
                ColorSource::Declared(_)
            ));
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn ordinary_tiff_with_thumbnail_keeps_primary_pixels_and_sixteen_bits() {
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(8, 4)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let jpeg = jpeg.into_inner();
        let entries = 11u16;
        let pixels_at = 8 + 2 + u32::from(entries) * 12 + 4;
        let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        bytes.extend(entries.to_le_bytes());
        for (tag, kind, value) in [
            (256u16, 4u16, 16u32),
            (257, 4, 8),
            (258, 3, 16),
            (259, 3, 1),
            (262, 3, 1),
            (273, 4, pixels_at),
            (277, 3, 1),
            (278, 4, 8),
            (279, 4, 256),
            (513, 4, pixels_at + 256),
            (514, 4, jpeg.len() as u32),
        ] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
        for _ in 0..128 {
            bytes.extend(32768u16.to_le_bytes());
        }
        bytes.extend(jpeg);
        assert!(container::embedded_preview(&bytes).is_ok());
        assert!(!container::declares_raw(&bytes));
        for engine in [
            RawEngine::LibRawBilinear,
            RawEngine::LibRawAhd,
            RawEngine::TrueRenderer,
        ] {
            let decoder = select_engine(Trust::External, engine).unwrap();
            let (probe, _) = decoder.probe(&bytes).unwrap();
            let (info, _, raster) = decoder.decode(&bytes, 0).unwrap();
            assert_eq!((probe.width, probe.height, info.native_bits), (16, 8, 16));
            assert_eq!((raster.width, raster.height), (16, 8));
            assert!(!info.format.starts_with("RAW"));
            assert!(raster.pixels[0][1] > 0.2); // Primary grey, never the black JPEG.
        }
    }
    /// Encode a 2x2 opaque PNG, optionally stating a colour space.
    fn png_with(gamma: Option<f32>, chromaticities: bool) -> Vec<u8> {
        let mut bytes = vec![];
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            if let Some(gamma) = gamma {
                encoder.set_source_gamma(png::ScaledFloat::new(gamma));
            }
            if chromaticities {
                // Display P3 primaries: a space this build cannot apply.
                encoder.set_source_chromaticities(png::SourceChromaticities::new(
                    (0.3127, 0.3290),
                    (0.680, 0.320),
                    (0.265, 0.690),
                    (0.150, 0.060),
                ));
            }
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0u8; 12]).unwrap();
        }
        bytes
    }

    /// The corpus states sRGB in its own chunks. The pipeline must read that
    /// statement rather than assume it, or the same provenance line would
    /// appear for a file that says nothing at all.
    #[test]
    fn corpus_colour_space_is_read_from_the_file_not_assumed() {
        let source = include_bytes!("../../../corpus/05_Trasparenza.png");
        let color = png_color_source(source).unwrap();
        assert_eq!(color, ColorSource::Declared("sRGB".into()));
        assert!(color.faithful());
        assert!(color.provenance().contains("dichiarato dal file"));
    }

    /// A file that states a colour space this build cannot apply is refused.
    /// Rendering it as sRGB would silently change its colours, which is the
    /// failure the port exists to prevent.
    #[test]
    fn declared_but_unsupported_colour_spaces_are_refused_never_assumed() {
        let chromaticities = png_color_source(&png_with(None, true)).unwrap_err();
        assert!(format!("{chromaticities:#}").contains("cHRM"));
        let curve = png_color_source(&png_with(Some(1.0), false)).unwrap_err();
        assert!(format!("{curve:#}").contains("gAMA"));
    }

    /// Silence is not a declaration. A file with no colour chunk still
    /// renders, but the provenance must show the assumption as an assumption.
    #[test]
    fn absent_colour_information_stays_visible_as_an_assumption() {
        let color = png_color_source(&png_with(None, false)).unwrap();
        assert_eq!(color, ColorSource::Assumed("nessun chunk di colore".into()));
        assert!(!color.faithful());
        assert!(color.provenance().starts_with("sRGB assunto"));
        // The sRGB gamma fixes the curve and leaves the primaries unstated.
        let gamma_only = png_color_source(&png_with(Some(0.45455), false)).unwrap();
        assert!(!gamma_only.faithful());
    }

    /// The recipe name lives in two files: this crate publishes it, the shim
    /// header sets the parameters. They must not drift, because the cache key
    /// carries one while the pixels come from the other.
    #[cfg(windows)]
    #[test]
    fn the_published_recipe_matches_the_one_the_shim_applies() {
        let header = include_str!("../../../native/libraw/tr_libraw.h");
        let declared = header
            .lines()
            .find_map(|line| line.strip_prefix("#define TR_LIBRAW_RECIPE "))
            .expect("TR_LIBRAW_RECIPE nell'header")
            .trim()
            .trim_matches('"');
        assert_eq!(declared, RECIPE);
    }

    /// Runs against real camera files, which are private and never committed.
    /// `TR_RAW_SAMPLE` names a folder of them; without it there is nothing to
    /// assert and the test reports that instead of passing quietly.
    #[test]
    #[ignore = "requires TR_RAW_SAMPLE pointing at authorised camera files"]
    fn real_containers_deliver_their_embedded_preview_and_declare_the_stage() {
        let Some(folder) = std::env::var_os("TR_RAW_SAMPLE").map(std::path::PathBuf::from) else {
            // `--include-ignored` must not fail on a machine with no samples.
            eprintln!("TR_RAW_SAMPLE non impostata: nessun campione da verificare");
            return;
        };
        let (mut seen, mut developed) = (0, 0);
        for entry in std::fs::read_dir(&folder).expect("cartella campioni") {
            let path = entry.expect("voce").path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nef") || e.eq_ignore_ascii_case("dng"))
            {
                continue;
            }
            let bytes = std::fs::read(&path).expect("lettura campione");
            let (info, color) = PortablePreview
                .probe(&bytes)
                .expect("probe del contenitore");
            // Whichever stage served it, the provenance names that stage and
            // the colour claim matches it: development declares its space, the
            // preview says sRGB was assumed. Nothing is left ambiguous.
            match info.format.as_str() {
                "RAW" => {
                    assert!(color.faithful(), "sviluppo senza spazio dichiarato");
                    assert!(info.input_color.contains("sviluppo RAW"));
                    assert_eq!(info.native_bits, 16, "sviluppo non a 16 bit");
                    assert!(info.decoder.contains(RECIPE), "{}", info.decoder);
                    developed += 1;
                }
                "RAW · anteprima" => {
                    assert!(!color.faithful());
                    assert!(info.input_color.starts_with("sRGB assunto"));
                }
                other => panic!("stadio non dichiarato: {other}"),
            }
            // Guards against serving the 160x120 thumbnail that IFD0 of a
            // TIFF-container NEF advertises, not against small sensors.
            assert!(info.width >= 512 && info.height >= 384, "{info:?}");

            let (full, _, raster) = PortablePreview
                .decode(&bytes, 512)
                .expect("sviluppo o anteprima");
            assert_eq!(
                full.format, info.format,
                "stadio diverso fra probe e decode: {}",
                full.decoder
            );
            assert_eq!(
                [full.source_width, full.source_height],
                [info.source_width, info.source_height]
            );
            assert_eq!(raster.width.max(raster.height), 512);
            assert!(
                raster.pixels.iter().any(|p| p[0] != raster.pixels[0][0]),
                "raster costante: nessun contenuto"
            );
            seen += 1;
        }
        assert!(seen > 0, "nessun RAW nella cartella indicata");
        eprintln!("{seen} contenitori, di cui {developed} sviluppati dal mosaico");
    }

    /// Ordinary formats must still open on the external port. A plain TIFF is
    /// the trap: it opens with the same header as a RAW container, so a decoder
    /// that assumes every container hides a mosaic or a preview would refuse a
    /// perfectly ordinary photograph.
    #[test]
    fn ordinary_formats_are_not_lost_to_the_container_path() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::Tiff,
        ] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(8, 6, |x, y| {
                image::Rgb([(x * 30) as u8, (y * 40) as u8, 90])
            }))
            .write_to(&mut bytes, format)
            .unwrap();
            let bytes = bytes.into_inner();
            let (info, _) = PortablePreview
                .probe(&bytes)
                .unwrap_or_else(|e| panic!("{format:?} rifiutato in probe: {e:#}"));
            assert_eq!([info.width, info.height], [8, 6], "{format:?}");
            let (_, _, raster) = PortablePreview
                .decode(&bytes, 0)
                .unwrap_or_else(|e| panic!("{format:?} rifiutato in decode: {e:#}"));
            assert_eq!([raster.width, raster.height], [8, 6], "{format:?}");
        }
    }

    /// The controlled port exists in every build; the external one is a
    /// platform capability. A build without it must say so instead of
    /// silently falling back to the corpus decoder for arbitrary bytes.
    #[test]
    fn port_selection_separates_controlled_and_external_trust() {
        let controlled = select(Trust::Controlled).unwrap();
        assert_eq!(controlled.name(), "corpus-png");
        match select(Trust::External) {
            Ok(external) => {
                assert!(external_decoder().is_some());
                assert_ne!(external.name(), controlled.name());
            }
            Err(error) => {
                assert!(external_decoder().is_none());
                assert!(format!("{error:#}").contains("non disponibile"));
            }
        }
    }
    /// A source outside the corpus never reaches a decoder on the portable
    /// transport, whichever platform provides the external port.
    #[test]
    fn portable_transport_never_grants_external_trust() {
        let arbitrary = b"\x89PNG\r\n\x1a\nnot a corpus file";
        let mut requests = vec![];
        protocol::write_control(
            &mut requests,
            protocol::REQUEST,
            7,
            &DecodeRequest {
                raw_engine: Default::default(),
                source_len: arbitrary.len(),
                max_edge: 0,
                intent: protocol::DecodeIntent::Probe,
                maximum_output_bytes: MAX_PIXELS as u64 * 16,
            },
        )
        .unwrap();
        requests.extend_from_slice(arbitrary);
        let mut responses = vec![];
        assert!(serve(requests.as_slice(), &mut responses).is_err()); // EOF after the job.
        let (kind, id, control) = protocol::read_control(&mut responses.as_slice()).unwrap();
        assert_eq!((kind, id), (protocol::ERROR, 7));
        assert!(
            protocol::parse::<String>(&control)
                .unwrap()
                .contains("corpus R0")
        );
    }
    #[test]
    fn transport_rejects_unapproved_bytes_and_accepts_next_corpus_job() {
        let invalid = b"\x89PNG\r\n\x1a\nnot a corpus file";
        let valid = include_bytes!("../../../corpus/05_Trasparenza.png");
        let mut requests = vec![];
        for (id, source) in [(1, invalid.as_slice()), (2, valid.as_slice())] {
            protocol::write_control(
                &mut requests,
                protocol::REQUEST,
                id,
                &DecodeRequest {
                    raw_engine: Default::default(),
                    source_len: source.len(),
                    max_edge: 300,
                    intent: protocol::DecodeIntent::LegacyRaster,
                    maximum_output_bytes: MAX_PIXELS as u64 * 16,
                },
            )
            .unwrap();
            requests.extend_from_slice(source);
        }
        let mut responses = vec![];
        assert!(serve(requests.as_slice(), &mut responses).is_err()); // EOF after the two jobs.
        let mut reader = responses.as_slice();
        let (kind, id, control) = protocol::read_control(&mut reader).unwrap();
        assert_eq!((kind, id), (protocol::ERROR, 1));
        assert!(
            protocol::parse::<String>(&control)
                .unwrap()
                .contains("corpus R0")
        );
        let (kind, id, control) = protocol::read_control(&mut reader).unwrap();
        assert_eq!((kind, id), (protocol::RESPONSE, 2));
        let info = protocol::parse::<RasterInfo>(&control).unwrap();
        let image = protocol::read_raster(&mut reader, &info).unwrap();
        assert_eq!((image.width, image.height), (300, 200));
        assert!(reader.is_empty());
    }
    #[test]
    fn native_sixteen_bit_samples_are_not_reduced_to_eight() {
        let source = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
            2,
            1,
            vec![32768, 32768, 32768, 65535, 32769, 32769, 32769, 65535],
        )
        .unwrap();
        let mut bytes = Cursor::new(vec![]);
        DynamicImage::ImageRgba16(source)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let (info, raster) = decode(bytes.get_ref(), 0).unwrap();
        assert_eq!(info.native_bits, 16);
        assert_ne!(raster.pixels[0][0], raster.pixels[1][0]);
        let expected = color::srgb_to_linear(32768.0 / 65535.0);
        assert!((raster.pixels[0][0] - expected).abs() < 2e-6);
    }
    #[test]
    fn malformed_files_do_not_produce_valid_pixels() {
        for bytes in [b"garbage".as_slice(), b"\x89PNG\r\n\x1a\n".as_slice()] {
            assert!(decode(bytes, 320).is_err());
        }
    }
    #[test]
    fn decoding_known_corpus_and_reduction() {
        let bytes = include_bytes!("../../../corpus/05_Trasparenza.png");
        let (info, raster) = decode(bytes, 300).unwrap();
        assert_eq!((raster.width, raster.height), (300, 200));
        assert_eq!(info.filter, color::FILTER_VERSION);
        assert_eq!(raster.pixels[0][3], 0.0);
    }
    #[test]
    fn reference_mip_transport_matches_full_graph_and_enforces_output_reservation() {
        for bytes in [
            include_bytes!("../../../corpus/05_Trasparenza.png").as_slice(),
            include_bytes!("../../../corpus/04_Frequenze_radiali.png").as_slice(),
        ] {
            let (_, raster) = decode(bytes, 0).unwrap();
            let reference = tr_core::resample::Pyramid::new(raster).unwrap();
            let source = reference.source();
            let (size, base) = protocol::mip_geometry([source.width, source.height], 128);
            let maximum = u64::from(size[0]) * u64::from(size[1]) * 16;
            for (id, output_bytes) in [(1, maximum), (2, maximum - 1)] {
                let mut wire = vec![];
                protocol::write_control(
                    &mut wire,
                    protocol::REQUEST,
                    id,
                    &DecodeRequest {
                        raw_engine: RawEngine::default(),
                        source_len: bytes.len(),
                        max_edge: 128,
                        intent: protocol::DecodeIntent::ReferenceMip { cpu_threads: 2 },
                        maximum_output_bytes: output_bytes,
                    },
                )
                .unwrap();
                wire.extend_from_slice(bytes);
                let mut output = vec![];
                assert!(serve(&wire[..], &mut output).is_err()); // EOF after this request.
                let mut reply = &output[..];
                let (kind, received, payload) = protocol::read_control(&mut reply).unwrap();
                assert_eq!(received, id);
                if output_bytes < maximum {
                    assert_eq!(kind, protocol::ERROR);
                    assert!(reply.is_empty());
                } else {
                    assert_eq!(kind, protocol::RESPONSE);
                    let info: RasterInfo = protocol::parse(&payload).unwrap();
                    assert_eq!(info.reference_mip.unwrap().base, base);
                    let actual = protocol::read_raster(&mut reply, &info).unwrap();
                    assert_eq!(actual.pixels, reference.levels()[base as usize].pixels);
                    assert!(reply.is_empty());
                }
            }
        }
    }
}
