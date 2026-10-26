//! Mask graphs evaluated from the actual GPU layer input, including color guides.
use super::*;
use tr_core::editing::layers::{MaskGraph, MaskKind};

fn shader() -> String {
    let full = post::shader();
    let (functions, _) = full.split_once("@compute").expect("post entry");
    let functions = functions.replace(
        "var<storage, read_write> output: array<vec4<f32>>;",
        "var<storage, read_write> output: array<f32>;",
    );
    format!("{functions}\n{}", include_str!("mask.wgsl"))
}
pub(super) struct Masks {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}
impl Masks {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries: Vec<_> = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 2,
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
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(shader().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("TR dynamic layer mask"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("coverage"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { layout, pipeline }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Buffer,
        origin: [u32; 2],
        size: [u32; 2],
        full: [u32; 2],
        native: [u32; 2],
        graph: &MaskGraph,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Buffer> {
        ensure!(!cancelled(), "Editing annullato");
        graph.validate()?;
        ensure!(
            graph
                .nodes
                .iter()
                .map(|node| match &node.kind {
                    MaskKind::Shape(m) => m.points.len(),
                    _ => 0,
                })
                .sum::<usize>()
                <= 512,
            "Layer mask points exceed admitted metadata"
        );
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        ensure!(
            native.into_iter().all(|v| v > 0)
                && (0..2).all(|a| size[a] > 0
                    && u64::from(origin[a]) + u64::from(size[a]) <= u64::from(full[a]))
                && pixels * 16
                    <= device
                        .limits()
                        .max_storage_buffer_binding_size
                        .min(device.limits().max_buffer_size)
                && pixels.div_ceil(256)
                    <= u64::from(device.limits().max_compute_workgroups_per_dimension),
            "Invalid mask region or device limit"
        );
        let aspect = native[1] as f32 / native[0] as f32;
        let mut masks = vec![
            [
                origin[0] as f32,
                origin[1] as f32,
                full[0] as f32,
                full[1] as f32,
            ],
            [aspect, 0., 0., 0.],
            [size[0] as f32, size[1] as f32, 0., 0.],
        ];
        let mut segments = Vec::new();
        let index = |id| graph.nodes.iter().position(|node| node.id == id).unwrap() as f32;
        let mut words = vec![[
            graph.root.map_or(0., index),
            graph.nodes.len() as f32,
            0.,
            0.,
        ]];
        for node in &graph.nodes {
            ensure!(!cancelled(), "Editing annullato");
            let mut record = [[0.; 4]; 4];
            record[0] = match &node.kind {
                MaskKind::Shape(m) => {
                    let index = masks[1][1];
                    post::append_mask(m, aspect, &mut masks, &mut segments);
                    [0., index, 0., 0.]
                }
                MaskKind::Color(range) => {
                    let rgb = range.reference;
                    let high = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
                    let low = rgb.into_iter().fold(f32::INFINITY, f32::min);
                    let chroma = high - low;
                    let (h, c) = if chroma <= 1e-8 {
                        (0., 0.)
                    } else if high == rgb[0] {
                        (((rgb[1] - rgb[2]) / chroma).rem_euclid(6.) * 60., chroma)
                    } else if high == rgb[1] {
                        (((rgb[2] - rgb[0]) / chroma + 2.) * 60., chroma)
                    } else {
                        (((rgb[0] - rgb[1]) / chroma + 4.) * 60., chroma)
                    };
                    record[1] = [
                        h,
                        c,
                        0.2627 * rgb[0] + 0.678 * rgb[1] + 0.0593 * rgb[2],
                        (c * 0.01).max(1e-6),
                    ];
                    record[2] = [
                        range.width[0],
                        range.width[1],
                        range.width[2],
                        range.softness,
                    ];
                    [1., 0., 0., 0.]
                }
                MaskKind::Constant(v) => [2., *v, 0., 0.],
                MaskKind::Add(a, b) => [3., index(*a), index(*b), 0.],
                MaskKind::Subtract(a, b) => [4., index(*a), index(*b), 0.],
                MaskKind::Intersect(a, b) => [5., index(*a), index(*b), 0.],
                MaskKind::Invert(a) => [6., index(*a), 0., 0.],
            };
            words.extend(record);
        }
        ensure!(
            segments.len() <= 1024,
            "Layer brush points exceed admitted metadata"
        );
        if segments.is_empty() {
            segments.push([0.; 4]);
        }
        let storage = |bytes: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage,
            })
        };
        let params = storage(&[0; 832], wgpu::BufferUsages::UNIFORM);
        let masks = storage(bytemuck::cast_slice(&masks), wgpu::BufferUsages::STORAGE);
        let segments = storage(bytemuck::cast_slice(&segments), wgpu::BufferUsages::STORAGE);
        let graph = storage(bytemuck::cast_slice(&words), wgpu::BufferUsages::STORAGE);
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: pixels * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let entries: Vec<_> = [&params, input, &output, &masks, &segments, &graph]
            .into_iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        ensure!(!cancelled(), "Editing annullato");
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((pixels as u32).div_ceil(256), 1, 1);
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn layer_mask_shader_validates_offline() {
        let module = super::wgpu::naga::front::wgsl::parse_str(&super::shader()).unwrap();
        super::wgpu::naga::valid::Validator::new(
            super::wgpu::naga::valid::ValidationFlags::all(),
            super::wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
