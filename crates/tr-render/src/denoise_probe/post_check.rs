//! Color/mask references use the independent CPU recipe and original input guides.
use super::*;
use std::{sync::Arc, time::Duration};
use tr_core::{
    budget::MemoryBudget,
    decoder::RawEngine,
    editing::{
        Advanced, EditRecipe,
        color::{Band, Color, Grade},
        masks::{Mask, Shape},
    },
};
pub(super) type Check = spatial_check::Check;

pub(super) fn recipe(variant: usize) -> EditRecipe {
    let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
    recipe.require_process(3);
    let mut advanced = Advanced::default();
    if [1, 2, 8, 9].contains(&variant) {
        advanced.color = Color {
            bands: std::array::from_fn(|i| Band {
                hue: (i as f32 - 3.) * 20.,
                saturation: 60. - i as f32 * 17.,
                luminance: i as f32 * 13. - 40.,
            }),
            grading: [
                Grade {
                    hue: 27.,
                    amount: 30.,
                },
                Grade {
                    hue: 230.,
                    amount: 50.,
                },
                Grade {
                    hue: 325.,
                    amount: 20.,
                },
            ],
            monochrome: variant == 2,
            rgb_midtones: [70., -80., 15.],
        };
    }
    let shapes = [
        Shape::Radial,
        Shape::Linear,
        Shape::Brush,
        Shape::Luminance,
        Shape::Hue,
    ];
    for (i, shape) in shapes.into_iter().enumerate() {
        if variant != i + 3 && variant != 8 && variant != 9 {
            continue;
        }
        advanced.masks.push(Mask {
            shape,
            center: [0.35, 0.65],
            radius: 0.35,
            feather: 0.7,
            angle: -71.,
            invert: variant == 9,
            interval: [0.2, 0.8],
            points: vec![[0.1, 0.25], [0.5, 0.35], [0.8, 0.9], [0.4, 0.6]],
            breaks: vec![2],
            exposure: 0.4 - i as f32 * 0.2,
            warmth: 35. - i as f32 * 17.,
            saturation: -60. + i as f32 * 25.,
            ..Default::default()
        });
    }
    if variant == 9 {
        advanced.masks.extend([
            Mask {
                enabled: false,
                exposure: 5.,
                ..Default::default()
            },
            Mask {
                shape: Shape::Brush,
                exposure: -4.,
                invert: false,
                ..Default::default()
            },
            Mask {
                shape: Shape::Radial,
                ..Default::default()
            },
        ]);
    }
    recipe.advanced = Some(Box::new(advanced));
    recipe
}

#[allow(clippy::too_many_arguments)]
fn direct(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    post: &post::Post,
    source: &LinearImage,
    native: [u32; 2],
    base: u32,
    recipe: EditRecipe,
    label: &str,
    result: &mut Check,
) -> Result<()> {
    let full = [source.width, source.height];
    let mut expected = source.clone();
    recipe.apply_preview(&mut expected, native, base)?;
    let edit = crate::live_edit::LiveEdit {
        recipe,
        input_gains: [1.; 3],
        max_edge: 0,
        revision: 1,
    };
    for (origin, size) in [([0, 0], full), ([17, 19], [full[0] - 37, full[1] - 43])] {
        let input = spatial_check::extract(source, origin, size)?;
        let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&input.pixels),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let output = post.encode(
            device,
            &mut encoder,
            &input,
            origin,
            size,
            full,
            native,
            &edit,
            &|| false,
        )?;
        queue.submit([encoder.finish()]);
        spatial_check::compare(
            device,
            queue,
            &output,
            &spatial_check::extract(&expected, origin, size)?,
            format!("{label} base={base} origin={origin:?}"),
            result,
        )?;
        result.regional_cases += usize::from(origin != [0, 0]);
        result.large_cases += usize::from(full[0] > 1000);
    }
    Ok(())
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let post = post::Post::new(device);
    let mut result = Check::default();
    let size = [517, 269];
    for pattern in [0, 2, 4, 8] {
        let source = reconstruction::source(size, pattern)?;
        for base in [0, 2] {
            let native = size.map(|v| v * (1 << base) - ((1 << base) - 1));
            for variant in 0..10 {
                direct(
                    device,
                    queue,
                    &post,
                    &source,
                    native,
                    base,
                    recipe(variant),
                    &format!("pattern={pattern} variant={variant}"),
                    &mut result,
                )?;
            }
        }
    }
    let large = reconstruction::source([1297, 865], 4)?;
    direct(
        device,
        queue,
        &post,
        &large,
        [1297, 865],
        0,
        recipe(8),
        "large mixed masks",
        &mut result,
    )?;
    let small = reconstruction::source([133, 131], 4)?;
    let mut stress = recipe(9);
    let a = stress.advanced.as_mut().unwrap();
    a.masks.resize_with(16, Mask::default);
    for (i, m) in a.masks.iter_mut().enumerate() {
        m.shape = Shape::Brush;
        m.enabled = true;
        m.exposure = (i as f32 - 7.) * 0.02;
        m.points = (0..32)
            .map(|j| {
                [
                    ((j * 17 + i) % 101) as f32 / 100.,
                    ((j * 13 + i * 3) % 101) as f32 / 100.,
                ]
            })
            .collect();
        m.breaks = vec![1, 8, 17];
    }
    direct(
        device,
        queue,
        &post,
        &small,
        [133, 131],
        0,
        stress,
        "16 masks 512 points",
        &mut result,
    )?;

    let point = point::Point::new(device);
    let denoise = multiscale::Multiscale::new(device);
    let spatial = spatial::Spatial::new(device);
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let gpu = MemoryBudget::new(512 * 1024 * 1024);
    let source = reconstruction::source(size, 4)?;
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
    for (i, exposure) in [-1., 0.25, 1.].into_iter().enumerate() {
        let mut recipe = recipe(8 + i % 2);
        recipe.exposure_ev = exposure;
        recipe.temperature = 8.;
        let advanced = recipe.advanced.as_mut().unwrap();
        advanced.detail = spatial_check::recipe(7).advanced.unwrap().detail;
        advanced.detail.luminance_noise = 25.;
        advanced.detail.chroma_noise = 35.;
        advanced.geometry.vignette = 15.;
        advanced.geometry.defringe = 20.;
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
                Some((&spatial, size)),
                Some((&post, size)),
                None,
                &|| false,
            )?
            .submit(queue, &|| false)?;
            spatial_check::compare(
                device,
                queue,
                &output.buffer,
                &spatial_check::extract(&expected, origin, request.size)?,
                format!("complete chain exposure={exposure} origin={origin:?}"),
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
        "Post-detail chain leaked admission"
    );
    Ok(result)
}
