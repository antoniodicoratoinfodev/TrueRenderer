use anyhow::{Result, ensure};
use eframe::wgpu::{self, util::DeviceExt};
use std::{sync::mpsc, time::Duration};
use tr_core::color::{linear_to_srgb, rec2020_to_linear_srgb, srgb_to_linear};

#[derive(Debug)]
pub struct GpuCheck {
    pub max_error: f32,
    pub samples: usize,
    pub passed: bool,
}
/// Measured on the same device and queue used by the UI. This checks arithmetic,
/// not ICC/LUT accuracy, monitor calibration, gamut, resampling or the compositor.
pub fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<GpuCheck> {
    let mut samples = Vec::with_capacity(4096);
    let mut seed = 913u32;
    for _ in 0..4096 {
        let mut p = [0.; 4];
        for v in &mut p {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (seed >> 8) as f32 / 16_777_215.;
        }
        for c in 0..3 {
            p[c] = (p[c] * 1.4 - 0.2) * p[3];
        }
        samples.push(p);
    }
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("TR diagnostic input"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("TR diagnostic output"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("TR diagnostic private readback"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("TrueRenderer SDR diagnostic"),
        source: wgpu::ShaderSource::Wgsl(include_str!("diagnostic.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("TR diagnostic"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: source.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(8, 8, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes.len() as u64);
    let submission = queue.submit([encoder.finish()]);
    let (tx, rx) = mpsc::sync_channel(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(5)),
    })?;
    rx.recv_timeout(Duration::from_secs(5))??;
    let mapped = readback.slice(..).get_mapped_range()?;
    let mut max_error = 0.0f32;
    let bg = srgb_to_linear(119. / 255.);
    for (p, raw) in samples.iter().zip(mapped.as_chunks::<16>().0) {
        let reference = rec2020_to_linear_srgb([
            p[0] + bg * (1. - p[3]),
            p[1] + bg * (1. - p[3]),
            p[2] + bg * (1. - p[3]),
        ])
        .map(linear_to_srgb);
        for c in 0..3 {
            let gpu = f32::from_le_bytes(raw[c * 4..c * 4 + 4].try_into()?);
            ensure!(gpu.is_finite(), "Risultato GPU non finito");
            max_error = max_error.max((gpu - reference[c]).abs());
        }
    }
    drop(mapped);
    readback.unmap();
    Ok(GpuCheck {
        max_error,
        samples: samples.len(),
        passed: max_error <= 1e-4,
    })
}
