//! Named RGB destinations. Working pixels remain extended linear Rec.2020.
use crate::{color, export};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorSpace {
    #[default]
    Srgb,
    AdobeRgb,
    DisplayP3,
    LinearRec2020,
}
impl ColorSpace {
    pub const ALL: [Self; 4] = [
        Self::Srgb,
        Self::AdobeRgb,
        Self::DisplayP3,
        Self::LinearRec2020,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::AdobeRgb => "Adobe RGB (1998)",
            Self::DisplayP3 => "Display P3",
            Self::LinearRec2020 => "Rec.2020 lineare",
        }
    }
    fn primaries(self) -> [[f64; 2]; 3] {
        match self {
            Self::Srgb => [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]],
            Self::AdobeRgb => [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]],
            Self::DisplayP3 => [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]],
            Self::LinearRec2020 => [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        }
    }
    fn native(self) -> moxcms::Matrix3d {
        let p = self.primaries().map(|[x, y]| [x / y, 1., (1. - x - y) / y]);
        moxcms::ColorProfile::rgb_to_xyz_d(
            moxcms::Matrix3d {
                v: [0, 1, 2].map(|r| [p[0][r], p[1][r], p[2][r]]),
            },
            white(),
        )
    }
    pub fn encode_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        if self == Self::Srgb {
            return color::rec2020_to_linear_srgb(rgb).map(color::linear_to_srgb);
        }
        if self == Self::LinearRec2020 {
            return rgb;
        }
        static MATRICES: OnceLock<[[[f64; 3]; 3]; 2]> = OnceLock::new();
        let matrices = MATRICES.get_or_init(|| {
            [Self::AdobeRgb, Self::DisplayP3].map(|space| {
                space
                    .native()
                    .inverse()
                    .mat_mul_const(Self::LinearRec2020.native())
                    .v
            })
        });
        let m = matrices[usize::from(self == Self::DisplayP3)];
        m.map(|row| {
            let v =
                (row[0] * rgb[0] as f64 + row[1] * rgb[1] as f64 + row[2] * rgb[2] as f64) as f32;
            if self == Self::AdobeRgb {
                v.signum() * v.abs().powf(256. / 563.)
            } else {
                color::linear_to_srgb(v)
            }
        })
    }
    pub fn rgba16(self, pixel: color::Pixel) -> ([u16; 4], u64) {
        if self == Self::Srgb {
            return export::srgb16(pixel);
        }
        let a = pixel[3];
        let rgb = self.encode_rgb(if a > 0. {
            [pixel[0] / a, pixel[1] / a, pixel[2] / a]
        } else {
            [0.; 3]
        });
        let clipped = rgb.iter().filter(|v| !(0. ..=1.).contains(*v)).count() as u64;
        (
            [rgb[0], rgb[1], rgb[2], a].map(|v| (v.clamp(0., 1.) * 65535.).round() as u16),
            clipped,
        )
    }
    pub fn icc(self) -> Result<Vec<u8>> {
        if self == Self::Srgb {
            return aligned_icc(&export::profile(false)?);
        }
        if self == Self::LinearRec2020 {
            return aligned_icc(&export::profile(true)?);
        }
        let mut p = if self == Self::AdobeRgb {
            moxcms::ColorProfile::new_adobe_rgb()
        } else {
            moxcms::ColorProfile::new_display_p3()
        };
        p.cicp = None;
        p.rendering_intent = moxcms::RenderingIntent::RelativeColorimetric;
        p.white_point = moxcms::Xyzd {
            x: 0.9642,
            y: 1.,
            z: 0.8249,
        };
        p.media_white_point = Some(p.white_point);
        let chad = moxcms::adaption_matrix_d(white().to_xyz(), p.white_point.to_xyz());
        let pcs = chad.mat_mul_const(self.native()).v;
        [p.red_colorant, p.green_colorant, p.blue_colorant] = [0, 1, 2].map(|c| moxcms::Xyzd {
            x: pcs[0][c],
            y: pcs[1][c],
            z: pcs[2][c],
        });
        p.chromatic_adaptation = Some(chad);
        aligned_icc(&p.encode()?)
    }
}
// moxcms 0.8.1 omits padding between generated tags and at EOF. ICC tag
// offsets and total profile size must be aligned to four bytes (ICC.1 §7.2).
// ImageIO's PNG reader rejects unaligned total sizes rather than using the ICC.
// Only trusted, procedurally generated profiles enter this function.
fn aligned_icc(bytes: &[u8]) -> Result<Vec<u8>> {
    use anyhow::ensure;
    ensure!(bytes.len() >= 132, "Profilo generato incompleto");
    let number = |i| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let count = number(128);
    ensure!(
        count <= (bytes.len() - 132) / 12,
        "Tabella ICC generata non valida"
    );
    let mut output = bytes[..132 + 12 * count].to_vec();
    for tag in 0..count {
        let entry = 132 + 12 * tag;
        let start = number(entry + 4);
        let size = number(entry + 8);
        ensure!(
            start <= bytes.len() && size <= bytes.len() - start,
            "Tag ICC generato fuori limite"
        );
        let offset = output.len() as u32;
        output[entry + 4..entry + 8].copy_from_slice(&offset.to_be_bytes());
        output.extend_from_slice(&bytes[start..start + size]);
        output.resize(output.len().div_ceil(4) * 4, 0);
    }
    let length = output.len() as u32;
    output[..4].copy_from_slice(&length.to_be_bytes());
    // Generated profiles have no profile ID; do not retain an invalid digest.
    output[84..100].fill(0);
    Ok(output)
}
fn white() -> moxcms::Xyzd {
    moxcms::XyY {
        x: 0.3127,
        y: 0.3290,
        yb: 1.,
    }
    .to_xyzd()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icc_tag_offsets_and_profile_size_are_four_byte_aligned() {
        for space in ColorSpace::ALL {
            let bytes = space.icc().unwrap();
            let number = |i| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
            assert_eq!(number(0), bytes.len());
            assert_eq!(bytes.len() % 4, 0);
            for i in 0..number(128) {
                let tag = 132 + 12 * i;
                assert_eq!(number(tag + 4) % 4, 0);
                assert!(number(tag + 4) + number(tag + 8) <= bytes.len());
            }
            moxcms::ColorProfile::new_from_slice(&bytes).unwrap();
        }
    }
    #[test]
    fn generated_icc_matches_published_primaries_white_and_encoded_pixels() {
        for (space, xy) in [
            (
                ColorSpace::AdobeRgb,
                [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]],
            ),
            (
                ColorSpace::DisplayP3,
                [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]],
            ),
        ] {
            let p = moxcms::ColorProfile::new_from_slice(&space.icc().unwrap()).unwrap();
            assert!(p.cicp.is_none());
            let native = p
                .chromatic_adaptation
                .unwrap()
                .inverse()
                .mat_mul_const(p.rgb_to_xyz_matrix())
                .v;
            for (c, expected) in xy.iter().enumerate() {
                let sum = native[0][c] + native[1][c] + native[2][c];
                assert!((native[0][c] / sum - expected[0]).abs() < 0.0001);
                assert!((native[1][c] / sum - expected[1]).abs() < 0.0001);
            }
            for v in [0., 0.01, 0.18, 0.5, 1.] {
                let rgb = space.encode_rgb([v; 3]);
                let expected = if space == ColorSpace::AdobeRgb {
                    v.powf(256. / 563.)
                } else {
                    color::linear_to_srgb(v)
                };
                assert!(rgb.iter().all(|x| (x - expected).abs() < 0.00001));
            }
        }
        // Published Adobe RGB (1998) matrix: sRGB red is less saturated in this gamut.
        let red = ColorSpace::AdobeRgb.encode_rgb(color::linear_srgb_to_rec2020([1., 0., 0.]));
        assert!((red[0] - 0.85861).abs() < 0.0001);
        assert!(red[1].abs() < 0.002 && red[2].abs() < 0.002);
    }
    #[test]
    fn destinations_preserve_alpha_and_report_gamut_clipping() {
        for space in ColorSpace::ALL {
            let (p, _) = space.rgba16([0.09, 0.09, 0.09, 0.5]);
            assert_eq!(p[3], 32768);
            assert_eq!(space.rgba16([0.; 4]).0, [0; 4]);
            assert!(space.rgba16([-1., 2., 0., 1.]).1 > 0);
        }
    }
}
