//! Bounded asynchronous CPU presentation; every uploaded texel is one backing pixel.
use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, TextureOptions};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
};
use tr_core::{
    budget::{Lease, MemoryBudget},
    provider::ImageLevels,
    resample::Region,
};

#[derive(Clone, Debug, PartialEq)]
struct Key {
    stretch: tr_core::science::Stretch,
    high_precision: bool,
    source: u64,
    region: Region,
    edit: Option<crate::live_edit::LiveEdit>,
    photographic: bool,
}
struct Job {
    warm: bool,
    cancel: Arc<AtomicBool>,
    lane: String,
    key: Key,
    placement: Placement,
    image: Arc<ImageLevels>,
    lease: Lease,
    gpu_lease: Lease,
    zooming: bool,
}
struct Preparation {
    key: Key,
    lane: String,
    cancel: Arc<AtomicBool>,
}
enum Rendered {
    Cpu(Vec<u8>),
    Cpu16(Vec<u8>),
    Gpu(crate::resident_compute::GpuFrame),
}
struct Completed {
    lane: String,
    key: Key,
    placement: Placement,
    result: Result<Rendered, String>,
    lease: Arc<Lease>,
    gpu_lease: Arc<Lease>,
}
struct PreparedGraph {
    job: Job,
    work: crate::resident_compute::GraphPreparation,
}
#[derive(Default)]
struct Shared {
    warming_edit: Option<Preparation>,
    prepared_edit: Option<Key>,
    jobs: VecDeque<Job>,
    completed: VecDeque<Completed>,
    prepared_graphs: VecDeque<PreparedGraph>,
    stop: bool,
    statistics: ComputeStatistics,
}
impl Shared {
    fn finish_graph_stage(&mut self, job: Job, complete: bool, optics: bool) -> bool {
        self.statistics.gpu_graph_preparations += u64::from(complete);
        self.statistics.gpu_source_preparations += u64::from(!complete && !optics);
        self.statistics.gpu_optics_preparations += u64::from(optics);
        if !complete
            && !self.stop
            && !job.cancel.load(Ordering::Acquire)
            && self
                .warming_edit
                .as_ref()
                .is_some_and(|p| Arc::ptr_eq(&p.cancel, &job.cancel))
        {
            // Keep the same preparation pending between upload and compute.
            self.jobs.push_back(job);
            true
        } else {
            self.finish_preparation(&job.key, &job.cancel, complete);
            false
        }
    }
    fn discard_jobs(&mut self, discard: impl Fn(&str, &Key, bool) -> bool) {
        // The worker may already have popped the optional job. Its cancellation
        // token remains here until completion, so visible work can preempt it.
        if self
            .warming_edit
            .as_ref()
            .is_some_and(|job| discard(&job.lane, &job.key, true))
        {
            self.warming_edit
                .take()
                .unwrap()
                .cancel
                .store(true, Ordering::Release);
        }
        self.jobs
            .retain(|job| !discard(&job.lane, &job.key, job.warm));
        self.prepared_graphs
            .retain(|prepared| !discard(&prepared.job.lane, &prepared.job.key, true));
    }
    fn finish_preparation(&mut self, key: &Key, cancel: &Arc<AtomicBool>, succeeded: bool) {
        // A replacement can have the same key. Never clear or certify that new
        // preparation when an older, cancelled generation completes late.
        if self
            .warming_edit
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(&p.cancel, cancel))
        {
            self.warming_edit = None;
            if succeeded && !cancel.load(Ordering::Acquire) {
                self.prepared_edit = Some(key.clone());
                self.statistics.prepared_edit_frames += 1;
            }
        }
    }
}
struct Entry {
    key: Key,
    placement: Placement,
    superseded: bool,
    texture: FrameTexture,
    touched: u64,
    zoomed_at: Option<f64>,
    lease: Arc<Lease>,
    gpu_lease: Arc<Lease>,
}
#[derive(Clone, Copy)]
struct Placement {
    rect: Rect,
    region: Region,
}
enum FrameTexture {
    Cpu(TextureHandle),
    HighCpu {
        id: egui::TextureId,
        texture: eframe::wgpu::Texture,
        state: eframe::egui_wgpu::RenderState,
    },
    Gpu {
        id: egui::TextureId,
        frame: crate::resident_compute::GpuFrame,
        state: eframe::egui_wgpu::RenderState,
    },
}
impl FrameTexture {
    fn bytes(&self) -> usize {
        let bpp = match self {
            Self::Cpu(_) => 4,
            Self::HighCpu { .. } => 8,
            Self::Gpu { frame, .. } => {
                if frame.texture.format() == eframe::wgpu::TextureFormat::Rgba16Float {
                    8
                } else {
                    4
                }
            }
        };
        self.size()[0] * self.size()[1] * bpp
    }
    fn id(&self) -> egui::TextureId {
        match self {
            Self::Cpu(t) => t.id(),
            Self::Gpu { id, .. } | Self::HighCpu { id, .. } => *id,
        }
    }
    fn size(&self) -> [usize; 2] {
        match self {
            Self::Cpu(t) => t.size(),
            Self::HighCpu { texture, .. } => [texture.width() as usize, texture.height() as usize],
            Self::Gpu { frame, .. } => frame.size.map(|v| v as usize),
        }
    }
}
impl Drop for FrameTexture {
    fn drop(&mut self) {
        if let Self::Gpu { id, state, .. } | Self::HighCpu { id, state, .. } = self {
            state.renderer.write().free_texture(id);
        }
    }
}
#[derive(Default, Clone, serde::Serialize)]
pub struct ComputeStatistics {
    pub gpu_graph_frames: u64,
    pub gpu_graph_preparations: u64,
    pub gpu_source_preparations: u64,
    pub gpu_optics_preparations: u64,
    pub gpu_budget_bytes: u64,
    pub gpu_budget_base_bytes: u64,
    pub gpu_budget_ceiling_bytes: u64,
    pub gpu_budget_growths: u64,
    pub gpu_reserved_bytes: u64,
    pub gpu_peak_bytes: u64,
    pub gpu_layer_frames: u64,
    pub resident_layer_frames: u64,
    pub hybrid_edit_frames: u64,
    pub prepared_edit_frames: u64,
    pub regional_edit: tr_core::editing::regional::Statistics,
    pub cpu_frames: u64,
    pub gpu_frames: u64,
    pub resident_zoom_frames: u64,
    pub resident_edit_frames: u64,
    pub resident_edit_encode_us: u64,
    pub resident_zoom_busy: u64,
    pub resident_zoom_misses: u64,
    pub resident_zoom_encode_us: u64,
    pub resident_zoom_max_us: u64,
    pub resident_zoom_pixels: u64,
    pub worker_render_us: u64,
    pub worker_render_max_us: u64,
    pub worker_pixels: u64,
    pub cpu_edit_frames: u64,
    pub gpu_edit_frames: u64,
    pub fallbacks: u64,
    pub last_fallback: String,
    pub wide_frames: usize,
    pub wide_gpu_bytes: u64,
    pub wide_reprojections: u64,
    pub wide_peak_frames: u64,
    pub wide_peak_memory_bytes: u64,
    pub wide_peak_gpu_bytes: u64,
}
pub struct Capture {
    pub compute: &'static str,
    pub source: u64,
    pub rect: Rect,
    pub clip: Rect,
    pub region: Region,
}
/// Draw-command coverage for diagnostic traces, not compositor visibility.
pub struct PaintCoverage {
    pub region: Region,
    pub displayed_region: Option<Region>,
    pub lane: String,
    pub source: u64,
    pub displayed_source: Option<u64>,
    pub displayed_revision: Option<u64>,
    pub fraction: f32,
    pub exact: bool,
    /// Previous complete scale held without resampling while a new one loads.
    pub held: bool,
}
pub struct Presenter {
    stretch: tr_core::science::Stretch,
    high_precision: bool,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    waiting: HashMap<String, Key>,
    entries: HashMap<String, Entry>,
    // Optional wider coverage, same revision lane and coordinate space only.
    wide: HashMap<String, Entry>,
    wide_reprojections: u64,
    wide_peak: [u64; 3],
    errors: HashMap<String, (Key, String)>,
    clock: u64,
    capturing: bool,
    captures: Vec<Capture>,
    coverage: Vec<PaintCoverage>,
    memory: MemoryBudget,
    gpu_memory: MemoryBudget,
    queue: Option<eframe::wgpu::Queue>,
    retired: Vec<(u64, Arc<Lease>, Arc<Lease>)>,
    render_state: Option<eframe::egui_wgpu::RenderState>,
    pressure: Arc<AtomicBool>,
    compute_verified: Arc<AtomicBool>,
    layer_compute_verified: Arc<AtomicBool>,
    graph_compute_verified: Arc<AtomicBool>,
    compute_mode: Arc<AtomicU8>,
    filter: Arc<Mutex<Option<Arc<crate::resident_compute::GpuFilter>>>>,
    clear_filter: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}
