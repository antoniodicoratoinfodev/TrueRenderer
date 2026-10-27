//! Budgeted experimental jobs. Encoding never submits; sources, outputs and
//! pending scratch have independent owners and retain credits through completion.
use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tr_core::budget::{Lease, MemoryBudget};

#[derive(Debug)]
pub(super) struct Quota {
    resource: &'static str,
    requested: u64,
    reserved: u64,
    limit: u64,
}
impl Quota {
    pub(super) fn new(resource: &'static str, budget: &MemoryBudget, requested: u64) -> Self {
        let usage = budget.usage();
        Self {
            resource,
            requested,
            reserved: usage.reserved,
            limit: usage.limit,
        }
    }
}
impl std::fmt::Display for Quota {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Quota {} denoiser: richiesti {}, occupati {}, limite {}",
            self.resource, self.requested, self.reserved, self.limit
        )
    }
}
impl std::error::Error for Quota {}

pub(super) struct Credits {
    _memory: Lease,
    _gpu: Lease,
}
impl Credits {
    fn within(memory: &MemoryBudget, gpu: &MemoryBudget, bytes: u64) -> Option<Self> {
        let memory = memory.try_reserve_within_limit(bytes)?;
        let gpu = gpu.try_reserve_within_limit(bytes)?;
        Some(Self {
            _memory: memory,
            _gpu: gpu,
        })
    }
    pub fn new(memory: &MemoryBudget, gpu: &MemoryBudget, bytes: u64) -> Result<Self> {
        let memory = memory
            .try_reserve(bytes)
            .ok_or_else(|| Quota::new("RAM", memory, bytes))?;
        let gpu = gpu
            .try_reserve(bytes)
            .ok_or_else(|| Quota::new("GPU", gpu, bytes))?;
        Ok(Self {
            _memory: memory,
            _gpu: gpu,
        })
    }
}

