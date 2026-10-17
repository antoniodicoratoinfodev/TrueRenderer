//! Autonomous, bounded RAW stages. No filesystem access, LibRaw, or native FFI.
//! The host must grant bytes inside its decoder sandbox before calling this API.
mod bayer_clipped_edges;
mod bayer_texture_edges;
#[cfg(test)]
#[path = "../research/brackets.rs"]
mod brackets_lab;
#[cfg(test)]
#[path = "../research/calibration.rs"]
mod calibration_lab;
mod cfa;
mod color;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../research/color_cal1.rs"]
mod color_cal1;
#[cfg(test)]
#[path = "../research/color_reference.rs"]
mod color_reference;
mod dng;
mod edge_planes;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../research/edge_planes_dir6.rs"]
mod edge_planes_dir6;
mod export;
mod highlight_render;
#[cfg(test)]
#[path = "../research/highlight_stability.rs"]
mod highlight_stability;
mod highlights;
#[cfg(test)]
#[path = "../research/highlights_hl1.rs"]
mod highlights_lab;
#[cfg(test)]
#[path = "../research/highlights.rs"]
mod highlights_research;
mod jpeg;
mod nef;
#[cfg(test)]
#[path = "../research/projection.rs"]
mod projection_lab;
mod raf;
mod render;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
mod tiff;
mod wb;
mod xtrans;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../research/xtrans_dir6.rs"]
mod xtrans_dir6;
mod xtrans_dr6;
mod xtrans_steps;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../research/xtrans_steps_dir6.rs"]
mod xtrans_steps_dir6;

use anyhow::Result;
use tr_core::color::LinearImage;

pub const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
pub const RECIPE: &str = tr_core::decoder::RawEngine::TrueRendererExperimental.recipe();

#[derive(Clone, Debug)]
pub struct RawInfo {
    pub width: u32,
    pub height: u32,
    pub camera: String,
    pub format: &'static str,
    pub profile_source: &'static str,
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

enum Raw<'a> {
    Dng(Box<dng::Dng<'a>>),
    Nef(Box<nef::Nef<'a>>),
    Raf(Box<raf::Raf<'a>>),
}
impl<'a> Raw<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.starts_with(b"FUJIFILMCCD-RAW ") {
            return Ok(Self::Raf(Box::new(raf::Raf::parse(bytes)?)));
        }
        let tiff = tiff::Tiff::parse(bytes)?;
        if tiff.ifds[0].get(50706).is_some() {
            Ok(Self::Dng(Box::new(dng::Dng::parse(bytes)?)))
        } else {
            Ok(Self::Nef(Box::new(nef::Nef::parse(tiff)?)))
        }
    }
    fn info(&self) -> Result<RawInfo> {
        match self {
            Self::Dng(raw) => raw.info(),
            Self::Nef(raw) => raw.info(),
            Self::Raf(raw) => raw.info(),
        }
    }
    fn validate_wb(&self, wb: [f64; 3]) -> Result<()> {
        match self {
            Self::Dng(raw) => raw.validate_wb(wb),
            Self::Nef(raw) => raw.validate_wb(wb),
            Self::Raf(raw) => raw.validate_wb(wb),
        }
    }
}

pub fn probe(bytes: &[u8]) -> Result<RawInfo> {
    Raw::parse(bytes)?.info()
}

pub fn probe_with_wb(bytes: &[u8], relative_wb: [f64; 3]) -> Result<RawInfo> {
    let raw = Raw::parse(bytes)?;
    raw.validate_wb(relative_wb)?;
    raw.info()
}

/// Neutral patch or robust grey-world estimate from calibrated CFA sites.
pub fn estimate_white_balance(
    bytes: &[u8],
    analysis: tr_core::raw_wb::Analysis,
) -> Result<tr_core::decoder::RawWhiteBalance> {
    let mosaic = match Raw::parse(bytes)? {
        Raw::Dng(raw) => raw.mosaic()?,
        Raw::Nef(raw) => raw.mosaic()?,
        Raw::Raf(raw) => raw.mosaic()?,
    };
    mosaic.estimate(analysis)
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

/// Relative camera gains R/G/B; unity selects AsShot. Locally supported clipped
/// CFA samples are estimated before WB. No look, tone curve or output clamp.
pub fn develop(bytes: &[u8], relative_wb: [f64; 3]) -> Result<LinearImage> {
    match Raw::parse(bytes)? {
        Raw::Dng(raw) => raw.develop(relative_wb),
        Raw::Nef(raw) => raw.develop(relative_wb),
        Raw::Raf(raw) => raw.develop(relative_wb),
    }
}

/// Unpack without applying the LUT/black/WB stages, for numerical diagnostics.
/// Values are in the stored code domain; dimensions include the sensor margins.
pub fn unpack(bytes: &[u8]) -> Result<(u32, u32, Vec<u16>)> {
    match Raw::parse(bytes)? {
        Raw::Dng(raw) => Ok((raw.width as u32, raw.height as u32, raw.unpack()?)),
        Raw::Nef(raw) => Ok((raw.width as u32, raw.height as u32, raw.unpack()?)),
        Raw::Raf(raw) => Ok((raw.width as u32, raw.height as u32, raw.unpack()?)),
    }
}

/// Repack without applying WB, black subtraction, LUT or demosaic. Retains all
/// supported calibration, margins and crop. Ancillary metadata is omitted.
pub fn repack_dng(bytes: &[u8], limit: usize) -> Result<(u32, u32, Vec<u8>)> {
    match Raw::parse(bytes)? {
        Raw::Dng(raw) => raw.repack(limit),
        Raw::Nef(raw) => raw.repack(limit),
        Raw::Raf(raw) => raw.repack(limit),
    }
}
