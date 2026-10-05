use super::*;
use crate::{color::LinearImage, provider::ImageLevels};

fn recipe() -> EditRecipe {
    let mut r = EditRecipe::neutral(RawEngine::TrueRenderer);
    r.process_version = 3;
    r.advanced = Some(Box::default());
    r
}
fn grid(w: u32, h: u32) -> LinearImage {
    LinearImage::new(
        w,
        h,
        (0..w * h)
            .map(|i| {
                [
                    i as f32 / 100.,
                    (i % w) as f32 / 10.,
                    (i / w) as f32 / 10.,
                    1.,
                ]
            })
            .collect(),
    )
    .unwrap()
}
#[test]
fn old_recipes_and_disabled_nodes_preserve_every_bit() {
    let source = LinearImage::new(
        4,
        1,
        vec![
            [-0.2, 0.5, 2., 1.],
            [0., 0., 0., 0.],
            [0.1, 0.2, 0.3, 0.5],
            [1e-5, 0., 10., 1.],
        ],
    )
    .unwrap();
    let old = EditRecipe::neutral(RawEngine::Apple);
    let json = serde_json::to_string(&old).unwrap();
    assert!(!json.contains("advanced"));
    for recipe in [serde_json::from_str::<EditRecipe>(&json).unwrap(), recipe()] {
        let mut output = source.clone();
        recipe.apply(&mut output).unwrap();
        assert_eq!(output.pixels, source.pixels);
    }
}
#[test]
fn crop_and_quarter_turn_use_known_source_landmarks() {
    let source = grid(6, 4);
    let mut r = recipe();
    r.advanced.as_mut().unwrap().geometry.crop = [1. / 6., 0.25, 5. / 6., 0.75];
    let mut cropped = source.clone();
    r.apply(&mut cropped).unwrap();
    assert_eq!([cropped.width, cropped.height], [4, 2]);
    for y in 0..2 {
        for x in 0..4 {
            for c in 0..4 {
                assert!(
                    (cropped.pixels[y * 4 + x][c] - source.pixels[(y + 1) * 6 + x + 1][c]).abs()
                        < 1e-6
                );
            }
        }
    }
    r.advanced.as_mut().unwrap().geometry.crop = [0., 0., 1., 1.];
    r.advanced.as_mut().unwrap().geometry.quarter_turns = 1;
    let mut rotated = source.clone();
    r.apply(&mut rotated).unwrap();
    assert_eq!([rotated.width, rotated.height], [4, 6]);
    assert_eq!(rotated.pixels[0], source.pixels[18]);
    assert_eq!(rotated.pixels[3], source.pixels[0]);
    assert_eq!(rotated.pixels[23], source.pixels[5]);
}

