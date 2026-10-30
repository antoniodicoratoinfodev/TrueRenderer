use super::*;
use retouch::{Kind, Retouch, Selection, Stroke};
fn source() -> LinearImage {
    LinearImage::new(
        96,
        64,
        (0..96 * 64)
            .map(|i| {
                [
                    0.1 + (i % 96) as f32 / 200.,
                    0.1 + (i / 96) as f32 / 160.,
                    0.2,
                    1.,
                ]
            })
            .collect(),
    )
    .unwrap()
}
fn stroke(kind: Kind) -> Stroke {
    Stroke {
        enabled: true,
        kind,
        points: vec![[0.4, 0.5], [0.5, 0.5]],
        radius: 0.04,
        hardness: 0.8,
        opacity: 0.6,
        color: [1., 0.1, 0., 0.75],
        offset: [0.2, 0.],
        selection: vec![],
    }
}
#[test]
fn reversible_strokes_preserve_input_replay_current_development_and_roundtrip() {
    let original = source();
    for kind in [
        Kind::Paint,
        Kind::Erase,
        Kind::Clone,
        Kind::Heal,
        Kind::AutoHeal,
    ] {
        let mut recipe = EditRecipe::neutral(Default::default());
        recipe.retouch_mut().strokes.push(stroke(kind));
        recipe.exposure_ev = 0.4;
        recipe.validate().unwrap();
        let mut first = original.clone();
        recipe.apply(&mut first).unwrap();
        assert_ne!(first.pixels, original.pixels, "{kind:?}");
        let json = serde_json::to_vec(&recipe).unwrap();
        assert!(!String::from_utf8_lossy(&json).contains("pixels"));
        let reopened: EditRecipe = serde_json::from_slice(&json).unwrap();
        assert_eq!(reopened, recipe);
        let mut second = original.clone();
        reopened.apply(&mut second).unwrap();
        assert_eq!(first.pixels, second.pixels);
        let mut different = original.clone();
        for p in &mut different.pixels {
            p[1] *= 0.8;
        }
        reopened.apply(&mut different).unwrap();
        assert_ne!(first.pixels, different.pixels);
        recipe.exposure_ev = 0.;
        recipe.retouch_mut().enabled = false;
        assert!(recipe.is_neutral());
        let mut disabled = original.clone();
        recipe.apply(&mut disabled).unwrap();
        assert_eq!(disabled.pixels, original.pixels);
    }
    assert_eq!(original.pixels, source().pixels);
}
#[test]
fn clone_freezes_donor_and_erase_preserves_premultiplication() {
    let mut image = source();
    let original = image.clone();
    let mut s = stroke(Kind::Clone);
    s.opacity = 1.;
    s.hardness = 1.;
    s.offset = [-20. / 96., 0.];
    let r = Retouch {
        strokes: vec![s],
        ..Default::default()
    };
    r.apply_strokes(&mut image, &|| false).unwrap();
    let i = 32 * 96 + 48;
    assert_eq!(image.pixels[i], original.pixels[32 * 96 + 28]);
    let mut overlapping = r.clone();
    overlapping.strokes[0].points = vec![[0.3, 0.5], [0.7, 0.5]];
    overlapping.strokes[0].offset = [-10. / 96., 0.];
    let mut image = original.clone();
    overlapping.apply_strokes(&mut image, &|| false).unwrap();
    // The donor has already been painted by earlier dabs of this stroke.
    assert_eq!(image.pixels[32 * 96 + 67], original.pixels[32 * 96 + 57]);
    let mut r = Retouch {
        strokes: vec![stroke(Kind::Erase)],
        ..Default::default()
    };
    r.strokes[0].points = vec![[0.5, 0.5]];
    r.strokes[0].opacity = 0.5;
    r.strokes[0].hardness = 1.;
    let mut image = original.clone();
    r.apply_strokes(&mut image, &|| false).unwrap();
    assert_eq!(image.pixels[i], original.pixels[i].map(|v| v * 0.5));
}
#[test]
fn retouch_geometry_and_regional_view_match_canonical_full_frame() {
    let original = source();
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.retouch_mut().strokes.push(stroke(Kind::Paint));
    recipe.retouch_mut().transform = retouch::Transform {
        offset: [0.07, -0.04],
        scale: 0.9,
        angle: 7.,
    };
    recipe.require_process(3);
    let mut advanced = Advanced::default();
    advanced.geometry.crop = [0.1, 0.1, 0.9, 0.9];
    advanced.geometry.quarter_turns = 1;
    recipe.advanced = Some(Box::new(advanced));
    recipe
        .layer_stack()
        .layers
        .push(layers::Layer::new("curve", layers::TOOLS[3].operator()));
    recipe.validate().unwrap();
    assert_eq!(recipe.schema_version, 3);
    let mut full = original.clone();
    let native = recipe.apply_preview(&mut full, [96, 64], 0).unwrap();
    let opaque = full.pixels.iter().all(|p| p[3] == 1.);
    let expected =
        crate::provider::ImageLevels::from_reference_mip(full, native, 0, opaque).unwrap();
    let input =
        crate::provider::ImageLevels::from_source(original, crate::preview::PreviewRequest::full())
            .unwrap();
    let mut processor = regional::Processor::new(crate::budget::MemoryBudget::new(0));
    for region in [
        crate::resample::Region::fitted(native, [24, 32]),
        crate::resample::Region {
            size: [20, 17],
            origin: [13.25, 15.5],
            step: [0.75, 1.],
        },
    ] {
        let result = processor.render(&input, &recipe, [1.; 3], region).unwrap();
        assert_eq!(result.pixels, expected.render(region).unwrap().pixels);
    }
    let t = recipe.retouch.as_ref().unwrap().transform;
    let uv = [0.4, 0.3];
    let p = t.forward(uv, 64. / 96.);
    let back = t.inverse(p, 64. / 96.);
    for a in 0..2 {
        assert!((uv[a] - back[a]).abs() < 1e-6);
    }
}
#[test]
fn retouch_validation_rejects_unknown_version_nonfinite_and_unbounded_work() {
    let mut r = EditRecipe::neutral(Default::default());
    assert!(!serde_json::to_string(&r).unwrap().contains("retouch"));
    r.retouch_mut().strokes.push(stroke(Kind::Clone));
    r.validate().unwrap();
    let good = r.clone();
    r.retouch_mut().version = 2;
    assert!(r.validate().is_err());
    r = good.clone();
    r.retouch_mut().strokes[0].offset[0] = f32::NAN;
    assert!(r.validate().is_err());
    r = good.clone();
    r.schema_version = 2;
    assert!(r.validate().is_err());
    r = good;
    r.retouch_mut().strokes[0].points = vec![[0.4, 0.4]; retouch::MAX_POINTS + 1];
    assert!(r.validate().is_err());
}
#[test]
fn vector_selection_restricts_stroke_and_survives_recipe_serialization() {
    let original = source();
    let mut r = EditRecipe::neutral(Default::default());
    let mut s = stroke(Kind::Erase);
    s.points = vec![[0.5, 0.5]];
    s.radius = 0.15;
    s.opacity = 1.;
    s.hardness = 1.;
    s.selection = vec![Selection {
        points: vec![[0.5, 0.], [1., 0.], [1., 1.], [0.5, 1.]],
        mode: crate::document::SelectionMode::Replace,
        feather: 0.,
    }];
    r.retouch_mut().strokes.push(s);
    let mut image = original.clone();
    r.apply(&mut image).unwrap();
    assert_eq!(image.pixels[32 * 96 + 47], original.pixels[32 * 96 + 47]);
    assert_eq!(image.pixels[32 * 96 + 49], [0.; 4]);
    let bytes = serde_json::to_vec(&r).unwrap();
    let back: EditRecipe = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(r, back);
}

#[test]
fn base_visibility_roundtrips_without_discarding_photo_or_adjustments() {
    let original = source();
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.exposure_ev = 1.;
    let mut visible = original.clone();
    recipe.apply(&mut visible).unwrap();
    recipe.retouch_mut().base_hidden = true;
    // The visibility belongs to the base, independent of the retouch toggle.
    recipe.retouch_mut().enabled = false;
    assert!(!recipe.is_neutral());
    assert!(recipe.has_retouch());
    let encoded = serde_json::to_string(&recipe).unwrap();
    let mut loaded: EditRecipe = serde_json::from_str(&encoded).unwrap();
    let mut hidden = original.clone();
    loaded.apply(&mut hidden).unwrap();
    assert!(hidden.pixels.iter().all(|p| *p == [0.; 4]));
    loaded.retouch_mut().base_hidden = false;
    let mut restored = original.clone();
    loaded.apply(&mut restored).unwrap();
    assert_eq!(restored.pixels, visible.pixels);
    assert_eq!(source().pixels, original.pixels);
}
