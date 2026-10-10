mod support;
use support::Fixture;

#[test]
fn endian_and_packed_code_domains_are_exact() {
    for le in [true, false] {
        for bits in [8, 10, 12, 14, 16] {
            let mut f = Fixture::new(le);
            f.ints(258, 3, &[bits]);
            f.pixels = (0..120)
                .map(|i| ((i * 1979 + 23) % (1 << bits)) as u16)
                .collect();
            let (w, h, samples) = tr_raw::unpack(&f.bytes()).unwrap();
            assert_eq!((w, h), (12, 10));
            assert_eq!(samples, f.pixels);
        }
    }
}

#[test]
fn active_origin_black_deltas_and_post_demosaic_crop() {
    let mut f = Fixture::new(true);
    // Odd active origin; CFAPattern and black repeat both start at ActiveArea.
    f.ints(50829, 4, &[1, 1, 9, 11]);
    f.ints(50713, 3, &[2, 2]);
    f.ints(50714, 3, &[10, 20, 30, 40]);
    f.rational(50715, true, &[0., 1., 2., 3., 4., 5., 6., 7., 8., 9.]);
    f.rational(50716, true, &[0., 2., 4., 6., 8., 10., 12., 14.]);
    f.ints(50719, 3, &[1, 1]);
    f.ints(50720, 3, &[7, 5]);
    let black = [10, 20, 30, 40];
    let neutral: [f64; 3] = [0.9504559, 1., 1.0890578];
    let cfa = [0, 1, 1, 2];
    // Maximum black = 40+9+14 = 63; same normalization for every CFA phase.
    for y in 0..8 {
        for x in 0..10 {
            f.pixels[(y + 1) * 12 + x + 1] = (black[(y % 2) * 2 + x % 2] + x + 2 * y) as u16
                + (0.25 * 961. * neutral[cfa[(y % 2) * 2 + x % 2]]).round() as u16;
        }
    }
    let image = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    assert_eq!((image.width, image.height), (7, 5));
    for p in image.pixels {
        for v in &p[..3] {
            assert!((v - 0.25).abs() < 0.001, "{p:?}");
        }
    }
}

#[test]
fn lut_once_extended_values_and_profile_identity() {
    let mut f = Fixture::new(true);
    f.ints(50714, 3, &[64]);
    f.ints(50712, 3, &[0, 400, 800, 1200]);
    let before = tr_raw::probe(&f.bytes()).unwrap();
    f.pixels.fill(100); // LUT out-of-range maps to its final entry.
    let image = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    assert!(image.pixels.iter().any(|p| p[..3].iter().any(|v| *v > 1.)));
    f.pixels.fill(0);
    let image = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    assert!(image.pixels.iter().any(|p| p[..3].iter().any(|v| *v < 0.)));
    f.rational(50721, true, &[1.1, 0., 0., 0., 1., 0., 0., 0., 1.]);
    assert_ne!(
        before.profile_hash,
        tr_raw::probe(&f.bytes()).unwrap().profile_hash
    );
}

#[test]
fn eight_orientations_are_exact_permutations_after_crop() {
    let mut f = Fixture::new(true);
    f.ints(50719, 3, &[1, 2]);
    f.ints(50720, 3, &[7, 5]);
    f.pixels = (0..120).map(|i| (i * 31 % 800) as u16).collect();
    let base = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    for o in 1..=8 {
        f.ints(274, 3, &[o]);
        let result = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
        assert_eq!(
            (result.width, result.height),
            if o >= 5 { (5, 7) } else { (7, 5) }
        );
        for y in 0..5 {
            for x in 0..7 {
                let (ox, oy) = match o {
                    1 => (x, y),
                    2 => (6 - x, y),
                    3 => (6 - x, 4 - y),
                    4 => (x, 4 - y),
                    5 => (y, x),
                    6 => (4 - y, x),
                    7 => (4 - y, 6 - x),
                    8 => (y, 6 - x),
                    _ => unreachable!(),
                };
                assert_eq!(
                    result.pixels[oy * result.width as usize + ox],
                    base.pixels[y * 7 + x]
                );
            }
        }
    }
}

