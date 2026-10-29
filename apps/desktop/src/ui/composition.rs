//! Entry from photographic editing to the same layer/tools/export workflow.
use super::*;
pub(super) struct Confirmation {
    item: Item,
    recipe: tr_core::editing::EditRecipe,
    digest: String,
    action: document::PhotoAction,
    output: PathBuf,
}

impl TrueRenderer {
    pub(super) fn photo_editing_tab(&self) -> document::Tab {
        if self.editing.layers.view_export {
            document::Tab::Export
        } else if self.editing.layers.view_tools {
            document::Tab::Tools
        } else if self.editing.layers.view_layers {
            document::Tab::Layers
        } else {
            document::Tab::Develop
        }
    }
    pub(super) fn select_photo_editing_tab(&mut self, tab: document::Tab) {
        self.cancel_wb_picker();
        if let Some(id) = self.state.current.clone() {
            self.commit_edit(&id);
        }
        self.editing.retouch.stop();
        self.editing.layers.open_tools(false);
        self.editing.layers.view_tools = tab == document::Tab::Tools;
        self.editing.layers.view_layers = tab == document::Tab::Layers;
        self.editing.layers.view_export = tab == document::Tab::Export;
        self.editing.advanced.paint = false;
        self.editing.advanced.horizon = false;
    }
    pub(super) fn open_editing_tab(&mut self, tab: document::Tab) {
        if self.current_document() {
            self.document.select_tab(tab);
        } else {
            self.select_photo_editing_tab(tab);
        }
        self.inspector_pages = [InspectorPage::Develop; 2];
        self.show_inspector = true;
    }
    pub(super) fn photo_composition_controls(&mut self, ui: &mut egui::Ui, item: &Item) {
        if self.document.busy() || self.document.import_failed() {
            self.document.messages(ui);
        }
        ui.collapsing("Aggiungi contenuto immagine", |ui| {
            ui.small("Copie e immagini indipendenti richiedono un progetto .truerenderer.");
            let mut action = None;
            ui.horizontal_wrapped(|ui| {
                if ui.button("Copia immagine…").clicked() {
                    action = Some(document::PhotoAction::Copy);
                }
                if ui.button("Livello raster vuoto…").clicked() {
                    action = Some(document::PhotoAction::Empty);
                }
                if ui.button("Inserisci immagine…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_directory(&self.folder)
                        .pick_file()
                {
                    action = Some(document::PhotoAction::Image(path));
                }
            });
            if let Some(action) = action {
                self.request_conversion(item, action);
            }
        });
    }
    pub(super) fn request_conversion(&mut self, item: &Item, action: document::PhotoAction) {
        if self.document.busy() {
            return;
        }
        let Some(entry) = self.editing.entries.get(&item.id) else {
            return;
        };
        let Some(saved) = &entry.loaded else {
            return;
        };
        let recipe = entry.draft.clone().unwrap_or_else(|| saved.recipe.clone());
        let stem = item.path.file_stem().unwrap_or_default().to_string_lossy();
        let folder = item.path.parent().unwrap_or(&self.folder);
        let mut output = folder.join(format!("{stem}.truerenderer"));
        let mut n = 2;
        while output.exists() {
            output = folder.join(format!("{stem}-{n}.truerenderer"));
            n += 1;
        }
        self.conversion = Some(Confirmation {
            item: item.clone(),
            recipe,
            digest: saved.source_digest.clone(),
            action,
            output,
        });
    }
    pub(super) fn conversion_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.conversion else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        let response=egui::Modal::new(egui::Id::new("photo-to-project")).show(ctx, |ui| {
            ui.set_max_width(420.);
            ui.heading("Continua in un progetto TrueRenderer?");
            ui.add_space(8.);
            ui.label(format!("{} aggiunge contenuto indipendente alla foto.", pending.action.label()));
            ui.label("Creeremo un file .truerenderer con lo sviluppo corrente al livello 0 e il nuovo contenuto sopra.");
            ui.add_space(6.);
            ui.label(egui::RichText::new(pending.output.file_name().unwrap_or_default().to_string_lossy()).strong());
            ui.small(format!("Nella cartella di {}",pending.item.name));
            ui.add_space(6.);
            ui.label("Il RAW originale e le sue regolazioni restano disponibili. Nel progetto lo sviluppo sensore è incorporato: per cambiare motore o WB RAW si torna alla foto originale.");
            ui.add_space(10.);
            ui.horizontal(|ui| {
                cancel=ui.button("Continua sulla foto").clicked();
                accept=ui.button("Crea progetto e continua").clicked();
            });
        });
        cancel |= response.should_close();
        if cancel {
            self.conversion = None;
        } else if accept {
            let pending = self.conversion.take().unwrap();
            self.accept_conversion(pending);
        }
    }
    fn accept_conversion(&mut self, pending: Confirmation) {
        if !self.state.current_item().is_some_and(|i| {
            i.id == pending.item.id
                && i.path == pending.item.path
                && i.observation == pending.item.observation
        }) {
            self.status =
                "La foto selezionata è cambiata: richiedi nuovamente l'operazione.".into();
            return;
        }
        self.commit_all_edits();
        self.cancel_wb_picker();
        if self.document.begin_photo(
            pending.item.id,
            pending.item.path,
            pending.recipe,
            pending.digest,
            document::Tab::Layers,
        ) {
            self.document
                .begin_conversion(pending.output, pending.action);
        }
    }
    pub(super) fn current_document(&self) -> bool {
        self.state
            .current_item()
            .is_some_and(|i| self.document.matches(&i.path))
    }
    pub(super) fn sync_document(&mut self, ctx: &egui::Context) {
        self.document.directory = self.folder.clone();
        self.document.poll();
        if self.document.busy() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let mut refresh = false;
        while let Some(path) = self.document.published.pop_front() {
            self.document.invalidate_preview(&path);
            refresh |= path.parent() == Some(self.folder.as_path());
        }
        if let Some(path) = self.document.reveal.take() {
            self.open_path(path);
            self.inspector_pages = [InspectorPage::Develop; 2];
            self.show_inspector = true;
            return;
        } else if refresh {
            self.refresh_folder();
        }
        // While navigation resolves, the previous selection is still visible.
        // It must not replace a newly saved document and drop its undo history.
        if self.scanning || self.browser.pending.is_some() {
            return;
        }
        if let Some(item) = self.state.current_item().cloned() {
            let previously = self.document.matches(&item.path);
            if self.document.select(&item.path) && !previously {
                self.inspector_pages = [InspectorPage::Develop; 2];
                self.state.transform = Default::default();
            }
            if self.document.matches(&item.path)
                && let Some(tab) = self.document.requested_tab.take()
            {
                self.document.select_tab(tab);
                self.inspector_pages = [InspectorPage::Develop; 2];
                self.show_inspector = true;
            }
            if self.document.loading(&item.path) {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }
    pub(super) fn document_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(30));
        let names = [
            "document-develop",
            "document-layers",
            "document-tools",
            "document-export",
        ];
        if self.smoke_stage < 8 && self.current_document() && !self.scanning {
            let index = self.smoke_stage as usize / 2;
            self.inspector_pages = [InspectorPage::Develop; 2];
            self.show_inspector = true;
            if self.smoke_stage.is_multiple_of(2) {
                self.document.select_tab(document::Tab::from_index(index));
                if self.frame_number.is_multiple_of(8) {
                    self.capture_screenshot(ctx, names[index]);
                    self.smoke_stage += 1;
                }
            } else if self.screenshots.contains(names[index]) {
                self.smoke_stage += 1;
            }
        }
        if self.smoke_stage == 8 {
            self.state.view = ViewMode::Grid;
            self.cell_size = 128.;
            self.smoke_stage = 9;
        } else if self.smoke_stage == 9
            && self.frame_number.is_multiple_of(8)
            && self.pending_images.is_empty()
            && self.cache.len()
                >= self
                    .state
                    .items
                    .iter()
                    .filter(|i| !tr_core::document::is_document_path(&i.path))
                    .count()
                    .min(4)
        {
            self.capture_screenshot(ctx, "document-grid");
            self.smoke_stage = 10;
        }
        if (self.smoke_stage == 10 && self.screenshots.contains("document-grid"))
            || (self.smoke_stage < 11 && self.document.import_failed())
            || (self.smoke_stage < 11 && self.started.elapsed() > Duration::from_secs(180))
        {
            self.smoke_stage = 11;
            let mut report = self.document.diagnostic();
            if std::env::args().any(|a| a == "--native-document-smoke") {
                report["native"] = self.document.verify_native_source(&self.root);
                report["passed"] = serde_json::json!(
                    report["passed"] == true && report["native"]["passed"] == true
                );
            } else if std::env::args().any(|a| a == "--composition-smoke") {
                report["workflow"] = serde_json::json!(self.document.verify_workflow(&self.root));
                report["passed"] = serde_json::json!(
                    report["passed"] == true && report["workflow"]["passed"] == true
                );
            }
            report["normal_browser"] = serde_json::json!({"folder":self.folder,"items":self.state.items.len(),"selected":self.state.current,"filmstrip":self.show_filmstrip,"view":format!("{:?}",self.state.view)});
            report["passed"] = serde_json::json!(
                report["passed"] == true
                    && self.current_document()
                    && names.iter().all(|n| self.screenshots.contains(*n))
                    && self.screenshots.contains("document-grid")
            );
            report["screenshots"] = serde_json::json!(self.screenshots);
            let _ = std::fs::write(
                self.root.join("reports/document-ui.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn begin_composition(&mut self, tab: document::Tab) {
        let Some(item) = self.state.current_item().cloned() else {
            self.document.new_document();
            return;
        };
        if self.document.matches(&item.path) || tr_core::document::is_document_path(&item.path) {
            if self.document.select(&item.path) {
                self.document.select_tab(tab);
                self.inspector_pages = [InspectorPage::Develop; 2];
            }
            return;
        }
        self.ensure_edit_loaded(&item);
        let Some(recipe) = self.view_recipe(&item.id) else {
            self.status = "Attendere il caricamento delle regolazioni della foto".into();
            return;
        };
        if self.editing.crop.is_some() {
            self.status = "Conferma o annulla il ritaglio prima di lavorare sui livelli".into();
            return;
        }
        let Some(digest) = self
            .editing
            .entries
            .get(&item.id)
            .and_then(|entry| entry.loaded.as_ref())
            .map(|saved| saved.source_digest.clone())
        else {
            return;
        };
        self.inspector_pages = [InspectorPage::Develop; 2];
        self.show_inspector = true;
        self.commit_all_edits();
        self.cancel_wb_picker();
        self.status.clear();
        self.document
            .begin_photo(item.id, item.path, recipe, digest, tab);
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    #[test]
    fn native_projects_use_normal_inventory_selection_and_never_enter_photo_decoding() {
        let (dir, ctx, mut app) = super::super::settings_regressions::app();
        let path = dir.path().join("corpus/third.truerenderer");
        tr_store::document::create(
            &path,
            &tr_core::document::Document::new(16, 12, 300., [0.; 4]).unwrap(),
        )
        .unwrap();
        super::super::settings_regressions::settle(&mut app, &ctx, true);
        app.open_path(path.clone());
        super::super::settings_regressions::settle(&mut app, &ctx, true);
        app.sync_document(&ctx);
        let path = path.canonicalize().unwrap();
        let item = app.state.current_item().cloned().unwrap();
        assert_eq!(item.path, path);
        assert_eq!(app.folder, path.parent().unwrap());
        assert_eq!(app.state.items.len(), 3);
        assert_eq!(app.state.view, ViewMode::Preview);
        assert!(app.current_document());
        app.ensure_edit_loaded(&item);
        app.ensure_image(&item, 256);
        assert!(!app.editing.entries.contains_key(&item.id));
        assert!(!app.pending_images.iter().any(|(id, _)| id == &item.id));
        assert!(!app.demand.iter().any(|(id, _)| id == &item.id));
        app.start_folder_preparation(true);
        assert!(!app.rebuild.iter().any(|i| i.id == item.id));
        let photo = app
            .state
            .items
            .iter()
            .find(|i| i.path.extension().unwrap() == "png")
            .unwrap()
            .clone();
        app.command(Command::Select {
            id: photo.id.clone(),
            extend: false,
        });
        app.sync_document(&ctx);
        assert!(!app.current_document());
        assert!(app.document.has_document());
        app.command(Command::Select {
            id: item.id,
            extend: false,
        });
        app.sync_document(&ctx);
        assert!(app.current_document());
        let copied = path.with_file_name("fourth.truerenderer");
        std::fs::copy(&path, &copied).unwrap();
        assert!(app.document.open(copied.clone()));
        app.document.select_tab(document::Tab::Export);
        app.document.reveal = Some(copied.clone());
        app.sync_document(&ctx);
        assert!(app.document.matches(&copied));
        super::super::settings_regressions::settle(&mut app, &ctx, true);
        app.sync_document(&ctx);
        assert_eq!(app.state.current_item().unwrap().path, copied);
        assert!(app.document.matches(&copied));
        assert_eq!(app.document.tab, document::Tab::Export);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400., 900.),
                )),
                ..Default::default()
            },
            |ui| {
                app.toolbar(ui);
                app.location_bar(ui);
                app.footer(ui);
                app.sidebar(ui);
                app.inspector(ui);
                egui::CentralPanel::default().show(ui, |ui| app.preview(ui));
            },
        );
        fn labels(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| labels(s, out)),
                _ => {}
            }
        }
        let mut text = vec![];
        for shape in &output.shapes {
            labels(&shape.shape, &mut text);
        }
        assert!(text.iter().any(|s| s == "TrueRenderer"));
        assert!(text.iter().any(|s| s == "Export"), "{text:?}");
        assert!(
            !text
                .iter()
                .any(|s| s == "Chiudi progetto" || s == "Spazio documento")
        );
        output.textures_delta.clear();
    }
}
