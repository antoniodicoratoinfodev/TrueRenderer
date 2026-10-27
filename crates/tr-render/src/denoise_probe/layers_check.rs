//! Scalar full recipes independently check guide changes, graph operations and alpha.
use super::*;
use std::{sync::Arc, time::Duration};
use tr_core::{
    budget::MemoryBudget,
    editing::{
        Advanced, EditRecipe,
        layers::{
            ColorRange, Combine, Layer, MaskGraph, MaskKind, Operator, SampleColor, TonalLevel,
        },
        masks::{Mask, Shape},
    },
};
pub(super) type Check = spatial_check::Check;
fn mask(variant: usize) -> MaskGraph {
    let mut graph = MaskGraph::default();
    if variant == 9 {
        return graph;
    }
    let shapes = [
        Shape::Radial,
        Shape::Linear,
        Shape::Brush,
        Shape::Luminance,
        Shape::Hue,
    ];
    for (i, shape) in shapes.into_iter().enumerate() {
        if variant != i && variant < 7 {
            continue;
        }
        graph.append(
            MaskKind::Shape(Mask {
                shape,
                center: [0.35, 0.65],
                radius: 0.35,
                feather: 0.7,
                angle: -71.,
                interval: [0.2, 0.8],
                points: vec![[0.1, 0.25], [0.5, 0.35], [0.8, 0.9], [0.4, 0.6]],
                breaks: vec![2],
                ..Default::default()
            }),
            [Combine::Add, Combine::Subtract, Combine::Intersect][i % 3],
        );
    }
    if variant >= 5 {
        graph.append(
            MaskKind::Color(ColorRange {
                reference: if variant == 6 {
                    [0.3; 3]
                } else {
                    [0.3, 0.5, 0.6]
                },
                width: [80., 1.5, 1.5],
                softness: if variant == 5 { 0.01 } else { 0.7 },
            }),
            Combine::Add,
        );
    }
    if variant >= 7 {
        graph.append(MaskKind::Constant(0.3), Combine::Subtract);
        if variant == 8 {
            graph.invert();
        }
    }
    graph
}
pub(super) fn add_layers(recipe: &mut EditRecipe, variant: usize) {
    let sample = Operator::SampleColor(SampleColor {
        range: ColorRange {
            width: [120., 2., 2.],
            ..Default::default()
        },
        correction: [-35., 20., -10.],
        uniformity: [20., 15., 10.],
    });
    let mut first = Layer::new("dynamic mask", sample);
    first.operators.push(Operator::Light {
        exposure: 0.25,
        temperature: 15.,
        tint: -12.,
        saturation: 10.,
    });
    first.opacity = if variant == 9 { 0. } else { 0.7 };
    first.fill = if variant.is_multiple_of(2) { 0.5 } else { 1. };
    first.mask = mask(variant);
    let channels = [
        TonalLevel {
            gamma: 1.12,
            ..Default::default()
        },
        TonalLevel::default(),
        TonalLevel::default(),
        TonalLevel::default(),
    ];
    recipe.layer_stack().layers = vec![first, Layer::new("levels", Operator::Levels(channels))];
}
pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let layers = layers::Layers::new(device);
    let mut result = Check::default();
    let full = [517, 269];
    for pattern in [2, 4, 8] {
        let source = reconstruction::source(full, pattern)?;
        for base in [0, 2] {
            let native = full.map(|v| v * (1 << base) - ((1 << base) - 1));
            for variant in 0..10 {
                let mut recipe = EditRecipe::neutral(Default::default());
                recipe.require_process(3);
                recipe.advanced = Some(Box::new(Advanced::default()));
                add_layers(&mut recipe, variant);
                let mut expected = source.clone();
                recipe.apply_preview(&mut expected, native, base)?;
                for (origin, size) in [([0, 0], full), ([17, 19], [480, 226])] {
                    let input = spatial_check::extract(&source, origin, size)?;
                    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(&input.pixels),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                    let mut encoder = device.create_command_encoder(&Default::default());
                    let output = layers.encode(
                        device,
                        &mut encoder,
                        &input,
                        origin,
                        size,
                        full,
                        native,
                        recipe.layers.as_ref().unwrap(),
                        &|| false,
                    )?;
                    queue.submit([encoder.finish()]);
                    spatial_check::compare(
                        device,
                        queue,
                        &output,
                        &spatial_check::extract(&expected, origin, size)?,
                        format!(
                            "layer mask pattern={pattern} base={base} variant={variant} origin={origin:?}"
                        ),
                        &mut result,
                    )?;
                    result.regional_cases += usize::from(origin != [0, 0]);
                }
            }
        }
    }
    let point = point::Point::new(device);
    let denoise = multiscale::Multiscale::new(device)?;
    let spatial = spatial::Spatial::new(device);
    let post = post::Post::new(device);
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let gpu = MemoryBudget::new(512 * 1024 * 1024);
    let source = reconstruction::source(full, 4)?;
    let uploaded = admission::Upload::prepare(device, &source, &memory, &gpu, &|| false)?
        .submit(queue, &|| false)?;
    let optics = Arc::new(point::Optics::prepare(
        device,
        full,
        15.,
        &memory,
        &gpu,
        &|| false,
    )?);
    for (i, exposure) in [-1., 0.25, 1.].into_iter().enumerate() {
        let mut recipe = post_check::recipe(8);
        recipe.exposure_ev = exposure;
        recipe.temperature = 8.;
        let advanced = recipe.advanced.as_mut().unwrap();
        advanced.detail = spatial_check::recipe(7).advanced.unwrap().detail;
        advanced.detail.luminance_noise = 25.;
        advanced.detail.chroma_noise = 35.;
        advanced.geometry.vignette = 15.;
        advanced.geometry.defringe = 20.;
        add_layers(&mut recipe, 5 + i);
        let mut expected = source.clone();
        recipe.apply(&mut expected)?;
        let edit = crate::live_edit::LiveEdit {
            recipe,
            input_gains: [1.; 3],
            max_edge: 0,
            revision: 1,
        };
        for origin in [[0, 0], [260, 76]] {
            let request = admission::Request {
                n: 16,
                amounts: [0.25, 0.35],
                threshold: 0.25,
                levels: 3,
                origin,
                size: [257, 193],
            };
            let output = admission::Pending::prepare_inner(
                device,
                &denoise,
                uploaded.clone(),
                request,
                &memory,
                &gpu,
                Some((&point, optics.clone(), &edit)),
                Some((&spatial, full)),
                Some((&post, full)),
                Some((&layers, full)),
                &|| false,
            )?
            .submit(queue, &|| false)?;
            spatial_check::compare(
                device,
                queue,
                &output.buffer,
                &spatial_check::extract(&expected, origin, request.size)?,
                format!("complete masked layer chain exposure={exposure} origin={origin:?}"),
                &mut result,
            )?;
            result.chain_cases += 1;
            result.regional_cases += 1;
        }
    }
    drop((uploaded, optics));
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    ensure!(
        memory.usage().reserved == 0 && gpu.usage().reserved == 0,
        "Layer continuation leaked admission"
    );
    Ok(result)
}
