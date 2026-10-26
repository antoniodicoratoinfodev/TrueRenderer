//! Full canonical point/vignette + denoiser comparisons; no CPU-developed input
//! is supplied to the GPU path, including a source beyond a buffer binding.
use super::*;
use std::{sync::Arc, time::Duration};
use tr_core::{
    budget::MemoryBudget,
    decoder::RawEngine,
    editing::{Advanced, CurvePoint, EditRecipe, detail::Detail},
};

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub cases: usize,
    pub samples: usize,
    pub large_cases: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub maximum_display_error: u8,
    pub admission_peak: u64,
    pub failed_cases: Vec<String>,
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let point = point::Point::new(device);
    let denoise = multiscale::Multiscale::new(device);
    let memory = MemoryBudget::new(1024 * 1024 * 1024);
    let gpu = MemoryBudget::new(1024 * 1024 * 1024);
    let mut result = Check::default();
    for (size, pattern) in [
        ([133, 131], 2),
        ([133, 131], 4),
        ([133, 131], 8),
        ([4097, 3073], 8),
    ] {
        let large = size[0] > 1000;
        let source = reconstruction::source(size, pattern)?;
        let uploaded = admission::Upload::prepare(device, &source, &memory, &gpu, &|| false)?
            .submit(queue, &|| false)?;
        for variant in 0..if large { 1 } else { 6 } {
            let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
            recipe.require_process(3);
            recipe.exposure_ev = [-2., -0.2, 0., 0.25, 1.2, 3.][variant];
            recipe.temperature = [-30., 0., 0., 20., 0., 0.][variant];
            recipe.tint = [15., 0., 0., -10., 0., 0.][variant];
            recipe.saturation = if variant % 2 == 0 { 25. } else { -20. };
            recipe.vibrance = if variant == 3 { 35. } else { 0. };
            if variant == 2 {
                recipe.require_process(4);
                recipe.brightness = 10.;
                recipe.contrast = 12.;
                recipe.shadows = 20.;
                recipe.highlights = -25.;
                recipe.blacks = 8.;
                recipe.whites = -10.;
                recipe.curve = vec![
                    CurvePoint { x: 0., y: 0.05 },
                    CurvePoint { x: 0.45, y: 0.6 },
                    CurvePoint { x: 1., y: 0.95 },
                ];
            }
            let vignette = [-60., 0., 75., 15., 0., 30.][variant];
            let detail = Detail {
                luminance_noise: 25.,
                chroma_noise: 35.,
                threshold: 25.,
                ..Default::default()
            };
            recipe.advanced = Some(Box::new(Advanced {
                detail,
                geometry: tr_core::editing::geometry::Geometry {
                    vignette,
                    ..Default::default()
                },
                ..Default::default()
            }));
            let edit = crate::live_edit::LiveEdit {
                recipe: recipe.clone(),
                input_gains: [1.1, 1., 0.9],
                max_edge: 0,
                revision: variant as u64,
            };
            let optics = Arc::new(point::Optics::prepare(
                device,
                size,
                vignette,
                &memory,
                &gpu,
                &|| false,
            )?);
            let mut expected = source.clone();
            for pixel in &mut expected.pixels {
                for (value, gain) in pixel[..3].iter_mut().zip(edit.input_gains) {
                    *value *= gain;
                }
            }
            recipe.apply(&mut expected)?;
            let (origin, dimensions) = if large {
                ([1601, 1203], [257, 193])
            } else {
                ([3, 5], [123, 117])
            };
            let request = admission::Request {
                n: 16,
                amounts: [0.25, 0.35],
                threshold: 0.25,
                levels: 3,
                origin,
                size: dimensions,
            };
            let output = admission::Pending::prepare_inner(
                device,
                &denoise,
                uploaded.clone(),
                request,
                &memory,
                &gpu,
                Some((&point, optics.clone(), &edit)),
                None,
                None,
                None,
                &|| false,
            )?
            .submit(queue, &|| false)?;
            let expected = LinearImage::new(
                dimensions[0],
                dimensions[1],
                expected
                    .pixels
                    .chunks_exact(size[0] as usize)
                    .skip(origin[1] as usize)
                    .take(dimensions[1] as usize)
                    .flat_map(|row| {
                        row[origin[0] as usize..(origin[0] + dimensions[0]) as usize]
                            .iter()
                            .copied()
                    })
                    .collect(),
            )?;
            let mut compared = benchmark::Check::default();
            benchmark::compare(device, queue, &output.buffer, &expected, &mut compared)?;
            result.samples += expected.pixels.len() * 4;
            result.failures += compared.failures;
            result.maximum_error = result.maximum_error.max(compared.maximum_error);
            result.maximum_display_error = result
                .maximum_display_error
                .max(compared.maximum_display_error);
            if compared.failures > 0 || compared.maximum_display_error > 1 {
                result.failed_cases.push(format!(
                    "size={size:?} pattern={pattern} variant={variant} failures={} display={}",
                    compared.failures, compared.maximum_display_error
                ));
            }
            result.cases += 1;
            result.large_cases += usize::from(large);
            drop((output, optics));
        }
        drop(uploaded);
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })?;
        ensure!(
            memory.usage().reserved == 0 && gpu.usage().reserved == 0,
            "Point pipeline leaked admission"
        );
    }
    result.admission_peak = memory.usage().peak;
    Ok(result)
}
