#[cfg(target_os = "macos")]
mod native;
use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageDecoder, Limits, codecs::png::PngDecoder};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use tr_core::{
    color::{self, LinearImage, MAX_PIXELS},
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

pub fn serve<R: Read, W: Write>(input: R, output: W) -> Result<()> {
    serve_with_policy(input, output, false)
}
fn serve_with_policy<R: Read, W: Write>(input: R, output: W, external: bool) -> Result<()> {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    let policy = tr_core::corpus::CorpusPolicy::default();
    loop {
        let (kind, id, data) = protocol::read_control(&mut input).context("Lettura richiesta")?;
        ensure!(kind == protocol::REQUEST, "Messaggio non richiesta");
        let request: DecodeRequest = protocol::parse(&data)?;
        ensure!(
            request.source_len > 0 && request.source_len <= MAX_SOURCE && request.max_edge <= 2048,
            "Richiesta fuori quota"
        );
        let mut bytes = vec![0; request.source_len];
        input.read_exact(&mut bytes)?;
        let decoded = (|| -> Result<(RasterInfo, Option<LinearImage>)> {
            let controlled = policy.approves_bytes(&bytes);
            ensure!(
                controlled || external,
                "Sorgente rifiutata: non appartiene al corpus R0 compilato nel decoder"
            );
            let metadata = if controlled {
                probe(&bytes)?
            } else {
                #[cfg(target_os = "macos")]
                {
                    native::probe(&bytes)?
                }
                #[cfg(not(target_os = "macos"))]
                {
                    anyhow::bail!("Decoder esterno non disponibile");
                }
            };
            if request.intent == protocol::DecodeIntent::Probe {
                return Ok((metadata, None));
            }
            ensure!(
                request.intent != protocol::DecodeIntent::FullSource || request.max_edge == 0,
                "Intent Full con scala ridotta"
            );
            // This backend requires a full frame even for legacy reductions.
            ensure!(
                metadata.width as u64 * metadata.height as u64 * 16 <= request.maximum_output_bytes,
                "Raster oltre prenotazione"
            );
            let (info, raster) = if controlled {
                decode(&bytes, request.max_edge)?
            } else {
                #[cfg(target_os = "macos")]
                {
                    native::decode(&bytes, request.max_edge)?
                }
                #[cfg(not(target_os = "macos"))]
                {
                    anyhow::bail!("Decoder esterno non disponibile");
                }
            };
            ensure!(
                [info.source_width, info.source_height]
                    == [metadata.source_width, metadata.source_height],
                "Dimensioni diverse dal probe"
            );
            Ok((info, Some(raster)))
        })();
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
}
