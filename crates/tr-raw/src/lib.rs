//! Autonomous, bounded RAW stages. No filesystem access, LibRaw, or native FFI.
//! The host must grant bytes inside its decoder sandbox before calling this API.
mod color;
mod dng;
mod jpeg;
mod render;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
mod tiff;

use anyhow::Result;
use tr_core::color::LinearImage;

pub const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
pub const RECIPE: &str = "TRExp-dng1-lj1-cal1-dir1-extended1";

#[derive(Clone, Debug)]
pub struct RawInfo {
    pub width: u32,
    pub height: u32,
    pub camera: String,
    pub profile_hash: String,
    pub orientation: u32,
    pub compression: u32,
    pub memory: RawMemoryPlan,
}

/// Upper bound for live image buffers in this implementation, excluding source
/// bytes owned by the caller and copies/transfers owned by the broker.
#[derive(Clone, Copy, Debug)]
pub struct RawMemoryPlan {
    pub sensor_pixels: usize,
    pub output_pixels: usize,
    pub scratch_peak_bytes: usize,
}

pub fn probe(bytes: &[u8]) -> Result<RawInfo> {
    dng::Dng::parse(bytes)?.info()
}

pub fn probe_with_wb(bytes: &[u8], relative_wb: [f64; 3]) -> Result<RawInfo> {
    let dng = dng::Dng::parse(bytes)?;
    dng.validate_wb(relative_wb)?;
    dng.info()
}

/// Positive bitmap classification; a malformed/unknown TIFF is never consent
/// to develop a RAW with a different engine. The platform also checks its type.
pub fn is_bitmap_tiff(bytes: &[u8]) -> Result<bool> {
    let tiff = tiff::Tiff::parse(bytes)?;
    for ifd in &tiff.ifds {
        if ifd.get(50706).is_some()
            || ifd.get(33422).is_some()
            || matches!(ifd.integer(262, 0)?, 32803 | 34892)
        {
            return Ok(false);
        }
    }
    let root = &tiff.ifds[0];
    let photometric = root.integer(262, u32::MAX)?;
    let samples = root.integer(277, 1)?;
    Ok(root.integer(254, 0)? == 0 && matches!((photometric, samples), (0 | 1, 1 | 2) | (2, 3 | 4)))
}

/// Relative camera gains R/G/B; unity selects AsShot. No look, tone curve,
/// highlight synthesis or clipping is applied to scene-linear samples.
pub fn develop(bytes: &[u8], relative_wb: [f64; 3]) -> Result<LinearImage> {
    dng::Dng::parse(bytes)?.develop(relative_wb)
}

/// Unpack without applying the LUT/black/WB stages, for numerical diagnostics.
/// Values are in the stored code domain; dimensions include the sensor margins.
pub fn unpack(bytes: &[u8]) -> Result<(u32, u32, Vec<u16>)> {
    let dng = dng::Dng::parse(bytes)?;
    Ok((dng.width as u32, dng.height as u32, dng.unpack()?))
}

/// Repack without applying WB, black subtraction, LUT or demosaic. Retains all
/// supported calibration, margins and crop. Ancillary metadata is omitted.
pub fn repack_dng(bytes: &[u8], limit: usize) -> Result<(u32, u32, Vec<u8>)> {
    dng::Dng::parse(bytes)?.repack(limit)
}
