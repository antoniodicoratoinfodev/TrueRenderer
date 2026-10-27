//! Native regression of partial source transfers, independent of image filters.
use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tr_core::budget::MemoryBudget;

fn drain(device: &wgpu::Device) -> Result<()> {
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    Ok(())
}

fn pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    resident: &admission::Source,
    image: &LinearImage,
) -> Result<()> {
    let shader = format!(
        "{}\n@group(0) @binding(0) var source: texture_2d<f32>;\n\
        @group(0) @binding(1) var<storage, read_write> output: array<vec4<f32>>;\n\
        @compute @workgroup_size(256) fn copy(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
        if id.x < {}u {{ output[id.x] = spectral_source_pixel(source, id.x, {}u, {}); }} }}",
        input::SOURCE_SHADER,
        image.pixels.len(),
        image.width,
        resident.packed
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("TR streamed source byte comparison"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("copy"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bytes = image.pixels.len() as u64 * 16;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resident.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((image.pixels.len() as u32).div_ceil(256), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    drain(device)?;
    rx.recv_timeout(Duration::from_secs(1))??;
    let mapped = readback.slice(..).get_mapped_range()?;
    ensure!(
        &*mapped == bytemuck::cast_slice::<Pixel, u8>(&image.pixels),
        "Streamed source changed pixel bits"
    );
    drop(mapped);
    readback.unmap();
    Ok(())
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<()> {
    let mut batches = 0;
    for pattern in [4, 8] {
        let image = reconstruction::source([517, 269], pattern)?;
        let packed = pattern == 8;
        let pitch = (u64::from(image.width) * 16).div_ceil(256) * 256;
        let resident_bytes = pitch * u64::from(input::texture_height([517, 269], packed)) + 65536;
        let limit = resident_bytes + 256 * 1024;
        let memory = MemoryBudget::new(limit);
        let gpu = MemoryBudget::new(limit);
        let unchanged = |bytes| -> Result<()> {
            ensure!(
                memory.usage().reserved == bytes && gpu.usage().reserved == bytes,
                "Streamed transfer leaked or released credits early"
            );
            Ok(())
        };
        ensure!(
            admission::Upload::prepare_point(device, &image, &memory, &gpu, &|| false).is_err(),
            "Fixture must not fit the old full transfer"
        );
        unchanged(0)?;
        let pending =
            admission::Upload::prepare_streamed(device, &image, None, &memory, &gpu, &|| false)?;
        let source = pending.source();
        ensure!(
            source.packed == packed && !source.submitted.load(Ordering::Acquire),
            "Premature source readiness"
        );
        ensure!(
            admission::Upload::prepare_streamed(
                device,
                &image,
                Some(source.clone()),
                &memory,
                &gpu,
                &|| false
            )
            .is_err(),
            "Duplicate upload accepted before submission"
        );
        drop(pending);
        unchanged(resident_bytes)?;
        let visits = AtomicUsize::new(0);
        ensure!(
            admission::Upload::prepare_streamed(
                device,
                &image,
                Some(source.clone()),
                &memory,
                &gpu,
                &|| visits.fetch_add(1, Ordering::Relaxed) >= 2
            )
            .is_err(),
            "Encoding ignored cancellation"
        );
        unchanged(resident_bytes)?;
        let cancelled = admission::Upload::prepare_streamed(
            device,
            &image,
            Some(source.clone()),
            &memory,
            &gpu,
            &|| false,
        )?;
        ensure!(
            cancelled.submit(queue, &|| true).is_err(),
            "Cancelled batch submitted"
        );
        unchanged(resident_bytes)?;
        let mut count = 0;
        while !source.submitted.load(Ordering::Acquire) {
            let pending = admission::Upload::prepare_streamed(
                device,
                &image,
                Some(source.clone()),
                &memory,
                &gpu,
                &|| false,
            )?;
            let (tx, rx) = std::sync::mpsc::channel();
            pending.submit_background(queue, move || {
                let _ = tx.send(());
            });
            // No polling has run the completion callback, even if the hardware
            // has copied the stripe. Another producer must still be rejected.
            ensure!(
                admission::Upload::prepare_streamed(
                    device,
                    &image,
                    Some(source.clone()),
                    &memory,
                    &gpu,
                    &|| false
                )
                .is_err(),
                "Duplicate upload accepted before completion"
            );
            drain(device)?;
            rx.recv_timeout(Duration::from_secs(1))?;
            unchanged(resident_bytes)?;
            count += 1;
            ensure!(count < 100, "Streamed upload made no progress");
        }
        ensure!(
            count > 2 && memory.usage().peak <= limit && gpu.usage().peak <= limit,
            "Fixture did not exercise bounded batches"
        );
        pixels(device, queue, &source, &image)?;
        drop(source);
        unchanged(0)?;
        // Losing the selected image mid-transfer retains its credits only until
        // the submitted copy completes; no owner or readiness survives eviction.
        let pending =
            admission::Upload::prepare_streamed(device, &image, None, &memory, &gpu, &|| false)?;
        let weak = Arc::downgrade(&pending.source());
        pending.submit_background(queue, || {});
        ensure!(weak.upgrade().is_some(), "Pending upload lost its source");
        drain(device)?;
        ensure!(
            weak.upgrade().is_none(),
            "Evicted source survived completion"
        );
        unchanged(0)?;
        println!(
            "streamed_upload packed={packed} batches={count} byte_exact=true limit={limit} peak={} final_ram=0 final_gpu=0",
            gpu.usage().peak
        );
        batches += count;
    }
    ensure!(batches > 4, "Missing streamed upload coverage");
    Ok(())
}
