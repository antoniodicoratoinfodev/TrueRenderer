use super::*;
use crate::{
    browser_session::{PanelMode, Session},
    filesystem_browser::BrowserEvent,
};
use tr_app::browser::{Browser, EntryFilter, Kind, ListingState};
use tr_core::location::{Favorite, Location};

fn entry_icon(painter: &egui::Painter, rect: egui::Rect, kind: Kind, expanded: bool) {
    let color = match kind {
        Kind::Directory | Kind::Package => egui::Color32::from_rgb(195, 170, 112),
        Kind::Image => egui::Color32::from_rgb(125, 173, 159),
        Kind::Link => egui::Color32::from_rgb(125, 166, 203),
        _ => egui::Color32::from_gray(150),
    };
    let stroke = egui::Stroke::new(1., color);
    let p = |x: f32, y: f32| rect.min + egui::vec2(x, y);
    if matches!(kind, Kind::Directory | Kind::Package) {
        painter.add(egui::Shape::closed_line(
            vec![
                p(1., 3.),
                p(5., 3.),
                p(7., 5.),
                p(13., 5.),
                p(13., 12.),
                p(1., 12.),
            ],
            stroke,
        ));
        if expanded {
            painter.add(egui::Shape::line(
                vec![p(1., 12.), p(3., 7.), p(14., 7.), p(12., 12.)],
                stroke,
            ));
        }
    } else {
        painter.add(egui::Shape::closed_line(
            vec![p(3., 1.), p(9., 1.), p(12., 4.), p(12., 13.), p(3., 13.)],
            stroke,
        ));
        painter.add(egui::Shape::line(
            vec![p(9., 1.), p(9., 4.), p(12., 4.)],
            stroke,
        ));
        if kind == Kind::Image {
            painter.circle_filled(p(6., 6.), 1., color);
            painter.add(egui::Shape::line(
                vec![p(4., 11.), p(7., 8.), p(9., 10.), p(11., 8.)],
                stroke,
            ));
        } else {
            painter.line_segment([p(5., 7.), p(10., 7.)], stroke);
            painter.line_segment([p(5., 10.), p(9., 10.)], stroke);
        }
    }
}

#[derive(Clone)]
enum DragLocation {
    Favorite(String),
    Folder(PathBuf),
}

#[derive(Clone)]
struct ReturnContext {
    path: PathBuf,
    current: Option<String>,
    selected: std::collections::BTreeSet<String>,
    view: ViewMode,
    transform: ViewTransform,
    observation: Option<String>,
    query: String,
    minimum_rating: i8,
    rejected_only: bool,
    label_filter: Option<Label>,
}

pub(super) struct Explorer {
    pub model: Browser,
    pub session: Session,
    pub saved: Session,
    saved_at: Instant,
    pub temporary: bool,
    pub favorites: Vec<Favorite>,
    pub favorite_pending: bool,
    rename: Option<(String, String)>,
    sequence: u64,
    pub scan: u64,
    pub pending: Option<u64>,
    pub scan_running: bool,
    pending_view: Option<ViewMode>,
    history: Vec<ReturnContext>,
    cursor: Option<usize>,
    going: Option<usize>,
    restore: Option<ReturnContext>,
    pub refreshing: bool,
    pub path_edit: bool,
    path_text: String,
    pub focus_tree: bool,
    typeahead: String,
    typed_at: Instant,
    last_refresh: Instant,
    was_focused: bool,
    restored_expansions: bool,
    scroll_to_focus: bool,
    smoke_at: Instant,
    tree_rect: egui::Rect,
    smoke_layouts: Vec<serde_json::Value>,
}
impl Explorer {
    pub fn new(data: &std::path::Path) -> Self {
        let (session, _) = Session::load(data);
        let mut model = Browser {
            show_hidden: session.show_hidden,
            filter: session.name_filter.clone(),
            entry_filter: match session.entry_filter {
                1 => EntryFilter::Images,
                2 => EntryFilter::Directories,
                _ => EntryFilter::All,
            },
            ..Default::default()
        };
        for path in session.roots.iter().filter_map(Location::path) {
            model.add_root(path);
        }
        Self {
            saved: session.clone(),
            session,
            model,
            saved_at: Instant::now(),
            temporary: false,
            favorites: vec![],
            favorite_pending: false,
            rename: None,
            sequence: 0,
            scan: 0,
            pending: None,
            scan_running: false,
            pending_view: None,
            history: vec![],
            cursor: None,
            going: None,
            restore: None,
            refreshing: false,
            path_edit: false,
            path_text: String::new(),
            focus_tree: false,
            typeahead: String::new(),
            typed_at: Instant::now(),
            last_refresh: Instant::now(),
            was_focused: true,
            restored_expansions: false,
            scroll_to_focus: false,
            smoke_at: Instant::now(),
            tree_rect: egui::Rect::NOTHING,
            smoke_layouts: vec![],
        }
    }
}

