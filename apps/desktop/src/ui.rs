use crate::service::{Event, Request, Service};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tr_app::{Command, Effect, State};
use tr_core::{
    Item, Label, ViewMode, ViewTransform,
    preview::{ImageCompute, PreviewPriority, PreviewQuality, PreviewRequest},
    protocol::RasterInfo,
    provider::ImageLevels,
};

const TEXT: Color32 = Color32::from_rgb(232, 232, 232);
const MUTED: Color32 = Color32::from_rgb(154, 154, 154);
const AMBER: Color32 = Color32::from_rgb(217, 164, 65);
const SURFACE: Color32 = Color32::from_rgb(38, 38, 38);

#[derive(Clone)]
struct CachedImage {
    digest: String,
    info: RasterInfo,
    histogram: [[u32; 256]; 3],
    pyramid: Arc<ImageLevels>,
    touched: u64,
    transport: &'static str,
    worker_pid: Option<u32>,
}
pub struct Startup {
    pub smoke: bool,
    pub sampling_smoke: bool,
    pub external_smoke: bool,
    pub settings_smoke: bool,
    pub open: Option<PathBuf>,
}
pub struct TrueRenderer {
    state: State,
    service: Service,
    root: PathBuf,
    folder: PathBuf,
    generation: u64,
    cache: HashMap<(String, PreviewRequest), CachedImage>,
    presenter: tr_render::presenter::Presenter,
    pending_images: HashSet<(String, PreviewRequest)>,
    full_overrides: HashSet<String>,
    demand: HashSet<(String, PreviewRequest)>,
    last_demand: HashSet<(String, PreviewRequest)>,
    foreground_demand: HashSet<(String, PreviewRequest)>,
    navigation_changed: Instant,
    navigation_anchor: Option<usize>,
    navigation_direction: isize,
    primary_demand: HashSet<String>,
    viewer_prefetch_edge: u32,
    requested_at: HashMap<(String, PreviewRequest), Instant>,
    recent_latencies: VecDeque<u64>,

