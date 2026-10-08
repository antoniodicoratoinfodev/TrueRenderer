use super::{layers::*, *};
use uuid::Uuid;

fn image() -> LinearImage {
    LinearImage::new(
        40,
        24,
        (0..960)
            .map(|i| {
                let a = [0., 0.125, 0.5, 1.][i % 4];
                let rgb = [
                    -0.1 + (i % 40) as f32 / 20.,
                    0.03 + (i / 40) as f32 / 30.,
                    0.7,
                ];
                [rgb[0] * a, rgb[1] * a, rgb[2] * a, a]
            })
            .collect(),
    )
    .unwrap()
}
fn light(ev: f32) -> Operator {
    Operator::Light {
        exposure: ev,
        temperature: 0.,
        tint: 0.,
        saturation: 0.,
    }
}
#[test]
fn promotion_preserves_all_historical_pixels_including_crop_and_partial_alpha() {
    for version in 1..=4 {
        let mut old = EditRecipe::neutral(Default::default());
        old.process_version = version;
        old.exposure_ev = 0.7;
        old.contrast = 12.;
        old.temperature = -15.;
        old.curve = vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 0.4, y: 0.55 },
            CurvePoint { x: 1., y: 1. },
        ];
        if version >= 2 {
            old.vibrance = 25.;
        }
        if version >= 3 {
            let mut a = Advanced::default();
            a.geometry.crop = [0.1, 0.2, 0.9, 0.8];
            a.geometry.quarter_turns = 1;
            a.detail.texture = 15.;
            a.color.bands[0].hue = 15.;
            a.masks.push(masks::Mask {
                exposure: 0.5,
                ..Default::default()
            });
            old.advanced = Some(Box::new(a));
        }
        if version == 4 {
            old.curve[0].y = 0.1;
        }
        let historical = serde_json::to_vec(&old).unwrap();
        assert!(
            !String::from_utf8(historical.clone())
                .unwrap()
                .contains("layers")
        );
        let mut upgraded = old.clone();
        upgraded
            .layer_stack()
            .layers
            .push(Layer::new("neutral", light(0.)));
        assert_eq!(upgraded.base_process(), version);
        let mut a = image();
        let mut b = a.clone();
        old.apply(&mut a).unwrap();
        upgraded.apply(&mut b).unwrap();
        assert_eq!((a.width, a.height), (b.width, b.height));
        assert_eq!(a.pixels, b.pixels, "historical version {version}");
        assert_eq!(serde_json::to_vec(&old).unwrap(), historical);
        upgraded.require_process(4);
        assert_eq!(upgraded.base_process(), 4);
        assert_eq!(upgraded.process_version, 5);
    }
}
#[test]
fn layer_order_opacity_coverage_and_alpha_have_independent_meanings() {
    let mut recipe = EditRecipe::neutral(Default::default());
    let mut first = Layer::new("exposure", light(1.));
    first.opacity = 0.5;
    first.mask.append(MaskKind::Constant(0.5), Combine::Add);
    let offset = Operator::Channels {
        matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        offset: [0.1; 3],
    };
    recipe.layer_stack().layers = vec![first, Layer::new("offset", offset)];
    let original = LinearImage::new(2, 1, vec![[0.1, 0.2, 0.3, 0.5], [0.; 4]]).unwrap();
    let mut out = original.clone();
    recipe.apply(&mut out).unwrap();
    for (a, b) in out.pixels[0].iter().zip([0.175, 0.3, 0.425, 0.5]) {
        assert!((a - b).abs() < 1e-6);
    }
    assert_eq!(out.pixels[1], [0.; 4]);
    recipe.layers.as_mut().unwrap().layers.swap(0, 1);
    let mut reversed = original;
    recipe.apply(&mut reversed).unwrap();
    assert_ne!(out.pixels, reversed.pixels);
    assert_eq!(reversed.pixels[0][3], 0.5);
}
#[test]
fn masks_follow_source_and_geometry_runs_exactly_once() {
    let mut r = EditRecipe::neutral(Default::default());
    let mut l = Layer::new("radial", light(1.));
    l.mask.append(
        MaskKind::Shape(masks::Mask {
            center: [0.25, 0.5],
            radius: 0.2,
            ..Default::default()
        }),
        Combine::Add,
    );
    r.layer_stack().layers.push(l);
    let src = LinearImage::new(100, 60, vec![[0.2, 0.3, 0.4, 1.]; 6000]).unwrap();
    let mut expected = src.clone();
    r.apply(&mut expected).unwrap();
    let mut a = Advanced::default();
    a.geometry.crop = [0.1, 0.1, 0.8, 0.9];
    a.geometry.quarter_turns = 1;
    a.geometry.apply(&mut expected, [100, 60], 0).unwrap();
    r.require_process(3);
    r.advanced = Some(Box::new(a));
    let mut got = src;
    r.apply(&mut got).unwrap();
    assert_eq!((got.width, got.height), (expected.width, expected.height));
    assert_eq!(got.pixels, expected.pixels);
}
#[test]
fn mask_algebra_and_prepared_brush_match_reference_and_do_not_bridge_strokes() {
    let mut g = MaskGraph::default();
    g.append(MaskKind::Constant(0.5), Combine::Add);
    g.append(MaskKind::Constant(0.5), Combine::Add);
    assert_eq!(g.weight([0.; 2], [0.; 3], 1.), 0.75);
    g.append(MaskKind::Constant(0.5), Combine::Subtract);
    assert_eq!(g.weight([0.; 2], [0.; 3], 1.), 0.375);
    g.append(MaskKind::Constant(0.5), Combine::Intersect);
    assert_eq!(g.weight([0.; 2], [0.; 3], 1.), 0.1875);
    g.invert();
    g.invert();
    assert_eq!(g.weight([0.; 2], [0.; 3], 1.), 0.1875);
    let mut layer = Layer::new("brush", light(1.));
    layer.mask.append(
        MaskKind::Shape(masks::Mask {
            shape: masks::Shape::Brush,
            points: vec![[0.1, 0.1], [0.2, 0.1], [0.8, 0.1], [0.9, 0.1]],
            breaks: vec![2],
            radius: 0.05,
            ..Default::default()
        }),
        Combine::Add,
    );
    layer.mask.append(
        MaskKind::Shape(masks::Mask {
            center: [0.15, 0.1],
            radius: 0.02,
            ..Default::default()
        }),
        Combine::Subtract,
    );
    assert_eq!(layer.mask.weight([0.5, 0.1], [0.2; 3], 1.), 0.);
    let stack = LayerStack {
        base_process: 1,
        layers: vec![layer],
    };
    stack.validate().unwrap();
    let prepared = stack.prepare(1.);
    for i in 0..1000 {
        let xy = [i as f32 / 1000., 0.12];
        let rgb = [0.2, 0.3, 0.6];
        let a = stack.apply(rgb, xy, 1.);
        let b = prepared[0].apply(rgb, xy);
        assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
#[test]
fn cyclic_missing_future_and_oversized_recipes_are_rejected() {
    let mut r = EditRecipe::neutral(Default::default());
    let mut l = Layer::new("mask", light(1.));
    let id = Uuid::new_v4();
    l.mask.nodes.push(MaskNode {
        id,
        kind: MaskKind::Invert(id),
    });
    l.mask.root = Some(id);
    r.layer_stack().layers.push(l);
    assert!(r.validate().is_err());
    r.layers.as_mut().unwrap().layers[0].mask = MaskGraph::default();
    r.validate().unwrap();
    let copy = r.layers.as_ref().unwrap().layers[0].clone();
    r.layers.as_mut().unwrap().layers.push(copy);
    assert!(r.validate().is_err());
    r.layers.as_mut().unwrap().layers.pop();
    r.process_version = 6;
    assert!(r.validate().is_err());
    r.process_version = 5;
    r.schema_version = 1;
    assert!(r.validate().is_err());
}
#[test]
fn color_selection_is_circular_continuous_and_neutrals_have_no_arbitrary_hue() {
    let c = ColorRange {
        reference: [0.8, 0.2, 0.2],
        width: [30., 1., 1.],
        softness: 0.5,
    };
    let left = c.weight([0.8, 0.2, 0.201]);
    let right = c.weight([0.8, 0.201, 0.2]);
    assert!((left - right).abs() < 1e-5 && left > 0.99);
    assert_eq!(c.weight([0.4; 3]), 0.);
    let grey = ColorRange {
        reference: [0.4; 3],
        ..c.clone()
    };
    assert_eq!(grey.weight([0.4; 3]), 1.);
    let mut s = SampleColor {
        range: c,
        correction: [20., 10., 5.],
        uniformity: [50., 50., 0.],
    };
    for v in [-1., 0., 0.2, 1., 4.] {
        let neutral = [v; 3];
        assert_eq!(
            Operator::SampleColor(s.clone()).apply(neutral, neutral),
            neutral
        );
    }
    s.correction = [0.; 3];
    s.uniformity = [0.; 3];
    assert_eq!(
        Operator::SampleColor(s).apply([-0.1, 0.2, 2.], [-0.1, 0.2, 2.]),
        [-0.1, 0.2, 2.]
    );
}
#[test]
fn uniformity_reduces_chroma_spread_and_keeps_luminance_texture_when_unselected() {
    let reference = [0.2, 0.3, 0.7];
    let op = Operator::SampleColor(SampleColor {
        range: ColorRange {
            reference,
            width: [80., 1., 1.],
            softness: 0.2,
        },
        uniformity: [100., 100., 0.],
        ..Default::default()
    });
    for rgb in [[0.2, 0.35, 0.8], [0.1, 0.25, 0.6]] {
        let result = op.apply(rgb, rgb);
        assert!((color::hue_chroma(result).1 - color::hue_chroma(reference).1).abs() < 1e-5);
        assert!((detail::luma(result) - detail::luma(rgb)).abs() < 1e-6);
    }
}
#[test]
fn every_registered_neutral_is_exact_and_extended_inputs_remain_finite() {
    for tool in TOOLS {
        let op = tool.operator();
        op.validate().unwrap();
        for rgb in [[-1., 0., 4.], [0.; 3], [0.18; 3], [1000., -100., 2.]] {
            assert_eq!(op.apply(rgb, rgb), rgb);
        }
    }
    let mut levels = [TonalLevel::default(); 4];
    levels[0].gamma = 0.5;
    let op = Operator::Levels(levels);
    op.validate().unwrap();
    assert_eq!(op.apply([-0.5, 0.5, 2.], [0.; 3]), [-0.25, 0.25, 4.]);
}
#[test]
fn duplicate_and_removal_preserve_independence_and_roundtrip() {
    let mut l = Layer::new("named", light(1.));
    let a = l.mask.append(MaskKind::Constant(0.5), Combine::Add);
    l.mask.append(MaskKind::Constant(0.5), Combine::Intersect);
    let mut copy = l.duplicate();
    assert_ne!(copy.id, l.id);
    assert_ne!(copy.mask.root, l.mask.root);
    assert_eq!(copy.mask.weight([0.; 2], [0.; 3], 1.), 0.25);
    l.mask.remove(a);
    l.mask.validate().unwrap();
    assert_eq!(l.mask.weight([0.; 2], [0.; 3], 1.), 0.5);
    copy.name = "renamed".into();
    let stack = LayerStack {
        base_process: 1,
        layers: vec![l, copy],
    };
    stack.validate().unwrap();
    let json = serde_json::to_vec(&stack).unwrap();
    assert_eq!(stack, serde_json::from_slice(&json).unwrap());
}
