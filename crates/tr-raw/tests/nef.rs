#[allow(dead_code)]
mod support;
use support::nef::NefFixture;

#[test]
fn direct_nef_samples_wb_geometry_and_export_are_consistent() {
    let mut f = NefFixture::new();
    f.root.ints(274, 3, &[6]);
    f.maker.ints(0x3d, 3, &[599, 601, 602, 603]);
    let bytes = f.bytes();
    assert!(!tr_raw::is_bitmap_tiff(&bytes).unwrap());
    let info = tr_raw::probe(&bytes).unwrap();
    assert_eq!((info.width, info.height, info.format), (4016, 6016, "NEF"));
    assert!(info.memory.scratch_peak_bytes <= info.memory.output_pixels * 48 + 128 * 1024 * 1024);
    let (w, h, samples) = tr_raw::unpack(&bytes).unwrap();
    assert_eq!((w, h), (6032, 4032));
    assert!(samples.iter().all(|v| *v == 1000));
    assert!(tr_raw::repack_dng(&bytes, 1024).is_err());
    let (_, _, dng) = tr_raw::repack_dng(&bytes, 128 * 1024 * 1024).unwrap();
    assert_eq!(tr_raw::unpack(&dng).unwrap().2, samples);
    drop(samples);
    // Changing WB must affect the render; NEF->DNG must preserve either WB.
    let mut hashes = Vec::new();
    for wb in [[1.; 3], [1.15, 1., 0.9]] {
        let original = tr_raw::develop(&bytes, wb).unwrap();
        let reopened = tr_raw::develop(&dng, wb).unwrap();
        assert_eq!(original.pixels, reopened.pixels);
        assert_eq!((original.width, original.height), (4016, 6016));
        hashes.push(original.pixels[0]);
    }
    assert_ne!(hashes[0], hashes[1]);
}

#[test]
fn nef_modes_metadata_and_nested_offsets_fail_closed() {
    let mut f = NefFixture::new();
    let bytes = f.bytes();
    // The final byte is TIFF alignment padding, not referenced file data.
    assert!(tr_raw::probe(&bytes[..bytes.len() - 1]).is_ok());
    for end in [0, 4, 7, 12, 100, bytes.len() - 2] {
        assert!(tr_raw::probe(&bytes[..end]).is_err());
    }
    // Maker directory relative offset, including cycles/overflow/truncation.
    let maker = bytes
        .windows(10)
        .position(|v| v == b"Nikon\0\x02\x11\0\0")
        .unwrap();
    for offset in [0u32, 4, u32::MAX] {
        let mut corrupt = bytes.clone();
        corrupt[maker + 14..maker + 18].copy_from_slice(&offset.to_le_bytes());
        assert!(tr_raw::probe(&corrupt).is_err());
    }
    for (tag, kind, values) in [
        (259, 3, vec![1]),
        (258, 3, vec![12]),
        (256, 4, vec![6024]),
        (33422, 1, vec![1, 0, 2, 1]),
    ] {
        let mut f = NefFixture::new();
        f.raw.ints(tag, kind, &values);
        assert!(tr_raw::probe(&f.bytes()).is_err());
    }
    let mut f = NefFixture::new();
    f.root.text(272, "NIKON D800");
    assert!(tr_raw::probe(&f.bytes()).is_err());
    for (tag, kind, values) in [
        (0x93, 3, vec![1]),
        (0x3d, 3, vec![15520; 4]),
        (1, 7, vec![48, 50, 49, 50]),
    ] {
        let mut f = NefFixture::new();
        f.maker.ints(tag, kind, &values);
        assert!(tr_raw::probe(&f.bytes()).is_err());
    }
    for (tag, values) in [
        (0x0c, vec![0., 1., 1., 1.]),
        (0x0c, vec![2., 1.5, 1.1, 1.]),
        (0x3b, vec![2.; 4]),
    ] {
        let mut f = NefFixture::new();
        f.maker.rational(tag, false, &values);
        assert!(tr_raw::probe(&f.bytes()).is_err());
    }
}

#[test]
fn d800_lossless_both_precisions_and_endians() {
    for bits in [12, 14] {
        for le in [false, true] {
            let mut f = NefFixture::d800(bits, le);
            let bytes = f.bytes();
            let info = tr_raw::probe(&bytes).unwrap();
            assert_eq!((info.width, info.height), (7360, 4912));
            let (w, h, samples) = tr_raw::unpack(&bytes).unwrap();
            assert_eq!((w, h), (7424, 4924));
            assert!(samples.iter().all(|v| *v == 1000));
            drop(samples);
            let (_, _, copy) = tr_raw::repack_dng(&bytes, 128 * 1024 * 1024).unwrap();
            assert!(tr_raw::unpack(&copy).unwrap().2.iter().all(|v| *v == 1000));
        }
    }
}
