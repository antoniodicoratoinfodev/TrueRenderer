//! Isolated stage timings on resident native input. Not event-to-frame latency:
//! point/color/detail/geometry/display, source upload and pixel checks are excluded.
use super::*;
use std::time::{Duration, Instant};
use tr_core::{budget::MemoryBudget, editing::detail::multiscale_region_reference};

#[derive(Default, serde::Serialize)]
pub struct Sample {
    pub cpu_first: bool,
    pub amounts: [f32; 2],
    pub cpu_us: f64,
    pub gpu_encode_us: f64,
    pub gpu_submit_wait_us: f64,
    pub gpu_ready_us: f64,
    pub gpu_commands_us: Option<f64>,
}
#[derive(Default, serde::Serialize)]
pub struct Check {
    pub source_size: [u32; 2],
    pub viewport: [u32; 2],
    pub cpu_threads: usize,
    pub warmup_pairs: usize,
    pub measurements: Vec<Sample>,
    pub correctness_cases: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub maximum_display_error: u8,
}

#[allow(clippy::too_many_arguments)]
fn gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &multiscale::Multiscale,
    input: &wgpu::TextureView,
    size: [u32; 2],
    origin: [u32; 2],
    viewport: [u32; 2],
    amounts: [f32; 2],
    memory: &MemoryBudget,
    gpu_budget: &MemoryBudget,
) -> Result<(wgpu::Buffer, Sample)> {
    let mark = crate::timings::mark();
    let mut timer = crate::timings::GpuTimer::new(device, memory, gpu_budget);
    let started = Instant::now();
    let mut encoder = device.create_command_encoder(&Default::default());
    if let Some(timer) = &mut timer {
        timer.begin(&mut encoder, "gpu_denoise_stage");
    }
    let output = pipeline.encode(
        device,
        &mut encoder,
        input::Input::Texture(input),
        size,
        16,
        amounts,
        0.25,
        3,
        false,
        origin,
        viewport,
    )?;
    if let Some(timer) = &timer {
        timer.end(&mut encoder);
        timer.resolve(&mut encoder);
    }
    let commands = encoder.finish();
    let gpu_encode_us = started.elapsed().as_secs_f64() * 1e6;
    let submitted = Instant::now();
    let submission = queue.submit([commands]);
    if let Some(timer) = timer {
        timer.submitted(queue);
    }
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(30)),
    })?;
    let gpu_submit_wait_us = submitted.elapsed().as_secs_f64() * 1e6;
    let gpu_ready_us = started.elapsed().as_secs_f64() * 1e6;
    let gpu_commands_us = crate::timings::samples_since(mark)
        .iter()
        .find(|s| s.phase == "gpu_denoise_stage")
        .map(|s| s.duration_us);
    Ok((
        output,
        Sample {
            amounts,
            gpu_encode_us,
            gpu_submit_wait_us,
            gpu_ready_us,
            gpu_commands_us,
            ..Default::default()
        },
    ))
}

pub(super) fn compare(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    actual: &wgpu::Buffer,
    expected: &LinearImage,
    result: &mut Check,
) -> Result<()> {
    let bytes = expected.pixels.len() as u64 * 16;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(actual, 0, &readback, 0, bytes);
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
    let trace = std::env::var("TR_GPU_COMPARE_TRACE").is_ok_and(|v| v == "1");
    let inspected: Vec<usize> = std::env::var("TR_GPU_COMPARE_PIXELS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|v| v.parse().ok())
        .collect();
    for (index, (actual, expected)) in mapped
        .as_chunks::<16>()
        .0
        .iter()
        .zip(&expected.pixels)
        .enumerate()
    {
        let actual: Pixel = std::array::from_fn(|i| {
            f32::from_le_bytes(actual[i * 4..i * 4 + 4].try_into().unwrap())
        });
        if trace && inspected.contains(&index) {
            eprintln!("Pixel {index} actual={actual:?} expected={expected:?}");
        }
        for (channel, (a, e)) in actual.iter().zip(expected).enumerate() {
            let error = (a - e).abs();
            if trace && error > 1e-5 + 1e-4 * e.abs() && result.failures < 12 {
                eprintln!(
                    "GPU difference pixel={index} channel={channel} actual={a:?} expected={e:?} error={error:?}"
                );
            }
            result.failures += usize::from(!a.is_finite() || error > 1e-5 + 1e-4 * e.abs());
            result.maximum_error = result.maximum_error.max(error);
        }
        let a = tr_core::color::display_pixel(actual, 119. / 255.);
        let e = tr_core::color::display_pixel(*expected, 119. / 255.);
        for (a, e) in a.into_iter().zip(e) {
            result.maximum_display_error = result.maximum_display_error.max(a.abs_diff(e));
        }
    }
    drop(mapped);
    readback.unmap();
    result.correctness_cases += 1;
    Ok(())
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let size = [4097, 3073];
    let origin = [1601, 1203];
    let viewport = [855, 596];
    let source = reconstruction::source(size, 8)?;
    let pipeline = multiscale::Multiscale::new(device)?;
    let mut upload = device.create_command_encoder(&Default::default());
    let texture = input::upload(device, &mut upload, &source)?;
    let submission = queue.submit([upload.finish()]);
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(30)),
    })?;
    let view = texture.create_view(&Default::default());
    let memory = MemoryBudget::new(4 * 1024 * 1024);
    let gpu_budget = MemoryBudget::new(4 * 1024 * 1024);
    let mut result = Check {
        source_size: size,
        viewport,
        cpu_threads: tr_core::compute::threads(),
        warmup_pairs: 4,
        ..Default::default()
    };
    for i in 0..20 {
        let amounts = [0.25 + (i % 5) as f32 * 0.07, 0.35 + (i % 3) as f32 * 0.11];
        let cpu = || -> Result<_> {
            let started = Instant::now();
            let output = multiscale_region_reference(&source, 1., origin, viewport, amounts, 0.25)?;
            Ok((output, started.elapsed().as_secs_f64() * 1e6))
        };
        let gpu = || {
            gpu(
                device,
                queue,
                &pipeline,
                &view,
                size,
                origin,
                viewport,
                amounts,
                &memory,
                &gpu_budget,
            )
        };
        let ((expected, cpu_us), (actual, mut sample)) = if i % 2 == 0 {
            (cpu()?, gpu()?)
        } else {
            let g = gpu()?;
            (cpu()?, g)
        };
        if i >= result.warmup_pairs {
            sample.cpu_us = cpu_us;
            sample.cpu_first = i % 2 == 0;
            result.measurements.push(sample);
            if i == result.warmup_pairs || i == 19 {
                compare(device, queue, &actual, &expected, &mut result)?;
            }
        }
    }
    Ok(result)
}
