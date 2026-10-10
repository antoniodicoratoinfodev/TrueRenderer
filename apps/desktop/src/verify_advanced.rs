//! Historical and layered native/export parity on an explicit, isolated root.
use anyhow::{Result, ensure};
use std::path::Path;
use tr_core::{
    decoder::RawEngine,
    editing::{
        Advanced, EditRecipe,
        masks::{Mask, Shape},
    },
    export::{Format, MAX_ENCODED, Options},
};

pub fn recipe(engine: RawEngine) -> EditRecipe {
    let mut r = EditRecipe::neutral(engine);
    r.process_version = 3;
    r.exposure_ev = 0.25;
    let mut a = Advanced::default();
    a.geometry.crop = [0.12, 0.08, 0.9, 0.92];
    a.geometry.quarter_turns = 1;
    a.geometry.angle = 3.;
    a.geometry.perspective = [8., -6.];
    a.geometry.distortion = 14.;
    a.geometry.vignette = 15.;
    a.geometry.ca = [0.7, -0.5];
    a.geometry.defringe = 20.;
    a.detail.texture = 15.;
    a.detail.clarity = 12.;
    a.detail.sharpen = 35.;
    a.detail.radius = 1.4;
    a.detail.luminance_noise = 25.;
    a.detail.chroma_noise = 35.;
    a.color.bands[0].hue = 12.;
    a.color.bands[5].saturation = -15.;
    a.color.grading[0].hue = 230.;
    a.color.grading[0].amount = 8.;
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
    r
}

pub fn layered_recipe(engine: RawEngine) -> EditRecipe {
    use tr_core::editing::layers::{Combine, Layer, MaskKind, Operator, SampleColor, TonalLevel};
    let mut r = recipe(engine);
    let mut color = Layer::new(
        "Blue fabric",
        Operator::SampleColor(SampleColor {
            correction: [20., -25., 10.],
            uniformity: [35., 25., 0.],
            ..Default::default()
        }),
    );
    color.opacity = 0.7;
    color.mask.append(
        MaskKind::Shape(Mask {
            shape: Shape::Linear,
            angle: 35.,
            ..Default::default()
        }),
        Combine::Add,
    );
    color.mask.append(
        MaskKind::Shape(Mask {
            center: [0.7, 0.5],
            radius: 0.2,
            ..Default::default()
        }),
        Combine::Subtract,
    );
    r.layer_stack().layers.push(color);
    let mut levels = [TonalLevel::default(); 4];
    levels[0].gamma = 1.12;
    r.layer_stack()
        .layers
        .push(Layer::new("Finish", Operator::Levels(levels)));
    r
}

pub fn toning_recipe(engine: RawEngine) -> EditRecipe {
    use tr_core::editing::layers::{
        BlackAndWhite, ColorFilter, Combine, GradientMap, GradientStop, Grading, GradingZone,
        Layer, MaskKind, Operator,
    };
    let mut r = recipe(engine);
    let mut grading = Grading {
        balance: 15.,
        overlap: 65.,
        ..Default::default()
    };
    grading.zones[0] = GradingZone {
        hue: 220.,
        amount: 28.,
        exposure: 0.2,
    };
    grading.zones[1].exposure = 0.1;
    grading.zones[2] = GradingZone {
        hue: 40.,
        amount: 20.,
        exposure: -0.1,
    };
    grading.zones[3] = GradingZone {
        hue: 315.,
        amount: 4.,
        exposure: 0.05,
    };
    let mut layer = Layer::new("Creative color", Operator::Grading(grading));
    layer.operators.extend([
        Operator::BlackAndWhite(BlackAndWhite {
            amount: 25.,
            bands: [25., 10., -15., 0., 5., -30., 0., 15.],
            ..Default::default()
        }),
        Operator::ColorFilter(ColorFilter {
            density: 20.,
            ..Default::default()
        }),
        Operator::GradientMap(GradientMap {
            amount: 15.,
            stops: vec![
                GradientStop {
                    position: 0.,
                    color: [0.02, 0.01, 0.06],
                },
                GradientStop {
                    position: 0.45,
                    color: [0.55, 0.4, 0.3],
                },
                GradientStop {
                    position: 1.,
                    color: [1.1, 1., 0.85],
                },
            ],
            ..Default::default()
        }),
    ]);
    layer.opacity = 0.85;
    layer.mask.append(
        MaskKind::Shape(Mask {
            shape: Shape::Linear,
            angle: 25.,
            radius: 0.65,
            ..Default::default()
        }),
        Combine::Add,
    );
    r.layer_stack().layers.push(layer);
    r
}

