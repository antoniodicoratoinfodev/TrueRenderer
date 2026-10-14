use super::*;
use crate::{cfa::XTRANS, color::ID};

pub(super) fn mosaic(cfa: Cfa, width: usize, height: usize) -> Mosaic {
    Mosaic {
        values: vec![0.; width * height],
        saturation: vec![0; width * height],
        width,
        active_width: width,
        height,
        cfa,
        crop: [0, 0, width, height],
        orientation: 1,
        calibration: Calibration {
            cm: [ID; 2],
            cc: [ID; 2],
            fm: [None; 2],
            temperatures: [6500.; 2],
            analog: [1.; 3],
            neutral: [0.5, 1., 0.75],
        },
    }
}

#[test]
fn sparse_valid_outlier_does_not_set_patch_colour() {
    for cfa in [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS).unwrap()] {
        let size = if cfa.period == 2 { 12 } else { 11 };
        let mut raw = mosaic(cfa, size, size);
        fill_plane(&mut raw, [0.2, -0.3]);
        raw.values[5 * size + 5] = 0.9;
        let result = raw.region([0, 0, size, size]).unwrap();
        for (v, target) in result.into_iter().zip([0.5, 0.75]) {
            assert!((v.exp() / target - 1.).abs() < 2e-5, "{result:?}");
        }
    }
}

#[test]
fn all_xtrans_transforms_levels_and_colour_ratios_preserve_planes() {
    for transform in 0..8 {
        let pattern: [u8; 36] = std::array::from_fn(|i| {
            let (mut x, mut y) = (i % 6, i / 6);
            if transform & 1 != 0 {
                std::mem::swap(&mut x, &mut y);
            }
            if transform & 2 != 0 {
                x = 5 - x;
            }
            if transform & 4 != 0 {
                y = 5 - y;
            }
            XTRANS[y * 6 + x]
        });
        let cfa = Cfa::new(6, &pattern).unwrap();
        for dy in 0..6 {
            for dx in 0..6 {
                for size in [5, 11, 12] {
                    for (neutral, scale) in [
                        ([0.5, 1., 0.75], 0.05),
                        ([1.5, 1., 0.45], 1.),
                        ([0.25, 1., 1.2], 0.6),
                    ] {
                        let mut raw = mosaic(cfa.shifted(dx, dy), size, size);
                        raw.calibration.neutral = neutral;
                        fill_plane(&mut raw, [-0.21, 0.32]);
                        raw.values.iter_mut().for_each(|v| *v *= scale);
                        let result = raw.region([0, 0, size, size]).unwrap();
                        for (v, expected) in result.into_iter().zip([neutral[0], neutral[2]]) {
                            assert!(
                                (v.exp() / expected - 1.).abs() < 3e-7,
                                "{transform}/{dx}/{dy}/{size}: {result:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn missing_plane_support_and_sensor_limits_are_rejected() {
    let line: Vec<_> = (0..6)
        .map(|i| Site {
            x: i as f64,
            y: i as f64,
            value: 0.4,
        })
        .collect();
    assert!(fit_plane(&line, &[1.; 6]).is_none());
    let mut raw = mosaic(Cfa::bayer([0, 1, 1, 2]), 11, 11);
    for v in [0., 0.002, 0.985, 1., f32::NAN, f32::INFINITY] {
        raw.values.fill(v);
        assert!(raw.region([0, 0, 11, 11]).is_none());
    }
    fill_plane(&mut raw, [0.2, 0.2]);
    // Some rejected sensor codes must not move the evaluation point to the
    // surviving samples' centroid. A plane still has a known common neutral.
    raw.values[0] = 1.;
    raw.values[1] = 0.;
    raw.values[12] = f32::NAN;
    let result = raw.region([0, 0, 11, 11]).unwrap();
    for (v, target) in result.into_iter().zip([0.5, 0.75]) {
        assert!((v.exp() / target - 1.).abs() < 3e-7);
    }
}

fn fill_plane(raw: &mut Mosaic, slope: [f64; 2]) {
    for y in 0..raw.height {
        for x in 0..raw.width {
            let light = 0.4
                + slope[0] * (x as f64 - (raw.width - 1) as f64 / 2.) / raw.width as f64
                + slope[1] * (y as f64 - (raw.height - 1) as f64 / 2.) / raw.height as f64;
            raw.values[y * raw.width + x] =
                (light * raw.calibration.neutral[raw.cfa.color(x, y)]) as f32;
        }
    }
}

#[test]
fn neutral_illumination_plane_is_not_a_white_balance_shift() {
    for cfa in [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS).unwrap()] {
        for dy in 0..cfa.period {
            for dx in 0..cfa.period {
                for size in [5, 11, 12] {
                    for slope in [[0., 0.], [0.2, 0.2], [-0.3, 0.1], [0., -0.4]] {
                        let mut raw = mosaic(cfa.shifted(dx, dy), size, size);
                        fill_plane(&mut raw, slope);
                        let measured = raw.region([0, 0, size, size]).unwrap();
                        for (value, expected) in measured.into_iter().zip([0.5_f64, 0.75]) {
                            let error = (value.exp() / expected - 1.).abs();
                            assert!(
                                error < 3e-7,
                                "period={}, phase=({dx},{dy}), size={size}, slope={slope:?}, error={error}",
                                cfa.period
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn clipped_crop_and_orientation_keep_the_same_neutral() {
    for cfa in [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS).unwrap()] {
        let mut raw = mosaic(cfa, 80, 72);
        raw.crop = [3, 5, 67, 59];
        fill_plane(&mut raw, [0.3, -0.2]);
        for orientation in 1..=8 {
            raw.orientation = orientation;
            let (w, h) = render::oriented_size(67, 59, orientation);
            for (x, y) in [
                (0, 0),
                (w - 1, 0),
                (0, h - 1),
                (w - 1, h - 1),
                (w / 2, h / 2),
            ] {
                let wb = raw.estimate(Analysis::Patch { x, y, side: 11 }).unwrap();
                assert_eq!(wb.gains_f64(), [1.; 3], "{orientation}: {x}, {y}");
            }
        }
    }
}
