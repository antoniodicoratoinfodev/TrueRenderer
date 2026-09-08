//! Qualification of the actual separable viewport kernel against scalar CPU.
//! This path cannot be enabled for presentation until the numerical gate passes.
use anyhow::{Result, ensure};
use eframe::wgpu::{self, util::DeviceExt};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use tr_core::{
    color::LinearImage,
    resample::{Region, Taps},
};

#[derive(serde::Serialize)]
pub struct ComputeCheck {
    pub cases: usize,
    pub display_channels: usize,
    pub display_failures: usize,
    pub display_max_error: u8,
    pub samples: usize,
    pub failures: usize,
    pub max_error: f32,
    pub worst_ratio: f32,
    pub cpu_seconds: f64,
    pub gpu_seconds: f64,
}
pub(crate) fn taps(rows: &[Taps]) -> (Vec<u8>, Vec<u8>) {
    let mut ranges = Vec::new();
    let mut weights = Vec::new();
    for row in rows {
        ranges.extend_from_slice(&((weights.len() / 8) as u32).to_le_bytes());
        ranges.extend_from_slice(&(row.len() as u32).to_le_bytes());
        for (index, weight) in row {
            weights.extend_from_slice(&(*index as u32).to_le_bytes());
            weights.extend_from_slice(&(*weight as f32).to_le_bytes());
        }
    }
    (ranges, weights)
}
fn compute(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &LinearImage,
    region: Region,
    opaque: bool,
) -> Result<Vec<[f32; 4]>> {
    let (xs, ys) = tr_core::resample::coefficients([source.width, source.height], region, opaque)?;
    let min_y = ys.iter().flatten().map(|(y, _)| *y).min().unwrap() as u32;
    let max_y = ys.iter().flatten().map(|(y, _)| *y).max().unwrap() as u32;
    let rows = max_y - min_y + 1;
    let output_bytes = region.size[0] as u64 * region.size[1] as u64 * 16;
    ensure!(
        source.pixels.len() as u64 * 16 <= device.limits().max_storage_buffer_binding_size,
        "Source exceeds device binding limit"
    );
    let storage = |bytes: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TR filter qualification input"),
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let params: Vec<_> = [
        source.width,
        region.size[0],
        region.size[1],
        min_y,
        rows,
        opaque as u32,
        0,
        0,
    ]
    .iter()
    .flat_map(|v| v.to_le_bytes())
    .collect();
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &params,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bytes: Vec<_> = source
        .pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let input = storage(&bytes);
    let buffer = |size, usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("TR filter qualification scratch"),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let horizontal = buffer(
        region.size[0] as u64 * rows as u64 * 16,
        wgpu::BufferUsages::STORAGE,
    );
    let output = buffer(
        output_bytes,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let readback = buffer(
        output_bytes,
        wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    );
    let (xr, xt) = taps(&xs);
    let (yr, yt) = taps(&ys);
    let xr = storage(&xr);
    let xt = storage(&xt);
    let yr = storage(&yr);
    let yt = storage(&yt);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("TR separable filter qualification"),
        source: wgpu::ShaderSource::Wgsl(include_str!("preview_compute.wgsl").into()),
    });
    // Explicit common layout keeps both entry points on the same bound buffers.
    let entries: Vec<_> = (0..8)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 0 {
                    wgpu::BufferBindingType::Uniform
                } else {
                    wgpu::BufferBindingType::Storage {
                        read_only: binding != 2 && binding != 3,
                    }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = |entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let h = pipeline("horizontal_pass");
    let v = pipeline("vertical_pass");
    let buffers = [&uniform, &input, &horizontal, &output, &xr, &xt, &yr, &yt];
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &entries,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    for (pipeline, height) in [(&h, rows), (&v, region.size[1])] {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(region.size[0].div_ceil(8), height.div_ceil(8), 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output_bytes);
    let submission = queue.submit([encoder.finish()]);
    let (tx, rx) = mpsc::sync_channel(1);
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(10)),
    })?;
    rx.recv_timeout(Duration::from_secs(10))??;
    let mapped = readback.slice(..).get_mapped_range()?;
    let values = mapped
        .as_chunks::<16>()
        .0
        .iter()
        .map(|raw| {
            std::array::from_fn(|c| f32::from_le_bytes(raw[c * 4..c * 4 + 4].try_into().unwrap()))
        })
        .collect();
    drop(mapped);
    readback.unmap();
    Ok(values)
}
pub fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<ComputeCheck> {
    ensure!(
        device.limits().max_storage_buffers_per_shader_stage >= 7,
        "Device lacks filter bindings"
    );
    let mut result = ComputeCheck {
        cases: 0,
        display_channels: 0,
        display_failures: 0,
        display_max_error: 0,
        samples: 0,
        failures: 0,
        max_error: 0.,
        worst_ratio: 0.,
        cpu_seconds: 0.,
        gpu_seconds: 0.,
    };
    let memory = tr_core::budget::MemoryBudget::new(512 * 1024 * 1024);
    let gpu = tr_core::budget::MemoryBudget::new(256 * 1024 * 1024);
    let mut renderer = crate::resident_compute::GpuFilter::new(device.clone());
    // SDR, transparent edges and out-of-range linear highlights. No RGB clamp.
    for (opaque, range) in [(true, 1.), (false, 1.), (true, 1000.)] {
        let source = LinearImage::new(
            513,
            257,
            (0..513 * 257)
                .map(|i| {
                    let x = i % 513;
                    let y = i / 513;
                    let alpha = if opaque { 1. } else { (x % 17) as f32 / 16. };
                    [
                        if x % 2 == 0 { range } else { -range },
                        ((x * 17 + y * 7) % 997) as f32 / 997. * alpha,
                        -0.1 * alpha,
                        alpha,
                    ]
                })
                .collect(),
        )?;
        let levels = tr_core::provider::ImageLevels::from_source(
            source.clone(),
            tr_core::preview::PreviewRequest::full(),
        )?;
        for region in [
            Region::fitted([513, 257], [257, 129]),
            Region {
                size: [350, 190],
                origin: [7.25, 0.5],
                step: [0.8, 1.1],
            },
            Region {
                size: [257, 129],
                origin: [256., 128.],
                step: [1., 1.],
            },
        ] {
            let start = Instant::now();
            let expected = tr_core::resample::filter_scalar(&source, region, opaque)?;
            result.cpu_seconds += start.elapsed().as_secs_f64();
            let start = Instant::now();
            let actual = compute(device, queue, &source, region, opaque)?;
            result.gpu_seconds += start.elapsed().as_secs_f64();
            for (a, b) in actual
                .iter()
                .flatten()
                .zip(expected.pixels.iter().flatten())
            {
                let error = (*a - *b).abs();
                let threshold = 1e-5 + 1e-4 * b.abs();
                result.samples += 1;
                result.max_error = result.max_error.max(error);
                result.worst_ratio = result.worst_ratio.max(error / threshold);
                if !a.is_finite() || error > threshold {
                    result.failures += 1;
                }
            }
            let mut working = memory.try_reserve(64 * 1024 * 1024).unwrap();
            let mut frame = renderer.render(&levels, region, &memory, &gpu, &mut working)?;
            frame.submit(queue);
            let display = read_display(device, queue, &frame)?;
            let expected = levels.render(region)?.to_display();
            for (a, b) in display.iter().zip(&expected) {
                let error = a.abs_diff(*b);
                result.display_channels += 1;
                result.display_max_error = result.display_max_error.max(error);
                if error > 1 {
                    result.display_failures += 1;
                }
            }
            if result.cases == 0 {
                let before = (memory.usage().reserved, gpu.usage().reserved);
                let mut cancelled_work = memory.try_reserve(64 * 1024 * 1024).unwrap();
                let cancelled_frame =
                    renderer.render(&levels, region, &memory, &gpu, &mut cancelled_work)?;
                drop(cancelled_frame);
                drop(cancelled_work);
                ensure!(
                    before == (memory.usage().reserved, gpu.usage().reserved),
                    "Cancelled GPU commands retained credits"
                );
            }
            result.cases += 1;
        }
    }
    drop(renderer);
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    ensure!(
        memory.usage().reserved == 0 && gpu.usage().reserved == 0,
        "GPU credits leaked after completion"
    );
    Ok(result)
}

