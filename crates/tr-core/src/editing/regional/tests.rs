use super::*;
use crate::{
    editing::{Advanced, CurvePoint, layers::*, masks::*},
    preview::PreviewRequest,
};

pub(super) fn source(alpha: bool, size: [u32; 2]) -> ImageLevels {
    let pixels = (0..size[0] * size[1])
        .map(|i| {
            let a = if alpha {
                [0., 0.2, 0.7, 1.][i as usize % 4]
            } else {
                1.
            };
            // Fine detail, negative and HDR samples expose degraded input and halos.
            [
                ((i * 17) % 113) as f32 / 79. * a - 0.05 * a,
                ((i * 31) % 71) as f32 / 67. * a,
                (i % 3) as f32 * 0.5 * a,
                a,
            ]
        })
        .collect();
    ImageLevels::from_source(
        LinearImage::new(size[0], size[1], pixels).unwrap(),
        PreviewRequest::full(),
    )
    .unwrap()
}
pub(super) fn recipe() -> EditRecipe {
    let mut r = EditRecipe::neutral(Default::default());
    r.process_version = 3;
    r.exposure_ev = 0.25;
    r.curve = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 0.4, y: 0.5 },
        CurvePoint { x: 1., y: 1. },
    ];
    let mut a = Advanced::default();
    a.geometry.vignette = 12.;
    a.geometry.defringe = 15.;
    a.detail.texture = 15.;
    a.detail.clarity = 12.;
    a.detail.sharpen = 35.;
    a.detail.radius = 1.4;
    a.detail.luminance_noise = 25.;
    a.detail.chroma_noise = 35.;
    a.detail.denoise = Denoise::Legacy;
    a.color.bands[0].hue = 12.;
    a.masks.push(Mask {
        exposure: 0.5,
        warmth: 10.,
        ..Default::default()
    });
    a.masks.push(Mask {
        shape: Shape::Brush,
        points: vec![[0.2, 0.3], [0.3, 0.4], [0.4, 0.4]],
        exposure: -0.4,
        ..Default::default()
    });
    r.advanced = Some(Box::new(a));
    let mut layer = Layer::new(
        "color",
        Operator::SampleColor(SampleColor {
            correction: [20., -25., 10.],
            uniformity: [35., 25., 0.],
            ..Default::default()
        }),
    );
    layer.opacity = 0.7;
    layer.mask.append(
        MaskKind::Shape(Mask {
            shape: Shape::Linear,
            angle: 35.,
            ..Default::default()
        }),
        Combine::Add,
    );
    r.layer_stack().layers.push(layer);
    let mut levels = [TonalLevel::default(); 4];
    levels[0].gamma = 1.12;
    r.layer_stack()
        .layers
        .push(Layer::new("finish", Operator::Levels(levels)));
    r
}
pub(super) fn reference(source: &ImageLevels, recipe: &EditRecipe, gains: [f32; 3]) -> ImageLevels {
    let mut raster = source.source().clone();
    for p in &mut raster.pixels {
        for (c, g) in p[..3].iter_mut().zip(gains) {
            *c *= g;
        }
    }
    let size = recipe
        .apply_preview(&mut raster, source.source_size(), source.base_level())
        .unwrap();
    let opaque = raster.pixels.iter().all(|p| p[3] == 1.);
    ImageLevels::from_reference_mip(raster, size, source.base_level(), opaque).unwrap()
}
pub(super) fn compare(actual: &LinearImage, expected: &LinearImage) {
    assert_eq!(
        [actual.width, actual.height],
        [expected.width, expected.height]
    );
    let mut maximum = 0_f32;
    for (a, b) in actual
        .pixels
        .iter()
        .flatten()
        .zip(expected.pixels.iter().flatten())
    {
        maximum = maximum.max((a - b).abs());
        assert!(
            (a - b).abs() <= 1e-6 + 1e-5 * b.abs(),
            "regional {a}, canonical {b}, max {maximum}"
        );
    }
    let max = actual
        .to_display()
        .iter()
        .zip(expected.to_display())
        .map(|(a, b)| a.abs_diff(b))
        .max()
        .unwrap();
    assert!(max <= 1, "display error {max}");
}

