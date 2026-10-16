use super::*;
use crate::cfa::XTRANS;

#[test]
fn continued_regions_require_support_for_all_connected_clipped_colours() {
    let cells = [1, 3, 2].map(|clipped| Cell {
        clipped,
        ..Cell::default()
    });
    let estimate = Some(Reconstruction {
        model: Model {
            plane: [1., 0., 0.],
            jump: 0.,
            split: 0,
            split2: 0,
            side: 6,
            border: 0,
            correction: [0.; 2],
        },
        ratio: [1.; 3],
        extended: true,
    });
    assert_eq!(
        complete_regions(&cells, &[None, estimate, estimate], 3, 1),
        vec![0; 3]
    );
    assert_eq!(
        complete_regions(&cells, &[estimate, estimate, estimate], 3, 1)
            .into_iter()
            .map(|v| v & 7)
            .collect::<Vec<_>>(),
        vec![1, 3, 2]
    );
}

#[test]
fn small_fully_clipped_tail_uses_surviving_coloured_plane_not_neutral() {
    for cfa in phases() {
        let (w, h) = (96, 48);
        let (truth, mut values, state) = scene(cfa, w, h, |x, y| {
            [0.8, 0.9, 1.].map(|c| {
                c * (0.3 + 0.98 * x as f32 / (w - 1) as f32 + 0.002 * y as f32 / (h - 1) as f32)
            })
        });
        let before = values.clone();
        let evidence = recover(&mut values, &state, w, w, h, cfa);
        assert!(
            evidence.reconstructed > evidence.saturated * 9 / 10,
            "{cfa:?}: {evidence:?}"
        );
        for i in 0..values.len() {
            if state[i] != SATURATED {
                assert_eq!(values[i].to_bits(), before[i].to_bits());
            }
            assert!(
                (values[i] - truth[i]).abs() < 0.00011,
                "{cfa:?} {i}: {} {}",
                values[i],
                truth[i]
            );
        }
    }
}

fn phases() -> Vec<Cfa> {
    [Cfa::bayer([0, 1, 1, 2]), Cfa::new(6, &XTRANS).unwrap()]
        .into_iter()
        .flat_map(|base| {
            (0..base.period * base.period)
                .map(move |p| base.shifted(p % base.period, p / base.period))
        })
        .collect()
}

fn scene(
    cfa: Cfa,
    width: usize,
    height: usize,
    f: impl Fn(usize, usize) -> [f32; 3],
) -> (Vec<f32>, Vec<f32>, Vec<u8>) {
    let mut truth = vec![];
    let mut values = vec![];
    let mut states = vec![];
    for y in 0..height {
        for x in 0..width {
            let v = f(x, y)[cfa.color(x, y)];
            truth.push(v);
            values.push(v.min(1.));
            states.push(classify(f64::from(v.min(1.)), 0., 1.));
        }
    }
    (truth, values, states)
}

#[test]
fn stricter_continuity_cannot_create_corrected_islands() {
    let cells = [Cell {
        clipped: 1,
        ..Cell::default()
    }; 3];
    let mut allowed = [129, 65, 137];
    complete_continuity(&cells, &mut allowed, 3, 1);
    assert_eq!(allowed, [1, 65, 9]);
    let mut allowed = [129; 3];
    complete_continuity(&cells, &mut allowed, 3, 1);
    assert_eq!(allowed, [129; 3]);
    let mut separate = cells;
    separate[1].clipped = 0;
    let mut allowed = [129, 0, 1];
    complete_continuity(&separate, &mut allowed, 3, 1);
    assert_eq!(allowed, [129, 0, 1]);
    let diagonal = [1, 0, 0, 1].map(|clipped| Cell {
        clipped,
        ..Cell::default()
    });
    let mut allowed = [129, 0, 0, 1];
    complete_continuity(&diagonal, &mut allowed, 2, 2);
    assert_eq!(allowed, [1, 0, 0, 1]);
}

