//! Diagnostic GPU model estimation, with no CPU coefficient or statistic readback.
use super::*;

pub(super) struct Models {
    block_layout: [wgpu::BindGroupLayout; 2],
    blocks: [wgpu::ComputePipeline; 2],
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 5],
}

pub(super) fn output_bytes(n: usize) -> u64 {
    (5 + 4 * n * n) as u64 * 16
}

impl Models {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = |count, write: fn(u32) -> bool, kind| {
            let entries: Vec<_> = (0..count)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: if binding == 1 && kind == 1 {
                        input::binding_type(kind)
                    } else {
                        wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: !write(binding),
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        }
                    },
                    count: None,
                })
                .collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries,
            })
        };
        let block_layout = [0, 1].map(|kind| layout(3, |b| b == 2, kind));
        let layout = layout(5, |b| b >= 2, 0);
        let pipelines = |layout: &wgpu::BindGroupLayout, text: &str, names: &[&str]| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(text.into()),
            });
            names
                .iter()
                .map(|entry| {
                    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&pipeline_layout),
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    })
                })
                .collect::<Vec<_>>()
        };
        let blocks = [0, 1].map(|kind| {
            pipelines(
                &block_layout[kind],
                &input::shader(
                    include_str!("../denoise_probe.wgsl"),
                    "source",
                    "params[0].x",
                    kind,
                ),
                &["blocks"],
            )
            .remove(0)
        });
        let pipelines = pipelines(
            &layout,
            include_str!("models.wgsl"),
            &[
                "sort_samples",
                "quantiles",
                "cap_global",
                "band_statistics",
                "finish_model",
            ],
        );
        Self {
            block_layout,
            blocks,
            layout,
            pipelines: pipelines.try_into().ok().unwrap(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        coarse: bool,
    ) -> wgpu::Buffer {
        let nx = size[0].div_ceil(n as u32).min(32);
        let ny = size[1].div_ceil(n as u32).min(32);
        let count = nx * ny;
        let mut params = vec![[size[0] as f32, size[1] as f32, n as f32, count as f32]];
        params.extend(super::basis(n));
        for row in 0..ny {
            for column in 0..nx {
                params.push([
                    (column * size[0].saturating_sub(n as u32) / nx.saturating_sub(1).max(1))
                        as f32,
                    (row * size[1].saturating_sub(n as u32) / ny.saturating_sub(1).max(1)) as f32,
                    0.,
                    0.,
                ]);
            }
        }
        params.push([u32::from(coarse) as f32, amounts[0], amounts[1], threshold]);
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TR experimental global noise parameters"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let buffer = |size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TR experimental global noise scratch"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let samples = buffer(u64::from(count) * 257 * 16);
        let order = buffer(1026 * 4);
        let raw = buffer((6 * n * n * 3 + 24) as u64 * 4);
        let output = buffer(output_bytes(n));
        let bind = |layout: &wgpu::BindGroupLayout, buffers: &[&wgpu::Buffer]| {
            let entries: Vec<_> = buffers
                .iter()
                .enumerate()
                .map(|(binding, b)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            })
        };
        let block_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.block_layout[source.kind()],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                source.entry(),
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: samples.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.blocks[source.kind()]);
            pass.set_bind_group(0, &block_bind, &[]);
            pass.dispatch_workgroups(count, 1, 1);
        }
        let bind = bind(&self.layout, &[&params, &samples, &order, &raw, &output]);
        let dispatches = [
            [1, 1, 1],
            [(n * n) as u32, 3, 5],
            [3, 1, 1],
            [4, 3, 1],
            [((4 * n * n) as u32).div_ceil(256), 1, 1],
        ];
        for (pipeline, dispatch) in self.pipelines.iter().zip(dispatches) {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spectral_model_shader_validates_offline() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("models.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
    use eframe::wgpu;
}
