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
            Self::DngRaw => "DNG RAW · mosaico sensore originale",
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

// Shared by photographic and composition export.
pub fn profile(linear: bool) -> Result<Vec<u8>> {
    let mut profile = if linear {
        moxcms::ColorProfile::new_bt2020()
    } else {
        moxcms::ColorProfile::new_srgb()
    };
    profile.cicp = None; // Matrix/TRC RGB profile, not a video range/matrix declaration.
    profile.rendering_intent = moxcms::RenderingIntent::RelativeColorimetric;
    profile.white_point = moxcms::Xyzd {
        x: 0.9642,
        y: 1.,
        z: 0.8249,
    };
    profile.media_white_point = Some(profile.white_point);
    if linear {
        profile.red_trc = Some(moxcms::ToneReprCurve::Parametric(vec![1.]));
        profile.green_trc = profile.red_trc.clone();
        profile.blue_trc = profile.red_trc.clone();
        // Derive both colorants and chad from the same BT.2020 D65 white.
        // The profile describes linear working samples, not the BT.2020 OETF.
        let white = moxcms::XyY {
            x: 0.3127,
            y: 0.3290,
            yb: 1.,
        }
        .to_xyzd();
        let primaries = moxcms::ColorPrimaries::BT_2020;
        let [r, g, b] = [primaries.red, primaries.green, primaries.blue].map(|p| p.to_xyzd());
        let native = moxcms::ColorProfile::rgb_to_xyz_d(
            moxcms::Matrix3d {
                v: [[r.x, g.x, b.x], [r.y, g.y, b.y], [r.z, g.z, b.z]],
            },
            white,
        );
        let chad = moxcms::adaption_matrix_d(white.to_xyz(), profile.white_point.to_xyz());
        let pcs = chad.mat_mul_const(native).v;
        [
            profile.red_colorant,
            profile.green_colorant,
            profile.blue_colorant,
        ] = [0, 1, 2].map(|c| moxcms::Xyzd {
            x: pcs[0][c],
            y: pcs[1][c],
            z: pcs[2][c],
        });
        profile.chromatic_adaptation = Some(chad);
        profile.description = Some(moxcms::ProfileText::Localizable(vec![
            moxcms::LocalizableString::new(
                "en".into(),
                "US".into(),
                "TrueRenderer Linear Rec.2020 D65".into(),
            ),
        ]));
    } else {
        // ICC sRGB.pdf §B.2: PCS D50 colorants and D65 -> D50 adaptation.
        // moxcms 0.8.1 defaults use a temperature-derived white and put the
        // Bradford cone matrix itself in chad, which is not this adaptation.
        // Keep the analytic sRGB TRC and the encoded image samples unchanged.
        profile.red_colorant = moxcms::Xyzd {
            x: 0.436030342570117,
            y: 0.222438466210245,
            z: 0.013897440074263,
        };
        profile.green_colorant = moxcms::Xyzd {
            x: 0.385101860087134,
            y: 0.716942745571917,
            z: 0.097076381494207,
        };
        profile.blue_colorant = moxcms::Xyzd {
            x: 0.143067806654203,
            y: 0.060618777416563,
            z: 0.713926257896652,
        };
        profile.chromatic_adaptation = Some(moxcms::Matrix3d {
            v: [
                [1.047844353856414, 0.022898981050086, -0.050206647741605],
                [0.029549007606644, 0.990508028941971, -0.017074711360960],
                [-0.009250984365223, 0.015072338237051, 0.751717835079977],
            ],
        });
    }
    Ok(profile.encode()?)
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
