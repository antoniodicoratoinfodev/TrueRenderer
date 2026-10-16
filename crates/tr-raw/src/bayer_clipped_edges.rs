//! Interpolate the observed surface at a partially clipped Bayer boundary.
//! This neither raises clipped RAW samples nor infers their missing colour.
use crate::{cfa::Cfa, render::Support};

const RADIUS: usize = 6;

pub(crate) fn interpolate(
    (mosaic, support): (&[f32], Support<'_>),
    w: usize,
    h: usize,
    cfa: Cfa,
    (x, y): (usize, usize),
    [low, high]: [[f32; 3]; 2],
    (previous, matrix): ([f32; 3], crate::color::Matrix),
) -> Option<[f32; 3]> {
    affine((mosaic, support), w, h, cfa, (x, y), [low, high]).or_else(|| {
        let mut prediction = constant((mosaic, support), w, h, cfa, (x, y), [low, high])?;
        // A clipped green plateau is a lower bound, not an equality for the
        // missing green. Couple its estimate to the corrected measured R/B.
        if prediction[1].to_bits() == high[1].to_bits() {
            // Project the existing RGB estimate onto the measured R/B
            // constraints, allowing green only above its censored bound.
            // This is the closest estimate in linear working-space RGB;
            // it is not a neutral or a recovered RAW measurement.
            let scale = matrix.iter().flatten().fold(0f64, |a, b| a.max(b.abs()));
            if !scale.is_finite() || scale <= 0. {
                return None;
            }
            let mut numerator = 0.;
            let mut denominator = 0.;
            for row in matrix {
                let g = row[1] / scale;
                numerator += g
                    * ((row[0] / scale) * f64::from(prediction[0] - previous[0])
                        + (row[2] / scale) * f64::from(prediction[2] - previous[2]));
                denominator += g * g;
            }
            let green = (f64::from(previous[1]) - numerator / denominator) as f32;
            if !green.is_finite() {
                return None;
            }
            prediction[1] = prediction[1].max(green);
        }
        Some(prediction)
    })
}

fn affine(
    (mosaic, support): (&[f32], Support<'_>),
    w: usize,
    h: usize,
    cfa: Cfa,
    (x, y): (usize, usize),
    [low, high]: [[f32; 3]; 2],
) -> Option<[f32; 3]> {
    debug_assert_eq!(cfa.period, 2);
    // Keep the directional construction at axial or ambiguous boundaries.
    // Both surfaces, all channels and the common monotone boundary must pass
    // the existing affine fit. Correcting only one side/channel creates seams.
    let model =
        crate::edge_planes::interpolate_with_bounds(mosaic, w, h, cfa, (x, y), [low, high], false)?;
    let midpoint = std::array::from_fn::<_, 3, _>(|c| low[c] + 0.5 * (high[c] - low[c]));
    let (x0, y0) = (x.saturating_sub(RADIUS), y.saturating_sub(RADIUS));
    let (x1, y1) = ((x + RADIUS + 1).min(w), (y + RADIUS + 1).min(h));
    let mut lower_count = [0; 3];
    let mut lower_min_xy = [[usize::MAX; 2]; 3];
    let mut lower_max_xy = [[0; 2]; 3];
    let mut upper_min = [f32::INFINITY; 3];
    let mut clipped_colours = 0u8;
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let value = mosaic[yy * w + xx];
            let measured = support.measured(xx, yy);
            if value > midpoint[c] {
                upper_min[c] = upper_min[c].min(value);
                if !measured {
                    // A recovered value is an estimate, not evidence of a
                    // flat sensor plateau. Use only unreconstructed samples.
                    if !support.unsupported_colour(xx, yy, c) {
                        return None;
                    }
                    clipped_colours |= 1 << c;
                }
            } else {
                if !measured {
                    return None;
                }
                lower_count[c] += 1;
                lower_min_xy[c][0] = lower_min_xy[c][0].min(xx);
                lower_min_xy[c][1] = lower_min_xy[c][1].min(yy);
                lower_max_xy[c][0] = lower_max_xy[c][0].max(xx);
                lower_max_xy[c][1] = lower_max_xy[c][1].max(yy);
            }
        }
    }
    // Qualify the failure of the directional green stage only: it can
    // propagate a clipped green boundary into false R/B residuals. Both
    // chromatic channels must remain measured. Clipped R/B and multiple
    // clipped colours retain the previous construction; their extension has
    // not passed the independent colour checks.
    // The clipped channel must be a bit-exact observed plateau throughout
    // the bright side; a partly clipped ramp is not an affine measurement.
    if clipped_colours != 1 << 1
        || lower_count.iter().any(|&n| n < 6)
        || (0..3).any(|c| {
            (0..2).any(|axis| lower_max_xy[c][axis].saturating_sub(lower_min_xy[c][axis]) < 4)
        })
        || (0..3)
            .any(|c| clipped_colours & (1 << c) != 0 && upper_min[c].to_bits() != high[c].to_bits())
    {
        return None;
    }
    Some(model.rgb)
}