#[test]
fn small_lifts_require_original_affine_support_outside_the_cell() {
    let (w, h, stride) = (42, 42, 47);
    let cfa = Cfa::bayer([0, 1, 1, 2]);
    let (truth, image, states) = scene(cfa, w, h, |x, _| {
        [1., 0.5, 0.7].map(|v| v * (1.00004 + 0.008 * (x as f32 - 18.)))
    });
    let mut values = vec![f32::NAN; stride * h];
    let mut state = vec![0; stride * h];
    for y in 0..h {
        values[y * stride..y * stride + w].copy_from_slice(&image[y * w..(y + 1) * w]);
        state[y * stride..y * stride + w].copy_from_slice(&states[y * w..(y + 1) * w]);
    }
    let r = Reconstruction {
        model: Model {
            plane: [1.02004, 0.02, 0.],
            jump: 0.,
            split: 0,
            split2: 0,
            side: 6,
            border: 0,
            correction: [0.; 2],
        },
        ratio: [1., 0.5, 0.7],
        extended: false,
    };
    let accepts = |v: &[f32]| {
        let grid = Grid {
            data: vec![],
            values: v,
            state: &state,
            stride,
            width: w,
            height: h,
            columns: w / SIDE,
            cfa,
            designs: vec![],
        };
        continuous_bayer_plane(&grid, 3, 3, r, 1)
    };
    assert!(truth[18 * w + 18] > 1.);
    assert!(accepts(&values));
    // A valid red site just outside the target cell invalidates continuation,
    // even though a 1% change is within HL4's looser colour agreement.
    let i = 18 * stride + 16;
    assert_ne!(state[i], SATURATED);
    values[i] *= 0.99;
    assert!(!accepts(&values));
}

#[test]
fn supported_bayer_ramps_remain_continuous_through_the_clipping_limit() {
    // A sub-0.01% highlight is still part of a supported plane. A relative
    // dead band leaves isolated clipped sites and creates a false colour edge.
    for cfa in phases().into_iter().filter(|cfa| cfa.period == 2) {
        for axis in 0..2 {
            for hue in [[1., 0.4, 0.7], [0.5, 1., 0.8], [0.3, 0.6, 1.]] {
                let (w, h) = (97, 73); // Partial cells and every CFA phase.
                let bright = hue.iter().position(|&v| v == 1.).unwrap();
                let crossing = (42..42 + cfa.period)
                    .find(|&p| {
                        (0..cfa.period).any(|q| {
                            let [x, y] = if axis == 0 { [p, q] } else { [q, p] };
                            cfa.color(x, y) == bright
                        })
                    })
                    .unwrap() as f32;
                let (truth, mut values, state) = scene(cfa, w, h, |x, y| {
                    let p = [x, y][axis] as f32;
                    hue.map(|v| v * (1.00004 + 0.008 * (p - crossing)))
                });
                let original = values.clone();
                recover(&mut values, &state, w, w, h, cfa);
                let mut near_limit = 0;
                for i in 0..values.len() {
                    if state[i] != SATURATED {
                        assert_eq!(values[i].to_bits(), original[i].to_bits());
                    } else {
                        assert!(
                            (values[i] - truth[i]).abs() < 2e-6,
                            "{cfa:?}, axis {axis}, {hue:?}, site {i}: {} vs {}",
                            values[i],
                            truth[i]
                        );
                        if truth[i] > 1. && truth[i] < 1.0001 {
                            near_limit += 1;
                            assert!(values[i] > 1.);
                        }
                    }
                }
                assert!(near_limit > 0);
            }
        }
    }
}

#[test]
fn ramps_recover_one_or_two_channels_without_changing_valid_measurements() {
    for cfa in phases() {
        for hue in [[0.4, 1., 0.7], [1., 0.35, 0.8], [0.3, 0.65, 1.]] {
            let (w, h) = (120, 48);
            let (truth, mut values, state) = scene(cfa, w, h, |x, y| {
                hue.map(|v| {
                    v * (0.35 + 1.1 * x as f32 / (w - 1) as f32 + 0.03 * y as f32 / h as f32)
                })
            });
            let before = values.clone();
            let result = recover(&mut values, &state, w, w, h, cfa);
            assert!(
                result.reconstructed > result.saturated / 3,
                "{hue:?} {cfa:?}: {result:?}"
            );
            for i in 0..values.len() {
                if state[i] != SATURATED {
                    assert_eq!(values[i].to_bits(), before[i].to_bits());
                }
                if values[i] != before[i] {
                    assert!(
                        (values[i] - truth[i]).abs() < 2e-5,
                        "{} {}",
                        values[i],
                        truth[i]
                    );
                }
            }
        }
    }
}

