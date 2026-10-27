//! Large-region checks against the full CPU graph, including every stripe seam.
use super::*;
use std::time::Duration;

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub cases: usize,
    pub samples: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub maximum_display_error: u8,
    pub patch_limit_bytes: u64,
    pub untiled_patch_lower_bound_bytes: u64,
    pub storage_binding_limit: u64,
    pub failed_cases: Vec<String>,
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let size = [1537, 1025];
    let dimensions = [1297, 865];
    let mut source = reconstruction::source(size, 2)?;
    for (i, pixel) in source.pixels.iter_mut().enumerate() {
        let alpha = if i % 4093 == 0 {
            0.
        } else {
            [0.01, 0.5, 1.][i % 3]
        };
        for c in &mut pixel[..3] {
            *c *= alpha;
        }
        pixel[3] = alpha;
    }
    let pipeline = multiscale::Multiscale::new(device)?;
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&source.pixels),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut upload = device.create_command_encoder(&Default::default());
    let texture = input::upload(device, &mut upload, &source)?;
    queue.submit([upload.finish()]);
    let view = texture.create_view(&Default::default());
    let mut result = Check {
        patch_limit_bytes: filter::PATCH_LIMIT.min(device.limits().max_storage_buffer_binding_size),
        untiled_patch_lower_bound_bytes: u64::from(dimensions[0]) * u64::from(dimensions[1]) * 128,
        storage_binding_limit: device.limits().max_storage_buffer_binding_size,
        ..Default::default()
    };
    let detail = tr_core::editing::detail::Detail {
        luminance_noise: 75.,
        chroma_noise: 85.,
        threshold: 31.,
        ..Default::default()
    };
    for (n, factor, levels) in [(4, 4, 1), (8, 2, 2), (16, 1, 3)] {
        let mut expected = source.clone();
        detail.apply(&mut expected, size.map(|v| v * factor));
        for origin in [[0, 0], [119, 79], [240, 160]] {
            for texture_mode in [false, true] {
                let input = if texture_mode {
                    input::Input::Texture(&view)
                } else {
                    input::Input::Buffer(&input)
                };
                let mut encoder = device.create_command_encoder(&Default::default());
                let output = pipeline.encode(
                    device,
                    &mut encoder,
                    input,
                    size,
                    n,
                    [0.75, 0.85],
                    0.31,
                    levels,
                    false,
                    origin,
                    dimensions,
                )?;
                let bytes = u64::from(dimensions[0]) * u64::from(dimensions[1]) * 16;
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
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
                let before = result.failures;
                let mut display_error = 0;
                for (i, actual) in mapped.as_chunks::<16>().0.iter().enumerate() {
                    let actual: Pixel = std::array::from_fn(|c| {
                        f32::from_le_bytes(actual[c * 4..c * 4 + 4].try_into().unwrap())
                    });
                    let x = origin[0] as usize + i % dimensions[0] as usize;
                    let y = origin[1] as usize + i / dimensions[0] as usize;
                    let expected = expected.pixels[y * size[0] as usize + x];
                    for (a, e) in actual.into_iter().zip(expected) {
                        let error = (a - e).abs();
                        result.samples += 1;
                        result.failures +=
                            usize::from(!a.is_finite() || error > 1e-5 + 1e-4 * e.abs());
                        result.maximum_error = result.maximum_error.max(error);
                    }
                    let a = tr_core::color::display_pixel(actual, 119. / 255.);
                    let e = tr_core::color::display_pixel(expected, 119. / 255.);
                    for (a, e) in a.into_iter().zip(e) {
                        display_error = display_error.max(a.abs_diff(e));
                    }
                }
                result.maximum_display_error = result.maximum_display_error.max(display_error);
                if result.failures > before || display_error > 1 {
                    result.failed_cases.push(format!("n={n} origin={origin:?} texture={texture_mode} failures={} display={display_error}", result.failures - before));
                }
                drop(mapped);
                readback.unmap();
                result.cases += 1;
            }
        }
    }
    Ok(result)
}