// A constant surface has only one parameter per colour: it does not require
// the two-dimensional support needed to identify an affine gradient. This
// separate model is admitted only for bit-exact plateaux, never by relaxing
// the affine residual or treating clipped values as recovered measurements.
fn constant(
    (mosaic, support): (&[f32], Support<'_>),
    w: usize,
    h: usize,
    cfa: Cfa,
    (x, y): (usize, usize),
    [low, high]: [[f32; 3]; 2],
) -> Option<[f32; 3]> {
    if (0..3).any(|c| high[c] - low[c] < 0.1 * high[c].abs().max(low[c].abs()).max(0.01)) {
        return None;
    }
    let (x0, y0) = (x.saturating_sub(RADIUS), y.saturating_sub(RADIUS));
    let (x1, y1) = ((x + RADIUS + 1).min(w), (y + RADIUS + 1).min(h));
    let mut mask = [[false; 2 * RADIUS + 1]; 2 * RADIUS + 1];
    let mut counts = [[0; 3]; 2];
    let mut clipped = false;
    for yy in y0..y1 {
        for xx in x0..x1 {
            let c = cfa.color(xx, yy);
            let value = mosaic[yy * w + xx];
            let upper = value.to_bits() == high[c].to_bits();
            if !upper && value.to_bits() != low[c].to_bits() {
                return None;
            }
            if !support.measured(xx, yy) {
                if c != 1 || !upper || !support.unsupported_colour(xx, yy, c) {
                    return None;
                }
                clipped = true;
            }
            counts[usize::from(upper)][c] += 1;
            mask[yy - y0][xx - x0] = upper;
        }
    }
    if !clipped || counts.iter().flatten().any(|&n| n < 3) {
        return None;
    }
    // Keep dir4's qualified non-axial scope. Replacing axial clipped plateaux
    // has not passed the separate colour and gradient checks.
    let axial = (1..y1 - y0).all(|yy| mask[yy][..x1 - x0] == mask[0][..x1 - x0])
        || (0..y1 - y0).all(|yy| (1..x1 - x0).all(|xx| mask[yy][xx] == mask[yy][0]));
    if axial {
        return None;
    }
    let monotone = (0..2).any(|axis| {
        [false, true].into_iter().any(|increasing| {
            let (lines, length) = if axis == 0 {
                (y1 - y0, x1 - x0)
            } else {
                (x1 - x0, y1 - y0)
            };
            (0..lines).all(|line| {
                (1..length).all(|p| {
                    let get = |k: usize| {
                        if axis == 0 {
                            mask[line][k]
                        } else {
                            mask[k][line]
                        }
                    };
                    if increasing {
                        !get(p - 1) || get(p)
                    } else {
                        get(p - 1) || !get(p)
                    }
                })
            })
        })
    });
    monotone.then_some(if mask[y - y0][x - x0] { high } else { low })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlights::SATURATED;

    const W: usize = 43;
    const H: usize = 37;

    #[test]
    fn constant_border_surfaces_do_not_need_an_identifiable_affine_gradient() {
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
            let mut values = vec![0.; W * H];
            let mut state = vec![0; W * H];
            let mut cells = vec![0; W.div_ceil(6) * H.div_ceil(6)];
            let low = [-0.02, 0.11, -0.04];
            let high = [0.9, 1., 1.7];
            for y in 0..H {
                for x in 0..W {
                    let c = cfa.color(x, y);
                    let upper = x >= 2 + y / 9;
                    values[y * W + x] = if upper { high[c] } else { low[c] };
                    if upper && c == 1 {
                        state[y * W + x] = SATURATED;
                        cells[y / 6 * W.div_ceil(6) + x / 6] |= 2;
                    }
                }
            }
            let bits = crate::render::original_clipping_bits(&state);
            let support = Support::new(&cells, W.div_ceil(6)).with_original_clipping(&bits, W);
            let mut ranges = crate::edge_planes::RangeRows::new(W, H);
            ranges.prepare(&values, W, cfa, 7);
            for x in [1, 2] {
                let bounds = ranges.at(x, 7);
                assert!(affine((&values, support), W, H, cfa, (x, 7), bounds).is_none());
                assert_eq!(
                    constant((&values, support), W, H, cfa, (x, 7), bounds),
                    Some(if x < 2 { low } else { high })
                );
                for previous_green in [0.8, 1.35] {
                    let prediction = interpolate(
                        (&values, support),
                        W,
                        H,
                        cfa,
                        (x, 7),
                        bounds,
                        ([4., previous_green, -2.], crate::color::ID),
                    )
                    .unwrap();
                    let expected_green = if x < 2 {
                        low[1]
                    } else {
                        previous_green.max(high[1])
                    };
                    assert_eq!(prediction[1], expected_green);
                    assert_eq!(prediction[0], if x < 2 { low[0] } else { high[0] });
                    assert_eq!(prediction[2], if x < 2 { low[2] } else { high[2] });
                }
                if x == 2 {
                    // Orthogonal projection onto the censored-green ray
                    // cannot increase squared RGB distance to any point of
                    // that ray. This does not assert a lost scene colour.
                    let matrix = [[1.2, -0.3, 0.1], [-0.2, 1.4, -0.2], [0.05, -0.2, 1.15]];
                    let previous = [4., 1.35, -2.];
                    let prediction = interpolate(
                        (&values, support),
                        W,
                        H,
                        cfa,
                        (x, 7),
                        bounds,
                        (previous, matrix),
                    )
                    .unwrap();
                    assert!(prediction[1].is_finite() && prediction[1] >= high[1]);
                    for green in [1., 1.3, 4., 16.] {
                        let truth = crate::color::project(matrix, [high[0], green, high[2]]);
                        let error = |rgb| {
                            crate::color::project(matrix, rgb)
                                .into_iter()
                                .zip(truth)
                                .map(|(a, b)| f64::from(a - b).powi(2))
                                .sum::<f64>()
                        };
                        assert!(error(prediction) <= error(previous) + 1e-5);
                    }
                    let unrepresentable = [[1., 1e-10, 0.], [0., 1e-10, 0.], [0., 0., 1.]];
                    assert!(
                        interpolate(
                            (&values, support),
                            W,
                            H,
                            cfa,
                            (x, 7),
                            bounds,
                            ([f32::MAX; 3], unrepresentable),
                        )
                        .is_none()
                    );
                }
            }
            let image = crate::render::develop_supported(
                (&values, support),
                W,
                H,
                cfa,
                crate::color::ID,
                [0, 0, W, H],
                1,
            )
            .unwrap();
            for (i, p) in image.pixels.iter().enumerate() {
                assert_eq!(p[cfa.color(i % W, i / W)].to_bits(), values[i].to_bits());
                assert!(p.iter().all(|v| v.is_finite()));
            }
            for x in [1, 2] {
                let expected = if x < 2 { low } else { high };
                let actual = image.pixels[7 * W + x];
                assert_eq!(actual[0], expected[0]);
                assert_eq!(actual[2], expected[2]);
                if x < 2 || cfa.color(x, 7) == 1 {
                    assert_eq!(actual[1], expected[1]);
                } else {
                    assert!(actual[1] >= expected[1]);
                }
            }
        }
    }

    #[test]
    fn constant_model_refuses_texture_recovered_evidence_and_missing_colour_support() {
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let (mut values, state, cells) = scene(cfa, 1);
        let bits = crate::render::original_clipping_bits(&state);
        let support = Support::new(&cells, W.div_ceil(6)).with_original_clipping(&bits, W);
        let sample = |values: &[f32], support, x, y| {
            let mut ranges = crate::edge_planes::RangeRows::new(W, H);
            ranges.prepare(values, W, cfa, y);
            constant((values, support), W, H, cfa, (x, y), ranges.at(x, y))
        };
        assert!(sample(&values, support, 18, 18).is_some());
        assert!(sample(&values, Support::default(), 18, 18).is_none());
        assert!(sample(&values, support, 0, 18).is_none());
        let i = 18 * W + 16;
        values[i] = f32::from_bits(values[i].to_bits() + 1);
        assert!(sample(&values, support, 18, 18).is_none());
        values[i] = 0.11;
        let mut reconstructed = cells.clone();
        reconstructed.fill(0);
        let support = Support::new(&reconstructed, W.div_ceil(6)).with_original_clipping(&bits, W);
        assert!(sample(&values, support, 18, 18).is_none());
    }

    fn scene(cfa: Cfa, clipped: usize) -> (Vec<f32>, Vec<u8>, Vec<u8>) {
        let mut values = vec![0.; W * H];
        let mut state = vec![0; W * H];
        let mut cells = vec![0; W.div_ceil(6) * H.div_ceil(6)];
        for y in 0..H {
            for x in 0..W {
                let c = cfa.color(x, y);
                let upper = x >= 13 + y / 3;
                values[y * W + x] = if upper {
                    if c == clipped { 1. } else { [0.8, 0.6, 0.7][c] }
                } else {
                    [0.11, 0.08, 0.16][c]
                };
                if upper && c == clipped {
                    state[y * W + x] = SATURATED;
                    cells[y / 6 * W.div_ceil(6) + x / 6] |= 1 << c;
                }
            }
        }
        (values, state, cells)
    }

    fn predict(values: &[f32], state: &[u8], cells: &[u8], cfa: Cfa) -> Option<[f32; 3]> {
        let bits = crate::render::original_clipping_bits(state);
        let support = Support::new(cells, W.div_ceil(6)).with_original_clipping(&bits, W);
        let mut ranges = crate::edge_planes::RangeRows::new(W, H);
        ranges.prepare(values, W, cfa, 18);
        interpolate(
            (values, support),
            W,
            H,
            cfa,
            (18, 18),
            ranges.at(18, 18),
            ([0.; 3], crate::color::ID),
        )
    }

    #[test]
    fn oblique_clipped_boundaries_retain_all_samples_and_the_two_observed_surfaces() {
        for phase in 0..4 {
            let cfa = Cfa::bayer([0, 1, 1, 2]).shifted(phase % 2, phase / 2);
            for clipped in 0..3 {
                let (values, state, cells) = scene(cfa, clipped);
                let bits = crate::render::original_clipping_bits(&state);
                let support = Support::new(&cells, W.div_ceil(6)).with_original_clipping(&bits, W);
                let image = crate::render::develop_supported(
                    (&values, support),
                    W,
                    H,
                    cfa,
                    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                    [0, 0, W, H],
                    1,
                )
                .unwrap();
                for y in 0..H {
                    for x in 0..W {
                        let i = y * W + x;
                        assert_eq!(
                            image.pixels[i][cfa.color(x, y)].to_bits(),
                            values[i].to_bits()
                        );
                    }
                }
                if clipped != 1 {
                    continue;
                }
                for x in [18, 20] {
                    let expected = if x < 19 {
                        [0.11, 0.08, 0.16]
                    } else {
                        std::array::from_fn(|c| if c == clipped { 1. } else { [0.8, 0.6, 0.7][c] })
                    };
                    for (c, &value) in expected.iter().enumerate() {
                        assert!(
                            (image.pixels[18 * W + x][c] - value).abs() < 1e-6,
                            "phase {phase}, clipped {clipped}, x {x}, channel {c}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_plateau_requires_original_unreconstructed_green_clipping() {
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let (mut values, mut state, mut cells) = scene(cfa, 1);
        assert!(predict(&values, &state, &cells, cfa).is_some());
        assert!(predict(&values, &vec![0; state.len()], &cells, cfa).is_none());
        assert!(predict(&values, &state, &vec![0; cells.len()], cfa).is_none());
        // A tiny measured shoulder passes the affine tolerance, but is not
        // a constant clipped plateau and must not enter this fallback.
        let i = 18 * W + 21;
        values[i] = 0.9995;
        state[i] = 0;
        assert!(predict(&values, &state, &cells, cfa).is_none());
        values[i] = 1.;
        state[i] = SATURATED;
        for y in 0..H {
            for x in 0..W {
                if x >= 13 + y / 3 && cfa.color(x, y) == 2 {
                    state[y * W + x] = SATURATED;
                    cells[y / 6 * W.div_ceil(6) + x / 6] |= 4;
                }
            }
        }
        assert!(predict(&values, &state, &cells, cfa).is_none());
    }

    #[test]
    fn clipped_dark_side_and_unmodelled_texture_are_refused() {
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let (mut values, mut state, cells) = scene(cfa, 1);
        let i = 18 * W + 16;
        state[i] = SATURATED;
        assert!(predict(&values, &state, &cells, cfa).is_none());
        state[i] = 0;
        values[i] += 0.02;
        assert!(predict(&values, &state, &cells, cfa).is_none());
    }
}
