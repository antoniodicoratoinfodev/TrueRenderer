//! Versioned export contract. Rendered images and sensor mosaics are distinct products.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_ENCODED: u64 = 512 * 1024 * 1024;

/// Canonical sRGB integer output, shared by PNG/TIFF encoding and output proof.
/// The input remains extended, linear, premultiplied Rec.2020.
pub fn srgb16(pixel: crate::color::Pixel) -> ([u16; 4], u64) {
    let alpha = pixel[3];
    let straight = if alpha > 0. {
        [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha]
    } else {
        [0.; 3]
    };
    let rgb = crate::color::rec2020_to_linear_srgb(straight).map(crate::color::linear_to_srgb);
    let clipped = rgb.iter().filter(|v| !(0. ..=1.).contains(*v)).count() as u64;
    (
        [rgb[0], rgb[1], rgb[2], alpha].map(|v| (v.clamp(0., 1.) * 65535.).round() as u16),
        clipped,
    )
}

/// Project a private render copy to the native PNG/TIFF16 output before mips.
/// No 8-bit intermediate and no change to the retained extended working image.
pub fn proof_srgb16(image: &mut crate::color::LinearImage) {
    for pixel in &mut image.pixels {
        let (encoded, _) = srgb16(*pixel);
        *pixel = crate::color::from_encoded_srgb(encoded.map(|v| v as f32 / 65535.));
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Jpeg,
    Png8,
    Png16,
    Tiff16,
    TiffFloat32,
    DngLinear16,
    DngRaw,
}
impl Format {
    pub const ALL: [Self; 7] = [
        Self::Jpeg,
        Self::Png8,
        Self::Png16,
        Self::Tiff16,
        Self::TiffFloat32,
        Self::DngLinear16,
        Self::DngRaw,
    ];
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png8 | Self::Png16 => "png",
            Self::Tiff16 | Self::TiffFloat32 => "tif",
            _ => "dng",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG · sRGB 8 bit",
            Self::Png8 => "PNG · sRGB 8 bit",
            Self::Png16 => "PNG · sRGB 16 bit",
            Self::Tiff16 => "TIFF · sRGB 16 bit",
            Self::TiffFloat32 => "TIFF · Rec.2020 lineare float32",
            Self::DngLinear16 => "DNG lineare · RGB sviluppato 16 bit",
            Self::DngRaw => "DNG RAW · mosaico area attiva originale",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Compression {
    Fast,
    #[default]
    Balanced,
    Best,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub format: Format,
    pub jpeg_quality: u8,
    pub compression: Compression,
    /// Zero means full native development, never a resident thumbnail.
    pub long_edge: u32,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Jpeg,
            jpeg_quality: 90,
            compression: Compression::Balanced,
            long_edge: 0,
        }
    }
}
impl Options {
    pub fn validate(self) -> Result<()> {
        ensure!(
            (1..=100).contains(&self.jpeg_quality),
            "Qualità JPEG: 1–100"
        );
        ensure!(
            self.long_edge <= 16384,
            "Lato export massimo: 16384 (0 = originale)"
        );
        ensure!(
            self.format != Format::DngRaw || self.long_edge == 0,
            "Un mosaico RAW non può essere ridimensionato"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub clipped_channels: u64,
    pub description: String,
}
impl Info {
    pub fn validate(&self, options: Options, limit: u64) -> Result<()> {
        options.validate()?;
        let pixels = self.width as u64 * self.height as u64;
        ensure!(
            self.width > 0 && self.height > 0 && pixels <= crate::color::MAX_PIXELS as u64,
            "Dimensioni export fuori quota"
        );
        ensure!(
            options.long_edge == 0 || self.width.max(self.height) <= options.long_edge,
            "Export supera il lato richiesto"
        );
        ensure!(
            self.format == options.format
                && self.bytes > 0
                && self.bytes <= limit.min(MAX_ENCODED)
                && self.description.len() <= 2048
                && self.clipped_channels <= pixels * 3,
            "Risposta export incoerente o fuori quota"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_is_not_a_resized_render() {
        assert!(
            Options {
                format: Format::DngRaw,
                long_edge: 1000,
                ..Options::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Options {
                jpeg_quality: 0,
                ..Options::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Options {
                format: Format::Png16,
                ..Options::default()
            }
            .validate()
            .is_ok()
        );
    }
}
