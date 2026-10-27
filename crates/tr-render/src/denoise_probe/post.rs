//! Advanced color and legacy masks after spatial detail, with immutable input guides.
use super::*;
use tr_core::editing::masks::{Mask, Shape};

pub(super) fn shader() -> String {
    let (functions, _) = include_str!("../editing_compute.wgsl")
        .split_once("@compute")
        .expect("point functions");
    format!("{functions}\n{}", include_str!("post.wgsl"))
}
pub(super) struct Post {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}
impl Post {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries: Vec<_> = (0..5)
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
            label: Some("TR resident color and mask continuation"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("post"),
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
        edit: &crate::live_edit::LiveEdit,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Buffer> {
        ensure!(!cancelled(), "Editing annullato");
        edit.recipe.validate()?;
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
            "Invalid post-detail region or device limit"
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
        let mut segments = Vec::<[f32; 4]>::new();
        if let Some(advanced) = &edit.recipe.advanced {
            for m in advanced.masks.iter().filter(|m| m.active()) {
                ensure!(!cancelled(), "Editing annullato");
                append_mask(m, aspect, &mut masks, &mut segments);
            }
        }
        if segments.is_empty() {
            segments.push([0.; 4]);
        }
        let storage = |data: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: data,
                usage,
            })
        };
        let params = storage(
            &crate::editing_compute::parameters(edit),
            wgpu::BufferUsages::UNIFORM,
        );
        let masks = storage(bytemuck::cast_slice(&masks), wgpu::BufferUsages::STORAGE);
        let segments = storage(bytemuck::cast_slice(&segments), wgpu::BufferUsages::STORAGE);
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: pixels * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let entries: Vec<_> = [&params, input, &output, &masks, &segments]
            .into_iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                // Spatial scratch can exceed the logical cropped raster. Bind
                // only its initialized prefix, including the partial last group.
                resource: if binding == 1 {
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(pixels * 16),
                    })
                } else {
                    buffer.as_entire_binding()
                },
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
pub(super) fn append_mask(
    m: &Mask,
    aspect: f32,
    masks: &mut Vec<[f32; 4]>,
    segments: &mut Vec<[f32; 4]>,
) {
    let begin = segments.len();
    if m.shape == Shape::Brush {
        for p in &m.points {
            segments.push([p[0], p[1] * aspect, p[0], p[1] * aspect]);
        }
        for (i, pair) in m.points.windows(2).enumerate() {
            if !m.breaks.contains(&(i + 1)) {
                segments.push([
                    pair[0][0],
                    pair[0][1] * aspect,
                    pair[1][0],
                    pair[1][1] * aspect,
                ]);
            }
        }
    }
    let (sin, cos) = m.angle.to_radians().sin_cos();
    let shape = match m.shape {
        Shape::Radial => 0.,
        Shape::Linear => 1.,
        Shape::Brush => 2.,
        Shape::Luminance => 3.,
        Shape::Hue => 4.,
    };
    masks.extend([
        [m.center[0], m.center[1], m.radius, m.feather],
        [shape, u32::from(m.invert) as f32, sin, cos],
        [
            m.interval[0],
            m.interval[1],
            m.exposure.exp2(),
            1. + m.saturation / 100.,
        ],
        [
            (m.warmth * 0.0018).exp(),
            1.,
            (-m.warmth * 0.0018).exp(),
            begin as f32,
        ],
        [(segments.len() - begin) as f32, 0., 0., 0.],
    ]);
    masks[1][1] += 1.;
}
#[cfg(test)]
mod tests {
    #[test]
    fn post_detail_shader_validates_offline() {
        let module = super::wgpu::naga::front::wgsl::parse_str(&super::shader()).unwrap();
        super::wgpu::naga::valid::Validator::new(
            super::wgpu::naga::valid::ValidationFlags::all(),
            super::wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