/// Readback is restricted to verification; the interactive renderer shares textures.
fn read_display(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &crate::resident_compute::GpuFrame,
) -> Result<Vec<u8>> {
    let row = (frame.size[0] * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("TR display verification"),
        size: row as u64 * frame.size[1] as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        frame.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        frame.texture.size(),
    );
    let submission = queue.submit([encoder.finish()]);
    let (tx, rx) = mpsc::sync_channel(1);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(10)),
    })?;
    rx.recv_timeout(Duration::from_secs(10))??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let result = mapped
        .chunks_exact(row as usize)
        .flat_map(|r| r[..frame.size[0] as usize * 4].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    Ok(result)
}

#[derive(serde::Serialize)]
pub struct PerformanceCase {
    size: [u32; 2],
    cpu_seconds: Vec<f64>,
    gpu_seconds: Vec<f64>,
    scalar_seconds: Vec<f64>,
    parallel_to_scalar_median_ratio: f64,
    parallel_to_scalar_bootstrap_95_percent: [f64; 2],
    parallel_improvement_beyond_five_percent: bool,
    first_gpu_submission_seconds: f64,
    gpu_to_cpu_median_ratio: f64,
    paired_bootstrap_95_percent_ratio: [f64; 2],
    improvement_beyond_five_percent: bool,
}
fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
fn paired_ratio_interval(numerator: &[f64], denominator: &[f64]) -> [f64; 2] {
    let mut seed = 0x12345678_u64;
    let mut ratios = Vec::new();
    for _ in 0..1000 {
        let mut a = Vec::new();
        let mut b = Vec::new();
        for _ in 0..numerator.len() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = (seed >> 32) as usize % numerator.len();
            a.push(numerator[i]);
            b.push(denominator[i]);
        }
        ratios.push(median(&a) / median(&b));
    }
    ratios.sort_by(f64::total_cmp);
    [ratios[25], ratios[974]]
}
pub fn performance(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<PerformanceCase>> {
    use tr_core::{budget::MemoryBudget, preview::PreviewRequest, provider::ImageLevels};
    let memory = MemoryBudget::new(768 * 1024 * 1024);
    let gpu = MemoryBudget::new(256 * 1024 * 1024);
    let mut renderer = crate::resident_compute::GpuFilter::new(device.clone());
    let source = LinearImage::new(
        2048,
        1365,
        (0..2048 * 1365)
            .map(|i| {
                let v = (i % 997) as f32 / 997.;
                [v, 0.3 * v, 0.7 * v, 1.]
            })
            .collect(),
    )?;
    let levels = ImageLevels::from_source(source, PreviewRequest::full())?;
    let mut cases = Vec::new();
    for size in [[256, 171], [1200, 800]] {
        let region = Region::fitted(levels.source_size(), size);
        let mut cpu_seconds = Vec::new();
        let mut gpu_seconds = Vec::new();
        let mut scalar_seconds = Vec::new();
        let (input, input_region, opaque) = levels.render_input(region)?;
        ensure!(
            tr_core::resample::filter_scalar(input, input_region, opaque)?.pixels
                == levels.render(region)?.pixels,
            "Scalar/parallel reference changed"
        );
        let mut gpu_trial = || -> Result<f64> {
            let mut work = memory.try_reserve(128 * 1024 * 1024).unwrap();
            let start = Instant::now();
            let mut frame = renderer.render(&levels, region, &memory, &gpu, &mut work)?;
            frame.submit(queue);
            device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })?;
            let elapsed = start.elapsed().as_secs_f64();
            drop(frame);
            Ok(elapsed)
        };
        let first_gpu_submission_seconds = gpu_trial()?;
        for i in 0..100 {
            let cpu_trial = || -> Result<f64> {
                let start = Instant::now();
                std::hint::black_box(levels.render(region)?.to_display());
                Ok(start.elapsed().as_secs_f64())
            };
            let scalar_trial = || -> Result<f64> {
                let start = Instant::now();
                let (input, input_region, opaque) = levels.render_input(region)?;
                std::hint::black_box(
                    tr_core::resample::filter_scalar(input, input_region, opaque)?.to_display(),
                );
                Ok(start.elapsed().as_secs_f64())
            };
            for stage in 0..3 {
                match (i + stage) % 3 {
                    0 => cpu_seconds.push(cpu_trial()?),
                    1 => gpu_seconds.push(gpu_trial()?),
                    _ => scalar_seconds.push(scalar_trial()?),
                }
            }
        }
        let ratio = median(&gpu_seconds) / median(&cpu_seconds);
        let interval = paired_ratio_interval(&gpu_seconds, &cpu_seconds);
        let parallel_ratio = median(&cpu_seconds) / median(&scalar_seconds);
        let parallel_interval = paired_ratio_interval(&cpu_seconds, &scalar_seconds);
        cases.push(PerformanceCase {
            size,
            cpu_seconds,
            gpu_seconds,
            scalar_seconds,
            parallel_to_scalar_median_ratio: parallel_ratio,
            parallel_to_scalar_bootstrap_95_percent: parallel_interval,
            parallel_improvement_beyond_five_percent: parallel_interval[1] < 0.95,
            first_gpu_submission_seconds,
            gpu_to_cpu_median_ratio: ratio,
            paired_bootstrap_95_percent_ratio: interval,
            improvement_beyond_five_percent: interval[1] < 0.95,
        });
    }
    Ok(cases)
}