    demand_jobs: Vec<crate::decode_pool::Job>,
    promoted: HashMap<(String, PreviewRequest), PreviewPriority>,
    rejected_admissions: u64,
    errors: HashMap<String, String>,
    cell_size: f32,
    status: String,
    scanning: bool,
    keyword_text: String,
    keyword_id: String,
    undo_available: bool,
    show_help: bool,
    show_settings: bool,
    settings_smoke: bool,
    cache_settings: crate::cache::Settings,
    settings_data: PathBuf,
    preparation_paused: bool,
    rebuild: VecDeque<Item>,
    rebuild_total: usize,
    cache_action: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    show_inspector: bool,
    show_filmstrip: bool,
    fullscreen: bool,
    adapter: String,
    surface: String,
    frame_number: u64,
    sample: Option<tr_render::Sample>,
    smoke: bool,
    sampling_smoke: bool,
    external_smoke: bool,
    pending_selection: Option<PathBuf>,
    started: Instant,
    smoke_stage: u8,
    screenshots: HashSet<String>,
    fatal: bool,
    closing: bool,
    search_focus: bool,
    gpu_rx: std::sync::mpsc::Receiver<Result<tr_render::preview_compute::ComputeCheck, String>>,
    gpu_status: String,
    gpu_passed: bool,
    image_focus_ids: HashSet<egui::Id>,
    grid_columns: i32,
    sample_source: String,
}
impl TrueRenderer {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        root: PathBuf,
        data: PathBuf,
        worker: PathBuf,
        startup: Startup,
    ) -> Self {
        let Startup {
            smoke,
            sampling_smoke,
            external_smoke,
            settings_smoke,
            open,
        } = startup;
        let ctx = &cc.egui_ctx;
        ctx.set_theme(egui::Theme::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_gray(28);
        visuals.window_fill = SURFACE;
        visuals.extreme_bg_color = Color32::from_gray(20);
        visuals.faint_bg_color = SURFACE;
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = Color32::from_rgb(49, 66, 84);
        visuals.selection.stroke = egui::Stroke::new(1.5, Color32::from_rgb(100, 163, 229));
        visuals.widgets.inactive.bg_fill = Color32::from_gray(46);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., Color32::from_gray(90));
        ctx.set_visuals(visuals);
        ctx.style_mut_of(egui::Theme::Dark, |s| {
            s.spacing.item_spacing = Vec2::new(8., 8.);
            s.spacing.button_padding = Vec2::new(10., 5.);
            s.spacing.interact_size = Vec2::new(28., 28.);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(13.));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
            s.text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(11.));
            s.text_styles
                .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.));
        });
        let (adapter, surface) = cc
            .wgpu_render_state
            .as_ref()
            .map(|r| {
                (
                    format!(
                        "{} · {:?}",
                        r.adapter.get_info().name,
                        r.adapter.get_info().backend
                    ),
                    format!("{:?}", r.target_format),
                )
            })
            .unwrap_or(("GPU non disponibile".into(), "sconosciuta".into()));
        let service = Service::start(root.clone(), data.clone(), worker, ctx.clone());
        let (gpu_tx, gpu_rx) = std::sync::mpsc::sync_channel(1);
        if let Some(gpu) = &cc.wgpu_render_state {
            let device = gpu.device.clone();
            let queue = gpu.queue.clone();
            let ctx = ctx.clone();
            let report_root = root.clone();
            // All queue submissions share the UI thread with surface configure.
            // Run this one-time qualification before entering the event loop;
            // a concurrent submission can make wgpu surface configure abort.
            {
                let result = tr_render::preview_compute::check(&device, &queue)
                    .map_err(|e| format!("{e:#}"));
                if std::env::args().any(|a| a.ends_with("smoke")) {
                    let report = match &result {
                        Ok(check) => {
                            serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":check.failures==0 && check.display_failures==0,"check":check,"linear_threshold":"1e-5 + 1e-4 * abs(cpu)","display_threshold_levels":1,"scope":"Separable WGSL with canonical coefficients and persistent display pipeline, odd sizes, alpha, boundaries, 1:1 and signed linear values up to 1000. GPU timing includes qualification compilation/upload/readback; not an interactive speed claim."})
                        }
                        Err(error) => serde_json::json!({"passed":false,"error":error}),
                    };
                    let _ = std::fs::write(
                        report_root.join("reports/preview-quality-gpu-macos.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                }
                if std::env::args().any(|a| a == "--preview-performance-smoke") {
                    let report = match tr_render::preview_compute::performance(&device, &queue) {
                        Ok(cases) => {
                            serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"cases":cases,"trials_per_case":100,"bootstrap_pairs":1000,"scope":"Rotating triads of scalar CPU, parallel/SIMD CPU and persistent GPU filter+SDR encoding through queue completion; source buffers reused after first upload. Excludes source/hash/cache and egui surface presentation. OS caches not flushed; generated pixels on the same adapter. Not user-event p95 or real RAW throughput."})
                        }
                        Err(error) => serde_json::json!({"error":format!("{error:#}")}),
                    };
                    let _ = std::fs::write(
                        report_root.join("reports/preview-compute-performance-macos.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                }
                let _ = gpu_tx.send(result);
                ctx.request_repaint();
            }
        }
        let presenter = tr_render::presenter::Presenter::new(
            ctx.clone(),
            service.cache.memory.clone(),
            cc.wgpu_render_state.clone(),
        );
        let folder = root.join("corpus");
        let mut app = Self {
            state: State::default(),
            cache_settings: service.cache.settings(),
            settings_data: data,
            cache_action: None,
            preparation_paused: false,
            rebuild: VecDeque::new(),
            rebuild_total: 0,
            service,
            root,
            folder: folder.clone(),
            generation: 0,
            cache: HashMap::new(),
            presenter,
            pending_images: HashSet::new(),
            full_overrides: HashSet::new(),
            demand: HashSet::new(),
            last_demand: HashSet::new(),
            foreground_demand: HashSet::new(),
            navigation_changed: Instant::now(),
            navigation_anchor: None,
            navigation_direction: 1,
            primary_demand: HashSet::new(),
            viewer_prefetch_edge: 2048,
            requested_at: HashMap::new(),
            recent_latencies: VecDeque::new(),
            demand_jobs: Vec::new(),
            promoted: HashMap::new(),
            rejected_admissions: 0,
            errors: HashMap::new(),
            cell_size: 206.,
            status: "Avvio del motore…".into(),
            scanning: true,
            keyword_text: String::new(),
            keyword_id: String::new(),
            undo_available: false,
            show_help: false,
            show_settings: settings_smoke,
            settings_smoke,
            show_inspector: true,
            show_filmstrip: true,
            fullscreen: false,
            adapter,
            surface,
            frame_number: 0,
            sample: None,
            smoke,
            sampling_smoke,
            external_smoke,
            pending_selection: None,
            started: Instant::now(),
            smoke_stage: 0,
            screenshots: HashSet::new(),
            fatal: false,
            closing: false,
            search_focus: false,
            gpu_rx,
            gpu_status: "Diagnostica GPU in corso…".into(),
            gpu_passed: false,
            image_focus_ids: HashSet::new(),
            grid_columns: 1,
            sample_source: String::new(),
        };
        if let Some(path) = open {
            app.open_path(path);
        } else {
            app.open_folder(folder);
        }
        app
    }
    fn open_path(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.pending_selection = None;
            self.open_folder(path);
        } else if let Some(folder) = path.parent() {
            let folder = folder.to_path_buf();
            self.pending_selection = path.canonicalize().ok();
            self.open_folder(folder);
        }
    }
    fn request(&mut self, request: Request) -> bool {
        if self.service.high.try_send(request).is_err() {
            self.status = "Coda occupata: riprovare fra un momento".into();
            false
        } else {
            true
        }
    }
    fn open_folder(&mut self, folder: PathBuf) {
        self.rebuild.clear();
        self.rebuild_total = 0;
        self.generation += 1;
        self.service
            .generation
            .store(self.generation, Ordering::Relaxed);
        self.cache.clear();
        self.presenter.clear();
        self.pending_images.clear();
        self.full_overrides.clear();
        self.demand.clear();
        self.last_demand.clear();
        self.demand_jobs.clear();
        self.promoted.clear();
        self.requested_at.clear();
        self.recent_latencies.clear();
        self.navigation_anchor = None;
        self.errors.clear();
        self.state.replace_items(vec![]);
        self.keyword_text.clear();
        self.keyword_id.clear();
        self.folder = folder.clone();
        self.scanning = self.request(Request::Scan {
            folder,
            generation: self.generation,
        });
        self.status = "Lettura della cartella…".into();
    }
    fn command(&mut self, command: Command) {
        for effect in self.state.dispatch(command) {
            match effect {
                Effect::Save {
                    id,
                    expected_revision,
                    annotation,
                } => {
                    if !self.request(Request::Save {
                        id: id.clone(),
                        expected: expected_revision,
                        annotation,
                    }) {
                        self.state.dispatch(Command::Failed(id));
                    }
                }
                Effect::Undo => {
                    self.request(Request::Undo);
                }
            }
        }
    }
    fn quality(&self, item: &Item) -> PreviewQuality {
        if self.full_overrides.contains(&item.id) {
            PreviewQuality::Full
        } else {
            self.service.cache.settings().quality
        }
    }
    fn preview_request(&self, item: &Item, edge: u32) -> PreviewRequest {
        PreviewRequest {
            quality: self.quality(item),
            edge,
        }
    }
    fn image_key(&self, item: &Item, edge: u32) -> (String, PreviewRequest) {
        (item.id.clone(), self.preview_request(item, edge))
    }
    fn ensure_image(&mut self, item: &Item, edge: u32) {
        self.ensure_image_priority(
            item,
            edge,
            if edge == 0 || self.state.view == ViewMode::Grid {
                PreviewPriority::Immediate
            } else {
                PreviewPriority::SecondaryVisible
            },
        );
    }
    fn ensure_image_priority(&mut self, item: &Item, edge: u32, priority: PreviewPriority) {
        if !item.approved {
            return;
        }
        let key = self.image_key(item, edge);
        if matches!(
            priority,
            PreviewPriority::Immediate | PreviewPriority::Refinement
        ) {
            self.primary_demand.insert(item.id.clone());
        }
        self.demand.insert(key.clone());
        if let Some(c) = self.cache.get_mut(&key) {
            c.touched = self.frame_number;
            return;
        }
        // Equivalent geometries can share an already resident tail. Do not pin
        // larger ancestors merely to satisfy a thumbnail, or cross qualities.
        let compatible = self
            .cache
            .iter()
            .find(|((id, request), cached)| {
                id == &item.id
                    && request.quality == key.1.quality
                    && cached.pyramid.sufficient_for(key.1)
                    && cached.pyramid.requested_base(key.1) == 0
            })
            .map(|(_, cached)| cached.clone());
        if let Some(mut cached) = compatible {
            cached.touched = self.frame_number;
            self.cache.insert(key, cached);
            return;
        }
        if self
            .errors
            .contains_key(&format!("{}:{:?}", item.id, key.1))
        {
            return;
        }
        let pending = self.pending_images.contains(&key);
        if pending && self.promoted.get(&key).is_some_and(|old| *old <= priority) {
            return;
        }
        if let Some(job) = self
            .demand_jobs
            .iter_mut()
            .find(|job| job.item.id == item.id && job.request == key.1)
        {
            job.priority = job.priority.min(priority);
            return;
        }
        self.demand_jobs.push(crate::decode_pool::Job {
            item: item.clone(),
            request: key.1,
            priority,
            generation: self.generation,
        });
    }
    fn background_demand(&mut self, ctx: &egui::Context) {
        if self.foreground_demand != self.demand {
            let anchor = self
                .state
                .visible
                .iter()
                .position(|i| self.primary_demand.contains(&self.state.items[*i].id));
            if let (Some(previous), Some(current)) = (self.navigation_anchor, anchor)
                && current != previous
            {
                self.navigation_direction = if current > previous { 1 } else { -1 };
            }
            self.navigation_anchor = anchor;
            self.foreground_demand.clone_from(&self.demand);
            self.navigation_changed = Instant::now();
        }
        let delay = Duration::from_millis(tr_app::scheduler::prefetch_delay_ms(
            self.recent_latencies.make_contiguous(),
        ));
        if self.rebuild.is_empty() && self.navigation_changed.elapsed() < delay {
            if self.service.cache.settings().prefetch != crate::cache::Prefetch::Disabled {
                ctx.request_repaint_after(delay.saturating_sub(self.navigation_changed.elapsed()));
            }
            return;
        }
        if self.preparation_paused || self.scanning || self.service.cache.under_pressure() {
            return;
        }
        let settings = self.service.cache.settings();
        if !self.rebuild.is_empty() {
            self.trim_images(true);
        }
        let memory = self.service.cache.memory.usage();
        // Leave at least half the budget for immediate navigation; never feed a
        // background decode while visible dependencies are still outstanding.
        if memory.reserved > memory.limit / 2
            || self.demand.iter().any(|key| {
                !self.cache.contains_key(key)
                    && !self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))
            })
        {
            return;
        }
        let mut candidates = Vec::new();
        if !self.rebuild.is_empty() {
            if let Some(item) = self.rebuild.front() {
                candidates.push((item.clone(), 256));
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        } else {
            let primary: Vec<_> = self
                .state
                .visible
                .iter()
                .enumerate()
                .filter(|(_, i)| self.primary_demand.contains(&self.state.items[**i].id))
                .map(|(index, _)| index)
                .collect();
            if let (Some(first), Some(last)) = (primary.first(), primary.last()) {
                let grid = self.state.view == ViewMode::Grid;
                let count = match settings.prefetch {
                    crate::cache::Prefetch::Disabled => 0,
                    crate::cache::Prefetch::Automatic => {
                        if grid {
                            (primary.len() * 3 / 2).clamp(2, 16)
                        } else {
                            2
                        }
                    }
                    crate::cache::Prefetch::Extended => {
                        if grid {
                            (primary.len() * 3 / 2).clamp(2, 32)
                        } else {
                            4
                        }
                    }
                };
                if settings.reusable_bytes(self.service.cache.physical_mib) > 0 {
                    for index in tr_app::scheduler::prefetch_indices(
                        self.state.visible.len(),
                        *first..=*last,
                        self.navigation_direction,
                        count,
                    ) {
                        let item = &self.state.items[self.state.visible[index]];
                        if !self.demand.iter().any(|(id, _)| id == &item.id) {
                            candidates.push((
                                item.clone(),
                                if grid { 256 } else { self.viewer_prefetch_edge },
                            ));
                        }
                    }
                }
            }
        }
        for (item, edge) in candidates {
            let key = self.image_key(&item, edge);
            self.demand.insert(key.clone());
            let rebuild = self.rebuild.front().is_some_and(|i| i.id == item.id) && edge == 256;
            if self.pending_images.contains(&key)
                || self
                    .demand_jobs
                    .iter()
                    .any(|j| j.item.id == item.id && j.request == key.1)
                || (!rebuild
                    && (self.cache.contains_key(&key)
                        || self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))))
            {
                continue;
            }
            self.demand_jobs.push(crate::decode_pool::Job {
                item,
                request: key.1,
                priority: if rebuild {
                    PreviewPriority::Background
                } else if self.state.view == ViewMode::Grid {
                    PreviewPriority::AdjacentRows
                } else {
                    PreviewPriority::NeighborPreview
                },
                generation: self.generation,
            });
        }
    }
    fn flush_demand(&mut self) {
        self.pending_images.retain(|key| self.demand.contains(key));
        self.requested_at
            .retain(|key, _| self.pending_images.contains(key));
        self.promoted.retain(|key, _| self.demand.contains(key));
        if self.last_demand == self.demand && self.demand_jobs.is_empty() {
            return;
        }
        let jobs = std::mem::take(&mut self.demand_jobs);
        let keys: Vec<_> = jobs
            .iter()
            .map(|job| ((job.item.id.clone(), job.request), job.priority))
            .collect();
        if self
            .service
            .high
            .try_send(Request::ViewDemand {
                wanted: self.demand.iter().cloned().collect(),
                jobs,
                generation: self.generation,
            })
            .is_ok()
        {
            for (key, priority) in keys {
                if self.pending_images.insert(key.clone()) {
                    self.requested_at.insert(key.clone(), Instant::now());
                }
                self.promoted
                    .entry(key)
                    .and_modify(|old| *old = (*old).min(priority))
                    .or_insert(priority);
            }
            self.last_demand.clone_from(&self.demand);
        }
    }
    fn trim_images(&mut self, pressure: bool) {
        let settings = self.service.cache.settings();
        let limit = if pressure {
            0
        } else {
            settings.reusable_bytes(self.service.cache.physical_mib) as usize
        };
        loop {
            let pinned: HashSet<_> = self
                .cache
                .values()
                .filter(|c| c.touched + 1 >= self.frame_number)
                .map(|c| c.pyramid.id())
                .collect();
            let mut counted = HashSet::new();
            let bytes: usize = self
                .cache
                .values()
                .filter(|c| !pinned.contains(&c.pyramid.id()) && counted.insert(c.pyramid.id()))
                .map(|c| c.pyramid.byte_len())
                .sum();
            if bytes <= limit && self.cache.len() <= 1024 {
                break;
            }
            let victim = self
                .cache
                .iter()
                .filter(|(_, c)| c.touched + 1 < self.frame_number)
                .min_by_key(|(_, c)| c.touched)
                .map(|(k, _)| k.clone());
            if let Some(victim) = victim {
                self.cache.remove(&victim);
            } else {
                break;
            }
        }
    }
    fn set_quality(&mut self, quality: PreviewQuality) {
        let mut settings = self.service.cache.settings();
        if settings.quality == quality {
            return;
        }
        settings.quality = quality;
        self.full_overrides.clear();
        self.errors.clear();
        self.cache_settings = settings.clone();
        self.service.cache.configure(settings.clone());
        let data = self.settings_data.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.cache_action = Some(rx);
        self.status = "Qualità aggiornata; override per foto rimossi".into();
        let cache = self.service.cache.clone();
        std::thread::spawn(move || {
            let _ = tx.send(
                cache
                    .save_current(&data)
                    .map_err(|e| format!("Salvataggio qualità fallito: {e:#}")),
            );
        });
    }
    fn full_for_current(&mut self) {
        if let Some(item) = self.state.current_item() {
            self.full_overrides.insert(item.id.clone());
        }
        if self.state.view == ViewMode::Compare {
            self.full_overrides
                .extend(self.state.selected.iter().cloned());
            if self
                .state
                .selected
                .iter()
                .all(|id| Some(id) == self.state.current.as_ref())
                && let Some(item) = self.state.current_item()
                && let Some(index) = self
                    .state
                    .visible
                    .iter()
                    .position(|i| self.state.items[*i].id == item.id)
                && let Some(next) = self.state.visible.get(index + 1)
            {
                self.full_overrides
                    .insert(self.state.items[*next].id.clone());
            }
        }
        self.state.transform.set_zoom(1.);
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let settings = self.service.cache.settings();
        let gpu_mib = if settings.gpu_mib == 0 {
            256
        } else {
            settings.gpu_mib
        };
        self.presenter.configure_gpu_limit(
            (gpu_mib * 1024 * 1024).min(self.service.cache.memory.usage().limit / 2),
        );
        self.presenter.poll(ctx);
        self.presenter
            .set_pressure(self.service.cache.under_pressure());
        if self.service.cache.under_pressure() {
            self.trim_images(true);
        }
        let rejected = self.service.cache.memory.usage().rejected;
        if rejected > self.rejected_admissions {
            self.trim_images(true);
            self.rejected_admissions = rejected;
        }
        if self.smoke {
            self.presenter.begin_capture();
        }
        if let Ok(result) = self.gpu_rx.try_recv() {
            match result {
                Ok(check) => {
                    self.gpu_passed = check.failures == 0 && check.display_failures == 0;
                    self.gpu_status = format!(
                        "GPU/CPU: {} campioni · errore max {:.2e} · {}",
                        check.samples,
                        check.max_error,
                        if self.gpu_passed {
                            "filtro e uscita verificati"
                        } else {
                            "fuori soglia"
                        }
                    );
                }
                Err(e) => self.gpu_status = format!("Diagnostica GPU non disponibile: {e}"),
            }
        }
        self.presenter
            .configure_compute(self.gpu_passed, settings.compute.code());
        while let Ok(event) = self.service.events.try_recv() {
            match event {
                Event::MemoryPressure => self.trim_images(true),
                Event::DecodeDeferred {
                    id,
                    request,
                    generation,
                } => {
                    if generation == self.generation {
                        self.pending_images.remove(&(id, request));
                        ctx.request_repaint_after(Duration::from_millis(25));
                    }
                }
                Event::Scanned {
                    items,
                    folder,
                    generation,
                    note,
                } if generation == self.generation => {
                    self.state.replace_items(items);
                    if let Some(path) = self.pending_selection.take()
                        && let Some(item) = self.state.items.iter().find(|i| i.path == path)
                    {
                        self.command(Command::Select {
                            id: item.id.clone(),
                            extend: false,
                        });
                        self.state.view = ViewMode::Preview;
                    }
                    self.folder = folder;
                    self.scanning = false;
                    self.status = note;
                }
                Event::Scanned { .. } => {}
                Event::Image {
                    id,
                    request,
                    generation,
                    result,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    let key = (id.clone(), request);
                    if let Some(started) = self.requested_at.remove(&key)
                        && self.promoted.get(&key).is_some_and(|p| p.visible())
                        && result.is_ok()
                    {
                        self.recent_latencies
                            .push_back(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
                        if self.recent_latencies.len() > 128 {
                            self.recent_latencies.pop_front();
                        }
                    }
                    self.pending_images.remove(&key);
                    if request.edge == 256 && self.rebuild.front().is_some_and(|i| i.id == id) {
                        self.rebuild.pop_front();
                    }
                    match *result {
                        Ok(decoded) => {
                            self.cache.insert(
                                (id, request),
                                CachedImage {
                                    digest: decoded.digest,
                                    info: decoded.info,
                                    histogram: decoded.prepared.histogram,
                                    pyramid: decoded.prepared.image,
                                    touched: self.frame_number,
                                    transport: decoded.transport,
                                    worker_pid: decoded.worker_pid,
                                },
                            );
                            self.trim_images(false);
                        }
                        Err(error) => {
                            self.errors.insert(format!("{id}:{request:?}"), error);
                        }
                    }
                }
                Event::Saved {
                    id,
                    annotation,
                    revision,
                    undo_available,
                } => {
                    self.state.dispatch(Command::Commit {
                        id,
                        annotation,
                        revision,
                    });
                    self.undo_available = undo_available;
                    self.status =
                        "Annotazioni salvate nella libreria · XMP non attivo in R0".into();
                }
                Event::SaveFailed { id, error } => {
                    self.state.dispatch(Command::Failed(id));
                    self.status = format!("Modifica NON salvata: {error}");
                }
                Event::Status(status) => {
                    self.scanning = false;
                    self.status = status;
                }
                Event::Fatal(error) => {
                    self.status = error;
                    self.scanning = false;
                    self.fatal = true;
                }
                Event::Stopped => {}
            }
        }
        if let Some(item) = self.state.current_item()
            && item.id != self.keyword_id
        {
            self.keyword_id = item.id.clone();
            self.keyword_text = item.annotation.keywords.join(", ");
        }
        if self.smoke {
            let screenshots = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| {
                        if let egui::Event::Screenshot {
                            user_data, image, ..
                        } = e
                        {
                            user_data
                                .data
                                .as_ref()
                                .and_then(|a| a.downcast_ref::<String>())
                                .map(|name| (name.clone(), image.clone()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            });
            for (name, img) in screenshots {
                let pixels: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                let path = self.root.join("reports").join(format!("{name}.png"));
                if image::save_buffer(
                    &path,
                    &pixels,
                    img.width() as u32,
                    img.height() as u32,
                    image::ColorType::Rgba8,
                )
                .is_ok()
                {
                    self.screenshots.insert(name);
                }
            }
        }
    }
    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            self.search_focus = true;
        }
        if ctx.text_edit_focused() {
            return;
        }
        let photo_context = ctx
            .memory(|m| m.focused())
            .is_none_or(|id| self.image_focus_ids.contains(&id));
        if !photo_context {
            return;
        }
        let command = ctx.input(|i| i.modifiers.command);
        let key = |key| ctx.input(|i| i.key_pressed(key));
        if command {
            if key(egui::Key::Z) {
                self.command(Command::Undo);
            }
            if key(egui::Key::Num0) {
                self.state.transform = ViewTransform::default();
            }
            if key(egui::Key::Num1) {
                self.full_for_current();
            }
            if key(egui::Key::Plus) || key(egui::Key::Equals) {
                self.state
                    .transform
                    .set_zoom(self.state.transform.zoom.unwrap_or(1.) * 1.25);
            }
            if key(egui::Key::Minus) {
                self.state
                    .transform
                    .set_zoom(self.state.transform.zoom.unwrap_or(1.) / 1.25);
            }
            return;
        }
        for (number, keycode) in [
            egui::Key::Num0,
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
        ]
        .into_iter()
        .enumerate()
        {
            if key(keycode) {
                self.command(Command::Rate(number as i8));
            }
        }
        for (keycode, label) in [
            (egui::Key::Num6, Label::Red),
            (egui::Key::Num7, Label::Yellow),
            (egui::Key::Num8, Label::Green),
            (egui::Key::Num9, Label::Blue),
        ] {
            if key(keycode) {
                self.command(Command::Label(label));
            }
        }
        if key(egui::Key::X) {
            self.command(Command::Rate(-1));
        }
        if key(egui::Key::G) {
            self.command(Command::SetView(ViewMode::Grid));
        }
        if key(egui::Key::E) || key(egui::Key::Space) {
            self.command(Command::SetView(ViewMode::Preview));
        }
        if key(egui::Key::C) {
            self.command(Command::SetView(ViewMode::Compare));
        }
        if key(egui::Key::Escape) {
            self.state.view = ViewMode::Grid;
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        if key(egui::Key::Z) {
            if self.state.transform.zoom.is_some() {
                self.state.transform = ViewTransform::default();
            } else {
                self.full_for_current();
            }
        }
        if key(egui::Key::ArrowRight) {
            self.command(Command::Move(1));
        }
        if key(egui::Key::ArrowLeft) {
            self.command(Command::Move(-1));
        }
        if key(egui::Key::ArrowDown) {
            self.command(Command::Move(if self.state.view == ViewMode::Grid {
                self.grid_columns
            } else {
                1
            }));
        }
        if key(egui::Key::ArrowUp) {
            self.command(Command::Move(if self.state.view == ViewMode::Grid {
                -self.grid_columns
            } else {
                -1
            }));
        }
        if key(egui::Key::I) {
            self.show_inspector = !self.show_inspector;
        }
        if key(egui::Key::T) {
            self.show_filmstrip = !self.show_filmstrip;
        }
        if key(egui::Key::F) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar")
            .exact_size(52.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(28))
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TrueRenderer").size(20.).strong());
                    ui.label(RichText::new("R0").small().color(AMBER));
                    ui.add_space(12.);
                    if ui
                        .button("Apri cartella…")
                        .on_hover_text(
                            "Apri immagini della cartella; su macOS il bundle usa decoder isolati.",
                        )
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.folder)
                            .pick_folder()
                    {
                        self.open_folder(path);
                    }
                    if ui.button("Apri file…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.folder)
                            .pick_file()
                    {
                        self.open_path(path);
                    }
                    if ui.button("↻").on_hover_text("Rileggi cartella").clicked() {
                        self.open_folder(self.folder.clone());
                    }
                    ui.separator();
                    for (mode, title) in [
                        (ViewMode::Grid, "Griglia"),
                        (ViewMode::Preview, "Anteprima"),
                        (ViewMode::Compare, "Confronto"),
                    ] {
                        if ui
                            .selectable_label(self.state.view == mode, title)
                            .clicked()
                        {
                            self.command(Command::SetView(mode));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("?")
                            .on_hover_text("Guida e stato del prototipo")
                            .clicked()
                        {
                            self.show_help = !self.show_help;
                        }
                        if ui.button("Impostazioni").clicked() {
                            self.cache_settings = self.service.cache.settings();
                            self.show_settings = true;
                        }
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.state.query)
                                .hint_text("Cerca nome o parola chiave")
                                .desired_width(225.),
                        );
                        if self.search_focus {
                            response.request_focus();
                            self.search_focus = false;
                        }
                        if response.changed() {
                            self.state.refilter();
                        }
                    });
                });
            });
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("navigation").default_size(190.).size_range(165.0..=250.0).frame(egui::Frame::new().fill(Color32::from_gray(28)).inner_margin(16)).show(ui,|ui|{
            section(ui,"LIBRERIA");
            if nav(ui,"Corpus di prova","12",self.folder==self.root.join("corpus")).clicked(){self.state.query.clear();self.state.minimum_rating=0;self.state.label_filter=None;self.state.rejected_only=false;self.open_folder(self.root.join("corpus"));}
            ui.add_space(20.);section(ui,"SELEZIONE");
            if nav(ui,"Tutte le immagini",&self.state.items.len().to_string(),self.state.minimum_rating==0&&!self.state.rejected_only).clicked(){self.state.minimum_rating=0;self.state.rejected_only=false;self.state.refilter();}
            if nav(ui,"Da conservare","≥ 1 ★",self.state.minimum_rating==1).clicked(){self.state.minimum_rating=1;self.state.rejected_only=false;self.state.refilter();}
            if nav(ui,"Cinque stelle","5 ★",self.state.minimum_rating==5).clicked(){self.state.minimum_rating=5;self.state.rejected_only=false;self.state.refilter();}
            if nav(ui,"Scartate","×",self.state.rejected_only).clicked(){self.state.rejected_only=true;self.state.minimum_rating=0;self.state.refilter();}
            ui.add_space(20.);section(ui,"FILTRI");
            ui.label(RichText::new("Valutazione minima").color(MUTED));
            if ui.add(egui::Slider::new(&mut self.state.minimum_rating,0..=5).suffix(" ★")).changed(){self.state.rejected_only=false;self.state.refilter();}
            ui.add_space(6.);let previous=self.state.label_filter;
            egui::ComboBox::from_id_salt("label_filter").selected_text(self.state.label_filter.map(Label::text).unwrap_or("Tutte le etichette")).width(ui.available_width()).show_ui(ui,|ui|{
                ui.selectable_value(&mut self.state.label_filter,None,"Tutte le etichette");
                for label in Label::ALL{ui.selectable_value(&mut self.state.label_filter,Some(label),label.text());}
            });
            if previous!=self.state.label_filter{self.state.refilter();}
            if ui.small_button("Azzera filtri").clicked(){self.state.query.clear();self.state.minimum_rating=0;self.state.rejected_only=false;self.state.label_filter=None;self.state.refilter();}
            ui.add_space(24.);section(ui,"DATI LOCALI");
            if ui.add_enabled(self.undo_available&&self.state.pending.is_empty(),egui::Button::new("Annulla modifica")).on_hover_text("Cmd/Ctrl + Z · crea una nuova revisione").clicked(){self.command(Command::Undo);}
            if ui.button("Crea backup").clicked(){self.request(Request::Backup);}
            if ui.button("Esporta annotazioni…").on_hover_text("JSON con annotazioni e percorsi locali").clicked() && let Some(path)=rfd::FileDialog::new().set_file_name("TrueRenderer-annotazioni.json").add_filter("JSON",&["json"]).save_file(){self.request(Request::Export(path));}
            ui.add_space(24.);section(ui,"MOTORE");
            ui.label(RichText::new("CPU · fp32").monospace());
            ui.label(RichText::new("SDR / sRGB di sistema").small().color(MUTED));
            ui.label(RichText::new("Anteprima di sviluppo").small().color(AMBER));
            ui.add_space(8.);ui.label(RichText::new("Gli originali rimangono intatti. Le annotazioni sono nella libreria locale.").small().color(MUTED));
        });
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .default_size(290.)
            .size_range(250.0..=370.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(28))
                    .inner_margin(16),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    section(ui, "ISPEZIONE");
                    let Some(item) = self.state.current_item().cloned() else {
                        ui.label(RichText::new("Seleziona un'immagine").color(MUTED));
                        return;
                    };
                    let edge = (ui.available_width() * ui.ctx().pixels_per_point()).ceil() as u32;
                    let edge = edge.next_power_of_two().min(4096);
                    self.ensure_image(&item, edge);
                    let key = self.image_key(&item, edge);
                    let info = self.cache.get(&key).map(|c| c.info.clone());
                    if let Some(cached) = self.cache.get(&key) {
                        let size = Vec2::new(
                            ui.available_width(),
                            ui.available_width() * cached.info.height as f32
                                / cached.info.width as f32,
                        );
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        tr_render::presenter::fitted(
                            &mut self.presenter,
                            ui,
                            format!("inspector:{}", item.id),
                            &cached.pyramid,
                            rect,
                        );
                        ui.add_space(16.);
                        tr_render::histogram(ui, &cached.histogram);
                        ui.label(
                            RichText::new(format!(
                                "Istogramma del livello {} · uscita sRGB composita",
                                cached.pyramid.base_level()
                            ))
                            .small()
                            .color(MUTED),
                        );
                    }
                    ui.add_space(12.);
                    ui.label(RichText::new(&item.name).strong());
                    ui.label(
                        RichText::new(if item.approved {
                            "ANTEPRIMA"
                        } else {
                            "ANTEPRIMA NON DISPONIBILE"
                        })
                        .small()
                        .color(AMBER),
                    );
                    if let Some(error) = self.errors.get(&item.id) {
                        ui.colored_label(AMBER, error);
                    }
                    ui.add_space(12.);
                    section(ui, "FILE");
                    field(
                        ui,
                        "Dimensioni",
                        &info
                            .as_ref()
                            .map(|i| format!("{} × {} px", i.source_width, i.source_height))
                            .unwrap_or_else(|| "Non decodificato".into()),
                    );
                    field(
                        ui,
                        "Formato",
                        &info
                            .as_ref()
                            .map(|i| {
                                if i.native_bits == 0 {
                                    format!("{} · profondità non dichiarata", i.format)
                                } else {
                                    format!("{} · {} bit/canale", i.format, i.native_bits)
                                }
                            })
                            .unwrap_or_else(|| "—".into()),
                    );
                    field(ui, "Dimensione", &human_bytes(item.bytes));
                    ui.add_space(12.);
                    section(ui, "VALUTAZIONE");
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(item.annotation.rating == 0, "0")
                            .clicked()
                        {
                            self.command(Command::Rate(0));
                        }
                        for r in 1..=5 {
                            if ui
                                .selectable_label(item.annotation.rating >= r, "★")
                                .on_hover_text(format!("{r} stelle · tasto {r}"))
                                .clicked()
                            {
                                self.command(Command::Rate(r));
                            }
                        }
                        if ui
                            .selectable_label(item.annotation.rating == -1, "×")
                            .on_hover_text("Scarta · X")
                            .clicked()
                        {
                            self.command(Command::Rate(-1));
                        }
                    });
                    let mut label = item.annotation.label;
                    egui::ComboBox::from_id_salt("annotation_label")
                        .selected_text(format!("Etichetta: {}", label.text()))
                        .show_ui(ui, |ui| {
                            for v in Label::ALL {
                                ui.selectable_value(&mut label, v, v.text());
                            }
                        });
                    if label != item.annotation.label {
                        self.command(Command::Label(label));
                    }
                    ui.add_space(12.);
                    section(ui, "PAROLE CHIAVE");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.keyword_text)
                            .hint_text("paesaggio, studio, colore")
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    if ui
                        .add_enabled(
                            !self.state.pending.contains(&item.id),
                            egui::Button::new("Salva parole chiave"),
                        )
                        .clicked()
                    {
                        let mut annotation = item.annotation.clone();
                        match annotation.set_keywords(&self.keyword_text) {
                            Ok(()) => self.command(Command::Keywords(self.keyword_text.clone())),
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                    ui.label(
                        RichText::new(if self.state.pending.contains(&item.id) {
                            "Salvataggio in corso…"
                        } else {
                            "Solo libreria · XMP non attivo"
                        })
                        .small()
                        .color(MUTED),
                    );
                    ui.add_space(16.);
                    section(ui, "PROVENIENZA DEL RENDER");
                    field(
                        ui,
                        "Ingresso",
                        info.as_ref()
                            .map(|i| i.input_color.as_str())
                            .unwrap_or("Non determinato"),
                    );
                    field(ui, "Lavoro", "Rec.2020 lineare · fp32");
                    field(ui, "Alpha", "Premoltiplicata in luce lineare");
                    field(ui, "Uscita", "sRGB 8 bit · clamp dichiarato");
                    field(ui, "Display", "Contratto da qualificare");
                    if let Some(cached) = self.cache.get(&key) {
                        field(ui, "Isolamento", cached.transport);
                        field(ui, "SHA-256", &cached.digest);
                    }
                    if let Some(info) = info {
                        field(ui, "Decoder", &info.decoder);
                        field(ui, "Orientamento", &info.orientation);
                        field(ui, "Decodifica", &info.filter);
                        field(ui, "Presentazione", "Lineare · Lanczos3 / Mitchell");
                        field(ui, "Alpha", "Area / triangolare");
                    }
                    ui.add_space(4.);
                    ui.label(
                        RichText::new("Standard e Riferimento richiedono le prove R1.")
                            .small()
                            .color(MUTED),
                    );
                    if let Some(sample) = &self.sample {
                        ui.add_space(12.);
                        section(ui, "CAMPIONE DEL VIEWPORT");
                        ui.label(RichText::new(&self.sample_source).small().color(MUTED));
                        ui.monospace(format!("x {:5}   y {:5}", sample.x, sample.y));
                        ui.monospace(format!(
                            "RGB8  {} / {} / {}",
                            sample.display[0], sample.display[1], sample.display[2]
                        ));
                        ui.monospace(format!(
                            "lin R {:.5}\nlin G {:.5}\nlin B {:.5}\nalpha {:.5}",
                            sample.working[0],
                            sample.working[1],
                            sample.working[2],
                            sample.working[3]
                        ));
                    }
                });
            });
    }
    fn thumbnail(&mut self, ui: &mut egui::Ui, item: &Item, size: Vec2, show_name: bool) {
        let edge = ((size.x - 20.).max(1.) * ui.ctx().pixels_per_point()).ceil() as u32;
        let edge = edge.next_power_of_two().min(4096);
        self.ensure_image(item, edge);
        let key = self.image_key(item, edge);
        let selected = self.state.selected.contains(&item.id);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        self.image_focus_ids.insert(response.id);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                &item.name,
            )
        });
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            5,
            if selected {
                Color32::from_gray(48)
            } else {
                SURFACE
            },
        );
        if selected || response.has_focus() {
            painter.rect_stroke(
                rect,
                5,
                egui::Stroke::new(1.5, TEXT),
                egui::StrokeKind::Inside,
            );
        }
        let bottom = if show_name { 47. } else { 8. };
        let area = egui::Rect::from_min_max(
            rect.min + Vec2::splat(10.),
            egui::pos2(rect.right() - 10., rect.bottom() - bottom),
        );
        if let Some(cache) = self.cache.get(&key) {
            tr_render::presenter::fitted(
                &mut self.presenter,
                ui,
                format!("thumbnail:{}:{show_name}", item.id),
                &cache.pyramid,
                area,
            );
        } else {
            let text = if self
                .errors
                .contains_key(&format!("{}:{:?}", item.id, key.1))
            {
                "Errore di lettura"
            } else if !item.approved {
                "Anteprima non abilitata"
            } else {
                "Caricamento…"
            };
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(12.),
                MUTED,
            );
        }
        if show_name {
            let title = item.name.trim_end_matches(".png").replace('_', " ");
            let title = if title.chars().count() > 29 {
                format!("{}…", title.chars().take(27).collect::<String>())
            } else {
                title
            };
            painter.text(
                egui::pos2(rect.left() + 12., rect.bottom() - 34.),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(12.),
                TEXT,
            );
            let stars = if item.annotation.rating == -1 {
                "Scartata".into()
            } else {
                format!(
                    "{}{}",
                    "★".repeat(item.annotation.rating.max(0) as usize),
                    "·".repeat((5 - item.annotation.rating.max(0)) as usize)
                )
            };
            painter.text(
                egui::pos2(rect.left() + 12., rect.bottom() - 15.),
                egui::Align2::LEFT_CENTER,
                stars,
                egui::FontId::proportional(12.),
                MUTED,
            );
            if item.annotation.label != Label::None {
                painter.text(
                    egui::pos2(rect.right() - 12., rect.bottom() - 15.),
                    egui::Align2::RIGHT_CENTER,
                    item.annotation.label.text(),
                    egui::FontId::proportional(10.),
                    MUTED,
                );
            }
        }
        if response.clicked() {
            self.command(Command::Select {
                id: item.id.clone(),
                extend: ui.input(|i| i.modifiers.command || i.modifiers.shift),
            });
        }
        if response.double_clicked() {
            self.command(Command::SetView(ViewMode::Preview));
        }
        response.on_hover_text(self.errors.get(&item.id).cloned().unwrap_or_else(|| {
            format!(
                "{}\n{} · {}",
                item.name,
                human_bytes(item.bytes),
                if item.approved {
                    "Anteprima disponibile · dettagli del decoder in Ispezione"
                } else {
                    "Decoder non disponibile o file oltre quota"
                }
            )
        }));
    }
    fn grid(&mut self, ui: &mut egui::Ui) {
        let columns = ((ui.available_width() + 12.) / (self.cell_size + 12.))
            .floor()
            .max(1.) as usize;
        self.grid_columns = columns as i32;
        let size = Vec2::new(
            (ui.available_width() - (columns - 1) as f32 * 12.) / columns as f32,
            self.cell_size * 0.70 + 57.,
        );
        let count = self.state.visible.len();
        let rows = count.div_ceil(columns);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, size.y, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.;
                        for column in 0..columns {
                            let idx = row * columns + column;
                            if idx >= count {
                                break;
                            }
                            let item = self.state.items[self.state.visible[idx]].clone();
                            self.thumbnail(ui, &item, size, true);
                        }
                    });
                }
            });
    }
    fn preview(&mut self, ui: &mut egui::Ui) {
        let Some(item) = self.state.current_item().cloned() else {
            return;
        };
        let mut quality = self.service.cache.settings().quality;
        ui.horizontal(|ui| {
            ui.label("Qualità anteprima");
            ui.selectable_value(&mut quality, PreviewQuality::Standard, "Standard");
            ui.selectable_value(&mut quality, PreviewQuality::Full, "Piena");
            if ui.button("Qualità piena per questa foto").clicked() {
                self.full_overrides.insert(item.id.clone());
            }
            if self.full_overrides.contains(&item.id) {
                ui.label("Override di sessione · il cambio globale lo rimuove");
            }
        });
        self.set_quality(quality);
        ui.horizontal(|ui| {
            if ui.button("Adatta").clicked() {
                self.state.transform = ViewTransform::default();
            }
            if ui
                .button("1:1")
                .on_hover_text("Un pixel sorgente per pixel fisico dello schermo")
                .clicked()
            {
                self.full_for_current();
            }
            if ui.button("−").clicked() {
                self.state
                    .transform
                    .set_zoom(self.state.transform.zoom.unwrap_or(1.) / 1.25);
            }
            if ui.button("+").clicked() {
                self.state
                    .transform
                    .set_zoom(self.state.transform.zoom.unwrap_or(1.) * 1.25);
            }
            ui.label(
                RichText::new(
                    self.state
                        .transform
                        .zoom
                        .map(|z| format!("{:.0}%", z * 100.))
                        .unwrap_or("Adatta alla finestra".into()),
                )
                .monospace()
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("ANTEPRIMA").small().color(AMBER));
            });
        });
        ui.add_space(8.);
        if self.show_filmstrip {
            egui::Panel::bottom("filmstrip")
                .exact_size(110.)
                .frame(egui::Frame::new().inner_margin(8))
                .show(ui, |ui| {
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let visible = self.state.visible.clone();
                            let pos = visible
                                .iter()
                                .position(|i| self.state.items[*i].id == item.id)
                                .unwrap_or(0);
                            for i in visible.into_iter().skip(pos.saturating_sub(4)).take(12) {
                                let thumb = self.state.items[i].clone();
                                self.thumbnail(ui, &thumb, Vec2::new(114., 84.), false);
                            }
                        });
                    });
                });
        }
        if self.state.view == ViewMode::Compare {
            let second = self
                .state
                .selected
                .iter()
                .find(|id| **id != item.id)
                .and_then(|id| self.state.items.iter().find(|i| &i.id == id))
                .cloned()
                .or_else(|| {
                    self.state
                        .visible
                        .iter()
                        .position(|i| self.state.items[*i].id == item.id)
                        .and_then(|p| self.state.visible.get(p + 1))
                        .map(|i| self.state.items[*i].clone())
                });
            if let Some(second) = second {
                let bottom = ui.available_rect_before_wrap().bottom();
                ui.columns(2, |columns| {
                    for column in columns.iter_mut() {
                        column.set_max_height((bottom - column.cursor().min.y).max(20.));
                    }
                    columns[0].label(RichText::new(format!("A · {}", item.name)).small());
                    self.paint_view(&mut columns[0], &item, "A");
                    columns[1].label(RichText::new(format!("B · {}", second.name)).small());
                    self.paint_view(&mut columns[1], &second, "B");
                });
            } else {
                ui.label("Seleziona due immagini con Cmd/Ctrl + clic nella griglia.");
            }
        } else {
            self.paint_view(ui, &item, "single");
        }
    }
    fn paint_view(&mut self, ui: &mut egui::Ui, item: &Item, id: &str) {
        let fitted_edge = (ui.available_width().max(ui.available_height())
            * ui.ctx().pixels_per_point())
        .ceil()
        .clamp(1., 4096.) as u32;
        self.viewer_prefetch_edge = self.viewer_prefetch_edge.max(fitted_edge);
        let edge = if self.state.transform.zoom.is_none() {
            fitted_edge
        } else {
            0
        };
        self.ensure_image_priority(
            item,
            edge,
            if self.cache.keys().any(|(id, _)| id == &item.id) {
                PreviewPriority::Refinement
            } else {
                PreviewPriority::Immediate
            },
        );
        let key = self.image_key(item, edge);
        let fallback = self
            .cache
            .iter()
            .filter(|((id, _), _)| id == &item.id)
            .max_by_key(|(_, c)| c.pyramid.source().width)
            .map(|(key, _)| key.clone());
        let completeness = if self.cache.contains_key(&key) {
            tr_core::preview::Completeness::Complete
        } else {
            tr_core::preview::Completeness::Refining
        };
        let selected = if completeness == tr_core::preview::Completeness::Complete {
            Some(key.clone())
        } else {
            fallback
        };
        if let Some(c) = selected.as_ref().and_then(|key| self.cache.get_mut(key)) {
            c.touched = self.frame_number;
        }
        if let Some(c) = selected.as_ref().and_then(|key| self.cache.get(key)) {
            if completeness == tr_core::preview::Completeness::Refining {
                ui.label(
                    if self.state.transform.zoom == Some(1.)
                        && key.1.quality == PreviewQuality::Full
                    {
                        "Preparazione dettaglio 1:1…"
                    } else {
                        "Raffinamento anteprima…"
                    },
                );
            } else {
                ui.label(key.1.quality.label());
                if key.1.quality == PreviewQuality::Standard && c.pyramid.base_level() > 0 {
                    ui.label(format!(
                        "Dettaglio limitato a {} × {} pixel",
                        c.pyramid.source().width,
                        c.pyramid.source().height
                    ));
                }
            }
            let lane = format!("view:{id}:{}:{}", item.id, c.digest);
            let (response, sample) = tr_render::viewport(
                ui,
                &mut self.presenter,
                &c.pyramid,
                &mut self.state.transform,
                &lane,
            );
            self.image_focus_ids.insert(response.id);
            if sample.is_some() {
                self.sample_source = format!("Livello {} · {}", c.pyramid.base_level(), item.name);
                self.sample = sample;
            }
        } else {
            egui::Frame::new().fill(Color32::from_gray(119)).show(ui,|ui|{
                ui.set_min_size(ui.available_size());ui.centered_and_justified(|ui|{
                    ui.label(self.errors.get(&format!("{}:{:?}", item.id, key.1)).map(String::as_str).unwrap_or(if item.approved{"Preparazione dell'immagine…"}else{"Aprire il bundle macOS con decoder XPC.\nLimite: 256 MiB per file, 64 Mi pixel."}));
                });
            });
        }
    }
    fn footer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .exact_size(39.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(28))
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} immagini · {} selezionate",
                            self.state.visible.len(),
                            self.state.selected.len()
                        ))
                        .small(),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new("sRGB > lineare Rec.2020 > sRGB")
                            .small()
                            .color(MUTED),
                    );
                    if !self.pending_images.is_empty() {
                        ui.label(
                            RichText::new(format!("{} in coda", self.pending_images.len()))
                                .small()
                                .color(AMBER),
                        );
                    }
                    if !self.state.pending.is_empty() {
                        ui.label(RichText::new("Salvataggio…").small().color(AMBER));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.cell_size, 150.0..=300.0).show_value(false),
                        );
                        ui.label(RichText::new("Miniature").small().color(MUTED));
                    });
                });
            });
    }
    fn help(&mut self, ctx: &egui::Context) {
        egui::Window::new("TrueRenderer · guida e stato").open(&mut self.show_help).default_width(620.).show(ctx,|ui|{
            ui.heading("Un'immagine, una resa tracciabile.");
            ui.label(format!("Prototipo R0 · {} · 6 settembre 2026", env!("CARGO_PKG_VERSION")));ui.separator();
            ui.label("Disponibile: corpus PNG 8/16 bit, griglia, anteprima, confronto a due, zoom fisico 1:1, campione al puntatore, rating, etichette, parole chiave, ricerca, undo e backup locali.");
            ui.add_space(8.);ui.label("Il bundle macOS apre JPEG, PNG, TIFF, RAW supportati da Apple, GIF, BMP, HEIC/HEIF e WebP in servizi XPC isolati. I RAW sono sviluppati con la ricetta Apple TR-linear-v1; il supporto dipende dalla fotocamera e dal sistema. Massimo 256 MiB e 64 Mi pixel. Per TIFF/GIF multipagina si mostra la prima pagina o fotogramma. Il percorso su pipe resta limitato al corpus.");
            ui.add_space(8.);ui.label("Restano da qualificare: XPC/App Sandbox e Windows, ICC/Little CMS, presentazione sul monitor, filtri e CPU/GPU, accessibilità e prestazioni. JPEG/TIFF, RAW, XMP e gigapixel seguono la roadmap. Il badge rimane Anteprima.");
            ui.add_space(8.);ui.monospace(format!("GPU: {}\nSuperficie: {}\nSQLite: {}",self.adapter,self.surface,tr_store::sqlite_version()));
            ui.label(&self.gpu_status);
            ui.separator();
            for (key,action) in [("0–5 / X","Valuta / scarta"),("6–9","Etichette rosso, giallo, verde, blu"),("G / E / C","Griglia / anteprima / confronto"),("Z / Cmd+1","Adatta o pixel fisici 1:1"),("Frecce / trascina / rotella","Naviga / pan / zoom"),("Cmd+F / Cmd+Z","Ricerca / annulla modifica"),("I / T / F / Esc","Pannello / miniature / schermo intero / griglia")]{field(ui,key,action);}
            ui.label(RichText::new("Su Windows usare Ctrl al posto di Cmd. Le scorciatoie non agiscono mentre scrivi in un campo.").small().color(MUTED));
            ui.add_space(8.);ui.label(format!("Progetto e piano: {}",self.root.display()));
        });
    }
    fn capture_screenshot(&self, ctx: &egui::Context, name: &str) {
        let ppp = ctx.pixels_per_point();
        let records: Vec<_> = self.presenter.captures().iter().filter_map(|c| {
            let ((id,_), _) = self.cache.iter().find(|(_,cached)| cached.pyramid.id() == c.source)?;
            let item = self.state.items.iter().find(|i| &i.id == id)?;
            if item.name != "04_Frequenze_radiali.png" || !c.clip.contains_rect(c.rect) { return None; }
            Some(serde_json::json!({"source":item.name,"rect_physical":[c.rect.min.x*ppp,c.rect.min.y*ppp,c.rect.max.x*ppp,c.rect.max.y*ppp],"size":c.region.size,"origin":c.region.origin,"step":c.region.step,"compute":c.compute}))
        }).collect();
        let _ = std::fs::write(
            self.root
                .join("reports")
                .join(format!("{name}-sampling.json")),
            serde_json::to_vec_pretty(&records).unwrap(),
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
            name.to_string(),
        )));
    }
    fn formats_smoke_tick(&mut self, ctx: &egui::Context) {
        let elapsed = self.started.elapsed().as_secs_f32();
        if self.smoke_stage == 0
            && !self.state.items.is_empty()
            && !self.demand.is_empty()
            && self.demand.iter().all(|key| self.cache.contains_key(key))
            && self.presenter.is_idle()
        {
            self.capture_screenshot(ctx, "10-external-grid");
            self.smoke_stage = 1;
        } else if self.smoke_stage == 1 && self.screenshots.contains("10-external-grid") {
            if let Some(item) = self.state.items.iter().find(|i| i.name.ends_with(".dng")) {
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                self.state.view = ViewMode::Preview;
                self.full_for_current();
                self.smoke_stage = 2;
            }
        } else if self.smoke_stage == 2 && self.presenter.is_idle() {
            self.capture_screenshot(ctx, "11-raw-full");
            self.smoke_stage = 3;
        } else if self.smoke_stage == 3 && self.screenshots.contains("11-raw-full") {
            if let Some(item) = self.state.items.iter().find(|i| i.name == "12mp-jpeg.jpg") {
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                self.full_for_current();
                self.smoke_stage = 4;
            }
        } else if self.smoke_stage == 4 && self.presenter.is_idle() {
            self.capture_screenshot(ctx, "12-jpeg12mp-1to1");
            self.smoke_stage = 5;
        } else if (self.smoke_stage == 5 && self.screenshots.len() == 3)
            || elapsed > 100.
            || self.fatal
            || !self.errors.is_empty()
        {
            let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":self.smoke_stage==5&&self.screenshots.len()==3&&!self.fatal&&self.errors.is_empty()&&!self.presenter.has_errors()&&self.gpu_passed,"images":self.state.items.len(),"decoded":self.cache.len(),"screenshots":self.screenshots,"decode_errors":self.errors,"presentation_errors":self.presenter.has_errors(),"seconds":elapsed,"adapter":self.adapter,"pixels_per_point":ctx.pixels_per_point(),"worker_transports":self.cache.values().map(|c|c.transport).collect::<std::collections::BTreeSet<_>>(),"scope":"generated external images, native grid, full DNG and 12 MP JPEG viewer; not camera-wide RAW qualification"});
            let _ = std::fs::write(
                self.root.join("reports/formats-smoke-macos.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
    fn settings_window(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.cache_action {
            match rx.try_recv() {
                Ok(result) => {
                    self.cache_action = None;
                    self.scanning = false;
                    self.status = match result {
                        Ok(()) => "Impostazioni/cache aggiornate".into(),
                        Err(e) => e,
                    };
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.cache_action = None;
                    self.scanning = false;
                    self.status = "Operazione cache interrotta".into();
                }
                _ => {}
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let mut action = 0;
        egui::Window::new("Preferenze · Anteprime e prestazioni").open(&mut self.show_settings).default_width(570.).resizable(false).vscroll(true).max_height((ctx.viewport_rect().height()-80.).max(250.)).show(ctx,|ui|{
            ui.label("Le impostazioni valgono per tutte le cartelle; la quota disco si applica a ciascuna cartella separatamente.");
            ui.horizontal(|ui| {
                ui.label("Qualità anteprima");
                ui.selectable_value(&mut self.cache_settings.quality, PreviewQuality::Standard, "Standard");
                ui.selectable_value(&mut self.cache_settings.quality, PreviewQuality::Full, "Piena");
            });
            let physical = self.service.cache.physical_mib;
            let mut automatic = self.cache_settings.memory_mib == 0;
            if ui.checkbox(&mut automatic, "Memoria automatica").changed() {
                self.cache_settings.memory_mib = if automatic {0} else {self.cache_settings.effective_memory_mib(physical).max(512)};
            }
            if !automatic { ui.add(egui::Slider::new(&mut self.cache_settings.memory_mib,512..=(physical*3/4).max(512)).text("Memoria richiesta (MiB)")); }
            ui.label(format!("Budget di ammissione effettivo: {} MiB",self.cache_settings.effective_memory_mib(physical)));
            let mut automatic = self.cache_settings.reusable_mib.is_none();
            if ui.checkbox(&mut automatic,"Cache RAM automatica").changed() { self.cache_settings.reusable_mib = if automatic { None } else { Some(0) }; }
            if let Some(value) = &mut self.cache_settings.reusable_mib { ui.add(egui::Slider::new(value,0..=self.service.cache.memory.usage().limit/(1024*1024)).text("Cache RAM riutilizzabile (MiB; 0 = solo viste)")); }
            let memory = self.service.cache.memory.usage();
            ui.label(format!("Memoria prenotata (base inclusa): {} · picco crediti: {}",human_bytes(memory.reserved),human_bytes(memory.peak)));
            if memory.reserved > memory.limit { ui.label("Riduzione memoria in corso"); }
            ui.label(format!("Stima di base app/worker/device: {} MiB",self.service.cache.baseline_bytes/(1024*1024)));
            ui.label("Il budget include stime dei decoder; non è un limite RSS imposto dal sistema.");
            ui.add(egui::Slider::new(&mut self.cache_settings.gpu_mib,0..=1024).text("Cache GPU (MiB; 0 = automatica)"));
            ui.horizontal(|ui| {
                ui.label("Profilo prestazioni");
                ui.selectable_value(&mut self.cache_settings.profile,crate::cache::PerformanceProfile::Performance,"Prestazioni");
                ui.selectable_value(&mut self.cache_settings.profile,crate::cache::PerformanceProfile::Balanced,"Bilanciato");
                ui.selectable_value(&mut self.cache_settings.profile,crate::cache::PerformanceProfile::Saver,"Risparmio");
            });
            ui.horizontal(|ui| {
                ui.label("Calcolo immagine");
                ui.selectable_value(&mut self.cache_settings.compute,ImageCompute::Automatic,"Automatico");
                ui.selectable_value(&mut self.cache_settings.compute,ImageCompute::Gpu,"GPU compatibile");
                ui.selectable_value(&mut self.cache_settings.compute,ImageCompute::Cpu,"CPU");
            });
            ui.label(&self.gpu_status);
            let compute=self.presenter.statistics();
            ui.label(format!("Frame elaborati: {} GPU · {} CPU · {} ripieghi",compute.gpu_frames,compute.cpu_frames,compute.fallbacks));
            if !compute.last_fallback.is_empty() {ui.label(format!("Ultimo ripiego CPU: {}",compute.last_fallback));}
            ui.label("Il decoder Apple usa il contesto CPU; Metal viene qualificato separatamente dal viewer.");
            ui.checkbox(&mut self.cache_settings.adapt_on_battery,"Riduci automaticamente il lavoro a batteria");
            ui.label(format!("Thread applicativi effettivi: {} · {}",self.service.cache.effective_threads(),if self.service.cache.on_battery() {"batteria"} else {"alimentazione esterna / non rilevata"}));
            ui.label(format!("Pressione memoria OS: {}", match self.service.cache.stats().memory_pressure {
                Some(tr_platform::MemoryPressure::Normal) => "normale",
                Some(tr_platform::MemoryPressure::Warning) => "elevata · lavoro anticipato sospeso",
                Some(tr_platform::MemoryPressure::Critical) => "critica · cache riutilizzabili in rilascio",
                None => "adattatore non disponibile",
            }));
            ui.add(egui::Slider::new(&mut self.cache_settings.cpu_threads,0..=std::thread::available_parallelism().map_or(1,usize::from)).text("Thread CPU (0 = automatici)"));
            ui.horizontal(|ui| {
                ui.label("Precaricamento");
                ui.selectable_value(&mut self.cache_settings.prefetch,crate::cache::Prefetch::Disabled,"Disattivato");
                ui.selectable_value(&mut self.cache_settings.prefetch,crate::cache::Prefetch::Automatic,"Automatico");
                ui.selectable_value(&mut self.cache_settings.prefetch,crate::cache::Prefetch::Extended,"Esteso");
            });
            ui.checkbox(&mut self.preparation_paused,"Pausa preparazione in background");
            ui.horizontal(|ui| {
                if ui.add_enabled(self.rebuild.is_empty() && !self.scanning,egui::Button::new("Ricostruisci anteprime della cartella")).clicked() {
                    self.rebuild=self.state.items.iter().filter(|i| i.approved).cloned().collect();
                    self.rebuild_total=self.rebuild.len();
                }
                if !self.rebuild.is_empty() && ui.button("Annulla preparazione").clicked() {self.rebuild.clear(); self.rebuild_total=0;}
            });
            if self.rebuild_total>0 { ui.label(format!("Anteprime elaborate: {} / {}",self.rebuild_total-self.rebuild.len(),self.rebuild_total)); }
            ui.separator();
            ui.checkbox(&mut self.cache_settings.enabled,"Abilita cache su disco nella cartella delle immagini");
            ui.add(egui::Slider::new(&mut self.cache_settings.disk_mib,64..=65536).logarithmic(true).text("Quota per cartella (MiB)"));
            ui.add(egui::Slider::new(&mut self.cache_settings.temporary_mib,16..=2048).logarithmic(true).text("Temporanei (MiB)"));
            ui.add(egui::Slider::new(&mut self.cache_settings.unused_days,1..=3650).logarithmic(true).text("Scadenza senza utilizzo (giorni)"));
            ui.add(egui::Slider::new(&mut self.cache_settings.free_mib,0..=65536).logarithmic(true).text("Spazio libero da riservare (MiB)"));
            ui.label("I temporanei rientrano nella quota disco. Le immagini troppo grandi per la cache restano visualizzabili in RAM. I file meno usati vengono rimossi per rispettare la quota.");
            ui.separator();
            ui.label("Cartella corrente:");
            ui.add(egui::Label::new(self.folder.join(crate::cache::NAME).display().to_string()).wrap());
            let stats=self.service.cache.stats();
            if stats.folder==self.folder.display().to_string() {
                ui.label(format!("Cache: {} in {} file · temporanei inattivi: {}",human_bytes(stats.bytes),stats.entries,human_bytes(stats.temporary_bytes)));
            }
            ui.label(format!("Sessione: {} riusi da disco · {} mancate corrispondenze · {} scritture",stats.hits,stats.misses,stats.writes));
            ui.add(egui::Label::new(&stats.message).wrap());
            ui.label("entries contiene i render fp32 senza perdita; tmp contiene le scritture in corso. I temporanei abbandonati vengono rimossi alla successiva apertura. Originali, annotazioni e backup sono separati.");
            ui.horizontal(|ui|{
                let ready=self.cache_action.is_none() && !self.scanning;
                if ui.add_enabled(ready,egui::Button::new("Applica e salva")).clicked(){action=1;}
                if ui.add_enabled(ready,egui::Button::new("Svuota cache cartella")).clicked(){action=2;}
            });
            if self.cache_action.is_some(){ui.label("Aggiornamento cache…");}
        });
        if action > 0 {
            if action == 1 && self.cache_settings.quality != self.service.cache.settings().quality {
                self.full_overrides.clear();
            }
            self.errors.clear();
            self.scanning = true;
            if action == 1 {
                self.service.cache.configure(self.cache_settings.clone());
            }
            let cache = self.service.cache.clone();
            let folder = self.folder.clone();
            let data = self.settings_data.clone();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            self.cache_action = Some(rx);
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<()> {
                    if action == 1 {
                        cache.save_current(&data)?;
                    }
                    let start = Instant::now();
                    loop {
                        match cache.maintain(&folder, action == 2) {
                            Err(e)
                                if e.downcast_ref::<std::io::Error>().is_some_and(|e| {
                                    e.kind() == std::io::ErrorKind::WouldBlock
                                }) && start.elapsed() < Duration::from_secs(3) =>
                            {
                                std::thread::sleep(Duration::from_millis(25))
                            }
                            result => return result,
                        }
                    }
                })()
                .map_err(|e| format!("Cache: {e:#}"));
                let _ = tx.send(result);
            });
        }
    }
    fn smoke_tick(&mut self, ctx: &egui::Context) {
        if !self.smoke {
            return;
        }
        if self.settings_smoke {
            if self.started.elapsed().as_secs_f32() > 1.
                && !self.scanning
                && self.cache_action.is_none()
                && self.gpu_status != "Diagnostica GPU in corso…"
                && self.presenter.is_idle()
                && !self.demand.is_empty()
                && self.demand.iter().all(|key| self.cache.contains_key(key))
            {
                if self.screenshots.contains("13-cache-settings") {
                    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":!self.fatal && self.errors.is_empty() && !self.presenter.has_errors() && self.gpu_passed,"settings":self.service.cache.settings(),"cache":self.service.cache.stats()});
                    let _ = std::fs::write(
                        self.root.join("reports/cache-settings-macos.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.capture_screenshot(ctx, "13-cache-settings");
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if self.external_smoke {
            if !self.presenter.is_idle() && self.started.elapsed().as_secs_f32() < 100. {
                ctx.request_repaint_after(Duration::from_millis(25));
                return;
            }
            self.formats_smoke_tick(ctx);
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        if (!self.presenter.is_idle()
            || self.demand.iter().any(|key| !self.cache.contains_key(key)))
            && elapsed < 55.
        {
            ctx.request_repaint_after(Duration::from_millis(25));
            return;
        }
        if self.smoke_stage == 0
            && !self.scanning
            && !self.demand.is_empty()
            && self.demand.iter().all(|key| self.cache.contains_key(key))
        {
            self.smoke_stage = 1;
            self.capture_screenshot(ctx, "01-grid");
        } else if self.smoke_stage == 1 && self.screenshots.contains("01-grid") {
            if let Some(item) = self.state.items.get(1) {
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
            }
            self.state.view = ViewMode::Preview;
            self.smoke_stage = 2;
        } else if self.smoke_stage == 2
            && self.state.current_item().is_some_and(|item| {
                self.demand
                    .iter()
                    .any(|key| key.0 == item.id && self.cache.contains_key(key))
            })
        {
            self.smoke_stage = 3;
            self.capture_screenshot(ctx, "02-preview");
        } else if self.smoke_stage == 3 && self.screenshots.contains("02-preview") {
            self.state.selected.clear();
            for index in [6, 7] {
                if let Some(item) = self.state.items.get(index) {
                    self.state.selected.insert(item.id.clone());
                }
            }
            self.state.current = self.state.items.get(6).map(|i| i.id.clone());
            self.state.view = ViewMode::Compare;
            self.smoke_stage = 4;
        } else if self.smoke_stage == 4
            && self.state.selected.iter().all(|id| {
                self.state
                    .items
                    .iter()
                    .find(|item| &item.id == id)
                    .is_some_and(|item| {
                        self.demand
                            .iter()
                            .any(|key| key.0 == item.id && self.cache.contains_key(key))
                    })
            })
        {
            self.smoke_stage = 5;
            self.capture_screenshot(ctx, "03-compare");
        } else if self.sampling_smoke
            && self.smoke_stage == 5
            && self.screenshots.contains("03-compare")
        {
            if let Some(item) = self
                .state
                .items
                .iter()
                .find(|i| i.name == "04_Frequenze_radiali.png")
            {
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
            }
            self.state.view = ViewMode::Preview;
            self.full_for_current();
            self.smoke_stage = 6;
        } else if self.sampling_smoke && [6, 8, 10, 12, 14].contains(&self.smoke_stage) {
            let name = match self.smoke_stage {
                6 => "04-radial-1to1",
                8 => "05-radial-fit",
                10 => "06-radial-37percent",
                12 => "07-grid-small",
                _ => "08-grid-large",
            };
            self.smoke_stage += 1;
            self.capture_screenshot(ctx, name);
        } else if self.sampling_smoke
            && self.smoke_stage == 7
            && self.screenshots.contains("04-radial-1to1")
        {
            self.state.transform = ViewTransform::default();
            self.smoke_stage = 8;
        } else if self.sampling_smoke
            && self.smoke_stage == 9
            && self.screenshots.contains("05-radial-fit")
        {
            self.state.transform.set_zoom(0.37);
            self.smoke_stage = 10;
        } else if self.sampling_smoke
            && self.smoke_stage == 11
            && self.screenshots.contains("06-radial-37percent")
        {
            self.state.view = ViewMode::Grid;
            self.cell_size = 132.;
            self.smoke_stage = 12;
        } else if self.sampling_smoke
            && self.smoke_stage == 13
            && self.screenshots.contains("07-grid-small")
        {
            self.cell_size = 288.;
            self.smoke_stage = 14;
        } else if (self.smoke_stage == (if self.sampling_smoke { 15 } else { 5 })
            && self.screenshots.len() == (if self.sampling_smoke { 8 } else { 3 }))
            || elapsed > 55.
            || self.fatal
        {
            let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":self.smoke_stage==(if self.sampling_smoke {15} else {5})&&self.screenshots.len()==(if self.sampling_smoke {8} else {3})&&!self.fatal&&self.errors.is_empty()&&!self.presenter.has_errors()&&self.gpu_passed,"sampling":tr_core::resample::VERSION,"presentation_errors":self.presenter.has_errors(),"pixels_per_point":ctx.pixels_per_point(),"adapter":self.adapter,"surface":self.surface,"gpu":self.gpu_status,"compute":self.presenter.statistics(),"sqlite":tr_store::sqlite_version(),"worker_pids":self.cache.values().filter_map(|c| c.worker_pid).collect::<std::collections::BTreeSet<_>>(),"worker_transports":self.cache.values().map(|c| c.transport).collect::<std::collections::BTreeSet<_>>(),"frames":self.frame_number,"elapsed_seconds":elapsed,"images":self.state.items.len(),"screenshots":self.screenshots,"decode_errors":self.errors,"status":self.status,"scope":"native macOS R0 corpus smoke; not color/display/sandbox qualification"});
            let _ = std::fs::write(
                self.root.join("reports/smoke-macos.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
impl eframe::App for TrueRenderer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_number += 1;
        self.demand.clear();
        self.primary_demand.clear();
        self.viewer_prefetch_edge = 0;
        self.demand_jobs.clear();
        self.poll(&ctx);
        self.keyboard(&ctx);
        self.image_focus_ids.clear();
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.pending.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.status = "Attendo il salvataggio delle annotazioni prima di chiudere…".into();
        }
        if self.closing && self.state.pending.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.toolbar(ui);
        self.footer(ui);
        self.sidebar(ui);
        if self.show_inspector {
            self.inspector(ui);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(20))
                    .inner_margin(20),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(if self.folder == self.root.join("corpus") {
                        "Corpus di prova"
                    } else {
                        self.folder
                            .file_name()
                            .and_then(|x| x.to_str())
                            .unwrap_or("Immagini")
                    });
                    ui.label(
                        RichText::new(format!("/ {} immagini", self.state.visible.len()))
                            .color(MUTED),
                    );
                });
                ui.label(RichText::new(&self.status).small().color(if self.fatal {
                    AMBER
                } else {
                    MUTED
                }));
                ui.add_space(14.);
                if self.scanning {
                    ui.label("Lettura dei file…");
                } else if self.state.visible.is_empty() {
                    ui.add_space(70.);
                    ui.vertical_centered(|ui| {
                        ui.heading("Nessuna immagine da mostrare");
                        ui.label("Apri una cartella oppure azzera i filtri.");
                        if ui.button("Apri il corpus di prova").clicked() {
                            self.state.query.clear();
                            self.state.minimum_rating = 0;
                            self.state.rejected_only = false;
                            self.state.label_filter = None;
                            self.open_folder(self.root.join("corpus"));
                        }
                    });
                } else {
                    match self.state.view {
                        ViewMode::Grid => self.grid(ui),
                        _ => self.preview(ui),
                    }
                }
            });
        self.help(&ctx);
        self.settings_window(&ctx);
        self.trim_images(false);
        self.smoke_tick(&ctx);
        self.background_demand(&ctx);
        self.flush_demand();
        if self.scanning || !self.pending_images.is_empty() || !self.state.pending.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        if let Some(path) = dropped.first() {
            self.open_path(path.clone());
        }
    }
    fn on_exit(&mut self) {
        let _ = self.service.high.try_send(Request::Shutdown);
    }
}
fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(11.).strong().color(MUTED));
    ui.add_space(4.);
}
fn field(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal_top(|ui| {
        ui.add_sized(
            [82., 16.],
            egui::Label::new(RichText::new(key).small().color(MUTED)),
        );
        ui.add(egui::Label::new(RichText::new(value).small()).wrap());
    });
}
fn nav(ui: &mut egui::Ui, title: &str, count: &str, selected: bool) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 31.],
        egui::Button::new(format!("{title}   {count}"))
            .selected(selected)
            .frame(selected),
    )
}
fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.)
    } else {
        format!("{:.0} KiB", bytes as f64 / 1024.)
    }
}
