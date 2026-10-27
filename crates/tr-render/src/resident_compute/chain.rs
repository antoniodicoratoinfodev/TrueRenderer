//! Canonical geometry and final resampling of a linear GPU graph. The input is
//! already edited at its admitted source resolution; no RGB readback is used.
use super::*;
use tr_core::editing::{geometry::Geometry, regional::tail::LayerInputPlan};

pub(crate) enum Preparation {
    Upload(crate::denoise_probe::GraphUpload),
    Optics(Arc<crate::denoise_probe::GraphOptics>),
    Frame(GpuFrame),
}
impl Preparation {
    pub(crate) fn submit(self, queue: &wgpu::Queue, done: impl FnOnce(bool) + Send + 'static) {
        match self {
            Self::Upload(upload) => upload.submit_background(queue, move || done(false)),
            Self::Optics(optics) => optics.submit_background(queue, move || done(false)),
            Self::Frame(mut frame) => {
                let texture = frame.texture.clone();
                frame.submit_and_then(queue, move || {
                    drop(texture);
                    done(true);
                });
            }
        }
    }
}

pub(super) struct DisplayPlan {
    image: u64,
    geometry_key: Geometry,
    region: Region,
    input_origin: [u32; 2],
    input_size: [u32; 2],
    mapped_size: [u32; 2],
    geometry: Option<(wgpu::Buffer, wgpu::Buffer)>,
    coefficients: [wgpu::Buffer; 5],
    mips: Vec<MipFilter>,
    rows: u32,
    _memory: Lease,
    _gpu: Lease,
}
struct MipFilter {
    size: [u32; 2],
    rows: u32,
    coefficients: [wgpu::Buffer; 5],
}
struct MipData {
    input: [u32; 2],
    size: [u32; 2],
    ranges: [Vec<u8>; 4],
    rows: u32,
    min_y: u32,
}
fn mip_sizes(native: [u32; 2], mut size: [u32; 2], region: Region) -> Result<Vec<[u32; 2]>> {
    ensure!(
        region.step.iter().all(|s| s.is_finite() && *s > 0.),
        "Invalid graph scale"
    );
    let mut sizes = Vec::new();
    while size != [1, 1]
        && (0..2).any(|a| region.step[a] * f64::from(size[a]) / f64::from(native[a]) > 2.)
    {
        size = size.map(|v| v.div_ceil(2));
        sizes.push(size);
    }
    Ok(sizes)
}
impl GpuFilter {
    fn graph(&self) -> Result<&crate::denoise_probe::Chain> {
        self.chain
            .get_or_init(|| {
                crate::denoise_probe::Chain::new(&self.device).map_err(|e| format!("{e:#}"))
            })
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))
    }
    fn release_other_inputs(&self) {
        // Complete-graph textures replace the old regional point/layer inputs.
        // Existing display frames retain their textures; in-flight commands
        // keep their own Arc and credits until completion. Destroy outside locks.
        let sources = std::mem::take(&mut self.sources.lock().unwrap().entries);
        let layers = self.layer_sources.lock().unwrap().take();
        drop((sources, layers));
    }
    /// Upload first, then build the exact graph in a later preparation. This
    /// releases all staging before admitting compute scratch and optical gains.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_chain(
        &self,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Preparation> {
        // Validate before uploading, but defer GPU geometry coefficients until
        // staging is released. A large viewport must not block a cold upload
        // with a display plan that cannot yet consume that source.
        Self::graph_plan(image, edit, region, memory, gpu, &self.device, cancelled)?;
        self.device.poll(wgpu::PollType::Poll)?;
        self.release_other_inputs();
        let chain = self.graph()?;
        if !chain.source_ready(image) {
            let old = self.chain_display.lock().unwrap().take();
            drop(old);
        }
        if let Some(upload) = chain.preload(&self.device, image, memory, gpu, cancelled)? {
            return Ok(Preparation::Upload(upload));
        }
        let optics = chain.optics(&self.device, image, edit, memory, gpu, cancelled)?;
        if !optics.ready() {
            return Ok(Preparation::Optics(optics));
        }
        self.render_chain(image, edit, region, memory, gpu, working, cancelled)
            .map(Preparation::Frame)
    }
    #[allow(clippy::too_many_arguments)]
    fn graph_plan(
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        device: &wgpu::Device,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<(LayerInputPlan, Vec<[u32; 2]>)> {
        let native = edit.source_size(image);
        let full = native.map(|v| v.div_ceil(1 << image.base_level()));
        let mips = mip_sizes(native, full, region)?;
        let graph_region = if mips.is_empty() {
            region
        } else {
            // Rebuild edited mips after the complete graph and geometry, never
            // apply nonlinear edits to a smaller unedited input. Bound the full
            // geometry map before allocating its CPU coordinates.
            let bytes = u64::from(full[0]) * u64::from(full[1]) * 48 + 128 * 1024;
            ensure!(
                bytes <= (memory.usage().limit / 8).min(gpu.usage().limit / 8)
                    && bytes <= device.limits().max_storage_buffer_binding_size,
                "Edited mip geometry exceeds admitted metadata quota"
            );
            Region::fitted(native, full)
        };
        Ok((
            LayerInputPlan::for_graph(image, &edit.recipe, graph_region, cancelled)?,
            mips,
        ))
    }
    #[allow(clippy::too_many_arguments)]
    fn chain_plan(
        &self,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Arc<DisplayPlan>> {
        let _plan = crate::timings::CpuSpan::new("graph_display_plan_cpu");
        let mut geometry_key = edit
            .recipe
            .advanced
            .as_ref()
            .map_or_else(Geometry::default, |a| a.geometry.clone());
        geometry_key.vignette = 0.;
        geometry_key.defringe = 0.;
        let cached = self
            .chain_display
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| {
                s.image == image.id() && s.region == region && s.geometry_key == geometry_key
            })
            .cloned();
        if let Some(cached) = cached.filter(|s| {
            s._memory.bytes() <= (memory.usage().limit / 8).min(gpu.usage().limit / 8)
                && memory.usage().reserved <= memory.usage().limit
                && gpu.usage().reserved <= gpu.usage().limit
        }) {
            return Ok(cached);
        }
        let old = self.chain_display.lock().unwrap().take();
        drop(old);
        let (plan, mip_sizes) =
            Self::graph_plan(image, edit, region, memory, gpu, &self.device, cancelled)?;
        let coordinates = plan.geometry_samples();
        let mut mip_data = Vec::new();
        let mut final_size = plan.output_full;
        for size in mip_sizes {
            ensure!(!cancelled(), "Editing annullato");
            let (xs, ys) = tr_core::resample::coefficients(
                final_size,
                Region::fitted(final_size, size),
                plan.opaque,
            )?;
            let min_y = ys
                .iter()
                .flatten()
                .map(|(y, _)| *y)
                .min()
                .context("Empty mip rows")? as u32;
            let max_y = ys.iter().flatten().map(|(y, _)| *y).max().unwrap() as u32;
            let (xr, xt) = crate::preview_compute::taps(&xs);
            let (yr, yt) = crate::preview_compute::taps(&ys);
            mip_data.push(MipData {
                input: final_size,
                size,
                ranges: [xr, xt, yr, yt],
                rows: max_y - min_y + 1,
                min_y,
            });
            final_size = size;
        }
        let display_region = if mip_data.is_empty() {
            plan.display_region
        } else {
            let native = edit.source_size(image);
            Region {
                size: region.size,
                origin: std::array::from_fn(|a| {
                    region.origin[a] * f64::from(final_size[a]) / f64::from(native[a])
                }),
                step: std::array::from_fn(|a| {
                    region.step[a] * f64::from(final_size[a]) / f64::from(native[a])
                }),
            }
        };
        let (mut xs, mut ys) =
            tr_core::resample::coefficients(final_size, display_region, plan.opaque)?;
        let final_origin = if mip_data.is_empty() {
            plan.mapped_origin
        } else {
            [0, 0]
        };
        for (taps, origin) in [(&mut xs, final_origin[0]), (&mut ys, final_origin[1])] {
            for (index, _) in taps.iter_mut().flatten() {
                *index -= origin as usize;
            }
        }
        let min_y = ys
            .iter()
            .flatten()
            .map(|(y, _)| *y)
            .min()
            .context("Empty display rows")? as u32;
        let max_y = ys.iter().flatten().map(|(y, _)| *y).max().unwrap() as u32;
        let rows = max_y - min_y + 1;
        let (xr, xt) = crate::preview_compute::taps(&xs);
        let (yr, yt) = crate::preview_compute::taps(&ys);
        let bytes = (coordinates.len() * 16 + xr.len() + xt.len() + yr.len() + yt.len()) as u64
            + 128 * 1024
            + mip_data
                .iter()
                .map(|m| m.ranges.iter().map(|r| r.len() as u64).sum::<u64>() + 5 * 65536)
                .sum::<u64>();
        ensure!(
            bytes <= (memory.usage().limit / 8).min(gpu.usage().limit / 8),
            "GPU display plan exceeds cache quota"
        );
        ensure!(
            [
                coordinates.len() as u64 * 16,
                xr.len() as u64,
                xt.len() as u64,
                yr.len() as u64,
                yt.len() as u64
            ]
            .iter()
            .all(|b| *b <= self.device.limits().max_storage_buffer_binding_size),
            "Display metadata exceeds binding"
        );
        ensure!(
            mip_data
                .iter()
                .flat_map(|m| &m.ranges)
                .all(|r| r.len() as u64 <= self.device.limits().max_storage_buffer_binding_size),
            "Mip metadata exceeds binding"
        );
        let source_memory = memory
            .try_reserve_within_limit(bytes)
            .context("Display plan RAM quota")?;
        let source_gpu = gpu
            .try_reserve_within_limit(bytes)
            .context("Display plan GPU quota")?;
        let storage = |bytes: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("TR graph display metadata"),
                    contents: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let geometry = if coordinates.is_empty() {
            None
        } else {
            let params = [
                plan.input_size[0] as f32,
                plan.input_origin[0] as f32,
                plan.input_origin[1] as f32,
                0.,
                plan.source_full[0] as f32,
                plan.source_full[1] as f32,
                u32::from(plan.chromatic_aberration()) as f32,
                0.,
            ];
            Some((
                storage(bytemuck::cast_slice(&coordinates)),
                storage(bytemuck::cast_slice(&params)),
            ))
        };
        let params = [
            if mip_data.is_empty() {
                plan.mapped_size[0]
            } else {
                final_size[0]
            },
            region.size[0],
            region.size[1],
            min_y,
            rows,
            u32::from(plan.opaque),
            0,
            0,
        ];
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let mips = mip_data
            .into_iter()
            .map(|m| {
                let params = [
                    m.input[0],
                    m.size[0],
                    m.size[1],
                    m.min_y,
                    m.rows,
                    u32::from(plan.opaque),
                    0,
                    0,
                ];
                let uniform = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("TR edited mip parameters"),
                        contents: bytemuck::cast_slice(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                MipFilter {
                    size: m.size,
                    rows: m.rows,
                    coefficients: [
                        uniform,
                        storage(&m.ranges[0]),
                        storage(&m.ranges[1]),
                        storage(&m.ranges[2]),
                        storage(&m.ranges[3]),
                    ],
                }
            })
            .collect();
        let prepared = Arc::new(DisplayPlan {
            image: image.id(),
            geometry_key,
            region,
            input_origin: plan.input_origin,
            input_size: plan.input_size,
            mapped_size: plan.mapped_size,
            geometry,
            coefficients: [
                uniform,
                storage(&xr),
                storage(&xt),
                storage(&yr),
                storage(&yt),
            ],
            mips,
            rows,
            _memory: source_memory,
            _gpu: source_gpu,
        });
        ensure!(!cancelled(), "Editing annullato");
        *self.chain_display.lock().unwrap() = Some(prepared.clone());
        Ok(prepared)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_chain(
        &self,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<GpuFrame> {
        ensure!(!cancelled(), "Editing annullato");
        self.device.poll(wgpu::PollType::Poll)?;
        let source = self.chain_plan(image, edit, region, memory, gpu, cancelled)?;
        self.release_other_inputs();
        let pipeline = self
            .layer_compute
            .get_or_init(|| crate::layer_compute::LayerCompute::new(&self.device));
        let chain = self.graph()?;
        let mapped_bytes = if source.geometry.is_some() {
            u64::from(source.mapped_size[0]) * u64::from(source.mapped_size[1]) * 16
        } else {
            0
        };
        let horizontal_bytes = source
            .mips
            .iter()
            .map(|m| u64::from(m.size[0]) * u64::from(m.rows) * 16)
            .chain(std::iter::once(
                u64::from(region.size[0]) * u64::from(source.rows) * 16,
            ))
            .max()
            .unwrap();
        let mip_bytes: Vec<_> = source
            .mips
            .iter()
            .map(|m| u64::from(m.size[0]) * u64::from(m.size[1]) * 16)
            .collect();
        let scratch_bytes = mapped_bytes
            + horizontal_bytes
            + 65536
            + mip_bytes.iter().map(|b| b + 65536).sum::<u64>();
        let display_bytes = u64::from(region.size[0])
            * u64::from(region.size[1])
            * if self.format == wgpu::TextureFormat::Rgba16Float {
                24
            } else {
                12
            };
        ensure!(
            working.bytes() >= scratch_bytes + display_bytes,
            "Graph display exceeds reservation"
        );
        ensure!(
            [mapped_bytes, horizontal_bytes]
                .iter()
                .chain(&mip_bytes)
                .all(|v| *v <= self.device.limits().max_storage_buffer_binding_size)
                && region
                    .size
                    .iter()
                    .all(|v| *v > 0 && *v <= self.device.limits().max_texture_dimension_2d),
            "Graph display exceeds device capability"
        );
        let scratch_gpu = chain.reserve_display(&self.device, gpu, scratch_bytes, cancelled)?;
        let (mut commands, completion) = chain.prepare(
            &self.device,
            image,
            edit,
            source.input_origin,
            source.input_size,
            memory,
            gpu,
            cancelled,
        )?;
        let _encoding = crate::timings::CpuSpan::new("graph_display_encode_cpu");
        let mut timer = crate::timings::GpuTimer::new(&self.device, memory, gpu);
        let buffer = |size| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TR graph display scratch"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let mapped = (mapped_bytes != 0).then(|| buffer(mapped_bytes));
        let mip_outputs: Vec<_> = mip_bytes.iter().map(|b| buffer(*b)).collect();
        let horizontal = buffer(horizontal_bytes);
        let dummy = buffer(16);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("TR complete GPU graph viewport"),
            size: wgpu::Extent3d {
                width: region.size[0],
                height: region.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let buffers = [
            &source.coefficients[0],
            mip_outputs
                .last()
                .unwrap_or_else(|| mapped.as_ref().unwrap_or(completion.buffer())),
            &horizontal,
            &dummy,
            &source.coefficients[1],
            &source.coefficients[2],
            &source.coefficients[3],
            &source.coefficients[4],
        ];
        let mut entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: wgpu::BindingResource::TextureView(&view),
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        if let Some(t) = &mut timer {
            t.begin(&mut encoder, "gpu_graph_geometry");
        }
        if let Some((map, params)) = &source.geometry {
            pipeline.geometry(
                &self.device,
                &mut encoder,
                completion.buffer(),
                mapped.as_ref().unwrap(),
                map,
                params,
                (mapped_bytes / 16) as u32,
            );
        }
        if let Some(t) = &timer {
            t.end(&mut encoder);
        }
        if !source.mips.is_empty() {
            if let Some(t) = &mut timer {
                t.begin(&mut encoder, "gpu_graph_edited_mips");
            }
            for (index, mip) in source.mips.iter().enumerate() {
                ensure!(!cancelled(), "Editing annullato");
                let input = if index == 0 {
                    mapped.as_ref().unwrap_or(completion.buffer())
                } else {
                    &mip_outputs[index - 1]
                };
                let buffers = [
                    &mip.coefficients[0],
                    input,
                    &horizontal,
                    &mip_outputs[index],
                    &mip.coefficients[1],
                    &mip.coefficients[2],
                    &mip.coefficients[3],
                    &mip.coefficients[4],
                ];
                let mut entries: Vec<_> = buffers
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect();
                entries.push(wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&view),
                });
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("TR edited mip"),
                    layout: &self.layout,
                    entries: &entries,
                });
                for (pipeline, height) in [
                    (&self.pipelines[0], mip.rows),
                    (&self.linear_pipeline, mip.size[1]),
                ] {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(mip.size[0].div_ceil(8), height.div_ceil(8), 1);
                }
            }
            if let Some(t) = &timer {
                t.end(&mut encoder);
            }
        }
        for ((pipeline, height), phase) in self
            .pipelines
            .iter()
            .zip([source.rows, region.size[1]])
            .zip(["gpu_graph_horizontal", "gpu_graph_display"])
        {
            if let Some(t) = &mut timer {
                t.begin(&mut encoder, phase);
            }
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(region.size[0].div_ceil(8), height.div_ceil(8), 1);
            drop(pass);
            if let Some(t) = &timer {
                t.end(&mut encoder);
            }
        }
        if let Some(t) = &timer {
            t.resolve(&mut encoder);
        }
        commands.push(encoder.finish());
        ensure!(!cancelled(), "Editing annullato");
        Ok(GpuFrame {
            texture,
            size: region.size,
            pending: Some(Box::new(PendingSubmission {
                commands,
                chain: Some(completion),
                scratch: working.split(scratch_bytes).unwrap(),
                scratch_gpu,
                source,
                timer,
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edited_mip_selection_matches_the_canonical_pyramid_at_odd_edges_and_thresholds() {
        for full in [[265, 199], [1, 257], [513, 1]] {
            for base in [0, 2] {
                let native = full.map(|v| (v - 1) * (1 << base) + 1);
                let pixels = vec![[0.2, 0.3, 0.4, 1.]; (full[0] * full[1]) as usize];
                let image = ImageLevels::from_reference_mip(
                    tr_core::color::LinearImage::new(full[0], full[1], pixels).unwrap(),
                    native,
                    base,
                    true,
                )
                .unwrap();
                for step in [0.75, 1.99999, 2., 2.00001, 4.1, 8.2, 2048.] {
                    let region = Region {
                        origin: [-1.25, 3.7],
                        size: [7, 5],
                        step: [step * f64::from(1 << base); 2],
                    };
                    let sizes = mip_sizes(native, full, region).unwrap();
                    let selected = image.render_input(region).unwrap().0;
                    assert_eq!(
                        sizes.last().copied().unwrap_or(full),
                        [selected.width, selected.height]
                    );
                }
            }
        }
    }
}
