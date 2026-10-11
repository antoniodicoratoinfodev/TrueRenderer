#[allow(dead_code)]
mod support;

#[test]
fn raf_nested_metadata_truncation_and_twenty_thousand_mutations_are_bounded() {
    let bytes = support::raf::bytes();
    let info = tr_raw::probe(&bytes).unwrap();
    assert_eq!(
        (info.width, info.height, info.format),
        (5152, 7728, "RAF lossless")
    );
    assert!(tr_raw::repack_dng(&bytes, 1024).is_err());
    assert!(tr_raw::unpack(&bytes).is_err());
    for end in 0..bytes.len() {
        assert!(tr_raw::probe(&bytes[..end]).is_err());
    }
    let mut random = 0x652ac79du32;
    for _ in 0..20_000 {
        let mut changed = bytes.clone();
        for _ in 0..4 {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            changed[random as usize % bytes.len()] ^= (random >> 24) as u8;
        }
        let _ = tr_raw::probe(&changed);
    }
    for offset in [u32::MAX, 0, 12, 148] {
        let mut changed = bytes.clone();
        changed[100..104].copy_from_slice(&offset.to_be_bytes());
        assert!(tr_raw::probe(&changed).is_err());
    }
}
