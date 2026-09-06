//! Pipe-based R0 transport. Control <=64 KiB; separate, bounded pixel/source body.
//! Pipes yield private bytes; this is deliberately not the future shared-memory ABI.
use crate::color::{LinearImage, MAX_PIXELS};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

pub const MAX_CONTROL: usize = 64 * 1024;
pub const MAX_SOURCE: usize = 256 * 1024 * 1024;
pub const REQUEST: u16 = 1;
pub const RESPONSE: u16 = 2;
pub const ERROR: u16 = 3;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodeRequest {
    pub source_len: usize,
    pub max_edge: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterInfo {
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
    let mut pixels = Vec::with_capacity(count);
    let mut row = vec![0; info.width as usize * 16];
    for _ in 0..info.height {
        reader.read_exact(&mut row)?;
        for chunk in row.as_chunks::<16>().0 {
            let mut p = [0.; 4];
            for c in 0..4 {
                p[c] = f32::from_le_bytes(chunk[c * 4..c * 4 + 4].try_into()?);
            }
            if p.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&p[3]) {
                bail!("Campione IPC non valido");
            }
            pixels.push(p);
        }
    }
    LinearImage::new(info.width, info.height, pixels)
}
pub fn write_raster<W: Write>(writer: &mut W, raster: &LinearImage) -> Result<()> {
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
    writer.flush()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
                source_len: 32,
                max_edge: 320,
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
