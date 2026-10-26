//! Independent full-graph references for spatial stages and their regional halos.
use super::*;
use std::{sync::Arc, time::Duration};
use tr_core::{
    budget::MemoryBudget,
    decoder::RawEngine,
    editing::{Advanced, EditRecipe, detail::Detail},
};

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub cases: usize,
    pub regional_cases: usize,
    pub chain_cases: usize,
    pub large_cases: usize,
    pub samples: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub maximum_display_error: u8,
    pub failed_cases: Vec<String>,
}
pub(super) fn recipe(variant: usize) -> EditRecipe {
    let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
    recipe.require_process(3);
    let mut advanced = Advanced::default();
    let d = &mut advanced.detail;
    match variant {
        0 => d.texture = -60.,
        1 => d.texture = 60.,
        2 => d.clarity = -60.,
        3 => d.clarity = 60.,
        4 => {
            d.sharpen = 35.;
            d.radius = 1.4;
            d.threshold = 75.;
        }
        5 => {
            d.sharpen = 150.;
            d.radius = 3.;
        }
        6 => advanced.geometry.defringe = 100.,
        _ => {
            d.texture = 15.;
            d.clarity = 12.;
            d.sharpen = 35.;
            d.radius = 1.4;
            d.threshold = 25.;
            advanced.geometry.defringe = 20.;
        }
    }
    recipe.advanced = Some(Box::new(advanced));
    recipe
}
pub(super) fn extract(
    image: &LinearImage,
    origin: [u32; 2],
    size: [u32; 2],
) -> Result<LinearImage> {
    LinearImage::new(
        size[0],
        size[1],
        image
            .pixels
            .chunks_exact(image.width as usize)
            .skip(origin[1] as usize)
            .take(size[1] as usize)
            .flat_map(|row| {
                row[origin[0] as usize..(origin[0] + size[0]) as usize]
                    .iter()
                    .copied()
            })
            .collect(),
    )
}
pub(super) fn compare(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    output: &wgpu::Buffer,
    expected: &LinearImage,
    label: String,
    result: &mut Check,
) -> Result<()> {
    let mut checked = benchmark::Check::default();
    benchmark::compare(device, queue, output, expected, &mut checked)?;
    result.cases += 1;
    result.samples += expected.pixels.len() * 4;
    result.failures += checked.failures;
    result.maximum_error = result.maximum_error.max(checked.maximum_error);
    result.maximum_display_error = result
        .maximum_display_error
        .max(checked.maximum_display_error);
    if checked.failures > 0 || checked.maximum_display_error > 1 {
        result.failed_cases.push(format!(
            "{label} failures={} display={}",
            checked.failures, checked.maximum_display_error
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn direct(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &spatial::Spatial,
    size: [u32; 2],
    pattern: u32,
    base: u32,
    variant: usize,
    result: &mut Check,
) -> Result<()> {
    let source = reconstruction::source(size, pattern)?;
    let recipe = recipe(variant);
    let advanced = recipe.advanced.as_ref().unwrap();
    let native = size.map(|v| v * (1 << base) - ((1 << base) - 1));
    let mut expected = source.clone();
    recipe.apply_preview(&mut expected, native, base)?;
    for (origin, dimensions) in [([0, 0], size), ([17, 19], [size[0] - 37, size[1] - 43])] {
        let radius = spatial::support(&advanced.detail, advanced.geometry.defringe, native, size)?;
        let start = origin.map(|v: u32| v.saturating_sub(radius));
        let end: [u32; 2] = std::array::from_fn(|a| {
            (origin[a] + dimensions[a])
                .saturating_add(radius)
                .min(size[a])
        });
        let work_size = std::array::from_fn(|a| end[a] - start[a]);
        let input = extract(&source, start, work_size)?;
        let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&input.pixels),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let output = pipeline.encode(
            device,
            &mut encoder,
            &input,
            work_size,
            [
                origin[0] - start[0],
                origin[1] - start[1],
                dimensions[0],
                dimensions[1],
            ],
            &advanced.detail,
            advanced.geometry.defringe,
            native,
            size,
            &|| false,
        )?;
        queue.submit([encoder.finish()]);
        compare(
            device,
            queue,
            &output,
            &extract(&expected, origin, dimensions)?,
            format!(
                "size={size:?} pattern={pattern} base={base} variant={variant} origin={origin:?}"
            ),
            result,
        )?;
        result.regional_cases += usize::from(origin != [0, 0]);
        result.large_cases += usize::from(size[0] > 1000);
    }
    Ok(())
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let spatial = spatial::Spatial::new(device);
    let mut result = Check::default();
    for size in [[133, 131], [517, 269]] {
        for pattern in [0, 2, 4, 8] {
            for base in [0, 2] {
                for variant in 0..8 {
                    direct(
                        device,
                        queue,
                        &spatial,
                        size,
                        pattern,
                        base,
                        variant,
                        &mut result,
                    )?;
                }
            }
        }
    }
    direct(device, queue, &spatial, [1297, 865], 4, 0, 7, &mut result)?;
    let point = point::Point::new(device);
    let denoise = multiscale::Multiscale::new(device);
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let gpu = MemoryBudget::new(512 * 1024 * 1024);
    let size = [517, 269];
    let source = reconstruction::source(size, 4)?;
    let mut recipe = recipe(7);
    recipe.exposure_ev = 0.25;
    recipe.temperature = 8.;
    let advanced = recipe.advanced.as_mut().unwrap();
    advanced.detail = Detail {
        luminance_noise: 25.,
        chroma_noise: 35.,
        ..advanced.detail.clone()
    };
    advanced.geometry.vignette = 15.;
    let edit = crate::live_edit::LiveEdit {
        recipe: recipe.clone(),
        input_gains: [1.; 3],
        max_edge: 0,
        revision: 1,
    };
    let mut expected = source.clone();
    recipe.apply(&mut expected)?;
    let uploaded = admission::Upload::prepare(device, &source, &memory, &gpu, &|| false)?
        .submit(queue, &|| false)?;
    let optics = Arc::new(point::Optics::prepare(
        device,
        size,
        15.,
        &memory,
        &gpu,
        &|| false,
    )?);
    for origin in [[0, 0], [119, 35], [260, 76], [17, 19]] {
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
            Some((&spatial, size)),
            None,
            None,
            &|| false,
        )?
        .submit(queue, &|| false)?;
        compare(
            device,
            queue,
            &output.buffer,
            &extract(&expected, origin, request.size)?,
            format!("point/noise/spatial chain origin={origin:?}"),
            &mut result,
        )?;
        result.regional_cases += 1;
        result.chain_cases += 1;
    }
    drop((uploaded, optics));
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    ensure!(
        memory.usage().reserved == 0 && gpu.usage().reserved == 0,
        "Spatial chain leaked admission"
    );
    Ok(result)
}
