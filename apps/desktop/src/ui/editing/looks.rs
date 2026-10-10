use super::*;
use tr_core::editing::{layers::Id, looks::Look};
use tr_store::{LookEdit, SavedLook};

pub(in crate::ui) struct Controls {
    pub open: bool,
    items: Vec<SavedLook>,
    loaded: bool,
    pending: bool,
    error: Option<String>,
    selected: Option<String>,
    chosen: Vec<Id>,
    amount: f32,
    masks: bool,
    search: String,
    archived: bool,
    rename: String,
    source: String,
    save_chosen: Vec<Id>,
    save_name: String,
    save_masks: bool,
    preview: Option<Preview>,
    smoke_stage: u8,
    pub(super) smoke_result: Option<serde_json::Value>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            open: false,
            items: vec![],
            loaded: false,
            pending: false,
            error: None,
            selected: None,
            chosen: vec![],
            amount: 1.,
            masks: false,
            search: String::new(),
            archived: false,
            rename: String::new(),
            source: String::new(),
            save_chosen: vec![],
            save_name: String::new(),
            save_masks: false,
            preview: None,
            smoke_stage: 0,
            smoke_result: None,
        }
    }
}
struct Preview {
    item: Item,
    generation: u64,
    look_id: String,
    look_revision: u64,
    name: String,
    before: EditRecipe,
    after: EditRecipe,
    show: bool,
}
enum Action {
    Refresh,
    Edit(LookEdit),
    Preview(SavedLook),
}
impl Controls {
    pub(in crate::ui) fn previewing(&self) -> bool {
        self.preview.is_some()
    }
    pub(in crate::ui) fn receive(&mut self, result: Result<Vec<SavedLook>, String>) {
        self.pending = false;
        match result {
            Ok(items) => {
                let new = items
                    .iter()
                    .find(|l| !l.archived && !self.items.iter().any(|old| old.id == l.id))
                    .map(|l| l.id.clone());
                self.items = items;
                self.loaded = true;
                self.error = None;
                if let Some(id) = new.or_else(|| self.selected.clone())
                    && self.selected.as_ref() != Some(&id)
                {
                    self.select(&id);
                }
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn select(&mut self, id: &str) {
        if let Some(look) = self.items.iter().find(|l| l.id == id) {
            self.selected = Some(id.into());
            self.rename = look.name.clone();
            self.chosen = look.look.layers.iter().map(|l| l.id).collect();
            self.amount = 1.;
            self.masks = false;
        }
    }
    fn bind(&mut self, id: &str, recipe: &EditRecipe) {
        if self.source != id {
            self.source = id.into();
            self.save_chosen = recipe
                .layers
                .as_ref()
                .map_or(vec![], |s| s.layers.iter().map(|l| l.id).collect());
        }
        self.save_chosen.retain(|id| {
            recipe
                .layers
                .as_ref()
                .is_some_and(|s| s.layers.iter().any(|l| l.id == *id))
        });
    }
    fn draw(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        recipe: &EditRecipe,
        ready: bool,
    ) -> Option<Action> {
        let mut action = None;
        ui.small(lang.text("I look aggiungono copie indipendenti dei livelli. RAW, base e ritaglio restano quelli della foto."));
        if let Some(error) = &self.error {
            ui.colored_label(AMBER, lang.text(error));
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.pending && (self.loaded || self.error.is_some()),
                    egui::Button::new(lang.text("Aggiorna elenco")),
                )
                .clicked()
            {
                action = Some(Action::Refresh);
            }
            ui.checkbox(&mut self.archived, lang.text("Mostra archiviati"));
        });
        if self.pending {
            ui.label(lang.text("Salvataggio…"));
        }
        if !self.loaded {
            ui.label(lang.text("Caricamento look…"));
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(lang.text("Cerca look…"))
                .char_limit(128),
        );
        let query = self.search.to_lowercase();
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("saved-look-list")
            .max_height(150.)
            .show(ui, |ui| {
                for look in &self.items {
                    if (self.archived || !look.archived)
                        && look.name.to_lowercase().contains(&query)
                        && ui
                            .selectable_label(
                                self.selected.as_deref() == Some(&look.id),
                                &look.name,
                            )
                            .clicked()
                    {
                        selected = Some(look.id.clone());
                    }
                }
            });
        if let Some(id) = selected {
            self.select(&id);
        }
        if self.loaded && self.items.is_empty() {
            ui.small(lang.text("Nessun look salvato. Crea il primo dai livelli di questa foto."));
        }
        if let Some(look) = self
            .items
            .iter()
            .find(|l| Some(&l.id) == self.selected.as_ref())
            .cloned()
        {
            ui.separator();
            ui.strong(&look.name);
            if look.archived {
                ui.label(lang.text("Look archiviato"));
            }
            for layer in &look.look.layers {
                let mut on = self.chosen.contains(&layer.id);
                if ui.checkbox(&mut on, &layer.name).changed() {
                    self.chosen.retain(|id| *id != layer.id);
                    if on {
                        self.chosen.push(layer.id);
                    }
                }
            }
            ui.add(egui::Slider::new(&mut self.amount, 0. ..=1.).text(lang.text("Intensità look")));
            ui.checkbox(&mut self.masks, lang.text("Usa maschere salvate"));
            ui.small(lang.text(
                "Le maschere usano coordinate relative alla sorgente; i valori Auto restano fissi.",
            ));
            if ui
                .add_enabled(
                    ready
                        && !self.pending
                        && !look.archived
                        && !self.chosen.is_empty()
                        && self.amount > 0.,
                    egui::Button::new(lang.text("Prova sulla foto")),
                )
                .clicked()
            {
                action = Some(Action::Preview(look.clone()));
            }
            egui::CollapsingHeader::new(lang.text("Gestisci look")).id_salt("manage-look").show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.rename).hint_text(lang.text("Nome look")).char_limit(128));
                let name = self.rename.trim();
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.pending && !name.is_empty() && name.len() <= 128 && name != look.name, egui::Button::new(lang.text("Rinomina"))).clicked() {
                        action = Some(Action::Edit(LookEdit::Rename { id: look.id.clone(), expected: look.revision, name: name.into() }));
                    }
                    if ui.add_enabled(!self.pending, egui::Button::new(lang.text(if look.archived { "Ripristina look" } else { "Archivia look" }))).clicked() {
                        action = Some(Action::Edit(LookEdit::Archive { id: look.id.clone(), expected: look.revision, archived: !look.archived }));
                    }
                });
                ui.small(lang.text("I look archiviati si possono ripristinare. Le foto già modificate restano indipendenti."));
            });
        }
        ui.separator();
        egui::CollapsingHeader::new(lang.text("Salva dai livelli della foto")).id_salt("capture-look").show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.save_name).hint_text(lang.text("Nome look")).char_limit(128));
            if let Some(stack) = &recipe.layers {
                for layer in &stack.layers {
                    let mut on = self.save_chosen.contains(&layer.id);
                    if ui.checkbox(&mut on, &layer.name).changed() {
                        self.save_chosen.retain(|id| *id != layer.id);
                        if on { self.save_chosen.push(layer.id); }
                    }
                }
            }
            ui.checkbox(&mut self.save_masks, lang.text("Includi maschere nel look"));
            let name = self.save_name.trim();
            if ui.add_enabled(ready && self.loaded && !self.pending && !self.save_chosen.is_empty() && !name.is_empty() && name.len() <= 128, egui::Button::new(lang.text("Salva nuovo look"))).clicked() {
                match Look::capture(recipe, &self.save_chosen, self.save_masks) {
                    Ok(look) => action = Some(Action::Edit(LookEdit::Create { name: name.into(), look })),
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            ui.small(lang.text("Solo i livelli selezionati; le regolazioni della base non fanno parte del look."));
        });
        action
    }
}