#[test]
fn unsupported_calibration_and_malformed_metadata_fail_closed() {
    for tag in [50937, 51009, 50831, 52525, 52531, 52543, 52544] {
        let mut f = Fixture::new(true);
        f.ints(tag, 7, &[1]);
        assert!(tr_raw::probe(&f.bytes()).is_err(), "tag {tag}");
    }
    for mutation in 0..8 {
        let mut f = Fixture::new(true);
        match mutation {
            0 => f.ints(50717, 4, &[0]),
            1 => f.ints(33421, 3, &[6, 6]),
            2 => f.rational(50721, true, &[0.; 9]),
            3 => f.rational(50728, false, &[0., 1., 1.]),
            4 => f.ints(50707, 1, &[2, 0, 0, 0]),
            5 => f.ints(50829, 4, &[3, 3, 2, 2]),
            6 => f.ints(256, 4, &[u32::MAX]),
            _ => f.ints(50879, 3, &[1]),
        }
        assert!(tr_raw::probe(&f.bytes()).is_err(), "mutation {mutation}");
    }
    let mut f = Fixture::new(true);
    let bytes = f.bytes();
    for end in 0..bytes.len() {
        assert!(
            tr_raw::develop(&bytes[..end], [1.; 3]).is_err(),
            "truncation {end}"
        );
    }
}

#[test]
fn memory_plan_bounds_peak_for_sensor_margins_and_crop() {
    let mut f = Fixture::new(true);
    f.ints(50720, 3, &[1, 1]);
    let info = tr_raw::probe(&f.bytes()).unwrap();
    assert_eq!(info.memory.sensor_pixels, 120);
    assert!(info.memory.scratch_peak_bytes >= 8 * 120 + 16);
}

#[test]
fn raw_export_preserves_full_sensor_and_color_without_baking_wb() {
    for le in [true, false] {
        let mut f = Fixture::new(le);
        f.ints(258, 3, &[12]);
        f.ints(50829, 4, &[1, 1, 9, 11]);
        f.ints(50713, 3, &[2, 2]);
        f.rational(50714, false, &[10.5, 20.25, 30., 40.]);
        f.rational(50715, true, &[-2., 0., 0., 0., 0., 0., 0., 0., 0., 4.]);
        f.ints(50719, 3, &[1, 1]);
        f.ints(50720, 3, &[7, 5]);
        f.ints(274, 3, &[7]);
        f.rational(50722, true, &[1.1, 0., 0., 0., 1., 0., 0., 0., 0.9]);
        f.ints(50779, 3, &[17]);
        f.text(50936, "Fixture matrix profile");
        f.text(50942, "Copyright TrueRenderer fixture author");
        f.ints(50941, 4, &[1]);
        f.pixels = (0..120).map(|i| (i * 37 % 4096) as u16).collect();
        let bytes = f.bytes();
        let (_, _, copy) = tr_raw::repack_dng(&bytes, 1 << 20).unwrap();
        let copyright = b"Copyright TrueRenderer fixture author\0";
        assert!(copy.windows(copyright.len()).any(|w| w == copyright));
        assert_eq!(tr_raw::unpack(&copy).unwrap().2, f.pixels);
        assert_eq!(
            tr_raw::probe(&copy).unwrap().profile_hash,
            tr_raw::probe(&bytes).unwrap().profile_hash
        );
        for wb in [[1.; 3], [1.1, 1., 0.9]] {
            assert_eq!(
                tr_raw::develop(&bytes, wb).unwrap().pixels,
                tr_raw::develop(&copy, wb).unwrap().pixels
            );
        }
        assert!(tr_raw::repack_dng(&bytes, copy.len() - 1).is_err());
    }
}

#[test]
fn calibration_signatures_are_exact_utf8_in_ascii_or_byte_fields() {
    let mut f = Fixture::new(true);
    let expected = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    f.rational(50723, true, &[1.2, 0.1, 0., 0., 1., 0., 0., 0., 0.8]);
    f.text(50931, "profile\n");
    f.text(50932, "profile");
    // Removing control characters before matching would incorrectly apply CC.
    let mismatched = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    assert_eq!(mismatched.pixels, expected.pixels);
    f.text(50931, "profile");
    f.tags.get_mut(&50931).unwrap().0 = 1;
    let matched = tr_raw::develop(&f.bytes(), [1.; 3]).unwrap();
    assert_ne!(matched.pixels, expected.pixels);
    f.tags.get_mut(&50931).unwrap().2[0] = 0xff;
    assert!(tr_raw::probe(&f.bytes()).is_err());
}