#[test]
fn minimum_crop_survives_float_rounding_and_tiny_ca_never_reverses_channels() {
    let mut r = recipe();
    let g = &mut r.advanced.as_mut().unwrap().geometry;
    g.crop = [0.5, 0.5, 0.51, 0.51];
    g.validate().unwrap();
    assert_eq!(g.output_size([1000, 1000]), [10, 10]);
    g.crop[2] = 0.5099;
    assert!(g.validate().is_err());
    g.crop = [0., 0., 1., 1.];
    g.ca = [-10., -10.];
    let mut image = grid(4, 1);
    r.apply(&mut image).unwrap();
    assert!(image.pixels.windows(2).all(|p| p[0][0] < p[1][0]));
    assert!(image.pixels.iter().all(|p| p[3] == 1.));
}
#[test]
fn odd_crop_mips_keep_canonical_dimensions_and_transparent_boundaries() {
    let mut r = recipe();
    let g = &mut r.advanced.as_mut().unwrap().geometry;
    g.crop = [0.1, 0.2, 0.8, 0.9];
    g.angle = 24.;
    g.perspective = [30., -20.];
    let native = [65, 43];
    let mut w = native[0];
    let mut h = native[1];
    for base in 0..6 {
        let mut image = grid(w, h);
        let output = r.apply_preview(&mut image, native, base).unwrap();
        assert_eq!(
            [image.width, image.height],
            output.map(|v| v.div_ceil(1 << base))
        );
        let opaque = image.pixels.iter().all(|p| p[3] == 1.);
        ImageLevels::from_reference_mip(image, output, base, opaque).unwrap();
        w = w.div_ceil(2);
        h = h.div_ceil(2);
    }
    let mut full = grid(65, 43);
    r.advanced.as_mut().unwrap().geometry.crop = [0., 0., 1., 1.];
    r.apply(&mut full).unwrap();
    assert!(full.pixels.iter().any(|p| p[3] == 0.));
    assert!(
        full.pixels
            .iter()
            .filter(|p| p[3] == 0.)
            .all(|p| p[..3] == [0.; 3])
    );
}
#[test]
fn flat_fields_and_alpha_survive_detail_and_noise_is_reduced() {
    let mut r = recipe();
    let d = &mut r.advanced.as_mut().unwrap().detail;
    d.texture = 45.;
    d.clarity = 60.;
    d.sharpen = 80.;
    d.luminance_noise = 50.;
    d.chroma_noise = 70.;
    let mut flat = LinearImage::new(37, 29, vec![[0.1, 0.2, 0.3, 0.5]; 37 * 29]).unwrap();
    r.apply(&mut flat).unwrap();
    assert!(flat.pixels.iter().all(|p| *p == [0.1, 0.2, 0.3, 0.5]));
    let d = &mut r.advanced.as_mut().unwrap().detail;
    d.texture = 0.;
    d.clarity = 0.;
    d.sharpen = 0.;
    d.luminance_noise = 100.;
    let mut noisy = LinearImage::new(
        32,
        32,
        (0..1024)
            .map(|i| {
                let n = if (i / 32 + i % 32) % 2 == 0 {
                    0.02
                } else {
                    -0.02
                };
                [0.3 + n, 0.3 + n, 0.3 + n, 1.]
            })
            .collect(),
    )
    .unwrap();
    let rms = |im: &LinearImage| {
        (im.pixels.iter().map(|p| (p[0] - 0.3).powi(2)).sum::<f32>() / im.pixels.len() as f32)
            .sqrt()
    };
    let before = rms(&noisy);
    r.apply(&mut noisy).unwrap();
    assert!(rms(&noisy) < before * 0.5);
    assert!(noisy.pixels.iter().all(|p| p[3] == 1.));
}
#[test]
fn masks_are_source_anchored_and_separate_brush_strokes_do_not_bridge() {
    let mut r = recipe();
    r.advanced.as_mut().unwrap().masks.push(masks::Mask {
        center: [0.25, 0.5],
        radius: 0.2,
        feather: 0.2,
        exposure: 1.,
        ..Default::default()
    });
    let source = LinearImage::new(100, 60, vec![[0.2, 0.2, 0.2, 1.]; 6000]).unwrap();
    let mut full = source.clone();
    r.apply(&mut full).unwrap();
    r.advanced.as_mut().unwrap().geometry.crop = [0.1, 0., 0.9, 1.];
    let mut crop = source;
    r.apply(&mut crop).unwrap();
    for y in 0..60 {
        for x in 0..80 {
            assert!((crop.pixels[y * 80 + x][0] - full.pixels[y * 100 + x + 10][0]).abs() < 1e-6);
        }
    }
    let m = masks::Mask {
        shape: masks::Shape::Brush,
        points: vec![[0.1, 0.1], [0.2, 0.1], [0.8, 0.1], [0.9, 0.1]],
        breaks: vec![2],
        radius: 0.03,
        ..Default::default()
    };
    m.validate().unwrap();
    assert_eq!(m.weight([0.5, 0.1], [0.5; 3], 1.), 0.);
    assert_eq!(m.weight([0.15, 0.1], [0.5; 3], 1.), 1.);
}
#[test]
fn hsl_keeps_extended_values_and_monochrome_is_achromatic() {
    let mut color = color::Color::default();
    color.bands[0].hue = 40.;
    color.bands[5].saturation = -30.;
    let extended = color.apply([-0.2, 0.1, 2.]);
    assert!(extended.iter().all(|v| v.is_finite()));
    assert!(extended.iter().any(|v| *v > 1.));
    assert!(extended.iter().any(|v| *v < 0.));
    assert_eq!(color.apply([0.2; 3]), [0.2; 3]);
    color.monochrome = true;
    let gray = color.apply([0.8, 0.2, 0.1]);
    assert_eq!(gray[0], gray[1]);
    assert_eq!(gray[1], gray[2]);
}
#[test]
fn recipes_reject_bad_coordinates_and_future_processes_before_work() {
    let mut r = recipe();
    r.advanced.as_mut().unwrap().geometry.crop[2] = 0.;
    assert!(r.validate().is_err());
    r = recipe();
    r.process_version = 2;
    assert!(r.validate().is_err());
    r = recipe();
    r.advanced.as_mut().unwrap().masks.push(masks::Mask {
        points: vec![[f32::NAN, 0.]],
        ..Default::default()
    });
    assert!(r.validate().is_err());
    r = recipe();
    r.advanced.as_mut().unwrap().masks.push(masks::Mask {
        points: vec![[0.; 2]; 513],
        ..Default::default()
    });
    assert!(r.validate().is_err());
    let mut old = recipe();
    old.advanced = None;
    old.process_version = 4;
    assert!(old.validate().is_err());
}

#[test]
fn accelerated_brush_matches_direct_segments_with_inversion_and_separate_strokes() {
    for invert in [false, true] {
        let mask = masks::Mask {
            shape: masks::Shape::Brush,
            points: (0..70)
                .map(|i| [((i * 17) % 71) as f32 / 71., ((i * 11) % 73) as f32 / 73.])
                .collect(),
            breaks: vec![14, 40],
            radius: 0.09,
            feather: 0.7,
            exposure: 0.8,
            saturation: -20.,
            invert,
            ..Default::default()
        };
        mask.validate().unwrap();
        let compiled = masks::Prepared::new(&mask, 0.6);
        for y in 0..31 {
            for x in 0..47 {
                let p = [x as f32 / 46., y as f32 / 30.];
                let mut reference = [0.2, 0.4, 0.3];
                let mut actual = reference;
                mask.apply(&mut reference, [0.2; 3], p, 0.6);
                compiled.apply(&mut actual, [0.2; 3], p);
                for c in 0..3 {
                    assert!((actual[c] - reference[c]).abs() < 2e-6, "{p:?}");
                }
            }
        }
    }
}

#[test]
fn optical_extremes_keep_radial_map_monotone_and_flat_colours_do_not_defringe() {
    for distortion in [-100., 100.] {
        for moustache in [-100., 100.] {
            let g = geometry::Geometry {
                distortion,
                moustache,
                ..Default::default()
            };
            let mut previous = 0.;
            for i in 0..100 {
                let p = g
                    .source_point([0.5 + i as f64 / 100., 0.5], [1000, 1000])
                    .unwrap();
                assert!(p[0] > previous);
                previous = p[0];
            }
        }
    }
    let mut r = recipe();
    r.advanced.as_mut().unwrap().geometry.defringe = 100.;
    let mut flat = LinearImage::new(8, 8, vec![[0.5, 0.1, 0.5, 1.]; 64]).unwrap();
    r.apply(&mut flat).unwrap();
    assert!(flat.pixels.iter().all(|p| *p == [0.5, 0.1, 0.5, 1.]));
}
