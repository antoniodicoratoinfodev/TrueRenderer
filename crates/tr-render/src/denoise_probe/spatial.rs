//! Canonical spatial operators on an admitted region with complete halo.
use super::*;
use tr_core::editing::detail::Detail;

pub(super) struct Spatial {
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 5],
}
struct Node {
    amount: f32,
    radius: u32,
    gaussian: bool,
    midtones: bool,
    defringe: bool,
    weights: Vec<f32>,
}
fn nodes(detail: &Detail, defringe: f32, native: [u32; 2], full: [u32; 2]) -> Result<Vec<Node>> {
    detail.validate()?;
    ensure!(
        native.into_iter().chain(full).all(|v| v > 0)
            && detail.dehaze == 0.
            && !detail.grain.active()
            && defringe.is_finite()
            && (0. ..=100.).contains(&defringe),
        "Unsupported spatial continuation"
    );
    let scale = full[0].max(full[1]) as f32 / native[0].max(native[1]) as f32;
    let mut nodes = Vec::new();
    for (index, amount) in [detail.texture, detail.clarity, detail.sharpen, defringe]
        .into_iter()
        .enumerate()
    {
        if amount == 0. {
            continue;
        }
        let sigma = (detail.radius * scale).max(0.15);
        let radius = match index {
            0 => (2. * scale).round().max(1.) as u32,
            1 => (native[0].max(native[1]) as f32 * 0.008 * scale)
                .round()
                .clamp(1., 512.) as u32,
            2 => (sigma * 2.).ceil() as u32,
            _ => 1,
        };
        let weights = if index == 2 {
            let mut kernel: Vec<_> = (-(radius as i32)..=radius as i32)
                .map(|x| (-0.5 * (f64::from(x) / f64::from(sigma)).powi(2)).exp())
                .collect();
            let norm: f64 = kernel.iter().sum();
            for k in &mut kernel {
                *k /= norm;
            }
            kernel.into_iter().map(|k| k as f32).collect()
        } else {
            vec![1.]
        };
        nodes.push(Node {
            amount: amount / 100.,
            radius,
            gaussian: index == 2,
            midtones: index == 1,
            defringe: index == 3,
            weights,
        });
    }
    Ok(nodes)
}

pub(super) fn support(
    detail: &Detail,
    defringe: f32,
    native: [u32; 2],
    full: [u32; 2],
) -> Result<u32> {
    Ok(nodes(detail, defringe, native, full)?
        .iter()
        .map(|n| n.radius)
        .sum())
}
impl Spatial {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries: Vec<_> = (0..5)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: binding != 4,
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
            source: wgpu::ShaderSource::Wgsl(include_str!("spatial.wgsl").into()),
        });
        let pipelines = ["horizontal", "vertical", "spatial", "defringe", "crop"].map(|entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        Self { layout, pipelines }
    }
    fn pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        kind: usize,
        buffers: [&wgpu::Buffer; 5],
        pixels: u32,
    ) {
        let entries: Vec<_> = buffers
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
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipelines[kind]);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(pixels.div_ceil(256), 1, 1);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Buffer,
        size: [u32; 2],
        crop: [u32; 4],
        detail: &Detail,
        defringe: f32,
        native: [u32; 2],
        full: [u32; 2],
        recycle_crop: bool,
        reusable: &mut Vec<wgpu::Buffer>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Buffer> {
        ensure!(!cancelled(), "Editing annullato");
        let nodes = nodes(detail, defringe, native, full)?;
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        ensure!(
            pixels > 0
                && pixels * 16
                    <= device
                        .limits()
                        .max_storage_buffer_binding_size
                        .min(device.limits().max_buffer_size)
                && pixels.div_ceil(256)
                    <= u64::from(device.limits().max_compute_workgroups_per_dimension)
                && (0..2).all(|a| crop[a + 2] > 0
                    && u64::from(crop[a]) + u64::from(crop[a + 2]) <= u64::from(size[a])),
            "Invalid spatial region or device limit"
        );
        let buffer = |bytes| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        ensure!(
            reusable.len() <= 2 && reusable.iter().all(|b| b.size() >= pixels * 16),
            "Invalid recycled spatial scratch"
        );
        // The preceding merge has finished reading these buffers before this
        // pass starts. Queue ordering supplies the dependency; their credits
        // stay in the same completion until every downstream pass is finished.
        let low = reusable.pop().unwrap_or_else(|| buffer(pixels * 16));
        let ping = [
            reusable.pop().unwrap_or_else(|| buffer(pixels * 16)),
            buffer(pixels * 16),
        ];
        let storage = |bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let mut current = input;
        for (index, node) in nodes.iter().enumerate() {
            ensure!(!cancelled(), "Editing annullato");
            // Horizontal scratch is dead after the vertical pass. Reuse it for
            // this node's output, keeping the previous node immutable throughout.
            let next = &ping[index % 2];
            let words = [
                [size[0], size[1], u32::from(node.gaussian), node.radius],
                [
                    node.amount.to_bits(),
                    (detail.threshold / 100. * 0.02).to_bits(),
                    u32::from(node.midtones),
                    0,
                ],
                crop,
            ];
            let params = storage(bytemuck::cast_slice(&words));
            let weights = storage(bytemuck::cast_slice(&node.weights));
            self.pass(
                device,
                encoder,
                0,
                [&params, current, current, &weights, next],
                pixels as u32,
            );
            self.pass(
                device,
                encoder,
                1,
                [&params, next, current, &weights, &low],
                pixels as u32,
            );
            self.pass(
                device,
                encoder,
                if node.defringe { 3 } else { 2 },
                [&params, current, &low, &weights, next],
                pixels as u32,
            );
            current = next;
        }
        // The low-pass buffer is dead after the final node. A downstream
        // color pass can consume its cropped prefix without another allocation.
        // Standalone output retains an exactly sized, independently owned buffer.
        let output = if recycle_crop {
            low
        } else {
            buffer(u64::from(crop[2]) * u64::from(crop[3]) * 16)
        };
        let params = storage(bytemuck::cast_slice(&[
            [size[0], size[1], 0, 0],
            [0; 4],
            crop,
        ]));
        let dummy = storage(&[0; 16]);
        self.pass(
            device,
            encoder,
            4,
            [&params, current, current, &dummy, &output],
            crop[2] * crop[3],
        );
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spatial_shader_validates_offline() {
        let module =
            super::wgpu::naga::front::wgsl::parse_str(include_str!("spatial.wgsl")).unwrap();
        super::wgpu::naga::valid::Validator::new(
            super::wgpu::naga::valid::ValidationFlags::all(),
            super::wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