#[test]
fn interrupted_preparation_releases_checkpoints_and_resumes_canonically() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let source = source(false, [265, 199]);
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let mut processor = Processor::new(memory.clone());
    let region = Region {
        origin: [61.3, 63.7],
        size: [67, 65],
        step: [1.; 2],
    };
    for denoise in [Denoise::SpectralV1, Denoise::MultiscaleV2] {
        let mut original = recipe();
        original.advanced.as_mut().unwrap().detail.denoise = denoise;
        let mut changed = original.clone();
        changed.exposure_ev += 0.3;
        let expected = reference(&source, &changed, [1.; 3])
            .render(region)
            .unwrap();
        for after in [0, 5, 40, 120] {
            processor.clear();
            processor
                .render(&source, &original, [1.; 3], region)
                .unwrap();
            assert!(processor.cache_bytes() > 0);
            let calls = AtomicUsize::new(0);
            let completed_before = processor.statistics.renders;
            let result = processor.render_cancellable(&source, &changed, [1.; 3], region, &|| {
                calls.fetch_add(1, Ordering::Relaxed) >= after
            });
            assert!(
                result.is_err(),
                "denoise={denoise:?}, cancellation after {after}"
            );
            assert_eq!(processor.statistics.renders, completed_before);
            assert!(processor.identity.is_none());
            assert_eq!(processor.cache_bytes(), 0);
            assert_eq!(memory.usage().reserved, 0);
            compare(
                &processor
                    .render(&source, &changed, [1.; 3], region)
                    .unwrap(),
                &expected,
            );
        }
    }
}

#[test]
fn regional_edit_matches_full_graph_at_native_fractional_and_mip_scales() {
    let mut processor = Processor::new(MemoryBudget::new(512 * 1024 * 1024));
    for alpha in [false, true] {
        for mapped in [false, true] {
            let source = source(alpha, [261, 197]);
            let mut r = recipe();
            if mapped {
                let g = &mut r.advanced.as_mut().unwrap().geometry;
                g.crop = [0.12, 0.08, 0.9, 0.92];
                g.quarter_turns = 1;
                g.angle = 3.;
                g.perspective = [8., -6.];
                g.distortion = 14.;
                g.ca = [0.7, -0.5];
            }
            let expected = reference(&source, &r, [1.1, 1., 0.9]);
            for step in [0.7, 1., 1.91, 2.1, 4.2] {
                for origin in [[33.25, 47.5], [-2.25, -1.5], [160., 113.]] {
                    let region = Region {
                        size: [39, 31],
                        origin,
                        step: [step; 2],
                    };
                    compare(
                        &processor
                            .render(&source, &r, [1.1, 1., 0.9], region)
                            .unwrap(),
                        &expected.render(region).unwrap(),
                    );
                }
            }
        }
    }
}

#[test]
fn regional_grain_and_global_filters_keep_the_full_image_domain() {
    let source = source(false, [129, 97]);
    let mut processor = Processor::new(MemoryBudget::new(128 * 1024 * 1024));
    for kind in 0..4 {
        let mut r = recipe();
        let a = r.advanced.as_mut().unwrap();
        a.detail.grain.enabled = true;
        a.detail.grain.monochrome = false;
        a.detail.grain.size = 2.7;
        match kind {
            0 => a.detail.dehaze = 20.,
            1 => a.detail.denoise = Denoise::SpectralV1,
            2 => a.detail.denoise = Denoise::MultiscaleV2,
            _ => a.geometry.crop = [0.1, 0.1, 0.8, 0.9],
        }
        let region = Region {
            size: [37, 23],
            origin: [31.25, 22.5],
            step: [1.; 2],
        };
        let expected = reference(&source, &r, [1.; 3]);
        compare(
            &processor.render(&source, &r, [1.; 3], region).unwrap(),
            &expected.render(region).unwrap(),
        );
    }
}

