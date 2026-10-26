//! Resident point/vignette input to the spectral filter. The tone shader is
//! shared with qualified point editing; advanced color follows spatial detail.
use super::*;
use tr_core::{budget::MemoryBudget, editing::geometry::Geometry};

pub(super) struct Optics {
    buffer: wgpu::Buffer,
    amount: f32,
    size: [u32; 2],
    _credits: admission::Credits,
}
impl Optics {
    pub fn matches(&self, size: [u32; 2], amount: f32) -> bool {
        self.size == size && self.amount == amount
    }
    pub fn prepare(
        device: &wgpu::Device,
        size: [u32; 2],
        amount: f32,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        ensure!(
            !cancelled() && amount.is_finite() && (-100. ..=100.).contains(&amount),
            "Optical map cancelled or invalid"
        );
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let bytes = if amount == 0. { 4 } else { pixels * 4 };
        ensure!(
            pixels > 0
                && bytes
                    <= device
                        .limits()
                        .max_storage_buffer_binding_size
                        .min(device.limits().max_buffer_size),
            "Optical map exceeds device limits"
        );
        let credits = admission::Credits::new(memory, gpu, bytes * 2 + 65536)?;
        let geometry = Geometry {
            vignette: amount,
            ..Default::default()
        };
        let mut gains = Vec::with_capacity((bytes / 4) as usize);
        if amount == 0. {
            gains.push(1.);
        } else {
            for y in 0..size[1] {
                ensure!(!cancelled(), "Editing annullato");
                for x in 0..size[0] {
                    let mut pixel = [1.; 4];
                    geometry.optical_color(
                        &mut pixel,
                        (x as f32 + 0.5) / size[0] as f32,
                        (y as f32 + 0.5) / size[1] as f32,
                    );
                    gains.push(pixel[0]);
                }
            }
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TR canonical retained optical gains"),
            contents: bytemuck::cast_slice(&gains),
            usage: wgpu::BufferUsages::STORAGE,
        });
        ensure!(!cancelled(), "Editing annullato");
        Ok(Self {
            buffer,
            amount,
            size,
            _credits: credits,
        })
    }
}

pub(super) fn parameters(edit: &crate::live_edit::LiveEdit) -> Vec<u8> {
    let mut params = crate::editing_compute::parameters(edit);
    // Advanced color follows spatial detail; its unused words must not
    // invalidate the upstream checkpoint when only the color stage changes.
    params[4 * 16..5 * 16].fill(0);
    params[40 * 16..52 * 16].fill(0);
    let vignette = edit
        .recipe
        .advanced
        .as_ref()
        .map_or(0., |a| a.geometry.vignette);
    params[6 * 16..6 * 16 + 4].copy_from_slice(&(u32::from(vignette != 0.) as f32).to_le_bytes());
    params
}
fn shader() -> String {
    let source = include_str!("../editing_compute.wgsl");
    let (functions, _) = source.split_once("@compute").expect("point shader entry");
    let functions = functions.replace(
        "@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;",
        "@group(0) @binding(1) var source: texture_2d<f32>;")
        .replace("@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;",
        "@group(0) @binding(2) var output: texture_storage_2d<rgba32float, write>;\n@group(0) @binding(3) var<storage, read> optical: array<f32>;");
    format!("{functions}\n{}", include_str!("point.wgsl"))
}
pub(super) struct Point {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}
impl Point {
    pub fn new(device: &wgpu::Device) -> Self {
        let types = [
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            input::binding_type(1),
            wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba32Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        ];
        let entries: Vec<_> = types
            .into_iter()
            .enumerate()
            .map(|(binding, ty)| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty,
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
            label: Some("TR resident point spectral input"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("prefix"),
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
        source: &wgpu::TextureView,
        size: [u32; 2],
        optics: &Optics,
        edit: &crate::live_edit::LiveEdit,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Texture> {
        ensure!(!cancelled(), "Editing annullato");
        edit.recipe.validate()?;
        let vignette = edit
            .recipe
            .advanced
            .as_ref()
            .map_or(0., |a| a.geometry.vignette);
        ensure!(
            optics.size == size
                && optics.amount == vignette
                && edit
                    .input_gains
                    .into_iter()
                    .all(|v| v.is_finite() && (0.05..=20.).contains(&v)),
            "Point input identity or gains mismatch"
        );
        let format = wgpu::TextureFormat::Rgba32Float;
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING;
        ensure!(
            format
                .guaranteed_format_features(device.features())
                .allowed_usages
                .contains(usage)
                && size.into_iter().all(|v| v > 0
                    && v <= device.limits().max_texture_dimension_2d
                    && v.div_ceil(8) <= device.limits().max_compute_workgroups_per_dimension),
            "Native point texture unsupported"
        );
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("TR resident developed spectral input"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let params = parameters(edit);
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let resources = [
            params.as_entire_binding(),
            wgpu::BindingResource::TextureView(source),
            wgpu::BindingResource::TextureView(&view),
            optics.buffer.as_entire_binding(),
        ];
        let entries: Vec<_> = resources
            .into_iter()
            .enumerate()
            .map(|(binding, resource)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource,
            })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn point_prefix_shader_validates_offline() {
        let module = super::wgpu::naga::front::wgsl::parse_str(&super::shader()).unwrap();
        super::wgpu::naga::valid::Validator::new(
            super::wgpu::naga::valid::ValidationFlags::all(),
            super::wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
