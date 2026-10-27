//! Experimental single-scale command encoding shared by the precision probes.
use super::*;

// A stripe reuses this scratch after its gather pass. Models and output retain
// the original domain; splitting changes neither the patch lattice nor sums.
pub(super) const PATCH_LIMIT: u64 = 32 * 1024 * 1024;

struct Stripe {
    origin: [u32; 2],
    size: [u32; 2],
    first: [i32; 2],
    grid: [u32; 2],
    count: u32,
    offset: u32,
}

impl Stripe {
    fn new(origin: [u32; 2], size: [u32; 2], n: usize, offset: u32) -> Self {
        let step = (n / 4) as i32;
        let first = origin.map(|v| ((v as i32 - n as i32).div_euclid(step) + 1) * step);
        let end = [origin[0] + size[0], origin[1] + size[1]].map(|v| v as i32);
        let grid = [
            ((end[0] - first[0] + 2 * step - 1) / (2 * step)) as u32,
            ((end[1] - first[1] + step - 1) / step) as u32,
        ];
        Self {
            origin,
            size,
            first,
            grid,
            count: grid[0] * grid[1],
            offset,
        }
    }
    fn bytes(&self, n: usize) -> u64 {
        u64::from(self.count) * n as u64 * n as u64 * 16
    }
}

