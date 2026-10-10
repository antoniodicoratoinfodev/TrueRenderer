use super::{layers::*, *};

fn creative_operators() -> Vec<Operator> {
    let mut grading = Grading {
        balance: 18.,
        overlap: 65.,
        ..Default::default()
    };
    grading.zones[0] = GradingZone {
        hue: 220.,
        amount: 24.,
        exposure: 0.2,
    };
    grading.zones[2] = GradingZone {
        hue: 40.,
        amount: 18.,
        exposure: -0.15,
    };
    grading.zones[3] = GradingZone {
        hue: 300.,
        amount: 3.,
        exposure: 0.1,
    };
    vec![
        Operator::Grading(grading),
        Operator::BlackAndWhite(BlackAndWhite {
            amount: 60.,
            bands: [30., 0., -15., 10., 0., -40., 0., 20.],
            ..Default::default()
        }),
        Operator::ColorFilter(ColorFilter {
            density: 35.,
            ..Default::default()
        }),
        Operator::GradientMap(GradientMap {
            amount: 40.,
            stops: vec![
                GradientStop {
                    position: 0.,
                    color: [0.02, -0.01, 0.08],
                },
                GradientStop {
                    position: 0.4,
                    color: [0.5, 0.3, 0.2],
                },
                GradientStop {
                    position: 1.,
                    color: [1.2, 1., 0.7],
                },
            ],
            ..Default::default()
        }),
    ]
}

#[test]
fn grading_weights_partition_extended_luminance_and_respond_to_balance_and_overlap() {
    for balance in [-100., 0., 100.] {
        for overlap in [0., 50., 100.] {
            let g = Grading {
                balance,
                overlap,
                ..Default::default()
            };
            for y in [-10., 0., 0.0001, 0.18, 1., 65504.] {
                let w = g.weights([y; 3]);
                assert!(w.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)));
                assert!((w.iter().sum::<f32>() - 1.).abs() < 1e-6);
            }
        }
    }
    let mut g = Grading::default();
    let mid = g.weights([0.18; 3]);
    assert!(mid[1] > mid[0] && mid[1] > mid[2]);
    g.balance = 70.;
    assert!(g.weights([0.18; 3])[2] > mid[2]);
    g.balance = -70.;
    assert!(g.weights([0.18; 3])[0] > mid[0]);
    g.balance = 0.;
    g.overlap = 100.;
    assert!(g.weights([0.18; 3])[0] > mid[0]);
}

#[test]
fn grading_preserves_luminance_without_ev_and_hue_seam_is_continuous() {
    let mut g = Grading {
        balance: -35.,
        overlap: 75.,
        ..Default::default()
    };
    // Inactive hue/balance/overlap controls are still a bit-exact bypass.
    g.zones[2].hue = 300.;
    let op = Operator::Grading(g.clone());
    assert!(op.is_neutral());
    let rgb = [-0.1, 0.6, 1.8];
    assert_eq!(op.apply(rgb, [0.2; 3]), rgb);
    g.zones[0].amount = 60.;
    g.zones[2].amount = 40.;
    g.zones[3].amount = 20.;
    for p in [rgb, [0.18; 3], [-1., 0.1, 2.], [1000., -4., 12.]] {
        let out = Operator::Grading(g.clone()).apply(p, p);
        assert!((detail::luma(out) - detail::luma(p)).abs() < 1e-5 * (1. + detail::luma(p).abs()));
    }
    g.zones[0].hue = 0.;
    let left = Operator::Grading(g.clone()).apply(rgb, rgb);
    g.zones[0].hue = 360.;
    assert_eq!(left, Operator::Grading(g.clone()).apply(rgb, rgb));
    g.zones[0].exposure = 0.5;
    g.zones[3].exposure = -0.2;
    let out = Operator::Grading(g.clone()).apply(rgb, rgb);
    let expected = detail::luma(rgb) * (g.weights(rgb)[0] * 0.5 - 0.2).exp2();
    assert!((detail::luma(out) - expected).abs() < 1e-5);
}

#[test]
fn black_white_families_use_the_input_guide_and_protect_neutrals() {
    let mut b = BlackAndWhite {
        amount: 100.,
        ..Default::default()
    };
    let red = [0.8, 0.1, 0.1];
    let blue = [0.1, 0.1, 0.8];
    let base_red = Operator::BlackAndWhite(b.clone()).apply(red, red);
    let base_blue = Operator::BlackAndWhite(b.clone()).apply(blue, blue);
    b.bands[0] = 100.;
    let op = Operator::BlackAndWhite(b.clone());
    let brighter = op.apply(red, red);
    assert!(brighter[0] > base_red[0]);
    assert_eq!(brighter[0], brighter[1]);
    assert_eq!(brighter[1], brighter[2]);
    assert_eq!(op.apply(blue, blue), base_blue);
    assert!(op.apply([0.5; 3], red)[0] > op.apply([0.5; 3], blue)[0]);
    b.bands = [100.; 8];
    let op = Operator::BlackAndWhite(b);
    for gray in [-1., 0., 0.18, 4.] {
        assert!((op.apply([gray; 3], [gray; 3])[0] - gray).abs() < 1e-6);
    }
    let left = op.apply([0.8, 0.1, 0.100001], [0.8, 0.1, 0.100001]);
    let right = op.apply([0.8, 0.100001, 0.1], [0.8, 0.100001, 0.1]);
    assert!((left[0] - right[0]).abs() < 1e-5);
}