#[test]
fn layer_opacity_reuses_prefix_and_invalidates_on_source_mask_geometry_and_upstream_changes() {
    let source = source(true, [517, 389]);
    let memory = MemoryBudget::new(128 * 1024 * 1024);
    let mut processor = Processor::new(memory.clone());
    let mut r = recipe();
    let region = Region {
        size: [59, 43],
        origin: [171., 109.],
        step: [1.; 2],
    };
    processor.render(&source, &r, [1.; 3], region).unwrap();
    let before = processor.statistics.stage_evaluations;
    r.layers.as_mut().unwrap().layers[0].opacity = 0.4;
    compare(
        &processor.render(&source, &r, [1.; 3], region).unwrap(),
        &reference(&source, &r, [1.; 3]).render(region).unwrap(),
    );
    assert!(processor.statistics.stages_reused > 0);
    assert!(processor.statistics.stage_evaluations - before <= 3);
    assert!(
        processor.statistics.input_pixels
            < 2 * u64::from(source.source().width) * u64::from(source.source().height) / 4
    );
    if let Operator::SampleColor(c) = &mut r.layers.as_mut().unwrap().layers[0].operators[0] {
        c.correction[0] = -35.;
        c.uniformity[2] = 20.;
    }
    compare(
        &processor.render(&source, &r, [1.; 3], region).unwrap(),
        &reference(&source, &r, [1.; 3]).render(region).unwrap(),
    );
    assert_eq!(processor.statistics.layer_mask_reuses, 1);
    for step in 0..4 {
        match step {
            0 => r.exposure_ev = 0.7,
            1 => {
                r.layers.as_mut().unwrap().layers[0]
                    .mask
                    .append(MaskKind::Constant(0.5), Combine::Intersect);
            }
            2 => r.advanced.as_mut().unwrap().geometry.angle = 2.,
            _ => r.advanced.as_mut().unwrap().detail.radius = 2.5,
        }
        compare(
            &processor.render(&source, &r, [1.; 3], region).unwrap(),
            &reference(&source, &r, [1.; 3]).render(region).unwrap(),
        );
        assert_eq!(processor.statistics.layer_mask_reuses, 1);
    }
    let replacement = super::tests::source(false, [517, 389]);
    compare(
        &processor.render(&replacement, &r, [1.; 3], region).unwrap(),
        &reference(&replacement, &r, [1.; 3]).render(region).unwrap(),
    );
    assert!(memory.usage().reserved > 0);
    assert!(memory.usage().peak <= memory.usage().limit / 4);
    // Diagnostic counters can be reset independently of cache ownership.
    // Lowering the budget must still reclaim the cache, including when the
    // next recipe is neutral and returns before evaluating any edit stages.
    processor.statistics = Statistics::default();
    memory.configure(memory.usage().reserved);
    let neutral = EditRecipe::neutral(Default::default());
    compare(
        &processor
            .render(&replacement, &neutral, [1.; 3], region)
            .unwrap(),
        &replacement.render(region).unwrap(),
    );
    assert_eq!(memory.usage().reserved, 0);
}

