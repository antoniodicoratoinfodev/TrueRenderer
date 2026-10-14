//! Owned DNGPrivateData record for NEF's separate normalization and code ceiling.
//! Other readers may ignore it; it is not a measured sensor saturation level.
use crate::tiff::Field;
use anyhow::{Result, ensure};

const OWNER: &[u8] = b"TrueRenderer\0";
const PREFIX: &[u8] = b"TrueRenderer\0CLP1";

pub(crate) fn encode(width: usize, height: usize, white: u32, clipping: u32) -> Vec<u8> {
    let mut data = PREFIX.to_vec();
    for v in [width as u32, height as u32, white, clipping] {
        data.extend(v.to_le_bytes());
    }
    data
}

pub(crate) fn decode(
    field: Option<Field<'_>>,
    width: usize,
    height: usize,
    white: f64,
    has_lut: bool,
) -> Result<Option<f64>> {
    let Some(f) = field else {
        return Ok(None);
    };
    if !f.data.starts_with(OWNER) {
        return Ok(None);
    }
    ensure!(
        f.kind == 1
            && f.count == PREFIX.len() + 16
            && f.data.len() == f.count
            && f.data.starts_with(PREFIX),
        "DNG: record TrueRenderer clipping sconosciuto/invalido"
    );
    let v: Vec<_> = f.data[PREFIX.len()..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    ensure!(
        v[0] as usize == width
            && v[1] as usize == height
            && f64::from(v[2]) == white
            && !has_lut
            && v[3] > v[2]
            && v[3] <= 65535,
        "DNG: record TrueRenderer clipping non coerente con il dominio dei campioni"
    );
    Ok(Some(f64::from(v[3])))
}