#[test]
fn bounded_ifd_cycles_duplicates_and_mutations_never_panic() {
    let mut f = Fixture::new(true);
    let bytes = f.bytes();
    let mut cyclic = bytes.clone();
    let count = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    cyclic[10 + 12 * count..14 + 12 * count].copy_from_slice(&8u32.to_le_bytes());
    assert!(tr_raw::probe(&cyclic).is_err());
    let mut duplicate = bytes.clone();
    duplicate[22..24].copy_from_slice(&bytes[10..12]);
    assert!(tr_raw::probe(&duplicate).is_err());
    let mut state = 0x13579bdfu32;
    for _ in 0..2048 {
        let mut damaged = bytes.clone();
        for _ in 0..3 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let i = state as usize % damaged.len();
            damaged[i] ^= (state >> 24) as u8;
        }
        assert!(std::panic::catch_unwind(|| tr_raw::probe(&damaged)).is_ok());
    }
}

#[test]
fn tiled_storage_and_subifd_primary_selection_preserve_samples() {
    let mut f = Fixture::new(true);
    f.pixels = (0..120).map(|i| (i * 83) as u16).collect();
    let mut bytes = f.bytes();
    let count = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    let mut entries: Vec<[u8; 12]> = bytes[10..10 + 12 * count].as_chunks::<12>().0.to_vec();
    let mut positions = vec![];
    for ty in 0..2 {
        for tx in 0..2 {
            positions.push(bytes.len() as u32);
            for y in 0..5 {
                for x in 0..6 {
                    bytes.extend(f.pixels[(ty * 5 + y) * 12 + tx * 6 + x].to_le_bytes());
                }
            }
        }
    }
    let offsets = bytes.len() as u32;
    for p in positions {
        bytes.extend(p.to_le_bytes());
    }
    let lengths = bytes.len() as u32;
    for _ in 0..4 {
        bytes.extend(60u32.to_le_bytes());
    }
    for e in &mut entries {
        match u16::from_le_bytes(e[..2].try_into().unwrap()) {
            273 | 279 => {
                let offset = u16::from_le_bytes(e[..2].try_into().unwrap()) == 273;
                e[..2].copy_from_slice(&if offset { 324u16 } else { 325u16 }.to_le_bytes());
                e[4..8].copy_from_slice(&4u32.to_le_bytes());
                e[8..].copy_from_slice(&if offset { offsets } else { lengths }.to_le_bytes());
            }
            _ => {}
        }
    }
    for (tag, size) in [(322u16, 6u32), (323, 5)] {
        let mut e = [0; 12];
        e[..2].copy_from_slice(&tag.to_le_bytes());
        e[2..4].copy_from_slice(&4u16.to_le_bytes());
        e[4..8].copy_from_slice(&1u32.to_le_bytes());
        e[8..].copy_from_slice(&size.to_le_bytes());
        entries.push(e);
    }
    entries.sort_by_key(|e| u16::from_le_bytes(e[..2].try_into().unwrap()));
    let raw = bytes.len() as u32;
    bytes.extend((entries.len() as u16).to_le_bytes());
    for e in entries {
        bytes.extend(e);
    }
    bytes.extend(0u32.to_le_bytes());
    // Root remains the profile/preview IFD. Its SubIFD selects the actual tile
    // mosaic, and must win over the root's obsolete strip bytes.
    let mut root_entries: Vec<[u8; 12]> = bytes[10..10 + 12 * count].as_chunks::<12>().0.to_vec();
    for e in &mut root_entries {
        if u16::from_le_bytes(e[..2].try_into().unwrap()) == 262 {
            e[8..10].copy_from_slice(&2u16.to_le_bytes());
        }
    }
    let mut sub = [0; 12];
    sub[..2].copy_from_slice(&330u16.to_le_bytes());
    sub[2..4].copy_from_slice(&4u16.to_le_bytes());
    sub[4..8].copy_from_slice(&1u32.to_le_bytes());
    sub[8..].copy_from_slice(&raw.to_le_bytes());
    root_entries.push(sub);
    let root = bytes.len() as u32;
    bytes.extend((root_entries.len() as u16).to_le_bytes());
    for e in root_entries {
        bytes.extend(e);
    }
    bytes.extend(0u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&root.to_le_bytes());
    assert_eq!(tr_raw::unpack(&bytes).unwrap().2, f.pixels);
    let (_, _, copy) = tr_raw::repack_dng(&bytes, 1 << 20).unwrap();
    assert_eq!(
        tr_raw::develop(&copy, [1.; 3]).unwrap().pixels,
        tr_raw::develop(&bytes, [1.; 3]).unwrap().pixels
    );
}
