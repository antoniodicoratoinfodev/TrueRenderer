use super::{layers::*, *};

fn operators() -> Vec<Operator> {
    vec![
        Operator::SelectiveColor(SelectiveColor {
            adjustments: [[10., -8., 6., 2.]; 9],
            ..Default::default()
        }),
        Operator::Colorize(Colorize {
            amount: 30.,
            hue: 210.,
            exposure: 0.2,
            ..Default::default()
        }),
        Operator::TonalAdjustments(TonalAdjustments {
            exposure: 0.2,
            brightness: 12.,
            contrast: 8.,
            shadows: 15.,
            highlights: -18.,
            blacks: -10.,
            whites: 6.,
            ..Default::default()
        }),
        Operator::ExposureGamma(ExposureGamma {
            exposure: -0.1,
            offset: -0.02,
            gamma: 1.1,
            ..Default::default()
        }),
    ]
}

#[test]
fn selective_weights_partition_hue_neutrals_and_extended_values_continuously() {
    let s = SelectiveColor::default();
    for n in 0..1000 {
        let t = n as f32 / 999.;
        for p in [
            [-65504. * t, 65504. * (1. - t), 1. - 2. * t],
            [t; 3],
            color::hue_rgb(t * 360.),
        ] {
            let w = s.weights(p);
            assert!(
                w.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "{p:?}: {w:?}"
            );
            assert!((w.iter().sum::<f32>() - 1.).abs() < 2e-6);
        }
    }
    assert_eq!(s.weights([0.; 3])[8], 1.);
    assert!((s.weights([1.; 3])[6] - 1.).abs() < 1e-6);
    assert!(s.weights([0.18; 3])[7] > 0.85);
    assert!(s.weights([0.18; 3])[..6].iter().all(|w| *w == 0.));
    for i in 0..6 {
        assert!((s.weights(color::hue_rgb(i as f32 * 60.))[i] - 1.).abs() < 1e-6);
    }
    let left = s.weights([1., 0., 1e-6]);
    let right = s.weights([1., 1e-6, 0.]);
    assert!(
        left.into_iter()
            .zip(right)
            .all(|(a, b)| (a - b).abs() < 1e-5)
    );
    assert!(s.weights([1e-8, 0., 0.])[0] < 1e-6);
    assert!(s.weights([0., 0., 1e-8])[5] < 1e-6);
}

#[test]
fn selective_components_methods_and_fixed_guide_have_distinct_meanings() {
    let mut s = SelectiveColor::default();
    s.adjustments[0] = [25., -10., 0., 5.];
    let rgb = [0.8, 0.2, -0.1];
    let relative = Operator::SelectiveColor(s.clone()).apply(rgb, [1., 0., 0.]);
    for (actual, expected) in relative.into_iter().zip([0.56, 0.21, -0.095]) {
        assert!((actual - expected).abs() < 1e-6);
    }
    assert_eq!(
        Operator::SelectiveColor(s.clone()).apply(rgb, [0., 1., 0.]),
        rgb
    );
    s.method = SelectiveMethod::Absolute;
    let absolute = Operator::SelectiveColor(s.clone()).apply(rgb, [1., 0., 0.]);
    for (actual, expected) in absolute.into_iter().zip([0.5, 0.25, -0.15]) {
        assert!((actual - expected).abs() < 1e-6);
    }
    s.adjustments[8][3] = -20.;
    assert!(
        Operator::SelectiveColor(s)
            .apply([0.; 3], [0.; 3])
            .into_iter()
            .all(|v| (v - 0.2).abs() < 1e-6)
    );
}

#[test]
fn colorize_adds_hue_to_gray_preserves_y_and_separates_brightness() {
    let mut c = Colorize {
        amount: 100.,
        chroma: 80.,
        hue: 210.,
        ..Default::default()
    };
    for rgb in [
        [0.18; 3],
        [-0.1, 0.5, 2.],
        [-2.; 3],
        [0.; 3],
        [8., 12., -1.],
    ] {
        let out = Operator::Colorize(c.clone()).apply(rgb, rgb);
        assert!((detail::luma(out) - detail::luma(rgb)).abs() < 2e-6);
    }
    let gray = [0.18; 3];
    let tinted = Operator::Colorize(c.clone()).apply(gray, gray);
    assert!(tinted[2] > tinted[1] && tinted[1] > tinted[0]);
    c.exposure = 1.;
    let brighter = Operator::Colorize(c.clone()).apply(gray, gray);
    assert!((detail::luma(brighter) - 0.36).abs() < 1e-6);
    c.exposure = 0.;
    c.chroma = 0.;
    let out = Operator::Colorize(c).apply([0.1, 0.3, 0.7], gray);
    assert_eq!(out[0], out[1]);
    assert_eq!(out[1], out[2]);
}

