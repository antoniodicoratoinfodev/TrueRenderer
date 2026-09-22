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
}
struct Job {
    lane: String,
    key: Key,
    image: Arc<ImageLevels>,
    lease: Lease,
    gpu_lease: Lease,
}
enum Rendered {
    Cpu(Vec<u8>),
    Cpu16(Vec<u8>),
    Gpu(crate::resident_compute::GpuFrame),
}
struct Completed {
    lane: String,
    key: Key,
    result: Result<Rendered, String>,
    lease: Arc<Lease>,
    gpu_lease: Arc<Lease>,
}
#[derive(Default)]
struct Shared {
    jobs: VecDeque<Job>,
    completed: VecDeque<Completed>,
    stop: bool,
    statistics: ComputeStatistics,
}
struct Entry {
    key: Key,
    texture: FrameTexture,
    touched: u64,
    lease: Arc<Lease>,
    gpu_lease: Arc<Lease>,
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
    pub cpu_frames: u64,
    pub gpu_frames: u64,
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
    pub source: u64,
    pub fraction: f32,
    pub exact: bool,
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
    compute_mode: Arc<AtomicU8>,
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
        let compute_mode = Arc::new(AtomicU8::new(0));
        let pressure = Arc::new(AtomicBool::new(false));
        let worker_pressure = pressure.clone();
        let verified = compute_verified.clone();
        let mode = compute_mode.clone();
        let worker_gpu = render_state.clone();
        let worker_memory = memory.clone();
        let worker_gpu_memory = gpu_memory.clone();
        let high_precision = render_state.as_ref().is_some_and(|s| {
            matches!(
                s.target_format,
                eframe::wgpu::TextureFormat::Rgb10a2Unorm
                    | eframe::wgpu::TextureFormat::Rgba16Float
            )
        });
        let worker = thread::spawn(move || {
            let mut filter = worker_gpu
                .as_ref()
                .filter(|r| r.device.limits().max_storage_buffers_per_shader_stage >= 7)
                .map(|r| {
                    crate::resident_compute::GpuFilter::with_precision(
                        r.device.clone(),
                        high_precision,
                    )
                });
            loop {
                let mut job = {
                    let (lock, ready) = &*worker_shared;
                    let mut state = lock.lock().unwrap();
                    while state.jobs.is_empty() && !state.stop {
                        let (next, timeout) = ready
                            .wait_timeout(state, std::time::Duration::from_secs(2))
                            .unwrap();
                        state = next;
                        if (timeout.timed_out() || worker_pressure.load(Ordering::Acquire))
                            && let Some(filter) = &mut filter
                        {
                            filter.clear();
                        }
                    }
                    if state.stop {
                        break;
                    }
                    state.jobs.pop_front().unwrap()
                };
                let pixels = job.key.region.size[0] as u64 * job.key.region.size[1] as u64;
                let choice = mode.load(Ordering::Acquire);
                if (choice == 1
                    || worker_pressure.load(Ordering::Acquire)
                    || worker_gpu_memory.usage().reserved > worker_gpu_memory.usage().limit
                    || worker_memory.usage().reserved > worker_memory.usage().limit)
                    && let Some(filter) = &mut filter
                {
                    filter.clear();
                }
                let mut fallback = None;
                let gpu = if verified.load(Ordering::Acquire)
                    && !job.image.scientific()
                    && choice != 1
                    && (choice == 2 || pixels >= 512 * 1024)
                    && let Some(filter) = &mut filter
                {
                    filter
                        .render(
                            &job.image,
                            job.key.region,
                            &worker_memory,
                            &worker_gpu_memory,
                            &mut job.lease,
                        )
                        .map_err(|e| {
                            fallback = Some(format!("{e:#}"));
                            e
                        })
                        .ok()
                } else {
                    None
                };
                let is_gpu = gpu.is_some();
                let result = if let Some(frame) = gpu {
                    Ok(Rendered::Gpu(frame))
                } else {
                    job.image
                        .render(job.key.region)
                        .map(|mut image| {
                            if job.image.scientific() {
                                job.key.stretch.apply(&mut image);
                            }
                            if high_precision {
                                Rendered::Cpu16(
                                    image
                                        .pixels
                                        .iter()
                                        .flat_map(|p| {
                                            tr_core::color::display_float(*p, 119. / 255.)
                                                .into_iter()
                                                .flat_map(|v| {
                                                    half::f16::from_f32(v).to_bits().to_le_bytes()
                                                })
                                        })
                                        .collect(),
                                )
                            } else {
                                Rendered::Cpu(image.to_display())
                            }
                        })
                        .map_err(|e| format!("{e:#}"))
                };
                job.lease.shrink(if result.is_ok() {
                    pixels * if high_precision { 24 } else { 12 }
                } else {
                    0
                });
                let lease = Arc::new(job.lease);
                let gpu_lease = Arc::new(job.gpu_lease);
                let mut state = worker_shared.0.lock().unwrap();
                if state.stop {
                    break;
                }
                if !is_gpu {
                    state.statistics.cpu_frames += 1;
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
            compute_mode,
        }
    }
    pub fn statistics(&self) -> ComputeStatistics {
        let mut stats = self.shared.0.lock().unwrap().statistics.clone();
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
    fn install(&mut self, lane: String, entry: Entry) {
        if self
            .wide
            .get(&lane)
            .is_some_and(|old| source_area(old.key.region) <= source_area(entry.key.region))
        {
            let old = self.wide.remove(&lane).unwrap();
            self.retire(old);
        }
        if let Some(old) = self.entries.remove(&lane) {
            let retain = lane.starts_with("view:")
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
        self.discard_wide();
        self.waiting.clear();
        for (_, entry) in self.entries.drain() {
            self.retired
                .push((self.clock, entry.lease, entry.gpu_lease));
        }
        self.errors.clear();
        let mut shared = self.shared.0.lock().unwrap();
        shared.jobs.clear();
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
                            texture,
                            touched: self.clock,
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
        let lanes: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| sources.contains(&entry.key.source))
            .map(|(lane, _)| lane.clone())
            .collect();
        for lane in lanes {
            if let Some(entry) = self.entries.remove(&lane) {
                self.retired
                    .push((self.clock, entry.lease, entry.gpu_lease));
            }
        }
        let mut shared = self.shared.0.lock().unwrap();
        shared.jobs.retain(|job| !sources.contains(&job.key.source));
        shared
            .completed
            .retain(|job| !sources.contains(&job.key.source));
        // An active encoder may finish; the missing waiting key discards it
        // before submission. In-flight GPU work keeps its completion leases.
    }
    pub fn set_pressure(&mut self, active: bool) {
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
        self.waiting.is_empty()
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
    pub fn configure_gpu_limit(&mut self, bytes: u64) {
        if self.gpu_memory.usage().limit != bytes {
            self.errors.clear();
        }
        self.gpu_memory.configure(bytes);
        self.trim_wide();
    }
    /// A lane must identify the asset revision, rendering recipe and source
    /// coordinate space. Distinct provider instances may hold compatible levels.
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        rect: Rect,
        region: Region,
    ) {
        let key = Key {
            stretch: if image.scientific() {
                self.stretch
            } else {
                Default::default()
            },
            high_precision: self.high_precision,
            source: image.id(),
            region,
        };
        if let Some(entry) = self.entries.get_mut(&lane).filter(|e| e.key == key) {
            entry.touched = self.clock;
            if self.capturing {
                self.coverage.push(PaintCoverage {
                    source: image.id(),
                    fraction: 1.,
                    exact: true,
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
            ui.painter().image(
                entry.texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
            return;
        }
        let mut covered = 0.;
        let wide_is_better = self
            .wide
            .get(&lane)
            .and_then(|e| reproject(e.key.region, region, rect))
            .is_some_and(|(wide, _)| {
                let current = self
                    .entries
                    .get(&lane)
                    .and_then(|e| reproject(e.key.region, region, rect))
                    .map_or(0., |(r, _)| r.area());
                wide.area() > current
            });
        let previous = if wide_is_better {
            self.wide_reprojections += 1;
            self.wide.get_mut(&lane)
        } else {
            self.entries.get_mut(&lane)
        };
        if let Some(entry) = previous {
            entry.touched = self.clock;
            if let Some((coverage, uv)) = reproject(entry.key.region, region, rect) {
                covered = coverage.intersect(rect).area() / rect.area();
                ui.painter()
                    .image(entry.texture.id(), coverage, uv, Color32::WHITE);
            }
        }
        if self.capturing {
            self.coverage.push(PaintCoverage {
                source: image.id(),
                fraction: covered,
                exact: false,
            });
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
        if self.waiting.get(&lane) != Some(&key) {
            let pixels = region.size[0] as u64 * region.size[1] as u64;
            let display_bytes = pixels * if self.high_precision { 8 } else { 4 };
            let required = pixels * 80 + 4 * 1024 * 1024;
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
            shared.jobs.retain(|j| j.lane != lane);
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
                    lane: lane.clone(),
                    key: key.clone(),
                    image: image.clone(),
                    lease,
                    gpu_lease,
                });
                self.waiting.insert(lane, key);
                self.shared.1.notify_one();
            }
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(25));
    }
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
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stop = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_entry(ctx: &egui::Context, p: &Presenter, source: u64, region: Region) -> Entry {
        Entry {
            key: Key {
                source,
                region,
                high_precision: false,
                stretch: Default::default(),
            },
            texture: FrameTexture::Cpu(ctx.load_texture(
                "wide-fixture",
                egui::ColorImage::filled([2, 2], Color32::WHITE),
                TextureOptions::NEAREST,
            )),
            touched: p.clock,
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
        assert!(p.statistics().wide_reprojections >= 2);
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
                key: Key {
                    stretch: Default::default(),
                    high_precision: false,
                    source: image.id(),
                    region,
                },
                texture: FrameTexture::Cpu(texture),
                touched: 0,
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
