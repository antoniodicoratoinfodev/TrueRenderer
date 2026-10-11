#[allow(dead_code)]
mod support;
use support::Fixture;
use tr_core::raw_wb::Analysis;

const XT: [u32; 36] = [
    1, 1, 0, 1, 1, 2, 1, 1, 2, 1, 1, 0, 2, 0, 1, 0, 2, 1, 1, 1, 2, 1, 1, 0, 1, 1, 0, 1, 1, 2, 0, 2,
    1, 2, 0, 1,
];
fn scene(xtrans: bool, orientation: u32) -> Fixture {
    let mut f = Fixture::new(true);
    f.width = 120;
    f.height = 96;
    f.ints(256, 4, &[120]);
    f.ints(257, 4, &[96]);
    f.ints(278, 4, &[96]);
    f.ints(274, 3, &[orientation]);
    f.rational(50728, false, &[0.5, 1., 0.75]);
    let (period, pattern) = if xtrans {
        (6, XT.to_vec())
    } else {
        (2, vec![0, 1, 1, 2])
    };
    f.ints(33421, 3, &[period, period]);
    f.ints(33422, 1, &pattern);
    f.pixels = (0..f.width * f.height)
        .map(|i| {
            [200, 600, 300][pattern
                [(i / f.width % period as usize) * period as usize + i % f.width % period as usize]
                as usize]
        })
        .collect();
    f
}
#[test]
fn cfa_white_balance_auto_patch_orientations_and_dng_roundtrip() {
    for xtrans in [false, true] {
        for orientation in 1..=8 {
            let mut f = scene(xtrans, orientation);
            let b = f.bytes();
            let auto = tr_raw::estimate_white_balance(&b, Analysis::Auto).unwrap();
            assert_eq!((auto.red, auto.blue), (1500, 1500));
            for side in [5, 11] {
                let patch =
                    tr_raw::estimate_white_balance(&b, Analysis::Patch { x: 43, y: 39, side })
                        .unwrap();
                assert_eq!(patch, auto);
            }
            let developed = tr_raw::develop(&b, auto.gains().map(f64::from)).unwrap();
            let p = developed.pixels[43 * developed.width as usize + 39];
            assert!(
                (p[0] - p[1]).abs() < 1e-5 && (p[2] - p[1]).abs() < 1e-5,
                "{p:?}"
            );
            let (_, _, copy) = tr_raw::repack_dng(&b, 1 << 20).unwrap();
            assert_eq!(
                tr_raw::estimate_white_balance(&copy, Analysis::Auto).unwrap(),
                auto
            );
            assert_eq!(
                tr_raw::develop(&copy, auto.gains().map(f64::from))
                    .unwrap()
                    .pixels,
                developed.pixels
            );
        }
    }
}
#[test]
fn robust_sensor_statistics_ignore_sparse_outliers_and_reject_invalid_regions() {
    let mut f = scene(true, 1);
    for i in (0..f.pixels.len()).step_by(97) {
        f.pixels[i] = 1024;
    }
    for i in (31..f.pixels.len()).step_by(101) {
        f.pixels[i] = 0;
    }
    let wb = tr_raw::estimate_white_balance(&f.bytes(), Analysis::Auto).unwrap();
    assert_eq!((wb.red, wb.blue), (1500, 1500));
    assert!(
        tr_raw::estimate_white_balance(
            &f.bytes(),
            Analysis::Patch {
                x: u32::MAX,
                y: 0,
                side: 5
            }
        )
        .is_err()
    );
    assert!(
        tr_raw::estimate_white_balance(
            &f.bytes(),
            Analysis::Patch {
                x: 50,
                y: 40,
                side: 7
            }
        )
        .is_err()
    );
    for value in [0, 1024, 65535] {
        f.pixels.fill(value);
        for analysis in [
            Analysis::Auto,
            Analysis::Patch {
                x: 40,
                y: 40,
                side: 11,
            },
        ] {
            assert!(tr_raw::estimate_white_balance(&f.bytes(), analysis).is_err());
        }
    }
}
