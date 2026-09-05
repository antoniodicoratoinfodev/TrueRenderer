//! Analytic SDR reference: encoded sRGB -> premultiplied linear Rec.2020.
//! No clipping in the working space. Quantization/clipping only at display output.
use anyhow::{Result, ensure};

pub type Pixel = [f32; 4];
pub const MAX_PIXELS: usize = 8_388_608;
pub const FILTER_VERSION: &str = "area-linear-premultiplied-v1";

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn mul(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
pub fn linear_srgb_to_rec2020(rgb: [f32; 3]) -> [f32; 3] {
    mul(
        [
            [0.627404, 0.329282, 0.0433136],
            [0.069097, 0.91954, 0.0113612],
            [0.0163916, 0.0880132, 0.895595],
        ],
        rgb,
    )
}
pub fn rec2020_to_linear_srgb(rgb: [f32; 3]) -> [f32; 3] {
    mul(
        [
            [1.660491, -0.5876411, -0.0728499],
            [-0.1245505, 1.1328999, -0.0083494],
            [-0.0181508, -0.1005789, 1.1187297],
        ],
        rgb,
    )
}
pub fn from_encoded_srgb(rgba: Pixel) -> Pixel {
    let rgb = linear_srgb_to_rec2020([
        srgb_to_linear(rgba[0]),
        srgb_to_linear(rgba[1]),
        srgb_to_linear(rgba[2]),
    ]);
    [
        rgb[0] * rgba[3],
        rgb[1] * rgba[3],
        rgb[2] * rgba[3],
        rgba[3],
    ]
}
pub fn display_pixel(pixel: Pixel, background: f32) -> [u8; 4] {
    let bg = srgb_to_linear(background);
    let rgb = rec2020_to_linear_srgb([
        pixel[0] + bg * (1.0 - pixel[3]),
        pixel[1] + bg * (1.0 - pixel[3]),
        pixel[2] + bg * (1.0 - pixel[3]),
    ]);
    let rgb = rgb.map(|v| (linear_to_srgb(v).clamp(0.0, 1.0) * 255.0).round() as u8);
    [rgb[0], rgb[1], rgb[2], 255]
}

#[derive(Debug, Clone)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Pixel>,
}
impl LinearImage {
    pub fn new(width: u32, height: u32, pixels: Vec<Pixel>) -> Result<Self> {
        let count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| anyhow::anyhow!("Dimensioni in overflow"))?;
        ensure!(
            width > 0 && height > 0 && count <= MAX_PIXELS,
            "Dimensioni fuori quota R0"
        );
        ensure!(count == pixels.len(), "Lunghezza raster incoerente");
        ensure!(
            pixels
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&p[3])),
            "Raster non finito o alpha invalida"
        );
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn to_display(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|p| display_pixel(*p, 119.0 / 255.0))
            .collect()
    }
    pub fn histogram(&self) -> [[u32; 256]; 3] {
        let mut bins = [[0; 256]; 3];
        for pixel in &self.pixels {
            // This histogram explicitly describes the composited 8-bit display buffer.
            let rgba = display_pixel(*pixel, 119.0 / 255.0);
            for c in 0..3 {
                bins[c][rgba[c] as usize] += 1;
            }
        }
        bins
    }
    /// Area integration, exact rectangular pixel footprints; works on premultiplied fp32.
    /// Not the future qualified Lanczos3 reference pyramid.
    pub fn reduced(&self, max_edge: u32) -> Self {
        if max_edge == 0 || self.width.max(self.height) <= max_edge {
            return self.clone();
        }
        let scale = max_edge as f64 / self.width.max(self.height) as f64;
        let width = ((self.width as f64 * scale).round() as u32).max(1);
        let height = ((self.height as f64 * scale).round() as u32).max(1);
        let sx = self.width as f64 / width as f64;
        let sy = self.height as f64 / height as f64;
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            let (y0, y1) = (y as f64 * sy, (y + 1) as f64 * sy);
            for x in 0..width {
                let (x0, x1) = (x as f64 * sx, (x + 1) as f64 * sx);
                let mut out = [0.0f64; 4];
                for iy in y0.floor() as u32..(y1.ceil() as u32).min(self.height) {
                    let wy = (y1.min(iy as f64 + 1.0) - y0.max(iy as f64)).max(0.0);
                    for ix in x0.floor() as u32..(x1.ceil() as u32).min(self.width) {
                        let wx = (x1.min(ix as f64 + 1.0) - x0.max(ix as f64)).max(0.0);
                        let p = self.pixels[(iy * self.width + ix) as usize];
                        for c in 0..4 {
                            out[c] += p[c] as f64 * wx * wy;
                        }
                    }
                }
                pixels.push(out.map(|v| (v / (sx * sy)) as f32));
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_and_matrix_round_trip() {
        for i in 0..=4096 {
            let v = i as f32 / 4096.0;
            assert!((linear_to_srgb(srgb_to_linear(v)) - v).abs() < 2e-6);
            let rgb = [v, 1.0 - v, 0.25];
            let out = rec2020_to_linear_srgb(linear_srgb_to_rec2020(rgb));
            for c in 0..3 {
                assert!((rgb[c] - out[c]).abs() < 3e-6);
            }
        }
    }
    #[test]
    fn no_working_clamp() {
        let v = [-0.3, 1.8, 2.1];
        let out = rec2020_to_linear_srgb(linear_srgb_to_rec2020(v));
        for c in 0..3 {
            assert!((v[c] - out[c]).abs() < 5e-6);
        }
    }
    #[test]
    fn reduction_is_linear_not_encoded() {
        let image = LinearImage::new(
            2,
            1,
            vec![
                from_encoded_srgb([0., 0., 0., 1.]),
                from_encoded_srgb([1.; 4]),
            ],
        )
        .unwrap();
        assert_eq!(image.reduced(1).to_display(), [188, 188, 188, 255]);
    }
    #[test]
    fn invisible_rgb_does_not_leak_into_edges() {
        let img = LinearImage::new(
            2,
            1,
            vec![
                from_encoded_srgb([1., 0., 0., 0.]),
                from_encoded_srgb([0., 0., 1., 1.]),
            ],
        )
        .unwrap();
        let p = img.reduced(1).pixels[0];
        let rgb = rec2020_to_linear_srgb([p[0], p[1], p[2]]);
        assert!(rgb[0].abs() < 1e-6 && (rgb[2] - 0.5).abs() < 1e-6);
        assert_eq!(p[3], 0.5);
    }
    #[test]
    fn odd_edge_constant_and_identity() {
        let img = LinearImage::new(7, 5, vec![[0.1, 0.2, 0.3, 0.4]; 35]).unwrap();
        assert_eq!(img.reduced(0).pixels, img.pixels);
        for p in img.reduced(3).pixels {
            for (a, b) in p.into_iter().zip([0.1, 0.2, 0.3, 0.4]) {
                assert!((a - b).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn invalid_raster_rejected() {
        assert!(LinearImage::new(u32::MAX, u32::MAX, vec![]).is_err());
        assert!(LinearImage::new(1, 1, vec![[f32::NAN; 4]]).is_err());
        assert!(LinearImage::new(1, 1, vec![[0., 0., 0., 2.]]).is_err());
    }
}