#[test]
fn unsupported_and_conflicting_colours_remain_unchanged() {
    for cfa in phases() {
        let (w, h) = (120, 60);
        for kind in 0..5 {
            let (_, mut values, state) = scene(cfa, w, h, |x, _| match kind {
                0 => [1.2, 1.3, 1.4],
                1 => [0.4, 1.3, 0.8],
                2 => {
                    if x < 30 {
                        [0.2, 0.3, 0.4]
                    } else {
                        [0.4, 1.3, 0.8]
                    }
                }
                3 => {
                    if x < 30 {
                        [0.2, 0.7, 0.4]
                    } else if x > 90 {
                        [0.2, 0.4, 0.4]
                    } else {
                        [0.4, 1.3, 0.8]
                    }
                }
                _ => {
                    if x < 30 {
                        [0.03, 0.3, 0.06]
                    } else {
                        [0.4, 1.3, 0.8]
                    }
                }
            });
            let before = values.clone();
            let result = recover(&mut values, &state, w, w, h, cfa);
            assert_eq!(result.reconstructed, 0, "{kind}: {result:?}");
            assert_eq!(values, before);
        }
    }
}

#[test]
fn digital_clipping_is_separate_from_the_nominal_white_and_wb() {
    assert_eq!(classify(15521., 512., 16383.), 0);
    assert_eq!(classify(16300., 512., 16383.), 1);
    assert_eq!(classify(16383., 512., 16383.), SATURATED);
    assert_eq!(classify(65000., 512., 65000.), SATURATED);
    assert_eq!(classify(65001., 512., 65000.), 1);
    assert_eq!(classify(98., 20., 100.), 0);
    assert_eq!(classify(98.5, 20., 100.), 1);
}

#[test]
fn extended_and_near_limit_measurements_survive_recovery_bit_exactly() {
    for cfa in phases() {
        // Odd geometry exercises the overlapping windows at the bottom/right.
        let (w, h) = (119, 47);
        let (_, mut values, mut state) = scene(cfa, w, h, |x, y| {
            [0.31, 1., 0.62]
                .map(|v| v * (0.35 + 1.13 * x as f32 / w as f32 + 0.02 * y as f32 / h as f32))
        });
        // Place real non-clipped samples next to highlights, in every channel.
        let measured = [-0.125f32, -0.0, 0.0, 0.979, 0.995, 1.00001, 1.7];
        for (j, &value) in measured.iter().enumerate() {
            for x in 80..86 {
                let i = (13 + j * 2) * w + x;
                values[i] = value;
                state[i] = classify(f64::from(value), 0., 1.);
            }
        }
        let before = values.clone();
        recover(&mut values, &state, w, w, h, cfa);
        for (i, value) in values.iter().enumerate() {
            assert!(value.is_finite());
            if state[i] != SATURATED {
                assert_eq!(value.to_bits(), before[i].to_bits(), "{cfa:?} at {i}");
            } else {
                assert!(*value >= before[i] && *value <= 4. * before[i]);
            }
        }
    }
}