impl TrueRenderer {
    pub(super) fn toggle_looks(&mut self, item: &Item) {
        self.editing.looks.open = !self.editing.looks.open;
        self.finish_look_preview(false);
        if self.editing.looks.open {
            let selected = self.editing.layers.selected;
            let view_layers = self.editing.layers.view_layers;
            let view_tools = self.editing.layers.view_tools;
            self.editing.layers = Default::default();
            self.editing.layers.bind(&item.id);
            self.editing.layers.selected = selected;
            self.editing.layers.view_layers = view_layers;
            self.editing.layers.view_tools = view_tools;
            self.editing.advanced = Default::default();
            self.clear_edit_preview_for(&item.id);
        }
    }
    /// Explicit native verification only; normal UI never runs this workflow.
    pub(super) fn look_smoke_step(&mut self, item: &Item) -> bool {
        if self.editing.looks.pending {
            return false;
        }
        if let Some(error) = self.editing.looks.error.clone() {
            self.status = error;
            self.fatal = true;
            return false;
        }
        let Some(entry) = self.editing.entries.get(&item.id) else {
            return false;
        };
        let Some(saved) = entry.loaded.clone() else {
            return false;
        };
        if entry.pending || entry.loading {
            return false;
        }
        match self.editing.looks.smoke_stage {
            0 if self.editing.looks.loaded => {
                let source = crate::verify_advanced::curves_recipe(saved.recipe.raw_engine, false);
                let ids = source
                    .layers
                    .as_ref()
                    .unwrap()
                    .layers
                    .iter()
                    .map(|l| l.id)
                    .collect::<Vec<_>>();
                let look = Look::capture(&source, &ids, true).unwrap();
                if self.request(Request::EditLook(LookEdit::Create {
                    name: "Curve riutilizzabili".into(),
                    look,
                })) {
                    self.editing.looks.pending = true;
                    self.editing.looks.smoke_stage = 1;
                }
                false
            }
            1 => {
                let Some(look) = self
                    .editing
                    .looks
                    .items
                    .iter()
                    .find(|l| l.name == "Curve riutilizzabili")
                    .cloned()
                else {
                    return false;
                };
                self.editing.looks.open = true;
                self.editing.looks.select(&look.id);
                self.editing.looks.amount = 0.65;
                self.editing.looks.masks = true;
                self.start_look_preview(item, look.clone());
                if self.editing.looks.preview.is_none() {
                    return false;
                }
                let trial = self.view_recipe(&item.id).unwrap();
                assert_ne!(trial, saved.recipe);
                assert!(!self.edits_have_pending());
                self.finish_look_preview(false);
                assert_eq!(self.view_recipe(&item.id), Some(saved.recipe));
                self.start_look_preview(item, look);
                self.finish_look_preview(true);
                self.editing.looks.open = true;
                self.editing.looks.smoke_stage = 2;
                false
            }
            2 => {
                self.editing.looks.smoke_result = Some(
                    serde_json::json!({"passed":true,"saved_looks":self.editing.looks.items.len(),"trial_without_revision":true,"cancel_preserved_recipe":true,"confirmed_generation":saved.generation,"independent_applied_layers":saved.recipe.layers.as_ref().map_or(0,|s|s.layers.len()),"scope":"Automated service/control workflow, not manual pointer interaction. Reopen, backup restore and Undo/Redo are covered by separate regressions."}),
                );
                true
            }
            _ => false,
        }
    }
    fn look_preview_valid(&self, p: &Preview) -> bool {
        self.state.view == ViewMode::Preview
            && self.editing.looks.open
            && self.show_inspector
            && self.inspector_pages[1] == InspectorPage::Develop
            && self.state.current.as_ref() == Some(&p.item.id)
            && self.editing.crop.is_none()
            && !self.editing.wb_pending
            && self
                .editing
                .looks
                .items
                .iter()
                .any(|l| l.id == p.look_id && l.revision == p.look_revision && !l.archived)
            && self.known_items().any(|i| {
                i.id == p.item.id
                    && i.digest == p.item.digest
                    && i.observation == p.item.observation
                    && i.approved
            })
            && self.editing.entries.get(&p.item.id).is_some_and(|e| {
                !e.pending
                    && !e.loading
                    && e.error.is_none()
                    && e.draft.as_ref() == Some(&p.before)
                    && e.loaded
                        .as_ref()
                        .is_some_and(|s| s.generation == p.generation && s.recipe == p.before)
            })
    }
    pub(in crate::ui) fn look_preview_recipe(&self, id: &str) -> Option<EditRecipe> {
        let p = self.editing.looks.preview.as_ref()?;
        (p.item.id == id && self.look_preview_valid(p)).then(|| {
            if p.show {
                p.after.clone()
            } else {
                p.before.clone()
            }
        })
    }
    pub(in crate::ui) fn reconcile_look_preview(&mut self) {
        if self
            .editing
            .looks
            .preview
            .as_ref()
            .is_some_and(|p| !self.look_preview_valid(p))
        {
            self.finish_look_preview(false);
        }
    }
    fn start_look_preview(&mut self, item: &Item, look: SavedLook) {
        let Some(saved) = self
            .editing
            .entries
            .get(&item.id)
            .and_then(|e| e.loaded.as_ref())
            .cloned()
        else {
            return;
        };
        let controls = &self.editing.looks;
        match look.look.append_to(
            &saved.recipe,
            &controls.chosen,
            controls.amount,
            controls.masks,
        ) {
            Ok(after) => {
                let p = Preview {
                    item: item.clone(),
                    generation: saved.generation,
                    look_id: look.id,
                    look_revision: look.revision,
                    name: look.name,
                    before: saved.recipe,
                    after,
                    show: true,
                };
                if !self.look_preview_valid(&p) {
                    return;
                }
                self.editing.layers = Default::default();
                self.editing.advanced = Default::default();
                self.editing.show_original = false;
                self.clear_edit_preview_for(&item.id);
                self.editing.looks.preview = Some(p);
                self.editing.looks.error = None;
            }
            Err(error) => self.editing.looks.error = Some(error.to_string()),
        }
    }
    pub(in crate::ui) fn finish_look_preview(&mut self, confirm: bool) {
        let Some(p) = self.editing.looks.preview.take() else {
            return;
        };
        let valid = self.look_preview_valid(&p);
        self.clear_edit_preview_for(&p.item.id);
        if confirm && valid {
            self.editing.layers.bind(&p.item.id);
            self.editing.layers.view_layers = true;
            self.editing.layers.view_tools = false;
            self.editing.layers.selected = p
                .after
                .layers
                .as_ref()
                .and_then(|s| s.layers.last())
                .map(|l| l.id);
            self.editing.looks.open = false;
            self.apply_edit_draft(&p.item.id, p.after);
            self.commit_edit(&p.item.id);
        } else if confirm {
            self.editing.looks.error =
                Some("La foto o il look sono cambiati: ripeti la prova".into());
        }
    }
    pub(super) fn look_controls(&mut self, ui: &mut egui::Ui, item: &Item) {
        let lang = self.cache_settings.language;
        self.reconcile_look_preview();
        if let Some(p) = &mut self.editing.looks.preview {
            ui.heading(&p.name);
            ui.label(lang.text("Prova temporanea · nessuna modifica salvata"));
            let mut redraw = false;
            ui.horizontal_wrapped(|ui| {
                redraw |= ui
                    .selectable_value(&mut p.show, false, lang.text("Senza look"))
                    .changed();
                redraw |= ui
                    .selectable_value(&mut p.show, true, lang.text("Con look"))
                    .changed();
            });
            let confirm = ui.button(lang.text("Applica look")).clicked();
            let cancel = ui.button(lang.text("Annulla prova")).clicked()
                || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            ui.small(lang.text("Conferma per aggiungere i livelli. Esc annulla; l’export usa la ricetta confermata."));
            if confirm || cancel {
                self.finish_look_preview(confirm && !cancel);
            } else if redraw {
                self.clear_edit_preview_for(&item.id);
            }
            return;
        }
        let Some(entry) = self.editing.entries.get(&item.id) else {
            return;
        };
        let Some(recipe) = entry.draft.clone() else {
            ui.label(lang.text("Caricamento ricetta…"));
            return;
        };
        let ready = !entry.pending
            && !entry.loading
            && !entry.dirty()
            && entry.error.is_none()
            && !self.editing.wb_pending
            && self.state.view == ViewMode::Preview
            && self.state.current.as_ref() == Some(&item.id);
        self.editing.looks.bind(&item.id, &recipe);
        let action = self.editing.looks.draw(ui, lang, &recipe, ready);
        match action {
            Some(Action::Preview(look)) => self.start_look_preview(item, look),
            Some(action) => {
                let request = match action {
                    Action::Refresh => Request::ListLooks,
                    Action::Edit(edit) => Request::EditLook(edit),
                    _ => unreachable!(),
                };
                if self.request(request) {
                    self.editing.looks.pending = true;
                } else {
                    self.editing.looks.error = Some(self.status.clone());
                }
            }
            None => {}
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};
    use tr_core::editing::layers::{Colorize, Layer, Operator};

    fn wait(app: &mut TrueRenderer, ctx: &egui::Context, done: impl Fn(&TrueRenderer) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll(ctx);
            if done(app) {
                break;
            }
            assert!(Instant::now() < deadline, "{}", app.status);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn setup() -> (
        tempfile::TempDir,
        egui::Context,
        TrueRenderer,
        Item,
        SavedLook,
    ) {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.state.current = Some(item.id.clone());
        app.state.view = ViewMode::Preview;
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, |a| {
            a.editing.looks.loaded
                && a.editing
                    .entries
                    .get(&item.id)
                    .is_some_and(|e| e.loaded.is_some() && !e.loading)
        });
        let mut source = EditRecipe::neutral(Default::default());
        source.layer_stack().layers.push(Layer::new(
            "Creative tone",
            Operator::Colorize(Colorize {
                amount: 25.,
                ..Default::default()
            }),
        ));
        let look = Look::capture(
            &source,
            &[source.layers.as_ref().unwrap().layers[0].id],
            false,
        )
        .unwrap();
        assert!(app.request(Request::EditLook(LookEdit::Create {
            name: "My look".into(),
            look
        })));
        app.editing.looks.pending = true;
        wait(&mut app, &ctx, |a| {
            !a.editing.looks.pending && a.editing.looks.items.len() == 1
        });
        app.editing.looks.open = true;
        let saved = app.editing.looks.items[0].clone();
        (dir, ctx, app, item, saved)
    }
    #[test]
    fn look_trial_cancel_confirm_and_undo_use_one_saved_revision_and_independent_layers() {
        let (dir, ctx, mut app, item, look) = setup();
        let original = std::fs::read(&item.path).unwrap();
        let before = app.editing.entries[&item.id].draft.clone().unwrap();
        app.start_look_preview(&item, look.clone());
        let trial = app.view_recipe(&item.id).unwrap();
        assert_ne!(trial, before);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&before));
        assert!(!app.edits_have_pending());
        app.editing.looks.preview.as_mut().unwrap().show = false;
        assert_eq!(app.view_recipe(&item.id).unwrap(), before);
        app.finish_look_preview(false);
        assert_eq!(app.view_recipe(&item.id).unwrap(), before);
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .generation,
            0
        );
        app.start_look_preview(&item, look.clone());
        let expected = app.view_recipe(&item.id).unwrap();
        app.finish_look_preview(true);
        wait(&mut app, &ctx, |a| !a.editing.entries[&item.id].pending);
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .generation,
            1
        );
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe,
            expected
        );
        app.step_edit(&item.id, true);
        wait(&mut app, &ctx, |a| !a.editing.entries[&item.id].pending);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&before));
        app.step_edit(&item.id, false);
        wait(&mut app, &ctx, |a| !a.editing.entries[&item.id].pending);
        assert_eq!(
            app.editing.entries[&item.id].draft.as_ref(),
            Some(&expected)
        );
        assert_eq!(std::fs::read(&item.path).unwrap(), original);
        drop(app);
        let c = tr_store::Catalog::open(&dir.path().join("data")).unwrap();
        assert_eq!(c.looks().unwrap(), vec![look]);
        assert_eq!(
            c.load_edit(&item.id, before.raw_engine).unwrap().recipe,
            expected
        );
    }
    #[test]
    fn look_preview_rejects_changed_photo_history_look_and_hidden_context() {
        for change in 0..8 {
            let (_dir, ctx, mut app, item, look) = setup();
            let before = app.editing.entries[&item.id].draft.clone().unwrap();
            app.start_look_preview(&item, look);
            assert!(app.editing.looks.previewing());
            match change {
                0 => app.state.current = Some(app.state.items[1].id.clone()),
                1 => app.state.view = ViewMode::Grid,
                2 => app.state.items[0].digest.push('x'),
                3 => app.editing.looks.items[0].revision += 1,
                4 => {
                    app.editing
                        .entries
                        .get_mut(&item.id)
                        .unwrap()
                        .loaded
                        .as_mut()
                        .unwrap()
                        .generation += 1
                }
                5 => app.show_inspector = false,
                6 => app.inspector_pages[1] = InspectorPage::Information,
                _ => app.editing.looks.items[0].archived = true,
            }
            assert!(app.look_preview_recipe(&item.id).is_none());
            app.finish_look_preview(true);
            app.poll(&ctx);
            assert!(!app.editing.looks.previewing());
            assert!(!app.edits_have_pending());
            assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&before));
        }
    }
    #[test]
    fn look_preview_esc_and_other_edits_cancel_without_saving_the_trial() {
        let (_dir, ctx, mut app, item, look) = setup();
        let before = app.editing.entries[&item.id].draft.clone().unwrap();
        app.start_look_preview(&item, look.clone());
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |_| app.poll(&ctx),
        );
        output.textures_delta.clear();
        assert!(!app.editing.looks.previewing());
        assert_eq!(app.view_recipe(&item.id).unwrap(), before);
        app.start_look_preview(&item, look);
        let mut changed = before;
        changed.exposure_ev = 0.5;
        app.apply_edit_draft(&item.id, changed.clone());
        assert!(!app.editing.looks.previewing());
        assert_eq!(app.view_recipe(&item.id).unwrap(), changed);
    }
    #[test]
    fn look_panels_localize_wrap_and_gate_trial_without_mutating_the_photo() {
        for lang in [Language::Italian, Language::English] {
            for ready in [false, true] {
                let ctx = egui::Context::default();
                let mut recipe = EditRecipe::neutral(Default::default());
                recipe.layer_stack().layers.push(Layer::new(
                    "Creative tone",
                    Operator::Colorize(Colorize {
                        amount: 25.,
                        ..Default::default()
                    }),
                ));
                let id = recipe.layers.as_ref().unwrap().layers[0].id;
                let mut controls = Controls::default();
                controls.receive(Ok(vec![SavedLook {
                    id: Id::new_v4().to_string(),
                    name: "My look".into(),
                    revision: 0,
                    archived: false,
                    look: Look::capture(&recipe, &[id], false).unwrap(),
                }]));
                controls.bind("photo", &recipe);
                let before = recipe.clone();
                let mut trial = None;
                let mut button = None;
                for n in 0..3 {
                    let events = if n == 0 {
                        vec![]
                    } else {
                        let pos = button.unwrap();
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: n == 1,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]
                    };
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(320., 1600.),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            trial = controls.draw(ui, lang, &recipe, ready);
                        },
                    );
                    output.textures_delta.clear();
                    fn visit(shape: &egui::Shape, label: &str, button: &mut Option<egui::Pos2>) {
                        match shape {
                            egui::Shape::Text(t) if t.galley.job.text == label => {
                                assert!(t.galley.rect.width() < 320.);
                                *button = Some(t.galley.rect.translate(t.pos.to_vec2()).center());
                            }
                            egui::Shape::Vec(shapes) => {
                                for s in shapes {
                                    visit(s, label, button);
                                }
                            }
                            _ => {}
                        }
                    }
                    for s in output.shapes {
                        visit(&s.shape, lang.text("Prova sulla foto"), &mut button);
                    }
                }
                assert_eq!(matches!(trial, Some(Action::Preview(_))), ready);
                assert_eq!(recipe, before);
                controls.chosen.clear();
                assert!(!controls.pending);
            }
        }
    }
}