#[test]
fn tonal_exposure_brightness_pivot_and_signed_contrast_are_separate() {
    let exposure = Operator::TonalAdjustments(TonalAdjustments {
        exposure: 1.,
        ..Default::default()
    });
    assert_eq!(exposure.apply([-1., 0., 4.], [0.; 3]), [-2., 0., 8.]);
    let brightness = Operator::TonalAdjustments(TonalAdjustments {
        brightness: 100.,
        ..Default::default()
    });
    assert!((brightness.apply([0.18; 3], [0.18; 3])[0] - 0.36).abs() < 1e-6);
    assert!(brightness.apply([10.; 3], [10.; 3])[0] < 11.);
    let c = Operator::TonalAdjustments(TonalAdjustments {
        contrast: 50.,
        ..Default::default()
    });
    assert!((c.apply([0.18; 3], [0.18; 3])[0] - 0.18).abs() < 1e-6);
    assert!(c.apply([0.09; 3], [0.09; 3])[0] < 0.09);
    assert!(c.apply([0.36; 3], [0.36; 3])[0] > 0.36);
    let mut previous = f32::NEG_INFINITY;
    for i in -1000..=1000 {
        let y = i as f32 / 100.;
        let v = c.apply([y; 3], [y; 3])[0];
        assert!(v >= previous);
        assert!((v + c.apply([-y; 3], [-y; 3])[0]).abs() < 1e-4);
        previous = v;
    }
    let neutral = Operator::TonalAdjustments(TonalAdjustments {
        pivot: 4.,
        ..Default::default()
    });
    assert!(neutral.is_neutral());
    assert_eq!(neutral.apply([-1., 0., 4.], [0.; 3]), [-1., 0., 4.]);
}

#[test]
fn tonal_zones_use_input_guide_without_division_at_zero_luminance() {
    for (field, dark) in [(0, true), (1, false), (2, true), (3, false)] {
        let mut t = TonalAdjustments::default();
        match field {
            0 => t.shadows = 100.,
            1 => t.highlights = 100.,
            2 => t.blacks = 100.,
            _ => t.whites = 100.,
        }
        let op = Operator::TonalAdjustments(t);
        let rgb = [0.1, 0.2, 0.3];
        let a = op.apply(rgb, [0.01; 3]);
        let b = op.apply(rgb, [4.; 3]);
        assert_eq!(a[0] > b[0], dark);
        assert!(((a[2] - a[0]) - (rgb[2] - rgb[0])).abs() < 1e-6);
        for p in [[0.; 3], [1., -0.2627 / 0.678, 0.], [-1.; 3]] {
            assert!(op.apply(p, p).iter().all(|v| v.is_finite()));
        }
    }
}

#[test]
fn technical_exposure_offset_then_gamma_is_signed_and_finite_at_domain_edges() {
    let op = Operator::ExposureGamma(ExposureGamma {
        exposure: 1.,
        offset: -1.,
        gamma: 2.,
        ..Default::default()
    });
    assert_eq!(op.apply([-1.5, 0.5, 2.5], [0.; 3]), [-2., 0., 2.]);
    for gamma in [0.25, 1., 4.] {
        let op = Operator::ExposureGamma(ExposureGamma {
            exposure: 10.,
            offset: 4.,
            gamma,
            ..Default::default()
        });
        let out = op.apply([-65504., 0., 65504.], [0.; 3]);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(out[0] < 0. && out[2] > 1.);
    }
}

#[test]
fn selective_nodes_require_known_versions_finite_parameters_and_valid_domains() {
    for op in operators() {
        op.validate().unwrap();
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(op, serde_json::from_str(&json).unwrap());
        let future: Operator =
            serde_json::from_str(&json.replace("\"version\":1", "\"version\":2")).unwrap();
        assert!(future.validate().is_err());
        assert!(serde_json::from_str::<Operator>(&json.replace("\"version\":1,", "")).is_err());
    }
    for value in [f32::NAN, f32::INFINITY, -101., 101.] {
        let s = SelectiveColor {
            adjustments: [[value; 4]; 9],
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }
    assert!(
        Colorize {
            amount: f32::NAN,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Colorize {
            hue: 361.,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        TonalAdjustments {
            pivot: 0.,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        TonalAdjustments {
            exposure: 11.,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        ExposureGamma {
            gamma: 0.,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        ExposureGamma {
            offset: f32::NAN,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn selective_stack_masks_alpha_roundtrip_and_prepared_evaluator_agree() {
    let mut layer = Layer::new("Selective", operators().remove(0));
    layer.operators = operators();
    layer.opacity = 0.75;
    layer.fill = 0.35;
    layer.mask.append(MaskKind::Constant(0.6), Combine::Add);
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.layer_stack().layers.push(layer);
    recipe.validate().unwrap();
    assert_eq!(
        recipe,
        serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap()
    );
    let rgb = [-0.1, 0.2, 1.5];
    let expected = recipe.layers.as_ref().unwrap().apply(rgb, [0.5; 2], 1.);
    let pixels = [0., 0.25, 0.5, 1.]
        .into_iter()
        .map(|a| [rgb[0] * a, rgb[1] * a, rgb[2] * a, a])
        .collect();
    let mut image = LinearImage::new(4, 1, pixels).unwrap();
    recipe.apply(&mut image).unwrap();
    for (p, alpha) in image.pixels.iter().zip([0., 0.25, 0.5, 1.]) {
        assert_eq!(p[3], alpha);
        for c in 0..3 {
            assert!((p[c] - expected[c] * alpha).abs() < 1e-6);
        }
    }
    let mut ops = operators();
    ops.push(Operator::TonalAdjustments(TonalAdjustments {
        exposure: 10.,
        brightness: 100.,
        contrast: 100.,
        shadows: 100.,
        highlights: 100.,
        blacks: 100.,
        whites: 100.,
        pivot: 0.01,
        version: 1,
    }));
    for op in ops {
        for n in 0..1000 {
            let t = n as f32 / 999.;
            let p = [-65504. * t, 65504. * (1. - t), 1. - 2. * t];
            assert!(
                op.apply(p, p).iter().all(|v| v.is_finite()),
                "{}",
                op.tool().id
            );
        }
    }
}
