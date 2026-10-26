//! Native-source pipeline timing before geometry/display integration. Source upload,
//! optics/pipeline preparation and pixel readback are outside the measured pairs.
use super::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tr_core::{
    budget::MemoryBudget, editing::regional::Processor, preview::PreviewRequest,
    provider::ImageLevels, resample::Region,
};

#[derive(serde::Serialize)]
pub struct Sample {
    pub control: &'static str,
    pub checkpoint: bool,
    pub schedule: &'static str,
    pub value: f32,
    pub cpu_first: bool,
    pub cpu_us: Option<f64>,
    pub gpu_encode_us: f64,
    pub gpu_submit_wait_us: f64,
    pub gpu_ready_us: f64,
}
#[derive(Default, serde::Serialize)]
pub struct Check {
    pub source_size: [u32; 2],
    pub viewport: [u32; 2],
    pub origin: [u32; 2],
    pub cpu_threads: usize,
    pub warmup_pairs_per_control: usize,
    pub gpu_burst_warmup_frames: usize,
    pub gpu_burst_measured_frames: usize,
    pub measurements: Vec<Sample>,
    pub correctness_cases: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub maximum_display_error: u8,
    pub admitted_memory_peak: u64,
    pub admitted_gpu_peak: u64,
    pub final_memory_reserved: u64,
    pub final_gpu_reserved: u64,
    pub checkpoint_hits: usize,
    pub checkpoint_misses: usize,
}
pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let size = [5184, 3456];
    let origin = [2112, 1429];
    let viewport = [855, 596];
    let source = reconstruction::source(size, 8)?;
    let image = ImageLevels::from_source(source, PreviewRequest::full())?;
    let memory = MemoryBudget::new(2 * 1024 * 1024 * 1024);
    let gpu = MemoryBudget::new(2 * 1024 * 1024 * 1024);
    let uploaded = admission::Upload::prepare(device, image.source(), &memory, &gpu, &|| false)?
        .submit(queue, &|| false)?;
    let optics = Arc::new(point::Optics::prepare(
        device,
        size,
        15.,
        &memory,
        &gpu,
        &|| false,
    )?);
    let point = point::Point::new(device);
    let denoise = multiscale::Multiscale::new(device);
    let spatial = spatial::Spatial::new(device);
    let post = post::Post::new(device);
    let layers = layers::Layers::new(device);
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    let mut result = Check {
        source_size: size,
        viewport,
        origin,
        cpu_threads: tr_core::compute::threads(),
        warmup_pairs_per_control: 4,
        gpu_burst_warmup_frames: 32,
        gpu_burst_measured_frames: 32,
        ..Default::default()
    };
    for (use_cache, control) in [
        (false, "exposure"),
        (false, "texture"),
        (true, "exposure"),
        (true, "texture"),
    ] {
        let checkpoint = checkpoint::Checkpoint::default();
        let mut processor = Processor::new(MemoryBudget::new(4 * 1024 * 1024 * 1024));
        let mut recipe = post_check::recipe(8);
        recipe.exposure_ev = 0.25;
        recipe.temperature = 8.;
        let a = recipe.advanced.as_mut().unwrap();
        a.detail = spatial_check::recipe(7).advanced.unwrap().detail;
        a.detail.luminance_noise = 25.;
        a.detail.chroma_noise = 35.;
        a.geometry.vignette = 15.;
        a.geometry.defringe = 20.;
        layers_check::add_layers(&mut recipe, 5);
        let mut retained = Vec::new();
        for frame in 0_u64..84 {
            let burst = frame >= 20;
            let index = if burst { frame - 20 } else { frame };
            let value = if control == "exposure" {
                -0.5 + (index % 20) as f32 * 0.075
            } else {
                -40. + (index % 20) as f32 * 5.
            };
            if control == "exposure" {
                recipe.exposure_ev = value;
            } else {
                recipe.advanced.as_mut().unwrap().detail.texture = value;
            }
            let edit = crate::live_edit::LiveEdit {
                recipe: recipe.clone(),
                input_gains: [1.; 3],
                max_edge: 0,
                revision: frame,
            };
            let request = admission::Request {
                n: 16,
                amounts: [0.25, 0.35],
                threshold: 0.25,
                levels: 3,
                origin,
                size: viewport,
            };
            let cpu_first = !burst && frame.is_multiple_of(2);
            let cpu = || -> Result<(LinearImage, f64)> {
                let started = Instant::now();
                let output = processor.render(
                    &image,
                    &recipe,
                    [1.; 3],
                    Region {
                        size: viewport,
                        origin: origin.map(f64::from),
                        step: [1.; 2],
                    },
                )?;
                Ok((output, started.elapsed().as_secs_f64() * 1e6))
            };
            let gpu_frame = || -> Result<(Arc<admission::Output>, [f64; 3])> {
                let started = Instant::now();
                let pending = admission::Pending::prepare_cached(
                    device,
                    &denoise,
                    uploaded.clone(),
                    request,
                    &memory,
                    &gpu,
                    Some((&point, optics.clone(), &edit)),
                    Some((&spatial, size)),
                    Some((&post, size)),
                    Some((&layers, size)),
                    use_cache.then_some(&checkpoint),
                    &|| false,
                )?;
                let encode = started.elapsed().as_secs_f64() * 1e6;
                let submitted = Instant::now();
                let output = pending.submit(queue, &|| false)?;
                device.poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(30)),
                })?;
                Ok((
                    output,
                    [
                        encode,
                        submitted.elapsed().as_secs_f64() * 1e6,
                        started.elapsed().as_secs_f64() * 1e6,
                    ],
                ))
            };
            let mut cpu = cpu;
            let ((expected, cpu_us), (output, times)) = if burst {
                ((None, None), gpu_frame()?)
            } else if cpu_first {
                let (expected, time) = cpu()?;
                let g = gpu_frame()?;
                ((Some(expected), Some(time)), g)
            } else {
                let g = gpu_frame()?;
                let (expected, time) = cpu()?;
                ((Some(expected), Some(time)), g)
            };
            if !burst && (frame == 4 || frame == 19) {
                let mut compared = benchmark::Check::default();
                benchmark::compare(
                    device,
                    queue,
                    &output.buffer,
                    expected.as_ref().unwrap(),
                    &mut compared,
                )?;
                result.correctness_cases += 1;
                result.failures += compared.failures;
                result.maximum_error = result.maximum_error.max(compared.maximum_error);
                result.maximum_display_error = result
                    .maximum_display_error
                    .max(compared.maximum_display_error);
            }
            if burst && (index == 32 || index == 63) {
                retained.push((output.clone(), recipe.clone()));
            }
            if index >= if burst { 32 } else { 4 } {
                result.measurements.push(Sample {
                    control,
                    checkpoint: use_cache,
                    schedule: if burst { "gpu_burst" } else { "alternating" },
                    value,
                    cpu_first,
                    cpu_us,
                    gpu_encode_us: times[0],
                    gpu_submit_wait_us: times[1],
                    gpu_ready_us: times[2],
                });
            }
        }
        // Reference work and readback occur after the sustained sequence, so
        // they cannot insert CPU pauses into the measured GPU cadence.
        for (output, recipe) in retained {
            let expected = processor.render(
                &image,
                &recipe,
                [1.; 3],
                Region {
                    size: viewport,
                    origin: origin.map(f64::from),
                    step: [1.; 2],
                },
            )?;
            let mut compared = benchmark::Check::default();
            benchmark::compare(device, queue, &output.buffer, &expected, &mut compared)?;
            result.correctness_cases += 1;
            result.failures += compared.failures;
            result.maximum_error = result.maximum_error.max(compared.maximum_error);
            result.maximum_display_error = result
                .maximum_display_error
                .max(compared.maximum_display_error);
        }
        result.checkpoint_hits += checkpoint.hits.load(std::sync::atomic::Ordering::Relaxed);
        result.checkpoint_misses += checkpoint.misses.load(std::sync::atomic::Ordering::Relaxed);
        checkpoint.clear();
    }
    drop((uploaded, optics));
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    result.admitted_memory_peak = memory.usage().peak;
    result.admitted_gpu_peak = gpu.usage().peak;
    result.final_memory_reserved = memory.usage().reserved;
    result.final_gpu_reserved = gpu.usage().reserved;
    ensure!(
        result.final_memory_reserved == 0 && result.final_gpu_reserved == 0,
        "Measured chain leaked admission"
    );
    Ok(result)
}