pub fn selective_recipe(engine: RawEngine, absolute: bool) -> EditRecipe {
    use tr_core::editing::layers::*;
    let mut r = recipe(engine);
    let mut selective = SelectiveColor {
        method: if absolute {
            SelectiveMethod::Absolute
        } else {
            SelectiveMethod::Relative
        },
        ..Default::default()
    };
    selective.adjustments[0] = [12., -8., 4., 2.];
    selective.adjustments[4] = [-10., 8., 15., -4.];
    selective.adjustments[7] = [2., -1., 0., 3.];
    let mut layer = Layer::new(
        "Selective color and tone",
        Operator::SelectiveColor(selective),
    );
    layer.operators.extend([
        Operator::Colorize(Colorize {
            amount: if absolute { 100. } else { 20. },
            hue: 210.,
            exposure: 0.1,
            ..Default::default()
        }),
        Operator::TonalAdjustments(TonalAdjustments {
            exposure: 0.1,
            brightness: 8.,
            contrast: 6.,
            shadows: 15.,
            highlights: -18.,
            blacks: -8.,
            whites: 6.,
            ..Default::default()
        }),
        Operator::ExposureGamma(ExposureGamma {
            exposure: -0.1,
            offset: -0.01,
            gamma: 1.08,
            ..Default::default()
        }),
    ]);
    layer.opacity = 0.8;
    layer.mask.append(
        MaskKind::Shape(Mask {
            shape: Shape::Linear,
            angle: 25.,
            radius: 0.65,
            ..Default::default()
        }),
        Combine::Add,
    );
    r.layer_stack().layers.push(layer);
    r
}

pub fn curves_recipe(engine: RawEngine, rgb: bool) -> EditRecipe {
    use tr_core::editing::{CurvePoint, layers::*};
    let mut r = recipe(engine);
    let mut layer = Layer::new(
        "Tone curves",
        Operator::ParametricCurve(ParametricCurve {
            space: if rgb {
                CurveSpace::Rgb
            } else {
                CurveSpace::Luminance
            },
            amounts: [30., -15., 20., -25.],
            boundaries: [0.18, 0.45, 0.8],
            ..Default::default()
        }),
    );
    let mut levels = LevelAdjustments::default();
    levels.channels[0].gamma = 1.08;
    layer.operators.extend([
        Operator::LuminanceCurve(LuminanceCurve {
            points: vec![
                CurvePoint { x: 0., y: 0.01 },
                CurvePoint { x: 0.4, y: 0.45 },
                CurvePoint { x: 1., y: 0.98 },
            ],
            ..Default::default()
        }),
        Operator::TonalLevels(Box::new(levels)),
    ]);
    layer.opacity = 0.8;
    layer.mask.append(
        MaskKind::Shape(Mask {
            shape: Shape::Linear,
            angle: 25.,
            radius: 0.65,
            ..Default::default()
        }),
        Combine::Add,
    );
    r.layer_stack().layers.push(layer);
    r
}

/// Verification helper using the same input projection and bounded statistics
/// as the interactive controls, with an explicit source digest.
pub fn freeze_levels_auto(
    r: &mut EditRecipe,
    source: &tr_core::color::LinearImage,
    digest: &str,
    independent: bool,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    use tr_core::editing::layers::*;
    let stack = r.layers.as_ref().unwrap();
    let layer = &stack.layers[0];
    let index = layer
        .operators
        .iter()
        .position(|op| matches!(op, Operator::TonalLevels(_)))
        .unwrap();
    let id = layer.id;
    let mut input = r.clone();
    input
        .layers
        .as_mut()
        .unwrap()
        .retain_before_operator(id, index)?;
    let mut image = source.clone();
    input.apply(&mut image)?;
    let Operator::TonalLevels(g) = &mut r.layers.as_mut().unwrap().layers[0].operators[index]
    else {
        unreachable!()
    };
    let stats = LevelStatistics::from_image(&image, g.channels[0])?;
    g.auto(
        &stats,
        LevelAnalysis {
            action: if independent {
                LevelsAction::AutoChannels
            } else {
                LevelsAction::AutoComposite
            },
            channel: 0,
            source_digest: digest.into(),
            input_recipe_hash: format!("{:x}", Sha256::digest(serde_json::to_vec(&input)?)),
            geometry: input
                .advanced
                .as_ref()
                .map(|a| a.geometry.clone())
                .unwrap_or_default(),
            image_size: [image.width, image.height],
            valid_samples: stats.valid,
            considered_samples: stats.considered,
            sample_xy: None,
            sample_rgb: None,
        },
    )
}

