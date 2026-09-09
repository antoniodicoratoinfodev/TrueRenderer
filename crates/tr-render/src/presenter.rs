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
    Gpu {
        id: egui::TextureId,
        frame: crate::resident_compute::GpuFrame,
        state: eframe::egui_wgpu::RenderState,
    },
}
impl FrameTexture {
    fn id(&self) -> egui::TextureId {
        match self {
            Self::Cpu(t) => t.id(),
            Self::Gpu { id, .. } => *id,
        }
    }
    fn size(&self) -> [usize; 2] {
        match self {
            Self::Cpu(t) => t.size(),
            Self::Gpu { frame, .. } => frame.size.map(|v| v as usize),
        }
    }
}
impl Drop for FrameTexture {
    fn drop(&mut self) {
        if let Self::Gpu { id, state, .. } = self {
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
}
pub struct Capture {
    pub compute: &'static str,
    pub source: u64,
    pub rect: Rect,
    pub clip: Rect,
    pub region: Region,
}
pub struct Presenter {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    waiting: HashMap<String, Key>,
    entries: HashMap<String, Entry>,
    errors: HashMap<String, (Key, String)>,
    clock: u64,
    capturing: bool,
    captures: Vec<Capture>,
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
        let worker = thread::spawn(move || {
            let mut filter = worker_gpu
                .as_ref()
                .filter(|r| r.device.limits().max_storage_buffers_per_shader_stage >= 7)
                .map(|r| crate::resident_compute::GpuFilter::new(r.device.clone()));
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
                        .map(|image| Rendered::Cpu(image.to_display()))
                        .map_err(|e| format!("{e:#}"))
                };
                job.lease
                    .shrink(if result.is_ok() { pixels * 12 } else { 0 });
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
            shared,
            worker: Some(worker),
            waiting: HashMap::new(),
            entries: HashMap::new(),
            errors: HashMap::new(),
            clock: 0,
            capturing: false,
            captures: Vec::new(),
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
        self.shared.0.lock().unwrap().statistics.clone()
    }
    pub fn begin_capture(&mut self) {
        self.capturing = true;
        self.captures.clear();
    }
    pub fn captures(&self) -> &[Capture] {
        &self.captures
    }
    pub fn clear(&mut self) {
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
                    if let Some(old) = self.entries.insert(
                        finished.lane,
                        Entry {
                            key: finished.key,
                            texture,
                            touched: self.clock,
                            lease: finished.lease,
                            gpu_lease: finished.gpu_lease,
                        },
                    ) {
                        self.retired.push((self.clock, old.lease, old.gpu_lease));
                    }
                }
                Err(error) => {
                    if self.errors.len() >= 64 {
                        self.errors.clear();
                    }
                    self.errors.insert(finished.lane, (finished.key, error));
                }
            }
        }
        while self.entries.len() > 64
            || self
                .entries
                .values()
                .map(|e| e.texture.size()[0] * e.texture.size()[1] * 4)
                .sum::<usize>()
                > self.gpu_memory.usage().limit as usize
        {
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
    }
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<ImageLevels>,
        rect: Rect,
        region: Region,
    ) {
        let key = Key {
            source: image.id(),
            region,
        };
        if let Some(entry) = self.entries.get_mut(&lane).filter(|e| e.key == key) {
            entry.touched = self.clock;
            if self.capturing {
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
        if let Some(entry) = self.entries.get_mut(&lane) {
            entry.touched = self.clock;
            if let Some((coverage, uv)) = reproject(entry.key.region, region, rect) {
                ui.painter()
                    .image(entry.texture.id(), coverage, uv, Color32::WHITE);
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
        if self.waiting.get(&lane) != Some(&key) {
            let pixels = region.size[0] as u64 * region.size[1] as u64;
            let required = pixels * 80 + 4 * 1024 * 1024;
            if required > self.memory.usage().limit || pixels * 4 > self.gpu_memory.usage().limit {
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
                    (pixels * 4).saturating_sub(
                        self.gpu_memory
                            .usage()
                            .limit
                            .saturating_sub(self.gpu_memory.usage().reserved),
                    ),
                );
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
                let Some(gpu_lease) = self.gpu_memory.try_reserve(pixels * 4) else {
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
