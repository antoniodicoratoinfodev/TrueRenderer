//! Generated metadata-only X-T5 container: tiny, deliberately truncated sample
//! streams. Useful for parser boundaries, never a photographic golden.
use super::{Fixture, nef::directory};

pub fn bytes() -> Vec<u8> {
    let empty = || {
        let mut f = Fixture::new(true);
        f.tags.clear();
        f
    };
    let mut exif = empty();
    exif.text(272, "X-T5");
    exif.ints(274, 3, &[8]);
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    directory(&mut tiff, &exif);
    let mut jpeg = vec![255, 216, 255, 225];
    jpeg.extend(((tiff.len() + 8) as u16).to_be_bytes());
    jpeg.extend(b"Exif\0\0");
    jpeg.extend(tiff);
    jpeg.extend([255, 218, 0, 2]);
    let mut records = vec![];
    for (tag, values) in [
        (0x100, [5196, 7872]),
        (0x110, [21, 12]),
        (0x111, [5152, 7728]),
        (0x117, [0, 0]),
        (0x118, [0, 0]),
        (0x119, [5152, 7728]),
    ] {
        records.push((
            tag,
            values
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        ));
    }
    let pattern = [
        1u8, 1, 0, 1, 1, 2, 1, 1, 2, 1, 1, 0, 2, 0, 1, 0, 2, 1, 1, 1, 2, 1, 1, 0, 1, 1, 0, 1, 1, 2,
        0, 2, 1, 2, 0, 1,
    ];
    records.push((0x131, pattern.into_iter().rev().collect()));
    let mut meta = (records.len() as u32).to_be_bytes().to_vec();
    for (tag, bytes) in records {
        meta.extend((tag as u16).to_be_bytes());
        meta.extend((bytes.len() as u16).to_be_bytes());
        meta.extend(bytes);
    }
    let mut codec = vec![b'I', b'S', 1, 16, 14];
    for v in [5196u16, 8448, 7872, 768] {
        codec.extend(v.to_be_bytes());
    }
    codec.push(11);
    codec.extend(866u16.to_be_bytes());
    for _ in 0..11 {
        codec.extend(1u32.to_be_bytes());
    }
    codec.resize(64, 0);
    codec.extend([255; 11]);
    let mut raw = empty();
    for (tag, v) in [
        (0xf001, 7872),
        (0xf002, 5196),
        (0xf003, 14),
        (0xf004, 0),
        (0xf005, 0),
        (0xf006, 0),
        (0xf007, 0),
        (0xf008, codec.len() as u32),
    ] {
        raw.ints(tag, 4, &[v]);
    }
    raw.ints(0xf00a, 4, &[1021; 36]);
    raw.ints(0xf00e, 4, &[302, 509, 763]);
    let mut sensor = b"II*\0\0\0\0\0".to_vec();
    let raw_at = directory(&mut sensor, &raw);
    let mut root = empty();
    root.ints(0xf000, 4, &[raw_at]);
    let root_at = directory(&mut sensor, &root);
    sensor[4..8].copy_from_slice(&root_at.to_le_bytes());
    let offset = sensor.len() as u32;
    let entry = raw.tags.keys().position(|t| *t == 0xf007).unwrap();
    sensor[raw_at as usize + 2 + entry * 12 + 8..raw_at as usize + 2 + entry * 12 + 12]
        .copy_from_slice(&offset.to_le_bytes());
    sensor.extend(codec);
    let mut out = vec![0; 148];
    out[..20].copy_from_slice(b"FUJIFILMCCD-RAW 0201");
    out[28..33].copy_from_slice(b"X-T5\0");
    out[60..64].copy_from_slice(b"0103");
    out[108..112].copy_from_slice(&2u32.to_be_bytes());
    for (at, data) in [(84, jpeg), (92, meta), (100, sensor)] {
        let offset = out.len() as u32;
        out[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        out[at + 4..at + 8].copy_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend(data);
    }
    out
}
