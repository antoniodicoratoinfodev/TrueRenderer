//! Persistent compute pipelines and reusable source buffers on the UI device.
//! The final rgba8 texture is presented directly, with no per-frame readback.
use anyhow::{Context, Result, ensure};
use eframe::wgpu::{self, util::DeviceExt};
use std::{collections::HashMap, sync::Arc};
use tr_core::{
    budget::{Lease, MemoryBudget},
    provider::ImageLevels,
    resample::Region,
};
struct Source {
    buffer: wgpu::Buffer,
    _memory: Lease,
    _gpu: Lease,
}
pub struct GpuFrame {
    pub texture: wgpu::Texture,
    pub size: [u32; 2],
    pending: Option<Box<PendingSubmission>>,
}
struct PendingSubmission {
    commands: wgpu::CommandBuffer,
    scratch: Lease,
    scratch_gpu: Lease,
    source: Arc<Source>,
}
impl GpuFrame {
    /// The UI owns queue submission, serialized with eframe surface configure.
    /// Encoding and allocation remain on the worker. Dropping an unsubmitted
    /// frame releases its credits immediately; submitted work keeps them alive.
    pub fn submit(&mut self, queue: &wgpu::Queue) {
        if let Some(pending) = self.pending.take() {
            let PendingSubmission {
                commands,
                scratch,
                scratch_gpu,
                source,
            } = *pending;
            queue.submit([commands]);
            queue.on_submitted_work_done(move || {
                drop(scratch);
                drop(scratch_gpu);
                drop(source);
            });
        }
    }
}
pub struct GpuFilter {
    device: wgpu::Device,
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 3],
    sources: HashMap<(u64, u32, u32), (Arc<Source>, u64)>,
    clock: u64,
}
impl GpuFilter {
    pub fn new(device: wgpu::Device) -> Self {
        let entries: Vec<_> = (0..9)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: if binding == 8 {
                    wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
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
            source: wgpu::ShaderSource::Wgsl(include_str!("preview_compute.wgsl").into()),
        });
        let pipelines = ["horizontal_pass", "vertical_pass", "display_pass"].map(|entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        Self {
            device,
            layout,
            pipelines,
            sources: HashMap::new(),
            clock: 0,
        }
    }
    pub fn clear(&mut self) {
        self.sources.clear();
    }
    pub fn render(
        &mut self,
        image: &ImageLevels,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
    ) -> Result<GpuFrame> {
        self.clock += 1;
        let (level, region, opaque) = image.render_input(region)?;
        let source_bytes = level.pixels.len() as u64 * 16;
        ensure!(
            source_bytes <= self.device.limits().max_storage_buffer_binding_size,
            "Livello oltre capability GPU"
        );
        let (xs, ys) =
            tr_core::resample::coefficients([level.width, level.height], region, opaque)?;
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
        let scratch_bytes = output_bytes
            + horizontal_bytes
            + (xr.len() + xt.len() + yr.len() + yt.len()) as u64
            + 32;
        ensure!(
            working.bytes() >= scratch_bytes + output_bytes / 16 * 12,
            "Compute oltre prenotazione"
        );
        let key = (image.id(), level.width, level.height);
        if !self.sources.contains_key(&key) {
            ensure!(
                level
                    .pixels
                    .iter()
                    .all(|p| p[..3].iter().all(|c| c.abs() <= 1000.)),
                "Valori fuori dall'intervallo lineare qualificato GPU"
            );
            // Release least recent inputs before admitting another upload.
            while !self.sources.is_empty()
                && (self.sources.len() >= 4
                    || source_bytes * 3
                        > memory.usage().limit.saturating_sub(memory.usage().reserved)
                    || source_bytes + scratch_bytes
                        > gpu.usage().limit.saturating_sub(gpu.usage().reserved))
            {
                let key = *self.sources.iter().min_by_key(|(_, (_, t))| *t).unwrap().0;
                self.sources.remove(&key);
            }
            let source_memory = memory
                .try_reserve(source_bytes)
                .context("Memoria sorgente GPU occupata")?;
            let source_gpu = gpu
                .try_reserve(source_bytes)
                .context("Quota sorgente GPU occupata")?;
            let upload = memory
                .try_reserve(source_bytes * 2)
                .context("Memoria upload GPU occupata")?;
            let bytes: Vec<_> = level
                .pixels
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("TR reusable linear source"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                });
            drop(bytes);
            drop(upload); // mapped-at-creation upload, no pending queue.write_buffer
            self.sources.insert(
                key,
                (
                    Arc::new(Source {
                        buffer,
                        _memory: source_memory,
                        _gpu: source_gpu,
                    }),
                    self.clock,
                ),
            );
        }
        let (source, touched) = self.sources.get_mut(&key).unwrap();
        *touched = self.clock;
        let source = source.clone();
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
            level.width,
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
        let output = buffer(output_bytes);
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
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let buffers = [
            &uniform,
            &source.buffer,
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
        for (pipeline, height) in self
            .pipelines
            .iter()
            .zip([rows, region.size[1], region.size[1]])
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(region.size[0].div_ceil(8), height.div_ceil(8), 1);
        }
        let scratch = working.split(scratch_bytes).unwrap();
        Ok(GpuFrame {
            texture,
            size: region.size,
            pending: Some(Box::new(PendingSubmission {
                commands: encoder.finish(),
                scratch,
                scratch_gpu,
                source,
            })),
        })
    }
}
