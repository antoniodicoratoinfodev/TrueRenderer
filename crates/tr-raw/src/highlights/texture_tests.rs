use super::*;

type Scene = (Vec<f32>, Vec<f32>, Vec<u8>, Vec<Cell>, Vec<u8>);

fn scene(cfa: Cfa, stride: usize) -> Scene {
    let (w, h) = (96_usize, 80_usize);
    let mut values = vec![-17.; stride * h];
    let mut truth = values.clone();
    let mut state = vec![SATURATED; values.len()];
    let mut cells = vec![Cell::default(); w.div_ceil(SIDE) * h.div_ceil(SIDE)];
    for y in 0..h {
        for x in 0..w {
            let r = 0.42 + 0.025 * (0.23 * x as f64 + 0.19 * y as f64).sin() + 0.0015 * y as f64;
            let b = 0.3 + 0.04 * (0.17 * x as f64 - 0.13 * y as f64).cos() + 0.001 * x as f64;
            let g = 0.8 * r + 1.4 * b + 0.003 * y as f64 + 0.03;
            let c = cfa.color(x, y);
            let v = [r, g, b][c] as f32;
            let i = y * stride + x;
            truth[i] = v;
            values[i] = (v.min(1.) * 64488.).round() / 64488.;
            state[i] = super::super::classify(f64::from(values[i]), 0., 1.);
            if state[i] == SATURATED {
                cells[y / SIDE * w.div_ceil(SIDE) + x / SIDE].clipped |= 1 << c;
            }
        }
    }
    let masks = cells.iter().map(|c| c.clipped).collect();
    (truth, values, state, cells, masks)
}

#[test]
fn two_frequency_colour_texture_uses_original_samples_in_all_bayer_phases() {
    let base = Cfa::bayer([0, 1, 1, 2]);
    for phase in 0..4 {
        let cfa = base.shifted(phase % 2, phase / 2);
        let (truth, mut v, state, cells, mut masks) = scene(cfa, 96);
        let before = v.clone();
        let evidence = recover(&mut v, &state, [96, 96, 80], cfa, &cells, &mut masks);
        assert!(evidence.reconstructed > 1000, "{cfa:?}: {evidence:?}");
        assert!(masks.iter().all(|&m| m & 7 == 0));
        for i in 0..v.len() {
            if state[i] != SATURATED {
                assert_eq!(v[i].to_bits(), before[i].to_bits());
            } else {
                assert!(
                    (v[i] - truth[i]).abs() < 0.001,
                    "phase {phase} site {i}: {} != {}",
                    v[i],
                    truth[i]
                );
            }
        }
    }
}

#[test]
fn a_colour_disagreement_or_partial_previous_support_rejects_the_whole_region() {
    let cfa = Cfa::bayer([0, 1, 1, 2]);
    for partial in [false, true] {
        let (_, mut v, state, cells, mut masks) = scene(cfa, 96);
        if partial {
            *masks.iter_mut().find(|m| **m != 0).unwrap() = 0;
        } else {
            let i = (15..50)
                .flat_map(|y| (16..80).map(move |x| y * 96 + x))
                .find(|&i| cfa.color(i % 96, i / 96) == 1 && state[i] == 0 && v[i] > 0.9)
                .unwrap();
            v[i] *= 0.95;
        }
        let before = v.clone();
        assert_eq!(
            recover(&mut v, &state, [96, 96, 80], cfa, &cells, &mut masks).reconstructed,
            0
        );
        assert_eq!(v, before);
    }
}

#[test]
fn active_width_and_padding_are_independent_of_the_fit() {
    for phase in 0..4 {
        let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
        let (_, mut v, state, cells, mut masks) = scene(cfa, 96);
        let (_, mut padded, ps, pc, mut pm) = scene(cfa, 103);
        let a = recover(&mut v, &state, [96, 96, 80], cfa, &cells, &mut masks);
        let b = recover(&mut padded, &ps, [103, 96, 80], cfa, &pc, &mut pm);
        assert_eq!(a.reconstructed, b.reconstructed);
        assert_eq!(masks, pm);
        for y in 0..80 {
            assert_eq!(v[y * 96..(y + 1) * 96], padded[y * 103..y * 103 + 96]);
            assert!(
                padded[y * 103 + 96..(y + 1) * 103]
                    .iter()
                    .all(|&v| v == -17.)
            );
        }
    }
}

#[test]
fn multiple_clipped_channels_and_unknown_constant_highlights_are_unchanged() {
    let cfa = Cfa::bayer([0, 1, 1, 2]);
    let (_, mut v, mut state, mut cells, mut masks) = scene(cfa, 96);
    for y in 0..80 {
        for x in 0..96 {
            if cfa.color(x, y) != 1 && y > 40 {
                let i = y * 96 + x;
                v[i] = 1.;
                state[i] = SATURATED;
                let cell = y / SIDE * 96_usize.div_ceil(SIDE) + x / SIDE;
                cells[cell].clipped |= 1 << cfa.color(x, y);
                masks[cell] = cells[cell].clipped;
            }
        }
    }
    let before = v.clone();
    assert_eq!(
        recover(&mut v, &state, [96, 96, 80], cfa, &cells, &mut masks).reconstructed,
        0
    );
    assert_eq!(v, before);
    let mut v = vec![1.; 96 * 80];
    let state = vec![SATURATED; v.len()];
    let cells = vec![
        Cell {
            clipped: 7,
            ..Cell::default()
        };
        16 * 14
    ];
    let mut masks = vec![7; cells.len()];
    assert_eq!(
        recover(&mut v, &state, [96, 96, 80], cfa, &cells, &mut masks).reconstructed,
        0
    );
    assert!(v.iter().all(|&v| v == 1.));
}

#[test]
fn underdetermined_and_nonfinite_systems_do_not_gain_a_colour_prior() {
    assert!(solve([[1.; TERMS]; TERMS], [1.; TERMS]).is_none());
    let mut matrix = [[0.; TERMS]; TERMS];
    for (i, r) in matrix.iter_mut().enumerate() {
        r[i] = 1.;
    }
    assert_eq!(solve(matrix, [2.; TERMS]), Some([2.; TERMS]));
    matrix[4][4] = 1e-8;
    assert!(solve(matrix, [2.; TERMS]).is_none());
    matrix[4][4] = f64::NAN;
    assert!(solve(matrix, [2.; TERMS]).is_none());
    // Two live model arrays plus the solve, moments and local basis fit inside
    // the existing 16 KiB recovery reserve; no image-sized feature raster.
    assert!(2 * size_of::<[[Option<Fit>; 9]; 2]>() + 4096 < 16 * 1024);
}