#[test]
fn every_layer_operator_uses_the_canonical_evaluator() {
    let source = source(true, [97, 73]);
    let mut processor = Processor::new(MemoryBudget::new(64 * 1024 * 1024));
    for tool in TOOLS {
        let mut r = recipe();
        let mut operator = tool.operator();
        match &mut operator {
            Operator::SampleColor(c) => {
                c.correction = [25., -20., 15.];
                c.uniformity = [30., 20., 10.];
            }
            Operator::Light {
                exposure,
                temperature,
                ..
            } => {
                *exposure = 0.7;
                *temperature = 15.;
            }
            Operator::Curves(c) => {
                c[0] = vec![
                    CurvePoint { x: 0., y: 0.1 },
                    CurvePoint { x: 0.4, y: 0.6 },
                    CurvePoint { x: 1., y: 0.9 },
                ]
            }
            Operator::Levels(c) => c[0].gamma = 1.2,
            Operator::TonalLevels(c) => c.channels[0].gamma = 1.2,
            Operator::Mixer(c) => c.bands[0].hue = 30.,
            Operator::Channels { offset, .. } => *offset = [0.1, -0.05, 0.2],
            Operator::Grading(c) => {
                c.zones[0] = GradingZone {
                    hue: 220.,
                    amount: 20.,
                    exposure: 0.2,
                }
            }
            Operator::BlackAndWhite(c) => c.amount = 45.,
            Operator::ColorFilter(c) => c.density = 35.,
            Operator::GradientMap(c) => c.amount = 30.,
            Operator::SelectiveColor(c) => c.adjustments[0] = [15., -20., 10., 5.],
            Operator::Colorize(c) => c.amount = 30.,
            Operator::TonalAdjustments(c) => c.shadows = 35.,
            Operator::ExposureGamma(c) => c.gamma = 1.2,
            Operator::LuminanceCurve(c) => {
                c.points = vec![
                    CurvePoint { x: 0., y: 0. },
                    CurvePoint { x: 0.5, y: 0.7 },
                    CurvePoint { x: 1., y: 1. },
                ]
            }
            Operator::ParametricCurve(c) => c.amounts = [20., -30., 15., -10.],
        }
        assert!(!operator.is_neutral(), "{}", tool.id);
        r.layers.as_mut().unwrap().layers[0].operators = vec![operator];
        let region = Region {
            size: [43, 31],
            origin: [17.25, 11.75],
            step: [1.3; 2],
        };
        compare(
            &processor.render(&source, &r, [1.; 3], region).unwrap(),
            &reference(&source, &r, [1.; 3]).render(region).unwrap(),
        );
        let before = processor.statistics.opacity_reuses;
        r.layers.as_mut().unwrap().layers[0].opacity = 0.35;
        r.layers.as_mut().unwrap().layers[0].fill = 0.7;
        compare(
            &processor.render(&source, &r, [1.; 3], region).unwrap(),
            &reference(&source, &r, [1.; 3]).render(region).unwrap(),
        );
        assert!(processor.statistics.opacity_reuses > before);
    }
}

#[test]
fn regional_cache_eviction_and_invalid_requests_never_change_pixels_or_leak_credits() {
    let source = source(false, [257, 193]);
    let recipe = recipe();
    let memory = MemoryBudget::new(64 * 1024 * 1024);
    let mut processor = Processor::new(memory.clone());
    let region = Region {
        size: [67, 43],
        origin: [111.25, 53.5],
        step: [0.91; 2],
    };
    let expected = reference(&source, &recipe, [1.; 3]).render(region).unwrap();
    compare(
        &processor.render(&source, &recipe, [1.; 3], region).unwrap(),
        &expected,
    );
    memory.configure(0);
    compare(
        &processor.render(&source, &recipe, [1.; 3], region).unwrap(),
        &expected,
    );
    assert_eq!(memory.usage().reserved, 0);
    assert!(
        processor
            .render(
                &source,
                &recipe,
                [1.; 3],
                Region {
                    step: [0.; 2],
                    ..region
                }
            )
            .is_err()
    );
    assert_eq!(memory.usage().reserved, 0);
}