pub(super) struct Source {
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: [u32; 2],
    pub packed: bool,
    pub submitted: AtomicBool,
    _credits: Credits,
}
pub(crate) struct Upload {
    source: Arc<Source>,
    commands: wgpu::CommandBuffer,
    staging: Credits,
}
impl Upload {
    pub(super) fn source(&self) -> Arc<Source> {
        self.source.clone()
    }
    pub(super) fn prepare(
        device: &wgpu::Device,
        source: &LinearImage,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        Self::prepare_inner(device, source, memory, gpu, false, cancelled)
    }
    pub(super) fn prepare_point(
        device: &wgpu::Device,
        source: &LinearImage,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        let mut packed = true;
        for pixels in source.pixels.chunks(2048) {
            ensure!(!cancelled(), "Editing annullato");
            if pixels.iter().any(|p| p[3] != 1.) {
                packed = false;
                break;
            }
        }
        Self::prepare_inner(device, source, memory, gpu, packed, cancelled)
    }
    fn prepare_inner(
        device: &wgpu::Device,
        source: &LinearImage,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        packed: bool,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        ensure!(!cancelled(), "Editing annullato");
        let size = [source.width, source.height];
        ensure!(
            size.into_iter()
                .all(|v| v > 0 && v <= device.limits().max_texture_dimension_2d),
            "Native spectral texture exceeds device limits"
        );
        let pitch = (u64::from(source.width) * 16).div_ceil(256) * 256;
        let height = u64::from(input::texture_height(size, packed));
        let bytes = pitch * height + 65536;
        let credits = Credits::new(memory, gpu, bytes)?;
        // All staged stripes are retained by one unsubmitted command buffer.
        // A 128-row chunk bounds each allocation, not the total upload memory.
        let rows = (device.limits().max_buffer_size / pitch).min(128);
        ensure!(rows > 0, "Native spectral upload row exceeds device limit");
        let staging = Credits::new(
            memory,
            gpu,
            bytes + pitch * rows + height.div_ceil(rows) * 65536,
        )?;
        let mut encoder = device.create_command_encoder(&Default::default());
        let texture = input::upload_point(device, &mut encoder, source, packed, cancelled)?;
        let view = texture.create_view(&Default::default());
        ensure!(!cancelled(), "Editing annullato");
        Ok(Self {
            source: Arc::new(Source {
                _texture: texture,
                view,
                size,
                packed,
                submitted: AtomicBool::new(false),
                _credits: credits,
            }),
            commands: encoder.finish(),
            staging,
        })
    }
    pub(super) fn submit(
        self,
        queue: &wgpu::Queue,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Source>> {
        ensure!(!cancelled(), "Editing annullato");
        let Self {
            source,
            commands,
            staging,
        } = self;
        let retained = source.clone();
        queue.submit([commands]);
        source.submitted.store(true, Ordering::Release);
        queue.on_submitted_work_done(move || drop((retained, staging)));
        Ok(source)
    }
    pub(super) fn into_parts(self) -> (Arc<Source>, wgpu::CommandBuffer, Credits) {
        (self.source, self.commands, self.staging)
    }
    pub(crate) fn submit_background(
        self,
        queue: &wgpu::Queue,
        done: impl FnOnce() + Send + 'static,
    ) {
        // The caller checked cancellation before entering the UI submission.
        // Notify preparation only after staging has been released on completion.
        let Self {
            source,
            commands,
            staging,
        } = self;
        queue.submit([commands]);
        source.submitted.store(true, Ordering::Release);
        queue.on_submitted_work_done(move || {
            drop((source, staging));
            done();
        });
    }
}

#[derive(Clone, Copy)]
pub(super) struct Request {
    pub n: usize,
    pub amounts: [f32; 2],
    pub threshold: f32,
    pub levels: u32,
    pub origin: [u32; 2],
    pub size: [u32; 2],
}
impl Request {
    fn validate(self, source: [u32; 2]) -> Result<()> {
        ensure!(
            [4, 8, 16].contains(&self.n)
                && self.levels <= 3
                && self
                    .amounts
                    .into_iter()
                    .chain([self.threshold])
                    .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
                && (0..2).all(|a| self.size[a] > 0
                    && u64::from(self.origin[a]) + u64::from(self.size[a]) <= u64::from(source[a])),
            "Invalid admitted spectral request"
        );
        Ok(())
    }
}
pub(super) struct Output {
    pub buffer: wgpu::Buffer,
    _credits: Credits,
}
pub(super) struct Pending {
    commands: wgpu::CommandBuffer,
    source: Arc<Source>,
    output: Arc<Output>,
    scratch: Credits,
    optics: Option<Arc<point::Optics>>,
    upstream: Option<Arc<checkpoint::Entry>>,
    timer: Option<crate::timings::GpuTimer>,
}
/// A UI submission owns all upstream resources until its completion callback.
/// Merely encoding or dropping a frame cannot make an upload/checkpoint ready.
pub(crate) struct Completion {
    source: Arc<Source>,
    output: Arc<Output>,
    _scratch: Credits,
    _optics: Option<Arc<point::Optics>>,
    upstream: Option<Arc<checkpoint::Entry>>,
    pub(super) upload: Option<Credits>,
    timer: std::sync::Mutex<Option<crate::timings::GpuTimer>>,
    // Last field: wake waiting encoders after the job credits above are freed.
    pub(super) flight: Option<chain::Flight>,
}
impl Completion {
    pub(crate) fn completed(&self) {
        if let Some(optics) = &self._optics {
            optics.completed();
        }
    }
    pub(crate) fn submitted(&self, queue: &wgpu::Queue) {
        self.source.submitted.store(true, Ordering::Release);
        if let Some(upstream) = &self.upstream {
            upstream.submitted();
        }
        if let Some(flight) = &self.flight {
            flight.submitted();
        }
        let timer = self.timer.lock().unwrap().take();
        if let Some(timer) = timer {
            timer.submitted(queue);
        }
    }
    pub(crate) fn buffer(&self) -> &wgpu::Buffer {
        &self.output.buffer
    }
}
impl Pending {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        device: &wgpu::Device,
        pipeline: &multiscale::Multiscale,
        source: Arc<Source>,
        request: Request,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        Self::prepare_inner(
            device, pipeline, source, request, memory, gpu, None, None, None, None, cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_inner(
        device: &wgpu::Device,
        pipeline: &multiscale::Multiscale,
        source: Arc<Source>,
        request: Request,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        point: Option<(
            &point::Point,
            Arc<point::Optics>,
            &crate::live_edit::LiveEdit,
        )>,
        spatial: Option<(&spatial::Spatial, [u32; 2])>,
        post: Option<(&post::Post, [u32; 2])>,
        layers: Option<(&layers::Layers, [u32; 2])>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        Self::prepare_cached(
            device, pipeline, source, request, memory, gpu, point, spatial, post, layers, None,
            cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_cached(
        device: &wgpu::Device,
        pipeline: &multiscale::Multiscale,
        source: Arc<Source>,
        request: Request,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        point: Option<(
            &point::Point,
            Arc<point::Optics>,
            &crate::live_edit::LiveEdit,
        )>,
        spatial: Option<(&spatial::Spatial, [u32; 2])>,
        post: Option<(&post::Post, [u32; 2])>,
        layers: Option<(&layers::Layers, [u32; 2])>,
        checkpoint: Option<&checkpoint::Checkpoint>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        ensure!(!cancelled(), "Editing annullato");
        request.validate(source.size)?;
        ensure!(
            !source.packed || point.is_some(),
            "Packed source needs point reader"
        );
        ensure!(
            checkpoint.is_none() || (point.is_some() && spatial.is_some()),
            "Checkpoint needs point and spatial continuation"
        );
        let mut work_origin = request.origin;
        let mut work_size = request.size;
        if let Some((_, native)) = spatial {
            let edit = point
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Spatial continuation needs point parameters"))?
                .2;
            let advanced = edit
                .recipe
                .advanced
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing spatial parameters"))?;
            let mut support = advanced.detail.clone();
            let defringe = if checkpoint.is_some() {
                // Keep a stable native support through zero and downstream
                // amount changes. Only a changed radius/domain invalidates it.
                support.texture = 1.;
                support.clarity = 1.;
                support.sharpen = 1.;
                1.
            } else {
                advanced.geometry.defringe
            };
            let radius = spatial::support(&support, defringe, native, source.size)?;
            work_origin = request.origin.map(|v| v.saturating_sub(radius));
            work_size = std::array::from_fn(|a| {
                (request.origin[a] + request.size[a])
                    .saturating_add(radius)
                    .min(source.size[a])
                    - work_origin[a]
            });
        }
        let mut key = checkpoint
            .map(|_| {
                let (_, optics, edit) = point.as_ref().unwrap();
                checkpoint::Key::new(
                    &source,
                    optics,
                    edit,
                    Request {
                        origin: work_origin,
                        size: work_size,
                        ..request
                    },
                )
            })
            .transpose()?;
        let cached = checkpoint.and_then(|cache| cache.get(key.as_ref().unwrap(), memory, gpu));
        let bytes = multiscale::working_bytes(
            &device.limits(),
            source.size,
            request.n,
            request.amounts[1],
            request.levels,
            work_origin,
            work_size,
        )?;
        let output_bytes = u64::from(request.size[0]) * u64::from(request.size[1]) * 16;
        let checkpoint_bytes = u64::from(work_size[0]) * u64::from(work_size[1]) * 16 + 65536;
        let checkpoint_credits = if checkpoint.is_some()
            && cached.is_none()
            && checkpoint_bytes <= checkpoint::Checkpoint::limit(memory, gpu)
        {
            Credits::within(memory, gpu, checkpoint_bytes)
        } else {
            None
        };
        let publish_checkpoint = checkpoint_credits.is_some();
        let output_credits = Credits::new(memory, gpu, output_bytes + 65536)?;
        let point_bytes = point.as_ref().map_or(0, |(pipeline, _, _)| {
            2 * 65536
                + if pipeline.fused {
                    0
                } else {
                    (u64::from(source.size[0]) * 16).div_ceil(256) * 256 * u64::from(source.size[1])
                }
        });
        let reused_spatial = if cached.is_none() && spatial.is_some() {
            multiscale::spatial_reuse_count(
                source.size,
                work_origin,
                work_size,
                request.amounts[1],
                request.levels,
            )
        } else {
            0
        };
        let spatial_bytes = if spatial.is_some() {
            u64::from(work_size[0]) * u64::from(work_size[1]) * 16 * (3 - reused_spatial)
                + if post.is_some() { 0 } else { output_bytes }
                + 6 * 65536
        } else {
            0
        };
        let layer_bytes = if layers.is_some() {
            output_bytes + output_bytes / 4 + 65536
        } else {
            0
        };
        let upstream_bytes = if cached.is_some() {
            0
        } else {
            (bytes + point_bytes)
                .checked_sub(if publish_checkpoint {
                    checkpoint_bytes
                } else {
                    0
                })
                .ok_or_else(|| anyhow::anyhow!("Checkpoint estimate underflow"))?
        };
        let scratch_bytes = (upstream_bytes
            + 65536
            + spatial_bytes
            + layer_bytes
            + if post.is_some() {
                output_bytes + 65536
            } else {
                0
            })
        // The final buffer has its own lease even when the upstream is
        // cached. Account for it once in both paths.
        .checked_sub(output_bytes)
        .ok_or_else(|| anyhow::anyhow!("Output estimate underflow"))?;
        let scratch = Credits::new(memory, gpu, scratch_bytes)?;
        let _encoding = crate::timings::CpuSpan::new("graph_encode_cpu")
            .bytes(u64::from(work_size[0]) * u64::from(work_size[1]) * 16);
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut timer = crate::timings::GpuTimer::new(device, memory, gpu);
        if let Some(t) = &mut timer {
            t.begin(&mut encoder, "gpu_graph_upstream");
        }
        let developed = point
            .as_ref()
            .filter(|(pipeline, _, _)| cached.is_none() && !pipeline.fused)
            .map(|(pipeline, optics, edit)| {
                pipeline.encode(
                    device,
                    &mut encoder,
                    &source.view,
                    source.size,
                    source.packed,
                    optics,
                    edit,
                    cancelled,
                )
            })
            .transpose()?;
        let developed_view = developed
            .as_ref()
            .map(|t| t.create_view(&Default::default()));
        let point_parameters = point
            .as_ref()
            .filter(|(pipeline, _, _)| cached.is_none() && pipeline.fused)
            .map(|(_, _, edit)| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("TR fused point source parameters"),
                    contents: &point::source_parameters(edit, source.packed),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
            });
        let input = if let Some(parameters) = &point_parameters {
            input::Input::Point {
                source: &source.view,
                parameters,
                optics: &point.as_ref().unwrap().1.buffer,
            }
        } else {
            input::Input::Texture(developed_view.as_ref().unwrap_or(&source.view))
        };
        let mut reusable = Vec::new();
        let mut uncached = if cached.is_none() {
            Some(pipeline.encode_cancellable(
                device,
                &mut encoder,
                input,
                source.size,
                request.n,
                request.amounts,
                request.threshold,
                request.levels,
                false,
                work_origin,
                work_size,
                &mut reusable,
                cancelled,
            )?)
        } else {
            None
        };
        let upstream = if let Some(credits) = checkpoint_credits {
            Some(Arc::new(checkpoint::Entry::new(
                uncached.take().unwrap(),
                key.take().unwrap(),
                checkpoint_bytes,
                credits,
            )))
        } else {
            cached
        };
        if let Some(t) = &mut timer {
            t.end(&mut encoder);
            t.begin(&mut encoder, "gpu_graph_detail");
        }
        let buffer = if let Some((spatial, native)) = spatial {
            ensure!(
                reusable.len() as u64 == reused_spatial,
                "Spatial reuse differs from admitted scratch"
            );
            let advanced = point.as_ref().unwrap().2.recipe.advanced.as_ref().unwrap();
            spatial.encode(
                device,
                &mut encoder,
                upstream
                    .as_ref()
                    .map_or_else(|| uncached.as_ref().unwrap(), |entry| &entry.buffer),
                work_size,
                [
                    request.origin[0] - work_origin[0],
                    request.origin[1] - work_origin[1],
                    request.size[0],
                    request.size[1],
                ],
                &advanced.detail,
                advanced.geometry.defringe,
                native,
                source.size,
                post.is_some(),
                &mut reusable,
                cancelled,
            )?
        } else {
            uncached.take().unwrap()
        };
        if let Some(t) = &mut timer {
            t.end(&mut encoder);
            t.begin(&mut encoder, "gpu_graph_color");
        }
        let buffer = if let Some((post, native)) = post {
            let edit = point
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Post-detail continuation needs recipe"))?
                .2;
            post.encode(
                device,
                &mut encoder,
                &buffer,
                request.origin,
                request.size,
                source.size,
                native,
                edit,
                cancelled,
            )?
        } else {
            buffer
        };
        if let Some(t) = &mut timer {
            t.end(&mut encoder);
            t.begin(&mut encoder, "gpu_graph_layers");
        }
        let buffer = if let Some((layers, native)) = layers {
            let edit = point
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Layer continuation needs recipe"))?
                .2;
            let stack = edit
                .recipe
                .layers
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Layer continuation needs stack"))?;
            layers.encode(
                device,
                &mut encoder,
                &buffer,
                request.origin,
                request.size,
                source.size,
                native,
                stack,
                cancelled,
            )?
        } else {
            buffer
        };
        ensure!(!cancelled(), "Editing annullato");
        if let Some(t) = &timer {
            t.end(&mut encoder);
            t.resolve(&mut encoder);
        }
        if publish_checkpoint {
            checkpoint
                .unwrap()
                .put(upstream.as_ref().unwrap().clone(), memory, gpu);
        }
        Ok(Self {
            commands: encoder.finish(),
            source,
            output: Arc::new(Output {
                buffer,
                _credits: output_credits,
            }),
            scratch,
            optics: point.map(|(_, optics, _)| optics),
            upstream,
            timer,
        })
    }
    pub fn submit(self, queue: &wgpu::Queue, cancelled: &impl Fn() -> bool) -> Result<Arc<Output>> {
        ensure!(!cancelled(), "Editing annullato");
        let (commands, completion) = self.into_parts();
        let output = completion.output.clone();
        queue.submit([commands]);
        completion.submitted(queue);
        queue.on_submitted_work_done(move || {
            completion.completed();
            drop(completion);
        });
        Ok(output)
    }
    pub fn into_parts(self) -> (wgpu::CommandBuffer, Completion) {
        let Self {
            commands,
            source,
            output,
            scratch,
            optics,
            upstream,
            timer,
        } = self;
        (
            commands,
            Completion {
                source,
                output,
                _scratch: scratch,
                _optics: optics,
                upstream,
                upload: None,
                timer: std::sync::Mutex::new(timer),
                flight: None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_second_budget_and_live_output_return_credits() {
        let memory = MemoryBudget::new(100);
        let gpu = MemoryBudget::new(50);
        assert!(Credits::new(&memory, &gpu, 60).is_err());
        assert_eq!(memory.usage().reserved, 0);
        assert_eq!(gpu.usage().reserved, 0);
        let retained = Arc::new(Credits::new(&memory, &gpu, 40).unwrap());
        let pending = retained.clone();
        gpu.configure(0);
        drop(retained);
        assert_eq!(gpu.usage().reserved, 40);
        drop(pending);
        assert_eq!(memory.usage().reserved, 0);
        assert_eq!(gpu.usage().reserved, 0);
    }
    #[test]
    fn invalid_region_and_large_guides_reject_before_gpu_allocation() {
        let request = Request {
            n: 16,
            amounts: [0.25, 0.35],
            threshold: 0.25,
            levels: 3,
            origin: [1, 1],
            size: [855, 596],
        };
        assert!(request.validate([4097, 3073]).is_ok());
        assert!(
            Request {
                origin: [u32::MAX, 0],
                ..request
            }
            .validate([4097, 3073])
            .is_err()
        );
        assert!(
            Request {
                amounts: [f32::NAN, 0.],
                ..request
            }
            .validate([4097, 3073])
            .is_err()
        );
        assert!(
            Request {
                size: [0, 1],
                ..request
            }
            .validate([4097, 3073])
            .is_err()
        );
        let limits = wgpu::Limits::default();
        let normal =
            multiscale::working_bytes(&limits, [4097, 3073], 16, 0.35, 3, [1, 1], [855, 596])
                .unwrap();
        assert!(normal > 64 * 1024 * 1024 && normal < 256 * 1024 * 1024);
        assert!(
            multiscale::working_bytes(&limits, [8192, 8192], 16, 0.35, 3, [1, 1], [855, 596])
                .is_err()
        );
    }
}
