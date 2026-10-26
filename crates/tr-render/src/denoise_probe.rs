//! Opt-in qualification of the complete GPU multiscale filter, not an editing backend.
use anyhow::{Result, ensure};
use eframe::wgpu::{self, util::DeviceExt};
use tr_core::color::{LinearImage, Pixel};
mod admission;
mod admission_check;
mod benchmark;
mod chain_benchmark;
mod checkpoint;
mod checkpoint_check;
mod filter;
mod input;
mod layers;
mod layers_check;
mod mask;
mod models;
mod multiscale;
#[cfg(test)]
mod native;
mod point;
mod point_check;
mod post;
mod post_check;
mod reconstruction;
mod spatial;
mod spatial_check;
mod tiling;

fn basis(n: usize) -> Vec<[f32; 4]> {
    let values: Vec<_> = (0..n * n)
        .map(|i| {
            let (k, x) = (i / n, i % n);
            let norm = (if k == 0 { 1. } else { 2. } / n as f64).sqrt();
            (norm * (std::f64::consts::PI / n as f64 * (x as f64 + 0.5) * k as f64).cos()) as f32
        })
        .collect();
    values.as_chunks::<4>().0.to_vec()
}

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub error: Option<String>,
    pub cases: usize,
    pub samples: usize,
    pub invalid_blocks: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub worst_ratio: f32,
    pub elapsed_seconds: f64,
    pub reconstruction: reconstruction::Check,
    pub multiscale: multiscale::Check,
    pub tiling: tiling::Check,
    pub admission: admission_check::Check,
    pub point: point_check::Check,
    pub spatial: spatial_check::Check,
    pub post: post_check::Check,
    pub layers: layers_check::Check,
    pub checkpoint: checkpoint_check::Check,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub benchmark: Option<benchmark::Check>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_benchmark: Option<chain_benchmark::Check>,
}

#[allow(clippy::needless_range_loop)]
fn reference(image: &LinearImage, origin: [i32; 2], n: usize) -> Vec<Pixel> {
    // Independent f64 matrix; never consume the shader's packed coefficients.
    let basis: Vec<_> = (0..n * n)
        .map(|i| {
            let (frequency, position) = (i / n, i % n);
            let norm = if frequency == 0 {
                (1. / n as f64).sqrt()
            } else {
                (2. / n as f64).sqrt()
            };
            norm * (std::f64::consts::PI * (position as f64 + 0.5) * frequency as f64 / n as f64)
                .cos()
        })
        .collect();
    let reflect = |v: i32, length: u32| {
        if length <= 1 {
            return 0;
        }
        let period = 2 * (length as i32 - 1);
        let v = v.rem_euclid(period);
        v.min(period - v) as usize
    };
    let mut raw = vec![[0_f32; 3]; n * n];
    let mut mean = [0_f64; 3];
    let mut output = vec![[0.; 4]; 257];
    for (i, out) in raw.iter_mut().enumerate() {
        let x = reflect(origin[0] + (i % n) as i32, image.width);
        let y = reflect(origin[1] + (i / n) as i32, image.height);
        let p = image.pixels[y * image.width as usize + x];
        if p[3] <= 0. {
            return output;
        }
        let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
        let luma = 0.2627 * rgb[0] + 0.678 * rgb[1] + 0.0593 * rgb[2];
        *out = [luma, rgb[0] - luma, rgb[2] - luma];
        for c in 0..3 {
            mean[c] += f64::from(out[c]);
        }
    }
    let mean = mean.map(|v| (v / (n * n) as f64) as f32);
    let mut rows = raw.clone();
    for y in 0..n {
        for k in 0..n {
            for c in 0..3 {
                rows[y * n + k][c] = (0..n)
                    .map(|j| f64::from(raw[y * n + j][c] - mean[c]) * basis[k * n + j])
                    .sum::<f64>() as f32;
            }
        }
    }
    for k in 0..n {
        for x in 0..n {
            for c in 0..3 {
                output[k * 16 + x][c] = (0..n)
                    .map(|j| f64::from(rows[j * n + x][c]) * basis[k * n + j])
                    .sum::<f64>() as f32;
            }
        }
    }
    output[256] = [mean[0], (mean[0].abs() + 0.003).sqrt(), 1., 0.];
    output
}

