//! Persistent compute pipelines and reusable source buffers on the UI device.
//! The final rgba8 texture is presented directly, with no per-frame readback.
use anyhow::{Context, Result, ensure};
use eframe::wgpu::{self, util::DeviceExt};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tr_core::{
    budget::{Lease, MemoryBudget},
    provider::ImageLevels,
    resample::Region,
};
mod chain;
pub(crate) use chain::Preparation as GraphPreparation;
pub(crate) mod tail;
struct Source {
    buffer: wgpu::Buffer,
    _memory: Lease,
    _gpu: Lease,
}
type SourceKey = (u64, [u32; 2], [u32; 2], [u32; 2]);
pub struct GpuFrame {
    pub texture: wgpu::Texture,
    pub size: [u32; 2],
    pending: Option<Box<PendingSubmission>>,
}
struct PendingSubmission {
    commands: Vec<wgpu::CommandBuffer>,
    scratch: Lease,
    scratch_gpu: Lease,
    source: Arc<dyn Send + Sync>,
    timer: Option<crate::timings::GpuTimer>,
    // Completion wakes blocked encoders; drop it after the other scratch even
    // when an encoded frame is discarded without ever reaching the UI queue.
    chain: Option<Arc<crate::denoise_probe::Completion>>,
}
impl GpuFrame {
    /// The UI owns queue submission, serialized with eframe surface configure.
    /// Encoding and allocation remain on the worker. Dropping an unsubmitted
    /// frame releases its credits immediately; submitted work keeps them alive.
    pub fn submit(&mut self, queue: &wgpu::Queue) {
        self.submit_and_then(queue, || {});
    }
    pub(crate) fn submit_and_then(
        &mut self,
        queue: &wgpu::Queue,
        done: impl FnOnce() + Send + 'static,
    ) {
        if let Some(pending) = self.pending.take() {
            let PendingSubmission {
                commands,
                chain,
                scratch,
                scratch_gpu,
                source,
                timer,
            } = *pending;
            let submission = crate::timings::CpuSpan::new("queue_submit_cpu");
            queue.submit(commands);
            if let Some(chain) = &chain {
                chain.submitted(queue);
            }
            drop(submission);
            if let Some(timer) = timer {
                timer.submitted(queue);
            }
            let completion = crate::timings::CpuSpan::new("submission_to_callback");
            queue.on_submitted_work_done(move || {
                if let Some(chain) = &chain {
                    chain.completed();
                }
                drop(scratch);
                drop(scratch_gpu);
                drop(source);
                drop(chain);
                drop(completion);
                done();
            });
        } else {
            done();
        }
    }
}
pub struct GpuFilter {
    format: wgpu::TextureFormat,
    device: wgpu::Device,
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 2],
    display_pipeline: wgpu::ComputePipeline,
    editing: crate::editing_compute::EditingCompute,
    sources: Mutex<SourceCache>,
    layer_sources: Mutex<Option<Arc<tail::LayerSource>>>,
    layer_compute: std::sync::OnceLock<crate::layer_compute::LayerCompute>,
    chain: std::sync::OnceLock<std::result::Result<crate::denoise_probe::Chain, String>>,
    chain_display: Mutex<Option<Arc<chain::DisplayPlan>>>,
}
#[derive(Default)]
struct SourceCache {
    entries: HashMap<SourceKey, (Arc<Source>, u64)>,
    clock: u64,
}
impl GpuFilter {
    pub fn new(device: wgpu::Device) -> Self {
        Self::with_precision(device, false)
    }
    pub fn with_precision(device: wgpu::Device, high: bool) -> Self {
        let format = if high {
            wgpu::TextureFormat::Rgba16Float
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let entries: Vec<_> = (0..9)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: if binding == 8 {
                    wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    }
                } else {
                    wgpu::BindingType::Buffer {
                        ty: if binding == 0 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage {
                                read_only: binding != 2 && binding != 3,
                            }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("TR viewer compute"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TR viewer compute"),
            source: wgpu::ShaderSource::Wgsl(if high {
                include_str!("preview_compute.wgsl")
                    .replace("rgba8unorm", "rgba16float")
                    .into()
            } else {
                include_str!("preview_compute.wgsl").into()
            }),
        });
        let pipelines = ["horizontal_pass", "vertical_display_pass"].map(|entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let editing = crate::editing_compute::EditingCompute::new(&device);
        let display_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("TR edited viewport display"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("direct_display_pass"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            format,
            device,
            layout,
            pipelines,
            display_pipeline,
            editing,
            sources: Mutex::new(SourceCache::default()),
            layer_sources: Mutex::new(None),
            chain: std::sync::OnceLock::new(),
            chain_display: Mutex::new(None),
            layer_compute: Default::default(),
        }
    }
    pub fn clear(&self) {
        if let Some(Ok(chain)) = self.chain.get() {
            chain.clear();
        }
        let display = self.chain_display.lock().unwrap().take();
        drop(display);
        // Resource destruction can be expensive: release the cache lock first.
        let old = std::mem::take(&mut self.sources.lock().unwrap().entries);
        drop(old);
        let layers = self.layer_sources.lock().unwrap().take();
        drop(layers);
    }
    pub fn display_linear(
        &self,
        image: &tr_core::color::LinearImage,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
    ) -> Result<GpuFrame> {
        let _span = crate::timings::CpuSpan::new("cpu_edit_gpu_display");
        let bytes = image.pixels.len() as u64 * 16;
        ensure!(
            bytes <= self.device.limits().max_storage_buffer_binding_size
                && image.samples_within(1000.),
            "Vista modificata oltre capacità GPU qualificata"
        );
        ensure!(
            working.bytes() >= bytes + 256 + image.pixels.len() as u64 * 24,
            "Display oltre prenotazione"
        );
        let gpu_source = gpu.try_reserve(bytes).context("Quota upload editing GPU")?;
        #[cfg(target_endian = "little")]
        let source_bytes = bytemuck::cast_slice(&image.pixels);
        #[cfg(target_endian = "big")]
        let packed: Vec<_> = image
            .pixels
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        #[cfg(target_endian = "big")]
        let source_bytes = packed.as_slice();
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("TR exact edited viewport"),
                contents: source_bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params = [
            image.width,
            image.width,
            image.height,
            0,
            image.height,
            0,
            0,
            0,
        ];
        let params: Vec<_> = params.iter().flat_map(|v| v.to_le_bytes()).collect();
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let dummy = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let dummy_x = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let dummy_y = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("TR edited display frame"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
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
            &uniform, &buffer, &dummy_x, &dummy_y, &dummy, &dummy, &dummy, &dummy,
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
        let mut timer = crate::timings::GpuTimer::new(&self.device, memory, gpu);
        if let Some(t) = &mut timer {
            t.begin(&mut encoder, "gpu_edited_display");
        }
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(image.width.div_ceil(8), image.height.div_ceil(8), 1);
        }
        if let Some(t) = &timer {
            t.end(&mut encoder);
            t.resolve(&mut encoder);
        }
        let source = Arc::new(Source {
            buffer,
            _memory: working.split(bytes).unwrap(),
            _gpu: gpu_source,
        });
        let scratch_gpu = gpu.try_reserve(256).context("Quota parametri display")?;
        Ok(GpuFrame {
            texture,
            size: [image.width, image.height],
            pending: Some(Box::new(PendingSubmission {
                commands: vec![encoder.finish()],
                chain: None,
                scratch: working.split(256).unwrap(),
                scratch_gpu,
                source,
                timer,
            })),
        })
    }
    pub fn render(
        &self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
    ) -> Result<GpuFrame> {
        self.render_edit(image, region, memory, gpu, working, None)
    }
    pub fn render_edit(
        &self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        edit: Option<&crate::live_edit::LiveEdit>,
    ) -> Result<GpuFrame> {
        self.render_input(
            image,
            region,
            memory,
            gpu,
            working,
            edit,
            false,
            edit.is_none(),
        )
    }
    /// Prepare nearby source coverage on the worker without rendering invisible
    /// display pixels. The UI can then resample adjacent scales immediately.
    pub(crate) fn render_zoom(
        &self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
    ) -> Result<GpuFrame> {
        self.render_input(image, region, memory, gpu, working, None, false, true)
    }
    /// No source scan, packing or upload: suitable for the current UI frame.
    pub(crate) fn render_resident(
        &self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
    ) -> Result<GpuFrame> {
        self.render_input(image, region, memory, gpu, working, None, true, false)
    }
    pub(crate) fn render_resident_edit(
        &self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        edit: Option<&crate::live_edit::LiveEdit>,
    ) -> Result<GpuFrame> {
        self.render_input(image, region, memory, gpu, working, edit, true, false)
    }
    #[allow(clippy::too_many_arguments)]
    fn render_input(
        &self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        edit: Option<&crate::live_edit::LiveEdit>,
        resident_only: bool,
        prepare_zoom: bool,
    ) -> Result<GpuFrame> {
        let _total = crate::timings::CpuSpan::new(if resident_only {
            "resident_prepare_total"
        } else {
            "worker_gpu_prepare_total"
        });
        let preparation = crate::timings::CpuSpan::new("cpu_region_coefficients");

        if let Some(edit) = edit {
            ensure!(edit.gpu_supported_for(image, region), "Nodo editing CPU");
        }
        let (level, mut region, opaque) = image.render_input(region)?;
        // The selected mip can be a full native RAW. Upload only the visible
        // region plus filter support, including for settled frames during pan.
        let (needed_origin, needed_size) = crate::live_edit::crop_bounds(level, region);
        // Only immutable handles cross threads. Source validation, mapped
        // allocation, copying and command encoding never hold this cache lock.
        let resident = {
            let wait = crate::timings::CpuSpan::new("source_cache_wait_cpu");
            let mut cache = if resident_only {
                self.sources
                    .try_lock()
                    .map_err(|_| anyhow::anyhow!("Cache sorgente GPU occupata"))?
            } else {
                self.sources.lock().unwrap()
            };
            drop(wait);
            let _held = crate::timings::CpuSpan::new("source_cache_held_cpu");
            cache.clock += 1;
            let clock = cache.clock;
            let key = cache
                .entries
                .keys()
                .filter(|key| {
                    key.0 == image.id()
                        && key.1 == [level.width, level.height]
                        && (edit.is_none() || (key.2 == needed_origin && key.3 == needed_size))
                        && (0..2).all(|a| {
                            key.2[a] <= needed_origin[a]
                                && key.2[a] + key.3[a] >= needed_origin[a] + needed_size[a]
                        })
                })
                .min_by_key(|key| u64::from(key.3[0]) * u64::from(key.3[1]))
                .copied();
            key.map(|key| {
                let (source, touched) = cache.entries.get_mut(&key).unwrap();
                *touched = clock;
                (key, source.clone())
            })
        };
        ensure!(
            !resident_only || resident.is_some(),
            "Regione sorgente GPU non residente"
        );
        let (origin, source_size) = resident.as_ref().map_or_else(
            || {
                if prepare_zoom {
                    let memory_free = memory.usage().limit.saturating_sub(memory.usage().reserved);
                    let gpu_free = gpu.usage().limit.saturating_sub(gpu.usage().reserved);
                    let maximum = self
                        .device
                        .limits()
                        .max_storage_buffer_binding_size
                        .min(memory_free / 3)
                        .min(gpu_free.saturating_sub(working.bytes()));
                    zoom_source_bounds(
                        needed_origin,
                        needed_size,
                        [level.width, level.height],
                        maximum,
                    )
                } else {
                    (needed_origin, needed_size)
                }
            },
            |(key, _)| (key.2, key.3),
        );
        region.origin[0] -= f64::from(origin[0]);
        region.origin[1] -= f64::from(origin[1]);
        let source_pixels = || {
            level
                .pixels
                .chunks_exact(level.width as usize)
                .skip(origin[1] as usize)
                .take(source_size[1] as usize)
                .flat_map(|row| {
                    row[origin[0] as usize..(origin[0] + source_size[0]) as usize].iter()
                })
        };
        let source_bytes = u64::from(source_size[0]) * u64::from(source_size[1]) * 16;
        ensure!(
            source_bytes <= self.device.limits().max_storage_buffer_binding_size,
            "Livello oltre capability GPU"
        );
        let (xs, ys) = tr_core::resample::coefficients(source_size, region, opaque)?;
        let min_y = ys.iter().flatten().map(|(y, _)| *y).min().unwrap() as u32;
        let max_y = ys.iter().flatten().map(|(y, _)| *y).max().unwrap() as u32;
        let rows = max_y - min_y + 1;
        let output_bytes = region.size[0] as u64 * region.size[1] as u64 * 16;
        let horizontal_bytes = region.size[0] as u64 * rows as u64 * 16;
        let limits = self.device.limits();
        ensure!(
            output_bytes <= limits.max_storage_buffer_binding_size
                && horizontal_bytes <= limits.max_storage_buffer_binding_size
                && region
                    .size
                    .iter()
                    .all(|n| *n <= limits.max_texture_dimension_2d),
            "Scratch o texture oltre capability GPU"
        );
        let (xr, xt) = crate::preview_compute::taps(&xs);
        let (yr, yt) = crate::preview_compute::taps(&ys);
        let scratch_bytes = 16
            + horizontal_bytes
            + (xr.len() + xt.len() + yr.len() + yt.len()) as u64
            + 32
            + if edit.is_some() {
                source_bytes + 52 * 16
            } else {
                0
            };
        ensure!(
            working.bytes()
                >= scratch_bytes
                    + output_bytes / 16
                        * if self.format == wgpu::TextureFormat::Rgba16Float {
                            24
                        } else {
                            12
                        },
            "Compute oltre prenotazione"
        );
        drop(preparation);
        let key = (image.id(), [level.width, level.height], origin, source_size);
        let source = if let Some((_, source)) = resident {
            source
        } else {
            let validation =
                crate::timings::CpuSpan::new("source_validation_cpu").bytes(source_bytes);
            ensure!(
                source_pixels().all(|p| p[..3].iter().all(|c| c.abs() <= 1000.)),
                "Valori fuori dall'intervallo lineare qualificato GPU"
            );
            drop(validation);
            // Evict under a short lock; destroy buffers outside it. Pending
            // submissions retain their source and its leases independently.
            loop {
                let removed = {
                    let mut cache = self.sources.lock().unwrap();
                    if !cache.entries.is_empty()
                        && (cache.entries.len() >= 4
                            || source_bytes * 3
                                > memory.usage().limit.saturating_sub(memory.usage().reserved)
                            || source_bytes + scratch_bytes
                                > gpu.usage().limit.saturating_sub(gpu.usage().reserved))
                    {
                        let key = *cache.entries.iter().min_by_key(|(_, (_, t))| *t).unwrap().0;
                        cache.entries.remove(&key)
                    } else {
                        None
                    }
                };
                if removed.is_none() {
                    break;
                }
                drop(removed);
            }
            let source_memory = memory
                .try_reserve(source_bytes)
                .context("Memoria sorgente GPU occupata")?;
            let source_gpu = gpu
                .try_reserve(source_bytes)
                .context("Quota sorgente GPU occupata")?;
            let upload = memory
                .try_reserve(source_bytes)
                .context("Memoria upload GPU occupata")?;
            // Copy rows straight into the mapped allocation. Avoid a second
            // source-sized Vec and per-channel packing on little-endian hosts.
            let allocation =
                crate::timings::CpuSpan::new("upload_allocate_cpu").bytes(source_bytes);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TR reusable linear source"),
                size: source_bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: true,
            });
            drop(allocation);
            let copy = crate::timings::CpuSpan::new("upload_copy_cpu").bytes(source_bytes);
            {
                let mut mapped = buffer
                    .get_mapped_range_mut(..)
                    .context("Mapping sorgente GPU")?;
                let row_bytes = source_size[0] as usize * 16;
                for (index, row) in level
                    .pixels
                    .chunks_exact(level.width as usize)
                    .skip(origin[1] as usize)
                    .take(source_size[1] as usize)
                    .enumerate()
                {
                    let row = &row[origin[0] as usize..(origin[0] + source_size[0]) as usize];
                    let offset = index * row_bytes;
                    #[cfg(target_endian = "little")]
                    mapped
                        .slice(offset..offset + row_bytes)
                        .copy_from_slice(bytemuck::cast_slice(row));
                    #[cfg(target_endian = "big")]
                    for (i, channel) in row.iter().flatten().enumerate() {
                        let start = offset + i * 4;
                        mapped
                            .slice(start..start + 4)
                            .copy_from_slice(&channel.to_le_bytes());
                    }
                }
            }
            drop(copy);
            let unmap = crate::timings::CpuSpan::new("upload_unmap_cpu").bytes(source_bytes);
            buffer.unmap();
            drop(unmap);
            drop(upload); // mapped-at-creation upload, no pending queue.write_buffer
            let source = Arc::new(Source {
                buffer,
                _memory: source_memory,
                _gpu: source_gpu,
            });
            let replaced = {
                let mut cache = self.sources.lock().unwrap();
                cache.clock += 1;
                let clock = cache.clock;
                cache.entries.insert(key, (source.clone(), clock))
            };
            drop(replaced);
            source
        };
        let resources = crate::timings::CpuSpan::new("gpu_resources_cpu");
        let scratch_gpu = gpu
            .try_reserve(scratch_bytes)
            .context("Scratch GPU oltre quota")?;
        let storage = |bytes: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("TR coefficients"),
                    contents: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let params: Vec<_> = [
            source_size[0],
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
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let buffer = |size| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TR compute scratch"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let horizontal = buffer(horizontal_bytes);
        // Binding retained for the separate numerical reference kernel only.
        let output = buffer(16);
        let edited = edit.map(|_| buffer(source_bytes));
        let xr = storage(&xr);
        let xt = storage(&xt);
        let yr = storage(&yr);
        let yt = storage(&yt);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("TR physical display frame"),
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
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let buffers = [
            &uniform,
            edited.as_ref().unwrap_or(&source.buffer),
            &horizontal,
            &output,
            &xr,
            &xt,
            &yr,
            &yt,
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drop(resources);
        let _encoding = crate::timings::CpuSpan::new("gpu_encode_cpu");
        let mut timer = crate::timings::GpuTimer::new(&self.device, memory, gpu);
        if let Some(edit) = edit {
            if let Some(timer) = &mut timer {
                timer.begin(&mut encoder, "gpu_edit");
            }
            self.editing.encode(
                &self.device,
                &mut encoder,
                &source.buffer,
                edited.as_ref().unwrap(),
                edit,
                source_size[0] * source_size[1],
            );
            if let Some(timer) = &timer {
                timer.end(&mut encoder);
            }
        }
        for ((pipeline, height), phase) in self
            .pipelines
            .iter()
            .zip([rows, region.size[1]])
            .zip(["gpu_horizontal", "gpu_vertical_display"])
        {
            if let Some(timer) = &mut timer {
                timer.begin(&mut encoder, phase);
            }
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(region.size[0].div_ceil(8), height.div_ceil(8), 1);
            drop(pass);
            if let Some(timer) = &timer {
                timer.end(&mut encoder);
            }
        }
        if let Some(timer) = &timer {
            timer.resolve(&mut encoder);
        }
        let scratch = working.split(scratch_bytes).unwrap();
        Ok(GpuFrame {
            texture,
            size: region.size,
            pending: Some(Box::new(PendingSubmission {
                commands: vec![encoder.finish()],
                chain: None,
                scratch,
                scratch_gpu,
                source,
                timer,
            })),
        })
    }
}

/// Optional source padding is bounded independently of the display frame.
/// Failure to admit padding never rejects a visible region that would fit.
fn zoom_source_bounds(
    origin: [u32; 2],
    size: [u32; 2],
    extent: [u32; 2],
    maximum_bytes: u64,
) -> ([u32; 2], [u32; 2]) {
    for divisor in [2, 4, 8] {
        let pad = size.map(|n| n.div_ceil(divisor).min(512));
        let start = std::array::from_fn(|a| origin[a].saturating_sub(pad[a]));
        let padded: [u32; 2] =
            std::array::from_fn(|a| (origin[a] + size[a] + pad[a]).min(extent[a]) - start[a]);
        if u64::from(padded[0]) * u64::from(padded[1]) * 16 <= maximum_bytes {
            return (start, padded);
        }
    }
    (origin, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_padding_obeys_edges_and_quota_without_reducing_visible_coverage() {
        for (origin, size, extent) in [
            ([0, 0], [90, 60], [1024, 768]),
            ([930, 700], [94, 68], [1024, 768]),
            ([700, 600], [400, 300], [2048, 1536]),
        ] {
            let visible = u64::from(size[0]) * u64::from(size[1]) * 16;
            for maximum in [0, visible, visible * 2, visible * 4] {
                let (start, padded) = zoom_source_bounds(origin, size, extent, maximum);
                for a in 0..2 {
                    assert!(start[a] <= origin[a]);
                    assert!(start[a] + padded[a] >= origin[a] + size[a]);
                    assert!(start[a] + padded[a] <= extent[a]);
                }
                if (start, padded) != (origin, size) {
                    assert!(u64::from(padded[0]) * u64::from(padded[1]) * 16 <= maximum);
                }
            }
        }
    }
}
