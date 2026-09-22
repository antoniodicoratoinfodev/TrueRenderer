//! Pipe-based R0 transport. Control <=64 KiB; separate, bounded pixel/source body.
//! Pipes yield private bytes; this is deliberately not the future shared-memory ABI.
use crate::color::{LinearImage, MAX_PIXELS};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

pub const MAX_CONTROL: usize = 64 * 1024;
pub const MAX_SOURCE: usize = 256 * 1024 * 1024;
pub const REQUEST: u16 = 1;
pub const RESPONSE: u16 = 2;
pub const ERROR: u16 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecodeIntent {
    #[default]
    LegacyRaster,
    Probe,
    FullSource,
    Export(crate::export::Options),
    ScientificSample {
        x: u32,
        y: u32,
    },
    /// Full development followed by the canonical reference mip graph inside
    /// the isolated decoder. This is not reduced RAW development.
    ReferenceMip {
        cpu_threads: usize,
    },
}
fn default_output_bytes() -> u64 {
    MAX_PIXELS as u64 * 16
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodeRequest {
    #[serde(default)]
    pub raw_engine: crate::decoder::RawEngine,
    pub source_len: usize,
    pub max_edge: u32,
    #[serde(default)]
    pub intent: DecodeIntent,
    #[serde(default = "default_output_bytes")]
    pub maximum_output_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scientific: Option<Box<crate::science::Metadata>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_mip: Option<ReferenceMip>,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub native_bits: u16,
    pub format: String,
    pub decoder: String,
    pub input_color: String,
    pub filter: String,
    pub orientation: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceMip {
    pub base: u32,
    /// Opacity of the full source, retained through every filter stage.
    pub opaque: bool,
}

pub fn mip_geometry(mut size: [u32; 2], maximum_edge: u32) -> ([u32; 2], u32) {
    let mut base = 0;
    while size[0].max(size[1]) > maximum_edge.max(1) {
        size = [size[0].div_ceil(2), size[1].div_ceil(2)];
        base += 1;
    }
    (size, base)
}
pub fn write_control<W: Write, T: Serialize>(
    writer: &mut W,
    kind: u16,
    id: u64,
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_CONTROL, "Controllo IPC troppo grande");
    writer.write_all(b"TRIP")?;
    writer.write_all(&1u16.to_le_bytes())?;
    writer.write_all(&kind.to_le_bytes())?;
    writer.write_all(&id.to_le_bytes())?;
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
pub fn read_control<R: Read>(reader: &mut R) -> Result<(u16, u64, Vec<u8>)> {
    let mut h = [0u8; 20];
    reader.read_exact(&mut h)?;
    ensure!(
        &h[..4] == b"TRIP" && u16::from_le_bytes([h[4], h[5]]) == 1,
        "Protocollo/versione IPC sconosciuti"
    );
    let kind = u16::from_le_bytes([h[6], h[7]]);
    ensure!(
        [REQUEST, RESPONSE, ERROR].contains(&kind),
        "Tipo IPC sconosciuto"
    );
    let id = u64::from_le_bytes(h[8..16].try_into()?);
    let len = u32::from_le_bytes(h[16..20].try_into()?) as usize;
    ensure!(len <= MAX_CONTROL, "Allocazione controllo IPC respinta");
    let mut data = vec![0; len];
    reader.read_exact(&mut data)?;
    Ok((kind, id, data))
}
pub fn parse<T: DeserializeOwned>(data: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(data)?)
}
pub fn validate_info(info: &RasterInfo) -> Result<usize> {
    if let Some(science) = &info.scientific {
        science.validate(info.source_width as u64 * info.source_height as u64)?;
        ensure!(
            info.format == "FITS" && info.filter == crate::science::FILTER,
            "Contrat scientifique incohérent"
        );
    }
    for (w, h) in [
        (info.width, info.height),
        (info.source_width, info.source_height),
    ] {
        ensure!(
            w > 0 && h > 0 && (w as u64 * h as u64) <= MAX_PIXELS as u64,
            "Dimensioni IPC fuori quota"
        );
    }
    ensure!(
        info.width <= info.source_width && info.height <= info.source_height,
        "Raster IPC ingrandito non richiesto"
    );
    if let Some(mip) = info.reference_mip {
        ensure!(mip.base < 32, "Livello IPC fuori quota");
        let mut size = [info.source_width, info.source_height];
        for _ in 0..mip.base {
            ensure!(size != [1, 1], "Livello IPC ridondante");
            size = [size[0].div_ceil(2), size[1].div_ceil(2)];
        }
        ensure!(
            size == [info.width, info.height]
                && (info.filter == crate::resample::VERSION
                    || (info.scientific.is_some() && info.filter == crate::science::FILTER)),
            "Grafo o geometria mip IPC incoerente"
        );
    }
    ensure!(
        info.format.len() <= 32
            && info.decoder.len() <= 128
            && info.input_color.len() <= 256
            && info.filter.len() <= 128
            && info.orientation.len() <= 128,
        "Metadati IPC fuori quota"
    );
    ensure!(
        [0, 1, 2, 4, 8, 10, 12, 14, 16, 32].contains(&info.native_bits),
        "Profondità non ammessa (0 = non dichiarata dal decoder)"
    );
    Ok(info.width as usize * info.height as usize)
}
pub fn read_raster<R: Read>(reader: &mut R, info: &RasterInfo) -> Result<LinearImage> {
    let count = validate_info(info)?;
    // This is an owned private buffer, never memory writable by the worker.
    // Read bounded blocks directly into aligned initialized pixels, then run
    // the same full finite/alpha validation in LinearImage::new before exposure.
    let mut pixels = vec![[0.; 4]; count];
    for block in pixels.chunks_mut(64 * 1024 / 16) {
        reader.read_exact(bytemuck::cast_slice_mut(block))?;
    }
    #[cfg(target_endian = "big")]
    for pixel in &mut pixels {
        for value in pixel {
            *value = f32::from_bits(value.to_bits().swap_bytes());
        }
    }
    LinearImage::new(info.width, info.height, pixels)
}
pub fn write_raster<W: Write>(writer: &mut W, raster: &LinearImage) -> Result<()> {
    #[cfg(target_endian = "little")]
    for block in raster.pixels.chunks(64 * 1024 / 16) {
        writer.write_all(bytemuck::cast_slice(block))?;
    }
    #[cfg(target_endian = "big")]
    {
        let mut row = Vec::with_capacity(raster.width as usize * 16);
        for pixels in raster.pixels.chunks(raster.width as usize) {
            row.clear();
            for pixel in pixels {
                for value in pixel {
                    row.extend_from_slice(&value.to_le_bytes());
                }
            }
            writer.write_all(&row)?;
        }
    }
    writer.flush()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn info(width: u32, height: u32) -> RasterInfo {
        RasterInfo {
            scientific: None,
            reference_mip: None,
            width,
            height,
            source_width: width,
            source_height: height,
            native_bits: 32,
            format: "test".into(),
            decoder: "test".into(),
            input_color: "Rec2020".into(),
            filter: "none".into(),
            orientation: "applied".into(),
        }
    }
    #[test]
    fn bulk_raster_preserves_wire_bytes_and_rejects_invalid_samples() {
        let source =
            LinearImage::new(2, 1, vec![[-0.25, 2., 0.125, 1.], [0., -0., 0.5, 0.25]]).unwrap();
        let expected: Vec<u8> = source
            .pixels
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut wire = vec![];
        write_raster(&mut wire, &source).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(
            read_raster(&mut &wire[..], &info(2, 1)).unwrap().pixels,
            source.pixels
        );
        assert!(read_raster(&mut &wire[..31], &info(2, 1)).is_err());
        for (index, value) in [(0, f32::NAN), (4, f32::INFINITY), (12, 1.01), (28, -0.1)] {
            let mut bad = wire.clone();
            bad[index..index + 4].copy_from_slice(&value.to_le_bytes());
            assert!(read_raster(&mut &bad[..], &info(2, 1)).is_err());
        }
    }
    #[test]
    fn reference_mip_metadata_requires_exact_geometry_and_filter() {
        let mut mip = info(513, 257);
        mip.width = 129;
        mip.height = 65;
        mip.reference_mip = Some(ReferenceMip {
            base: 2,
            opaque: true,
        });
        assert!(validate_info(&mip).is_err());
        mip.filter = crate::resample::VERSION.into();
        assert!(validate_info(&mip).is_ok());
        mip.width = 128;
        assert!(validate_info(&mip).is_err());
        mip.width = 129;
        mip.reference_mip.as_mut().unwrap().base = 32;
        assert!(validate_info(&mip).is_err());
    }
    #[test]
    fn engine_is_explicit_and_unknown_recipes_are_not_silently_defaulted() {
        let legacy: DecodeRequest =
            serde_json::from_str(r#"{"source_len":32,"max_edge":0}"#).unwrap();
        assert_eq!(legacy.raw_engine, Default::default());
        for engine in crate::decoder::RawEngine::choices() {
            let request = DecodeRequest {
                raw_engine: engine,
                ..legacy
            };
            let wire = serde_json::to_vec(&request).unwrap();
            assert_eq!(parse::<DecodeRequest>(&wire).unwrap().raw_engine, engine);
        }
        assert!(
            serde_json::from_str::<DecodeRequest>(
                r#"{"source_len":32,"max_edge":0,"raw_engine":"Unknown"}"#
            )
            .is_err()
        );
    }
    #[test]
    fn reject_oversize_before_allocating() {
        let mut wire = vec![];
        write_control(&mut wire, REQUEST, 7, &"ok").unwrap();
        wire[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_control(&mut &wire[..]).is_err());
    }
    #[test]
    fn framing_truncation_and_version() {
        let mut wire = vec![];
        write_control(
            &mut wire,
            REQUEST,
            42,
            &DecodeRequest {
                raw_engine: Default::default(),
                source_len: 32,
                max_edge: 320,
                intent: DecodeIntent::LegacyRaster,
                maximum_output_bytes: default_output_bytes(),
            },
        )
        .unwrap();
        let (_, id, bytes) = read_control(&mut &wire[..]).unwrap();
        assert_eq!(id, 42);
        assert_eq!(parse::<DecodeRequest>(&bytes).unwrap().source_len, 32);
        for len in 0..wire.len() {
            assert!(read_control(&mut &wire[..len]).is_err());
        }
        wire[4] = 2;
        assert!(read_control(&mut &wire[..]).is_err());
    }
}
