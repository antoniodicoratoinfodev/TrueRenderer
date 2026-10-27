//! Retained graph preparation. All uploads and compute commands are returned to
//! the UI; source/checkpoint readiness changes only after that UI submission.
use super::*;
use std::sync::{Arc, Condvar, Mutex, atomic::Ordering};
use std::time::Duration;
use tr_core::{budget::MemoryBudget, editing::detail::Denoise, provider::ImageLevels};

#[derive(Default)]
struct FlightState {
    count: usize,
    generation: u64,
}
type Flights = Arc<(Mutex<FlightState>, Condvar)>;
pub(super) struct Flight(Flights);
impl Flight {
    fn new(flights: &Flights) -> Self {
        flights.0.lock().unwrap().count += 1;
        Self(flights.clone())
    }
    pub(super) fn submitted(&self) {
        let mut state = self.0.0.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        self.0.1.notify_all();
    }
}
impl Drop for Flight {
    fn drop(&mut self) {
        let mut state = self.0.0.lock().unwrap();
        state.count -= 1;
        state.generation = state.generation.wrapping_add(1);
        self.0.1.notify_all();
    }
}

pub(crate) struct Chain {
    noise: multiscale::Multiscale,
    point: point::Point,
    spatial: spatial::Spatial,
    post: post::Post,
    layers: layers::Layers,
    source: Mutex<Option<(u64, Arc<admission::Source>)>>,
    optics: Mutex<Option<Arc<point::Optics>>>,
    checkpoint: checkpoint::Checkpoint,
    flights: Flights,
}
impl Chain {
    fn retry<T>(
        &self,
        device: &wgpu::Device,
        cancelled: &(impl Fn() -> bool + Sync),
        mut encode: impl FnMut() -> Result<T>,
    ) -> Result<T> {
        loop {
            ensure!(!cancelled(), "Editing annullato");
            let result = encode();
            let (in_flight, generation) = {
                let state = self.flights.0.lock().unwrap();
                (state.count, state.generation)
            };
            match result {
                Err(error) if error.is::<admission::Quota>() && in_flight > 0 => {
                    let _wait = crate::timings::CpuSpan::new("graph_quota_wait_cpu");
                    // Poll without waiting inside the backend: its locks also
                    // serve UI submissions. The short pause owns no GPU lock.
                    device.poll(wgpu::PollType::Poll)?;
                    ensure!(!cancelled(), "Editing annullato");
                    let state = self.flights.0.lock().unwrap();
                    if state.count > 0 && state.generation == generation {
                        #[cfg(windows)]
                        {
                            drop(state);
                            // The pinned Rust 1.98 toolchain uses a private
                            // high-resolution waitable timer for nonzero sleep.
                            // Windows condvar timeouts can overshoot by a tick.
                            // No GPU lock, busy wait or global timer change.
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        #[cfg(not(windows))]
                        drop(
                            self.flights
                                .1
                                .wait_timeout(state, Duration::from_millis(2))
                                .unwrap(),
                        );
                    }
                }
                result => return result,
            }
        }
    }
    pub(crate) fn reserve_display(
        &self,
        device: &wgpu::Device,
        gpu: &MemoryBudget,
        bytes: u64,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<tr_core::budget::Lease> {
        self.retry(device, cancelled, || {
            gpu.try_reserve(bytes)
                .ok_or_else(|| admission::Quota::new("GPU display", gpu, bytes).into())
        })
    }
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self> {
        super::guarded(device, || {
            Ok(Self {
                noise: multiscale::Multiscale::new(device)?,
                point: point::Point::new(device),
                spatial: spatial::Spatial::new(device),
                post: post::Post::new(device),
                layers: layers::Layers::new(device),
                source: Mutex::new(None),
                optics: Mutex::new(None),
                checkpoint: checkpoint::Checkpoint::default(),
                flights: Arc::new((Mutex::new(FlightState::default()), Condvar::new())),
            })
        })
    }
    pub(crate) fn clear(&self) {
        let source = self.source.lock().unwrap().take();
        let optics = self.optics.lock().unwrap().take();
        self.checkpoint.clear();
        drop((source, optics));
    }
    pub(crate) fn source_ready(&self, image: &ImageLevels) -> bool {
        self.source
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(id, source)| {
                *id == image.id() && source.submitted.load(Ordering::Acquire)
            })
    }
    fn source(
        &self,
        device: &wgpu::Device,
        image: &ImageLevels,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<(Arc<admission::Source>, Option<admission::Upload>)> {
        let cached = self
            .source
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(id, s)| *id == image.id() && s.submitted.load(Ordering::Acquire))
            .map(|(_, s)| s.clone());
        if let Some(source) = cached {
            return Ok((source, None));
        }
        self.clear();
        ensure!(!image.scientific(), "Invalid GPU graph source");
        for pixels in image.source().pixels.chunks(2048) {
            ensure!(!cancelled(), "Editing annullato");
            ensure!(
                pixels.iter().all(|p| p.iter().all(|v| v.is_finite())
                    && (0. ..=1.).contains(&p[3])
                    && p[..3]
                        .iter()
                        .all(|v| (if p[3] > 0. { *v / p[3] } else { *v }).abs() <= 10.)),
                "Graph source outside qualified linear domain"
            );
        }
        let upload =
            admission::Upload::prepare_point(device, image.source(), memory, gpu, cancelled)?;
        let source = upload.source();
        *self.source.lock().unwrap() = Some((image.id(), source.clone()));
        Ok((source, Some(upload)))
    }
    pub(crate) fn preload(
        &self,
        device: &wgpu::Device,
        image: &ImageLevels,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Option<admission::Upload>> {
        self.source(device, image, memory, gpu, cancelled)
            .map(|(_, upload)| upload)
    }
    pub(crate) fn optics(
        &self,
        device: &wgpu::Device,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Arc<point::Optics>> {
        let source_size = [image.source().width, image.source().height];
        let amount = edit
            .recipe
            .advanced
            .as_ref()
            .map_or(0., |a| a.geometry.vignette);
        let cached = self
            .optics
            .lock()
            .unwrap()
            .as_ref()
            .filter(|o| o.matches(source_size, amount))
            .cloned();
        if let Some(optics) = cached {
            return Ok(optics);
        }
        let old = self.optics.lock().unwrap().take();
        drop(old);
        let optics = Arc::new(point::Optics::prepare(
            device,
            source_size,
            amount,
            memory,
            gpu,
            cancelled,
        )?);
        *self.optics.lock().unwrap() = Some(optics.clone());
        Ok(optics)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &self,
        device: &wgpu::Device,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        origin: [u32; 2],
        size: [u32; 2],
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<(Vec<wgpu::CommandBuffer>, Arc<Completion>)> {
        ensure!(
            !cancelled() && !image.scientific(),
            "Invalid GPU graph source"
        );
        edit.recipe.validate()?;
        let advanced = edit
            .recipe
            .advanced
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing graph parameters"))?;
        let d = &advanced.detail;
        ensure!(
            d.denoise == Denoise::MultiscaleV2 || (d.luminance_noise == 0. && d.chroma_noise == 0.),
            "Historical denoiser stays on CPU"
        );
        let source_size = [image.source().width, image.source().height];
        let native = image.source_size();
        spatial::support(d, advanced.geometry.defringe, native, source_size)?;
        let scale = source_size[0].max(source_size[1]) as f32 / native[0].max(native[1]) as f32;
        let (n, levels) = if scale <= 0.3 {
            (4, 1)
        } else if scale <= 0.6 {
            (8, 2)
        } else {
            (16, 3)
        };
        let (source, upload) = self.source(device, image, memory, gpu, cancelled)?;
        let mut commands = Vec::new();
        let mut staging = None;
        if let Some(upload) = upload {
            let (_, work, credits) = upload.into_parts();
            commands.push(work);
            staging = Some(credits);
        }
        let optics = self.optics(device, image, edit, memory, gpu, cancelled)?;
        let layers = edit.recipe.layers.as_ref().is_some_and(|s| {
            s.layers
                .iter()
                .any(|l| l.enabled && !l.operators.is_empty())
        });
        let pending = self.retry(device, cancelled, || {
            admission::Pending::prepare_cached(
                device,
                &self.noise,
                source.clone(),
                admission::Request {
                    n,
                    levels,
                    amounts: [d.luminance_noise / 100., d.chroma_noise / 100.],
                    threshold: d.threshold / 100.,
                    origin,
                    size,
                },
                memory,
                gpu,
                Some((&self.point, optics.clone(), edit)),
                Some((&self.spatial, native)),
                Some((&self.post, native)),
                layers.then_some((&self.layers, native)),
                Some(&self.checkpoint),
                cancelled,
            )
        })?;
        let (work, mut completion) = pending.into_parts();
        completion.upload = staging;
        completion.flight = Some(Flight::new(&self.flights));
        commands.push(work);
        Ok((commands, Arc::new(completion)))
    }
}