impl TrueRenderer {
    pub(super) fn filesystem_smoke(&mut self, ctx: &egui::Context) {
        if self.started.elapsed() > Duration::from_secs(90) || self.fatal {
            let report = serde_json::json!({"passed":false,"stage":self.smoke_stage,"fatal":self.fatal,"errors":self.errors});
            std::fs::write(
                self.root.join("reports/filesystem-ui.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            )
            .unwrap();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(50));
        if self.scanning || !self.presenter.is_idle() || self.state.items.is_empty() {
            return;
        }
        let configurations = [
            (
                Language::English,
                PanelMode::Explorer,
                ViewMode::Grid,
                [1440., 940.],
                1.,
            ),
            (
                Language::Italian,
                PanelMode::Explorer,
                ViewMode::Preview,
                [1100., 720.],
                1.,
            ),
            (
                Language::English,
                PanelMode::Library,
                ViewMode::Preview,
                [1440., 940.],
                1.,
            ),
            (
                Language::Italian,
                PanelMode::Explorer,
                ViewMode::Grid,
                [550., 360.],
                1.,
            ),
            (
                Language::English,
                PanelMode::Library,
                ViewMode::Grid,
                [550., 360.],
                1.,
            ),
            (
                Language::Italian,
                PanelMode::Explorer,
                ViewMode::Preview,
                [1100., 720.],
                2.,
            ),
        ];
        let index = self.smoke_stage as usize / 2;
        if index >= configurations.len() {
            let layout_ok = self.browser.smoke_layouts.len() == 6
                && self
                    .browser
                    .smoke_layouts
                    .iter()
                    .all(|v| v["passed"] == true);
            let report = serde_json::json!({"passed":layout_ok && self.screenshots.iter().filter(|s| s.starts_with("filesystem-")).count()==6 && self.errors.is_empty(),"screenshots":6,"layouts":self.browser.smoke_layouts,"gpu_verified":self.gpu_passed,"scope":"Native macOS generated corpus; Library/Explorer, grid/viewer, IT/EN, 1440x940, 1100x720, 550x360 and 200% UI. No screen-reader, Windows, cloud, removable-volume, NAS or statistical latency qualification."});
            std::fs::write(
                self.root.join("reports/filesystem-ui.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            )
            .unwrap();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let (language, panel, view, size, zoom) = configurations[index];
        let name = format!("filesystem-{index}");
        if self.smoke_stage.is_multiple_of(2) {
            self.set_language(language);
            self.show_inspector = false;
            self.state.view = view;
            self.panel_mode(panel, true);
            self.browser.temporary = size[0] / zoom < 1000.;
            self.browser.session.library_scroll = 0.;
            self.browser.session.explorer_scroll = 0.;
            ctx.set_zoom_factor(zoom);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                size[0], size[1],
            )));
            self.browser.smoke_at = Instant::now();
            self.smoke_stage += 1;
        } else if self.screenshots.contains(&name) {
            let tree = self.browser.tree_rect;
            let viewport = ctx.content_rect();
            self.browser.smoke_layouts.push(serde_json::json!({
                "case": index,
                "viewport": [viewport.width(), viewport.height()],
                "tree_height": tree.height(),
                "passed": panel != PanelMode::Explorer || (tree.height() >= 48. && viewport.contains_rect(tree)),
            }));
            self.smoke_stage += 1;
        } else if self.browser.smoke_at.elapsed() >= Duration::from_millis(800)
            && self.demand.iter().all(|key| self.cache.contains_key(key))
            && !self
                .browser
                .model
                .branches
                .values()
                .any(|b| b.state == ListingState::Loading)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(name)));
        }
    }
    fn return_context(&self) -> ReturnContext {
        ReturnContext {
            path: self.folder.clone(),
            current: self.state.current.clone(),
            selected: self.state.selected.clone(),
            view: self.state.view,
            transform: self.state.transform,
            observation: self.state.current_item().map(|i| i.observation.clone()),
            query: self.state.query.clone(),
            minimum_rating: self.state.minimum_rating,
            rejected_only: self.state.rejected_only,
            label_filter: self.state.label_filter,
        }
    }
    pub(super) fn navigate(&mut self, path: PathBuf, view: Option<ViewMode>) {
        self.browser.going = None;
        let path = if path.is_absolute() {
            path
        } else {
            self.folder.join(path)
        };
        self.browser.sequence += 1;
        let id = self.browser.sequence;
        if self.service.filesystem.client.resolve(id, path) {
            self.browser.pending = Some(id);
            self.browser.pending_view = view;
            self.scanning = true;
            self.status = "Apertura della posizione…".into();
        } else {
            self.status = "Coda filesystem occupata".into();
        }
    }
    fn history_go(&mut self, delta: isize) {
        let Some(cursor) = self.browser.cursor else {
            return;
        };
        let target = cursor as isize + delta;
        if target < 0 || target >= self.browser.history.len() as isize {
            return;
        }
        self.browser.history[cursor] = self.return_context();
        self.navigate(self.browser.history[target as usize].path.clone(), None);
        self.browser.going = Some(target as usize);
    }
    pub(super) fn refresh_folder(&mut self) {
        if self.browser.pending.is_some() {
            return;
        }
        self.browser.sequence += 1;
        self.browser.scan = self.browser.sequence;
        self.browser.refreshing = true;
        self.request(Request::ScanHidden(self.browser.model.show_hidden));
        self.scanning = self.request(Request::Scan {
            folder: self.folder.clone(),
            generation: self.browser.scan,
        });
        self.browser.scan_running = self.scanning;
        self.status = "Rilettura della cartella…".into();
    }
    fn select_path(&mut self, path: PathBuf, view: Option<ViewMode>) -> bool {
        let Some(id) = self
            .state
            .items
            .iter()
            .find(|i| i.path == path)
            .map(|i| i.id.clone())
        else {
            return false;
        };
        self.state.targeted = Some(id.clone());
        self.state.refilter();
        self.state.dispatch(Command::Select { id, extend: false });
        if let Some(view) = view {
            self.state.view = view;
        } else if self.state.view == ViewMode::Compare {
            self.state.view = ViewMode::Grid;
        }
        true
    }
    pub(super) fn select_pending_photo(&mut self) {
        if let Some(path) = self.pending_selection.clone()
            && self.select_path(path, self.browser.pending_view)
        {
            self.pending_selection = None;
        }
    }
    pub(super) fn browser_user_selection(&mut self) {
        self.pending_selection = None;
        self.browser.restore = None;
    }
    pub(super) fn finish_browser_scan(&mut self) {
        self.select_pending_photo();
        if self.pending_selection.take().is_some() {
            self.status = "La foto richiesta non è disponibile nell'elenco".into();
        }
        if let Some(context) = self.browser.restore.take() {
            self.state.current = context
                .current
                .filter(|id| self.state.items.iter().any(|i| &i.id == id));
            self.state.selected = context.selected;
            self.state.view = context.view;
            self.state.refilter();
            if self.state.current_item().map(|i| &i.observation) == context.observation.as_ref() {
                self.state.transform = context.transform;
            }
        }
        if self.state.current.is_none()
            && let Some(&index) = self.state.visible.first()
        {
            self.command(Command::Select {
                id: self.state.items[index].id.clone(),
                extend: false,
            });
        }
        self.browser.refreshing = false;
    }
    pub(super) fn poll_browser(&mut self, ctx: &egui::Context) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2) {
            let Ok(event) = self.service.filesystem.events.try_recv() else {
                break;
            };
            match event {
                BrowserEvent::Resolved { id, result } if self.browser.pending == Some(id) => {
                    self.browser.pending = None;
                    match result {
                        Err(error) => {
                            self.scanning = self.browser.scan_running;
                            self.browser.going = None;
                            self.status = format!("Posizione non disponibile: {error}");
                        }
                        Ok((folder, photo)) => {
                            self.browser.temporary = false;
                            if folder == self.folder
                                && self.browser.cursor.is_some()
                                && self.browser.going.is_none()
                            {
                                self.scanning = self.browser.scan_running;
                                if let Some(photo) = photo
                                    && !self.select_path(photo.clone(), self.browser.pending_view)
                                {
                                    self.pending_selection = Some(photo);
                                }
                                continue;
                            }
                            if let Some(cursor) = self.browser.cursor {
                                self.browser.history[cursor] = self.return_context();
                            }
                            if let Some(index) = self.browser.going.take() {
                                self.browser.cursor = Some(index);
                                let context = self.browser.history[index].clone();
                                self.state.query = context.query.clone();
                                self.state.minimum_rating = context.minimum_rating;
                                self.state.rejected_only = context.rejected_only;
                                self.state.label_filter = context.label_filter;
                                self.browser.restore = Some(context);
                            } else {
                                if let Some(cursor) = self.browser.cursor {
                                    self.browser.history.truncate(cursor + 1);
                                }
                                let mut context = self.return_context();
                                context.path = folder.clone();
                                self.browser.history.push(context);
                                if self.browser.history.len() > 100 {
                                    self.browser.history.remove(0);
                                }
                                self.browser.cursor = Some(self.browser.history.len() - 1);
                                self.browser.restore = None;
                            }
                            self.browser.scan = id;
                            self.pending_selection = photo;
                            self.state.view = self.browser.pending_view.unwrap_or(ViewMode::Grid);
                            self.browser.session.visit(&folder);
                            self.browser.model.add_root(folder.clone());
                            self.activate_folder(folder);
                            if self.browser.session.mode == PanelMode::Explorer {
                                self.expand_branch(self.folder.clone(), false);
                            }
                        }
                    }
                }
                BrowserEvent::Listing {
                    path,
                    epoch,
                    entries,
                    result,
                } => {
                    self.browser.model.accept(&path, epoch, entries, result);
                }
                BrowserEvent::Locations(paths) if !self.smoke => {
                    for path in paths {
                        self.browser.model.add_root(path);
                    }
                }
                BrowserEvent::Revealed(Err(error)) => self.status = error,
                _ => {}
            }
        }
        if start.elapsed() >= Duration::from_millis(2) {
            ctx.request_repaint();
        }
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        // Bounded polling fallback on the active location. No recursive watchers,
        // no filesystem calls in the frame, paused while the window is inactive.
        if focused
            && (!self.browser.was_focused
                || self.browser.last_refresh.elapsed() >= Duration::from_secs(15))
            && !self.scanning
            && !self.smoke
        {
            self.refresh_folder();
            let branches: Vec<_> = self
                .browser
                .model
                .branches
                .iter()
                .filter(|(_, b)| b.expanded && b.state != ListingState::Loading)
                .take(16)
                .map(|(p, _)| p.clone())
                .collect();
            for path in branches {
                self.expand_branch(path, true);
            }
            self.service.filesystem.client.locations();
            self.browser.last_refresh = Instant::now();
        }
        self.browser.was_focused = focused;
        if focused {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if self.browser.saved_at.elapsed() >= Duration::from_secs(1) {
            self.persist_browser();
            self.browser.saved_at = Instant::now();
        }
    }
    pub(super) fn persist_browser(&mut self) {
        self.browser.session.show_hidden = self.browser.model.show_hidden;
        self.browser.session.name_filter = self.browser.model.filter.clone();
        self.browser.session.entry_filter = match self.browser.model.entry_filter {
            EntryFilter::All => 0,
            EntryFilter::Images => 1,
            EntryFilter::Directories => 2,
        };
        self.browser.session.roots = self
            .browser
            .model
            .roots
            .iter()
            .take(128)
            .map(|e| Location::from_path(&e.path))
            .collect();
        self.browser.session.expanded = self
            .browser
            .model
            .branches
            .iter()
            .filter(|(_, b)| b.expanded)
            .take(128)
            .map(|(p, _)| Location::from_path(p))
            .collect();
        self.browser
            .session
            .expanded
            .sort_by(|a, b| a.bytes.cmp(&b.bytes));
        if self.browser.session != self.browser.saved {
            self.request(Request::BrowserSession(self.browser.session.clone()));
        }
    }
    fn expand_branch(&mut self, path: PathBuf, refresh: bool) {
        if self
            .browser
            .model
            .rows
            .iter()
            .any(|r| r.entry.path == path && r.depth >= 128)
        {
            self.status = "Limite profondità: apri la cartella come nuova posizione".into();
            return;
        }
        if !refresh
            && let Some(branch) = self.browser.model.branches.get_mut(&path)
            && matches!(branch.state, ListingState::Ready | ListingState::Loading)
        {
            branch.expanded = true;
            self.browser.model.rebuild();
            return;
        }
        self.browser.model.evict_collapsed();
        let epoch = self.browser.model.begin(path.clone());
        if !self
            .service
            .filesystem
            .client
            .list(path.clone(), epoch, self.browser.model.show_hidden)
        {
            self.browser.model.accept(
                &path,
                epoch,
                vec![],
                Some(Err("Coda filesystem occupata".into())),
            );
        }
    }
    fn toggle_branch(&mut self, path: PathBuf) {
        if self
            .browser
            .model
            .branches
            .get(&path)
            .is_some_and(|b| b.expanded)
        {
            self.service.filesystem.client.cancel_list(path.clone());
            self.browser.model.collapse(&path);
        } else {
            self.expand_branch(path, false);
        }
    }
    pub(super) fn panel_mode(&mut self, mode: PanelMode, reveal: bool) {
        self.browser.session.mode = mode;
        if reveal {
            self.browser.session.visible = true;
            if self.context.content_rect().width() < 1000. {
                self.browser.temporary = true;
            }
        }
        if mode == PanelMode::Explorer {
            self.service.filesystem.client.locations();
            self.browser.model.add_root(self.folder.clone());
            self.expand_branch(self.folder.clone(), false);
            if !self.browser.restored_expansions {
                self.browser.restored_expansions = true;
                // Saved expansions are hints: activate only roots now, descendants
                // remain lazy until their parent is explicitly explored.
                let roots: Vec<_> = self
                    .browser
                    .session
                    .expanded
                    .iter()
                    .filter_map(Location::path)
                    .filter(|p| self.browser.model.roots.iter().any(|e| &e.path == p))
                    .take(16)
                    .collect();
                for path in roots {
                    self.expand_branch(path, false);
                }
            }
        }
    }
    pub(super) fn left_panel_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        // Dense, flat sidebar chrome, independent of image and toolbar styling.
        ui.spacing_mut().item_spacing = egui::vec2(6., 4.);
        ui.spacing_mut().button_padding = egui::vec2(6., 3.);
        ui.spacing_mut().interact_size.y = 22.;
        {
            let widget = &mut ui.visuals_mut().widgets.inactive;
            widget.bg_stroke = egui::Stroke::NONE;
            widget.corner_radius = 2.into();
        }
        ui.visuals_mut().selection.bg_fill = egui::Color32::from_rgb(43, 65, 83);
        ui.visuals_mut().selection.stroke =
            egui::Stroke::new(1., egui::Color32::from_rgb(100, 166, 215));
        ui.horizontal(|ui| {
            for (mode, title) in [
                (PanelMode::Library, "Libreria"),
                (PanelMode::Explorer, "Esplora"),
            ] {
                let response =
                    ui.selectable_label(self.browser.session.mode == mode, lang.text(title));
                if self.browser.session.mode == mode {
                    ui.painter().line_segment(
                        [response.rect.left_bottom(), response.rect.right_bottom()],
                        egui::Stroke::new(2., egui::Color32::from_rgb(100, 166, 215)),
                    );
                }
                if response.clicked()
                    || (response.has_focus()
                        && ui.input(|i| {
                            i.key_pressed(egui::Key::ArrowLeft)
                                || i.key_pressed(egui::Key::ArrowRight)
                        }))
                {
                    let selected = if response.clicked() {
                        mode
                    } else if mode == PanelMode::Library {
                        PanelMode::Explorer
                    } else {
                        PanelMode::Library
                    };
                    self.panel_mode(selected, false);
                }
            }
        });
        ui.separator();
        match self.browser.session.mode {
            PanelMode::Library => {
                let response = egui::ScrollArea::vertical()
                    .id_salt("library-scroll")
                    .vertical_scroll_offset(self.browser.session.library_scroll)
                    .show(ui, |ui| self.navigation_contents(ui));
                self.browser.session.library_scroll = response.state.offset.y;
            }
            PanelMode::Explorer => self.explorer_contents(ui),
        }
    }
    pub(super) fn temporary_panel(&mut self, ctx: &egui::Context) {
        if !self.browser.temporary || self.show_settings {
            return;
        }
        let lang = self.cache_settings.language;
        let mut open = true;
        egui::Window::new(lang.text("Navigazione"))
            .id(egui::Id::new("temporary-navigation"))
            .open(&mut open)
            .collapsible(false)
            .fixed_pos(ctx.content_rect().min + egui::vec2(8., 8.))
            .fixed_size(egui::vec2(
                (ctx.content_rect().width() - 32.).clamp(200., 340.),
                (ctx.content_rect().height() - 64.).max(220.),
            ))
            .show(ctx, |ui| self.left_panel_contents(ui));
        if !open {
            self.browser.temporary = false;
        }
    }
    fn favorite_edit(&mut self, edit: tr_store::FavoriteEdit) {
        if !self.browser.favorite_pending {
            self.browser.favorite_pending = self.request(Request::Favorite(edit));
        }
    }
    fn pin(&mut self, path: PathBuf) {
        self.favorite_edit(tr_store::FavoriteEdit::Add {
            label: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .chars()
                .take(256)
                .collect(),
            location: Location::from_path(&path),
        });
    }
    fn explorer_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.browser.model.filter)
                    .char_limit(256)
                    .hint_text(lang.text("Filtra nomi nei rami caricati")),
            )
            .changed()
        {
            self.browser.model.rebuild();
        }
        ui.horizontal(|ui| {
            ui.menu_button(lang.text("Mostra"), |ui| {
                let previous = self.browser.model.entry_filter;
                for (value, text) in [
                    (EntryFilter::All, "Tutti i file"),
                    (EntryFilter::Images, "Cartelle e immagini"),
                    (EntryFilter::Directories, "Solo cartelle"),
                ] {
                    ui.selectable_value(
                        &mut self.browser.model.entry_filter,
                        value,
                        lang.text(text),
                    );
                }
                if previous != self.browser.model.entry_filter {
                    self.browser.model.rebuild();
                }
                if ui
                    .checkbox(
                        &mut self.browser.model.show_hidden,
                        lang.text("Mostra nascosti"),
                    )
                    .changed()
                {
                    self.request(Request::ScanHidden(self.browser.model.show_hidden));
                    self.refresh_folder();
                    let paths: Vec<_> = self
                        .browser
                        .model
                        .branches
                        .iter()
                        .filter(|(_, b)| b.expanded)
                        .map(|(p, _)| p.clone())
                        .collect();
                    for path in paths {
                        self.expand_branch(path, true);
                    }
                }
                if ui.button(lang.text("Svuota recenti")).clicked() {
                    self.browser.session.recent.clear();
                }
                if ui
                    .checkbox(
                        &mut self.browser.session.remember_recent,
                        lang.text("Memorizza recenti"),
                    )
                    .changed()
                    && !self.browser.session.remember_recent
                {
                    self.browser.session.recent.clear();
                }
            });
            if ui
                .button("+")
                .on_hover_text(lang.text("Aggiungi posizione…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.browser.model.add_root(path);
            }
            if ui.button(lang.text("Foto corrente")).clicked() {
                self.browser.model.focused = self.state.current_item().map(|i| i.path.clone());
                self.expand_branch(self.folder.clone(), false);
                self.browser.scroll_to_focus = true;
            }
        });
        // Keep the tree usable at the minimum window size and at high UI scale.
        // Lists live in bounded popups instead of consuming the tree's viewport.
        ui.horizontal(|ui| {
            ui.menu_button(lang.text("Preferiti"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(240.)
                    .show(ui, |ui| {
                        self.favorite_contents(ui);
                    });
            });
            ui.menu_button(lang.text("Recenti"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(240.)
                    .show(ui, |ui| {
                        self.recent_contents(ui);
                    });
            });
        });
        ui.add_space(4.);
        ui.label(
            egui::RichText::new(lang.text("Posizioni").to_uppercase())
                .size(11.)
                .strong()
                .color(MUTED),
        );
        self.tree(ui);
    }
    fn favorite_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let add = ui.add_enabled(
            !self.browser.favorite_pending,
            egui::Button::new(lang.text("Aggiungi cartella corrente")),
        );
        if add.clicked() {
            self.pin(self.folder.clone());
        }
        if let Some(payload) = add.dnd_release_payload::<DragLocation>()
            && let DragLocation::Folder(path) = &*payload
        {
            self.pin(path.clone());
        }
        let favorites = self.browser.favorites.clone();
        for (index, favorite) in favorites.iter().enumerate() {
            let path = favorite.location.path();
            let response = ui.add_enabled(
                path.is_some(),
                egui::Button::new(&favorite.label)
                    .frame(false)
                    .sense(egui::Sense::click_and_drag()),
            );
            response.dnd_set_drag_payload(DragLocation::Favorite(favorite.id.clone()));
            if let Some(payload) = response.dnd_release_payload::<DragLocation>() {
                match &*payload {
                    DragLocation::Favorite(id) => {
                        self.favorite_edit(tr_store::FavoriteEdit::Move {
                            id: id.clone(),
                            index,
                        })
                    }
                    DragLocation::Folder(path) => self.pin(path.clone()),
                }
            }
            if response.clicked()
                && let Some(path) = path.clone()
            {
                self.open_folder(path);
            }
            response.context_menu(|ui| {
                ui.add_enabled_ui(!self.browser.favorite_pending, |ui| {
                    if ui.button(lang.text("Rinomina preferito")).clicked() {
                        self.browser.rename = Some((favorite.id.clone(), favorite.label.clone()));
                        ui.close();
                    }
                    if ui
                        .add_enabled(index > 0, egui::Button::new(lang.text("Sposta su")))
                        .clicked()
                    {
                        self.favorite_edit(tr_store::FavoriteEdit::Move {
                            id: favorite.id.clone(),
                            index: index - 1,
                        });
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            index + 1 < favorites.len(),
                            egui::Button::new(lang.text("Sposta giù")),
                        )
                        .clicked()
                    {
                        self.favorite_edit(tr_store::FavoriteEdit::Move {
                            id: favorite.id.clone(),
                            index: index + 1,
                        });
                        ui.close();
                    }
                    if ui.button(lang.text("Rimuovi preferito")).clicked() {
                        self.favorite_edit(tr_store::FavoriteEdit::Remove(favorite.id.clone()));
                        ui.close();
                    }
                });
            });
        }
        if let Some((id, mut label)) = self.browser.rename.clone() {
            ui.add(egui::TextEdit::singleline(&mut label).char_limit(256));
            self.browser.rename = Some((id.clone(), label.clone()));
            ui.horizontal(|ui| {
                if ui.button(lang.text("Salva")).clicked() {
                    self.favorite_edit(tr_store::FavoriteEdit::Rename { id, label });
                    self.browser.rename = None;
                }
                if ui.button(lang.text("Annulla")).clicked() {
                    self.browser.rename = None;
                }
            });
        }
    }
    fn recent_contents(&mut self, ui: &mut egui::Ui) {
        for path in self
            .browser
            .session
            .recent
            .clone()
            .iter()
            .filter_map(Location::path)
        {
            if ui
                .button(
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy(),
                )
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                self.open_folder(path);
            }
        }
    }
    fn activate_entry(&mut self, entry: &tr_app::browser::Entry, viewer: bool) {
        self.browser.model.selected = Some(entry.path.clone());
        match entry.kind {
            Kind::Directory => self.open_folder(entry.path.clone()),
            Kind::Image => self.navigate(
                entry.path.clone(),
                if viewer {
                    Some(ViewMode::Preview)
                } else {
                    Some(if self.state.view == ViewMode::Compare {
                        ViewMode::Grid
                    } else {
                        self.state.view
                    })
                },
            ),
            Kind::Link => self.status = "Collegamento: usa Apri destinazione nel menu".into(),
            _ => self.status = "Anteprima non disponibile per questo tipo di file".into(),
        }
    }
    fn tree(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let tree_id = egui::Id::new("filesystem-tree");
        let height = ui.text_style_height(&egui::TextStyle::Body).max(18.) + 4.;
        ui.spacing_mut().item_spacing.y = 0.;
        let count = self.browser.model.rows.len();
        let mut focus = self
            .browser
            .model
            .focused
            .as_ref()
            .and_then(|p| {
                self.browser
                    .model
                    .rows
                    .iter()
                    .position(|r| &r.entry.path == p)
            })
            .unwrap_or(0);
        let has_focus = ui.memory(|m| m.has_focus(tree_id));
        if has_focus && !egui::Popup::is_any_open(ui.ctx()) {
            let key = |key| ui.input(|i| i.key_pressed(key));
            let previous = focus;
            if key(egui::Key::ArrowDown) {
                focus = (focus + 1).min(count.saturating_sub(1));
            }
            if key(egui::Key::ArrowUp) {
                focus = focus.saturating_sub(1);
            }
            if key(egui::Key::Home) {
                focus = 0;
            }
            if key(egui::Key::End) {
                focus = count.saturating_sub(1);
            }
            if key(egui::Key::PageDown) {
                focus = (focus + (ui.available_height() / height) as usize)
                    .min(count.saturating_sub(1));
            }
            if key(egui::Key::PageUp) {
                focus = focus.saturating_sub((ui.available_height() / height) as usize);
            }
            let text: String = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| {
                        if let egui::Event::Text(s) = e {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect()
            });
            if !text.is_empty() && !ui.input(|i| i.modifiers.command) {
                if self.browser.typed_at.elapsed() > Duration::from_millis(700) {
                    self.browser.typeahead.clear();
                }
                self.browser.typeahead.push_str(&text.to_lowercase());
                self.browser.typed_at = Instant::now();
                if let Some(index) = self.browser.model.rows.iter().position(|r| {
                    r.entry
                        .name
                        .to_lowercase()
                        .starts_with(&self.browser.typeahead)
                }) {
                    focus = index;
                }
            }
            if let Some(row) = self.browser.model.rows.get(focus).cloned() {
                self.browser.model.focused = Some(row.entry.path.clone());
                if key(egui::Key::Enter) {
                    self.activate_entry(&row.entry, true);
                }
                if key(egui::Key::Space) {
                    self.browser.model.selected = Some(row.entry.path.clone());
                }
                if key(egui::Key::ArrowRight) && row.entry.kind == Kind::Directory {
                    if self
                        .browser
                        .model
                        .branches
                        .get(&row.entry.path)
                        .is_some_and(|b| b.expanded)
                    {
                        if let Some(child) = self
                            .browser
                            .model
                            .rows
                            .get(focus + 1)
                            .filter(|c| c.depth > row.depth)
                        {
                            self.browser.model.focused = Some(child.entry.path.clone());
                        }
                    } else {
                        self.expand_branch(row.entry.path.clone(), false);
                    }
                }
                if key(egui::Key::ArrowLeft) {
                    if self
                        .browser
                        .model
                        .branches
                        .get(&row.entry.path)
                        .is_some_and(|b| b.expanded)
                    {
                        self.toggle_branch(row.entry.path.clone());
                    } else if let Some(parent) = self.browser.model.rows[..focus]
                        .iter()
                        .rev()
                        .find(|r| r.depth < row.depth)
                    {
                        self.browser.model.focused = Some(parent.entry.path.clone());
                    }
                }
            }
            self.browser.scroll_to_focus |= previous != focus;
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("explorer-tree-scroll")
            .vertical_scroll_offset(self.browser.session.explorer_scroll);
        if self.browser.scroll_to_focus {
            scroll = scroll.vertical_scroll_offset(focus as f32 * height);
            self.browser.scroll_to_focus = false;
        }
        let output = scroll.show_rows(ui, height, count, |ui, range| {
            for index in range {
                let row = self.browser.model.rows[index].clone();
                ui.push_id((&row.entry.path, index), |ui| {
                    ui.horizontal(|ui| {
                        let branch = self.browser.model.branches.get(&row.entry.path);
                        let expanded = branch.is_some_and(|b| b.expanded);
                        let state = branch
                            .map(|b| b.state.clone())
                            .unwrap_or(ListingState::Unloaded);
                        let current = row.entry.path == self.folder
                            || self
                                .state
                                .current_item()
                                .is_some_and(|i| i.path == row.entry.path);
                        let selected =
                            self.browser.model.selected.as_ref() == Some(&row.entry.path);
                        let title = format!(
                            "{}{}",
                            row.entry.name,
                            if state == ListingState::Loading {
                                " …"
                            } else if matches!(
                                state,
                                ListingState::Partial(_) | ListingState::Failed(_)
                            ) {
                                " !"
                            } else {
                                ""
                            }
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), height),
                            egui::Sense::hover(),
                        );
                        let indent = (row.depth as f32 * 14.).min(84.);
                        let arrow_rect = egui::Rect::from_min_size(
                            rect.min + egui::vec2(indent, 0.),
                            egui::vec2(16., height),
                        );
                        let response = ui.interact(
                            egui::Rect::from_min_max(
                                egui::pos2(arrow_rect.right(), rect.top()),
                                rect.max,
                            ),
                            ui.id().with("entry"),
                            egui::Sense::click_and_drag(),
                        );
                        let active = selected
                            || self
                                .state
                                .current_item()
                                .is_some_and(|i| i.path == row.entry.path);
                        if active || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0.,
                                if active {
                                    egui::Color32::from_rgb(43, 65, 83)
                                } else {
                                    egui::Color32::from_gray(42)
                                },
                            );
                        }
                        if active {
                            ui.painter().line_segment(
                                [rect.left_top(), rect.left_bottom()],
                                egui::Stroke::new(2., egui::Color32::from_rgb(100, 166, 215)),
                            );
                        }
                        for level in 0..row.depth.min(6) {
                            let x = rect.left() + level as f32 * 14. + 8.;
                            ui.painter().line_segment(
                                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                egui::Stroke::new(1., egui::Color32::from_gray(48)),
                            );
                        }
                        if row.entry.kind == Kind::Directory {
                            let arrow = ui
                                .interact(arrow_rect, ui.id().with("expand"), egui::Sense::click())
                                .on_hover_text(lang.text("Espandi o comprimi"));
                            arrow.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    lang.text("Espandi o comprimi"),
                                )
                            });
                            if arrow.clicked() {
                                self.toggle_branch(row.entry.path.clone());
                            }
                            let center = arrow_rect.center();
                            let points = if expanded {
                                vec![
                                    center + egui::vec2(-3., -2.),
                                    center + egui::vec2(0., 1.),
                                    center + egui::vec2(3., -2.),
                                ]
                            } else {
                                vec![
                                    center + egui::vec2(-1., -3.),
                                    center + egui::vec2(2., 0.),
                                    center + egui::vec2(-1., 3.),
                                ]
                            };
                            ui.painter()
                                .add(egui::Shape::line(points, egui::Stroke::new(1.2, MUTED)));
                        }
                        let icon_rect = egui::Rect::from_center_size(
                            egui::pos2(arrow_rect.right() + 9., rect.center().y),
                            egui::vec2(14., 14.),
                        );
                        entry_icon(ui.painter(), icon_rect, row.entry.kind, expanded);
                        let mut job = egui::text::LayoutJob::simple_singleline(
                            title,
                            egui::TextStyle::Body.resolve(ui.style()),
                            if current { TEXT } else { MUTED },
                        );
                        job.wrap.max_width = (response.rect.width() - 26.).max(0.);
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                        let position = egui::pos2(
                            response.rect.left() + 24.,
                            response.rect.center().y - galley.size().y / 2.,
                        );
                        ui.painter().galley(position, galley, TEXT);
                        if row.entry.kind == Kind::Directory {
                            response
                                .dnd_set_drag_payload(DragLocation::Folder(row.entry.path.clone()));
                        }
                        ui.ctx().accesskit_node_builder(response.id, |node| {
                            node.set_role(egui::accesskit::Role::TreeItem);
                            node.set_label(row.entry.name.as_str());
                            node.set_description(row.entry.path.to_string_lossy());
                            node.set_level(row.depth + 1);
                            node.set_selected(selected || current);
                            if row.entry.kind == Kind::Directory {
                                node.set_expanded(expanded);
                            }
                        });
                        if has_focus && self.browser.model.focused.as_ref() == Some(&row.entry.path)
                        {
                            ui.painter().rect_stroke(
                                response.rect,
                                2.,
                                egui::Stroke::new(1., MUTED),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let response = response.on_hover_text(format!(
                            "{}\n{}",
                            row.entry.path.display(),
                            match &state {
                                ListingState::Partial(e) | ListingState::Failed(e) => e.as_str(),
                                ListingState::Unloaded if row.entry.kind == Kind::Directory =>
                                    lang.text("Ramo non letto"),
                                _ => "",
                            }
                        ));
                        if response.clicked() || response.double_clicked() {
                            ui.memory_mut(|m| m.request_focus(tree_id));
                            self.browser.model.focused = Some(row.entry.path.clone());
                            self.activate_entry(&row.entry, response.double_clicked());
                            if response.double_clicked() && row.entry.kind == Kind::Directory {
                                self.expand_branch(row.entry.path.clone(), false);
                            }
                        }
                        let popup_id = egui::Popup::default_response_id(&response);
                        if has_focus
                            && self.browser.model.focused.as_ref() == Some(&row.entry.path)
                            && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F10))
                        {
                            egui::Popup::open_id(ui.ctx(), popup_id);
                        }
                        egui::Popup::context_menu(&response)
                            .at_position(response.rect.left_bottom())
                            .show(|ui| {
                                if matches!(
                                    row.entry.kind,
                                    Kind::Directory | Kind::Image | Kind::Link
                                ) && ui.button(lang.text("Apri destinazione")).clicked()
                                {
                                    self.navigate(
                                        row.entry.path.clone(),
                                        if row.entry.kind == Kind::Image {
                                            Some(ViewMode::Preview)
                                        } else {
                                            None
                                        },
                                    );
                                    ui.close();
                                }
                                if row.entry.kind == Kind::Directory {
                                    if ui.button(lang.text("Aggiungi ai preferiti")).clicked() {
                                        self.pin(row.entry.path.clone());
                                        ui.close();
                                    }
                                    if ui.button(lang.text("Rileggi questo ramo")).clicked() {
                                        self.expand_branch(row.entry.path.clone(), true);
                                        ui.close();
                                    }
                                }
                                if ui.button(lang.text("Copia percorso")).clicked() {
                                    ui.ctx().copy_text(row.entry.path.to_string_lossy().into());
                                    ui.close();
                                }
                                if ui.button(lang.text("Mostra nel sistema")).clicked() {
                                    self.service
                                        .filesystem
                                        .client
                                        .reveal(row.entry.path.clone());
                                    ui.close();
                                }
                            });
                    });
                });
            }
        });
        self.browser.session.explorer_scroll = output.state.offset.y;
        self.browser.tree_rect = output.inner_rect;
        let response = ui.interact(
            output.inner_rect,
            tree_id,
            egui::Sense::focusable_noninteractive(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                lang.text("Albero file e cartelle"),
            )
        });
        ui.ctx().accesskit_node_builder(tree_id, |node| {
            node.set_role(egui::accesskit::Role::Tree);
            node.set_label(lang.text("Albero file e cartelle"));
        });
        if self.browser.focus_tree {
            response.request_focus();
            self.browser.focus_tree = false;
        }
    }
    pub(super) fn location_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        egui::Panel::top("location-bar")
            .frame(style::panel())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(lang.text("Pannello"))
                        .on_hover_text("Cmd/Ctrl+B")
                        .clicked()
                    {
                        if ui.ctx().content_rect().width() < 1000. {
                            self.browser.temporary = !self.browser.temporary;
                        } else {
                            self.browser.session.visible = !self.browser.session.visible;
                        }
                    }
                    if ui
                        .add_enabled(
                            self.browser.cursor.is_some_and(|c| c > 0),
                            egui::Button::new("<"),
                        )
                        .on_hover_text(lang.text("Indietro"))
                        .clicked()
                    {
                        self.history_go(-1);
                    }
                    if ui
                        .add_enabled(
                            self.browser
                                .cursor
                                .is_some_and(|c| c + 1 < self.browser.history.len()),
                            egui::Button::new(">"),
                        )
                        .on_hover_text(lang.text("Avanti"))
                        .clicked()
                    {
                        self.history_go(1);
                    }
                    if ui
                        .add_enabled(self.folder.parent().is_some(), egui::Button::new("^"))
                        .on_hover_text(lang.text("Cartella superiore"))
                        .clicked()
                        && let Some(parent) = self.folder.parent()
                    {
                        self.open_folder(parent.into());
                    }
                    if ui
                        .button("↻")
                        .on_hover_text(lang.text("Rileggi cartella"))
                        .clicked()
                    {
                        self.refresh_folder();
                    }
                    if self.browser.path_edit {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.browser.path_text)
                                .id(egui::Id::new("browser-path"))
                                .desired_width(ui.available_width().max(40.)),
                        );
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let path = PathBuf::from(&self.browser.path_text);
                            self.browser.path_edit = false;
                            self.open_path(path);
                        }
                    } else {
                        let mut ancestors: Vec<_> = self
                            .folder
                            .ancestors()
                            .map(std::path::Path::to_path_buf)
                            .collect();
                        ancestors.reverse();
                        let keep = if ui.available_width() < 350. { 1 } else { 3 };
                        if ancestors.len() > keep {
                            ui.menu_button("…", |ui| {
                                for path in &ancestors[..ancestors.len() - keep] {
                                    if ui.button(path.display().to_string()).clicked() {
                                        self.open_folder(path.clone());
                                        ui.close();
                                    }
                                }
                            });
                        }
                        for path in ancestors.iter().skip(ancestors.len().saturating_sub(keep)) {
                            if ui
                                .add(
                                    egui::Button::new(
                                        path.file_name()
                                            .unwrap_or(path.as_os_str())
                                            .to_string_lossy(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(path.display().to_string())
                                .clicked()
                            {
                                self.open_folder(path.clone());
                            }
                            ui.label("›");
                        }
                        if ui
                            .small_button("/")
                            .on_hover_text(lang.text("Inserisci un percorso"))
                            .clicked()
                        {
                            self.edit_location();
                        }
                    }
                });
                if self.state.targeted.is_some() {
                    ui.horizontal(|ui| {
                        ui.label(lang.text("Filtri sospesi per apertura diretta"));
                        if ui.button(lang.text("Ripristina filtri")).clicked() {
                            self.state.targeted = None;
                            self.state.refilter();
                        }
                    });
                } else if !self.state.query.is_empty()
                    || self.state.minimum_rating != 0
                    || self.state.rejected_only
                    || self.state.label_filter.is_some()
                {
                    ui.horizontal(|ui| {
                        ui.label(lang.text("Filtri fotografici attivi"));
                        if ui.small_button(lang.text("Mostra filtri")).clicked() {
                            self.panel_mode(PanelMode::Library, true);
                        }
                    });
                }
            });
    }
    fn edit_location(&mut self) {
        self.browser.path_edit = true;
        self.browser.path_text = self.folder.to_string_lossy().into();
        self.context
            .memory_mut(|m| m.request_focus(egui::Id::new("browser-path")));
    }
    pub(super) fn browser_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if self.show_settings || self.show_help {
            return true;
        }
        if self.browser.temporary && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.browser.temporary = false;
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
            return true;
        }
        if self.browser.path_edit && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.browser.path_edit = false;
            return true;
        }
        if ctx.text_edit_focused() {
            return true;
        }
        let modifiers = ctx.input(|i| i.modifiers);
        let key = |k| ctx.input(|i| i.key_pressed(k));
        if modifiers.command {
            if key(egui::Key::B) {
                if ctx.content_rect().width() < 1000. {
                    self.browser.temporary = !self.browser.temporary;
                } else {
                    self.browser.session.visible = !self.browser.session.visible;
                }
                return true;
            }
            if modifiers.shift && key(egui::Key::E) {
                self.panel_mode(PanelMode::Explorer, true);
                self.browser.focus_tree = true;
                return true;
            }
            if key(egui::Key::L) {
                self.edit_location();
                return true;
            }
            if key(egui::Key::ArrowUp)
                && let Some(parent) = self.folder.parent()
            {
                self.open_folder(parent.into());
                return true;
            }
            if key(egui::Key::OpenBracket) {
                self.history_go(-1);
                return true;
            }
            if key(egui::Key::CloseBracket) {
                self.history_go(1);
                return true;
            }
        }
        if modifiers.alt && key(egui::Key::ArrowLeft) {
            self.history_go(-1);
            return true;
        }
        if modifiers.alt && key(egui::Key::ArrowRight) {
            self.history_go(1);
            return true;
        }
        if key(egui::Key::F5) {
            self.refresh_folder();
            return true;
        }
        ctx.memory(|m| m.has_focus(egui::Id::new("filesystem-tree"))) || self.browser.temporary
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings_regressions::{app, settle};
    use super::*;
    fn wait_tree(app: &mut TrueRenderer, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app
            .browser
            .model
            .branches
            .values()
            .any(|b| b.state == ListingState::Loading)
        {
            app.poll_browser(ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn switching_and_expanding_keep_photo_geometry_quality_and_pending_edits() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.state.view = ViewMode::Preview;
        app.state.transform.zoom = Some(2.5);
        let current = app.state.current.clone();
        let selected = app.state.selected.clone();
        let generation = app.generation;
        app.quality_overrides
            .insert(current.clone().unwrap(), PreviewQuality::Full);
        app.command(Command::Rate(5));
        let pending = app.state.pending.clone();
        app.panel_mode(PanelMode::Explorer, false);
        wait_tree(&mut app, &ctx);
        app.panel_mode(PanelMode::Library, false);
        app.panel_mode(PanelMode::Explorer, false);
        assert_eq!(app.generation, generation);
        assert_eq!(app.state.current, current);
        assert_eq!(app.state.selected, selected);
        assert_eq!(app.state.transform.zoom, Some(2.5));
        assert_eq!(app.state.pending, pending);
        assert_eq!(app.quality_overrides.len(), 1);
        assert!(!app.browser.model.rows.is_empty());
    }
    #[test]
    fn rapid_navigation_failure_history_and_refresh_keep_the_correct_context() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let origin = app.folder.clone();
        let a = app.root.join("a");
        let b = app.root.join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let current = app.state.current.clone();
        app.state.transform.zoom = Some(2.);
        app.open_folder(a);
        app.open_folder(b.clone());
        settle(&mut app, &ctx, true);
        assert_eq!(app.folder, b);
        assert!(app.state.items.is_empty());
        app.open_folder(app.root.join("missing"));
        settle(&mut app, &ctx, true);
        assert_eq!(app.folder, b);
        assert_eq!(app.browser.history.len(), 2);
        app.history_go(-1);
        settle(&mut app, &ctx, true);
        assert_eq!(app.folder, origin);
        assert_eq!(app.state.current, current);
        assert_eq!(app.state.transform.zoom, Some(2.));
        let generation = app.generation;
        app.refresh_folder();
        settle(&mut app, &ctx, true);
        assert_eq!(app.generation, generation);
        assert_eq!(app.state.current, current);
        assert_eq!(app.state.transform.zoom, Some(2.));
    }
    #[test]
    fn direct_file_reaches_target_without_erasing_filters_and_engine_does_not_revoke_scan() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let target = app.state.items[1].path.clone();
        app.state.query = "does-not-match".into();
        app.state.minimum_rating = 5;
        app.state.refilter();
        app.open_path(target.clone());
        app.invalidate_raw_engine();
        settle(&mut app, &ctx, true);
        assert_eq!(app.state.current_item().unwrap().path, target);
        assert_eq!(app.state.query, "does-not-match");
        assert_eq!(app.state.minimum_rating, 5);
        assert_eq!(app.state.view, ViewMode::Preview);
        app.state.targeted = None;
        app.state.refilter();
        assert!(app.state.visible.is_empty());
    }
    #[test]
    fn tree_keyboard_never_rates_or_changes_view_and_escape_is_consumed() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.panel_mode(PanelMode::Explorer, false);
        wait_tree(&mut app, &ctx);
        app.state.view = ViewMode::Preview;
        let frame = |app: &mut TrueRenderer, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100., 720.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.keyboard(ui.ctx());
                    app.sidebar(ui);
                },
            );
            output.textures_delta.clear();
        };
        app.browser.focus_tree = true;
        frame(&mut app, vec![]);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut app,
            vec![
                key(egui::Key::Num5),
                egui::Event::Text("5".into()),
                key(egui::Key::G),
            ],
        );
        assert!(app.state.pending.is_empty());
        assert_eq!(app.state.view, ViewMode::Preview);
        app.browser.temporary = true;
        frame(&mut app, vec![key(egui::Key::Escape)]);
        assert!(!app.browser.temporary);
        assert_eq!(app.state.view, ViewMode::Preview);
    }
    #[test]
    fn favorite_commit_and_session_survive_restart_without_settings_drafts() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        app.pin(app.folder.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.browser.favorite_pending {
            app.poll(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.browser.favorites.len(), 1);
        app.browser.session.mode = PanelMode::Explorer;
        app.cache_settings.disk_mib += 1024;
        let draft = app.cache_settings.disk_mib;
        app.persist_browser();
        drop(app);
        assert_eq!(
            Session::load(&dir.path().join("data")).0.mode,
            PanelMode::Explorer
        );
        assert_ne!(
            crate::cache::Settings::load(&dir.path().join("data"))
                .unwrap()
                .disk_mib,
            draft
        );
        assert_eq!(
            tr_store::Catalog::open(&dir.path().join("data"))
                .unwrap()
                .favorites()
                .unwrap()
                .len(),
            1
        );
    }
}
