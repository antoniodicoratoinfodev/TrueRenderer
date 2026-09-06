use crate::service::{Event, Request, Service};
use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions, Vec2};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tr_app::{Command, Effect, State};
use tr_core::{Item, Label, ViewMode, ViewTransform, color::LinearImage, protocol::RasterInfo};

const TEXT: Color32 = Color32::from_rgb(232, 232, 232);
const MUTED: Color32 = Color32::from_rgb(154, 154, 154);
const AMBER: Color32 = Color32::from_rgb(217, 164, 65);
const SURFACE: Color32 = Color32::from_rgb(38, 38, 38);

struct CachedImage {
    texture: TextureHandle,
    info: RasterInfo,
    histogram: [[u32; 256]; 3],
    raster: Option<Arc<LinearImage>>,
    touched: u64,
    transport: &'static str,
    worker_pid: Option<u32>,
}
pub struct TrueRenderer {
    state: State,
    service: Service,
    root: PathBuf,
    folder: PathBuf,
    generation: u64,
    cache: HashMap<(String, u32), CachedImage>,
    pending_images: HashSet<(String, u32)>,
    errors: HashMap<String, String>,
    cell_size: f32,
    status: String,
    scanning: bool,
    keyword_text: String,
    keyword_id: String,
    undo_available: bool,
    show_help: bool,
    show_inspector: bool,
    show_filmstrip: bool,
    fullscreen: bool,
    adapter: String,
    surface: String,
    frame_number: u64,
    sample: Option<tr_render::Sample>,
    smoke: bool,
    started: Instant,
    smoke_stage: u8,
    screenshots: HashSet<String>,
    fatal: bool,
    closing: bool,
    search_focus: bool,
    gpu_rx: std::sync::mpsc::Receiver<Result<tr_render::diagnostic::GpuCheck, String>>,
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
        smoke: bool,
    ) -> Self {
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
        let service = Service::start(root.clone(), data, worker, ctx.clone());
        let (gpu_tx, gpu_rx) = std::sync::mpsc::sync_channel(1);
        if let Some(gpu) = &cc.wgpu_render_state {
            let device = gpu.device.clone();
            let queue = gpu.queue.clone();
            let ctx = ctx.clone();
            let report_root = root.clone();
            std::thread::spawn(move || {
                let result =
                    tr_render::diagnostic::check(&device, &queue).map_err(|e| format!("{e:#}"));
                let report = match &result {
                    Ok(c) => {
                        serde_json::json!({"samples":c.samples,"maximum_absolute_error":c.max_error,"threshold":0.0001,"passed":c.passed,"scope":"Rec.2020 fp32 -> opaque encoded sRGB; arithmetic only, no display/ICC/LUT qualification"})
                    }
                    Err(e) => serde_json::json!({"passed":false,"error":e}),
                };
                let _ = std::fs::write(
                    report_root.join("reports/gpu-diagnostic.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
                let _ = gpu_tx.send(result);
                ctx.request_repaint();
            });
        }
        let folder = root.join("corpus");
        let mut app = Self {
            state: State::default(),
            service,
            root,
            folder: folder.clone(),
            generation: 0,
            cache: HashMap::new(),
            pending_images: HashSet::new(),
            errors: HashMap::new(),
            cell_size: 206.,
            status: "Avvio del motore…".into(),
            scanning: true,
            keyword_text: String::new(),
            keyword_id: String::new(),
            undo_available: false,
            show_help: false,
            show_inspector: true,
            show_filmstrip: true,
            fullscreen: false,
            adapter,
            surface,
            frame_number: 0,
            sample: None,
            smoke,
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
        app.open_folder(folder);
        app
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
        self.generation += 1;
        self.service
            .generation
            .store(self.generation, Ordering::Relaxed);
        self.cache.clear();
        self.pending_images.clear();
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
    fn ensure_image(&mut self, item: &Item, edge: u32) {
        if !item.approved {
            return;
        }
        let key = (item.id.clone(), edge);
        if self.cache.contains_key(&key) {
            if let Some(c) = self.cache.get_mut(&key) {
                c.touched = self.frame_number;
            }
            return;
        }
        if self.pending_images.contains(&key) || self.errors.contains_key(&item.id) {
            return;
        }
        let request = Request::Decode {
            item: item.clone(),
            edge,
            generation: self.generation,
        };
        let tx = if edge == 0 {
            &self.service.high
        } else {
            &self.service.low
        };
        if tx.try_send(request).is_ok() {
            self.pending_images.insert(key);
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if let Ok(result) = self.gpu_rx.try_recv() {
            match result {
                Ok(check) => {
                    self.gpu_passed = check.passed;
                    self.gpu_status = format!(
                        "GPU/CPU: {} campioni · errore max {:.2e} · {}",
                        check.samples,
                        check.max_error,
                        if check.passed {
                            "entro soglia R0"
                        } else {
                            "fuori soglia"
                        }
                    );
                }
                Err(e) => self.gpu_status = format!("Diagnostica GPU non disponibile: {e}"),
            }
        }
        while let Ok(event) = self.service.events.try_recv() {
            match event {
                Event::DecodeDeferred {
                    id,
                    edge,
                    generation,
                } => {
                    if generation == self.generation {
                        self.pending_images.remove(&(id, edge));
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
                    self.folder = folder;
                    self.scanning = false;
                    self.status = note;
                }
                Event::Scanned { .. } => {}
                Event::Image {
                    id,
                    edge,
                    generation,
                    result,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.pending_images.remove(&(id.clone(), edge));
                    match *result {
                        Ok((decoded, prepared)) => {
                            let texture = ctx.load_texture(
                                format!("{id}:{edge}"),
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [
                                        decoded.raster.width as usize,
                                        decoded.raster.height as usize,
                                    ],
                                    &prepared.rgba,
                                ),
                                TextureOptions::NEAREST,
                            );
                            self.cache.insert(
                                (id, edge),
                                CachedImage {
                                    texture,
                                    info: decoded.info,
                                    histogram: prepared.histogram,
                                    raster: if edge == 0 {
                                        Some(Arc::new(decoded.raster))
                                    } else {
                                        None
                                    },
                                    touched: self.frame_number,
                                    transport: decoded.transport,
                                    worker_pid: decoded.worker_pid,
                                },
                            );
                            for (full, limit) in [(false, 64), (true, 3)] {
                                while self.cache.keys().filter(|(_, e)| (*e == 0) == full).count()
                                    > limit
                                {
                                    let key = self
                                        .cache
                                        .iter()
                                        .filter(|((_, e), _)| (*e == 0) == full)
                                        .min_by_key(|(_, v)| v.touched)
                                        .map(|(k, _)| k.clone());
                                    if let Some(k) = key {
                                        self.cache.remove(&k);
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            self.errors.insert(id, error);
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
                self.state.transform.set_zoom(1.);
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
                self.state.transform.set_zoom(1.);
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
                            "Elenca una cartella. In R0 sono decodificati solo i campioni inclusi.",
                        )
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.folder)
                            .pick_folder()
                    {
                        self.open_folder(path);
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
                    self.ensure_image(&item, 320);
                    let key = if self.cache.contains_key(&(item.id.clone(), 0)) {
                        (item.id.clone(), 0)
                    } else {
                        (item.id.clone(), 320)
                    };
                    let info = self.cache.get(&key).map(|c| c.info.clone());
                    if let Some(cached) = self.cache.get(&key) {
                        ui.add(
                            egui::Image::new(&cached.texture).fit_to_exact_size(Vec2::new(
                                ui.available_width(),
                                ui.available_width() * cached.info.height as f32
                                    / cached.info.width as f32,
                            )),
                        );
                        ui.add_space(16.);
                        tr_render::histogram(ui, &cached.histogram);
                        ui.label(
                            RichText::new("Istogramma · uscita sRGB 8 bit composita")
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
                            .map(|i| format!("{} · {} bit/canale", i.format, i.native_bits))
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
                    }
                    if let Some(info) = info {
                        field(ui, "Decoder", &info.decoder);
                        field(ui, "Riduzione", &info.filter);
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
        self.ensure_image(item, 320);
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
        if let Some(cache) = self.cache.get(&(item.id.clone(), 320)) {
            let ratio = (area.width() / cache.info.width as f32)
                .min(area.height() / cache.info.height as f32);
            let image_rect = egui::Rect::from_center_size(
                area.center(),
                Vec2::new(
                    cache.info.width as f32 * ratio,
                    cache.info.height as f32 * ratio,
                ),
            );
            painter.image(
                cache.texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        } else {
            let text = if self.errors.contains_key(&item.id) {
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
                    "PNG del corpus R0"
                } else {
                    "Archivio esterno: solo elenco in R0"
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
        self.ensure_image(&item, 0);
        ui.horizontal(|ui| {
            if ui.button("Adatta").clicked() {
                self.state.transform = ViewTransform::default();
            }
            if ui
                .button("1:1")
                .on_hover_text("Un pixel sorgente per pixel fisico dello schermo")
                .clicked()
            {
                self.state.transform.set_zoom(1.);
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
                ui.label(RichText::new("ANTEPRIMA · CPU").small().color(AMBER));
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
                self.ensure_image(&second, 0);
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
        if let Some(c) = self.cache.get(&(item.id.clone(), 0))
            && let Some(raster) = &c.raster
        {
            let (response, sample) =
                tr_render::viewport(ui, &c.texture, raster, &mut self.state.transform, id);
            self.image_focus_ids.insert(response.id);
            if sample.is_some() {
                self.sample_source = item.name.clone();
                self.sample = sample;
            }
        } else {
            egui::Frame::new().fill(Color32::from_gray(119)).show(ui,|ui|{
                ui.set_min_size(ui.available_size());ui.centered_and_justified(|ui|{
                    ui.label(self.errors.get(&item.id).map(String::as_str).unwrap_or(if item.approved{"Preparazione dell'immagine…"}else{"Anteprima non abilitata per file esterni in questa fase R0.\nApri il Corpus di prova per usare il viewer."}));
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
            ui.add_space(8.);ui.label("Le anteprime sono disponibili per il corpus incluso. Nel bundle macOS due servizi isolati gestiscono la decodifica, con controllo di timeout e memoria. Le cartelle esterne possono essere elencate; le loro anteprime attendono la qualifica completa della sicurezza.");
            ui.add_space(8.);ui.label("Restano da qualificare: XPC/App Sandbox e Windows, ICC/Little CMS, presentazione sul monitor, filtri e CPU/GPU, accessibilità e prestazioni. JPEG/TIFF, RAW, XMP e gigapixel seguono la roadmap. Il badge rimane Anteprima.");
            ui.add_space(8.);ui.monospace(format!("GPU: {}\nSuperficie: {}\nSQLite: {}",self.adapter,self.surface,tr_store::sqlite_version()));
            ui.label(&self.gpu_status);
            ui.separator();
            for (key,action) in [("0–5 / X","Valuta / scarta"),("6–9","Etichette rosso, giallo, verde, blu"),("G / E / C","Griglia / anteprima / confronto"),("Z / Cmd+1","Adatta o pixel fisici 1:1"),("Frecce / trascina / rotella","Naviga / pan / zoom"),("Cmd+F / Cmd+Z","Ricerca / annulla modifica"),("I / T / F / Esc","Pannello / miniature / schermo intero / griglia")]{field(ui,key,action);}
            ui.label(RichText::new("Su Windows usare Ctrl al posto di Cmd. Le scorciatoie non agiscono mentre scrivi in un campo.").small().color(MUTED));
            ui.add_space(8.);ui.label(format!("Progetto e piano: {}",self.root.display()));
        });
    }
    fn smoke_tick(&mut self, ctx: &egui::Context) {
        if !self.smoke {
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        if self.smoke_stage == 0 && self.cache.keys().filter(|(_, edge)| *edge == 320).count() >= 12
        {
            self.smoke_stage = 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "01-grid".to_string(),
            )));
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
            && self
                .state
                .current
                .as_ref()
                .is_some_and(|id| self.cache.contains_key(&(id.clone(), 0)))
        {
            self.smoke_stage = 3;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "02-preview".to_string(),
            )));
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
            && self
                .state
                .selected
                .iter()
                .all(|id| self.cache.contains_key(&(id.clone(), 0)))
        {
            self.smoke_stage = 5;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "03-compare".to_string(),
            )));
        } else if (self.smoke_stage == 5 && self.screenshots.len() == 3)
            || elapsed > 55.
            || self.fatal
        {
            let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":self.smoke_stage==5&&self.screenshots.len()==3&&!self.fatal&&self.errors.is_empty()&&self.gpu_passed,"adapter":self.adapter,"surface":self.surface,"gpu":self.gpu_status,"sqlite":tr_store::sqlite_version(),"worker_pids":self.cache.values().filter_map(|c| c.worker_pid).collect::<std::collections::BTreeSet<_>>(),"worker_transports":self.cache.values().map(|c| c.transport).collect::<std::collections::BTreeSet<_>>(),"frames":self.frame_number,"elapsed_seconds":elapsed,"images":self.state.items.len(),"screenshots":self.screenshots,"decode_errors":self.errors,"status":self.status,"scope":"native macOS R0 corpus smoke; not color/display/sandbox qualification"});
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
        self.smoke_tick(&ctx);
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
            let folder = if path.is_dir() {
                path.clone()
            } else {
                path.parent().unwrap_or(path).to_path_buf()
            };
            self.open_folder(folder);
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
        ui.label(RichText::new(value).small());
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