impl Presenter {
    pub fn new(
        ctx: egui::Context,
        memory: MemoryBudget,
        render_state: Option<eframe::egui_wgpu::RenderState>,
    ) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let worker_shared = shared.clone();
        let queue = render_state.as_ref().map(|r| r.queue.clone());
        let gpu_memory = MemoryBudget::new(256 * 1024 * 1024);
        let compute_verified = Arc::new(AtomicBool::new(false));
        let layer_compute_verified = Arc::new(AtomicBool::new(false));
        let graph_compute_verified = Arc::new(AtomicBool::new(false));
        let compute_mode = Arc::new(AtomicU8::new(0));
        let pressure = Arc::new(AtomicBool::new(false));
        let worker_pressure = pressure.clone();
        let verified = compute_verified.clone();
        let layers_verified = layer_compute_verified.clone();
        let graph_verified = graph_compute_verified.clone();
        let mode = compute_mode.clone();
        let worker_gpu = render_state.clone();
        let worker_memory = memory.clone();
        let worker_gpu_memory = gpu_memory.clone();
        let filter = Arc::new(Mutex::new(None));
        let worker_filter = filter.clone();
        let clear_filter = Arc::new(AtomicBool::new(false));
        let worker_clear_filter = clear_filter.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let high_precision = render_state.as_ref().is_some_and(|s| {
            matches!(
                s.target_format,
                eframe::wgpu::TextureFormat::Rgb10a2Unorm
                    | eframe::wgpu::TextureFormat::Rgba16Float
            )
        });
        let worker = thread::spawn(move || {
            let mut edit_cache = tr_core::editing::regional::Processor::new(worker_memory.clone());
            let initialized = worker_gpu
                .as_ref()
                .filter(|r| r.device.limits().max_storage_buffers_per_shader_stage >= 7)
                .map(|r| {
                    Arc::new(crate::resident_compute::GpuFilter::with_precision(
                        r.device.clone(),
                        high_precision,
                    ))
                });
            *worker_filter.lock().unwrap() = initialized;
            loop {
                let mut job = {
                    let (lock, ready) = &*worker_shared;
                    let mut state = lock.lock().unwrap();
                    while state.jobs.is_empty() && !state.stop {
                        let (next, _) = ready
                            .wait_timeout(state, std::time::Duration::from_secs(2))
                            .unwrap();
                        state = next;
                        let clearing = worker_clear_filter.swap(false, Ordering::AcqRel);
                        if worker_memory.usage().reclaim_bytes > 0
                            || worker_pressure.load(Ordering::Acquire)
                            || clearing
                        {
                            edit_cache.clear();
                            state.prepared_edit = None;
                        }
                        if (clearing || worker_pressure.load(Ordering::Acquire))
                            && let Some(filter) = worker_filter.lock().unwrap().clone()
                        {
                            filter.clear();
                        }
                    }
                    if state.stop {
                        break;
                    }
                    state.jobs.pop_front().unwrap()
                };
                let started = std::time::Instant::now();
                let pixels = job.key.region.size[0] as u64 * job.key.region.size[1] as u64;
                let choice = mode.load(Ordering::Acquire);
                let clearing = worker_clear_filter.swap(false, Ordering::AcqRel)
                    || worker_pressure.load(Ordering::Acquire)
                    || worker_memory.usage().reserved > worker_memory.usage().limit;
                if clearing {
                    edit_cache.clear();
                    worker_shared.0.lock().unwrap().prepared_edit = None;
                }
                if (choice == 1
                    || clearing
                    || worker_gpu_memory.usage().reserved > worker_gpu_memory.usage().limit
                    || worker_memory.usage().reserved > worker_memory.usage().limit)
                    && let Some(filter) = worker_filter.lock().unwrap().clone()
                {
                    filter.clear();
                }
                let mut fallback = None;
                let filter_wait = crate::timings::CpuSpan::new("filter_lock_wait_cpu");
                let filter = worker_filter.lock().unwrap().clone();
                drop(filter_wait);
                let mut gpu = None;
                let mut layers_prepared = false;
                let mut layer_frame = false;
                let mut graph_frame = false;
                let mut graph_preparation = None;
                if graph_verified.load(Ordering::Acquire)
                    && verified.load(Ordering::Acquire)
                    && choice != 1
                    && let Some(edit) = &job.key.edit
                    && edit.recipe.advanced.is_some()
                    // Point-only recipes already have a regional GPU path.
                    // Do not upload a full source or prepare noise statistics
                    // when the same edit needs neither spatial work nor layers.
                    && !edit.gpu_supported_for(&job.image, job.key.region)
                    && let Some(filter) = &filter
                {
                    let _span = crate::timings::CpuSpan::new("gpu_graph_prepare_cpu");
                    let result = if job.warm {
                        filter.prepare_chain(
                            &job.image,
                            edit,
                            job.key.region,
                            &worker_memory,
                            &worker_gpu_memory,
                            &mut job.lease,
                            &|| {
                                job.cancel.load(Ordering::Acquire)
                                    || worker_stopped.load(Ordering::Acquire)
                                    || worker_pressure.load(Ordering::Acquire)
                                    || worker_clear_filter.load(Ordering::Acquire)
                            },
                        )
                    } else {
                        filter
                            .render_chain(
                                &job.image,
                                edit,
                                job.key.region,
                                &worker_memory,
                                &worker_gpu_memory,
                                &mut job.lease,
                                &|| {
                                    job.cancel.load(Ordering::Acquire)
                                        || worker_stopped.load(Ordering::Acquire)
                                },
                            )
                            .map(crate::resident_compute::GraphPreparation::Frame)
                    };
                    match result {
                        Ok(crate::resident_compute::GraphPreparation::Frame(frame))
                            if !job.warm =>
                        {
                            gpu = Some(frame);
                            graph_frame = true;
                        }
                        Ok(preparation) => graph_preparation = Some(preparation),
                        Err(e) => fallback = Some(format!("{e:#}")),
                    }
                }
                if worker_stopped.load(Ordering::Acquire) {
                    break;
                }
                if gpu.is_none()
                    && graph_preparation.is_none()
                    && layers_verified.load(Ordering::Acquire)
                    && verified.load(Ordering::Acquire)
                    && !job.image.scientific()
                    && choice != 1
                    && let Some(edit) = &job.key.edit
                    && edit.recipe.layers.as_ref().is_some_and(|s| {
                        s.layers
                            .iter()
                            .any(|l| l.enabled && !l.operators.is_empty())
                    })
                    && let Some(filter) = &filter
                {
                    let _span = crate::timings::CpuSpan::new("layer_tail_prepare_cpu");
                    match filter.render_layers(
                        &job.image,
                        edit,
                        job.key.region,
                        &mut edit_cache,
                        &worker_memory,
                        &worker_gpu_memory,
                        &mut job.lease,
                        job.warm,
                        &|| {
                            job.cancel.load(Ordering::Acquire)
                                || (job.warm
                                    && (worker_pressure.load(Ordering::Acquire)
                                        || worker_clear_filter.load(Ordering::Acquire)))
                        },
                    ) {
                        Ok(frame) => {
                            layer_frame = frame.is_some();
                            gpu = frame;
                            layers_prepared = job.warm;
                        }
                        Err(e) => fallback = Some(format!("{e:#}")),
                    }
                }
                let gpu = if gpu.is_none()
                    && !job.warm
                    && verified.load(Ordering::Acquire)
                    && !job.image.scientific()
                    && choice != 1
                    && job
                        .key
                        .edit
                        .as_ref()
                        .is_none_or(|e| e.gpu_supported_for(&job.image, job.key.region))
                    && (choice == 2
                        || job.zooming
                        || job.key.edit.is_some()
                        || pixels >= 512 * 1024)
                    && let Some(filter) = &filter
                {
                    let result = if job.zooming && job.key.edit.is_none() {
                        filter.render_zoom(
                            &job.image,
                            job.key.region,
                            &worker_memory,
                            &worker_gpu_memory,
                            &mut job.lease,
                        )
                    } else {
                        filter.render_edit(
                            &job.image,
                            job.key.region,
                            &worker_memory,
                            &worker_gpu_memory,
                            &mut job.lease,
                            job.key.edit.as_ref(),
                        )
                    };
                    result
                        .map_err(|e| {
                            fallback = Some(format!("{e:#}"));
                            e
                        })
                        .ok()
                } else {
                    gpu
                };
                let is_gpu = gpu.is_some();
                let mut hybrid = false;
                let result = if let Some(frame) = gpu {
                    Ok(Rendered::Gpu(frame))
                } else if layers_prepared || graph_preparation.is_some() {
                    Ok(Rendered::Cpu(Vec::new()))
                } else {
                    let _cpu = crate::timings::CpuSpan::new("cpu_filter_and_display");
                    job.key
                        .edit
                        .as_ref()
                        .map_or_else(
                            || job.image.render(job.key.region),
                            |edit| {
                                edit.render_cached_cancellable(
                                    &job.image,
                                    job.key.region,
                                    &mut edit_cache,
                                    &|| {
                                        job.cancel.load(Ordering::Acquire)
                                            || (job.warm
                                                && (worker_pressure.load(Ordering::Acquire)
                                                    || worker_clear_filter.load(Ordering::Acquire)))
                                    },
                                )
                            },
                        )
                        .map(|mut image| {
                            if job.warm {
                                return Rendered::Cpu(Vec::new());
                            }
                            if job.key.edit.is_some()
                                && choice != 1
                                && verified.load(Ordering::Acquire)
                                && let Some(filter) = &filter
                                && let Ok(frame) = filter.display_linear(
                                    &image,
                                    &worker_memory,
                                    &worker_gpu_memory,
                                    &mut job.lease,
                                )
                            {
                                hybrid = true;
                                return Rendered::Gpu(frame);
                            }
                            if job.image.scientific() {
                                job.key.stretch.apply(&mut image);
                            }
                            if high_precision {
                                let mut bytes = vec![0; image.pixels.len() * 8];
                                image.map_display(&mut bytes, |p| {
                                    let values = tr_core::color::display_float(p, 119. / 255.)
                                        .map(|v| half::f16::from_f32(v).to_bits().to_le_bytes());
                                    std::array::from_fn::<_, 8, _>(|i| values[i / 2][i % 2])
                                });
                                Rendered::Cpu16(bytes)
                            } else {
                                Rendered::Cpu(image.to_display())
                            }
                        })
                        .map_err(|e| format!("{e:#}"))
                };
                // An upload continues as the same optional job after the UI
                // callback. Keep its working reservation for the compute phase.
                if !matches!(
                    graph_preparation,
                    Some(
                        crate::resident_compute::GraphPreparation::Upload(_)
                            | crate::resident_compute::GraphPreparation::Optics(_)
                    )
                ) {
                    job.lease.shrink(if result.is_ok() {
                        pixels * if high_precision { 24 } else { 12 }
                    } else {
                        0
                    });
                }
                let lease = Arc::new(job.lease);
                let gpu_lease = Arc::new(job.gpu_lease);
                let mut state = worker_shared.0.lock().unwrap();
                let elapsed = started.elapsed().as_micros() as u64;
                state.statistics.worker_render_us += elapsed;
                state.statistics.worker_render_max_us =
                    state.statistics.worker_render_max_us.max(elapsed);
                state.statistics.worker_pixels += pixels;
                state.statistics.regional_edit = edit_cache.statistics.clone();
                if state.stop {
                    break;
                }
                if job.warm {
                    if let Some(work) = graph_preparation {
                        state.prepared_graphs.clear();
                        state.prepared_graphs.push_back(PreparedGraph {
                            job: Job {
                                key: job.key,
                                lane: job.lane,
                                cancel: job.cancel,
                                placement: job.placement,
                                image: job.image,
                                warm: true,
                                zooming: false,
                                lease: Arc::try_unwrap(lease).ok().unwrap(),
                                gpu_lease: Arc::try_unwrap(gpu_lease).ok().unwrap(),
                            },
                            work,
                        });
                    } else {
                        state.finish_preparation(&job.key, &job.cancel, result.is_ok());
                    }
                    drop(state);
                    ctx.request_repaint();
                    continue;
                }
                if job.key.edit.is_some() {
                    state.statistics.hybrid_edit_frames += u64::from(hybrid);
                    state.statistics.gpu_layer_frames += u64::from(layer_frame);
                    state.statistics.gpu_graph_frames += u64::from(graph_frame);
                    state.prepared_edit = None;
                }
                if !is_gpu {
                    state.statistics.cpu_frames += 1;
                }
                if job.key.edit.is_some() {
                    if is_gpu {
                        state.statistics.gpu_edit_frames += 1;
                    } else {
                        state.statistics.cpu_edit_frames += 1;
                    }
                }
                if let Some(error) = fallback {
                    state.statistics.fallbacks += 1;
                    state.statistics.last_fallback = error;
                }
                state.completed.retain(|old| old.lane != job.lane);
                // At most 64 waiting lanes plus one active operation; never block shutdown.
                if state.completed.len() >= 65 {
                    state.completed.pop_front();
                }
                state.completed.push_back(Completed {
                    lane: job.lane,
                    key: job.key,
                    placement: job.placement,
                    result,
                    lease,
                    gpu_lease,
                });
                drop(state);
                ctx.request_repaint();
            }
        });
        Self {
            stretch: Default::default(),
            high_precision,
            shared,
            worker: Some(worker),
            waiting: HashMap::new(),
            entries: HashMap::new(),
            wide: HashMap::new(),
            wide_reprojections: 0,
            wide_peak: [0; 3],
            errors: HashMap::new(),
            clock: 0,
            capturing: false,
            captures: Vec::new(),
            coverage: Vec::new(),
            memory,
            gpu_memory,
            queue,
            retired: Vec::new(),
            render_state,
            pressure,
            compute_verified,
            layer_compute_verified,
            graph_compute_verified,
            compute_mode,
            filter,
            clear_filter,
            stopped,
        }
    }
    pub fn statistics(&self) -> ComputeStatistics {
        let mut stats = self.shared.0.lock().unwrap().statistics.clone();
        let gpu = self.gpu_memory.usage();
        stats.gpu_budget_bytes = gpu.limit;
        stats.gpu_budget_base_bytes = gpu.base;
        stats.gpu_budget_ceiling_bytes = gpu.ceiling;
        stats.gpu_budget_growths = gpu.growths;
        stats.gpu_reserved_bytes = gpu.reserved;
        stats.gpu_peak_bytes = gpu.peak;
        stats.wide_frames = self.wide.len();
        stats.wide_gpu_bytes = self.wide.values().map(|e| e.gpu_lease.bytes()).sum();
        stats.wide_reprojections = self.wide_reprojections;
        [
            stats.wide_peak_frames,
            stats.wide_peak_memory_bytes,
            stats.wide_peak_gpu_bytes,
        ] = self.wide_peak;
        stats
    }
    pub fn interactive_edit_edge(&self, edit: &crate::live_edit::LiveEdit) -> u32 {
        if self.compute_verified.load(Ordering::Acquire)
            && self.compute_mode.load(Ordering::Acquire) != 1
            && edit.gpu_supported()
        {
            2048
        } else {
            1024
        }
    }

    /// Prime the exact graph only after the settled visible frame is ready.
    /// This is optional work behind visible jobs; it never replaces a texture.
    pub fn prepare_live_edit(
        &self,
        lane: &str,
        image: &Arc<ImageLevels>,
        edit: crate::live_edit::LiveEdit,
    ) {
        if edit.gpu_supported() || self.pressure.load(Ordering::Acquire) {
            return;
        }
        let Some(entry) = self
            .entries
            .get(lane)
            .filter(|e| !e.superseded && e.key.edit.is_none())
        else {
            return;
        };
        let key = Key {
            source: image.id(),
            edit: Some(edit),
            region: entry.placement.region,
            ..entry.key.clone()
        };
        let mut shared = self.shared.0.lock().unwrap();
        if shared.prepared_edit.as_ref() == Some(&key)
            || shared.warming_edit.is_some()
            || !shared.jobs.is_empty()
        {
            return;
        }
        let pixels = u64::from(key.region.size[0]) * u64::from(key.region.size[1]);
        let bytes = pixels * 80
            + 4 * 1024 * 1024
            + key.edit.as_ref().unwrap().working_bytes(image, key.region);
        let usage = self.memory.usage();
        if bytes > usage.limit.saturating_sub(usage.reserved) {
            return;
        }
        let Some(lease) = self.memory.try_reserve(bytes) else {
            return;
        };
        let gpu_bytes = if self.graph_compute_verified.load(Ordering::Acquire) {
            pixels * if self.high_precision { 8 } else { 4 }
        } else {
            0
        };
        let Some(gpu_lease) = self.gpu_memory.try_reserve(gpu_bytes) else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        shared.warming_edit = Some(Preparation {
            key: key.clone(),
            lane: lane.into(),
            cancel: cancel.clone(),
        });
        shared.jobs.push_back(Job {
            warm: true,
            cancel,
            lane: lane.into(),
            key,
            placement: entry.placement,
            image: image.clone(),
            lease,
            gpu_lease,
            zooming: false,
        });
        self.shared.1.notify_one();
    }
    pub fn set_scientific_stretch(&mut self, stretch: tr_core::science::Stretch) {
        if self.stretch != stretch {
            self.stretch = stretch;
            self.clear();
        }
    }
    fn retire(&mut self, entry: Entry) {
        self.retired
            .push((self.clock, entry.lease, entry.gpu_lease));
    }
    fn discard_wide(&mut self) {
        let old: Vec<_> = self.wide.drain().map(|(_, e)| e).collect();
        for entry in old {
            self.retire(entry);
        }
    }
    /// Optional coverage must not pin credits needed by decode/source work.
    pub fn release_optional_frames(&mut self) {
        self.discard_wide();
        self.clear_filter.store(true, Ordering::Release);
        self.shared.1.notify_all();
    }
    fn trim_wide(&mut self) {
        // Existing leases travel with the texture: retention reserves no new
        // credits and never removes the original accounting. Optional frames
        // use at most two slots and a quarter of either applicable budget.
        while !self.wide.is_empty()
            && (self.pressure.load(Ordering::Acquire)
                || self.wide.len() > 2
                || self.wide.len() + self.entries.len() > 64
                || self.wide.values().map(|e| e.lease.bytes()).sum::<u64>()
                    > self.memory.usage().limit / 4
                || self.wide.values().map(|e| e.gpu_lease.bytes()).sum::<u64>()
                    > self.gpu_memory.usage().limit / 4
                || self.memory.usage().reserved > self.memory.usage().limit
                || self.gpu_memory.usage().reserved > self.gpu_memory.usage().limit)
        {
            let lane = self
                .wide
                .iter()
                .min_by_key(|(_, e)| e.touched)
                .unwrap()
                .0
                .clone();
            let old = self.wide.remove(&lane).unwrap();
            self.retire(old);
        }
        let usage = [
            self.wide.len() as u64,
            self.wide.values().map(|e| e.lease.bytes()).sum(),
            self.wide.values().map(|e| e.gpu_lease.bytes()).sum(),
        ];
        for (peak, value) in self.wide_peak.iter_mut().zip(usage) {
            *peak = (*peak).max(value);
        }
    }
    fn install(&mut self, lane: String, mut entry: Entry) {
        if self.wide.get(&lane).is_some_and(|old| {
            source_area(old.key.region) <= source_area(entry.key.region)
                || ((old.key.photographic || entry.key.photographic)
                    && (old.key.source != entry.key.source || old.key.edit != entry.key.edit))
        }) {
            let old = self.wide.remove(&lane).unwrap();
            self.retire(old);
        }
        if let Some(old) = self.entries.remove(&lane) {
            if old.key.source == entry.key.source && entry.key.region == entry.placement.region {
                entry.zoomed_at = old.zoomed_at;
            }
            let retain = lane.starts_with("view:")
                && !old.superseded
                && (old.key.source == entry.key.source
                    || (!old.key.photographic && !entry.key.photographic))
                && old.key.edit == entry.key.edit
                && !self.pressure.load(Ordering::Acquire)
                && source_area(old.key.region) > source_area(entry.key.region)
                && self
                    .wide
                    .get(&lane)
                    .is_none_or(|wide| source_area(old.key.region) > source_area(wide.key.region));
            if retain {
                if let Some(replaced) = self.wide.insert(lane.clone(), old) {
                    self.retire(replaced);
                }
            } else {
                self.retire(old);
            }
        }
        self.entries.insert(lane, entry);
        self.trim_wide();
    }
    pub fn begin_capture(&mut self) {
        self.capturing = true;
        self.captures.clear();
        self.coverage.clear();
    }
    pub fn captures(&self) -> &[Capture] {
        &self.captures
    }
    pub fn coverage(&self) -> &[PaintCoverage] {
        &self.coverage
    }
    pub fn clear(&mut self) {
        self.clear_filter.store(true, Ordering::Release);
        self.shared.1.notify_all();
        self.discard_wide();
        self.waiting.clear();
        for (_, entry) in self.entries.drain() {
            self.retired
                .push((self.clock, entry.lease, entry.gpu_lease));
        }
        self.errors.clear();
        let mut shared = self.shared.0.lock().unwrap();
        shared.discard_jobs(|_, _, _| true);
        shared.prepared_edit = None;
        shared.completed.clear();
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.clock += 1;
        let mut retired = Vec::new();
        // Credits for handles retired in the previous egui frame are released
        // after that frame's GPU submission completes, never on cancellation.
        std::mem::swap(&mut retired, &mut self.retired);
        if let Some(queue) = &self.queue {
            queue.on_submitted_work_done(move || drop(retired));
        }
        let prepared: Vec<_> = self
            .shared
            .0
            .lock()
            .unwrap()
            .prepared_graphs
            .drain(..)
            .collect();
        for PreparedGraph { job, work } in prepared {
            let valid = !self.pressure.load(Ordering::Acquire)
                && !job.cancel.load(Ordering::Acquire)
                && self
                    .shared
                    .0
                    .lock()
                    .unwrap()
                    .warming_edit
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(&p.cancel, &job.cancel));
            if !valid {
                self.shared
                    .0
                    .lock()
                    .unwrap()
                    .finish_preparation(&job.key, &job.cancel, false);
                continue;
            }
            if let Some(queue) = &self.queue {
                let shared = self.shared.clone();
                let ctx = ctx.clone();
                let optics = matches!(work, crate::resident_compute::GraphPreparation::Optics(_));
                work.submit(queue, move |complete| {
                    let mut state = shared.0.lock().unwrap();
                    if state.finish_graph_stage(job, complete, optics) {
                        shared.1.notify_one();
                    }
                    drop(state);
                    ctx.request_repaint();
                });
            } else {
                self.shared
                    .0
                    .lock()
                    .unwrap()
                    .finish_preparation(&job.key, &job.cancel, false);
            }
        }
        let completed: Vec<_> = self.shared.0.lock().unwrap().completed.drain(..).collect();
        for finished in completed {
            if self.waiting.get(&finished.lane) != Some(&finished.key) {
                continue;
            }
            self.waiting.remove(&finished.lane);
            match finished.result {
                Ok(rendered) => {
                    self.errors.remove(&finished.lane);
                    let [w, h] = finished.key.region.size;
                    let texture = match rendered {
                        Rendered::Cpu16(bytes) => {
                            use eframe::wgpu;
                            let state = self.render_state.as_ref().unwrap().clone();
                            let texture = state.device.create_texture(&wgpu::TextureDescriptor {
                                label: Some("TR CPU SDR fp16 presentation"),
                                size: wgpu::Extent3d {
                                    width: w,
                                    height: h,
                                    depth_or_array_layers: 1,
                                },
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba16Float,
                                usage: wgpu::TextureUsages::TEXTURE_BINDING
                                    | wgpu::TextureUsages::COPY_DST,
                                view_formats: &[],
                            });
                            state.queue.write_texture(
                                texture.as_image_copy(),
                                &bytes,
                                wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(w * 8),
                                    rows_per_image: None,
                                },
                                texture.size(),
                            );
                            let memory = finished.lease.clone();
                            let gpu_memory = finished.gpu_lease.clone();
                            state.queue.on_submitted_work_done(move || {
                                drop(memory);
                                drop(gpu_memory);
                            });
                            let id = state.renderer.write().register_native_texture(
                                &state.device,
                                &texture.create_view(&Default::default()),
                                wgpu::FilterMode::Nearest,
                            );
                            FrameTexture::HighCpu { id, texture, state }
                        }
                        Rendered::Cpu(rgba) => FrameTexture::Cpu(ctx.load_texture(
                            &finished.lane,
                            egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &rgba,
                            ),
                            TextureOptions::NEAREST,
                        )),
                        Rendered::Gpu(mut frame) => {
                            let state = self.render_state.as_ref().unwrap().clone();
                            frame.submit(&state.queue);
                            let memory = finished.lease.clone();
                            let gpu_memory = finished.gpu_lease.clone();
                            state.queue.on_submitted_work_done(move || {
                                drop(memory);
                                drop(gpu_memory);
                            });
                            self.shared.0.lock().unwrap().statistics.gpu_frames += 1;
                            let id = state.renderer.write().register_native_texture(
                                &state.device,
                                &frame
                                    .texture
                                    .create_view(&eframe::wgpu::TextureViewDescriptor::default()),
                                eframe::wgpu::FilterMode::Nearest,
                            );
                            FrameTexture::Gpu { id, frame, state }
                        }
                    };
                    self.install(
                        finished.lane,
                        Entry {
                            key: finished.key,
                            placement: finished.placement,
                            superseded: false,
                            texture,
                            touched: self.clock,
                            zoomed_at: None,
                            lease: finished.lease,
                            gpu_lease: finished.gpu_lease,
                        },
                    );
                }
                Err(error) => {
                    if self.errors.len() >= 64 {
                        self.errors.clear();
                    }
                    self.errors.insert(finished.lane, (finished.key, error));
                }
            }
        }
        self.trim_wide();
        while self.entries.len() + self.wide.len() > 64
            || self
                .entries
                .values()
                .chain(self.wide.values())
                .map(|e| e.texture.bytes())
                .sum::<usize>()
                > self.gpu_memory.usage().limit as usize
        {
            if !self.wide.is_empty() {
                self.discard_wide();
                continue;
            }
            let oldest = self
                .entries
                .iter()
                .filter(|(_, e)| e.touched + 1 < self.clock)
                .min_by_key(|(_, e)| e.touched)
                .map(|(k, _)| k.clone());
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(old) = self.entries.remove(&oldest) {
                self.retired.push((self.clock, old.lease, old.gpu_lease));
            }
        }
    }
    pub fn invalidate_sources(&mut self, sources: &std::collections::HashSet<u64>) {
        self.supersede_sources(sources);
        let lanes: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| sources.contains(&entry.key.source))
            .map(|(lane, _)| lane.clone())
            .collect();
        for lane in lanes {
            if let Some(entry) = self.entries.remove(&lane) {
                self.retire(entry);
            }
        }
    }
    /// Revoke obsolete work but retain the last displayed texture until its
    /// replacement is ready. It owns only display-frame credits, not CPU mips.
    /// Callers must keep lanes isolated by asset, source epoch and display mode.
    pub fn supersede_sources(&mut self, sources: &std::collections::HashSet<u64>) {
        let stale: Vec<_> = self
            .wide
            .iter()
            .filter(|(_, entry)| sources.contains(&entry.key.source))
            .map(|(lane, _)| lane.clone())
            .collect();
        for lane in stale {
            let old = self.wide.remove(&lane).unwrap();
            self.retire(old);
        }
        self.waiting.retain(|_, key| !sources.contains(&key.source));
        self.errors
            .retain(|_, (key, _)| !sources.contains(&key.source));
        for entry in self.entries.values_mut() {
            if sources.contains(&entry.key.source) {
                entry.superseded = true;
            }
        }
        let mut shared = self.shared.0.lock().unwrap();
        shared.discard_jobs(|_, key, _| sources.contains(&key.source));
        if shared
            .prepared_edit
            .as_ref()
            .is_some_and(|k| sources.contains(&k.source))
        {
            shared.prepared_edit = None;
        }
        shared
            .completed
            .retain(|job| !sources.contains(&job.key.source));
        // An active encoder may finish; the missing waiting key discards it
        // before submission. In-flight GPU work keeps its completion leases.
    }
    pub fn set_pressure(&mut self, active: bool) {
        let previous = self.gpu_memory.usage().limit;
        self.gpu_memory.set_pressure(active);
        if self.gpu_memory.usage().limit != previous {
            self.gpu_budget_changed(previous);
        }
        if self.pressure.swap(active, Ordering::AcqRel) == active {
            return;
        }
        if active {
            self.discard_wide();
            let stale: Vec<_> = self
                .entries
                .iter()
                .filter(|(_, e)| e.touched + 1 < self.clock)
                .map(|(key, _)| key.clone())
                .collect();
            for key in stale {
                if let Some(entry) = self.entries.remove(&key) {
                    self.retired
                        .push((self.clock, entry.lease, entry.gpu_lease));
                }
            }
        }
        self.shared.1.notify_all();
    }
    pub fn is_idle(&self) -> bool {
        self.waiting.is_empty() && !self.live_preparation_pending()
    }
    pub fn live_preparation_pending(&self) -> bool {
        self.shared.0.lock().unwrap().warming_edit.is_some()
    }
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    pub fn configure_compute(&mut self, verified: bool, mode: u8) {
        let changed = self.compute_verified.swap(verified, Ordering::AcqRel) != verified;
        if self.compute_mode.swap(mode, Ordering::AcqRel) != mode || changed {
            self.clear();
        }
    }
    pub fn configure_layer_compute(&mut self, verified: bool) {
        if self.layer_compute_verified.swap(verified, Ordering::AcqRel) != verified {
            self.clear();
        }
    }
    pub fn configure_graph_compute(&mut self, verified: bool) {
        // Default to the independently checked graph. Keep a diagnostic off
        // switch for comparisons; it cannot bypass a failed device gate.
        let enabled = verified && !std::env::var("TR_DENOISE_GPU_EDIT").is_ok_and(|v| v == "0");
        if self.graph_compute_verified.swap(enabled, Ordering::AcqRel) != enabled {
            self.clear();
        }
    }
    pub fn configure_gpu_limit(&mut self, bytes: u64) {
        let previous = self.gpu_memory.usage().limit;
        self.gpu_memory.configure(bytes);
        self.gpu_budget_changed(previous);
    }
    pub fn configure_gpu_automatic(&mut self) {
        let mib = 1024 * 1024;
        let global = self.memory.usage().limit;
        let ceiling = (global / 4).min(2048 * mib);
        let base = ((global / 8 / mib).clamp(256, 2048) * mib).min(ceiling);
        let previous = self.gpu_memory.usage().limit;
        // Visible work can grow on demand. Optional caches and upload staging
        // still use existing headroom, and every allocation also owns global
        // credits. Neither configuration nor growth preallocates GPU memory.
        self.gpu_memory.configure_automatic(base, ceiling);
        self.gpu_memory
            .set_pressure(self.pressure.load(Ordering::Acquire));
        self.gpu_budget_changed(previous);
    }
    fn gpu_budget_changed(&mut self, previous: u64) {
        let limit = self.gpu_memory.usage().limit;
        if previous != limit {
            self.errors.clear();
            if limit < previous {
                self.clear_filter.store(true, Ordering::Release);
                self.shared.1.notify_all();
            }
        }
        self.trim_wide();
    }
    /// A lane identifies the asset/source epoch, RAW recipe, display mode and
    /// coordinate space. `supersede_sources` allows a complete previous edit to
    /// remain visible while its replacement is computed, without qualifying it
    /// as exact or retaining it as optional wide coverage afterwards.
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        rect: Rect,
        region: Region,
    ) {
        self.paint_frame(ui, lane, image, rect, region, None, None, false);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn paint_progressive(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        rect: Rect,
        region: Region,
        provisional: bool,
        photographic: bool,
    ) {
        self.paint_frame(
            ui,
            lane,
            image,
            rect,
            region,
            Some(provisional),
            None,
            photographic,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub fn paint_edit(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        rect: Rect,
        region: Region,
        edit: crate::live_edit::LiveEdit,
    ) {
        self.paint_frame(ui, lane, image, rect, region, Some(true), Some(edit), true);
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_frame(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        rect: Rect,
        region: Region,
        progressive: Option<bool>,
        edit: Option<crate::live_edit::LiveEdit>,
        photographic: bool,
    ) {
        let mut key = Key {
            stretch: if image.scientific() {
                self.stretch
            } else {
                Default::default()
            },
            high_precision: self.high_precision,
            source: image.id(),
            region,
            edit,
            photographic,
        };
        let placement = Placement { rect, region };
        let changing_scale = self
            .entries
            .get(&lane)
            .is_some_and(|e| e.placement.region.step != region.step);
        let now = ui.input(|i| i.time);
        if lane.starts_with("view:")
            && changing_scale
            && let Some(entry) = self.entries.get_mut(&lane)
        {
            entry.zoomed_at = Some(now);
        }
        // A pan margin is useful at a stable scale. During zoom it multiplies
        // the work and competes with the next visible frame for the GPU input.
        let zooming = self
            .entries
            .get(&lane)
            .is_some_and(|e| e.zoomed_at.is_some_and(|t| now - t < 0.12));
        if zooming {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
        }
        let needs_frame = self.entries.get(&lane).is_none_or(|e| {
            e.superseded || !same_content(&e.key, &key) || exact_uv(e.key.region, region).is_none()
        });
        if lane.starts_with("view:")
            && ((key.edit.is_none() && (changing_scale || zooming && needs_frame))
                || (needs_frame
                    && key.edit.as_ref().is_some_and(|e| {
                        e.gpu_supported()
                            || (self.layer_compute_verified.load(Ordering::Acquire)
                                && e.recipe.layers.as_ref().is_some_and(|s| {
                                    s.layers
                                        .iter()
                                        .any(|l| l.enabled && !l.operators.is_empty())
                                }))
                    })))
        {
            self.render_resident_frame(ui, &lane, image, &key, placement);
        }
        let pan = lane.starts_with("view:")
            && key.edit.is_none()
            && progressive != Some(true)
            && !self.pressure.load(Ordering::Acquire);
        let mut painted = false;
        if let Some(entry) = self
            .entries
            .get_mut(&lane)
            .filter(|e| same_content(&e.key, &key) && !e.superseded)
            .filter(|e| exact_uv(e.key.region, region).is_some())
        {
            let uv = exact_uv(entry.key.region, region).unwrap();
            entry.touched = self.clock;
            entry.placement = placement;
            if self.capturing {
                self.coverage.push(PaintCoverage {
                    region,
                    displayed_region: Some(entry.key.region),
                    lane: lane.clone(),
                    source: image.id(),
                    displayed_source: Some(image.id()),
                    displayed_revision: entry.key.edit.as_ref().map(|e| e.revision),
                    fraction: 1.,
                    exact: progressive != Some(true),
                    held: false,
                });
                self.captures.push(Capture {
                    compute: if matches!(&entry.texture, FrameTexture::Gpu { .. }) {
                        "GPU"
                    } else {
                        "CPU"
                    },
                    source: image.id(),
                    rect,
                    clip: ui.clip_rect(),
                    region,
                });
            }
            ui.painter()
                .image(entry.texture.id(), rect, uv, Color32::WHITE);
            painted = true;
            // Refresh before the cursor reaches the edge of the resident
            // frame. A pure crop on the same sample grid stays pixel-exact.
            if !pan
                || zooming
                || exact_uv(
                    entry.key.region,
                    pan_region(region, image.source_size(), 0.25),
                )
                .is_some()
            {
                return;
            }
        }
        if !painted {
            self.paint_previous(ui, &lane, image.id(), rect, region);
        }
        if pan && !zooming {
            // Extra coverage is optional and must fit existing admission and
            // device limits. Never reduce sampling resolution to obtain it.
            let memory = self.memory.usage();
            let gpu = self.gpu_memory.usage();
            let mut margin = 0.5;
            while margin >= 0.0625 {
                let guarded = pan_region(region, image.source_size(), margin);
                let pixels = u64::from(guarded.size[0]) * u64::from(guarded.size[1]);
                if pixels <= tr_core::color::MAX_PRESENTATION_PIXELS as u64
                    && guarded.size.iter().all(|n| *n <= 16384)
                    && pixels * 80 + 4 * 1024 * 1024 <= memory.limit.saturating_sub(memory.reserved)
                    && pixels * if key.high_precision { 8 } else { 4 }
                        <= gpu.limit.saturating_sub(gpu.reserved)
                {
                    key.region = guarded;
                    break;
                }
                margin *= 0.5;
            }
            if painted
                && self
                    .entries
                    .get(&lane)
                    .is_some_and(|e| exact_uv(e.key.region, key.region).is_some())
            {
                return;
            }
        }
        if let Some((_, error)) = self.errors.get(&lane).filter(|(failed, _)| *failed == key) {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(12.),
                Color32::LIGHT_RED,
            );
            return;
        }
        self.errors.remove(&lane);
        // One admitted render per lane. Keep it publishable while the next
        // draft changes; the following paint observes only the latest request.
        if (progressive.is_some() || pan) && self.waiting.contains_key(&lane) {
            return;
        }
        self.enqueue(ui, lane, image, placement, key, zooming);
    }

    /// Encode only from resident, validated linear input. Never wait for the
    /// worker lock, scan a source, upload pixels or read GPU results on the UI.
    fn render_resident_frame(
        &mut self,
        ui: &egui::Ui,
        lane: &str,
        image: &Arc<ImageLevels>,
        key: &Key,
        placement: Placement,
    ) -> Option<()> {
        if image.scientific()
            || self.pressure.load(Ordering::Acquire)
            || self.clear_filter.load(Ordering::Acquire)
            || !self.compute_verified.load(Ordering::Acquire)
            || self.compute_mode.load(Ordering::Acquire) == 1
        {
            return None;
        }
        let pixels = u64::from(key.region.size[0]) * u64::from(key.region.size[1]);
        if pixels > tr_core::color::MAX_PRESENTATION_PIXELS as u64 {
            return None;
        }
        let memory = self.memory.usage();
        let gpu = self.gpu_memory.usage();
        // Optional immediate work must not trigger global cache reclamation
        // merely because the larger candidate margin does not fit.
        let working_bytes = pixels * 80
            + 4 * 1024 * 1024
            + key
                .edit
                .as_ref()
                .map_or(0, |edit| edit.working_bytes(image, key.region));
        if working_bytes > memory.limit.saturating_sub(memory.reserved)
            || pixels * if key.high_precision { 8 } else { 4 }
                > gpu.limit.saturating_sub(gpu.reserved)
        {
            return None;
        }
        let mut working = self.memory.try_reserve_within_limit(working_bytes)?;
        let gpu_lease = Arc::new(
            self.gpu_memory
                .try_reserve_within_limit(pixels * if key.high_precision { 8 } else { 4 })?,
        );
        let started = std::time::Instant::now();
        let mut layer_frame = false;
        let result = {
            let Ok(filter) = self.filter.try_lock() else {
                self.shared.0.lock().unwrap().statistics.resident_zoom_busy += 1;
                return None;
            };
            let renderer = filter.as_ref()?.clone();
            drop(filter);
            let layers = key
                .edit
                .as_ref()
                .filter(|_| self.layer_compute_verified.load(Ordering::Acquire))
                .and_then(|edit| {
                    renderer
                        .render_resident_layers(
                            image,
                            edit,
                            key.region,
                            &self.memory,
                            &self.gpu_memory,
                            &mut working,
                        )
                        .ok()
                });
            if let Some(frame) = layers {
                layer_frame = true;
                Ok(frame)
            } else {
                renderer.render_resident_edit(
                    image,
                    key.region,
                    &self.memory,
                    &self.gpu_memory,
                    &mut working,
                    key.edit.as_ref(),
                )
            }
        };
        let Ok(mut frame) = result else {
            self.shared
                .0
                .lock()
                .unwrap()
                .statistics
                .resident_zoom_misses += 1;
            return None;
        };
        working.shrink(pixels * if key.high_precision { 24 } else { 12 });
        let lease = Arc::new(working);
        let state = self.render_state.as_ref()?.clone();
        frame.submit(&state.queue);
        let retained = (lease.clone(), gpu_lease.clone());
        state.queue.on_submitted_work_done(move || drop(retained));
        let id = state.renderer.write().register_native_texture(
            &state.device,
            &frame.texture.create_view(&Default::default()),
            eframe::wgpu::FilterMode::Nearest,
        );
        self.install(
            lane.into(),
            Entry {
                key: key.clone(),
                placement,
                superseded: false,
                touched: self.clock,
                zoomed_at: None,
                texture: FrameTexture::Gpu { id, frame, state },
                lease,
                gpu_lease,
            },
        );
        self.errors.remove(lane);
        // An older asynchronous margin must not overwrite this newer scale.
        self.waiting.remove(lane);
        let mut shared = self.shared.0.lock().unwrap();
        shared.discard_jobs(|job_lane, _, _| job_lane == lane);
        shared.completed.retain(|job| job.lane != lane);
        shared.statistics.gpu_frames += 1;
        shared.statistics.gpu_layer_frames += u64::from(layer_frame);
        shared.statistics.resident_layer_frames += u64::from(layer_frame);
        let elapsed = started.elapsed().as_micros() as u64;
        if key.edit.is_some() {
            shared.statistics.resident_edit_frames += 1;
            shared.statistics.gpu_edit_frames += 1;
            shared.statistics.resident_edit_encode_us += elapsed;
        } else {
            shared.statistics.resident_zoom_frames += 1;
            shared.statistics.resident_zoom_encode_us += elapsed;
        }
        shared.statistics.resident_zoom_max_us =
            shared.statistics.resident_zoom_max_us.max(elapsed);
        shared.statistics.resident_zoom_pixels += pixels;
        ui.ctx().request_repaint();
        Some(())
    }

    pub(crate) fn hold_scale_change(
        &mut self,
        ui: &egui::Ui,
        lane: &str,
        source: u64,
        rect: Rect,
        region: Region,
    ) -> bool {
        if !self
            .entries
            .get(lane)
            .is_some_and(|e| e.placement.region.step != region.step)
        {
            return false;
        }
        self.waiting.remove(lane);
        {
            let mut shared = self.shared.0.lock().unwrap();
            shared.discard_jobs(|job_lane, _, _| job_lane == lane);
            shared.completed.retain(|job| job.lane != lane);
        }
        self.paint_previous(ui, lane, source, rect, region);
        true
    }

    pub(crate) fn is_current(&self, lane: &str, source: u64, region: Region) -> bool {
        self.entries.get(lane).is_some_and(|e| {
            !e.superseded
                && e.key.source == source
                && exact_uv(e.key.region, region).is_some()
                && e.key.edit.is_none()
        })
    }
    pub fn source_is_current(&self, lane: &str, source: u64) -> bool {
        self.entries
            .get(lane)
            .is_some_and(|e| !e.superseded && e.key.source == source && e.key.edit.is_none())
    }

    /// While the CPU edit is pending, draw only the last complete frame. Never
    /// enqueue the neutral source as a replacement for an already edited photo.
    pub fn paint_pending(
        &mut self,
        ui: &egui::Ui,
        lane: &str,
        source: u64,
        rect: Rect,
        region: Region,
    ) {
        self.waiting.remove(lane);
        self.errors.remove(lane);
        {
            let mut shared = self.shared.0.lock().unwrap();
            shared.discard_jobs(|job_lane, _, _| job_lane == lane);
            shared.completed.retain(|job| job.lane != lane);
        }
        if let Some(entry) = self.entries.get_mut(lane) {
            entry.superseded = true;
        }
        if let Some(old) = self.wide.remove(lane) {
            self.retire(old);
        }
        self.paint_previous(ui, lane, source, rect, region);
    }

    fn paint_previous(
        &mut self,
        ui: &egui::Ui,
        lane: &str,
        source: u64,
        rect: Rect,
        region: Region,
    ) {
        if let Some(entry) = self.entries.get_mut(lane).filter(|e| {
            e.placement.region.step != region.step
                || (e.zoomed_at.is_some() && exact_uv(e.key.region, region).is_none())
        }) && let Some((held_rect, uv)) = reproject(
            entry.key.region,
            entry.placement.region,
            entry.placement.rect,
        ) {
            // A scale transition is atomic: retain the last complete scale
            // until the next one is ready, without filtering display pixels a
            // second time. UI clipping also protects neighbouring panels.
            entry.touched = self.clock;
            ui.painter()
                .image(entry.texture.id(), held_rect, uv, Color32::WHITE);
            if self.capturing {
                let visible = held_rect.intersect(rect).intersect(ui.clip_rect());
                self.coverage.push(PaintCoverage {
                    region,
                    displayed_region: Some(entry.placement.region),
                    lane: lane.into(),
                    source,
                    displayed_source: Some(entry.key.source),
                    displayed_revision: entry.key.edit.as_ref().map(|e| e.revision),
                    fraction: if visible.is_positive() {
                        (visible.area() / rect.area()).clamp(0., 1.)
                    } else {
                        0.
                    },
                    exact: false,
                    held: true,
                });
                self.captures.push(Capture {
                    compute: if matches!(&entry.texture, FrameTexture::Gpu { .. }) {
                        "GPU"
                    } else {
                        "CPU"
                    },
                    source: entry.key.source,
                    rect: held_rect,
                    clip: ui.clip_rect(),
                    region: entry.placement.region,
                });
            }
            return;
        }
        let mut drawn = Vec::with_capacity(2);
        let mut displayed_source = None;
        let mut displayed_revision = None;
        let mut displayed_region = None;
        let mut previous: Vec<_> = [
            (true, self.wide.get_mut(lane)),
            (false, self.entries.get_mut(lane)),
        ]
        .into_iter()
        .filter_map(|(wide, entry)| entry.map(|e| (wide, e)))
        .collect();
        // Fill missing coverage first, then preserve the finest available
        // samples on top. A wider fitted view must not replace sharp detail.
        previous.sort_by(|(_, a), (_, b)| {
            (b.key.region.step[0] * b.key.region.step[1])
                .total_cmp(&(a.key.region.step[0] * a.key.region.step[1]))
        });
        for (wide, entry) in previous {
            entry.touched = self.clock;
            if let Some((coverage, uv)) = reproject(entry.key.region, region, rect) {
                if wide {
                    self.wide_reprojections += 1;
                }
                drawn.push(coverage.intersect(rect));
                displayed_source = Some(entry.key.source);
                displayed_revision = entry.key.edit.as_ref().map(|e| e.revision);
                displayed_region = Some(entry.key.region);
                ui.painter()
                    .image(entry.texture.id(), coverage, uv, Color32::WHITE);
            }
        }
        let overlap = if drawn.len() == 2 {
            let intersection = drawn[0].intersect(drawn[1]);
            if intersection.is_positive() {
                intersection.area()
            } else {
                0.
            }
        } else {
            0.
        };
        let covered = drawn.iter().map(|r| r.area()).sum::<f32>() - overlap;
        if self.capturing {
            self.coverage.push(PaintCoverage {
                region,
                displayed_region,
                lane: lane.into(),
                source,
                displayed_source,
                displayed_revision,
                fraction: (covered / rect.area()).clamp(0., 1.),
                exact: false,
                held: false,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        placement: Placement,
        key: Key,
        zooming: bool,
    ) {
        let rect = placement.rect;
        let region = key.region;
        if self.waiting.get(&lane) != Some(&key) {
            let pixels = region.size[0] as u64 * region.size[1] as u64;
            let display_bytes = pixels * if self.high_precision { 8 } else { 4 };
            let required = pixels * 80
                + 4 * 1024 * 1024
                + key
                    .edit
                    .as_ref()
                    .map_or(0, |e| e.working_bytes(image, region));
            if required > self.memory.usage().limit || display_bytes > self.gpu_memory.usage().limit
            {
                self.errors.insert(
                    lane,
                    (key, "Vista oltre il limite di memoria configurato".into()),
                );
                ui.ctx().request_repaint();
                return;
            }
            // Make room for the next visible frame before admitting it. Retired
            // GPU credits become available after completion in a later poll.
            let shortage = required
                .saturating_sub(
                    self.memory
                        .usage()
                        .limit
                        .saturating_sub(self.memory.usage().reserved),
                )
                .max(
                    display_bytes.saturating_sub(
                        self.gpu_memory
                            .usage()
                            .limit
                            .saturating_sub(self.gpu_memory.usage().reserved),
                    ),
                );
            // These are optional, including the current lane's backup. Their
            // credits remain retired until the previous submission completes.
            if shortage > 0 {
                self.discard_wide();
            }
            let mut released = 0;
            while released < shortage {
                let victim = self
                    .entries
                    .iter()
                    .filter(|(name, e)| **name != lane && e.touched + 1 < self.clock)
                    .min_by_key(|(_, e)| e.touched)
                    .map(|(name, _)| name.clone());
                let Some(victim) = victim else {
                    break;
                };
                let old = self.entries.remove(&victim).unwrap();
                released += old.lease.bytes();
                self.retired.push((self.clock, old.lease, old.gpu_lease));
            }
            let mut shared = self.shared.0.lock().unwrap();
            shared.discard_jobs(|job_lane, _, warm| job_lane == lane || warm);
            if shared.jobs.len() < 64
                && (self.waiting.contains_key(&lane) || self.waiting.len() < 64)
            {
                let Some(lease) = self.memory.try_reserve(required) else {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(25));
                    return;
                };
                let Some(gpu_lease) = self.gpu_memory.try_reserve(display_bytes) else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Limite cache GPU: aumentare la quota",
                        egui::FontId::proportional(12.),
                        Color32::LIGHT_RED,
                    );
                    return;
                };
                shared.jobs.push_back(Job {
                    warm: false,
                    cancel: Arc::new(AtomicBool::new(false)),
                    lane: lane.clone(),
                    key: key.clone(),
                    placement,
                    image: image.clone(),
                    lease,
                    gpu_lease,
                    zooming,
                });
                self.waiting.insert(lane, key);
                self.shared.1.notify_one();
            }
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(25));
    }
}

fn same_content(a: &Key, b: &Key) -> bool {
    a.source == b.source
        && a.stretch == b.stretch
        && a.high_precision == b.high_precision
        && a.edit == b.edit
        && a.photographic == b.photographic
}

/// A nearest texture crop is exact only at the same scale and integer sample
/// offset. Fractional zooms must retain their original sampling phase too.
fn exact_uv(previous: Region, current: Region) -> Option<Rect> {
    let mut offset = [0.; 2];
    for (a, offset) in offset.iter_mut().enumerate() {
        if previous.size[a] == 0
            || current.size[a] == 0
            || !previous.step[a].is_finite()
            || previous.step[a] <= 0.
            || (previous.step[a] - current.step[a]).abs() > previous.step[a] * 1e-12
        {
            return None;
        }
        let delta = (current.origin[a] - previous.origin[a]) / previous.step[a];
        if !delta.is_finite()
            || (delta - delta.round()).abs() > 1e-6
            || delta.round() < 0.
            || delta.round() + f64::from(current.size[a]) > f64::from(previous.size[a])
        {
            return None;
        }
        *offset = delta.round();
    }
    Some(Rect::from_min_max(
        Pos2::new(
            (offset[0] / f64::from(previous.size[0])) as f32,
            (offset[1] / f64::from(previous.size[1])) as f32,
        ),
        Pos2::new(
            ((offset[0] + f64::from(current.size[0])) / f64::from(previous.size[0])) as f32,
            ((offset[1] + f64::from(current.size[1])) / f64::from(previous.size[1])) as f32,
        ),
    ))
}

fn pan_region(mut region: Region, source: [u32; 2], margin: f64) -> Region {
    for (a, extent) in source.into_iter().enumerate() {
        if !region.step[a].is_finite() || region.step[a] <= 0. {
            return region;
        }
        let border = (f64::from(region.size[a]) * margin).ceil().min(512.);
        let before = (region.origin[a] / region.step[a])
            .floor()
            .clamp(0., border);
        let end = region.origin[a] + f64::from(region.size[a]) * region.step[a];
        let after = ((f64::from(extent) - end) / region.step[a])
            .floor()
            .clamp(0., border);
        region.origin[a] -= before * region.step[a];
        region.size[a] += before as u32 + after as u32;
    }
    region
}

/// Reuse only source coverage present in the previous frame; panning must not
/// stretch old pixels into unrelated source coordinates.
fn reproject(previous: Region, current: Region, rect: Rect) -> Option<(Rect, Rect)> {
    let old_end = std::array::from_fn::<_, 2, _>(|a| {
        previous.origin[a] + previous.step[a] * previous.size[a] as f64
    });
    let end = std::array::from_fn::<_, 2, _>(|a| {
        current.origin[a] + current.step[a] * current.size[a] as f64
    });
    let min = std::array::from_fn::<_, 2, _>(|a| previous.origin[a].max(current.origin[a]));
    let max = std::array::from_fn::<_, 2, _>(|a| old_end[a].min(end[a]));
    if min[0] >= max[0] || min[1] >= max[1] {
        return None;
    }
    let position = |p: [f64; 2]| {
        Pos2::new(
            rect.min.x
                + ((p[0] - current.origin[0]) / (end[0] - current.origin[0])) as f32 * rect.width(),
            rect.min.y
                + ((p[1] - current.origin[1]) / (end[1] - current.origin[1])) as f32
                    * rect.height(),
        )
    };
    let uv = |p: [f64; 2]| {
        Pos2::new(
            ((p[0] - previous.origin[0]) / (old_end[0] - previous.origin[0])) as f32,
            ((p[1] - previous.origin[1]) / (old_end[1] - previous.origin[1])) as f32,
        )
    };
    Some((
        Rect::from_min_max(position(min), position(max)),
        Rect::from_min_max(uv(min), uv(max)),
    ))
}
fn source_area(region: Region) -> f64 {
    region.step[0] * region.size[0] as f64 * region.step[1] * region.size[1] as f64
}

impl Drop for Presenter {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stop = true;
            state.discard_jobs(|_, _, _| true);
            state.jobs.clear();
        }
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // No new submissions can arrive after the encoder has joined. Let the
        // device complete outstanding work and release callback-owned credits.
        if let Some(state) = &self.render_state {
            let _ = state.device.poll(eframe::wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(5)),
            });
        }
    }
}

