//! Experimental complete multiscale denoiser, still excluded from editing.
use super::*;
use std::time::Duration;

pub(super) struct Multiscale {
    filter: filter::Filter,
    layout: [wgpu::BindGroupLayout; 3],
    pipelines: [[wgpu::ComputePipeline; 3]; 3],
}

struct Footprint {
    use_coarse: bool,
    coarse_full: [u32; 2],
    coarse_origin: [u32; 2],
    coarse_size: [u32; 2],
    origin: [u32; 2],
    size: [u32; 2],
}
impl Footprint {
    fn new(
        size: [u32; 2],
        origin: [u32; 2],
        dimensions: [u32; 2],
        chroma: f32,
        levels: u32,
    ) -> Self {
        let use_coarse = chroma > 0. && levels > 0 && size[0].min(size[1]) >= 32;
        let coarse_full = size.map(|v| v.div_ceil(2));
        let coarse_origin = origin.map(|v| ((v as f32 * 0.5 - 0.25).floor() as i64).max(0) as u32);
        let coarse_end: [u32; 2] = std::array::from_fn(|a| {
            (((origin[a] + dimensions[a] - 1) as f32 * 0.5 - 0.25).floor() as i64 + 2)
                .clamp(1, i64::from(coarse_full[a])) as u32
        });
        let coarse_size = std::array::from_fn(|a| coarse_end[a] - coarse_origin[a]);
        let footprint_origin = if use_coarse {
            coarse_origin.map(|v| v * 2)
        } else {
            origin
        };
        let footprint_size = if use_coarse {
            std::array::from_fn(|a| (coarse_end[a] * 2).min(size[a]) - footprint_origin[a])
        } else {
            dimensions
        };
        Self {
            use_coarse,
            coarse_full,
            coarse_origin,
            coarse_size,
            origin: footprint_origin,
            size: footprint_size,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn working_bytes(
    limits: &wgpu::Limits,
    size: [u32; 2],
    n: usize,
    chroma: f32,
    levels: u32,
    origin: [u32; 2],
    dimensions: [u32; 2],
) -> Result<u64> {
    let (bytes, patches) = working_parts(limits, size, n, chroma, levels, origin, dimensions)?;
    Ok(bytes + patches)
}
/// Top-level fine/guide buffers are dead after the merge. They remain charged
/// to upstream scratch and can serve the following spatial passes in order.
pub(super) fn spatial_reuse_count(
    size: [u32; 2],
    origin: [u32; 2],
    dimensions: [u32; 2],
    chroma: f32,
    levels: u32,
) -> u64 {
    let footprint = Footprint::new(size, origin, dimensions, chroma, levels);
    if !footprint.use_coarse {
        return 0;
    }
    let pixels = u64::from(dimensions[0]) * u64::from(dimensions[1]);
    [footprint.size, footprint.coarse_full]
        .into_iter()
        .filter(|s| u64::from(s[0]) * u64::from(s[1]) >= pixels)
        .count() as u64
}
#[allow(clippy::too_many_arguments)]
fn working_parts(
    limits: &wgpu::Limits,
    size: [u32; 2],
    n: usize,
    chroma: f32,
    levels: u32,
    origin: [u32; 2],
    dimensions: [u32; 2],
) -> Result<(u64, u64)> {
    let footprint = Footprint::new(size, origin, dimensions, chroma, levels);
    let mut patches = filter::patch_bytes(footprint.origin, footprint.size, n, limits)?;
    let mut bytes = filter::working_bytes(footprint.origin, footprint.size, n, limits)? - patches;
    if footprint.use_coarse {
        for shape in [footprint.coarse_full, footprint.coarse_size, dimensions] {
            let pixels = u64::from(shape[0]) * u64::from(shape[1]);
            let allocation = pixels * 16;
            ensure!(
                allocation
                    <= limits
                        .max_storage_buffer_binding_size
                        .min(limits.max_buffer_size)
                    && pixels.div_ceil(256)
                        <= u64::from(limits.max_compute_workgroups_per_dimension),
                "Multiscale allocation or dispatch exceeds device limits"
            );
            bytes = bytes
                .checked_add(allocation + 65536)
                .ok_or_else(|| anyhow::anyhow!("Multiscale allocation overflow"))?;
        }
        let (coarse, coarse_patches) = working_parts(
            limits,
            footprint.coarse_full,
            n,
            chroma,
            levels - 1,
            footprint.coarse_origin,
            footprint.coarse_size,
        )?;
        patches = patches.max(coarse_patches);
        bytes = bytes
            .checked_add(65536 + coarse)
            .ok_or_else(|| anyhow::anyhow!("Multiscale allocation overflow"))?;
    }
    Ok((bytes, patches))
}
impl Multiscale {
    pub(super) fn new(device: &wgpu::Device) -> Result<Self> {
        super::guarded(device, || Ok(Self::compile(device)))
    }
    fn compile(device: &wgpu::Device) -> Self {
        let [buffer, texture, point] = [0, 1, 2].map(|kind| {
            let mut entries: Vec<_> = (0..5)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: if binding == 1 {
                        input::binding_type(kind)
                    } else {
                        wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: binding != 4,
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        }
                    },
                    count: None,
                })
                .collect();
            input::extend_layout(kind, &mut entries);
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries,
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(
                    input::shader(include_str!("multiscale.wgsl"), "fine", "params[0].x", kind)
                        .into(),
                ),
            });
            let pipelines = ["downsample", "chroma_delta", "merge_chroma"].map(|entry| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            });
            (layout, pipelines)
        });
        Self {
            filter: filter::Filter::new(device),
            layout: [buffer.0, texture.0, point.0],
            pipelines: [buffer.1, texture.1, point.1],
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: usize,
        input: input::Input<'_>,
        buffers: [&wgpu::Buffer; 4],
        count: u32,
    ) -> Result<()> {
        ensure!(
            input.kind() == 0 || pipeline == 0,
            "Texture input only for full-domain downsampling"
        );
        ensure!(
            count.div_ceil(256) <= device.limits().max_compute_workgroups_per_dimension,
            "Multiscale dispatch exceeds device limits"
        );
        let mut entries: Vec<_> = buffers
            .iter()
            .zip([0, 2, 3, 4])
            .map(|(b, binding)| wgpu::BindGroupEntry {
                binding,
                resource: b.as_entire_binding(),
            })
            .collect();
        input.extend(&mut entries);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout[input.kind()],
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipelines[input.kind()][pipeline]);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(count.div_ceil(256), 1, 1);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        levels: u32,
        coarse_level: bool,
        origin: [u32; 2],
        dimensions: [u32; 2],
    ) -> Result<wgpu::Buffer> {
        self.encode_cancellable(
            device,
            encoder,
            input,
            size,
            n,
            amounts,
            threshold,
            levels,
            coarse_level,
            origin,
            dimensions,
            &mut Vec::new(),
            &|| false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_cancellable(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        levels: u32,
        coarse_level: bool,
        origin: [u32; 2],
        dimensions: [u32; 2],
        reusable: &mut Vec<wgpu::Buffer>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Buffer> {
        ensure!(!cancelled(), "Editing annullato");
        let (_, bytes) = working_parts(
            &device.limits(),
            size,
            n,
            amounts[1],
            levels,
            origin,
            dimensions,
        )?;
        let scratch = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("TR shared multiscale spectral scratch"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        self.encode_shared(
            device,
            encoder,
            input,
            size,
            n,
            amounts,
            threshold,
            levels,
            coarse_level,
            origin,
            dimensions,
            &scratch,
            reusable,
            cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_shared(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        levels: u32,
        coarse_level: bool,
        origin: [u32; 2],
        dimensions: [u32; 2],
        scratch: &wgpu::Buffer,
        reusable: &mut Vec<wgpu::Buffer>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Buffer> {
        ensure!(!cancelled(), "Editing annullato");
        let Footprint {
            use_coarse,
            coarse_full,
            coarse_origin,
            coarse_size,
            origin: footprint_origin,
            size: footprint_size,
        } = Footprint::new(size, origin, dimensions, amounts[1], levels);
        let (fine, _) = self.filter.encode_with_scratch(
            device,
            encoder,
            input,
            size,
            n,
            amounts,
            threshold,
            coarse_level,
            footprint_origin,
            footprint_size,
            Some(scratch),
            cancelled,
        )?;
        if !use_coarse {
            return Ok(fine);
        }
        let words = [
            [size[0], size[1], origin[0], origin[1]],
            [
                dimensions[0],
                dimensions[1],
                footprint_origin[0],
                footprint_origin[1],
            ],
            [
                footprint_size[0],
                footprint_size[1],
                coarse_origin[0],
                coarse_origin[1],
            ],
            [
                coarse_size[0],
                coarse_size[1],
                coarse_full[0],
                coarse_full[1],
            ],
            [0; 4], // Rounding barrier's runtime zero, shared by all passes.
        ];
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&words),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let buffer = |size: [u32; 2]| -> Result<_> {
            let bytes = u64::from(size[0]) * u64::from(size[1]) * 16;
            ensure!(
                bytes <= device.limits().max_storage_buffer_binding_size,
                "Multiscale buffer exceeds device limits"
            );
            Ok(device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }))
        };
        let dummy = buffer([1, 1])?;
        let guide = buffer(coarse_full)?;
        self.pass(
            device,
            encoder,
            0,
            input,
            [&params, &dummy, &dummy, &guide],
            coarse_full[0] * coarse_full[1],
        )?;
        let coarse = self.encode_shared(
            device,
            encoder,
            input::Input::Buffer(&guide),
            coarse_full,
            n,
            [0., amounts[1]],
            threshold,
            levels - 1,
            true,
            coarse_origin,
            coarse_size,
            scratch,
            &mut Vec::new(),
            cancelled,
        )?;
        ensure!(!cancelled(), "Editing annullato");
        let delta = buffer(coarse_size)?;
        self.pass(
            device,
            encoder,
            1,
            input::Input::Buffer(&fine),
            [&params, &coarse, &dummy, &delta],
            coarse_size[0] * coarse_size[1],
        )?;
        let output = buffer(dimensions)?;
        self.pass(
            device,
            encoder,
            2,
            input::Input::Buffer(&fine),
            [&params, &delta, &guide, &output],
            dimensions[0] * dimensions[1],
        )?;
        let bytes = u64::from(dimensions[0]) * u64::from(dimensions[1]) * 16;
        reusable.extend([fine, guide].into_iter().filter(|b| b.size() >= bytes));
        Ok(output)
    }
}

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub cases: usize,
    pub regional_cases: usize,
    pub texture_cases: usize,
    pub large_cases: usize,
    pub large_source_bytes: u64,
    pub storage_binding_limit: u64,
    pub samples: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub worst_ratio: f32,
    pub maximum_display_error: u8,
    pub failed_cases: Vec<String>,
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let pipeline = Multiscale::new(device)?;
    let mut result = Check::default();
    for n in [4_usize, 8, 16] {
        for size in [[17, 13], [65, 33], [133, 131], [517, 269]] {
            for pattern in 0..9 {
                let source = reconstruction::source(size, pattern)?;
                let (amounts, threshold) = match pattern {
                    0 => ([80., 80.], 0.),
                    1 => ([100., 0.], 100.),
                    2 => ([0., 100.], 50.),
                    3 => ([75., 85.], 31.),
                    4 => ([90., 60.], 0.),
                    7 => ([0., 0.], 50.),
                    8 => ([90., 80.], 70.),
                    _ => ([100., 100.], 100.),
                };
                let detail = tr_core::editing::detail::Detail {
                    luminance_noise: amounts[0],
                    chroma_noise: amounts[1],
                    threshold,
                    ..Default::default()
                };
                let factor = match n {
                    4 => 4,
                    8 => 2,
                    _ => 1,
                };
                let levels = match n {
                    4 => 1,
                    8 => 2,
                    _ => 3,
                };
                let mut expected = source.clone();
                detail.apply(&mut expected, size.map(|v| v * factor));
                let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&source.pixels),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let mut regions = vec![([0, 0], size)];
                if size[0] > 100 {
                    regions.push(([3, 5], [size[0] - 7, size[1] - 11]));
                }
                for (origin, dimensions) in regions {
                    for texture_mode in [false, true] {
                        let mut encoder = device.create_command_encoder(&Default::default());
                        let texture = texture_mode
                            .then(|| input::upload(device, &mut encoder, &source))
                            .transpose()?;
                        let view = texture.as_ref().map(|t| t.create_view(&Default::default()));
                        let source = view
                            .as_ref()
                            .map_or(input::Input::Buffer(&input), input::Input::Texture);
                        let output = pipeline.encode(
                            device,
                            &mut encoder,
                            source,
                            size,
                            n,
                            [detail.luminance_noise / 100., detail.chroma_noise / 100.],
                            detail.threshold / 100.,
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
                        let values: Vec<_> = mapped
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|v| f32::from_le_bytes(*v))
                            .collect();
                        let before = result.failures;
                        let mut display_error = 0_u8;
                        for (i, actual) in values.as_chunks::<4>().0.iter().enumerate() {
                            let x = origin[0] as usize + i % dimensions[0] as usize;
                            let y = origin[1] as usize + i / dimensions[0] as usize;
                            let e = expected.pixels[y * size[0] as usize + x];
                            for (a, e) in actual.iter().zip(e) {
                                let error = (a - e).abs();
                                let tolerance = 1e-5 + 1e-4 * e.abs();
                                result.samples += 1;
                                result.failures += usize::from(!a.is_finite() || error > tolerance);
                                result.maximum_error = result.maximum_error.max(error);
                                result.worst_ratio = result.worst_ratio.max(error / tolerance);
                            }
                            let a = tr_core::color::display_pixel(*actual, 119. / 255.);
                            let e = tr_core::color::display_pixel(e, 119. / 255.);
                            for (a, e) in a.into_iter().zip(e) {
                                display_error = display_error.max(a.abs_diff(e));
                            }
                        }
                        result.maximum_display_error =
                            result.maximum_display_error.max(display_error);
                        if result.failures > before || display_error > 1 {
                            result.failed_cases.push(format!("n={n} size={size:?} pattern={pattern} origin={origin:?} texture={texture_mode} failures={} display={display_error}", result.failures - before));
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
    large(device, queue, &pipeline, &mut result)?;
    Ok(result)
}

fn large(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &Multiscale,
    result: &mut Check,
) -> Result<()> {
    let size = [4097, 3073];
    let mut source = reconstruction::source(size, 8)?;
    for (i, p) in source.pixels.iter_mut().enumerate() {
        if i % size[0] as usize >= size[0] as usize / 2 {
            p[0] += 0.15;
        }
    }
    result.large_source_bytes = source.pixels.len() as u64 * 16;
    result.storage_binding_limit = device.limits().max_storage_buffer_binding_size;
    let detail = tr_core::editing::detail::Detail {
        luminance_noise: 75.,
        chroma_noise: 85.,
        threshold: 31.,
        ..Default::default()
    };
    let mut expected = source.clone();
    detail.apply(&mut expected, size);
    let mut upload = device.create_command_encoder(&Default::default());
    let texture = input::upload(device, &mut upload, &source)?;
    // One source upload, reused by all three regional reconstructions.
    queue.submit([upload.finish()]);
    let view = texture.create_view(&Default::default());
    for origin in [[0, 0], [2001, 1499], [3840, 2880]] {
        let dimensions = [257, 193];
        let mut encoder = device.create_command_encoder(&Default::default());
        let output = pipeline.encode(
            device,
            &mut encoder,
            input::Input::Texture(&view),
            size,
            16,
            [detail.luminance_noise / 100., detail.chroma_noise / 100.],
            detail.threshold / 100.,
            3,
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
        let values: Vec<_> = mapped
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| f32::from_le_bytes(*v))
            .collect();
        let before = result.failures;
        let mut display_error = 0_u8;
        for (i, actual) in values.as_chunks::<4>().0.iter().enumerate() {
            let x = origin[0] as usize + i % dimensions[0] as usize;
            let y = origin[1] as usize + i / dimensions[0] as usize;
            let expected = expected.pixels[y * size[0] as usize + x];
            for (a, e) in actual.iter().zip(expected) {
                let error = (a - e).abs();
                let tolerance = 1e-5 + 1e-4 * e.abs();
                result.samples += 1;
                result.failures += usize::from(!a.is_finite() || error > tolerance);
                result.maximum_error = result.maximum_error.max(error);
                result.worst_ratio = result.worst_ratio.max(error / tolerance);
            }
            let a = tr_core::color::display_pixel(*actual, 119. / 255.);
            let e = tr_core::color::display_pixel(expected, 119. / 255.);
            for (a, e) in a.into_iter().zip(e) {
                display_error = display_error.max(a.abs_diff(e));
            }
        }
        result.maximum_display_error = result.maximum_display_error.max(display_error);
        if result.failures > before || display_error > 1 {
            result.failed_cases.push(format!(
                "large size={size:?} origin={origin:?} failures={} display={display_error}",
                result.failures - before
            ));
        }
        drop(mapped);
        readback.unmap();
        result.cases += 1;
        result.large_cases += 1;
        result.regional_cases += 1;
        result.texture_cases += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiscale_shader_validates_offline() {
        for kind in [0, 1] {
            let module = wgpu::naga::front::wgsl::parse_str(&input::shader(
                include_str!("multiscale.wgsl"),
                "fine",
                "params[0].x",
                kind,
            ))
            .unwrap();
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