#[test]
fn spatial_amount_reuses_its_basis_without_changing_limits_or_alpha() {
    let source = source(true, [269, 203]);
    let memory = MemoryBudget::new(128 * 1024 * 1024);
    let region = Region {
        size: [71, 53],
        origin: [95.25, 63.5],
        step: [1.1; 2],
    };
    for kind in 0..3 {
        let mut processor = Processor::new(memory.clone());
        let mut r = recipe();
        for amount in [15., 25., 50., 0., 100., 35.] {
            let d = &mut r.advanced.as_mut().unwrap().detail;
            match kind {
                0 => d.texture = amount,
                1 => d.clarity = amount,
                _ => d.sharpen = amount,
            }
            compare(
                &processor.render(&source, &r, [1.; 3], region).unwrap(),
                &reference(&source, &r, [1.; 3]).render(region).unwrap(),
            );
        }
        assert!(processor.statistics.spatial_reuses >= 2);
        for edit in 0..3 {
            match edit {
                0 => r.advanced.as_mut().unwrap().detail.threshold = 15.,
                1 => r.advanced.as_mut().unwrap().detail.radius = 2.7,
                _ => r.exposure_ev += 0.25,
            }
            compare(
                &processor.render(&source, &r, [1.; 3], region).unwrap(),
                &reference(&source, &r, [1.; 3]).render(region).unwrap(),
            );
        }
        processor.clear();
        assert_eq!(memory.usage().reserved, 0);
    }
}

#[test]
fn fused_global_noise_matches_materialized_input_and_switches_back_to_cached_tools() {
    let memory = MemoryBudget::new(128 * 1024 * 1024);
    for alpha in [false, true] {
        for noise in [Denoise::SpectralV1, Denoise::MultiscaleV2] {
            let mut raster = source(false, [277, 213]).source().clone();
            if alpha {
                for y in 71..93 {
                    for x in 102..139 {
                        let p = &mut raster.pixels[y * 277 + x];
                        let a = if x < 115 { 0. } else { 0.4 };
                        for v in p {
                            *v *= a;
                        }
                    }
                }
            }
            let source = ImageLevels::from_source(raster, PreviewRequest::full()).unwrap();
            let mut processor = Processor::new(memory.clone());
            let mut r = recipe();
            let a = r.advanced.as_mut().unwrap();
            a.detail.denoise = noise;
            a.geometry.vignette = 19.;
            a.geometry.angle = 2.;
            a.geometry.ca = [0.5, -0.3];
            let region = Region {
                size: [63, 49],
                origin: [91.25, 67.5],
                step: [1.4; 2],
            };
            for ev in [0.25, 0.7, -0.35] {
                r.exposure_ev = ev;
                compare(
                    &processor
                        .render(&source, &r, [1.1, 1., 0.9], region)
                        .unwrap(),
                    &reference(&source, &r, [1.1, 1., 0.9])
                        .render(region)
                        .unwrap(),
                );
            }
            assert_eq!(processor.statistics.fused_source_frames, 2);
            for texture in [-15., 0., 30., 45.] {
                r.advanced.as_mut().unwrap().detail.texture = texture;
                compare(
                    &processor
                        .render(&source, &r, [1.1, 1., 0.9], region)
                        .unwrap(),
                    &reference(&source, &r, [1.1, 1., 0.9])
                        .render(region)
                        .unwrap(),
                );
            }
            assert!(processor.statistics.stages_reused >= 4);
            for origin in [[-1.25, -0.75], [249.5, 185.25]] {
                let region = Region { origin, ..region };
                r.exposure_ev += 0.1;
                compare(
                    &processor
                        .render(&source, &r, [1.1, 1., 0.9], region)
                        .unwrap(),
                    &reference(&source, &r, [1.1, 1., 0.9])
                        .render(region)
                        .unwrap(),
                );
            }
            r.advanced.as_mut().unwrap().geometry.vignette = -37.;
            compare(
                &processor
                    .render(&source, &r, [1.1, 1., 0.9], region)
                    .unwrap(),
                &reference(&source, &r, [1.1, 1., 0.9])
                    .render(region)
                    .unwrap(),
            );
            processor.clear();
            assert_eq!(memory.usage().reserved, 0);
        }
    }
}