#[test]
fn intensity_steps_keep_their_colour_across_axes_parities_and_cfa_phases() {
    for cfa in phases() {
        for axis in 0..2 {
            for shift in 0..6 {
                let (w, h) = (132, 84);
                let (n, first, second) = if axis == 0 {
                    (w, w / 3 + shift, 2 * w / 3 + shift)
                } else {
                    (h, h / 3 + shift, 2 * h / 3 + shift)
                };
                let (truth, mut values, state) = scene(cfa, w, h, |x, y| {
                    let p = if axis == 0 { x } else { y };
                    let light = if p < first {
                        0.75
                    } else if p < second {
                        1.12
                    } else {
                        1.5
                    };
                    [0.4, 1., 0.6].map(|v| v * light)
                });
                let before = values.clone();
                let result = recover(&mut values, &state, w, w, h, cfa);
                assert!(
                    result.reconstructed > result.saturated / 2,
                    "{cfa:?} axis {axis} shift {shift}: {result:?}"
                );
                for i in 0..values.len() {
                    if state[i] != SATURATED {
                        assert_eq!(values[i], before[i]);
                    }
                    let p = if axis == 0 { i % w } else { i / w };
                    if p > first + 3 && p.abs_diff(second) > 3 && p < n - 3 && state[i] == SATURATED
                    {
                        assert!(
                            (values[i] - truth[i]).abs() < 5e-5,
                            "{cfa:?} axis {axis} shift {shift} at {p}: {} vs {}",
                            values[i],
                            truth[i]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn an_unclipped_foreground_stops_colour_search_without_vetoing_the_background() {
    for cfa in phases() {
        let (w, h) = (156, 96);
        let (truth, mut values, state) = scene(cfa, w, h, |x, y| {
            if x < (y / 2 + 18) {
                [0.07, 0.16, 0.04]
            } else {
                [0.4, 1., 0.6].map(|v| v * (0.35 + 1.05 * x as f32 / (w - 1) as f32))
            }
        });
        let before = values.clone();
        let result = recover(&mut values, &state, w, w, h, cfa);
        assert!(
            result.reconstructed > result.saturated * 9 / 10,
            "{cfa:?}: {result:?}"
        );
        for i in 0..values.len() {
            if state[i] != SATURATED {
                assert_eq!(values[i], before[i]);
            }
            if values[i] != before[i] {
                assert!((values[i] - truth[i]).abs() < 5e-5);
            }
        }
    }
}

#[test]
fn different_fully_clipped_colours_produce_the_same_raw_and_are_not_invented() {
    for cfa in phases() {
        let (_, mut a, sa) = scene(cfa, 60, 42, |_, _| [2., 3., 4.]);
        let (_, b, sb) = scene(cfa, 60, 42, |_, _| [4., 2., 3.]);
        assert_eq!(a, b);
        assert_eq!(sa, sb);
        assert_eq!(recover(&mut a, &sa, 60, 60, 42, cfa).reconstructed, 0);
        assert_eq!(a, b);
    }
}

#[test]
fn ambiguous_bayer_cell_boundaries_use_neighbouring_measurements_symmetrically() {
    let cfa = Cfa::bayer([0, 1, 1, 2]);
    for axis in 0..2 {
        let (w, h) = if axis == 0 { (156, 60) } else { (60, 156) };
        let make = |cut| {
            scene(cfa, w, h, |x, y| {
                let p = if axis == 0 { x } else { y };
                let light = if p < 62 {
                    0.75
                } else if p < cut {
                    1.2
                } else {
                    1.5
                };
                [0.25, 1., 0.9].map(|v| v * light)
            })
        };
        let (_, mut values, state) = make(125);
        let (_, other, other_state) = make(126);
        // Red is the only surviving channel here; neither of these two
        // different edge positions can be distinguished by the recorded RAW.
        assert_eq!(values, other);
        assert_eq!(state, other_state);
        let before = values.clone();
        recover(&mut values, &state, w, w, h, cfa);
        let i = if axis == 0 {
            10 * w + 125
        } else {
            125 * w + 10
        };
        assert!(
            (values[i] - 1.35).abs() < 1e-5,
            "axis {axis}: {}",
            values[i]
        );
        for i in 0..values.len() {
            if state[i] != SATURATED {
                assert_eq!(values[i], before[i]);
            }
        }
    }
}

#[test]
fn insufficient_support_does_not_leave_reconstruction_islands_or_radius_seams() {
    for cfa in phases() {
        let (w, h) = (600, 42);
        let (_, mut values, state) = scene(cfa, w, h, |x, _| {
            [0.3, 1., 0.65].map(|v| v * (0.4 + 1.1 * x as f32 / (w - 1) as f32))
        });
        let before = values.clone();
        let result = recover(&mut values, &state, w, w, h, cfa);
        assert!(result.saturated > 0);
        assert_eq!(result.reconstructed, 0, "{result:?}");
        assert_eq!(values, before);
    }
}

#[test]
fn edge_cells_and_excluded_sensor_margin_keep_the_active_plane_consistent() {
    for cfa in phases() {
        let (w, h, stride) = (115, 43, 124);
        let (truth, mut values, state) = scene(cfa, stride, h, |x, y| {
            if x >= w {
                [0.8, 0.2, 0.1]
            } else {
                [0.3, 1., 0.65]
                    .map(|v| v * (0.4 + 1.1 * x as f32 / w as f32 + 0.02 * y as f32 / h as f32))
            }
        });
        let before = values.clone();
        let mut packed: Vec<_> = (0..h)
            .flat_map(|y| before[y * stride..y * stride + w].iter().copied())
            .collect();
        let packed_state: Vec<_> = (0..h)
            .flat_map(|y| state[y * stride..y * stride + w].iter().copied())
            .collect();
        recover(&mut packed, &packed_state, w, w, h, cfa);
        recover(&mut values, &state, stride, w, h, cfa);
        for y in 0..h {
            for x in 0..stride {
                let i = y * stride + x;
                if x < w {
                    assert_eq!(values[i].to_bits(), packed[y * w + x].to_bits());
                }
                if x >= w {
                    assert_eq!(values[i], before[i]);
                }
                if values[i] != before[i] {
                    assert!((values[i] - truth[i]).abs() < 2e-5);
                }
            }
        }
        assert!(
            scratch_bytes(stride, h) >= state.len() + w / SIDE * (h / SIDE) * size_of::<Cell>()
        );
    }
}

#[test]
fn clipping_metadata_roundtrips_and_rejects_changed_sample_domains() {
    use crate::{dng::Dng, test_support::Fixture};
    for le in [false, true] {
        let mut f = Fixture::new(le);
        f.ints(50717, 4, &[768]);
        let payload = metadata::encode(f.width, f.height, 768, 1024);
        f.tags
            .insert(50740, (1, payload.len() as u32, payload.clone()));
        f.pixels = (0..f.width * f.height)
            .map(|i| [700, 800, 1010, 1024, 1025][i % 5])
            .collect();
        let bytes = f.bytes();
        let parsed = Dng::parse(&bytes).unwrap();
        let raw = parsed.mosaic().unwrap();
        for (i, &s) in raw.saturation.iter().enumerate() {
            assert_eq!(s, [0, 0, 1, 2, 1][i % 5]);
        }
        let (_, _, copy) = parsed.repack(1 << 20).unwrap();
        let reopened = Dng::parse(&copy).unwrap().mosaic().unwrap();
        assert_eq!(raw.saturation, reopened.saturation);
        assert_eq!(raw.values, reopened.values);
        assert_eq!(
            crate::develop(&bytes, [1.; 3]).unwrap().pixels,
            crate::develop(&copy, [1.; 3]).unwrap().pixels
        );
        for broken in 0..8 {
            let mut data = payload.clone();
            let prefix = data.len() - 16;
            let mut kind = 1;
            match broken {
                0 => data[prefix - 1] = b'9',
                1 => {
                    data.pop();
                }
                2 => kind = 7,
                3 => data[prefix] ^= 1,
                4 => data[prefix + 4] ^= 1,
                5 => data[prefix + 8] ^= 1,
                6 => data[prefix + 12..].copy_from_slice(&767u32.to_le_bytes()),
                _ => data[prefix + 12..].copy_from_slice(&65536u32.to_le_bytes()),
            }
            f.tags.insert(50740, (kind, data.len() as u32, data));
            assert!(Dng::parse(&f.bytes()).is_err(), "{broken}");
        }
        f.tags.insert(50740, (1, payload.len() as u32, payload));
        f.ints(50712, 3, &[1, 2, 3]);
        assert!(Dng::parse(&f.bytes()).is_err());
        f.tags.remove(&50712);
        f.ints(50740, 1, b"Foreign\0opaque".map(u32::from).as_slice());
        assert!(Dng::parse(&f.bytes()).is_ok());
    }
}

#[test]
fn mask_is_after_lut_in_code_units_and_independent_of_spatial_black() {
    use crate::{dng::Dng, test_support::Fixture};
    for le in [false, true] {
        let mut f = Fixture::new(le);
        f.ints(50717, 4, &[1000]);
        f.ints(50712, 3, &[700, 999, 1000, 1001]);
        f.ints(50713, 3, &[2, 2]);
        f.rational(50714, false, &[4., 10., 20., 40.]);
        f.rational(
            50715,
            true,
            &(0..f.width).map(|x| x as f64 * 0.1).collect::<Vec<_>>(),
        );
        f.rational(
            50716,
            true,
            &(0..f.height).map(|y| y as f64 * 0.2).collect::<Vec<_>>(),
        );
        f.pixels = (0..f.width * f.height).map(|i| (i % 4) as u16).collect();
        let bytes = f.bytes();
        let raw = Dng::parse(&bytes).unwrap().mosaic().unwrap();
        for (i, &s) in raw.saturation.iter().enumerate() {
            assert_eq!(s, [0, 1, 2, 1][i % 4]);
        }
        let (_, _, copy) = crate::repack_dng(&bytes, 1 << 20).unwrap();
        assert_eq!(
            raw.saturation,
            Dng::parse(&copy).unwrap().mosaic().unwrap().saturation
        );
        assert_eq!(
            crate::develop(&bytes, [1.; 3]).unwrap().pixels,
            crate::develop(&copy, [1.; 3]).unwrap().pixels
        );
    }
}
