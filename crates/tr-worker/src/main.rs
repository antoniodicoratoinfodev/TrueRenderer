use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageDecoder, Limits, codecs::png::PngDecoder};
use std::io::{self, BufReader, BufWriter, Cursor, Read};
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
        "Limite R0: 8.388.608 pixel per immagine"
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

fn run() -> Result<()> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
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
        match decode(&bytes, request.max_edge) {
            Ok((info, raster)) => {
                protocol::write_control(&mut output, protocol::RESPONSE, id, &info)?;
                protocol::write_raster(&mut output, &raster)?;
            }
            Err(error) => {
                let error: String = format!("{error:#}").chars().take(2048).collect();
                protocol::write_control(&mut output, protocol::ERROR, id, &error)?;
            }
        }
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("tr-worker: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
