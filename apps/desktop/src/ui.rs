use crate::i18n::{Language, localized_format};
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

mod editing;
mod explorer;
mod export;
mod loading;
pub(crate) mod navigation;
mod preferences;
pub(crate) mod raw_previews;
mod science;
mod style;
use preferences::SettingsPage;
use style::{AMBER, CANVAS, MUTED, PANEL, TEXT};

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
    pub navigation: bool,
    pub smoke: bool,
    pub sampling_smoke: bool,
    pub external_smoke: bool,
    pub settings_smoke: bool,
    pub open: Option<PathBuf>,
}
pub struct TrueRenderer {
    editing: editing::EditingUi,
    science: science::ScienceUi,
    photo_export: export::ExportUi,
    browser: explorer::Explorer,
    navigation_probe: Option<navigation::Probe>,
    state: State,
    service: Service,
    root: PathBuf,
    folder: PathBuf,
    generation: u64,
    cache: HashMap<(String, PreviewRequest), CachedImage>,
    presenter: tr_render::presenter::Presenter,
    pending_images: HashSet<(String, PreviewRequest)>,
    quality_overrides: HashMap<String, PreviewQuality>,
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
    settings_page: SettingsPage,
    settings_smoke: bool,
    raw_engine_smoke_results: Vec<serde_json::Value>,
    raw_engine_smoke_ready_at: Option<Instant>,
    raw_engine_smoke_capture_pending: bool,
    cache_settings: crate::cache::Settings,
    settings_data: PathBuf,
    preparation_paused: bool,
    rebuild: VecDeque<Item>,
    rebuild_total: usize,
    folder_load: loading::FolderLoad,
    cache_action: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    show_inspector: bool,
    show_filmstrip: bool,
    fullscreen: bool,
    adapter: String,
    surface: String,
    frame_number: u64,
    sample: Option<tr_render::Sample>,
    sample_item_id: Option<String>,
    sample_level: Option<u32>,
    sample_from_current_render: bool,
    smoke: bool,
    sampling_smoke: bool,
    external_smoke: bool,
    pending_selection: Option<PathBuf>,
    started: Instant,
    smoke_stage: u8,
    smoke_layout: Option<(u8, String, Instant)>,
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
    source_monitor: crate::source_monitor::Monitor,
    watched_sources: HashSet<String>,
    source_status: HashMap<String, String>,
    context: egui::Context,
}
impl TrueRenderer {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        root: PathBuf,
        data: PathBuf,
        worker: PathBuf,
        startup: Startup,
    ) -> Self {
        let root = root.canonicalize().unwrap_or(root);
        let Startup {
            navigation,
            smoke,
            sampling_smoke,
            external_smoke,
            settings_smoke,
            open,
        } = startup;
        let ctx = &cc.egui_ctx;
        style::apply(ctx);
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
                    r.surface_diagnostics.clone(),
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
                        report_root.join(if cfg!(windows) {
                            "var/preview-quality-gpu-windows.json"
                        } else {
                            "reports/preview-quality-gpu-macos.json"
                        }),
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
        let source_monitor = crate::source_monitor::Monitor::new(service.wake.clone());
        let mut app = Self {
            editing: Default::default(),
            science: Default::default(),
            photo_export: export::ExportUi::default(),
            browser: explorer::Explorer::new(&data),
            navigation_probe: navigation.then(navigation::Probe::new),
            state: State::default(),
            cache_settings: service.cache.settings(),
            settings_data: data,
            cache_action: None,
            preparation_paused: false,
            rebuild: VecDeque::new(),
            rebuild_total: 0,
            folder_load: loading::FolderLoad::default(),
            service,
            root,
            folder: folder.clone(),
            generation: 0,
            cache: HashMap::new(),
            presenter,
            pending_images: HashSet::new(),
            quality_overrides: HashMap::new(),
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
            settings_page: SettingsPage::Previews,
            settings_smoke,
            show_inspector: true,
            show_filmstrip: true,
            fullscreen: false,
            adapter,
            surface,
            frame_number: 0,
            sample: None,
            sample_item_id: None,
            sample_level: None,
            sample_from_current_render: false,
            smoke,
            sampling_smoke,
            external_smoke,
            pending_selection: None,
            started: Instant::now(),
            smoke_stage: 0,
            smoke_layout: None,
            raw_engine_smoke_results: vec![],
            raw_engine_smoke_ready_at: None,
            raw_engine_smoke_capture_pending: false,
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
            source_monitor,
            context: ctx.clone(),
            watched_sources: HashSet::new(),
            source_status: HashMap::new(),
        };
        if let Some(path) = open {
            app.open_path(path);
        } else {
            app.open_folder(folder);
        }
        if app.smoke && std::env::args().any(|arg| arg == "--raw-engine-scan-change") {
            // Exercise a settings change before the pending scan can be polled,
            // independent of how quickly the background thread lists the files.
            assert!(app.scanning, "Smoke requires a pending folder scan");
            app.cache_settings.raw_engine = tr_core::decoder::RawEngine::choices()
                .find(|engine| *engine != app.cache_settings.raw_engine)
                .expect("At least two RAW engines for this smoke");
            app.start_cache_action(true);
        }
        app
    }
    pub fn detach_graphics(&mut self) {
        // Drop the old presenter and join its encoder before creating a device.
        // CPU artifacts, pending saves, selection and undo history remain alive.
        self.presenter = tr_render::presenter::Presenter::new(
            egui::Context::default(),
            self.service.cache.memory.clone(),
            None,
        );
    }
    pub fn rebind_graphics(&mut self, cc: &eframe::CreationContext<'_>) {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx
            .set_style_of(egui::Theme::Dark, self.context.style_of(egui::Theme::Dark));
        self.context = cc.egui_ctx.clone();
        self.service.wake.rebind(cc.egui_ctx.clone());
        self.presenter = tr_render::presenter::Presenter::new(
            cc.egui_ctx.clone(),
            self.service.cache.memory.clone(),
            cc.wgpu_render_state.clone(),
        );
        // Old qualification messages must not qualify the newly created device.
        while self.gpu_rx.try_recv().is_ok() {}
        self.gpu_passed = false;
        if let Some(gpu) = &cc.wgpu_render_state {
            self.adapter = format!(
                "{} · {:?}",
                gpu.adapter.get_info().name,
                gpu.adapter.get_info().backend
            );
            self.surface = gpu.surface_diagnostics.clone();
            match tr_render::preview_compute::check(&gpu.device, &gpu.queue) {
                Ok(check) => {
                    self.gpu_passed = check.failures == 0 && check.display_failures == 0;
                    self.gpu_status = if self.gpu_passed {
                        "GPU ricreata e riverificata".into()
                    } else {
                        "GPU ricreata · calcolo CPU, verifica compute non superata".into()
                    };
                }
                Err(error) => self.gpu_status = format!("GPU ricreata · calcolo CPU: {error:#}"),
            }
        }
        self.presenter.configure_compute(
            self.gpu_passed,
            self.service.cache.settings().compute.code(),
        );
        self.status = "Dispositivo grafico ripristinato · sessione e annotazioni conservate".into();
        cc.egui_ctx.request_repaint();
    }
    pub fn graphics_test_ready(&self) -> bool {
        !self.scanning
            && !self.fatal
            && self.presenter.is_idle()
            && !self.demand.is_empty()
            && self.demand.iter().all(|key| self.cache.contains_key(key))
            && self.state.pending.is_empty()
    }
    pub fn graphics_test_save(&mut self) -> i8 {
        let rating = if self
            .state
            .current_item()
            .is_some_and(|item| item.annotation.rating == 4)
        {
            3
        } else {
            4
        };
        self.command(Command::Rate(rating));
        rating
    }
    pub fn graphics_test_state(&self) -> serde_json::Value {
        serde_json::json!({"current":self.state.current,"selected":self.state.selected,
            "rating":self.state.current_item().map(|item| item.annotation.rating),
            "undo_available":self.undo_available,"gpu_verified":self.gpu_passed,
            "pending":self.state.pending.len()})
    }
    fn open_path(&mut self, path: PathBuf) {
        self.navigate(path, Some(ViewMode::Preview));
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
        self.navigate(folder, None);
    }
    fn activate_folder(&mut self, folder: PathBuf) {
        self.commit_all_edits();
        let targeted_hidden = self.pending_selection.as_ref().is_some_and(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        });
        self.request(Request::ScanHidden(
            self.browser.model.show_hidden || targeted_hidden,
        ));
        self.rebuild.clear();
        self.rebuild_total = 0;
        self.begin_folder_loading();
        self.generation += 1;
        self.service
            .generation
            .store(self.generation, Ordering::Relaxed);
        self.cache.clear();
        self.watched_sources.clear();
        self.source_status.clear();
        self.source_monitor.watch(self.generation, vec![]);
        self.presenter.clear();
        self.pending_images.clear();
        self.quality_overrides.clear();
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
            generation: self.browser.scan,
        });
        self.browser.scan_running = self.scanning;
        self.status = "Lettura della cartella…".into();
    }
    fn command(&mut self, command: Command) {
        if matches!(&command, Command::Select { .. } | Command::Move(_)) {
            self.browser_user_selection();
        }
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
        if self.output_proof_for(&item.id) {
            return PreviewQuality::Full;
        }
        self.quality_overrides
            .get(&item.id)
            .copied()
            .unwrap_or_else(|| self.service.cache.settings().quality)
    }
    fn preview_request(&self, item: &Item, edge: u32) -> PreviewRequest {
        PreviewRequest {
            raw_engine: self
                .editing
                .entries
                .get(&item.id)
                .and_then(|entry| entry.loaded.as_ref())
                .filter(|saved| saved.generation > 0)
                .map_or(self.service.cache.settings().raw_engine, |saved| {
                    saved.recipe.raw_engine
                }),
            quality: self.quality(item),
            edge,
        }
    }
    fn image_key(&self, item: &Item, edge: u32) -> (String, PreviewRequest) {
        (item.id.clone(), self.preview_request(item, edge))
    }
    fn viewer_fallback_key(
        &self,
        item: &Item,
        request: PreviewRequest,
    ) -> Option<(String, PreviewRequest)> {
        self.cache
            .iter()
            .filter(|((id, cached_request), _)| {
                id == &item.id && cached_request.raw_engine == request.raw_engine
            })
            .max_by_key(|(_, cached)| cached.pyramid.source().width)
            .map(|(key, _)| key.clone())
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
        self.watched_sources.insert(item.id.clone());
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
        // Equivalent geometries share immediately. Smaller independent tails
        // are derived off the UI thread, with their own credits and histogram.
        let compatible = self
            .cache
            .iter()
            .filter(|((id, request), cached)| {
                id == &item.id
                    && request.raw_engine == key.1.raw_engine
                    && request.quality == key.1.quality
                    && cached.pyramid.sufficient_for(key.1)
            })
            .min_by_key(|(_, cached)| cached.pyramid.byte_len())
            .map(|(_, cached)| cached.clone());
        if let Some(mut cached) = compatible
            .clone()
            .filter(|cached| cached.pyramid.requested_base(key.1) == 0)
        {
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
            resident: compatible.map(|cached| crate::service::PreviewDecoded {
                digest: cached.digest,
                info: cached.info,
                prepared: tr_render::PreparedPreview {
                    image: cached.pyramid,
                    histogram: cached.histogram,
                },
                transport: cached.transport,
                worker_pid: cached.worker_pid,
            }),
            item: item.clone(),
            request: key.1,
            priority,
            generation: self.generation,
        });
    }
    fn background_demand(&mut self, ctx: &egui::Context) {
        if self.folder_preparation_demand(ctx) {
            return;
        }
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
        if self.navigation_changed.elapsed() < delay {
            if self.service.cache.settings().prefetch != crate::cache::Prefetch::Disabled {
                ctx.request_repaint_after(delay.saturating_sub(self.navigation_changed.elapsed()));
            }
            return;
        }
        if self.preparation_paused || self.scanning || self.service.cache.under_pressure() {
            return;
        }
        let settings = self.service.cache.settings();
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
        {
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
            if self.pending_images.contains(&key)
                || self
                    .demand_jobs
                    .iter()
                    .any(|j| j.item.id == item.id && j.request == key.1)
                || self.cache.contains_key(&key)
                || self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))
            {
                continue;
            }
            self.demand_jobs.push(crate::decode_pool::Job {
                resident: None,
                item,
                request: key.1,
                priority: if self.state.view == ViewMode::Grid {
                    PreviewPriority::AdjacentRows
                } else {
                    PreviewPriority::NeighborPreview
                },
                generation: self.generation,
            });
        }
    }
    fn flush_demand(&mut self) {
        let mut watched = self.watched_sources.clone();
        watched.extend(self.cache.keys().map(|(id, _)| id.clone()));
        self.source_monitor.watch(
            self.generation,
            self.state
                .items
                .iter()
                .filter(|item| watched.contains(&item.id))
                .map(|item| crate::source_monitor::Watch {
                    id: item.id.clone(),
                    path: item.path.clone(),
                    observation: item.observation.clone(),
                })
                .collect(),
        );
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
        self.prune_edit_previews(pressure);
    }
    /// Revoke old work and presentation while preserving the catalogue and selection.
    fn invalidate_raw_engine(&mut self) {
        self.generation += 1;
        self.service
            .generation
            .store(self.generation, Ordering::Release);
        self.cache.clear();
        self.prune_edit_previews(true);
        self.presenter.clear();
        self.pending_images.clear();
        self.demand.clear();
        self.last_demand.clear();
        self.demand_jobs.clear();
        self.promoted.clear();
        self.requested_at.clear();
        self.recent_latencies.clear();
        self.rebuild.clear();
        self.rebuild_total = 0;
        self.watched_sources.clear();
        self.source_monitor.watch(self.generation, vec![]);
        self.sample = None;
        self.sample_item_id = None;
        self.sample_level = None;
        self.sample_from_current_render = false;
        self.errors.clear();
        // Filesystem scan IDs are independent of decoder generations.
    }
    fn apply_settings(&mut self) {
        let old = self.service.cache.settings();
        let restart = old.raw_engine != self.cache_settings.raw_engine
            || old.quality != self.cache_settings.quality
            || old.folder_loading != self.cache_settings.folder_loading;
        if self.cache_settings.raw_engine != self.service.cache.settings().raw_engine {
            self.invalidate_raw_engine();
        }
        self.service.cache.configure(self.cache_settings.clone());
        if restart {
            self.begin_folder_loading();
            self.start_folder_preparation(true);
        }
    }
    fn set_quality(&mut self, quality: PreviewQuality) {
        let mut settings = self.service.cache.settings();
        if settings.quality == quality {
            return;
        }
        settings.quality = quality;
        self.quality_overrides.clear();
        self.errors.clear();
        // The toolbar changes only quality, not other unapplied preferences.
        self.cache_settings.quality = quality;
        self.service.cache.configure(settings.clone());
        self.begin_folder_loading();
        self.start_folder_preparation(true);
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
            self.quality_overrides
                .insert(item.id.clone(), PreviewQuality::Full);
        }
        if self.state.view == ViewMode::Compare {
            self.quality_overrides.extend(
                self.state
                    .selected
                    .iter()
                    .cloned()
                    .map(|id| (id, PreviewQuality::Full)),
            );
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
                self.quality_overrides
                    .insert(self.state.items[*next].id.clone(), PreviewQuality::Full);
            }
        }
        self.state.transform.set_zoom(1.);
    }
    fn apply_source_changes(&mut self, changes: Vec<crate::source_monitor::Change>) {
        let changed: HashSet<_> = changes.iter().map(|change| change.id.clone()).collect();
        let sources: HashSet<_> = self
            .cache
            .iter()
            .filter(|((id, _), _)| changed.contains(id))
            .map(|(_, cached)| cached.pyramid.id())
            .collect();
        // Invalidate the decode epoch before accepting any queued completions.
        // This is a source revision change, not a navigation/view generation.
        self.generation += 1;
        self.service
            .generation
            .store(self.generation, Ordering::Release);
        self.pending_images.clear();
        self.last_demand.clear();
        self.demand_jobs.clear();
        self.promoted.clear();
        self.requested_at.clear();
        self.cache.retain(|(id, _), _| !changed.contains(id));
        self.presenter.invalidate_sources(&sources);
        self.errors
            .retain(|key, _| !changed.iter().any(|id| key.starts_with(&format!("{id}:"))));
        self.sample = None;
        self.sample_item_id = None;
        self.sample_level = None;
        self.sample_from_current_render = false;
        for change in changes {
            let Some(item) = self
                .state
                .items
                .iter_mut()
                .find(|item| item.id == change.id)
            else {
                continue;
            };
            item.observation.clone_from(&change.observation);
            item.bytes = change.bytes;
            // External sources retain the broker's unverified-token contract.
            // The pipe corpus retains its pinned digest and never gains authority.
            if item.digest.starts_with("unverified:") && change.available {
                item.digest.clone_from(&change.observation);
            }
            item.approved = change.available
                && change.bytes <= tr_core::protocol::MAX_SOURCE as u64
                && (tr_platform::CorpusPolicy::default().approves(&item.digest)
                    || std::env::current_exe()
                        .ok()
                        .is_some_and(|p| tr_platform::external_decoding_available(&p)));
            self.source_status.insert(
                item.id.clone(),
                if change.available {
                    "Sorgente modificata · aggiornamento anteprima…".into()
                } else {
                    "Sorgente non disponibile · anteprima precedente rimossa".into()
                },
            );
        }
        // A queued batch may contain old Item clones.
        for queued in &mut self.rebuild {
            if let Some(item) = self.state.items.iter().find(|item| item.id == queued.id) {
                *queued = item.clone();
            }
        }
        self.status = "Sorgenti cambiate: anteprime invalidate; annotazioni conservate".into();
    }
    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_edit_preview();
        self.poll_browser(ctx);
        while let Ok((generation, changes)) = self.source_monitor.changes.try_recv() {
            if generation != self.generation {
                continue;
            }
            self.apply_source_changes(changes);
        }
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
            self.presenter.release_optional_frames();
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
                Event::Edit { id, result } => self.edit_result(id, result),
                Event::PhotoExport(result) => {
                    self.record_develop_export(&result);
                    self.export_result(result);
                }
                Event::ScientificSample { id, result } => self.science_result(id, result),
                Event::BrowserSessionSaved(result) => match result {
                    Ok(session) => self.browser.saved = session,
                    Err(error) => self.status = format!("Browser session: {error}"),
                },
                Event::Favorites(result) => {
                    self.browser.favorite_pending = false;
                    match result {
                        Ok(favorites) => self.browser.favorites = favorites,
                        Err(error) => self.status = format!("Preferiti: {error}"),
                    }
                }
                Event::ScanBatch { items, generation } => {
                    if generation == self.browser.scan {
                        self.state.reconcile(items, false);
                        self.select_pending_photo();
                    }
                }
                Event::ScanFailed { generation, error } => {
                    if generation == self.browser.scan {
                        self.folder_scan_failed();
                        self.browser.scan_running = false;
                        self.scanning = self.browser.pending.is_some();
                        self.status = format!("Elenco parziale o non disponibile: {error}");
                        self.finish_browser_scan();
                    }
                }
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
                } if generation == self.browser.scan => {
                    self.state.reconcile(items, true);
                    self.folder = folder;
                    self.browser.scan_running = false;
                    self.scanning = self.browser.pending.is_some();
                    self.status = note;
                    self.finish_browser_scan();
                    self.start_folder_preparation(false);
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
                    if self.folder_load.request == Some(request)
                        && self.rebuild.front().is_some_and(|i| i.id == id)
                    {
                        self.rebuild.pop_front();
                        self.folder_load.errors += usize::from(result.is_err());
                    }
                    match *result {
                        Ok(decoded) => {
                            self.source_status.remove(&id);
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
        if let Some(id) = self.state.current_item().map(|item| item.id.clone())
            && id != self.keyword_id
        {
            let previous = self.keyword_id.clone();
            self.commit_edit(&previous);
            self.keyword_id = id;
            self.keyword_text = self
                .state
                .current_item()
                .unwrap()
                .annotation
                .keywords
                .join(", ");
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
                if name.starts_with("navigation-") {
                    continue;
                }
                let pixels: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                let path = self
                    .root
                    .join(if name.starts_with("raw-engine-") {
                        "var"
                    } else {
                        "reports"
                    })
                    .join(format!("{name}.png"));
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
        // Menus and quality selectors own keyboard input until they close.
        // In particular, Escape must not also leave the viewer behind a popup.
        if egui::Popup::is_any_open(ctx) {
            return;
        }
        if self.browser_keyboard(ctx) {
            return;
        }
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
    fn set_language(&mut self, language: Language) {
        match self
            .service
            .cache
            .set_language(language, &self.settings_data)
        {
            Ok(()) => {
                self.cache_settings.language = language;
                self.context.request_repaint();
            }
            Err(error) => self.status = format!("Salvataggio lingua fallito: {error:#}"),
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let compact = ui.available_width() < 1000.;
        let viewer = self.state.view != ViewMode::Grid;
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TrueRenderer").size(17.).strong());
                    ui.add_space(8.);
                    ui.menu_button(lang.text("Apri"), |ui| {
                        if ui.button(lang.text("Apri cartella…")).clicked() {
                            ui.close();
                            if let Some(path) = rfd::FileDialog::new()
                                .set_directory(&self.folder)
                                .pick_folder()
                            {
                                self.open_folder(path);
                            }
                        }
                        if ui.button(lang.text("Apri file…")).clicked() {
                            ui.close();
                            if let Some(path) = rfd::FileDialog::new()
                                .set_directory(&self.folder)
                                .pick_file()
                            {
                                self.open_path(path);
                            }
                        }
                        if ui.button(lang.text("Rileggi cartella")).clicked() {
                            self.refresh_folder();
                            ui.close();
                        }
                    });
                    if compact {
                        ui.menu_button(lang.text("Vista"), |ui| {
                            self.view_choices(ui);
                        });
                    } else {
                        ui.add_space(8.);
                        self.view_choices(ui);
                    }
                    self.folder_loading_button(ui, compact);
                    if !compact {
                        self.engine_indicator(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(lang.text("Menu"), |ui| {
                            ui.set_min_width(200.);
                            ui.menu_button(lang.text("Lingua"), |ui| {
                                self.language_choices(ui);
                            });
                            if ui.button(lang.text("Libreria e filtri")).clicked() {
                                self.panel_mode(crate::browser_session::PanelMode::Library, true);
                                ui.close();
                            }
                            if ui.button(lang.text("Esplora")).clicked() {
                                self.panel_mode(crate::browser_session::PanelMode::Explorer, true);
                                ui.close();
                            }
                            ui.checkbox(&mut self.show_inspector, lang.text("Mostra ispettore"));
                            ui.checkbox(
                                &mut self.show_filmstrip,
                                lang.text("Mostra miniature nel viewer"),
                            );
                            ui.separator();
                            self.library_actions(ui);
                            ui.separator();
                            if ui
                                .button(lang.text("Guida e stato del prototipo"))
                                .clicked()
                            {
                                self.show_help = true;
                                ui.close();
                            }
                        });
                        if ui
                            .add(egui::Button::new(lang.text("Impostazioni")))
                            .clicked()
                        {
                            self.open_preferences(SettingsPage::Previews);
                        }
                        if !compact {
                            self.search_box(ui, ui.available_width().clamp(100., 240.));
                        }
                    });
                });
                if compact {
                    ui.horizontal(|ui| {
                        self.engine_indicator(ui);
                        self.search_box(ui, ui.available_width());
                    });
                }
                if viewer {
                    ui.add_space(4.);
                    self.viewer_controls(ui);
                }
            });
    }

    fn engine_indicator(&self, ui: &mut egui::Ui) {
        let settings = self.service.cache.settings();
        let lang = self.cache_settings.language;
        let saved = self
            .state
            .current_item()
            .and_then(|item| self.editing.entries.get(&item.id))
            .and_then(|entry| entry.loaded.as_ref())
            .filter(|edit| edit.generation > 0);
        let engine = saved.map_or(settings.raw_engine, |edit| edit.recipe.raw_engine);
        let name = match engine {
            tr_core::decoder::RawEngine::Apple => "Apple RAW",
            tr_core::decoder::RawEngine::LibRawBilinear => "LibRaw bilinear",
            tr_core::decoder::RawEngine::LibRawAhd => "LibRaw AHD",
            tr_core::decoder::RawEngine::TrueRenderer => "TrueRenderer fp32",
        };
        ui.add_sized([150., 28.], egui::Label::new(RichText::new(format!("RAW: {name}")).small().color(MUTED)).truncate())
            .on_hover_text(format!("{}: {}\n{}", lang.text("Motore RAW"), lang.text(engine.label()),
                if saved.is_some() {lang.text("Motore salvato nella ricetta della foto.")}
                else {lang.text("Motore selezionato e applicato. Il calcolo CPU/GPU del viewer si configura in Prestazioni.")}));
    }

    fn search_box(&mut self, ui: &mut egui::Ui, width: f32) {
        let lang = self.cache_settings.language;
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.state.query)
                .id(egui::Id::new("catalog-search"))
                .hint_text(lang.text("Cerca nome o parola chiave"))
                .desired_width(width),
        );
        if self.search_focus {
            response.request_focus();
            self.search_focus = false;
        }
        if response.changed() {
            self.state.refilter();
        }
    }

    fn view_choices(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        for (mode, title, shortcut) in [
            (ViewMode::Grid, "Griglia", "G"),
            (ViewMode::Preview, "Anteprima", "E"),
            (ViewMode::Compare, "Confronto", "C"),
        ] {
            if ui
                .add(
                    egui::Button::new(lang.text(title))
                        .selected(self.state.view == mode)
                        .min_size(egui::vec2(72., 28.)),
                )
                .on_hover_text(shortcut)
                .clicked()
            {
                self.command(Command::SetView(mode));
            }
        }
    }

    fn language_choices(&mut self, ui: &mut egui::Ui) {
        for (language, name) in [
            (Language::English, "English"),
            (Language::Italian, "Italiano"),
        ] {
            if ui
                .selectable_label(self.cache_settings.language == language, name)
                .clicked()
            {
                self.set_language(language);
                ui.close();
            }
        }
    }

    fn library_actions(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        if ui.button(lang.text("Esporta fotografie…")).clicked() {
            self.photo_export.open = true;
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                self.undo_available && self.state.pending.is_empty(),
                egui::Button::new(lang.text("Annulla modifica")),
            )
            .on_hover_text("Cmd/Ctrl + Z")
            .clicked()
        {
            self.command(Command::Undo);
        }
        if ui.button(lang.text("Crea backup")).clicked() {
            self.request(Request::Backup);
        }
        if ui
            .button(lang.text("Esporta annotazioni…"))
            .on_hover_text(lang.text("JSON con annotazioni e percorsi locali"))
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("TrueRenderer-annotations.json")
                .add_filter("JSON", &["json"])
                .save_file()
        {
            self.request(Request::Export(path));
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let explorer = self.browser.session.mode == crate::browser_session::PanelMode::Explorer;
        let width = if explorer {
            self.browser.session.explorer_width
        } else {
            self.browser.session.library_width
        };
        let panel = egui::Panel::left(if explorer {
            "navigation-explorer"
        } else {
            "navigation-library"
        })
        .default_size(width)
        .resizable(true)
        .size_range(if explorer {
            200.0..=420.0
        } else {
            192.0..=270.0
        })
        .frame(egui::Frame::new().fill(style::PANEL).inner_margin(8))
        .show(ui, |ui| {
            self.left_panel_contents(ui);
        });
        if explorer {
            self.browser.session.explorer_width = panel.response.rect.width().clamp(200., 420.);
        } else {
            self.browser.session.library_width = panel.response.rect.width().clamp(192., 270.);
        }
    }

    fn navigation_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Libreria"));
        if nav(
            ui,
            lang.text("Corpus di prova"),
            "12",
            self.folder == self.root.join("corpus"),
        )
        .clicked()
        {
            self.reset_filters();
            self.open_folder(self.root.join("corpus"));
        }
        ui.add_space(20.);
        section(ui, lang.text("Selezione"));
        if nav(
            ui,
            lang.text("Tutte le immagini"),
            &self.state.items.len().to_string(),
            self.state.minimum_rating == 0 && !self.state.rejected_only,
        )
        .clicked()
        {
            self.state.minimum_rating = 0;
            self.state.rejected_only = false;
            self.state.refilter();
        }
        if nav(
            ui,
            lang.text("Da conservare"),
            "≥1 ★",
            self.state.minimum_rating == 1 && !self.state.rejected_only,
        )
        .clicked()
        {
            self.state.minimum_rating = 1;
            self.state.rejected_only = false;
            self.state.refilter();
        }
        if nav(
            ui,
            lang.text("Cinque stelle"),
            "5 ★",
            self.state.minimum_rating == 5 && !self.state.rejected_only,
        )
        .clicked()
        {
            self.state.minimum_rating = 5;
            self.state.rejected_only = false;
            self.state.refilter();
        }
        if nav(ui, lang.text("Scartate"), "×", self.state.rejected_only).clicked() {
            self.state.rejected_only = true;
            self.state.minimum_rating = 0;
            self.state.refilter();
        }
        ui.add_space(24.);
        section(ui, lang.text("Filtri"));
        ui.label(
            RichText::new(lang.text("Valutazione minima"))
                .small()
                .color(MUTED),
        );
        let previous = self.state.minimum_rating;
        egui::ComboBox::from_id_salt("minimum-rating")
            .width(ui.available_width())
            .selected_text(if previous == 0 {
                lang.text("Qualsiasi valutazione").to_owned()
            } else {
                format!("{} ★ +", previous)
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.state.minimum_rating,
                    0,
                    lang.text("Qualsiasi valutazione"),
                );
                for rating in 1..=5 {
                    ui.selectable_value(
                        &mut self.state.minimum_rating,
                        rating,
                        format!("{rating} ★ +"),
                    );
                }
            });
        if previous != self.state.minimum_rating {
            self.state.rejected_only = false;
            self.state.refilter();
        }
        ui.add_space(8.);
        ui.label(RichText::new(lang.text("Etichetta")).small().color(MUTED));
        let previous = self.state.label_filter;
        egui::ComboBox::from_id_salt("label_filter")
            .selected_text(
                self.state
                    .label_filter
                    .map(|label| lang.text(label.text()))
                    .unwrap_or(lang.text("Tutte le etichette")),
            )
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.state.label_filter,
                    None,
                    lang.text("Tutte le etichette"),
                );
                for label in Label::ALL {
                    ui.selectable_value(
                        &mut self.state.label_filter,
                        Some(label),
                        lang.text(label.text()),
                    );
                }
            });
        if previous != self.state.label_filter {
            self.state.refilter();
        }
        ui.add_space(8.);
        if ui
            .add(egui::Button::new(lang.text("Azzera filtri")).frame(false))
            .clicked()
        {
            self.reset_filters();
        }
        ui.add_space(24.);
        ui.separator();
        ui.label(
            RichText::new(lang.text("Originali in sola lettura"))
                .small()
                .color(MUTED),
        );
    }

    fn reset_filters(&mut self) {
        self.state.query.clear();
        self.state.minimum_rating = 0;
        self.state.rejected_only = false;
        self.state.label_filter = None;
        self.state.refilter();
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        if ui.available_width() < 700. {
            let mut open = self.show_inspector;
            egui::Window::new(self.cache_settings.language.text("Ispettore"))
                .id(egui::Id::new("compact-inspector"))
                .open(&mut open)
                .default_width(300.)
                .max_width((ui.ctx().content_rect().width() - 56.).max(220.))
                .max_height((ui.ctx().content_rect().height() - 120.).max(180.))
                .vscroll(true)
                .show(ui.ctx(), |ui| self.inspector_contents(ui));
            self.show_inspector = open;
        } else {
            egui::Panel::right("inspector")
                .default_size(280.)
                .size_range(260.0..=360.0)
                .frame(style::panel())
                .show(ui, |ui| {
                    // Reserve a gutter: a floating scrollbar paints over image
                    // pixels and makes the inspector preview depend on hover.
                    ui.style_mut().spacing.scroll.floating = false;
                    let mut scroll = egui::ScrollArea::vertical().id_salt("inspector-scroll");
                    if self.smoke
                        && self.state.view != ViewMode::Grid
                        && std::env::args()
                            .any(|arg| arg == "--develop-smoke" || arg == "--output-proof-smoke")
                    {
                        scroll = scroll.vertical_scroll_offset(230.);
                    }
                    scroll.show(ui, |ui| self.inspector_contents(ui));
                });
        }
    }

    fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let Some(item) = self.state.current_item().cloned() else {
            ui.label(RichText::new(lang.text("Seleziona un'immagine")).color(MUTED));
            return;
        };
        self.ensure_edit_loaded(&item);
        ui.add_space(12.);
        ui.label(RichText::new(&item.name).strong());
        ui.label(
            RichText::new(if item.approved {
                lang.text("ANTEPRIMA")
            } else {
                lang.text("ANTEPRIMA NON DISPONIBILE")
            })
            .small()
            .color(AMBER),
        );
        if let Some(error) =
            self.errors
                .get(&format!("{}:{:?}", item.id, self.image_key(&item, 0).1))
        {
            ui.colored_label(AMBER, error);
        }

        ui.add_space(12.);
        let edge = (ui.available_width() * ui.ctx().pixels_per_point()).ceil() as u32;
        let edge = tr_core::preview::thumbnail_edge(edge);
        self.ensure_image(&item, edge);
        let key = self.image_key(&item, edge);
        let info = self.cache.get(&key).map(|c| c.info.clone());
        if let Some(info) = &info
            && info.scientific.is_some()
        {
            self.science_inspector(ui, &item, info);
            return;
        }
        if let Some((source, digest, source_histogram, cached_info)) =
            self.cache.get(&key).map(|c| {
                (
                    c.pyramid.clone(),
                    c.digest.clone(),
                    c.histogram,
                    c.info.clone(),
                )
            })
        {
            let edited = self
                .editing
                .entries
                .get(&item.id)
                .and_then(|entry| entry.draft.as_ref())
                .is_some_and(|recipe| !recipe.is_neutral());
            let shown = self
                .edited_thumbnail(&item.id, &digest, source.clone())
                .unwrap_or_else(|| source.clone());
            let rendered_edit = shown.id() != source.id();
            let histogram = if rendered_edit {
                self.edited_thumbnail_histogram(source.id())
                    .unwrap_or(source_histogram)
            } else {
                source_histogram
            };
            let mut preview = |ui: &mut egui::Ui| {
                let size = Vec2::new(
                    ui.available_width(),
                    ui.available_width() * cached_info.source_height as f32
                        / cached_info.source_width as f32,
                );
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                tr_render::presenter::fitted(
                    &mut self.presenter,
                    ui,
                    format!("inspector:{}", item.id),
                    &shown,
                    rect,
                );
                ui.add_space(12.);
            };
            if self.state.view == ViewMode::Grid {
                preview(ui);
            } else {
                egui::CollapsingHeader::new(lang.text("Anteprima immagine"))
                    .id_salt("inspector-preview")
                    .default_open(false)
                    .show(ui, preview);
            }
            tr_render::histogram(ui, &histogram);
            if let Some(error) = self.edited_thumbnail_error(source.id()) {
                ui.colored_label(AMBER, format!("Sviluppo: {error}"));
            }
            ui.label(
                RichText::new(if edited && self.editing.show_original {
                    localized_format!(
                        lang,
                        "Istogramma dello sviluppo originale · livello {}",
                        "Original development histogram · level {}",
                        source.base_level()
                    )
                } else if edited && rendered_edit {
                    localized_format!(
                        lang,
                        "Istogramma dell'anteprima modificata · livello {}",
                        "Edited preview histogram · level {}",
                        shown.base_level()
                    )
                } else if edited {
                    localized_format!(
                        lang,
                        "Istogramma della sorgente · modifica in calcolo · livello {}",
                        "Source histogram · edit computing · level {}",
                        source.base_level()
                    )
                } else {
                    localized_format!(
                        lang,
                        "Istogramma del livello {} · uscita sRGB composita",
                        "Level {} histogram · composited sRGB output",
                        source.base_level()
                    )
                })
                .small()
                .color(MUTED),
            );
        }
        ui.add_space(12.);
        if self.state.view != ViewMode::Grid {
            self.editing_controls(ui, &item);
        }
        ui.add_space(12.);
        egui::CollapsingHeader::new(lang.text("File"))
            .id_salt("inspector-file")
            .default_open(true)
            .show(ui, |ui| {
                field(
                    ui,
                    lang.text("Dimensioni"),
                    &info
                        .as_ref()
                        .map(|i| format!("{} × {} px", i.source_width, i.source_height))
                        .unwrap_or_else(|| lang.text("Non decodificato").into()),
                );
                field(
                    ui,
                    lang.text("Formato"),
                    &info
                        .as_ref()
                        .map(|i| {
                            if i.native_bits == 0 {
                                localized_format!(
                                    lang,
                                    "{} · profondità non dichiarata",
                                    "{} · bit depth not declared",
                                    i.format
                                )
                            } else {
                                localized_format!(
                                    lang,
                                    "{} · {} bit/canale",
                                    "{} · {} bits/channel",
                                    i.format,
                                    i.native_bits
                                )
                            }
                        })
                        .unwrap_or_else(|| "—".into()),
                );
                field(ui, lang.text("Dimensione"), &human_bytes(item.bytes));
            });
        ui.add_space(12.);
        egui::CollapsingHeader::new(lang.text("Valutazione"))
            .id_salt("inspector-rating")
            .default_open(true)
            .show(ui, |ui| {
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
                            .on_hover_text(localized_format!(
                                lang,
                                "{r} stelle · tasto {r}",
                                "{r} stars · key {r}"
                            ))
                            .clicked()
                        {
                            self.command(Command::Rate(r));
                        }
                    }
                    if ui
                        .selectable_label(item.annotation.rating == -1, "×")
                        .on_hover_text(lang.text("Scarta · X"))
                        .clicked()
                    {
                        self.command(Command::Rate(-1));
                    }
                });
                let mut label = item.annotation.label;
                egui::ComboBox::from_id_salt("annotation_label")
                    .selected_text(localized_format!(
                        lang,
                        "Etichetta: {}",
                        "Label: {}",
                        lang.text(label.text())
                    ))
                    .show_ui(ui, |ui| {
                        for v in Label::ALL {
                            ui.selectable_value(&mut label, v, lang.text(v.text()));
                        }
                    });
                if label != item.annotation.label {
                    self.command(Command::Label(label));
                }
            });
        ui.add_space(12.);
        egui::CollapsingHeader::new(lang.text("Parole chiave"))
            .id_salt("inspector-keywords")
            .default_open(true)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.keyword_text)
                        .hint_text(lang.text("paesaggio, studio, colore"))
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                if ui
                    .add_enabled(
                        !self.state.pending.contains(&item.id),
                        egui::Button::new(lang.text("Salva parole chiave")),
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
                        lang.text("Salvataggio in corso…")
                    } else {
                        lang.text("Solo libreria · XMP non attivo")
                    })
                    .small()
                    .color(MUTED),
                );
            });
        ui.add_space(16.);
        egui::CollapsingHeader::new(lang.text("Provenienza del render"))
            .id_salt("inspector-provenance")
            .default_open(false)
            .show(ui, |ui| {
                field(
                    ui,
                    lang.text("Ingresso"),
                    &lang.message(
                        info.as_ref()
                            .map(|i| i.input_color.as_str())
                            .unwrap_or(lang.text("Non determinato")),
                    ),
                );
                field(
                    ui,
                    lang.text("Lavoro"),
                    lang.text("Rec.2020 lineare · fp32"),
                );
                field(ui, "Alpha", lang.text("Premoltiplicata in luce lineare"));
                field(
                    ui,
                    lang.text("Uscita"),
                    "sRGB SDR · clamp [0,1] · precisione effettiva nelle preferenze",
                );
                field(ui, "Display", lang.text("Contratto da qualificare"));
                if let Some(cached) = self.cache.get(&key) {
                    field(ui, lang.text("Isolamento"), cached.transport);
                    field(ui, "SHA-256", &cached.digest);
                }
                if let Some(info) = info {
                    field(ui, "Decoder", &info.decoder);
                    field(
                        ui,
                        lang.text("Orientamento"),
                        &lang.message(&info.orientation),
                    );
                    field(ui, lang.text("Decodifica"), &lang.message(&info.filter));
                    field(
                        ui,
                        lang.text("Presentazione"),
                        lang.text("Lineare · Lanczos3 / Mitchell"),
                    );
                    field(ui, "Alpha", lang.text("Area / triangolare"));
                }
                ui.add_space(4.);
                ui.label(
                    RichText::new(lang.text("Standard e Riferimento richiedono le prove R1."))
                        .small()
                        .color(MUTED),
                );
            });
        if let Some(sample) = &self.sample
            && self.sample_item_id.as_deref() == Some(item.id.as_str())
        {
            ui.add_space(12.);
            section(ui, lang.text("CAMPIONE DEL VIEWPORT"));
            ui.label(RichText::new(&self.sample_source).small().color(MUTED));
            ui.monospace(format!("x {:5}   y {:5}", sample.x, sample.y));
            ui.monospace(format!(
                "RGB8  {} / {} / {}",
                sample.display[0], sample.display[1], sample.display[2]
            ));
            ui.monospace(format!(
                "lin R {:.5}\nlin G {:.5}\nlin B {:.5}\nalpha {:.5}",
                sample.working[0], sample.working[1], sample.working[2], sample.working[3]
            ));
        }
    }
    fn thumbnail(&mut self, ui: &mut egui::Ui, item: &Item, size: Vec2, show_name: bool) {
        let lang = self.cache_settings.language;
        let edge = ((size.x - 20.).max(1.) * ui.ctx().pixels_per_point()).ceil() as u32;
        let edge = tr_core::preview::thumbnail_edge(edge);
        self.ensure_edit_loaded(item);
        self.ensure_image(item, edge);
        let key = self.image_key(item, edge);
        let selected = self.state.selected.contains(&item.id);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let response = if let Some(error) = self.errors.get(&format!("{}:{:?}", item.id, key.1)) {
            response.on_hover_text(error)
        } else {
            response
        };
        self.image_focus_ids.insert(response.id);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                &item.name,
            )
        });
        let painter = ui.painter().clone();
        painter.rect_filled(
            rect,
            5,
            if selected {
                Color32::from_gray(42)
            } else if response.hovered() {
                Color32::from_gray(32)
            } else {
                CANVAS
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
        let mut edit_error = None;
        if let Some((source, digest)) = self
            .cache
            .get(&key)
            .map(|cache| (cache.pyramid.clone(), cache.digest.clone()))
        {
            let has_edit = self
                .editing
                .entries
                .get(&item.id)
                .and_then(|entry| entry.draft.as_ref())
                .is_some_and(|recipe| !recipe.is_neutral());
            let shown = if source.scientific() {
                Some(source.clone())
            } else {
                self.edited_thumbnail(&item.id, &digest, source.clone())
            };
            edit_error = self.edited_thumbnail_error(source.id()).map(str::to_owned);
            let rendered_edit = shown
                .as_ref()
                .is_some_and(|image| image.id() != source.id());
            tr_render::presenter::fitted(
                &mut self.presenter,
                ui,
                format!("thumbnail:{}:{show_name}", item.id),
                &shown.unwrap_or(source),
                area,
            );
            if has_edit {
                let label = if edit_error.is_some() {
                    lang.text("Errore")
                } else if self.editing.show_original {
                    lang.text("Prima")
                } else if rendered_edit {
                    lang.text("Modificata")
                } else {
                    lang.text("In calcolo")
                };
                let badge = egui::Rect::from_min_size(
                    area.min + Vec2::splat(4.),
                    Vec2::new(
                        (label.chars().count() as f32 * 6.5 + 12.).min(area.width()),
                        19.,
                    ),
                );
                painter.rect_filled(badge, 3., Color32::from_black_alpha(190));
                painter.text(
                    badge.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(11.),
                    Color32::WHITE,
                );
            }
        } else {
            let text = if self
                .errors
                .contains_key(&format!("{}:{:?}", item.id, key.1))
            {
                lang.text("Errore di lettura")
            } else if !item.approved {
                lang.text("Anteprima non abilitata")
            } else {
                lang.text("Caricamento…")
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
            let mut title = egui::text::LayoutJob::simple_singleline(
                item.name.clone(),
                egui::FontId::proportional(12.),
                TEXT,
            );
            title.wrap.max_width = (rect.width() - 28.).max(20.);
            title.wrap.max_rows = 1;
            title.wrap.break_anywhere = true;
            let galley = painter.layout_job(title);
            painter.galley(
                egui::pos2(rect.left() + 12., rect.bottom() - 42.),
                galley,
                TEXT,
            );
            let stars = if item.annotation.rating == -1 {
                lang.text("Scartata").into()
            } else {
                format!(
                    "{}{}",
                    "★".repeat(item.annotation.rating.max(0) as usize),
                    if item.annotation.rating == 0 {
                        "—".into()
                    } else {
                        String::new()
                    }
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
                    lang.text(item.annotation.label.text()),
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
        let tooltip = self
            .errors
            .get(&format!("{}:{:?}", item.id, key.1))
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    "{}\n{} · {}",
                    item.name,
                    human_bytes(item.bytes),
                    if item.approved {
                        lang.text("Anteprima disponibile · dettagli del decoder in Ispezione")
                    } else {
                        lang.text("Decoder non disponibile o file oltre quota")
                    }
                )
            });
        response.on_hover_text(if let Some(error) = edit_error {
            format!("{tooltip}\nSviluppo: {error}")
        } else {
            tooltip
        });
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
    fn viewer_controls(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let compact = ui.available_width() < 780.;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(self.state.current_item().is_some(), |ui| {
                if ui
                    .add(
                        egui::Button::new(lang.text("Adatta"))
                            .selected(self.state.transform.zoom.is_none())
                            .min_size(egui::vec2(52., 28.)),
                    )
                    .clicked()
                {
                    self.state.transform = ViewTransform::default();
                }
                if ui
                    .add(
                        egui::Button::new("1:1")
                            .selected(
                                self.state.transform.zoom == Some(1.)
                                    && self.state.current_item().is_some_and(|item| {
                                        self.quality(item) == PreviewQuality::Full
                                    }),
                            )
                            .min_size(egui::vec2(42., 28.)),
                    )
                    .on_hover_text(lang.text("Un pixel sorgente per pixel fisico dello schermo"))
                    .clicked()
                {
                    self.full_for_current();
                }
                if ui.small_button("−").clicked() {
                    self.state
                        .transform
                        .set_zoom(self.state.transform.zoom.unwrap_or(1.) / 1.25);
                }
                ui.label(
                    RichText::new(
                        self.state
                            .transform
                            .zoom
                            .map(|z| format!("{:.0}%", z * 100.))
                            .unwrap_or_else(|| lang.text("Adatta").into()),
                    )
                    .monospace()
                    .color(MUTED),
                );
                if ui.small_button("+").clicked() {
                    self.state
                        .transform
                        .set_zoom(self.state.transform.zoom.unwrap_or(1.) * 1.25);
                }
            });
            if !compact {
                ui.separator();
                self.quality_controls(ui);
            }
        });
        if compact {
            ui.horizontal(|ui| self.quality_controls(ui));
        }
    }

    fn quality_controls(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let mut global = self.service.cache.settings().quality;
        let quality_name = |quality| match quality {
            PreviewQuality::Standard => "Standard",
            PreviewQuality::Full => lang.text("Piena"),
        };
        ui.add_enabled_ui(self.cache_action.is_none(), |ui| {
            egui::ComboBox::from_id_salt("global-quality")
                .width(155.)
                .selected_text(format!(
                    "{}: {}",
                    lang.text("Globale"),
                    quality_name(global)
                ))
                .show_ui(ui, |ui| {
                    ui.label(lang.text("Qualità globale delle anteprime"));
                    ui.selectable_value(&mut global, PreviewQuality::Standard, "Standard");
                    ui.selectable_value(&mut global, PreviewQuality::Full, lang.text("Piena"));
                    ui.separator();
                    ui.label(lang.text("Cambia tutte le foto e azzera le eccezioni di sessione."));
                })
                .response
                .on_hover_text(
                    lang.text("Cambia tutte le foto e azzera le eccezioni di sessione."),
                );
        });
        self.set_quality(global);
        let item = self.state.current_item().cloned();
        let effective = item.as_ref().map_or(global, |item| self.quality(item));
        ui.add_enabled_ui(item.is_some(), |ui| {
            egui::ComboBox::from_id_salt("photo-quality")
                .width(210.)
                .selected_text(format!(
                    "{}: {}",
                    lang.text("Solo questa foto"),
                    quality_name(effective)
                ))
                .show_ui(ui, |ui| {
                    let Some(item) = &item else {
                        return;
                    };
                    let mut choice = self.quality_overrides.get(&item.id).copied();
                    let before = choice;
                    ui.selectable_value(&mut choice, Some(PreviewQuality::Standard), "Standard");
                    ui.selectable_value(
                        &mut choice,
                        Some(PreviewQuality::Full),
                        lang.text("Piena"),
                    );
                    ui.separator();
                    ui.selectable_value(&mut choice, None, lang.text("Usa la qualità globale"));
                    ui.label(lang.text("Solo la foto corrente · eccezione per questa sessione."));
                    if choice != before {
                        self.set_photo_quality(&item.id, choice);
                    }
                })
                .response
                .on_hover_text(lang.text("Solo la foto corrente · eccezione per questa sessione."));
        });
    }

    fn set_photo_quality(&mut self, id: &str, quality: Option<PreviewQuality>) {
        if let Some(quality) = quality {
            self.quality_overrides.insert(id.to_owned(), quality);
        } else {
            self.quality_overrides.remove(id);
        }
        self.context.request_repaint();
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let Some(item) = self.state.current_item().cloned() else {
            return;
        };
        if self.show_filmstrip {
            egui::Panel::bottom("filmstrip")
                .exact_size(104.)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(8))
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
                ui.label(lang.text("Seleziona due immagini con Cmd/Ctrl + clic nella griglia."));
            }
        } else {
            self.paint_view(ui, &item, "single");
        }
    }
    fn paint_view(&mut self, ui: &mut egui::Ui, item: &Item, id: &str) {
        let lang = self.cache_settings.language;
        if let Some(status) = self.source_status.get(&item.id) {
            ui.colored_label(AMBER, lang.message(status));
        }
        let fitted_edge = (ui.available_width().max(ui.available_height())
            * ui.ctx().pixels_per_point())
        .ceil()
        .clamp(1., 4096.) as u32;
        self.viewer_prefetch_edge = self.viewer_prefetch_edge.max(fitted_edge);
        let edge = if self.state.transform.zoom.is_none()
            && self.editing.verify_final.as_deref() != Some(item.id.as_str())
        {
            fitted_edge
        } else {
            0
        };
        let key = self.image_key(item, edge);
        if !self.cache.contains_key(&key) {
            // A prior zoom/quality must not pin an oversized fallback while the
            // replacement needs a full RAW working set. Smaller tails survive.
            self.release_large_ancestors();
        }
        // Finish visible small derivatives before retaining a native pyramid.
        // At 2 GiB a resident 24 MP pyramid plus the next full RAW development
        // cannot coexist. Keep the existing small preview, explicitly refining.
        if self.prepare_view_detail() {
            self.ensure_image_priority(
                item,
                edge,
                if self.cache.keys().any(|(id, _)| id == &item.id) {
                    PreviewPriority::Refinement
                } else {
                    PreviewPriority::Immediate
                },
            );
        }
        let fallback = self.viewer_fallback_key(item, key.1);
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
        if let Some((source, digest, base, source_size)) = selected
            .as_ref()
            .and_then(|key| self.cache.get(key))
            .map(|c| {
                (
                    c.pyramid.clone(),
                    c.digest.clone(),
                    c.pyramid.base_level(),
                    c.pyramid.source_size(),
                )
            })
        {
            if completeness == tr_core::preview::Completeness::Refining {
                ui.label(
                    if self.state.transform.zoom == Some(1.)
                        && key.1.quality == PreviewQuality::Full
                    {
                        lang.text("Preparazione dettaglio 1:1…")
                    } else {
                        lang.text("Raffinamento anteprima…")
                    },
                );
            } else {
                if key.1.quality == PreviewQuality::Standard && base > 0 {
                    ui.label(localized_format!(
                        lang,
                        "Dettaglio limitato a {} × {} pixel",
                        "Detail limited to {} × {} pixels",
                        source.source().width,
                        source.source().height
                    ));
                }
            }
            let image = if source.scientific() {
                Some(source.clone())
            } else {
                self.edited_preview(&item.id, &digest, source.clone())
            };
            let edited = self
                .editing
                .entries
                .get(&item.id)
                .and_then(|e| e.draft.as_ref())
                .is_some_and(|r| !r.is_neutral());
            let proof = self.output_proof_for(&item.id) && !self.editing.show_original;
            if proof {
                ui.label(lang.text("Anteprima export sRGB16 · PNG/TIFF · dimensioni native"));
                if image.is_none() {
                    ui.spinner();
                    ui.label(lang.text("Preparazione anteprima export dalla sorgente nativa…"));
                    if let Some(error) = self.edited_preview_error(&item.id) {
                        ui.colored_label(AMBER, error);
                    }
                    return;
                }
            } else if edited {
                if self.editing.show_original {
                    ui.label(lang.text("Prima · sviluppo originale"));
                } else if image.is_none() {
                    ui.label(lang.text("Calcolo regolazioni · originale provvisorio"));
                } else if image.as_ref().is_some_and(|image| image.base_level() == 0) {
                    ui.label(lang.text("Resa finale alla risoluzione nativa"));
                } else {
                    ui.label(lang.text("Anteprima modificata provvisoria · export nativo"));
                }
            }
            if let Some(error) = self.edited_preview_error(&item.id) {
                ui.colored_label(AMBER, error);
            }
            let sample_from_current_render =
                !source.scientific() && !self.editing.show_original && !proof && image.is_some();
            let image = image.unwrap_or(source);
            let lane = format!(
                "view:{id}:{}:{}:{:?}:{:?}:{}",
                item.id,
                digest,
                key.1.raw_engine,
                source_size,
                image.id()
            );
            let (response, sample) = tr_render::viewport(
                ui,
                &mut self.presenter,
                &image,
                &mut self.state.transform,
                &lane,
            );
            self.image_focus_ids.insert(response.id);
            if sample.is_some() {
                self.sample_source = localized_format!(
                    lang,
                    "Livello {} · {}",
                    "Level {} · {}",
                    image.base_level(),
                    item.name
                );
                self.sample = sample;
                self.sample_item_id = Some(item.id.clone());
                self.sample_level = Some(image.base_level());
                self.sample_from_current_render = sample_from_current_render;
            }
        } else {
            egui::Frame::new().fill(Color32::from_gray(119)).show(ui,|ui|{
                ui.set_min_size(ui.available_size());ui.centered_and_justified(|ui|{
                    ui.label(lang.message(self.errors.get(&format!("{}:{:?}", item.id, key.1)).map(String::as_str).unwrap_or(if item.approved{lang.text("Preparazione dell'immagine…")}else{lang.text("Aprire il bundle macOS con decoder XPC.\nLimite: 256 MiB per file, 64 Mi pixel.")})));
                });
            });
        }
    }
    fn folder_label(&self) -> &str {
        if self.folder == self.root.join("corpus") {
            self.cache_settings.language.text("Corpus di prova")
        } else {
            self.folder
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(self.cache_settings.language.text("Immagini"))
        }
    }

    fn thumbnail_size_control(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        ui.spacing_mut().slider_width = 100.;
        ui.add(egui::Slider::new(&mut self.cell_size, 150.0..=300.0).show_value(false))
            .on_hover_text(lang.text("Miniature"));
        ui.label(RichText::new(lang.text("Miniature")).small().color(MUTED));
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let compact = ui.available_width() < 1000.;
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let name = self.folder_label().to_owned();
                    ui.add_sized(
                        [if compact { 135. } else { 200. }, 28.],
                        egui::Label::new(RichText::new(name).small()).truncate(),
                    )
                    .on_hover_text(self.folder.display().to_string());
                    ui.label(
                        RichText::new(localized_format!(
                            lang,
                            "/ {} immagini · {} selezionate",
                            "/ {} images · {} selected",
                            self.state.visible.len(),
                            self.state.selected.len()
                        ))
                        .small(),
                    );
                    ui.separator();
                    ui.label(RichText::new(lang.text("Anteprima")).small().color(AMBER))
                        .on_hover_text(lang.message(&self.status));
                    if !compact {
                        ui.label(
                            RichText::new("fp32 working > SDR sRGB")
                                .small()
                                .color(MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.thumbnail_size_control(ui);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(lang.message(&self.status))
                                        .small()
                                        .color(if self.fatal { AMBER } else { MUTED }),
                                )
                                .truncate(),
                            )
                            .on_hover_text(lang.message(&self.status));
                        });
                    }
                });
                if compact {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("fp32 working > SDR sRGB")
                                .small()
                                .color(MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.thumbnail_size_control(ui);
                        });
                    });
                }
                if self.fatal && compact {
                    ui.add(
                        egui::Label::new(
                            RichText::new(lang.message(&self.status))
                                .small()
                                .color(AMBER),
                        )
                        .truncate(),
                    )
                    .on_hover_text(lang.message(&self.status));
                }
            });
    }
    fn help(&mut self, ctx: &egui::Context) {
        let lang = self.cache_settings.language;
        egui::Window::new(lang.text("TrueRenderer · guida e stato")).id(egui::Id::new("help-window")).open(&mut self.show_help).default_width(620.).show(ctx,|ui|{
            ui.heading(lang.text("Un'immagine, una resa tracciabile."));
            ui.label(localized_format!(lang, "Prototipo R0 · {}", "R0 prototype · {}", env!("CARGO_PKG_VERSION")));ui.separator();
            ui.label(lang.text("Disponibile: corpus PNG 8/16 bit, griglia, anteprima, confronto a due, zoom fisico 1:1, campione al puntatore, rating, etichette, parole chiave, ricerca, undo e backup locali."));
            ui.add_space(8.);ui.label(lang.text("Il motore RAW si sceglie nelle impostazioni: Apple sul Mac, LibRaw bilineare/AHD e TrueRenderer fp32 sperimentale. Il motore proprio supporta attualmente Nikon D750 e D40 Bayer; compatibilità e resa dipendono dal motore. Il bundle Mac usa servizi XPC, il port Windows un worker confinato sperimentale. Massimo 256 MiB e 64 Mi pixel; il normale worker non confinato accetta soltanto il corpus."));
            ui.add_space(8.);ui.label(lang.text("Restano da qualificare: XPC/App Sandbox e Windows, ICC/Little CMS, presentazione sul monitor, filtri e CPU/GPU, accessibilità e prestazioni. JPEG/TIFF, RAW, XMP e gigapixel seguono la roadmap. Il badge rimane Anteprima."));
            ui.add_space(8.);ui.monospace(localized_format!(lang, "GPU: {}\nSuperficie: {}\nSQLite: {}", "GPU: {}\nSurface: {}\nSQLite: {}",lang.text(&self.adapter),lang.text(&self.surface),tr_store::sqlite_version()));
            ui.label(lang.message(&self.gpu_status));
            ui.separator();
            for (key,action) in [("0–5 / X",lang.text("Valuta / scarta")),("6–9",lang.text("Etichette rosso, giallo, verde, blu")),("G / E / C",lang.text("Griglia / anteprima / confronto")),("Z / Cmd+1",lang.text("Adatta o pixel fisici 1:1")),(lang.text("Frecce / trascina / rotella"),lang.text("Naviga / pan / zoom")),("Cmd+F / Cmd+Z",lang.text("Ricerca / annulla modifica")),("I / T / F / Esc",lang.text("Pannello / miniature / schermo intero / griglia"))]{field(ui,key,action);}
            ui.label(RichText::new(lang.text("Su Windows usare Ctrl al posto di Cmd. Le scorciatoie non agiscono mentre scrivi in un campo.")).small().color(MUTED));
            ui.add_space(8.);ui.label(localized_format!(lang, "Progetto e piano: {}", "Project and plan: {}",self.root.display()));
        });
    }
    fn capture_screenshot(&self, ctx: &egui::Context, name: &str) {
        let ppp = ctx.pixels_per_point();
        let records: Vec<_> = self.presenter.captures().iter().filter_map(|c| {
            let ((id,_), cached) = self.cache.iter().find(|(_,cached)| cached.pyramid.id() == c.source)?;
            let item = self.state.items.iter().find(|i| &i.id == id)?;
            if name.starts_with("raw-engine-") && c.rect.width() > 400. && c.rect.height() > 200. {
                let expected = cached.pyramid.render(c.region).ok()?.to_display();
                image::save_buffer(self.root.join("var").join(format!("{name}-expected.png")), &expected, c.region.size[0], c.region.size[1], image::ColorType::Rgba8).ok()?;
                return Some(serde_json::json!({"rect_physical":[c.rect.min.x*ppp,c.rect.min.y*ppp,c.rect.max.x*ppp,c.rect.max.y*ppp],"clip_physical":[c.clip.min.x*ppp,c.clip.min.y*ppp,c.clip.max.x*ppp,c.clip.max.y*ppp],"size":c.region.size,"compute":c.compute}));
            }
            if item.name != "04_Frequenze_radiali.png" || !c.clip.contains_rect(c.rect) { return None; }
            Some(serde_json::json!({"source":item.name,"rect_physical":[c.rect.min.x*ppp,c.rect.min.y*ppp,c.rect.max.x*ppp,c.rect.max.y*ppp],"size":c.region.size,"origin":c.region.origin,"step":c.region.step,"compute":c.compute}))
        }).collect();
        let _ = std::fs::write(
            self.root
                .join(if name.starts_with("raw-engine-") {
                    "var"
                } else {
                    "reports"
                })
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
    fn poll_cache_action(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.cache_action {
            match rx.try_recv() {
                Ok(result) => {
                    self.cache_action = None;
                    self.status = match result {
                        Ok(()) => "Impostazioni/cache aggiornate".into(),
                        Err(e) => e,
                    };
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.cache_action = None;
                    self.status = "Operazione cache interrotta".into();
                }
                _ => {}
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    /// Full settings-button action, also used by native and headless regression
    /// checks. Cache maintenance has its own receiver; `scanning` only describes
    /// a real pending Request::Scan and must survive maintenance completion.
    fn start_cache_action(&mut self, save: bool) {
        if save && self.cache_settings.quality != self.service.cache.settings().quality {
            self.quality_overrides.clear();
        }
        self.errors.clear();
        if save {
            self.apply_settings();
        }
        let cache = self.service.cache.clone();
        let folder = self.folder.clone();
        let data = self.settings_data.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.cache_action = Some(rx);
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                if save {
                    cache.save_current(&data)?;
                }
                let start = Instant::now();
                loop {
                    match cache.maintain(&folder, !save) {
                        Err(e)
                            if e.downcast_ref::<std::io::Error>()
                                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
                                && start.elapsed() < Duration::from_secs(3) =>
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
    fn source_change_smoke(&mut self, ctx: &egui::Context) {
        let finish = |passed: bool, stage: u8, generation: u64| {
            let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),
                "passed":passed,"stage":stage,"generation":generation,
                "scope":"Native viewer on generated PNG: replace, remove and restore while resident, reject old decode epoch, preserve saved rating. Polling observation is best-effort, not a coherent snapshot under arbitrary concurrent writers."});
            let _ = std::fs::write(
                self.root.join("reports/preview-source-changes-macos.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        };
        if self.started.elapsed() > Duration::from_secs(60) || self.fatal {
            finish(false, self.smoke_stage, self.generation);
            return;
        }
        let Some(item) = self.state.current_item().cloned() else {
            return;
        };
        let ready = self.demand.iter().any(|key| key.0 == item.id)
            && self.demand.iter().all(|key| self.cache.contains_key(key))
            && self.presenter.is_idle();
        let replacement = self.root.join("corpus/04_Frequenze_radiali.png");
        match self.smoke_stage {
            0 if ready => {
                self.command(Command::Rate(4));
                self.smoke_stage = 1;
            }
            1 if self.state.pending.is_empty() && item.annotation.rating == 4 => {
                std::fs::copy(&replacement, &item.path).unwrap();
                self.smoke_stage = 2;
            }
            2 if self.generation >= 2 && ready => {
                let expected = tr_platform::snapshot(&replacement).unwrap().1;
                if self
                    .cache
                    .iter()
                    .filter(|((id, _), _)| *id == item.id)
                    .any(|(_, c)| c.digest != expected)
                {
                    finish(false, self.smoke_stage, self.generation);
                    return;
                }
                std::fs::remove_file(&item.path).unwrap();
                self.smoke_stage = 3;
            }
            3 if self.generation >= 3 && !item.approved => {
                if self.cache.keys().any(|(id, _)| *id == item.id) {
                    finish(false, self.smoke_stage, self.generation);
                    return;
                }
                std::fs::copy(&replacement, &item.path).unwrap();
                self.smoke_stage = 4;
            }
            4 if self.generation >= 4 && ready => {
                finish(
                    item.annotation.rating == 4
                        && self.errors.is_empty()
                        && !self.presenter.has_errors(),
                    self.smoke_stage,
                    self.generation,
                );
            }
            _ => {}
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
    fn prepare_view_detail(&mut self) -> bool {
        let pending = self.demand.iter().any(|key| {
            key.1.maximum_level_edge() <= 2048
                && !self.cache.contains_key(key)
                && !self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))
        });
        if pending {
            self.release_large_ancestors();
        }
        !pending
    }
    fn release_large_ancestors(&mut self) {
        let limit = self.service.cache.memory.usage().limit / 8;
        let pinned = self
            .cache
            .iter()
            .filter(|(key, _)| self.demand.contains(*key))
            .map(|(_, c)| c.pyramid.id())
            .collect::<HashSet<_>>();
        let sources = self
            .cache
            .values()
            .filter(|c| !pinned.contains(&c.pyramid.id()) && c.pyramid.byte_len() as u64 > limit)
            .map(|c| c.pyramid.id())
            .collect::<HashSet<_>>();
        if !sources.is_empty() {
            self.cache.retain(|_, c| !sources.contains(&c.pyramid.id()));
            // Completed frames own output textures, not the CPU ancestor. Keep
            // them for reprojection; the next paint replaces queued input jobs.
            // Active work retains its lease until actual completion.
        }
    }
    fn raw_engines_smoke(&mut self, ctx: &egui::Context) {
        use tr_core::decoder::RawEngine;
        let mut engines: Vec<_> = RawEngine::choices().collect();
        engines.push(RawEngine::default()); // return to an earlier engine/cache
        if self.raw_engine_smoke_results.len() >= engines.len() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(50));
        let stage = self.smoke_stage as usize;
        if self.started.elapsed() > Duration::from_secs(180)
            || self.fatal
            || !self.errors.is_empty()
        {
            let report = serde_json::json!({"passed":false,"stage":stage,"errors":self.errors,"status":self.status});
            let _ = std::fs::write(
                self.root.join("var/raw-engine-ui.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if stage == 0
            && !self.scanning
            && self.cache_action.is_none()
            && !self.state.items.is_empty()
        {
            let id = self
                .state
                .items
                .get(1)
                .unwrap_or(&self.state.items[0])
                .id
                .clone();
            self.command(Command::Select { id, extend: false });
            self.state.transform.zoom = Some(1.0);
            self.state.view = ViewMode::Preview;
            self.show_filmstrip = std::env::args().any(|arg| arg == "--raw-engine-filmstrip");
            self.cache_settings.quality = PreviewQuality::Full;
            self.cache_settings.raw_engine = engines[0];
            self.start_cache_action(true);
            self.smoke_stage = 1;
        } else if stage > 0
            && !self.scanning
            && self.cache_action.is_none()
            && self.gpu_passed
            && self.presenter.is_idle()
            && !self.demand.is_empty()
            && self.demand.iter().all(|key| self.cache.contains_key(key))
            && self
                .state
                .current_item()
                .is_some_and(|item| self.cache.contains_key(&self.image_key(item, 0)))
        {
            let engine = engines[stage - 1];
            let correct = self.demand.iter().all(|key| {
                key.1.raw_engine == engine
                    && self.cache.get(key).is_some_and(|c| {
                        c.info.format == "RAW"
                            && (c.info.decoder.contains(engine.recipe())
                                || engine == RawEngine::Apple)
                    })
            });
            if !correct {
                self.raw_engine_smoke_ready_at = None;
                return;
            }
            // CPU delivery and an empty upload queue precede the first visible
            // GPU frame. Require the current source in the main viewport and
            // allow presentation to settle before requesting the screenshot.
            let painted = self.presenter.captures().iter().any(|capture| {
                capture.rect.width() > 400.
                    && capture.rect.height() > 200.
                    && self.demand.iter().any(|key| {
                        self.cache
                            .get(key)
                            .is_some_and(|cached| cached.pyramid.id() == capture.source)
                    })
            });
            if !painted {
                self.raw_engine_smoke_ready_at = None;
                return;
            }
            if self
                .raw_engine_smoke_ready_at
                .get_or_insert_with(Instant::now)
                .elapsed()
                < Duration::from_secs(1)
            {
                return;
            }
            let name = format!("raw-engine-ui-{stage}");
            if !self.screenshots.contains(&name) {
                if !self.raw_engine_smoke_capture_pending {
                    self.capture_screenshot(ctx, &name);
                    self.raw_engine_smoke_capture_pending = true;
                }
            } else {
                let selection = self.state.current.clone();
                self.raw_engine_smoke_results.push(serde_json::json!({"engine":engine,"selection":selection,"generation":self.generation,"cache":self.service.cache.stats(),"zoom":self.state.transform.zoom,"center":self.state.transform.center}));
                if stage == engines.len() {
                    let same_selection = self.raw_engine_smoke_results.iter().all(|r| {
                        r["selection"] == self.raw_engine_smoke_results[0]["selection"]
                            && r["zoom"] == self.raw_engine_smoke_results[0]["zoom"]
                            && r["center"] == self.raw_engine_smoke_results[0]["center"]
                    });
                    let report = serde_json::json!({"passed":same_selection && !self.presenter.has_errors(),"platform":std::env::consts::OS,"stages":self.raw_engine_smoke_results,"changed_engine_during_scan":std::env::args().any(|arg| arg == "--raw-engine-scan-change"),"scanned_items":self.state.items.len(),"scope":"Native UI, production apply-settings path, full RAW, engine provenance, selection preserved and return to earlier engine; private screenshots in var. Not colour/display qualification."});
                    let _ = std::fs::write(
                        self.root.join("var/raw-engine-ui.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.cache_settings.raw_engine = engines[stage];
                    self.start_cache_action(true);
                    self.smoke_stage += 1;
                    self.raw_engine_smoke_ready_at = None;
                    self.raw_engine_smoke_capture_pending = false;
                }
            }
        } else {
            self.raw_engine_smoke_ready_at = None;
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
    fn smoke_tick(&mut self, ctx: &egui::Context) {
        if std::env::args().any(|a| a == "--folder-loading-smoke") {
            self.folder_loading_smoke(ctx);
            return;
        }
        if std::env::args().any(|arg| arg == "--filesystem-smoke") {
            self.filesystem_smoke(ctx);
            return;
        }
        if self.navigation_probe.is_some() {
            return;
        }
        if !self.smoke {
            return;
        }
        if std::env::args().any(|arg| arg == "--develop-smoke" || arg == "--output-proof-smoke") {
            self.develop_smoke(ctx);
            return;
        }
        if std::env::args().any(|arg| arg == "--source-change-smoke") {
            self.source_change_smoke(ctx);
            return;
        }
        if std::env::args().any(|arg| arg == "--raw-engines-smoke") {
            self.raw_engines_smoke(ctx);
            return;
        }
        if self.settings_smoke {
            if std::env::args().any(|arg| arg == "--restyle-smoke") {
                self.restyle_smoke(ctx);
                return;
            }
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
            self.smoke_layout = None;
            ctx.request_repaint_after(Duration::from_millis(25));
            return;
        }
        // Screenshot delivery is asynchronous. Let panel/scrollbar layout settle
        // before recording geometry, so it describes the captured frame too.
        if [0, 2, 4, 6, 8, 10, 12, 14].contains(&self.smoke_stage) && elapsed < 55. {
            let geometry = self
                .presenter
                .captures()
                .iter()
                .map(|c| format!("{}:{:?}:{:?}:{:?};", c.source, c.rect, c.clip, c.region))
                .collect::<String>();
            if !self
                .smoke_layout
                .as_ref()
                .is_some_and(|(stage, previous, _)| {
                    *stage == self.smoke_stage && previous == &geometry
                })
            {
                self.smoke_layout = Some((self.smoke_stage, geometry, Instant::now()));
            }
            if self
                .smoke_layout
                .as_ref()
                .is_some_and(|(_, _, since)| since.elapsed() < Duration::from_millis(250))
            {
                ctx.request_repaint_after(Duration::from_millis(25));
                return;
            }
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
            let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":self.smoke_stage==(if self.sampling_smoke {15} else {5})&&self.screenshots.len()==(if self.sampling_smoke {8} else {3})&&!self.fatal&&self.errors.is_empty()&&!self.presenter.has_errors()&&self.gpu_passed,"sampling":tr_core::resample::VERSION,"presentation_errors":self.presenter.has_errors(),"pixels_per_point":ctx.pixels_per_point(),"adapter":self.adapter,"surface":self.surface,"gpu":self.gpu_status,"compute":self.presenter.statistics(),"sqlite":tr_store::sqlite_version(),"worker_pids":self.cache.values().filter_map(|c| c.worker_pid).collect::<std::collections::BTreeSet<_>>(),"worker_transports":self.cache.values().map(|c| c.transport).collect::<std::collections::BTreeSet<_>>(),"frames":self.frame_number,"elapsed_seconds":elapsed,"images":self.state.items.len(),"screenshots":self.screenshots,"decode_errors":self.errors,"status":self.status,"scope":"native R0 corpus smoke; not color/display/sandbox qualification"});
            let _ = std::fs::write(
                self.root
                    .join(format!("reports/smoke-{}.json", std::env::consts::OS)),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
impl eframe::App for TrueRenderer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let lang = self.cache_settings.language;
        let ctx = ui.ctx().clone();
        self.frame_number += 1;
        self.watched_sources.clear();
        self.demand.clear();
        self.primary_demand.clear();
        self.viewer_prefetch_edge = 0;
        self.demand_jobs.clear();
        self.navigation_receive(&ctx);
        self.poll(&ctx);
        if !self.navigation_begin(&ctx) {
            return;
        }
        if !self.folder_loading_blocks() {
            self.keyboard(&ctx);
        }
        self.image_focus_ids.clear();
        if ctx.input(|i| i.viewport().close_requested())
            && (!self.state.pending.is_empty() || self.edits_have_pending())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.commit_all_edits();
            self.status = "Attendo il salvataggio delle modifiche prima di chiudere…".into();
        }
        if self.closing && self.state.pending.is_empty() && !self.edits_have_pending() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.toolbar(ui);
        self.location_bar(ui);
        self.footer(ui);
        if ui.available_width() >= 1000. && self.browser.session.visible {
            self.sidebar(ui);
        }
        if !self.folder_loading_blocks()
            && self.show_inspector
            && (!self.show_settings || ui.available_width() >= 700.)
        {
            self.inspector(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(CANVAS).inner_margin(16))
            .show(ui, |ui| {
                if self.folder_loading_blocks() {
                    ui.centered_and_justified(|ui| {
                        ui.label(lang.text("Preparazione delle anteprime…"));
                    });
                } else if self.scanning && self.state.visible.is_empty() {
                    ui.label(lang.text("Lettura dei file…"));
                } else if self.state.visible.is_empty() {
                    ui.add_space((ui.available_height() * 0.2).min(70.));
                    ui.vertical_centered(|ui| {
                        ui.heading(lang.text("Nessuna immagine da mostrare"));
                        ui.label(lang.text("Apri una cartella oppure azzera i filtri."));
                        ui.add_space(16.);
                        if ui.button(lang.text("Apri cartella…")).clicked()
                            && let Some(folder) = rfd::FileDialog::new()
                                .set_directory(&self.folder)
                                .pick_folder()
                        {
                            self.open_folder(folder);
                        }
                        if !self.state.items.is_empty()
                            && ui.button(lang.text("Azzera filtri")).clicked()
                        {
                            self.reset_filters();
                        }
                        if ui.button(lang.text("Apri il corpus di prova")).clicked() {
                            self.reset_filters();
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
        self.temporary_panel(&ctx);
        self.settings_window(&ctx);
        self.export_window(&ctx);
        self.trim_images(false);
        self.smoke_tick(&ctx);
        self.background_demand(&ctx);
        self.flush_demand();
        self.folder_loading_popup(&ctx);
        self.navigation_capture(&ctx);
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
        self.persist_browser();
        let _ = self.service.high.try_send(Request::Shutdown);
    }
}
fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(12.).strong().color(TEXT));
    ui.add_space(8.);
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
    let response = ui.add_sized(
        [ui.available_width(), 32.],
        egui::Button::new("").selected(selected).frame(false),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            format!("{title} {count}"),
        )
    });
    let painter = ui.painter();
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        egui::FontId::proportional(13.),
        TEXT,
    );
    job.wrap.max_width = (response.rect.width() - 60.).max(20.);
    job.wrap.max_rows = 1;
    let title = painter.layout_job(job);
    painter.galley(
        egui::pos2(
            response.rect.left() + 10.,
            response.rect.center().y - title.size().y / 2.,
        ),
        title,
        TEXT,
    );
    painter.text(
        egui::pos2(response.rect.right() - 10., response.rect.center().y),
        egui::Align2::RIGHT_CENTER,
        count,
        egui::FontId::monospace(12.),
        MUTED,
    );
    response
}
fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.)
    } else {
        format!("{:.0} KiB", bytes as f64 / 1024.)
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod settings_regressions {
    use super::*;

    pub(super) fn app() -> (tempfile::TempDir, egui::Context, TrueRenderer) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("corpus")).unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        for (name, bytes) in [
            (
                "first.png",
                include_bytes!("../../../corpus/01_Studio_cromatico.png").as_slice(),
            ),
            (
                "second.png",
                include_bytes!("../../../corpus/05_Trasparenza.png").as_slice(),
            ),
        ] {
            std::fs::write(dir.path().join("corpus").join(name), bytes).unwrap();
        }
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let app = TrueRenderer::new(
            &cc,
            dir.path().into(),
            dir.path().join("data"),
            dir.path().join("unused-worker"),
            Startup {
                navigation: false,
                smoke: false,
                sampling_smoke: false,
                external_smoke: false,
                settings_smoke: false,
                open: None,
            },
        );
        (dir, ctx, app)
    }

    pub(super) fn settle(app: &mut TrueRenderer, ctx: &egui::Context, scans: bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let ctx = ui.ctx();
                app.poll_cache_action(ctx);
                if scans {
                    app.poll(ctx);
                }
            });
            // This is a headless state/service check, with no texture backend.
            output.textures_delta.clear();
            assert!(!app.fatal, "{}", app.status);
            if app.cache_action.is_none() && (!scans || !app.scanning) {
                return;
            }
            assert!(Instant::now() < deadline, "Timed out: {}", app.status);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn external_edit_identity_resolves_only_through_current_verified_cache() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let mut item = app.state.items[0].clone();
        item.digest = "unverified:original".into();
        app.state.items[0] = item.clone();
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(16, 8, vec![[0.2, 0.3, 0.4, 1.]; 128]).unwrap(),
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        let digest = "a".repeat(64);
        let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.exposure_ev = 1.;
        app.editing.entries.insert(
            item.id.clone(),
            editing::EditEntry {
                loaded: Some(tr_store::LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: item.digest.clone(),
                    generation: 1,
                    revision: 1,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe),
                ..Default::default()
            },
        );
        assert!(
            app.edited_preview(&item.id, &digest, source.clone())
                .is_none()
        );
        let key = app.image_key(&item, 0);
        app.cache.insert(
            key.clone(),
            CachedImage {
                digest: digest.clone(),
                info: RasterInfo {
                    scientific: None,
                    reference_mip: None,
                    width: 16,
                    height: 8,
                    source_width: 16,
                    source_height: 8,
                    native_bits: 32,
                    format: "test".into(),
                    decoder: "test".into(),
                    input_color: "linear Rec2020".into(),
                    filter: "reference".into(),
                    orientation: "applied".into(),
                },
                histogram: source.source().histogram(),
                pyramid: source.clone(),
                touched: app.frame_number,
                transport: "test",
                worker_pid: None,
            },
        );
        for proof in [false, true] {
            app.request_final_preview(&item.id, proof);
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                app.poll_edit_preview();
                if app
                    .edited_preview(&item.id, &digest, source.clone())
                    .is_some()
                    && app
                        .edited_thumbnail(&item.id, &digest, source.clone())
                        .is_some()
                {
                    break;
                }
                assert!(Instant::now() < deadline, "External recipe did not render");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(
            app.edited_preview(&item.id, &"b".repeat(64), source.clone())
                .is_none()
        );
        app.state.items[0].digest = "unverified:replacement".into();
        assert!(
            app.edited_preview(&item.id, &digest, source.clone())
                .is_none()
        );
        assert!(
            app.edited_thumbnail(&item.id, &digest, source.clone())
                .is_none()
        );
        app.state.items[0].digest = item.digest;
        app.cache.remove(&key);
        assert!(
            app.edited_preview(&item.id, &digest, source.clone())
                .is_none()
        );
        assert!(app.edited_thumbnail(&item.id, &digest, source).is_none());
    }

    #[test]
    fn edited_previews_return_temporary_memory_credits() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.exposure_ev = 0.75;
        app.editing.entries.insert(
            item.id.clone(),
            editing::EditEntry {
                loaded: Some(tr_store::LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: "lease".into(),
                    generation: 1,
                    revision: 1,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe),
                ..Default::default()
            },
        );
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(1024, 64, vec![[0.2, 0.3, 0.4, 1.]; 1024 * 64])
                    .unwrap(),
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        let baseline = app.service.cache.memory.usage().reserved;
        for proof in [false, true] {
            app.request_final_preview(&item.id, proof);
            let deadline = Instant::now() + Duration::from_secs(5);
            let rendered = loop {
                app.poll_edit_preview();
                if let Some(ready) = app.edited_preview(&item.id, "lease", source.clone()) {
                    break ready;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(
                app.service.cache.memory.usage().reserved - baseline,
                rendered.byte_len() as u64
            );
            app.clear_edit_preview();
            // A consumer still holding the image must retain its resident credits.
            assert_eq!(
                app.service.cache.memory.usage().reserved - baseline,
                rendered.byte_len() as u64
            );
            drop(rendered);
            assert_eq!(app.service.cache.memory.usage().reserved, baseline);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let thumbnail = loop {
            app.poll_edit_preview();
            if let Some(ready) = app.edited_thumbnail(&item.id, "lease", source.clone()) {
                break ready;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            app.service.cache.memory.usage().reserved - baseline,
            thumbnail.byte_len() as u64
        );
        app.clear_edit_thumbnails(&item.id);
        assert_eq!(
            app.service.cache.memory.usage().reserved - baseline,
            thumbnail.byte_len() as u64
        );
        drop(thumbnail);
        assert_eq!(app.service.cache.memory.usage().reserved, baseline);
    }
    #[test]
    fn output_proof_requires_native_handles_neutral_alpha_and_inflight_mode_changes() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        for alpha in [1., 0.999999] {
            let raster = tr_core::color::LinearImage::new(
                2048,
                2,
                (0..4096)
                    .map(|i| {
                        [
                            if i % 2 == 0 { 2. * alpha } else { -0.1 * alpha },
                            0.3 * alpha,
                            0.1 * alpha,
                            alpha,
                        ]
                    })
                    .collect(),
            )
            .unwrap();
            let source =
                Arc::new(ImageLevels::from_source(raster.clone(), PreviewRequest::full()).unwrap());
            for ev in [0., 0.75] {
                let mut recipe =
                    tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
                recipe.exposure_ev = ev;
                app.editing.entries.insert(
                    item.id.clone(),
                    editing::EditEntry {
                        loaded: Some(tr_store::LoadedEdit {
                            asset_id: item.id.clone(),
                            source_digest: "proof".into(),
                            generation: 1,
                            revision: 1,
                            recipe: recipe.clone(),
                            can_undo: true,
                            can_redo: false,
                        }),
                        draft: Some(recipe.clone()),
                        ..Default::default()
                    },
                );
                app.request_final_preview(&item.id, false);
                let _ = app.edited_preview(&item.id, "proof", source.clone());
                app.request_final_preview(&item.id, true);
                let reduced = Arc::new(
                    ImageLevels::from_source(
                        raster.clone(),
                        PreviewRequest {
                            edge: 512,
                            ..PreviewRequest::full()
                        },
                    )
                    .unwrap(),
                );
                assert!(app.edited_preview(&item.id, "proof", reduced).is_none());
                app.quality_overrides
                    .insert(item.id.clone(), PreviewQuality::Standard);
                assert_eq!(app.preview_request(&item, 0).quality, PreviewQuality::Full);
                let mut expected = raster.clone();
                recipe.apply(&mut expected).unwrap();
                tr_core::export::proof_srgb16(&mut expected);
                let expected = ImageLevels::from_source(expected, PreviewRequest::full()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    app.poll_edit_preview();
                    if let Some(actual) = app.edited_preview(&item.id, "proof", source.clone()) {
                        assert_eq!(actual.base_level(), 0);
                        assert!(actual.opaque()); // The almost-opaque case quantizes to opaque PNG16.
                        for (a, b) in actual.levels().iter().zip(expected.levels()) {
                            assert_eq!(a.pixels, b.pixels);
                        }
                        break;
                    }
                    assert!(Instant::now() < deadline, "Output proof did not converge");
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(source.source().pixels, raster.pixels);
                app.request_final_preview(&item.id, false);
                assert!(!app.output_proof_for(&item.id));
            }
        }
    }
    #[test]
    fn edited_preview_and_thumbnail_keep_reference_filters() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.state.current = Some(item.id.clone());
        app.editing.verify_final = Some(item.id.clone());
        let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.exposure_ev = 0.75;
        app.editing.entries.insert(
            item.id.clone(),
            editing::EditEntry {
                loaded: Some(tr_store::LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: "filters".into(),
                    generation: 1,
                    revision: 1,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe.clone()),
                ..Default::default()
            },
        );
        for opaque in [true, false] {
            let raster = tr_core::color::LinearImage::new(
                65,
                33,
                (0..65 * 33)
                    .map(|i| {
                        let alpha = if opaque { 1. } else { (i % 7) as f32 / 6. };
                        [
                            ((i % 11) as f32 / 13.) * alpha,
                            0.3 * alpha,
                            ((i % 17) as f32 / 19.) * alpha,
                            alpha,
                        ]
                    })
                    .collect(),
            )
            .unwrap();
            let source =
                Arc::new(ImageLevels::from_source(raster.clone(), PreviewRequest::full()).unwrap());
            let mut modified = raster;
            recipe.apply(&mut modified).unwrap();
            let reference = ImageLevels::from_source(modified, PreviewRequest::full()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                app.poll_edit_preview();
                let view = app.edited_preview(&item.id, "filters", source.clone());
                let thumb = app.edited_thumbnail(&item.id, "filters", source.clone());
                if let (Some(view), Some(thumb)) = (view, thumb) {
                    for actual in [view, thumb] {
                        for (a, b) in actual.levels().iter().zip(reference.levels()) {
                            assert_eq!(a.pixels, b.pixels, "Changed filter for opaque={opaque}");
                        }
                        assert_eq!(actual.opaque(), reference.opaque());
                    }
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    #[test]
    fn viewer_fallback_never_uses_another_raw_engine() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let engines: Vec<_> = tr_core::decoder::RawEngine::choices().collect();
        let request = PreviewRequest {
            raw_engine: engines[0],
            ..PreviewRequest::full()
        };
        let other = PreviewRequest {
            raw_engine: engines[1],
            ..request
        };
        let pyramid = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(2, 1, vec![[0.2, 0.3, 0.4, 1.]; 2]).unwrap(),
                request,
            )
            .unwrap(),
        );
        let cached = CachedImage {
            digest: item.digest.clone(),
            info: RasterInfo {
                scientific: None,
                reference_mip: None,
                width: 2,
                height: 1,
                source_width: 2,
                source_height: 1,
                native_bits: 32,
                format: "test".into(),
                decoder: "test".into(),
                input_color: "Rec2020".into(),
                filter: "reference".into(),
                orientation: "applied".into(),
            },
            histogram: pyramid.source().histogram(),
            pyramid,
            touched: 0,
            transport: "test",
            worker_pid: None,
        };
        app.cache.insert((item.id.clone(), other), cached.clone());
        assert!(app.viewer_fallback_key(&item, request).is_none());
        let compatible = PreviewRequest {
            edge: 512,
            ..request
        };
        app.cache.insert((item.id.clone(), compatible), cached);
        assert_eq!(
            app.viewer_fallback_key(&item, request),
            Some((item.id.clone(), compatible))
        );
    }
    #[test]
    fn comparison_keeps_both_edited_previews_ready() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let items = app.state.items[..2].to_vec();
        app.state.current = Some(items[0].id.clone());
        app.state.view = ViewMode::Compare;
        let sources: Vec<_> = [0.2, 0.3]
            .into_iter()
            .map(|value| {
                Arc::new(
                    ImageLevels::from_source(
                        tr_core::color::LinearImage::new(
                            16,
                            8,
                            vec![[value, value, value, 1.]; 128],
                        )
                        .unwrap(),
                        PreviewRequest::full(),
                    )
                    .unwrap(),
                )
            })
            .collect();
        for item in &items {
            let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
            recipe.exposure_ev = 1.;
            app.editing.entries.insert(
                item.id.clone(),
                editing::EditEntry {
                    loaded: Some(tr_store::LoadedEdit {
                        asset_id: item.id.clone(),
                        source_digest: "comparison".into(),
                        generation: 1,
                        revision: 1,
                        recipe: recipe.clone(),
                        can_undo: true,
                        can_redo: false,
                    }),
                    draft: Some(recipe),
                    ..Default::default()
                },
            );
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            app.poll_edit_preview();
            let a = app.edited_preview(&items[0].id, "comparison", sources[0].clone());
            let b = app.edited_preview(&items[1].id, "comparison", sources[1].clone());
            if let (Some(a), Some(b)) = (a, b) {
                assert_eq!(a.source().pixels[0], [0.4, 0.4, 0.4, 1.]);
                assert_eq!(b.source().pixels[0], [0.6, 0.6, 0.6, 1.]);
                assert!(
                    app.edited_preview(&items[0].id, "comparison", sources[0].clone())
                        .is_some()
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Both comparison edits must converge and stay ready"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn edited_preview_marks_reduced_work_and_can_converge_from_native_source() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.state.current = Some(item.id.clone());
        let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
        recipe.exposure_ev = 1.;
        app.editing.entries.insert(
            item.id.clone(),
            editing::EditEntry {
                loaded: Some(tr_store::LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: "synthetic".into(),
                    generation: 1,
                    revision: 1,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe),
                ..Default::default()
            },
        );
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage {
                    width: 2048,
                    height: 1,
                    pixels: vec![[0.25, 0.125, 0.5, 1.]; 2048],
                },
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        let wait = |app: &mut TrueRenderer| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.poll_edit_preview();
                if let Some(image) = app.edited_preview(&item.id, "synthetic", source.clone()) {
                    return image;
                }
                assert!(
                    Instant::now() < deadline,
                    "anteprima modificata non disponibile"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let quick = wait(&mut app);
        assert_eq!(quick.base_level(), 1);
        assert_eq!(quick.source().pixels[0][0], 0.5);
        app.editing.verify_final = Some(item.id.clone());
        app.clear_edit_preview();
        let final_image = wait(&mut app);
        assert_eq!(final_image.base_level(), 0);
        assert_eq!(final_image.source().pixels[0], [0.5, 0.25, 1., 1.]);
        // Request native verification while the quick result is still queued.
        app.editing.verify_final = None;
        app.clear_edit_preview();
        assert!(
            app.edited_preview(&item.id, "synthetic", source.clone())
                .is_none()
        );
        app.editing.verify_final = Some(item.id.clone());
        app.clear_edit_preview();
        assert_eq!(wait(&mut app).base_level(), 0);
    }

    #[test]
    fn visible_thumbnails_use_independent_bounded_edit_results() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let items = app.state.items[..2].to_vec();
        let sources: Vec<_> = [0.2, 0.3]
            .into_iter()
            .map(|red| {
                Arc::new(
                    ImageLevels::from_source(
                        tr_core::color::LinearImage {
                            width: 128,
                            height: 128,
                            pixels: vec![[red, 0.1, 0.05, 1.]; 128 * 128],
                        },
                        PreviewRequest::full(),
                    )
                    .unwrap(),
                )
            })
            .collect();
        for item in &items {
            let mut recipe = tr_core::editing::EditRecipe::neutral(app.cache_settings.raw_engine);
            recipe.exposure_ev = 1.;
            app.editing.entries.insert(
                item.id.clone(),
                editing::EditEntry {
                    loaded: Some(tr_store::LoadedEdit {
                        asset_id: item.id.clone(),
                        source_digest: "synthetic".into(),
                        generation: 1,
                        revision: 1,
                        recipe: recipe.clone(),
                        can_undo: true,
                        can_redo: false,
                    }),
                    draft: Some(recipe),
                    ..Default::default()
                },
            );
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let rendered = loop {
            for (item, source) in items.iter().zip(&sources) {
                app.edited_thumbnail(&item.id, "synthetic", source.clone());
            }
            std::thread::sleep(Duration::from_millis(5));
            app.poll_edit_preview();
            let ready: Option<Vec<_>> = items
                .iter()
                .zip(&sources)
                .map(|(item, source)| app.edited_thumbnail(&item.id, "synthetic", source.clone()))
                .collect();
            if let Some(ready) = ready {
                break ready;
            }
            assert!(
                Instant::now() < deadline,
                "miniature modificate non disponibili"
            );
        };
        for (source, image) in sources.iter().zip(&rendered) {
            assert_eq!(
                image.source().pixels[0][0],
                source.source().pixels[0][0] * 2.
            );
            assert_eq!(
                image.source().histogram(),
                app.edited_thumbnail_histogram(source.id()).unwrap()
            );
            assert_eq!(source.source().pixels[0][3], image.source().pixels[0][3]);
        }
        let first = &items[0].id;
        app.editing
            .entries
            .get_mut(first)
            .unwrap()
            .draft
            .as_mut()
            .unwrap()
            .exposure_ev = 2.;
        app.clear_edit_thumbnails(first);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll_edit_preview();
            if let Some(image) = app.edited_thumbnail(first, "synthetic", sources[0].clone()) {
                assert_eq!(
                    image.source().pixels[0][0],
                    sources[0].source().pixels[0][0] * 4.
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "miniatura aggiornata non disponibile"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            app.edited_thumbnail(&items[1].id, "synthetic", sources[1].clone())
                .unwrap()
                .source()
                .pixels[0][0],
            sources[1].source().pixels[0][0] * 2.
        );
    }

    #[test]
    fn large_retina_grid_requests_only_the_needed_thumbnail_bucket() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.state.view = ViewMode::Grid;
        app.demand.clear();
        ctx.set_pixels_per_point(2.);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(ui.ctx().pixels_per_point(), 2.);
            app.thumbnail(ui, &item, Vec2::new(288., 230.), true);
        });
        output.textures_delta.clear();
        assert!(app.demand.contains(&app.image_key(&item, 640)));
        assert!(!app.demand.contains(&app.image_key(&item, 1024)));
    }

    #[test]
    fn visible_thumbnails_release_large_ancestors_before_native_refinement() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.state.transform.zoom = Some(1.0);
        let full = (item.id.clone(), PreviewRequest::full());
        let thumb = (item.id.clone(), PreviewRequest { edge: 1, ..full.1 });
        let make_image = |size| {
            let pyramid = Arc::new(
                ImageLevels::from_source(
                    tr_core::color::LinearImage {
                        width: size,
                        height: size,
                        pixels: vec![[0.2, 0.3, 0.4, 1.]; (size * size) as usize],
                    },
                    PreviewRequest::full(),
                )
                .unwrap(),
            );
            CachedImage {
                digest: "test".into(),
                info: RasterInfo {
                    scientific: None,
                    reference_mip: None,
                    width: size,
                    height: size,
                    source_width: size,
                    source_height: size,
                    native_bits: 32,
                    format: "test".into(),
                    decoder: "test".into(),
                    input_color: "linear Rec2020".into(),
                    filter: "reference".into(),
                    orientation: "applied".into(),
                },
                histogram: pyramid.source().histogram(),
                pyramid,
                touched: app.frame_number,
                transport: "test",
                worker_pid: None,
            }
        };
        let large = make_image(4);
        let small = make_image(1);
        app.cache.insert(
            (
                item.id.clone(),
                PreviewRequest {
                    edge: 4096,
                    ..full.1
                },
            ),
            large.clone(),
        );
        app.cache.insert(full.clone(), large);
        app.service.cache.memory.configure(2048); // Scaled headless residency test.
        app.demand.insert(full.clone());
        app.release_large_ancestors();
        assert!(
            app.cache.contains_key(&full),
            "Keep the other demanded comparison pane"
        );
        app.demand.clear();
        app.demand.insert(thumb.clone());
        assert!(!app.prepare_view_detail());
        assert!(!app.cache.contains_key(&full));
        app.cache.insert(thumb.clone(), small);
        assert!(app.prepare_view_detail());
        assert!(app.cache.contains_key(&thumb));
        assert_eq!(app.state.transform.zoom, Some(1.0));
        app.cache.remove(&thumb);
        app.errors
            .insert(format!("{}:{:?}", thumb.0, thumb.1), "Invalid file".into());
        assert!(
            app.prepare_view_detail(),
            "A genuinely invalid neighbour must not block detail forever"
        );
    }

    #[test]
    fn photo_quality_is_bidirectional_independent_and_global_resets_overrides() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let first = app.state.items[0].clone();
        let second = app.state.items[1].clone();
        app.set_quality(PreviewQuality::Full);
        settle(&mut app, &ctx, false);
        app.set_photo_quality(&first.id, Some(PreviewQuality::Standard));
        assert_eq!(
            app.preview_request(&first, 512).quality,
            PreviewQuality::Standard
        );
        assert_eq!(app.quality(&second), PreviewQuality::Full);
        assert_eq!(app.service.cache.settings().quality, PreviewQuality::Full);
        app.set_photo_quality(&first.id, Some(PreviewQuality::Full));
        assert_eq!(app.quality(&first), PreviewQuality::Full);
        app.set_photo_quality(&first.id, None);
        assert!(!app.quality_overrides.contains_key(&first.id));
        app.cache_settings.disk_mib += 1024;
        let draft_disk = app.cache_settings.disk_mib;
        let selected = app.state.selected.clone();
        app.state.transform.zoom = Some(1.75);
        app.set_photo_quality(&second.id, Some(PreviewQuality::Full));
        app.set_quality(PreviewQuality::Standard);
        settle(&mut app, &ctx, false);
        assert!(app.quality_overrides.is_empty());
        assert_eq!(app.quality(&second), PreviewQuality::Standard);
        assert_eq!(app.cache_settings.disk_mib, draft_disk);
        assert_ne!(app.service.cache.settings().disk_mib, draft_disk);
        assert_eq!(app.state.selected, selected);
        assert_eq!(app.state.transform.zoom, Some(1.75));
        app.full_for_current();
        assert_eq!(
            app.quality(app.state.current_item().unwrap()),
            PreviewQuality::Full
        );
        assert_eq!(
            app.service.cache.settings().quality,
            PreviewQuality::Standard
        );
    }

    fn chrome_frame(
        app: &mut TrueRenderer,
        ctx: &egui::Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::epaint::Shape, result: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(text) => result.push((
                    text.galley.job.text.clone(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, result);
                    }
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ui| {
                app.keyboard(ui.ctx());
                app.toolbar(ui);
                app.footer(ui);
                app.settings_window(ui.ctx());
            },
        );
        output.textures_delta.clear();
        let mut text = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn chrome_is_aligned_tools_are_above_and_folder_is_below_in_both_languages() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.state.view = ViewMode::Preview;
        for lang in [Language::English, Language::Italian] {
            app.set_language(lang);
            for size in [
                Vec2::new(1440., 940.),
                Vec2::new(1100., 720.),
                Vec2::new(550., 360.),
            ] {
                let mut text = Vec::new();
                for _ in 0..20 {
                    text = chrome_frame(&mut app, &ctx, size, Vec::new());
                }
                let rect = |label: &str| {
                    text.iter()
                        .find(|(s, _)| s == label)
                        .unwrap_or_else(|| panic!("Missing {label}: {text:?}"))
                        .1
                };
                assert!(
                    (rect(lang.text("Apri")).center().y
                        - rect(lang.text("Impostazioni")).center().y)
                        .abs()
                        < 1.
                );
                for label in [
                    format!("{}: Standard", lang.text("Globale")),
                    format!("{}: Standard", lang.text("Solo questa foto")),
                    "1:1".to_owned(),
                    if cfg!(target_os = "macos") {
                        "RAW: Apple RAW"
                    } else {
                        "RAW: LibRaw bilinear"
                    }
                    .to_owned(),
                ] {
                    let r = rect(&label);
                    assert!(
                        r.bottom() < size.y / 2. && r.left() >= 0. && r.right() <= size.x,
                        "Toolbar overflow: {label}, {size:?}: {r:?}"
                    );
                }
                assert!(rect(lang.text("Corpus di prova")).top() > size.y - 100.);
            }
        }
    }

    #[test]
    fn menus_and_preferences_are_reachable_by_click_without_losing_drafts() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.state.view = ViewMode::Preview;
        let size = Vec2::new(1440., 940.);
        let click = |app: &mut TrueRenderer, label: &str| {
            let mut text = Vec::new();
            for _ in 0..10 {
                text = chrome_frame(app, &ctx, size, Vec::new());
            }
            let pos = text
                .iter()
                .find(|(s, _)| s == label)
                .unwrap_or_else(|| panic!("Missing {label}: {text:?}"))
                .1
                .center();
            for pressed in [true, false] {
                chrome_frame(
                    app,
                    &ctx,
                    size,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            chrome_frame(app, &ctx, size, Vec::new())
        };
        let open = click(&mut app, "Open");
        for label in ["Open folder…", "Open file…", "Refresh folder"] {
            assert!(open.iter().any(|(s, _)| s == label));
        }
        let menu = click(&mut app, "Menu");
        for label in [
            "Language",
            "Show inspector",
            "Create backup",
            "Export annotations…",
        ] {
            assert!(
                menu.iter().any(|(s, _)| s == label),
                "Missing {label}: {menu:?}"
            );
        }
        chrome_frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(app.state.view, ViewMode::Preview);
        assert!(!egui::Popup::is_any_open(&ctx));
        click(&mut app, "Settings");
        assert!(app.show_settings);
        app.cache_settings.disk_mib += 1024;
        let draft = app.cache_settings.clone();
        for title in [
            "General",
            "Previews and RAW",
            "Performance",
            "Cache and data",
            "Settings",
        ] {
            click(&mut app, title);
            assert_eq!(app.cache_settings, draft);
        }
    }

    #[test]
    fn both_languages_render_all_preferences_with_visible_footer_at_small_sizes() {
        fn text(
            shape: &egui::epaint::Shape,
            clip: egui::Rect,
            result: &mut Vec<(String, egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::epaint::Shape::Text(text) => {
                    result.push((
                        text.galley.job.text.clone(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                        clip,
                    ));
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, clip, result);
                    }
                }
                _ => {}
            }
        }
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        for language in [Language::English, Language::Italian] {
            app.set_language(language);
            app.show_settings = true;
            app.cache_settings.disk_mib += 1;
            let draft = app.cache_settings.clone();
            for size in [
                egui::vec2(1440., 940.),
                egui::vec2(1100., 720.),
                egui::vec2(550., 360.),
            ] {
                for (page, expected) in [
                    (SettingsPage::General, "Lingua"),
                    (SettingsPage::Previews, "Motore RAW"),
                    (SettingsPage::Performance, "Prestazioni"),
                    (SettingsPage::Cache, "Cache e dati"),
                ] {
                    app.settings_page = page;
                    let mut rendered = Vec::new();
                    // Let egui's window sizing and scroll-bar animation converge after each resize.
                    for _ in 0..20 {
                        let input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        };
                        let mut output = ctx.run_ui(input, |ui| {
                            app.toolbar(ui);
                            app.footer(ui);
                            app.settings_window(ui.ctx());
                        });
                        output.textures_delta.clear(); // headless: no texture backend
                        rendered.clear();
                        for shape in output.shapes {
                            text(&shape.shape, shape.clip_rect, &mut rendered);
                        }
                    }
                    for label in ["Apri", "Menu", expected, "Chiudi", "Applica e salva"] {
                        let label = language.text(label);
                        assert!(
                            rendered.iter().any(|(s, _, _)| s == label),
                            "Missing {label}, {page:?}: {rendered:?}"
                        );
                    }
                    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    for label in ["Chiudi", "Applica e salva", "Ripristina modifiche"] {
                        let label = language.text(label);
                        let (_, rect, clip) = rendered.iter().find(|(s, _, _)| s == label).unwrap();
                        assert!(
                            viewport.contains_rect(*rect) && clip.contains_rect(*rect),
                            "Clipped {label}, {page:?}, {size:?}: {rect:?}, clip {clip:?}"
                        );
                    }
                    assert_eq!(
                        app.cache_settings, draft,
                        "Rendering or changing tabs altered the draft"
                    );
                    assert_ne!(app.service.cache.settings().disk_mib, draft.disk_mib);
                }
            }
        }
    }

    #[test]
    fn language_menu_persists_without_applying_drafts_or_resetting_the_view() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let id = app.state.items[1].id.clone();
        app.command(Command::Select { id, extend: false });
        app.state.transform.zoom = Some(1.75);
        let selected = app.state.selected.clone();
        let generation = app.generation;
        let live_disk = app.service.cache.settings().disk_mib;
        app.cache_settings.disk_mib = live_disk + 1024; // unsaved preferences
        for language in [Language::Italian, Language::English] {
            app.set_language(language);
            assert_eq!(app.cache_settings.language, language);
            let saved = crate::cache::Settings::load(&app.settings_data).unwrap();
            assert_eq!(saved.language, language);
            assert_eq!(saved.disk_mib, live_disk);
            assert_eq!(app.cache_settings.disk_mib, live_disk + 1024);
            assert_eq!(app.state.selected, selected);
            assert_eq!(app.state.transform.zoom, Some(1.75));
            assert_eq!(app.generation, generation);
        }
        // Failed persistence must not claim the new language is active or saved.
        app.settings_data = app.settings_data.join("settings.json");
        app.set_language(Language::Italian);
        assert_eq!(app.cache_settings.language, Language::English);
        assert_eq!(app.service.cache.settings().language, Language::English);
        assert!(app.status.starts_with("Salvataggio lingua fallito:"));
    }

    #[test]
    fn full_settings_action_preserves_nonfirst_selection_and_zoom() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        assert_eq!(app.state.items.len(), 2);
        let id = app.state.items[1].id.clone();
        app.command(Command::Select { id, extend: true });
        app.state.transform = ViewTransform {
            zoom: Some(1.75),
            center: [0.4, 0.6],
        };
        let selected = app.state.selected.clone();
        let current = app.state.current.clone();
        let original = app.cache_settings.raw_engine;
        let other = tr_core::decoder::RawEngine::choices()
            .find(|e| *e != original)
            .unwrap();
        for engine in [other, original] {
            app.cache_settings.raw_engine = engine;
            let generation = app.generation;
            app.start_cache_action(true); // exact action used by Applica e salva
            assert!(!app.scanning, "Saving preferences must not invent a scan");
            assert_eq!(app.generation, generation + 1);
            settle(&mut app, &ctx, true);
            assert_eq!(app.state.current, current);
            assert_eq!(app.state.selected, selected);
            assert_eq!(app.state.transform.zoom, Some(1.75));
            assert_eq!(app.state.transform.center, [0.4, 0.6]);
            assert_eq!(
                crate::cache::Settings::load(&app.settings_data)
                    .unwrap()
                    .raw_engine,
                engine
            );
        }
        // Clearing a cache is also maintenance, with no catalogue reset.
        app.start_cache_action(false);
        assert!(!app.scanning);
        settle(&mut app, &ctx, true);
        assert_eq!(app.state.current, current);
        assert_eq!(app.state.transform.zoom, Some(1.75));
    }

    #[test]
    fn finishing_maintenance_does_not_cancel_a_real_pending_scan() {
        let (_dir, ctx, mut app) = app();
        assert!(app.scanning); // initial scan queued, no event consumed yet
        let generation = app.generation;
        app.cache_settings.raw_engine = tr_core::decoder::RawEngine::choices()
            .find(|e| *e != app.cache_settings.raw_engine)
            .unwrap();
        app.start_cache_action(true);
        assert_eq!(app.generation, generation + 1);
        settle(&mut app, &ctx, false); // consume maintenance only
        assert!(app.scanning, "A pending scan must await its own result");
        settle(&mut app, &ctx, true);
        assert_eq!(app.state.items.len(), 2);
        assert!(!app.scanning);
    }
}