fn stripes(origin: [u32; 2], size: [u32; 2], n: usize, limit: u64) -> Result<Vec<Stripe>> {
    let mut stripes = Vec::new();
    let mut row = 0;
    while row < size[1] {
        let start = [origin[0], origin[1] + row];
        let offset = row * size[0];
        ensure!(
            Stripe::new(start, [size[0], 1], n, offset).bytes(n) <= limit,
            "Spectral stripe cannot fit one row and its support"
        );
        let (mut low, mut high) = (1, size[1] - row);
        while low < high {
            let mid = low + (high - low).div_ceil(2);
            if Stripe::new(start, [size[0], mid], n, offset).bytes(n) <= limit {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        stripes.push(Stripe::new(start, [size[0], low], n, offset));
        row += low;
    }
    Ok(stripes)
}

pub(super) fn patch_bytes(
    origin: [u32; 2],
    size: [u32; 2],
    n: usize,
    limits: &wgpu::Limits,
) -> Result<u64> {
    let limit = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size)
        .min(PATCH_LIMIT);
    Ok(stripes(origin, size, n, limit)?
        .iter()
        .map(|s| s.bytes(n))
        .max()
        .unwrap())
}

pub(super) fn working_bytes(
    origin: [u32; 2],
    size: [u32; 2],
    n: usize,
    limits: &wgpu::Limits,
) -> Result<u64> {
    let limit = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    let output = u64::from(size[0]) * u64::from(size[1]) * 16;
    ensure!(
        output > 0 && output <= limit,
        "Spectral output exceeds device limits"
    );
    let stripes = stripes(origin, size, n, PATCH_LIMIT.min(limit))?;
    let patches = stripes.iter().map(|s| s.bytes(n)).max().unwrap();
    let mut total = output + patches + 8 * 1024 * 1024;
    // Global models: at most 1024 samples of 257 vec4, order/statistics,
    // parameters and model output fit inside the 8 MiB allowance above.
    for stripe in stripes {
        ensure!(
            stripe.count.div_ceil(256) <= limits.max_compute_workgroups_per_dimension
                && (stripe.size[0] * stripe.size[1]).div_ceil(256)
                    <= limits.max_compute_workgroups_per_dimension,
            "Spectral stripe dispatch exceeds device limits"
        );
        // Both the packed CPU parameters and their initialized GPU buffer,
        // plus allowance for binding/command records and allocation alignment.
        total = total
            .checked_add((1 + n * n / 4 + stripe.count as usize + 3) as u64 * 32 + 65536)
            .ok_or_else(|| anyhow::anyhow!("Spectral allocation overflow"))?;
    }
    Ok(total)
}

pub(super) struct Filter {
    models: models::Models,
    layout: [wgpu::BindGroupLayout; 3],
    pipelines: [[wgpu::ComputePipeline; 2]; 3],
}
impl Filter {
    pub fn new(device: &wgpu::Device) -> Self {
        let prepared = [0, 1, 2].map(|kind| {
            let mut entries: Vec<_> = (0..5)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: if binding == 1 {
                        input::binding_type(kind)
                    } else {
                        wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: binding != 2 && binding != 4,
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
                    input::shader(&transform::shader(), "source", "params[0].x", kind).into(),
                ),
            });
            let pipelines = ["reconstruct", "gather"].map(|entry| {
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
        let [buffer, texture, point] = prepared;
        Self {
            models: models::Models::new(device),
            layout: [buffer.0, texture.0, point.0],
            pipelines: [buffer.1, texture.1, point.1],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        coarse: bool,
        origin: [u32; 2],
        dimensions: [u32; 2],
    ) -> Result<(wgpu::Buffer, wgpu::Buffer)> {
        self.encode_cancellable(
            device,
            encoder,
            input,
            size,
            n,
            amounts,
            threshold,
            coarse,
            origin,
            dimensions,
            &|| false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode_cancellable(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        coarse: bool,
        origin: [u32; 2],
        dimensions: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<(wgpu::Buffer, wgpu::Buffer)> {
        self.encode_with_scratch(
            device, encoder, input, size, n, amounts, threshold, coarse, origin, dimensions, None,
            cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_with_scratch(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: input::Input<'_>,
        size: [u32; 2],
        n: usize,
        amounts: [f32; 2],
        threshold: f32,
        coarse: bool,
        origin: [u32; 2],
        dimensions: [u32; 2],
        shared: Option<&wgpu::Buffer>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<(wgpu::Buffer, wgpu::Buffer)> {
        ensure!(!cancelled(), "Editing annullato");
        ensure!(
            [4, 8, 16].contains(&n)
                && (0..2).all(|a| dimensions[a] > 0
                    && u64::from(origin[a]) + u64::from(dimensions[a]) <= u64::from(size[a])),
            "Invalid experimental spectral region"
        );
        let output_bytes = u64::from(dimensions[0]) * u64::from(dimensions[1]) * 16;
        let limits = device.limits();
        let binding_limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        ensure!(
            size.into_iter().all(|v| v <= i32::MAX as u32 - 32)
                && output_bytes <= binding_limit
                && output_bytes / 16 <= u64::from(u32::MAX),
            "Spectral output exceeds device limits"
        );
        let stripes = stripes(origin, dimensions, n, PATCH_LIMIT.min(binding_limit))?;
        let patch_bytes = stripes.iter().map(|s| s.bytes(n)).max().unwrap();
        let buffer = |size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let owned = shared
            .is_none()
            .then(|| buffer(patch_bytes, wgpu::BufferUsages::STORAGE));
        let patches = shared.unwrap_or_else(|| owned.as_ref().unwrap());
        ensure!(
            patches.size() >= patch_bytes && patches.size() <= binding_limit,
            "Shared spectral scratch outside bounds"
        );
        let output = buffer(
            output_bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let model = self
            .models
            .encode(device, encoder, input, size, n, amounts, threshold, coarse);
        for stripe in stripes {
            ensure!(!cancelled(), "Editing annullato");
            let count = stripe.count;
            let pixels = stripe.size[0] * stripe.size[1];
            ensure!(
                count.div_ceil(256) <= limits.max_compute_workgroups_per_dimension
                    && pixels.div_ceil(256) <= limits.max_compute_workgroups_per_dimension,
                "Spectral stripe dispatch exceeds device limits"
            );
            let step = (n / 4) as i32;
            let mut params = vec![[size[0] as f32, size[1] as f32, n as f32, count as f32]];
            params.extend(super::basis(n));
            for row in 0..stripe.grid[1] {
                if row % 32 == 0 {
                    ensure!(!cancelled(), "Editing annullato");
                }
                let y = stripe.first[1] + row as i32 * step;
                let start =
                    stripe.first[0] + (stripe.first[0] / step + y / step).rem_euclid(2) * step;
                for column in 0..stripe.grid[0] {
                    params.push([(start + column as i32 * 2 * step) as f32, y as f32, 0., 0.]);
                }
            }
            params.push([
                stripe.origin[0] as f32,
                stripe.origin[1] as f32,
                stripe.size[0] as f32,
                stripe.size[1] as f32,
            ]);
            params.push([
                stripe.first[0] as f32,
                stripe.first[1] as f32,
                stripe.grid[0] as f32,
                stripe.grid[1] as f32,
            ]);
            params.push([stripe.offset as f32, 0., 0., 0.]);
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let mut entries: Vec<_> = [(0, &params), (2, patches), (3, &model), (4, &output)]
                .into_iter()
                .map(|(binding, b)| wgpu::BindGroupEntry {
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
            for (pipeline, dispatch) in self.pipelines[input.kind()].iter().zip([
                [count.min(256), count.div_ceil(256)],
                [pixels.div_ceil(256), 1],
            ]) {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(dispatch[0], dispatch[1], 1);
            }
        }
        Ok((output, model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stripes_cover_odd_regions_and_keep_canonical_support_within_quota() {
        for n in [4, 8, 16] {
            for origin in [[0, 0], [119, 79], [240, 160]] {
                let size = [1297, 865];
                let stripes = stripes(origin, size, n, PATCH_LIMIT).unwrap();
                assert!(stripes.len() > 1);
                let mut row = 0;
                for stripe in stripes {
                    assert_eq!(stripe.origin, [origin[0], origin[1] + row]);
                    assert_eq!(stripe.offset, row * size[0]);
                    assert_eq!(stripe.size[0], size[0]);
                    assert!(stripe.bytes(n) <= PATCH_LIMIT);
                    // The first and last pixel row must each retain all four
                    // overlapping patch rows, including negative reflected edges.
                    let step = (n / 4) as i32;
                    for y in [stripe.origin[1], stripe.origin[1] + stripe.size[1] - 1] {
                        let last = y as i32 / step * step;
                        for patch_y in (0..4).map(|i| last - i * step) {
                            assert!(patch_y >= stripe.first[1]);
                            assert!((patch_y - stripe.first[1]) / step < stripe.grid[1] as i32);
                        }
                    }
                    row += stripe.size[1];
                }
                assert_eq!(row, size[1]);
            }
        }
        assert!(stripes([0, 0], [8192, 1], 16, 1024).is_err());
    }
}