/// Snap both image edges to physical pixels, then sample exactly that raster.
pub fn fitted_rect(area: Rect, source: [u32; 2], ppp: f32) -> Rect {
    let scale = (area.width() * ppp / source[0] as f32).min(area.height() * ppp / source[1] as f32);
    let size = egui::vec2(
        (source[0] as f32 * scale).round().max(1.),
        (source[1] as f32 * scale).round().max(1.),
    );
    let top = area.center() * ppp - size / 2.;
    Rect::from_min_size(
        egui::pos2(top.x.round() / ppp, top.y.round() / ppp),
        size / ppp,
    )
}
pub fn fitted(
    presenter: &mut Presenter,
    ui: &egui::Ui,
    lane: String,
    image: &Arc<ImageLevels>,
    area: Rect,
) {
    let source = image.source_size();
    let rect = fitted_rect(area, source, ui.ctx().pixels_per_point());
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    ];
    presenter.paint(ui, lane, image, rect, Region::fitted(source, size));
}

pub fn fitted_pending(
    presenter: &mut Presenter,
    ui: &egui::Ui,
    lane: &str,
    image: &ImageLevels,
    source: [u32; 2],
    area: Rect,
) {
    let ppp = ui.ctx().pixels_per_point();
    let rect = fitted_rect(area, source, ppp);
    let size = [
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    ];
    presenter.paint_pending(ui, lane, image.id(), rect, Region::fitted(source, size));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_work_revokes_staged_preparation_and_rejects_late_same_key_completion() {
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(2, 2, vec![[0.3, 0.4, 0.5, 1.]; 4]).unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        let memory = MemoryBudget::new(4096);
        let region = Region::fitted([2, 2], [2, 2]);
        let key = Key {
            source: image.id(),
            region,
            edit: None,
            photographic: true,
            high_precision: false,
            stretch: Default::default(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let mut shared = Shared {
            warming_edit: Some(Preparation {
                key: key.clone(),
                lane: "photo".into(),
                cancel: cancel.clone(),
            }),
            ..Default::default()
        };
        shared.jobs.push_back(Job {
            warm: true,
            cancel: cancel.clone(),
            lane: "photo".into(),
            key: key.clone(),
            placement: Placement {
                rect: Rect::NOTHING,
                region,
            },
            image,
            lease: memory.try_reserve(512).unwrap(),
            gpu_lease: memory.try_reserve(0).unwrap(),
            zooming: false,
        });
        let running = shared.jobs.pop_front().unwrap();
        assert!(shared.finish_graph_stage(running, false, false));
        let running = shared.jobs.pop_front().unwrap();
        assert!(shared.finish_graph_stage(running, false, true));
        assert!(shared.warming_edit.is_some());
        assert!(shared.prepared_edit.is_none());
        assert_eq!(shared.statistics.prepared_edit_frames, 0);
        assert_eq!(memory.usage().reserved, 512);
        let running = shared.jobs.pop_front().unwrap();
        shared.discard_jobs(|lane, _, _| lane == "unrelated");
        assert!(!cancel.load(Ordering::Acquire));
        shared.discard_jobs(|_, _, warm| warm);
        assert!(cancel.load(Ordering::Acquire));
        assert!(shared.warming_edit.is_none());
        // In-flight credits still belong to the worker until it stops.
        assert_eq!(memory.usage().reserved, 512);
        let replacement = Arc::new(AtomicBool::new(false));
        shared.warming_edit = Some(Preparation {
            key: key.clone(),
            lane: "photo".into(),
            cancel: replacement.clone(),
        });
        shared.finish_preparation(&key, &cancel, true);
        assert!(shared.warming_edit.is_some());
        assert!(shared.prepared_edit.is_none());
        assert_eq!(shared.statistics.prepared_edit_frames, 0);
        assert!(!shared.finish_graph_stage(running, false, true));
        assert!(shared.jobs.is_empty());
        assert!(shared.warming_edit.is_some());
        assert_eq!(memory.usage().reserved, 0);
        shared.finish_preparation(&key, &replacement, true);
        assert!(shared.warming_edit.is_none());
        assert_eq!(shared.prepared_edit, Some(key));
        assert_eq!(shared.statistics.prepared_edit_frames, 1);
    }

    #[test]
    fn zoom_prioritizes_visible_frame_then_restores_pan_margin() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(ctx.clone(), MemoryBudget::new(128 * 1024 * 1024), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(1024, 768, vec![[0.4, 0.3, 0.2, 1.]; 1024 * 768])
                    .unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        let initial = Region {
            size: [160, 120],
            origin: [432., 324.],
            step: [1.; 2],
        };
        let lane = "view:zoom-priority";
        p.install(lane.into(), fixture_entry(&ctx, &p, image.id(), initial));
        let draw = |p: &mut Presenter, region, time| {
            p.begin_capture();
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    p.paint(
                        ui,
                        lane.into(),
                        &image,
                        Rect::from_min_size(Pos2::ZERO, egui::vec2(160., 120.)),
                        region,
                    );
                },
            );
            out.textures_delta.clear();
        };
        let zoom = Region {
            step: [0.8; 2],
            ..initial
        };
        draw(&mut p, zoom, 1.);
        assert_eq!(
            p.waiting[lane].region, zoom,
            "Zoom must not render an invisible margin"
        );
        let wait = |p: &mut Presenter, region, time| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                p.poll(&ctx);
                draw(p, region, time);
                if p.is_current(lane, image.id(), region) {
                    break;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };
        wait(&mut p, zoom, 1.01);
        let pan = Region {
            origin: [456., 332.],
            ..zoom
        };
        draw(&mut p, pan, 1.02);
        assert!(
            p.coverage()[0].held,
            "Pan immediately after zoom must hold a complete frame until ready"
        );
        assert_eq!(p.coverage()[0].displayed_region, Some(zoom));
        assert_eq!(p.waiting[lane].region, pan);
        wait(&mut p, pan, 1.03);
        draw(&mut p, pan, 1.1);
        assert!(
            p.is_idle(),
            "Do not start optional work between zoom inputs"
        );
        draw(&mut p, pan, 1.2);
        assert_eq!(
            p.waiting[lane].region,
            pan_region(pan, image.source_size(), 0.5)
        );
    }

    #[test]
    fn zoom_never_stretches_the_previous_display_frame_while_render_is_pending() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(ctx.clone(), MemoryBudget::new(128 * 1024 * 1024), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(1024, 768, vec![[0.4, 0.3, 0.2, 1.]; 1024 * 768])
                    .unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        let rect = Rect::from_min_size(Pos2::new(20., 20.), egui::vec2(160., 120.));
        let initial = Region {
            size: [160, 120],
            origin: [432., 324.],
            step: [1.; 2],
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            p.poll(&ctx);
            let mut out = ctx.run_ui(Default::default(), |ui| {
                p.paint(ui, "view:zoom".into(), &image, rect, initial)
            });
            out.textures_delta.clear();
            if p.is_current("view:zoom", image.id(), initial) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let texture = p.entries["view:zoom"].texture.id();
        let size = p.entries["view:zoom"].texture.size();
        // No worker publication: wheel changes and direction reversals must
        // not stretch already filtered display pixels to the requested zoom.
        for step in [0.73, 0.62, 0.9, 1.12, 1.33] {
            let region = Region {
                size: initial.size,
                origin: [512. - 80. * step, 384. - 60. * step],
                step: [step; 2],
            };
            let mut out = ctx.run_ui(Default::default(), |ui| {
                p.paint(ui, "view:zoom".into(), &image, rect, region)
            });
            out.textures_delta.clear();
            let meshes: Vec<_> = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Mesh(m) if m.texture_id == texture => Some(m),
                    _ => None,
                })
                .collect();
            assert!(!meshes.is_empty());
            for mesh in meshes {
                let a = mesh.vertices[0];
                for b in &mesh.vertices[1..] {
                    for (axis, size) in size.iter().enumerate() {
                        let screen = (b.pos[axis] - a.pos[axis]).abs();
                        let texels = (b.uv[axis] - a.uv[axis]).abs() * *size as f32;
                        assert!(
                            (screen - texels).abs() < 0.001,
                            "Zoom rescaled the old display frame: {screen} backing pixels from {texels} texels"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn wheel_waits_for_missing_detail_then_publishes_native_scale() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(ctx.clone(), MemoryBudget::new(128 * 1024 * 1024), None);
        let source =
            tr_core::color::LinearImage::new(1024, 768, vec![[0.4, 0.2, 0.3, 1.]; 1024 * 768])
                .unwrap();
        let small = Arc::new(
            ImageLevels::from_source(
                source.clone(),
                tr_core::preview::PreviewRequest {
                    edge: 64,
                    ..tr_core::preview::PreviewRequest::full()
                },
            )
            .unwrap(),
        );
        let native = Arc::new(
            ImageLevels::from_source(source, tr_core::preview::PreviewRequest::full()).unwrap(),
        );
        let mut transform = tr_core::ViewTransform::default();
        let draw = |p: &mut Presenter,
                    t: &mut tr_core::ViewTransform,
                    image: &Arc<ImageLevels>,
                    refining,
                    events| {
            p.begin_capture();
            let mut sampled = false;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(180., 140.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let (_, sample) = crate::viewport_with_options(
                        ui,
                        p,
                        image,
                        t,
                        "view:wheel",
                        crate::ViewportOptions {
                            refining,
                            ..Default::default()
                        },
                    );
                    sampled = sample.is_some();
                },
            );
            out.textures_delta.clear();
            sampled
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            p.poll(&ctx);
            draw(&mut p, &mut transform, &small, false, vec![]);
            if p.coverage().iter().any(|c| c.exact) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let old = p.entries["view:wheel"].texture.id();
        assert!(!draw(
            &mut p,
            &mut transform,
            &small,
            false,
            vec![
                egui::Event::PointerMoved(Pos2::new(80., 70.)),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., 600.),
                    modifiers: Default::default()
                },
            ]
        ));
        assert!(transform.zoom.is_some());
        for _ in 0..5 {
            assert!(!draw(&mut p, &mut transform, &small, true, vec![]));
            assert_eq!(p.entries["view:wheel"].texture.id(), old);
            assert!(p.coverage()[0].held);
            assert!(
                p.waiting.is_empty(),
                "Missing detail must not render an enlarged reduced source"
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            p.poll(&ctx);
            draw(&mut p, &mut transform, &native, false, vec![]);
            if p.coverage()
                .iter()
                .any(|c| c.exact && c.source == native.id())
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_ne!(p.entries["view:wheel"].texture.id(), old);
        assert!(!p.coverage()[0].held);
    }

    #[test]
    fn pan_margin_preserves_filter_phase_and_pixels_at_fractional_zoom_and_edges() {
        let image = ImageLevels::from_source(
            tr_core::color::LinearImage::new(
                257,
                193,
                (0..257 * 193)
                    .map(|i| {
                        [
                            ((i * 17) % 113) as f32 / 112.,
                            ((i * 31) % 97) as f32 / 96.,
                            (i % 3) as f32 / 2.,
                            if i % 11 == 0 { 0.4 } else { 1. },
                        ]
                    })
                    .collect(),
            )
            .unwrap(),
            tr_core::preview::PreviewRequest::full(),
        )
        .unwrap();
        for step in [0.5, 1., 1. / 1.37, 2.25] {
            for origin in [[0., 0.], [0.3, 0.7], [55.3, 40.7], [180., 125.]] {
                let visible = Region {
                    size: [31, 23],
                    origin,
                    step: [step; 2],
                };
                let guarded = pan_region(visible, image.source_size(), 0.5);
                assert!(exact_uv(guarded, visible).is_some());
                let frame = image.render(guarded).unwrap().to_display();
                let direct = image.render(visible).unwrap().to_display();
                let offset: [usize; 2] = std::array::from_fn(|a| {
                    ((visible.origin[a] - guarded.origin[a]) / step).round() as usize
                });
                for y in 0..visible.size[1] as usize {
                    let start = ((y + offset[1]) * guarded.size[0] as usize + offset[0]) * 4;
                    let width = visible.size[0] as usize * 4;
                    for (a, b) in frame[start..start + width]
                        .iter()
                        .zip(&direct[y * width..(y + 1) * width])
                    {
                        assert!(a.abs_diff(*b) <= 1, "Guard changed sampling at {visible:?}");
                    }
                }
                assert!(
                    exact_uv(
                        guarded,
                        Region {
                            origin: [origin[0] + step * 0.25, origin[1]],
                            ..visible
                        }
                    )
                    .is_none()
                );
                assert!(
                    exact_uv(
                        guarded,
                        Region {
                            step: [step * 1.001; 2],
                            ..visible
                        }
                    )
                    .is_none()
                );
            }
        }
    }

    #[test]
    fn pointer_drag_keeps_resident_detail_on_every_move_and_release() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(ctx.clone(), MemoryBudget::new(128 * 1024 * 1024), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(1024, 768, vec![[0.3, 0.4, 0.5, 1.]; 1024 * 768])
                    .unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        let mut transform = tr_core::ViewTransform::default();
        transform.set_zoom(1.37);
        let draw = |p: &mut Presenter,
                    transform: &mut tr_core::ViewTransform,
                    events: Vec<egui::Event>| {
            p.begin_capture();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(180., 140.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::viewport(ui, p, &image, transform, "view:pointer-pan", None, false);
                },
            );
            output.textures_delta.clear();
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            p.poll(&ctx);
            draw(&mut p, &mut transform, vec![]);
            if p.coverage().first().is_some_and(|c| c.exact) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let center = transform.center;
        draw(
            &mut p,
            &mut transform,
            vec![
                egui::Event::PointerMoved(Pos2::new(80., 70.)),
                egui::Event::PointerButton {
                    pos: Pos2::new(80., 70.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        for x in [100., 120., 90., 65., 40., 75.] {
            draw(
                &mut p,
                &mut transform,
                vec![egui::Event::PointerMoved(Pos2::new(x, 70.))],
            );
            assert!(p.coverage().iter().all(|c| c.exact && c.fraction >= 0.999));
            assert_eq!(p.captures().len(), 1);
        }
        assert_ne!(transform.center, center);
        draw(
            &mut p,
            &mut transform,
            vec![egui::Event::PointerButton {
                pos: Pos2::new(75., 70.),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert!(p.coverage().iter().all(|c| c.exact && c.fraction >= 0.999));
    }

    #[test]
    fn resident_pan_reuses_full_detail_before_the_next_render_completes() {
        let ctx = egui::Context::default();
        let mut presenter = Presenter::new(ctx.clone(), MemoryBudget::new(128 * 1024 * 1024), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(
                    1024,
                    768,
                    (0..1024 * 768)
                        .map(|i| [if i % 2 == 0 { 0.1 } else { 0.9 }, 0.2, 0.3, 1.])
                        .collect(),
                )
                .unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        let region = Region {
            size: [160, 120],
            origin: [400., 300.],
            step: [1., 1.],
        };
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(160., 120.));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            presenter.poll(&ctx);
            let mut output = ctx.run_ui(Default::default(), |ui| {
                presenter.paint(ui, "view:pan".into(), &image, rect, region)
            });
            output.textures_delta.clear();
            if presenter.is_current("view:pan", image.id(), region) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // No poll: the first displaced draw must use resident samples, even if
        // the next render is slow or the cursor reverses before it completes.
        for origin in [[432., 316.], [368., 280.], [448., 324.]] {
            presenter.begin_capture();
            let pan = Region { origin, ..region };
            let mut output = ctx.run_ui(Default::default(), |ui| {
                presenter.paint(ui, "view:pan".into(), &image, rect, pan)
            });
            output.textures_delta.clear();
            assert!(
                presenter.coverage()[0].fraction >= 0.999,
                "Pan left uncovered pixels"
            );
            assert!(
                presenter.coverage()[0].exact,
                "Pan substituted a reduced or rescaled frame"
            );
            assert_eq!(presenter.captures()[0].region, pan);
        }
    }

    fn fixture_entry(ctx: &egui::Context, p: &Presenter, source: u64, region: Region) -> Entry {
        Entry {
            placement: Placement {
                rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(2., 2.)),
                region,
            },
            superseded: false,
            key: Key {
                source,
                region,
                edit: None,
                photographic: false,
                high_precision: false,
                stretch: Default::default(),
            },
            texture: FrameTexture::Cpu(ctx.load_texture(
                "wide-fixture",
                egui::ColorImage::filled([2, 2], Color32::WHITE),
                TextureOptions::NEAREST,
            )),
            touched: p.clock,
            zoomed_at: None,
            lease: Arc::new(p.memory.try_reserve(48).unwrap()),
            gpu_lease: Arc::new(p.gpu_memory.try_reserve(16).unwrap()),
        }
    }
    fn seed_wide(ctx: &egui::Context, p: &mut Presenter, lane: &str, source: u64) {
        let full = Region::fitted([2, 2], [2, 2]);
        let crop = Region {
            origin: [1., 0.],
            step: [0.5, 1.],
            ..full
        };
        p.install(lane.into(), fixture_entry(ctx, p, source, full));
        p.install(lane.into(), fixture_entry(ctx, p, source, crop));
    }
    #[test]
    fn provisional_wide_coverage_does_not_replace_sharper_overlap() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(ctx.clone(), MemoryBudget::new(4096), None);
        seed_wide(&ctx, &mut p, "view:pan", 7);
        let sharp = p.entries["view:pan"].texture.id();
        let coarse = p.wide["view:pan"].texture.id();
        p.begin_capture();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            p.paint_previous(
                ui,
                "view:pan",
                7,
                Rect::from_min_size(Pos2::ZERO, egui::vec2(20., 20.)),
                Region {
                    size: [2, 2],
                    origin: [0.5, 0.],
                    step: [0.5, 1.],
                },
            );
        });
        output.textures_delta.clear();
        let textures: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => Some(m.texture_id),
                _ => None,
            })
            .collect();
        assert_eq!(textures, vec![coarse, sharp]);
        assert_eq!(p.coverage()[0].fraction, 1.);
        assert_eq!(
            p.coverage()[0].displayed_region,
            Some(p.entries["view:pan"].key.region)
        );
        assert!(!p.coverage()[0].exact);
    }
    #[test]
    fn superseded_edits_hold_only_the_last_texture_and_reject_late_work() {
        let ctx = egui::Context::default();
        let memory = MemoryBudget::new(4096);
        let mut p = Presenter::new(ctx.clone(), memory.clone(), None);
        let lane = "view:edit";
        let region = Region::fitted([2, 2], [2, 2]);
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(2., 2.));
        let first = fixture_entry(&ctx, &p, 1, region);
        let texture = first.texture.id();
        p.install(lane.into(), first);
        let old_key = p.entries[lane].key.clone();
        p.waiting.insert(lane.into(), old_key.clone());
        p.supersede_sources(&std::collections::HashSet::from([1]));
        assert_eq!(memory.usage().reserved, 48);
        assert_eq!(p.entries[lane].texture.id(), texture);
        assert!(!p.is_current(lane, 1, region));
        // An already active worker can still finish after logical revocation.
        p.shared.0.lock().unwrap().completed.push_back(Completed {
            placement: Placement { rect, region },
            lane: lane.into(),
            key: old_key,
            result: Ok(Rendered::Cpu(vec![0; 16])),
            lease: Arc::new(memory.try_reserve(48).unwrap()),
            gpu_lease: Arc::new(p.gpu_memory.try_reserve(16).unwrap()),
        });
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 48);
        assert_eq!(p.entries[lane].texture.id(), texture);
        for (name, coverage) in [(lane, 1.), ("view:other-photo", 0.), ("view:proof", 0.)] {
            p.begin_capture();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                p.paint_pending(ui, name, 99, rect, region);
            });
            output.textures_delta.clear();
            assert_eq!(p.coverage()[0].fraction, coverage);
            assert!(!p.coverage()[0].exact);
            assert!(p.waiting.is_empty());
            assert!(p.shared.0.lock().unwrap().jobs.is_empty());
        }
        let crop = Region {
            origin: [1., 0.],
            step: [0.5, 1.],
            ..region
        };
        p.install(lane.into(), fixture_entry(&ctx, &p, 2, crop));
        assert!(
            p.wide.is_empty(),
            "An older edit must not return during a later pan"
        );
        assert_eq!(memory.usage().reserved, 96); // Retired until completion.
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 48);
        assert!(p.is_current(lane, 2, crop));
        p.invalidate_sources(&std::collections::HashSet::from([2]));
        assert!(!p.entries.contains_key(lane));
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 0);
    }
    #[test]
    fn wider_frame_covers_pan_and_fit_without_duplicating_or_early_releasing_credits() {
        let ctx = egui::Context::default();
        let memory = MemoryBudget::new(4096);
        let mut p = Presenter::new(ctx.clone(), memory.clone(), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(2, 2, vec![[1.; 4]; 4]).unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        // Cache reopening may create a new provider for the same revision.
        let full = Region::fitted([2, 2], [2, 2]);
        p.install(
            "view:test".into(),
            fixture_entry(&ctx, &p, image.id() + 100, full),
        );
        p.install(
            "view:test".into(),
            fixture_entry(
                &ctx,
                &p,
                image.id(),
                Region {
                    origin: [1., 0.],
                    step: [0.5, 1.],
                    ..full
                },
            ),
        );
        assert_eq!(p.wide.len(), 1);
        assert_eq!(memory.usage().reserved, 96); // Two owned frames, no duplicate lease.
        assert_eq!(p.gpu_memory.usage().reserved, 32);
        for region in [
            Region {
                origin: [0.5, 0.],
                step: [0.5, 1.],
                size: [2, 2],
            },
            Region::fitted([2, 2], [2, 2]),
        ] {
            p.begin_capture();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                p.paint(
                    ui,
                    "view:test".into(),
                    &image,
                    Rect::from_min_size(Pos2::ZERO, egui::vec2(2., 2.)),
                    region,
                )
            });
            output.textures_delta.clear();
            assert!(p.coverage().iter().all(|c| c.fraction == 1. && !c.exact));
        }
        // Pan composes wide coverage; zoom holds the previous sharp scale.
        assert!(p.statistics().wide_reprojections >= 1);
        assert!(p.coverage()[0].held);
        p.invalidate_sources(&std::collections::HashSet::from([
            image.id(),
            image.id() + 100,
        ]));
        assert!(p.wide.is_empty() && p.entries.is_empty());
        assert_eq!(memory.usage().reserved, 96); // Still retired, not immediately free.
        p.poll(&ctx); // No GPU queue in this fixture; native path uses its callback.
        assert_eq!(memory.usage().reserved, 0);
        assert_eq!(p.gpu_memory.usage().reserved, 0);
    }
    #[test]
    fn automatic_gpu_admission_grows_for_work_but_respects_pressure_and_manual_limits() {
        let mib = 1024 * 1024;
        let memory = MemoryBudget::new(6144 * mib);
        let mut p = Presenter::new(egui::Context::default(), memory.clone(), None);
        p.configure_gpu_automatic();
        assert_eq!(p.gpu_memory.usage().limit, 768 * mib);
        assert_eq!(p.gpu_memory.usage().ceiling, 1536 * mib);
        let source = p.gpu_memory.try_reserve(700 * mib).unwrap();
        assert!(p.gpu_memory.try_reserve_within_limit(100 * mib).is_none());
        assert_eq!(p.gpu_memory.usage().growths, 0);
        let work = p.gpu_memory.try_reserve(300 * mib).unwrap();
        let grown = p.gpu_memory.usage();
        assert_eq!(grown.growths, 1);
        assert!(grown.limit > grown.base && grown.limit <= grown.ceiling);
        p.configure_gpu_automatic();
        assert_eq!(p.gpu_memory.usage().limit, grown.limit);
        p.set_pressure(true);
        p.configure_gpu_automatic();
        assert_eq!(p.gpu_memory.usage().limit, 768 * mib);
        assert_eq!(p.gpu_memory.usage().reserved, 1000 * mib);
        assert!(p.gpu_memory.try_reserve(1).is_none());
        p.set_pressure(false);
        // A smaller global budget cannot revoke leases already in flight.
        memory.configure(1024 * mib);
        p.configure_gpu_automatic();
        assert_eq!(p.gpu_memory.usage().limit, 256 * mib);
        assert_eq!(p.gpu_memory.usage().reserved, 1000 * mib);
        assert!(p.gpu_memory.try_reserve(1).is_none());
        drop((source, work));
        assert_eq!(p.gpu_memory.usage().reserved, 0);
        memory.configure(16 * 1024 * mib);
        p.configure_gpu_automatic();
        assert_eq!(p.gpu_memory.usage().ceiling, 2048 * mib);
        p.configure_gpu_limit(256 * mib);
        assert!(!p.gpu_memory.usage().automatic);
        assert!(p.gpu_memory.try_reserve(257 * mib).is_none());
    }
    #[test]
    fn wider_frames_obey_identity_slots_pressure_and_lowered_quota() {
        let ctx = egui::Context::default();
        let memory = MemoryBudget::new(4096);
        let mut p = Presenter::new(ctx.clone(), memory.clone(), None);
        for source in 1..=3 {
            seed_wide(&ctx, &mut p, &format!("view:{source}"), source);
        }
        assert_eq!(p.wide.len(), 2);
        assert_eq!(p.statistics().wide_peak_frames, 2);
        let lane = p.wide.keys().next().unwrap().clone();
        p.install(
            lane.clone(),
            fixture_entry(&ctx, &p, 99, Region::fitted([2, 2], [2, 2])),
        );
        assert!(!p.wide.contains_key(&lane));
        p.set_pressure(true);
        assert!(p.wide.is_empty());
        seed_wide(&ctx, &mut p, "view:pressure", 100);
        assert!(p.wide.is_empty());
        p.set_pressure(false);
        seed_wide(&ctx, &mut p, "view:quota", 101);
        assert_eq!(p.wide.len(), 1);
        p.configure_gpu_limit(16);
        assert!(p.wide.is_empty());
        p.clear();
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 0);
        assert_eq!(p.gpu_memory.usage().reserved, 0);
    }
    #[test]
    fn optional_coverage_yields_to_renderer_and_decoder_admission() {
        let ctx = egui::Context::default();
        let required = 4 * 80 + 4 * 1024 * 1024;
        let memory = MemoryBudget::new(required + 64);
        let mut p = Presenter::new(ctx.clone(), memory.clone(), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(2, 2, vec![[1.; 4]; 4]).unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        seed_wide(&ctx, &mut p, "view:test", image.id());
        let mut output = ctx.run_ui(Default::default(), |ui| {
            p.paint(
                ui,
                "view:test".into(),
                &image,
                Rect::from_min_size(Pos2::ZERO, egui::vec2(2., 2.)),
                Region::fitted([2, 2], [2, 2]),
            )
        });
        output.textures_delta.clear();
        assert!(p.wide.is_empty());
        assert_eq!(memory.usage().reserved, 96);
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 48);
        assert!(memory.try_reserve(required).is_some());
        p.clear();
        p.poll(&ctx);
        seed_wide(&ctx, &mut p, "view:decode", image.id());
        p.release_optional_frames();
        assert!(p.wide.is_empty());
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 48);
        p.clear();
        p.poll(&ctx);
        assert_eq!(memory.usage().reserved, 0);
    }
    #[test]
    fn diagnostic_coverage_distinguishes_exact_overlap_and_missing_content() {
        let ctx = egui::Context::default();
        // Zero budget prevents asynchronous jobs from changing the fixture.
        let budget = MemoryBudget::new(0);
        let mut presenter = Presenter::new(ctx.clone(), budget.clone(), None);
        let image = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(2, 2, vec![[1.; 4]; 4]).unwrap(),
                tr_core::preview::PreviewRequest::full(),
            )
            .unwrap(),
        );
        let region = Region::fitted([2, 2], [2, 2]);
        let texture = ctx.load_texture(
            "coverage",
            egui::ColorImage::filled([2, 2], Color32::WHITE),
            Default::default(),
        );
        presenter.entries.insert(
            "view".into(),
            Entry {
                placement: Placement {
                    rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(2., 2.)),
                    region,
                },
                superseded: false,
                key: Key {
                    stretch: Default::default(),
                    high_precision: false,
                    source: image.id(),
                    region,
                    edit: None,
                    photographic: false,
                },
                texture: FrameTexture::Cpu(texture),
                touched: 0,
                zoomed_at: None,
                lease: Arc::new(budget.try_reserve(0).unwrap()),
                gpu_lease: Arc::new(budget.try_reserve(0).unwrap()),
            },
        );
        for (lane, origin, fraction, exact) in [
            ("view", [0., 0.], 1., true),
            ("view", [1., 0.], 0.5, false),
            ("view", [2., 0.], 0., false),
            ("different-revision", [0., 0.], 0., false),
        ] {
            presenter.begin_capture();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                presenter.paint(
                    ui,
                    lane.into(),
                    &image,
                    Rect::from_min_size(Pos2::ZERO, egui::vec2(2., 2.)),
                    Region { origin, ..region },
                );
            });
            output.textures_delta.clear();
            assert!(!presenter.coverage().is_empty());
            assert!(
                presenter
                    .coverage()
                    .iter()
                    .all(|c| c.source == image.id() && c.fraction == fraction && c.exact == exact)
            );
        }
    }
    #[test]
    fn refinement_reprojects_overlap_and_never_substitutes_an_uncovered_region() {
        let old = Region::fitted([100, 100], [100, 100]);
        let new = Region {
            origin: [50., 0.],
            ..old
        };
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(100., 100.));
        let (coverage, uv) = reproject(old, new, rect).unwrap();
        assert_eq!(coverage.max.x, 50.);
        assert_eq!(uv.min.x, 0.5);
        assert!(
            reproject(
                old,
                Region {
                    origin: [100., 0.],
                    ..old
                },
                rect
            )
            .is_none()
        );
    }
}