#[test]
fn color_filter_preserves_y_without_dividing_by_dark_or_negative_luminance() {
    let f = ColorFilter {
        hue: 230.,
        density: 100.,
        ..Default::default()
    };
    let op = Operator::ColorFilter(f.clone());
    for p in [
        [0.; 3],
        [0.18; 3],
        [-2., 0.01, 0.03],
        [4., 0.1, -1.],
        [0.1, -0.03875, 0.],
    ] {
        let out = op.apply(p, p);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!((detail::luma(out) - detail::luma(p)).abs() < 1e-6);
    }
    let rgb = [1., 2., 3.];
    let raw = Operator::ColorFilter(ColorFilter {
        preserve_luminance: false,
        ..f.clone()
    })
    .apply(rgb, rgb);
    assert!(detail::luma(raw) < detail::luma(rgb));
    assert_eq!(
        Operator::ColorFilter(ColorFilter {
            saturation: 0.,
            ..f
        })
        .apply(rgb, rgb),
        rgb
    );
}

#[test]
fn gradient_interpolation_endpoints_continuation_and_reverse_keep_extended_values() {
    let mut g = GradientMap {
        amount: 100.,
        ..Default::default()
    };
    g.stops.insert(
        1,
        GradientStop {
            position: 0.5,
            color: [0.25, 0.5, 0.75],
        },
    );
    g.validate().unwrap();
    assert_eq!(g.color_at(0.25), [0.125, 0.25, 0.375]);
    assert_eq!(g.color_at(-1.), [-0.5, -1., -1.5]);
    assert_eq!(g.color_at(2.), [2.5, 2., 1.5]);
    let above = Operator::GradientMap(g.clone()).apply([2.; 3], [2.; 3]);
    assert!(above.into_iter().all(|v| v > 1.));
    for t in [0., 0.25, 0.5, 1.] {
        let a = Operator::GradientMap(g.clone()).apply([1. - t; 3], [0.; 3]);
        g.reverse = true;
        let b = Operator::GradientMap(g.clone()).apply([t; 3], [0.; 3]);
        assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6));
        g.reverse = false;
    }
}

#[test]
fn creative_nodes_reject_unknown_versions_nonfinite_and_invalid_palettes() {
    for op in creative_operators() {
        op.validate().unwrap();
        let json = serde_json::to_string(&op).unwrap();
        let future: Operator =
            serde_json::from_str(&json.replace("\"version\":1", "\"version\":2")).unwrap();
        assert!(future.validate().is_err());
        assert_eq!(op, serde_json::from_str(&json).unwrap());
    }
    let mut g = GradientMap::default();
    g.stops.clear();
    assert!(g.validate().is_err());
    let mut g = GradientMap::default();
    g.stops[1].position = f32::NAN;
    assert!(g.validate().is_err());
    let mut g = GradientMap::default();
    g.stops.insert(1, g.stops[0]);
    assert!(g.validate().is_err());
    let mut g = GradientMap::default();
    g.stops[1].color[0] = f32::INFINITY;
    assert!(g.validate().is_err());
    let g = GradientMap {
        input: [1., 1.],
        ..Default::default()
    };
    assert!(g.validate().is_err());
    let g = GradientMap {
        stops: (0..17)
            .map(|i| GradientStop {
                position: i as f32 / 16.,
                color: [0.; 3],
            })
            .collect(),
        ..Default::default()
    };
    assert!(g.validate().is_err());
    let mut g = Grading::default();
    g.zones[3].exposure = f32::NAN;
    assert!(g.validate().is_err());
}

#[test]
fn creative_stack_mask_alpha_roundtrip_and_prepared_evaluator_agree() {
    let mut layer = Layer::new("Viraggio", creative_operators().remove(0));
    layer.operators = creative_operators();
    layer.opacity = 0.75;
    layer.fill = 0.35;
    layer.mask.append(MaskKind::Constant(0.6), Combine::Add);
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.layer_stack().layers.push(layer);
    recipe.validate().unwrap();
    let saved = serde_json::to_vec(&recipe).unwrap();
    assert_eq!(recipe, serde_json::from_slice(&saved).unwrap());
    let rgb = [-0.1, 0.2, 1.5];
    let expected = recipe.layers.as_ref().unwrap().apply(rgb, [0.5; 2], 1.);
    let pixels: Vec<_> = [0., 0.25, 0.5, 1.]
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
    for op in creative_operators() {
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
