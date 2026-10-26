//! Compare GPU shrinkage, inverse transform and deterministic regional gather
//! against the canonical CPU filter, including independent GPU global models.
use super::*;
use std::time::Duration;
use tr_core::editing::detail::spectral_reference;

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub cases: usize,
    pub regional_cases: usize,
    pub texture_cases: usize,
    pub samples: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub worst_ratio: f32,
    pub maximum_display_error: u8,
    pub model_samples: usize,
    pub model_failures: usize,
    pub model_band_mismatches: usize,
    pub model_maximum_error: f32,
    pub model_worst_ratio: f32,
}

pub(super) fn source(size: [u32; 2], pattern: u32) -> Result<LinearImage> {
    LinearImage::new(
        size[0],
        size[1],
        (0..size[0] * size[1])
            .map(|i| {
                let x = (i % size[0]) as f32 / size[0] as f32;
                let mut random = i.wrapping_mul(747796405).wrapping_add(2891336453);
                random = ((random >> ((random >> 28) + 4)) ^ random).wrapping_mul(277803737);
                let noise = ((random >> 22) ^ random) as f64 / u32::MAX as f64 - 0.5;
                let noise = noise as f32;
                let alpha = match pattern {
                    6 => 0.,
                    4 if i % 53 == 0 => 0.,
                    4 | 5 => [0.01, 0.5, 1.][i as usize % 3],
                    _ => 1.,
                };
                let rgb = match pattern {
                    0 => [0.3; 3],
                    1 => [if x < 0.5 { 0.1 } else { 0.8 } + noise * 0.02; 3],
                    2 => [x * 8. - 2. + noise * 0.2, x * 4. - 1., x * 6. + noise * 0.3],
                    3 => [
                        0.2 + noise * 0.0002,
                        0.3 - noise * 0.0003,
                        0.4 + noise * 0.0004,
                    ],
                    _ => [
                        0.1 + x * 0.6 + noise * 0.03,
                        0.2 + x * 0.5 - noise * 0.05,
                        0.3 + x * 0.4 + noise * 0.07,
                    ],
                };
                [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
            })
            .collect(),
    )
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let filter = super::filter::Filter::new(device);
    let mut result = Check::default();
    for n in [4_usize, 8, 16] {
        for size in [[1, 1], [17, 13], [133, 131], [517, 269]] {
            for pattern in 0..9 {
                let image = source(size, pattern)?;
                let (amounts, threshold) = match pattern {
                    0 => ([0.8, 0.8], 0.),
                    1 => ([1., 0.], 1.),
                    2 => ([0., 1.], 0.5),
                    3 => ([0.75, 0.85], 0.31),
                    4 => ([0.9, 0.6], 0.),
                    7 => ([0., 0.], 0.5),
                    8 => ([0.9, 0.8], 0.7),
                    _ => ([1., 1.], 1.),
                };
                let mut regions = vec![([0, 0], size)];
                if size[0] > 100 {
                    regions.push(([3, 5], [size[0] - 7, size[1] - 11]));
                }
                for (origin, dimensions) in regions {
                    let reference = spectral_reference(
                        &image,
                        n,
                        origin,
                        dimensions,
                        amounts,
                        threshold,
                        pattern == 3 || pattern == 8,
                    )?;
                    let mut reference_model = vec![[
                        reference.levels.len() as f32,
                        amounts[0],
                        amounts[1],
                        threshold,
                    ]];
                    reference_model.extend(reference.levels.iter().map(|v| [*v, 0., 0., 0.]));
                    reference_model.extend(&reference.sigma);
                    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(&image.pixels),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                    for texture_mode in [false, true] {
                        let mut encoder = device.create_command_encoder(&Default::default());
                        let texture = texture_mode
                            .then(|| input::upload(device, &mut encoder, &image))
                            .transpose()?;
                        let view = texture.as_ref().map(|t| t.create_view(&Default::default()));
                        let source = view
                            .as_ref()
                            .map_or(input::Input::Buffer(&input), input::Input::Texture);
                        let (output, model) = filter.encode(
                            device,
                            &mut encoder,
                            source,
                            size,
                            n,
                            amounts,
                            threshold,
                            pattern == 3 || pattern == 8,
                            origin,
                            dimensions,
                        )?;
                        let model_bytes = super::models::output_bytes(n);
                        let output_bytes = reference.output.pixels.len() as u64 * 16;
                        let readback = device.create_buffer(&wgpu::BufferDescriptor {
                            label: None,
                            size: output_bytes + model_bytes,
                            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output_bytes);
                        encoder.copy_buffer_to_buffer(
                            &model,
                            0,
                            &readback,
                            output_bytes,
                            model_bytes,
                        );
                        let submission = queue.submit([encoder.finish()]);
                        let (tx, rx) = std::sync::mpsc::sync_channel(1);
                        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                            let _ = tx.send(r);
                        });
                        device.poll(wgpu::PollType::Wait {
                            submission_index: Some(submission),
                            timeout: Some(Duration::from_secs(30)),
                        })?;
                        rx.recv_timeout(Duration::from_secs(30))??;
                        let mapped = readback.slice(..).get_mapped_range()?;
                        let values: Vec<f32> = mapped
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|v| f32::from_le_bytes(*v))
                            .collect();
                        let actual_model = &values[output_bytes as usize / 4..];
                        result.model_band_mismatches +=
                            usize::from(actual_model[0] != reference.levels.len() as f32);
                        for (a, e) in actual_model.iter().zip(reference_model.iter().flatten()) {
                            let error = (a - e).abs();
                            let tolerance = 1e-5 + 1e-4 * e.abs();
                            result.model_samples += 1;
                            result.model_failures +=
                                usize::from(!a.is_finite() || error > tolerance);
                            result.model_maximum_error = result.model_maximum_error.max(error);
                            result.model_worst_ratio =
                                result.model_worst_ratio.max(error / tolerance);
                        }
                        for (actual, expected) in values
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(&reference.output.pixels)
                        {
                            for (a, e) in actual.iter().zip(expected) {
                                let error = (a - e).abs();
                                let tolerance = 1e-5 + 1e-4 * e.abs();
                                result.samples += 1;
                                result.failures += usize::from(!a.is_finite() || error > tolerance);
                                result.maximum_error = result.maximum_error.max(error);
                                result.worst_ratio = result.worst_ratio.max(error / tolerance);
                            }
                            let a = tr_core::color::display_pixel(*actual, 119. / 255.);
                            let e = tr_core::color::display_pixel(*expected, 119. / 255.);
                            for (a, e) in a.into_iter().zip(e) {
                                result.maximum_display_error =
                                    result.maximum_display_error.max(a.abs_diff(e));
                            }
                        }
                        drop(mapped);
                        readback.unmap();
                        result.cases += 1;
                        result.regional_cases += usize::from(origin != [0, 0]);
                        result.texture_cases += usize::from(texture_mode);
                    }
                }
            }
        }
    }
    Ok(result)
}