pub fn look_recipe(engine: RawEngine, masks: bool) -> EditRecipe {
    use tr_core::editing::looks::Look;
    let source = curves_recipe(engine, false);
    let ids: Vec<_> = source
        .layers
        .as_ref()
        .unwrap()
        .layers
        .iter()
        .map(|l| l.id)
        .collect();
    let captured = Look::capture(&source, &ids, true).unwrap();
    let saved: Look = serde_json::from_slice(&serde_json::to_vec(&captured).unwrap()).unwrap();
    let ids: Vec<_> = saved.layers.iter().map(|l| l.id).collect();
    saved
        .append_to(&selective_recipe(engine, false), &ids, 0.65, masks)
        .unwrap()
}

pub fn run(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    std::fs::create_dir_all(root.join("reports"))?;
    let report_path = root.join("reports/advanced-editing.json");
    std::fs::write(
        &report_path,
        br#"{"passed":false,"reason":"probe incomplete"}"#,
    )?;
    let (_, digest) = tr_platform::snapshot(source)?;
    let mut cases = Vec::new();
    for engine in RawEngine::choices() {
        for slot in 0..2 {
            let mut broker = tr_platform::Broker::with_slot(worker.into(), slot);
            broker.set_raw_engine(engine);
            let snapshot = broker.prepare_snapshot(source, &digest, || false)?;
            let decoded = broker.decode(source, &digest, 0)?;
            let native_size = [decoded.raster.width, decoded.raster.height];
            for mode in 0..12 {
                let mut r = recipe(engine);
                if mode == 11 {
                    r = toning_recipe(engine);
                    let stack = r.layer_stack();
                    stack
                        .layers
                        .extend(selective_recipe(engine, true).layers.unwrap().layers);
                    stack
                        .layers
                        .extend(curves_recipe(engine, false).layers.unwrap().layers);
                    for layer in &mut stack.layers {
                        layer.opacity = 0.6;
                        layer.fill = 0.35;
                    }
                    stack
                        .layers
                        .push(tr_core::editing::layers::Layer::empty("Empty"));
                } else if mode == 1 {
                    r.advanced.as_mut().unwrap().geometry = Default::default();
                    r.advanced.as_mut().unwrap().color.monochrome = true;
                } else if mode == 2 {
                    r.advanced.as_mut().unwrap().detail.dehaze = 15.;
                    r.advanced.as_mut().unwrap().geometry.crop = [0.495, 0.495, 0.51, 0.51];
                } else if mode == 3 {
                    r = layered_recipe(engine);
                } else if mode == 4 {
                    r = toning_recipe(engine);
                } else if mode == 5 || mode == 6 {
                    r = selective_recipe(engine, mode == 6);
                } else if mode >= 9 {
                    r = look_recipe(engine, mode == 9);
                } else if mode >= 7 {
                    r = curves_recipe(engine, mode == 8);
                    freeze_levels_auto(&mut r, &decoded.raster, &digest, mode == 8)?;
                }
                let mut expected = decoded.raster.clone();
                r.apply(&mut expected)?;
                for format in [Format::Png16, Format::Tiff16] {
                    let options = Options {
                        format,
                        ..Default::default()
                    };
                    let (info, bytes) = broker.export_edited_snapshot_bounded(
                        snapshot.clone(),
                        options,
                        Some(r.clone()),
                        MAX_ENCODED,
                        || false,
                    )?;
                    let actual = image::load_from_memory(&bytes)?.to_rgba16();
                    ensure!(
                        [expected.width, expected.height] == [info.width, info.height]
                            && actual.dimensions() == (info.width, info.height),
                        "Dimensioni diverse dopo editing"
                    );
                    let mut maximum = 0_u16;
                    for (working, encoded) in expected.pixels.iter().zip(actual.pixels()) {
                        let (reference, _) = tr_core::export::srgb16(*working);
                        for (a, b) in reference.into_iter().zip(encoded.0) {
                            maximum = maximum.max(a.abs_diff(b));
                        }
                    }
                    ensure!(
                        maximum == 0,
                        "Vista/export diversi: {engine:?}, slot {slot}, {format:?}, {maximum}"
                    );
                    cases.push(serde_json::json!({"engine":engine,"slot":slot,"mode":mode,"format":format,"source_size":native_size,"output_size":[info.width,info.height],"maximum_error_u16":maximum,"recipe":r}));
                }
            }
        }
    }
    ensure!(
        tr_platform::snapshot(source)?.1 == digest,
        "Sorgente cambiata durante la prova"
    );
    let report = serde_json::json!({"passed":true,"source_digest":digest,"source_unchanged":true,"cases":cases,"scope":"Exact PNG16/TIFF16 parity against the canonical CPU graph, all available engines and both workers, explicit source. No camera colour, display, Windows or performance qualification."});
    std::fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
    println!("Advanced editing: {} cases passed", cases.len());
    Ok(())
}
