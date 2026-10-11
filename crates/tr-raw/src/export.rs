//! Shared uint16 sensor container writer; no decoder or format conversion fallback.
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
pub(crate) type Fields = BTreeMap<u16, (u16, u32, Vec<u8>)>;

pub(crate) fn sensor(
    mut fields: Fields,
    le: bool,
    width: usize,
    height: usize,
    limit: usize,
    unpack: impl FnOnce() -> Result<Vec<u16>>,
) -> Result<(u32, u32, Vec<u8>)> {
    let short = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let long = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let directory_end = 8 + 2 + fields.len() * 12 + 4;
    let payload_bytes: usize = fields
        .values()
        .filter(|(_, _, v)| v.len() > 4)
        .map(|(_, _, v)| (v.len() + 1) & !1)
        .sum();
    let sample_start = directory_end + payload_bytes;
    let total = sample_start + width * height * 2;
    ensure!(
        total <= limit.min(crate::MAX_SOURCE_BYTES),
        "DNG export oltre quota"
    );
    fields.insert(273, (4, 1, long(sample_start as u32).to_vec()));
    let samples = unpack()?;
    let mut output = Vec::with_capacity(total);
    output.extend(if le { b"II\x2a\0" } else { b"MM\0\x2a" });
    output.extend(long(8));
    output.extend(short(fields.len() as u16));
    let mut payload = Vec::with_capacity(payload_bytes);
    for (tag, (kind, count, bytes)) in fields {
        output.extend(short(tag));
        output.extend(short(kind));
        output.extend(long(count));
        if bytes.len() <= 4 {
            output.extend(&bytes);
            output.resize(output.len() + 4 - bytes.len(), 0);
        } else {
            output.extend(long((directory_end + payload.len()) as u32));
            payload.extend(bytes);
            if payload.len() % 2 != 0 {
                payload.push(0);
            }
        }
    }
    output.extend(long(0));
    output.extend(payload);
    for sample in samples {
        output.extend(short(sample));
    }
    Ok((width as u32, height as u32, output))
}
