//! Shared photographic commands; targets are frozen when the menu opens.
use super::*;

#[derive(Clone)]
struct Targets {
    clicked: Option<Item>,
    path: PathBuf,
    photos: Vec<Item>,
    navigation: u64,
}
#[derive(Clone)]
enum Action {
    Open,
    Fit,
    One,
    Info,
    Crop,
    Proof,
    Copy,
    Paste,
    Reset,
    Turn(u8),
    Flip(bool),
    Rate(i8),
    Label(Label),
    Keywords(String),
    Export,
    Undo,
    Redo,
}
impl Action {
    fn annotation(&self) -> bool {
        matches!(self, Self::Rate(_) | Self::Label(_) | Self::Keywords(_))
    }
    fn single(&self) -> bool {
        matches!(
            self,
            Self::Open | Self::Fit | Self::One | Self::Info | Self::Crop | Self::Proof | Self::Copy
        )
    }
}
struct Batch {
    action: Action,
    remaining: VecDeque<Item>,
    active: Option<Item>,
    verifying: Option<u64>,
    navigation: u64,
}
#[derive(Clone)]
struct PendingView {
    expected: Item,
    action: Action,
    navigation: u64,
}
#[derive(Default)]
pub(super) struct PhotoMenu {
    snapshot: Option<(egui::Id, Targets)>,
    pub extra: HashMap<String, Item>,
    token: u64,
    pub navigation: u64,
    resolving: Option<(u64, PathBuf)>,
    resolve_error: Option<String>,
    batch: Option<Batch>,
    results: Vec<(String, String)>,
    results_open: bool,
    keywords: Option<(Targets, String)>,
    navigation_action: Option<PendingView>,
}
fn same_photo(current: &Item, expected: &Item) -> bool {
    current.id == expected.id
        && current.path == expected.path
        && current.digest == expected.digest
        && current.observation == expected.observation
}
fn recipients(state: &tr_app::State, item: &Item, multiple: bool) -> Vec<Item> {
    if multiple && state.selected.contains(&item.id) {
        state
            .items
            .iter()
            .filter(|p| state.selected.contains(&p.id))
            .cloned()
            .collect()
    } else {
        vec![item.clone()]
    }
}
impl TrueRenderer {
    pub(super) fn context_photo_result(&mut self, token: u64, result: Result<Item, String>) {
        if self
            .photo_menu
            .resolving
            .as_ref()
            .is_some_and(|(t, _)| *t == token)
        {
            self.photo_menu.resolving = None;
            match result {
                Ok(item) => {
                    if let Some((_, s)) = &mut self.photo_menu.snapshot
                        && s.path == item.path
                    {
                        s.clicked = Some(item.clone());
                        s.photos = vec![item.clone()];
                    }
                    if self.photo_menu.extra.len() >= 64 {
                        self.photo_menu.extra.clear();
                    }
                    self.ensure_edit_loaded(&item);
                    self.photo_menu.extra.insert(item.id.clone(), item);
                }
                Err(e) => self.photo_menu.resolve_error = Some(e),
            }
            return;
        }
        let Some(batch) = &mut self.photo_menu.batch else {
            return;
        };
        if batch.verifying != Some(token) {
            return;
        }
        batch.verifying = None;
        let item = batch.active.clone().unwrap();
        if batch.navigation != self.photo_menu.navigation {
            self.finish_context_photo(
                &item.id,
                Err("Navigazione cambiata: operazione annullata".into()),
            );
            return;
        }
        match result {
            Ok(current) if same_photo(&current, &item) && current.approved => {
                let action = batch.action.clone();
                self.execute_context_photo(&item, action);
            }
            Ok(_) => self
                .finish_context_photo(&item.id, Err("Sorgente cambiata o non disponibile".into())),
            Err(e) => self.finish_context_photo(&item.id, Err(e)),
        }
    }
    fn context_ready(&self, item: &Item) -> bool {
        self.editing.crop.is_none()
            && !self.editing.wb_pending
            && item.approved
            && self.editing.entries.get(&item.id).is_some_and(|e| {
                e.loaded.is_some()
                    && e.draft.is_some()
                    && !e.loading
                    && !e.pending
                    && e.error.is_none()
                    && e.draft.as_ref() == e.loaded.as_ref().map(|s| &s.recipe)
            })
    }
    pub(super) fn photo_menu_for(
        &mut self,
        response: &egui::Response,
        item: &Item,
        multiple: bool,
    ) {
        if response.clicked() || response.secondary_clicked() {
            response.request_focus();
        }
        self.photo_menu_path(response, &item.path, Some(item), multiple);
    }
    pub(super) fn photo_menu_path(
        &mut self,
        response: &egui::Response,
        path: &std::path::Path,
        item: Option<&Item>,
        multiple: bool,
    ) {
        self.photo_menu_path_inner(response, path, item, multiple, false);
    }
    pub(super) fn crop_menu_for(&mut self, response: &egui::Response, item: &Item) {
        if response.clicked() || response.secondary_clicked() {
            response.request_focus();
        }
        self.photo_menu_path_inner(response, &item.path, Some(item), false, true);
    }
    fn photo_menu_path_inner(
        &mut self,
        response: &egui::Response,
        path: &std::path::Path,
        item: Option<&Item>,
        multiple: bool,
        crop_tool: bool,
    ) {
        let popup = egui::Popup::default_response_id(response);
        let keyboard = response.has_focus()
            && response
                .ctx
                .input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F10));
        if response.secondary_clicked() || keyboard {
            let clicked = item
                .cloned()
                .or_else(|| self.known_items().find(|p| p.path == path).cloned());
            let photos = clicked
                .as_ref()
                .map_or_else(Vec::new, |p| recipients(&self.state, p, multiple));
            self.photo_menu.snapshot = Some((
                popup,
                Targets {
                    clicked: clicked.clone(),
                    path: path.into(),
                    photos,
                    navigation: self.photo_menu.navigation,
                },
            ));
            self.photo_menu.resolve_error = None;
            if clicked.is_none() {
                self.photo_menu.token += 1;
                let token = self.photo_menu.token;
                if self.request(Request::ContextPhoto {
                    token,
                    path: path.into(),
                }) {
                    self.photo_menu.resolving = Some((token, path.into()));
                }
            }
            if keyboard {
                egui::Popup::open_id(&response.ctx, popup);
            }
        }
        egui::Popup::context_menu(response)
            .at_position(response.rect.left_bottom())
            .show(|ui| {
                let Some((id, targets)) = self
                    .photo_menu
                    .snapshot
                    .clone()
                    .filter(|(id, _)| *id == popup)
                else {
                    return;
                };
                let _ = id;
                if crop_tool
                    && targets
                        .clicked
                        .as_ref()
                        .is_some_and(|p| self.crop_active(&p.id))
                {
                    self.crop_menu_contents(ui);
                    ui.separator();
                    ui.menu_button(self.cache_settings.language.text("Menu fotografia"), |ui| {
                        self.context_menu_contents(ui, targets);
                    });
                } else {
                    self.context_menu_contents(ui, targets);
                }
            });
    }
    fn context_menu_contents(&mut self, ui: &mut egui::Ui, targets: Targets) {
        let lang = self.cache_settings.language;
        ui.add_enabled_ui(targets.clicked.is_some(), |ui| {
            self.comparison_menu(ui, &targets.path, targets.clicked.as_ref());
        });
        ui.separator();
        let busy = self.photo_menu.batch.is_some() || self.photo_menu.navigation_action.is_some();
        for item in &targets.photos {
            self.ensure_edit_loaded(item);
        }
        let ready = !busy
            && !targets.photos.is_empty()
            && targets.photos.iter().all(|p| self.context_ready(p));
        let single_ready = !busy
            && targets
                .clicked
                .as_ref()
                .is_some_and(|p| self.context_ready(p));
        let annotations = !busy
            && !targets.photos.is_empty()
            && targets
                .photos
                .iter()
                .all(|p| !self.state.pending.contains(&p.id));
        if targets.photos.len() > 1 {
            ui.label(localized_format!(
                lang,
                "{} foto destinatarie",
                "{} target photos",
                targets.photos.len()
            ));
        }
        if !ready {
            ui.small(lang.text(if busy {
                "Operazione in corso"
            } else if self.editing.crop.is_some() {
                "Conferma o annulla il ritaglio prima di modificare le regolazioni."
            } else {
                "Sorgente o ricetta non disponibile; attendere caricamento/salvataggio."
            }));
        }
        if let Some(e) = &self.photo_menu.resolve_error {
            ui.small(e);
        }
        let mut action = None;
        for (label, a, enabled) in [
            (
                "Apri in Anteprima",
                Action::Open,
                !busy && targets.clicked.is_some(),
            ),
            ("Adatta", Action::Fit, single_ready),
            ("1:1", Action::One, single_ready),
            (
                "Informazioni",
                Action::Info,
                !busy && targets.clicked.is_some(),
            ),
        ] {
            if ui
                .add_enabled(enabled, egui::Button::new(lang.text(label)))
                .clicked()
            {
                action = Some(a);
            }
        }
        ui.separator();
        ui.menu_button(lang.text("Valutazione e metadati"), |ui| {
            ui.add_enabled_ui(annotations, |ui| {
                for value in 0..=5 {
                    if ui.button(format!("{value} ★")).clicked() {
                        action = Some(Action::Rate(value));
                    }
                }
                for (label, value) in [("Scarta", -1), ("Rimuovi valutazione/scarto", 0)] {
                    if ui.button(lang.text(label)).clicked() {
                        action = Some(Action::Rate(value));
                    }
                }
                ui.menu_button(lang.text("Etichetta colore"), |ui| {
                    for label in Label::ALL {
                        if ui.button(lang.text(label.text())).clicked() {
                            action = Some(Action::Label(label));
                        }
                    }
                });
                if ui.button(lang.text("Modifica parole chiave…")).clicked() {
                    let text = targets
                        .clicked
                        .as_ref()
                        .map_or(String::new(), |p| p.annotation.keywords.join(", "));
                    self.photo_menu.keywords = Some((targets.clone(), text));
                    ui.close();
                }
            });
        });
        ui.menu_button(lang.text("Regolazioni"), |ui| {
            if ui
                .add_enabled(
                    single_ready,
                    egui::Button::new(lang.text("Copia regolazioni")),
                )
                .clicked()
            {
                action = Some(Action::Copy);
            }
            ui.add_enabled_ui(ready, |ui| {
                ui.menu_button(lang.text("Incolla regolazioni…"), |ui| {
                    if self.editing.clipboard.menu(ui, lang) {
                        action = Some(Action::Paste);
                    }
                });
                for (label, a) in [
                    ("Azzera regolazioni", Action::Reset),
                    ("Ruota 90° a sinistra", Action::Turn(3)),
                    ("Ruota 90° a destra", Action::Turn(1)),
                    ("Specchio orizzontale", Action::Flip(true)),
                    ("Specchio verticale", Action::Flip(false)),
                    ("Annulla regolazioni", Action::Undo),
                    ("Ripeti regolazioni", Action::Redo),
                ] {
                    let available = match a {
                        Action::Undo | Action::Redo => targets.photos.iter().any(|p| {
                            self.editing
                                .entries
                                .get(&p.id)
                                .and_then(|e| e.loaded.as_ref())
                                .is_some_and(|saved| {
                                    if matches!(a, Action::Undo) {
                                        saved.can_undo
                                    } else {
                                        saved.can_redo
                                    }
                                })
                        }),
                        _ => true,
                    };
                    if ui
                        .add_enabled(available, egui::Button::new(lang.text(label)))
                        .on_disabled_hover_text(lang.text("Nessuna revisione disponibile"))
                        .clicked()
                    {
                        action = Some(a);
                    }
                }
            });
            if ui
                .add_enabled(single_ready, egui::Button::new(lang.text("Ritaglio…")))
                .clicked()
            {
                action = Some(Action::Crop);
            }
        });
        ui.menu_button(lang.text("File e uscita"), |ui| {
            let export = if targets.photos.len() > 1 {
                localized_format!(
                    lang,
                    "Esporta {} foto…",
                    "Export {} photos…",
                    targets.photos.len()
                )
            } else {
                lang.text("Esporta foto…").into()
            };
            if ui
                .add_enabled(ready && !self.photo_export.open, egui::Button::new(export))
                .clicked()
            {
                action = Some(Action::Export);
            }
            if ui
                .add_enabled(
                    single_ready,
                    egui::Button::new(lang.text("Verifica resa finale")),
                )
                .clicked()
            {
                action = Some(Action::Proof);
            }
            if ui.button(lang.text("Mostra nel sistema")).clicked() {
                self.service.filesystem.client.reveal(targets.path.clone());
                ui.close();
            }
            if ui.button(lang.text("Copia percorso")).clicked() {
                ui.ctx().copy_text(targets.path.to_string_lossy().into());
                ui.close();
            }
        });
        if let Some(action) = action {
            self.queue_context_action(targets, action);
            ui.close();
        }
    }
    fn queue_context_action(&mut self, targets: Targets, action: Action) {
        if targets.navigation != self.photo_menu.navigation || self.photo_menu.batch.is_some() {
            return;
        }
        if targets
            .clicked
            .as_ref()
            .is_some_and(|expected| !self.known_items().any(|p| same_photo(p, expected)))
        {
            self.status = self
                .cache_settings
                .language
                .text("Foto o revisione cambiata")
                .into();
            return;
        }
        if matches!(action, Action::Open | Action::Info) {
            let Some(item) = targets.clicked else {
                return;
            };
            if self.state.items.iter().any(|i| i.id == item.id) {
                if !self
                    .state
                    .visible
                    .iter()
                    .any(|i| self.state.items[*i].id == item.id)
                {
                    self.state.targeted = Some(item.id.clone());
                    self.state.refilter();
                }
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                self.state.view = ViewMode::Preview;
                self.context_view_action(&item, action);
            } else {
                self.navigate(item.path.clone(), Some(ViewMode::Preview));
                self.photo_menu.navigation_action = Some(PendingView {
                    expected: item,
                    action,
                    navigation: self.photo_menu.navigation,
                });
            }
            return;
        }
        if matches!(action, Action::Export) {
            self.photo_export.targets = Some(targets.photos);
            self.photo_export.open = true;
            return;
        }
        let photos = if action.single() {
            targets.clicked.into_iter().collect()
        } else {
            targets.photos
        };
        self.photo_menu.results.clear();
        self.photo_menu.results_open = !action.single();
        self.photo_menu.batch = Some(Batch {
            action,
            remaining: photos.into(),
            active: None,
            verifying: None,
            navigation: self.photo_menu.navigation,
        });
    }
    fn context_view_action(&mut self, item: &Item, action: Action) {
        match action {
            Action::Open | Action::Fit => self.state.transform = ViewTransform::default(),
            Action::One => {
                self.quality_overrides
                    .insert(item.id.clone(), PreviewQuality::Full);
                self.state.transform.set_zoom(1.);
            }
            Action::Info => {
                self.show_inspector = true;
                self.inspector_pages[1] = InspectorPage::Information;
            }
            Action::Crop => self.start_crop(item),
            Action::Proof => self.request_final_preview(&item.id, true),
            _ => {}
        }
    }
    fn execute_context_photo(&mut self, item: &Item, action: Action) {
        if action.annotation() {
            let current = self.known_items().find(|p| p.id == item.id).cloned();
            let Some(current) = current.filter(|p| {
                p.path == item.path && p.digest == item.digest && p.revision == item.revision
            }) else {
                self.finish_context_photo(&item.id, Err("Foto o revisione cambiata".into()));
                return;
            };
            let mut annotation = current.annotation.clone();
            match action {
                Action::Rate(r) => annotation.rating = r,
                Action::Label(l) => annotation.label = l,
                Action::Keywords(s) => {
                    if let Err(e) = annotation.set_keywords(&s) {
                        self.finish_context_photo(&item.id, Err(e.to_string()));
                        return;
                    }
                }
                _ => {}
            }
            if annotation == current.annotation {
                self.finish_context_photo(&item.id, Ok(()));
                return;
            }
            if self.request(Request::Save {
                id: item.id.clone(),
                expected: item.revision,
                annotation,
            }) {
                self.state.pending.insert(item.id.clone());
            } else {
                self.finish_context_photo(&item.id, Err("Coda occupata".into()));
            }
            return;
        }
        if !self.context_ready(item) {
            self.finish_context_photo(&item.id, Err("Ricetta occupata o non disponibile".into()));
            return;
        }
        if matches!(
            action,
            Action::Fit | Action::One | Action::Crop | Action::Proof
        ) {
            if self.state.items.iter().any(|p| p.id == item.id) {
                if !self
                    .state
                    .visible
                    .iter()
                    .any(|i| self.state.items[*i].id == item.id)
                {
                    self.state.targeted = Some(item.id.clone());
                    self.state.refilter();
                }
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                self.state.view = ViewMode::Preview;
                self.context_view_action(item, action);
            } else {
                self.navigate(item.path.clone(), Some(ViewMode::Preview));
                self.photo_menu.navigation_action = Some(PendingView {
                    expected: item.clone(),
                    action,
                    navigation: self.photo_menu.navigation,
                });
            }
            self.finish_context_photo(&item.id, Ok(()));
            return;
        }
        let e = &self.editing.entries[&item.id];
        let mut recipe = e.draft.clone().unwrap();
        match action {
            Action::Copy => {
                self.editing.clipboard.copy(&item.name, &recipe);
                self.finish_context_photo(&item.id, Ok(()));
                return;
            }
            Action::Paste => {
                if let Some(p) = self.editing.clipboard.paste(&recipe) {
                    recipe = p;
                }
            }
            Action::Reset => recipe = tr_core::editing::EditRecipe::neutral(recipe.raw_engine),
            Action::Turn(n) => {
                let g = &mut recipe
                    .advanced
                    .get_or_insert_with(Default::default)
                    .geometry;
                g.rotate_crop(n);
                recipe.require_process(3);
            }
            Action::Flip(horizontal) => {
                let g = &mut recipe
                    .advanced
                    .get_or_insert_with(Default::default)
                    .geometry;
                g.reflect_crop(horizontal);
                recipe.require_process(3);
            }
            Action::Undo | Action::Redo => {
                let can = if matches!(action, Action::Undo) {
                    e.loaded.as_ref().unwrap().can_undo
                } else {
                    e.loaded.as_ref().unwrap().can_redo
                };
                if can {
                    self.step_edit(&item.id, matches!(action, Action::Undo));
                    if !self.editing.entries[&item.id].pending {
                        self.finish_context_photo(&item.id, Err("Coda occupata".into()));
                    }
                } else {
                    self.finish_context_photo(&item.id, Ok(()));
                }
                return;
            }
            _ => {
                self.finish_context_photo(&item.id, Err("Azione non disponibile".into()));
                return;
            }
        }
        if e.draft.as_ref() == Some(&recipe) {
            self.finish_context_photo(&item.id, Ok(()));
            return;
        }
        self.apply_edit_draft(&item.id, recipe);
        self.commit_edit(&item.id);
        if !self.editing.entries[&item.id].pending {
            self.finish_context_photo(
                &item.id,
                Err("Salvataggio non avviato; bozza conservata".into()),
            );
        }
    }
    pub(super) fn finish_context_photo(&mut self, id: &str, result: Result<(), String>) {
        if let Some(batch) = &mut self.photo_menu.batch
            && batch.active.as_ref().is_some_and(|p| p.id == id)
        {
            let photo = batch.active.take().unwrap();
            batch.verifying = None;
            self.photo_menu.results_open |= result.is_err();
            let message = result
                .err()
                .unwrap_or_else(|| self.cache_settings.language.text("Completato").into());
            self.status = format!("{}: {}", photo.name, message);
            self.photo_menu.results.push((photo.name, message));
        }
    }
    fn fail_context_navigation(&mut self, expected: &Item, reason: &str) {
        self.photo_menu.navigation_action = None;
        self.status = format!(
            "{}: {}",
            expected.name,
            self.cache_settings.language.text(reason)
        );
        self.photo_menu
            .results
            .push((expected.name.clone(), reason.into()));
        self.photo_menu.results_open = true;
    }
    pub(super) fn process_photo_actions(&mut self, ctx: &egui::Context) {
        if let Some(pending) = self.photo_menu.navigation_action.clone() {
            if pending.navigation != self.photo_menu.navigation {
                self.photo_menu.navigation_action = None;
            } else if let Some(item) = self
                .state
                .current_item()
                .filter(|p| p.path == pending.expected.path)
                .cloned()
            {
                let view_only = matches!(pending.action, Action::Open | Action::Info);
                if !same_photo(&item, &pending.expected) {
                    self.fail_context_navigation(&pending.expected, "Foto o revisione cambiata");
                } else if !view_only
                    && (!item.approved
                        || self
                            .editing
                            .entries
                            .get(&item.id)
                            .is_some_and(|e| e.error.is_some()))
                {
                    self.fail_context_navigation(
                        &pending.expected,
                        "Sorgente o ricetta non disponibile",
                    );
                } else {
                    self.ensure_edit_loaded(&item);
                    if self.context_ready(&item) || view_only {
                        self.photo_menu.navigation_action = None;
                        self.context_view_action(&item, pending.action);
                    }
                }
            } else if !self.scanning && self.browser.pending.is_none() {
                self.fail_context_navigation(
                    &pending.expected,
                    "Sorgente o ricetta non disponibile",
                );
            }
        }
        if let Some(batch) = &mut self.photo_menu.batch {
            if batch.navigation != self.photo_menu.navigation {
                // Abandon a read immediately: an offline filesystem may not
                // answer promptly. Accepted writes still await their result.
                if batch.verifying.take().is_some()
                    && let Some(p) = batch.active.take()
                {
                    self.photo_menu
                        .results
                        .push((p.name, "Navigazione cambiata: operazione annullata".into()));
                }
                for p in batch.remaining.drain(..) {
                    self.photo_menu
                        .results
                        .push((p.name, "Navigazione cambiata: operazione annullata".into()));
                }
            }
            if batch.active.is_none() {
                if let Some(item) = batch.remaining.pop_front() {
                    batch.active = Some(item.clone());
                    let action = batch.action.clone();
                    if action.annotation() {
                        self.execute_context_photo(&item, action);
                    } else {
                        self.photo_menu.token += 1;
                        let token = self.photo_menu.token;
                        if self.request(Request::ContextPhoto {
                            token,
                            path: item.path.clone(),
                        }) {
                            self.photo_menu.batch.as_mut().unwrap().verifying = Some(token);
                        } else {
                            self.finish_context_photo(&item.id, Err("Coda occupata".into()));
                        }
                    }
                } else {
                    self.photo_menu.batch = None;
                }
            }
        }
        let lang = self.cache_settings.language;
        if let Some((targets, mut text)) = self.photo_menu.keywords.take() {
            let mut open = true;
            let mut save = false;
            egui::Window::new(lang.text("Modifica parole chiave…"))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(localized_format!(
                        lang,
                        "Sostituisci parole chiave di {} foto (separate da virgole)",
                        "Replace keywords of {} photos (comma separated)",
                        targets.photos.len()
                    ));
                    ui.text_edit_multiline(&mut text);
                    save = ui.button(lang.text("Applica")).clicked();
                });
            if save {
                self.queue_context_action(targets, Action::Keywords(text));
            } else if open {
                self.photo_menu.keywords = Some((targets, text));
            }
        }
        if self.photo_menu.results_open {
            let mut open = true;
            egui::Window::new(lang.text("Esiti per foto"))
                .open(&mut open)
                .vscroll(true)
                .show(ctx, |ui| {
                    if ui
                        .add_enabled(
                            self.photo_menu.batch.is_some(),
                            egui::Button::new(lang.text("Annulla operazioni rimanenti")),
                        )
                        .clicked()
                        && let Some(batch) = &mut self.photo_menu.batch
                    {
                        if batch.verifying.take().is_some()
                            && let Some(p) = batch.active.take()
                        {
                            self.photo_menu
                                .results
                                .push((p.name, lang.text("Annullato").into()));
                        }
                        for p in batch.remaining.drain(..) {
                            self.photo_menu
                                .results
                                .push((p.name, lang.text("Annullato").into()));
                        }
                    }
                    ui.small(lang.text(
                        "Regolazioni: Annulla dalla foto. Annotazioni: Annulla modifica dal menu.",
                    ));
                    for (name, result) in &self.photo_menu.results {
                        ui.label(format!("{name}: {}", lang.text(result)));
                    }
                });
            self.photo_menu.results_open = open;
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};
    fn load(app: &mut TrueRenderer, ctx: &egui::Context) {
        for p in app.state.items.clone() {
            app.ensure_edit_loaded(&p);
        }
        let end = Instant::now() + Duration::from_secs(10);
        while app.editing.entries.values().any(|e| e.loading) {
            app.poll(ctx);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn targets(app: &TrueRenderer, item: &Item, multiple: bool) -> Targets {
        Targets {
            clicked: Some(item.clone()),
            path: item.path.clone(),
            photos: recipients(&app.state, item, multiple),
            navigation: app.photo_menu.navigation,
        }
    }
    fn run(app: &mut TrueRenderer, ctx: &egui::Context) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                app.poll(ui.ctx());
                app.process_photo_actions(ui.ctx());
            });
            output.textures_delta.clear();
            if app.photo_menu.batch.is_none() && app.photo_menu.navigation_action.is_none() {
                break;
            }
            assert!(Instant::now() < end, "{}", app.status);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn clicked_unselected_selection_and_viewer_targets_are_independent() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let a = app.state.items[0].clone();
        let b = app.state.items[1].clone();
        assert_eq!(
            recipients(&app.state, &b, true)
                .iter()
                .map(|p| &p.id)
                .collect::<Vec<_>>(),
            [&b.id]
        );
        app.state.selected.insert(b.id.clone());
        assert_eq!(recipients(&app.state, &a, true).len(), 2);
        assert_eq!(recipients(&app.state, &b, false).len(), 1);
        app.comparison.slots = [Some(a.clone()), Some(b.clone())];
        app.queue_context_action(targets(&app, &b, false), Action::Rate(5));
        run(&mut app, &ctx);
        assert_eq!(app.state.items[0].annotation.rating, 0);
        assert_eq!(app.state.items[1].annotation.rating, 5);
        assert_eq!(
            app.comparison.slots[1].as_ref().unwrap().annotation.rating,
            5
        );
    }
    #[test]
    fn rejected_history_request_finishes_the_context_operation() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        load(&mut app, &ctx);
        let item = app.state.items[0].clone();
        app.queue_context_action(targets(&app, &item, false), Action::Turn(1));
        run(&mut app, &ctx);
        app.queue_context_action(targets(&app, &item, false), Action::Undo);
        app.photo_menu.batch.as_mut().unwrap().remaining.clear();
        app.photo_menu.batch.as_mut().unwrap().active = Some(item.clone());
        let (blocked, _receiver) = std::sync::mpsc::sync_channel(0);
        let original = std::mem::replace(&mut app.service.high, blocked);
        app.execute_context_photo(&item, Action::Undo);
        app.service.high = original;
        assert!(app.photo_menu.batch.as_ref().unwrap().active.is_none());
        assert!(!app.editing.entries[&item.id].pending);
        assert_eq!(app.photo_menu.results.len(), 1);
        assert_ne!(
            app.photo_menu.results[0].1,
            app.cache_settings.language.text("Completato")
        );
    }
    #[test]
    fn deferred_crop_rejects_a_replaced_photo_at_the_same_path() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        load(&mut app, &ctx);
        let item = app.state.items[1].clone();
        app.photo_menu.extra.insert(item.id.clone(), item.clone());
        app.state.items.retain(|p| p.id != item.id);
        app.queue_context_action(targets(&app, &item, false), Action::Crop);
        app.photo_menu.batch.as_mut().unwrap().remaining.clear();
        app.photo_menu.batch.as_mut().unwrap().active = Some(item.clone());
        app.execute_context_photo(&item, Action::Crop);
        assert!(app.photo_menu.navigation_action.is_some());
        // The asynchronous scan finds a replacement at the requested path.
        let mut replacement = app.state.items[0].clone();
        replacement.path = item.path;
        app.state.current = Some(replacement.id.clone());
        app.state.items = vec![replacement];
        app.process_photo_actions(&ctx);
        assert!(app.editing.crop.is_none());
        assert!(app.photo_menu.navigation_action.is_none());
    }
    #[test]
    fn navigation_releases_a_verification_without_waiting_for_filesystem_io() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        load(&mut app, &ctx);
        let item = app.state.items[0].clone();
        let before = app.editing.entries[&item.id].draft.clone();
        app.queue_context_action(targets(&app, &item, false), Action::Turn(1));
        let batch = app.photo_menu.batch.as_mut().unwrap();
        batch.remaining.clear();
        batch.active = Some(item.clone());
        batch.verifying = Some(999);
        app.command(Command::Select {
            id: app.state.items[1].id.clone(),
            extend: false,
        });
        app.process_photo_actions(&ctx);
        assert!(app.photo_menu.batch.is_none());
        assert_eq!(app.photo_menu.results.len(), 1);
        app.context_photo_result(999, Ok(item.clone()));
        assert_eq!(app.editing.entries[&item.id].draft, before);
    }
    #[test]
    fn unavailable_deferred_crop_and_external_source_watch_release_busy_state() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        load(&mut app, &ctx);
        let item = app.state.items[0].clone();
        app.photo_menu.extra.insert(item.id.clone(), item.clone());
        app.apply_source_changes(vec![crate::source_monitor::Change {
            id: item.id.clone(),
            observation: "offline".into(),
            bytes: 0,
            available: false,
        }]);
        assert!(!app.photo_menu.extra[&item.id].approved);
        assert_eq!(app.photo_menu.extra[&item.id].observation, "offline");
        let offline = app.state.items[0].clone();
        app.photo_menu.navigation_action = Some(PendingView {
            expected: offline,
            action: Action::Crop,
            navigation: app.photo_menu.navigation,
        });
        app.process_photo_actions(&ctx);
        assert!(app.photo_menu.navigation_action.is_none());
        assert!(app.editing.crop.is_none());
        assert!(app.photo_menu.results_open);
        assert_eq!(
            app.photo_menu.results.last().unwrap().1,
            "Sorgente o ricetta non disponibile"
        );
    }
    #[test]
    fn batch_edits_history_export_and_navigation_preserve_declared_recipients() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        load(&mut app, &ctx);
        let a = app.state.items[0].clone();
        let b = app.state.items[1].clone();
        let original = std::fs::read(&a.path).unwrap();
        app.state.selected.insert(b.id.clone());
        let frozen = targets(&app, &a, true);
        app.state.selected.clear();
        app.queue_context_action(frozen, Action::Turn(1));
        run(&mut app, &ctx);
        assert_eq!(app.photo_menu.results.len(), 2);
        for p in [&a, &b] {
            assert_eq!(
                app.editing.entries[&p.id]
                    .draft
                    .as_ref()
                    .unwrap()
                    .advanced
                    .as_ref()
                    .unwrap()
                    .geometry
                    .quarter_turns,
                1
            );
        }
        app.queue_context_action(targets(&app, &b, false), Action::Undo);
        run(&mut app, &ctx);
        assert!(
            app.editing.entries[&b.id]
                .draft
                .as_ref()
                .unwrap()
                .is_neutral()
        );
        app.queue_context_action(targets(&app, &b, false), Action::Redo);
        run(&mut app, &ctx);
        assert!(
            !app.editing.entries[&b.id]
                .draft
                .as_ref()
                .unwrap()
                .is_neutral()
        );
        app.queue_context_action(targets(&app, &b, false), Action::Export);
        app.state.selected.insert(a.id.clone());
        assert_eq!(
            app.photo_export
                .targets
                .as_ref()
                .unwrap()
                .iter()
                .map(|p| &p.id)
                .collect::<Vec<_>>(),
            [&b.id]
        );
        app.photo_export.open = false;
        app.queue_context_action(targets(&app, &a, false), Action::Flip(true));
        app.photo_menu.navigation += 1;
        run(&mut app, &ctx);
        assert!(
            !app.editing.entries[&a.id]
                .draft
                .as_ref()
                .unwrap()
                .advanced
                .as_ref()
                .unwrap()
                .geometry
                .flip_horizontal
        );
        assert_eq!(std::fs::read(&a.path).unwrap(), original);
        drop(app);
        let catalog = tr_store::Catalog::open(&dir.path().join("data")).unwrap();
        assert_eq!(
            catalog
                .load_edit(&b.id, tr_core::decoder::RawEngine::Apple)
                .unwrap()
                .recipe
                .advanced
                .unwrap()
                .geometry
                .quarter_turns,
            1
        );
    }
    #[test]
    fn offline_filtered_cross_folder_and_stale_verification_do_not_redirect_edits() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        load(&mut app, &ctx);
        let a = app.state.items[0].clone();
        let b = app.state.items[1].clone();
        app.state.query = "does not match".into();
        app.state.refilter();
        app.queue_context_action(targets(&app, &b, false), Action::Rate(-1));
        run(&mut app, &ctx);
        assert_eq!(app.state.items[1].annotation.rating, -1);
        app.photo_menu.extra.insert(a.id.clone(), a.clone());
        app.state.items.retain(|p| p.id != a.id);
        app.queue_context_action(targets(&app, &a, false), Action::Flip(true));
        run(&mut app, &ctx);
        assert!(
            app.editing.entries[&a.id]
                .draft
                .as_ref()
                .unwrap()
                .advanced
                .as_ref()
                .unwrap()
                .geometry
                .flip_horizontal
        );
        std::fs::remove_file(&a.path).unwrap();
        let prior = app.editing.entries[&a.id].draft.clone();
        app.queue_context_action(targets(&app, &a, false), Action::Reset);
        run(&mut app, &ctx);
        assert_eq!(app.editing.entries[&a.id].draft, prior);
        assert!(!app.photo_menu.results[0].1.is_empty());
        app.queue_context_action(targets(&app, &a, false), Action::Rate(4));
        run(&mut app, &ctx);
        assert_eq!(app.photo_menu.extra[&a.id].annotation.rating, 4);
        // A verification delivered after navigation may not write a recipe.
        let t = targets(&app, &b, false);
        app.queue_context_action(t, Action::Reset);
        app.photo_menu.batch.as_mut().unwrap().active = Some(b.clone());
        app.photo_menu.batch.as_mut().unwrap().verifying = Some(999);
        app.photo_menu.navigation += 1;
        app.context_photo_result(999, Ok(b.clone()));
        run(&mut app, &ctx);
        assert!(
            app.editing.entries[&b.id]
                .draft
                .as_ref()
                .unwrap()
                .is_neutral()
        );
    }
    #[test]
    fn explorer_resolution_is_asynchronous_and_does_not_navigate_or_select() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[1].clone();
        let current = app.state.current.clone();
        app.photo_menu.resolving = Some((41, item.path.clone()));
        assert!(app.request(Request::ContextPhoto {
            token: 41,
            path: item.path.clone()
        }));
        let end = Instant::now() + Duration::from_secs(10);
        while app.photo_menu.resolving.is_some() {
            app.poll(&ctx);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.state.current, current);
        assert_eq!(app.photo_menu.extra[&item.id].digest, item.digest);
        assert!(app.browser.pending.is_none());
    }
    #[test]
    fn keyboard_menu_opens_for_focused_photo_in_both_languages() {
        for lang in [Language::Italian, Language::English] {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            load(&mut app, &ctx);
            app.cache_settings.language = lang;
            let item = app.state.items[1].clone();
            let before = app.state.current.clone();
            let focus = egui::Id::new("keyboard-photo");
            for events in [
                vec![],
                vec![egui::Event::Key {
                    key: egui::Key::F10,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                }],
            ] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 900.),
                        )),
                        events: std::iter::once(egui::Event::ModifiersChanged(
                            egui::Modifiers::SHIFT,
                        ))
                        .chain(events)
                        .collect(),
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(200., 200.)),
                            focus,
                            egui::Sense::click(),
                        );
                        response.request_focus();
                        app.photo_menu_for(&response, &item, true);
                    },
                );
                output.textures_delta.clear();
            }
            assert!(egui::Popup::is_any_open(&ctx));
            assert_eq!(app.state.current, before);
            assert_eq!(
                app.photo_menu
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .1
                    .clicked
                    .as_ref()
                    .unwrap()
                    .id,
                item.id
            );
            assert_ne!(
                Language::English.text("Valutazione e metadati"),
                "Valutazione e metadati"
            );
        }
    }
}