pub(crate) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    use std::time::{Duration, Instant};
    let started = Instant::now();
    ensure!(
        cfg!(target_endian = "little"),
        "Spectral probe requires little endian"
    );
    let entries: Vec<_> = (0..3)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: binding != 2,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("TR experimental spectral block check"),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("TR spectral block probe"),
        source: wgpu::ShaderSource::Wgsl(include_str!("denoise_probe.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("blocks"),
        compilation_options: Default::default(),
        cache: None,
    });
    let storage = |bytes: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let mut result = Check::default();
    for n in [4_usize, 8, 16] {
        let basis = basis(n);
        for size in [[1, 1], [13, 9], [65, 33], [129, 97]] {
            for pattern in 0..6 {
                let image = LinearImage::new(
                    size[0],
                    size[1],
                    (0..size[0] * size[1])
                        .map(|i| {
                            let alpha = match pattern {
                                4 if i % 53 == 0 => 0.,
                                5 => [0.01, 0.5, 1.][i as usize % 3],
                                _ => 1.,
                            };
                            let rgb = match pattern {
                                0 => [0.3; 3],
                                1 => [
                                    (i % 7) as f32 * 0.1,
                                    (i % 11) as f32 * 0.07,
                                    (i % 17) as f32 * 0.03,
                                ],
                                2 => [
                                    (i % 13) as f32 - 4.,
                                    (i % 11) as f32 - 7.,
                                    (i % 17) as f32 * 0.5 - 2.,
                                ],
                                _ => [
                                    0.2 + (i % 113) as f32 * 0.0002,
                                    0.3 + (i % 71) as f32 * 0.0003,
                                    0.4 + (i % 47) as f32 * 0.0004,
                                ],
                            };
                            [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
                        })
                        .collect(),
                )?;
                let origins: Vec<_> = (0..16)
                    .map(|i| {
                        [
                            (i * 17) % (size[0] as i32 + n as i32) - n as i32 / 2,
                            (i * 13) % (size[1] as i32 + n as i32) - n as i32 / 2,
                        ]
                    })
                    .collect();
                let mut params = vec![[
                    size[0] as f32,
                    size[1] as f32,
                    n as f32,
                    origins.len() as f32,
                ]];
                params.extend(&basis);
                params.extend(origins.iter().map(|v| [v[0] as f32, v[1] as f32, 0., 0.]));
                let params = storage(bytemuck::cast_slice(&params));
                let source = storage(bytemuck::cast_slice(&image.pixels));
                let bytes = (origins.len() * 257 * 16) as u64;
                let buffer = |usage| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: bytes,
                        usage,
                        mapped_at_creation: false,
                    })
                };
                let output = buffer(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
                let readback = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
                let entries: Vec<_> = [&params, &source, &output]
                    .into_iter()
                    .enumerate()
                    .map(|(binding, b)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect();
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &layout,
                    entries: &entries,
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(origins.len() as u32, 1, 1);
                }
                encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
                let submission = queue.submit([encoder.finish()]);
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx.send(r);
                });
                device.poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(10)),
                })?;
                rx.recv_timeout(Duration::from_secs(10))??;
                let mapped = readback.slice(..).get_mapped_range()?;
                let values: Vec<_> = mapped
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|v| f32::from_le_bytes(*v))
                    .collect();
                for (block, origin) in origins.iter().enumerate() {
                    let expected = reference(&image, *origin, n);
                    result.invalid_blocks += usize::from(expected[256][2] == 0.);
                    for (a, e) in values[block * 257 * 4..(block + 1) * 257 * 4]
                        .iter()
                        .zip(expected.iter().flatten())
                    {
                        let error = (a - e).abs();
                        let tolerance = 1e-5 + 1e-4 * e.abs();
                        result.samples += 1;
                        result.failures += usize::from(!a.is_finite() || error > tolerance);
                        result.maximum_error = result.maximum_error.max(error);
                        result.worst_ratio = result.worst_ratio.max(error / tolerance);
                    }
                }
                drop(mapped);
                readback.unmap();
                result.cases += 1;
            }
        }
    }
    result.reconstruction = reconstruction::check(device, queue)?;
    result.multiscale = multiscale::check(device, queue)?;
    result.tiling = tiling::check(device, queue)?;
    result.admission = admission_check::check(device, queue)?;
    result.point = point_check::check(device, queue)?;
    result.spatial = spatial_check::check(device, queue)?;
    result.post = post_check::check(device, queue)?;
    result.layers = layers_check::check(device, queue)?;
    result.checkpoint = checkpoint_check::check(device, queue)?;
    if std::env::var("TR_DENOISE_GPU_BENCH").is_ok_and(|v| v == "1") {
        result.benchmark = Some(benchmark::check(device, queue)?);
    }
    if std::env::var("TR_DENOISE_GPU_CHAIN_BENCH").is_ok_and(|v| v == "1") {
        result.chain_benchmark = Some(chain_benchmark::check(device, queue)?);
    }
    result.elapsed_seconds = started.elapsed().as_secs_f64();
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn spectral_block_shader_validates_offline() {
        for kind in [0, 1] {
            let module = eframe::wgpu::naga::front::wgsl::parse_str(&super::input::shader(
                include_str!("denoise_probe.wgsl"),
                "source",
                "params[0].x",
                kind,
            ))
            .unwrap();
            eframe::wgpu::naga::valid::Validator::new(
                eframe::wgpu::naga::valid::ValidationFlags::all(),
                eframe::wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
