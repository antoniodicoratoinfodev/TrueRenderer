use super::{layers::*, *};

fn record(action: LevelsAction, size: [u32; 2]) -> LevelAnalysis {
    LevelAnalysis {
        action,
        channel: 0,
        source_digest: "a".repeat(64),
        input_recipe_hash: "b".repeat(64),
        geometry: Default::default(),
        image_size: size,
        valid_samples: 1,
        considered_samples: 1,
        sample_xy: None,
        sample_rgb: None,
    }
}
fn sample(action: LevelsAction, channel: usize, rgb: [f32; 3]) -> LevelAnalysis {
    LevelAnalysis {
        channel,
        sample_xy: Some([4, 3]),
        sample_rgb: Some(rgb),
        ..record(action, [32, 16])
    }
}
fn ramp() -> LinearImage {
    LinearImage::new(
        100,
        1,
        (0..100)
            .map(|i| {
                let x = i as f32 / 100.;
                [0.1 + 0.6 * x, 0.2 + 0.5 * x, 0.3 + 0.4 * x, 1.]
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn parametric_zones_are_monotone_c1_and_keep_extended_endpoints() {
    for boundaries in [[0.25, 0.5, 0.75], [0.02, 0.04, 0.98], [0.4, 0.8, 0.95]] {
        for amounts in [[100.; 4], [-100.; 4], [100., -100., 100., -100.]] {
            let c = ParametricCurve {
                boundaries,
                amounts,
                ..Default::default()
            };
            c.validate().unwrap();
            let mut prev = -1.;
            for i in 0..=10000 {
                let x = i as f32 / 10000.;
                let v = c.value(x);
                assert!(v >= prev && (0. ..=1.).contains(&v));
                prev = v;
            }
            for edge in [0., boundaries[0], boundaries[1], boundaries[2], 1.] {
                assert_eq!(c.value(edge), edge);
                let left = (c.value(edge) - c.value(edge - 1e-5)) / 1e-5;
                let right = (c.value(edge + 1e-5) - c.value(edge)) / 1e-5;
                assert!((left - right).abs() < 0.02, "{edge}: {left} {right}");
            }
            for x in [-65504., -1., 2., 65504.] {
                assert_eq!(c.value(x), x);
            }
        }
    }
    for zone in 0..4 {
        let mut c = ParametricCurve::default();
        c.amounts[zone] = 100.;
        for i in 0..4 {
            let x = (i as f32 + 0.5) / 4.;
            assert_eq!(c.value(x) > x, i == zone);
        }
    }
}

#[test]
fn luminance_and_rgb_curves_have_distinct_chroma_behavior_and_exact_neutrality() {
    let c = LuminanceCurve {
        points: vec![
            CurvePoint { x: 0., y: 0.1 },
            CurvePoint { x: 0.4, y: 0.6 },
            CurvePoint { x: 1., y: 0.9 },
        ],
        ..Default::default()
    };
    c.validate().unwrap();
    let rgb = [-0.1, 0.3, 1.2];
    let out = Operator::LuminanceCurve(c.clone()).apply(rgb, rgb);
    assert!((detail::luma(out) - tone_curve_value(detail::luma(rgb), &c.points)).abs() < 1e-6);
    for i in 1..3 {
        assert!(((out[i] - out[0]) - (rgb[i] - rgb[0])).abs() < 1e-6);
    }
    let mut p = ParametricCurve {
        amounts: [50., -20., 40., -10.],
        ..Default::default()
    };
    let luma = Operator::ParametricCurve(p.clone()).apply(rgb, rgb);
    assert!(((luma[1] - luma[0]) - (rgb[1] - rgb[0])).abs() < 1e-6);
    p.space = CurveSpace::Rgb;
    let channels = Operator::ParametricCurve(p).apply(rgb, rgb);
    assert_eq!(channels[0], rgb[0]);
    assert_eq!(channels[2], rgb[2]);
    assert_ne!(channels, luma);
    let p = ParametricCurve {
        boundaries: [0.1, 0.2, 0.9],
        ..Default::default()
    };
    assert_eq!(Operator::ParametricCurve(p).apply(rgb, rgb), rgb);
    let c = LuminanceCurve {
        points: vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 0.2, y: 0.2 },
            CurvePoint { x: 1., y: 1. },
        ],
        ..Default::default()
    };
    assert_eq!(Operator::LuminanceCurve(c).apply(rgb, rgb), rgb);
}

#[test]
fn auto_levels_freezes_robust_endpoints_and_keeps_other_parameters() {
    let mut image = ramp();
    image.pixels[0] = [-10., -10., -10., 0.];
    image.pixels[99] = [0.5, 0.5, 0.5, 0.5];
    let mut g = LevelAdjustments::default();
    g.channels[0].gamma = 1.2;
    g.channels[0].output = [-0.02, 1.1];
    let stats = LevelStatistics::from_image(&image, g.channels[0]).unwrap();
    assert_eq!((stats.valid, stats.considered), (98, 100));
    assert_eq!(stats.histogram[0].iter().sum::<u32>(), 98);
    let before = g.clone();
    g.auto(&stats, record(LevelsAction::AutoComposite, [100, 1]))
        .unwrap();
    assert_eq!(g.channels[0].black, stats.bounds[0][0]);
    assert_eq!(g.channels[0].white, stats.bounds[0][1]);
    assert_eq!(g.channels[0].gamma, before.channels[0].gamma);
    assert_eq!(g.channels[0].output, before.channels[0].output);
    assert_eq!(g.channels[1..], before.channels[1..]);
    let mut separate = before.clone();
    separate
        .auto(&stats, record(LevelsAction::AutoChannels, [100, 1]))
        .unwrap();
    assert_eq!(separate.channels[0], before.channels[0]);
    assert_ne!(separate.channels[1].black, separate.channels[2].black);
    assert_eq!(separate.channels[2].white, stats.bounds[2][1]);
    let json = serde_json::to_string(&separate).unwrap();
    assert_eq!(separate, serde_json::from_str(&json).unwrap());
    let analysis = separate.analysis.as_ref().unwrap();
    assert_eq!(analysis.valid_samples, 98);
    assert_eq!(analysis.source_digest, "a".repeat(64));
}

#[test]
fn auto_levels_limits_work_and_rejects_unusable_or_stale_data_atomically() {
    let image = LinearImage::new(
        40000,
        1,
        (0..40000)
            .map(|i| {
                let v = i as f32 / 40000.;
                [v, v, v, 1.]
            })
            .collect(),
    )
    .unwrap();
    let stats = LevelStatistics::from_image(&image, TonalLevel::default()).unwrap();
    assert_eq!(stats.considered, MAX_LEVEL_SAMPLES);
    let flat = LinearImage::new(64, 1, vec![[0.5; 4]; 64]).unwrap();
    assert!(LevelStatistics::from_image(&flat, TonalLevel::default()).is_err());
    let flat = LinearImage::new(64, 1, vec![[0.5, 0.5, 0.5, 1.]; 64]).unwrap();
    let stats = LevelStatistics::from_image(&flat, TonalLevel::default()).unwrap();
    let mut g = LevelAdjustments::default();
    let before = g.clone();
    assert!(
        g.auto(&stats, record(LevelsAction::AutoComposite, [64, 1]))
            .is_err()
    );
    assert_eq!(g, before);
    let stats = LevelStatistics::from_image(&ramp(), TonalLevel::default()).unwrap();
    g.channels[0].gamma = 2.;
    let before = g.clone();
    assert!(
        g.auto(&stats, record(LevelsAction::AutoChannels, [100, 1]))
            .is_err()
    );
    assert_eq!(g, before);
    g = LevelAdjustments::default();
    assert!(
        g.auto(&stats, record(LevelsAction::AutoChannels, [200, 1]))
            .is_err()
    );
}

#[test]
fn levels_pickers_target_selected_input_and_neutralize_without_erasing_endpoints() {
    let mut g = LevelAdjustments::default();
    g.channels[0].gamma = 1.1;
    let rgb = [0.2, 0.3, 0.4];
    let old = Operator::TonalLevels(Box::new(g.clone())).apply(rgb, rgb);
    let y = detail::luma(old);
    g.pick(sample(LevelsAction::Gray, 0, rgb)).unwrap();
    let out = Operator::TonalLevels(Box::new(g.clone())).apply(rgb, rgb);
    for v in out {
        assert!((v - y).abs() < 1e-6);
    }
    assert_eq!(g.channels[0].gamma, 1.1);
    let before = g.clone();
    g.pick(sample(LevelsAction::Black, 2, [0.05, 0.08, 0.1]))
        .unwrap();
    assert!((g.channels[2].black - 0.08_f32.powf(1. / 1.1)).abs() < 1e-6);
    assert_eq!(g.channels[1], before.channels[1]);
    assert_eq!(g.channels[3], before.channels[3]);
    let before = g.clone();
    assert!(g.pick(sample(LevelsAction::White, 2, [0.; 3])).is_err());
    assert_eq!(g, before);
    assert!(
        g.pick(sample(LevelsAction::Gray, 0, [-1., 0.2, 0.3]))
            .is_err()
    );
    assert_eq!(g, before);
    g.pick(sample(LevelsAction::White, 0, [0.8; 3])).unwrap();
    assert!((g.channels[0].white - 0.8).abs() < 1e-6);
    assert_eq!(g.channels[1..], before.channels[1..]);
}

#[test]
fn curves_levels_reject_unknown_versions_invalid_points_boundaries_and_provenance() {
    for op in [
        Operator::TonalLevels(Box::default()),
        Operator::LuminanceCurve(Default::default()),
        Operator::ParametricCurve(Default::default()),
    ] {
        let json = serde_json::to_string(&op).unwrap();
        let future: Operator =
            serde_json::from_str(&json.replace("\"version\":1", "\"version\":99")).unwrap();
        assert!(future.is_neutral());
        assert!(future.validate().is_err());
        assert!(serde_json::from_str::<Operator>(&json.replace("\"version\":1,", "")).is_err());
    }
    for boundaries in [[0.4, 0.3, 0.7], [0.1, 0.11, 0.8], [f32::NAN, 0.5, 0.7]] {
        assert!(
            ParametricCurve {
                boundaries,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        LuminanceCurve {
            points: vec![CurvePoint { x: 0., y: 0.8 }, CurvePoint { x: 1., y: 0.2 }],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    let mut r = sample(LevelsAction::Gray, 0, [0.2; 3]);
    r.source_digest = "invalid".into();
    assert!(r.validate().is_err());
    let mut r = sample(LevelsAction::Gray, 0, [0.2; 3]);
    r.sample_xy = Some([32, 0]);
    assert!(r.validate().is_err());
    let mut r = sample(LevelsAction::Gray, 0, [0.2; 3]);
    r.sample_rgb = Some([f32::NAN; 3]);
    assert!(r.validate().is_err());
}

#[test]
fn historical_levels_and_new_wrapper_render_exactly_with_alpha_masks_and_curves() {
    let mut levels = [TonalLevel::default(); 4];
    levels[0].gamma = 1.1;
    levels[2].black = -0.01;
    let old = Operator::Levels(levels);
    let new = Operator::TonalLevels(Box::new(LevelAdjustments {
        channels: levels,
        ..Default::default()
    }));
    for rgb in [[-0.1, 0.2, 1.5], [0.; 3], [65504., -30., 1.]] {
        assert_eq!(old.apply(rgb, rgb), new.apply(rgb, rgb));
    }
    let mut recipe = EditRecipe::neutral(Default::default());
    let mut layer = Layer::new("Curves", new);
    layer.operators.extend([
        Operator::LuminanceCurve(LuminanceCurve {
            points: vec![
                CurvePoint { x: 0., y: 0.02 },
                CurvePoint { x: 0.4, y: 0.5 },
                CurvePoint { x: 1., y: 0.98 },
            ],
            ..Default::default()
        }),
        Operator::ParametricCurve(ParametricCurve {
            amounts: [10., -20., 30., -40.],
            ..Default::default()
        }),
    ]);
    layer.opacity = 0.7;
    layer.mask.append(MaskKind::Constant(0.6), Combine::Add);
    recipe.layer_stack().layers.push(layer);
    recipe.validate().unwrap();
    assert_eq!(
        recipe,
        serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap()
    );
    let rgb = [-0.1, 0.2, 1.5];
    let expected = recipe.layers.as_ref().unwrap().apply(rgb, [0.5; 2], 1.);
    let mut image = LinearImage::new(
        4,
        1,
        [0., 0.25, 0.5, 1.]
            .into_iter()
            .map(|a| [rgb[0] * a, rgb[1] * a, rgb[2] * a, a])
            .collect(),
    )
    .unwrap();
    recipe.apply(&mut image).unwrap();
    for (p, a) in image.pixels.iter().zip([0., 0.25, 0.5, 1.]) {
        assert_eq!(p[3], a);
        for c in 0..3 {
            assert!((p[c] - expected[c] * a).abs() < 1e-6);
        }
    }
}
