use super::*;
use std::sync::atomic::AtomicBool;
use tr_core::export::{Compression, Format, Options};

#[derive(Default)]
pub(super) struct ExportUi {
    pub open: bool,
    pub targets: Option<Vec<Item>>,
    pub(in crate::ui) options: Options,
    destination: Option<PathBuf>,
    remaining: VecDeque<(Item, tr_store::LoadedEdit)>,
    total: usize,
    completed: usize,
    succeeded: usize,
    active: bool,
    current: String,
    cancel: Arc<AtomicBool>,
    messages: Vec<String>,
}
impl TrueRenderer {
    pub(super) fn export_result(&mut self, result: Result<crate::photo_export::Completed, String>) {
        let state = &mut self.photo_export;
        state.active = false;
        state.completed += 1;
        let message = match result {
            Ok(done) => {
                state.succeeded += 1;
                format!(
                    "{} · {}×{} · {} canali ritagliati\n{}",
                    done.path.display(),
                    done.info.width,
                    done.info.height,
                    done.info.clipped_channels,
                    done.info.description
                )
            }
            Err(error) => format!("{}: {error}", state.current),
        };
        self.status = message.clone();
        state.messages.push(message);
        if state.cancel.load(Ordering::Acquire) {
            state.remaining.clear();
        }
        self.export_next();
    }
    fn export_next(&mut self) {
        let state = &mut self.photo_export;
        if state.active || state.cancel.load(Ordering::Acquire) {
            return;
        }
        let Some((item, edit)) = state.remaining.pop_front() else {
            return;
        };
        state.current = item.name.clone();
        state.active = true;
        let job = crate::photo_export::Job {
            item,
            options: state.options,
            engine: edit.recipe.raw_engine,
            source_digest: edit.source_digest,
            recipe: Some(edit.recipe),
            destination: state.destination.clone().unwrap(),
            cancel: state.cancel.clone(),
        };
        if !self.request(Request::ExportPhoto(Box::new(job))) {
            self.photo_export.active = false;
            self.photo_export.remaining.clear();
            self.photo_export
                .messages
                .push("Servizio occupato: export non avviato; riprovare.".into());
        }
    }
    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.photo_export.open {
            return;
        }
        let lang = self.cache_settings.language;
        let selected: Vec<_> = self.photo_export.targets.clone().unwrap_or_else(|| {
            self.state
                .items
                .iter()
                .filter(|item| self.state.selected.contains(&item.id))
                .cloned()
                .collect()
        });
        let mut open = true;
        egui::Window::new(lang.text("Esporta fotografie"))
            .id(egui::Id::new("photo-export"))
            .open(&mut open)
            .default_width(540.)
            .max_width((ctx.content_rect().width() - 64.).max(280.))
            .default_pos(egui::pos2(
                ((ctx.content_rect().width() - 572.) / 2.).max(16.),
                96.,
            ))
            .collapsible(false)
            .resizable(false)
            .vscroll(true)
            .max_height((ctx.content_rect().height() - 70.).max(180.))
            .show(ctx, |ui| self.photo_export_controls(ui, selected));
        self.photo_export.open = open;
        if !open {
            self.photo_export.targets = None;
        }
    }
    pub(super) fn photo_export_controls(&mut self, ui: &mut egui::Ui, selected: Vec<Item>) {
        let lang = self.cache_settings.language;
        for item in &selected {
            self.ensure_edit_loaded(item);
        }
        let ready = selected.iter().all(|item| {
            self.editing.entries.get(&item.id).is_some_and(|entry| {
                entry.loaded.is_some()
                    && !entry.loading
                    && !entry.pending
                    && entry.error.is_none()
                    && entry
                        .draft
                        .as_ref()
                        .zip(entry.loaded.as_ref())
                        .is_some_and(|(draft, saved)| draft == &saved.recipe)
            })
        });
        let frozen: VecDeque<_> = if ready {
            selected
                .iter()
                .filter_map(|item| {
                    self.editing
                        .entries
                        .get(&item.id)
                        .and_then(|entry| entry.loaded.clone())
                        .map(|mut edit| {
                            if edit.generation == 0 {
                                edit.recipe.raw_engine = self.service.cache.settings().raw_engine;
                            }
                            (item.clone(), edit)
                        })
                })
                .collect()
        } else {
            VecDeque::new()
        };
        let state = &mut self.photo_export;
        let mut start = false;
        ui.label(
            RichText::new(
                lang.text("Nuovi file: nessun originale o export esistente viene sostituito."),
            )
            .small()
            .color(MUTED),
        );
        ui.add_enabled_ui(!state.active && state.remaining.is_empty(), |ui| {
                    egui::Frame::new().fill(style::SURFACE).corner_radius(5).inner_margin(10).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let selection = if selected.len() == 1 {
                            localized_format!(lang, "{} fotografia selezionata · sviluppo nativo", "{} selected photo · native development", 1)
                        } else {
                            localized_format!(lang, "{} fotografie selezionate · sviluppo nativo", "{} selected photos · native development", selected.len())
                        };
                        ui.label(RichText::new(selection).strong());
                        egui::ScrollArea::vertical().id_salt("export-targets").max_height(72.).show(ui,|ui| {
                            for item in &selected { ui.add(egui::Label::new(RichText::new(&item.name).small().color(MUTED)).truncate()).on_hover_text(&item.name); }
                        });
                    });
                    ui.add_space(6.);
                    let label_width = (ui.available_width() * 0.34).clamp(40., 220.);
                    ui.horizontal(|ui| {
                        export_label(ui, lang.text("Formato"), label_width);
                        egui::ComboBox::from_id_salt("export-format").width(ui.available_width())
                            .selected_text(lang.text(state.options.format.label())).show_ui(ui, |ui| {
                            for format in Format::ALL { ui.selectable_value(&mut state.options.format, format, lang.text(format.label())); }
                        });
                    });
                    if matches!(state.options.format, Format::Jpeg | Format::Png8 | Format::Png16 | Format::Tiff16) {
                        profile_controls(ui, &mut state.options.color_space);
                    }
                    match state.options.format {
                        Format::Jpeg => {
                            super::preferences::preference_slider(ui, lang.text("Qualità JPEG"), &mut state.options.jpeg_quality, 1..=100, false, false);
                            ui.label(RichText::new(lang.text("RGB 8 bit nel profilo scelto, compressione con perdita. Trasparenza composta su bianco.")).small().color(MUTED));
                        }
                        Format::Png8 | Format::Png16 => {
                            ui.label(lang.text("Profilo RGB scelto, alpha conservata. La compressione non cambia la qualità."));
                            ui.horizontal(|ui| { for (value, label) in [(Compression::Fast,"Veloce"),(Compression::Balanced,"Bilanciata"),(Compression::Best,"Compatta")] {
                                ui.selectable_value(&mut state.options.compression, value, lang.text(label));
                            }});
                        }
                        Format::DngLinear16 => { ui.label(lang.text("RGB Rec.2020 lineare già sviluppato, 16 bit interi. Non conserva negativi o valori oltre 1. Trasparenza non supportata.")); ui.colored_label(AMBER, lang.text("DNG lineare: sviluppo tecnico senza regolazioni fotografiche; usare JPEG, PNG o TIFF per la resa modificata.")); ui.colored_label(AMBER, lang.text("Riapertura: scegliere LibRaw bilineare/AHD. Apple RAW e motore mosaico TrueRenderer non supportano questo DNG lineare.")); }
                        Format::Tiff16 => {ui.label(lang.text("Profilo RGB ICC scelto, 16 bit interi, alpha conservata; non compresso, clamp [0,1]."));}
                        Format::TiffFloat32 => {ui.label(lang.text("Rec.2020 lineare ICC, RGBA float32 con alpha associata. Conserva negativi e valori oltre 1 del render; non compresso. Non è un RAW sensore né un export FITS."));}
                        Format::DngRaw => {
                            state.options.long_edge = 0;
                            ui.colored_label(AMBER, lang.text("Mosaico sensore senza WB o demosaic: DNG, NEF D750/D800 e RAF X-T5 supportati con Experimental, Nikon D750/D40 con i motori storici."));
                            ui.label(lang.text("DNG RAW conserva il mosaico: le regolazioni fotografiche non vengono applicate."));
                            ui.label(lang.text("Experimental conserva calibrazione, margini e crop; NEF e RAF ricevono un profilo D65 di compatibilità. La compressione lossy della fotocamera non è reversibile. EXIF, MakerNotes e RAW compresso sono esclusi: conservare l'originale."));
                            ui.colored_label(AMBER, lang.text("Per riaprire usare trueRendererExperimental 32fp o LibRaw bilineare/AHD, entro i rispettivi formati supportati."));
                        }
                    }
                    if state.options.format != Format::DngRaw {
                        ui.horizontal(|ui| {
                            export_label(ui, lang.text("Lato lungo (0 = originale)"), label_width);
                            ui.add_sized([96., 28.], egui::DragValue::new(&mut state.options.long_edge).range(0..=16384).suffix(" px"));
                        });
                        ui.label(RichText::new(lang.text("Motore salvato per foto · clamp nei formati interi, nessun dither")).small().color(MUTED));
                    }
                    if state.options.format != Format::DngRaw {
                        let mut custom=state.options.resolution.is_some();
                        if ui.checkbox(&mut custom,"Scegli DPI/PPI per la copia esportata").changed() {state.options.resolution=custom.then_some(tr_core::protocol::Resolution{x:300.,y:300.});}
                        if let Some(r)=&mut state.options.resolution {
                            ui.horizontal(|ui| {ui.label("PPI X / Y");ui.add(egui::DragValue::new(&mut r.x).range(1. ..=9600.));ui.add(egui::DragValue::new(&mut r.y).range(1. ..=9600.));});
                        } else {ui.small("DPI/PPI della sorgente, quando dichiarati. La foto e la ricetta restano invariate.");}
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button(lang.text("Cartella destinazione…")).clicked() && let Some(folder) = rfd::FileDialog::new().pick_folder() { state.destination = Some(folder); }
                        if let Some(folder) = &state.destination {
                            ui.add(egui::Label::new(RichText::new(folder.display().to_string()).small()).truncate()).on_hover_text(folder.display().to_string());
                        }
                    });
                    if !ready {ui.label(lang.text("Attendere il caricamento o salvataggio delle ricette selezionate."));}
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        start = ui.add_enabled(ready && !selected.is_empty() && selected.len() <= 1000 && state.destination.is_some(),
                            egui::Button::new(lang.text("Esporta selezione")).fill(style::SELECTED)).clicked();
                    });
                });
        if state.total > 0 {
            ui.separator();
            ui.add(
                egui::ProgressBar::new(state.completed as f32 / state.total as f32).text(
                    localized_format!(
                        lang,
                        "{} / {} completati · {} salvati",
                        "{} / {} completed · {} saved",
                        state.completed,
                        state.total,
                        state.succeeded
                    ),
                ),
            );
            if state.active {
                ui.label(&state.current);
                if ui.button(lang.text("Annulla export")).clicked() {
                    state.cancel.store(true, Ordering::Release);
                    state.remaining.clear();
                }
            } else if state.cancel.load(Ordering::Acquire) {
                ui.label(lang.text("Annullato. I file già salvati restano nella destinazione."));
            }
            egui::ScrollArea::vertical()
                .max_height(180.)
                .show(ui, |ui| {
                    for message in &state.messages {
                        ui.label(message);
                        ui.separator();
                    }
                });
        }
        if start {
            state.remaining = frozen;
            state.total = state.remaining.len();
            state.completed = 0;
            state.succeeded = 0;
            state.messages.clear();
            state.cancel = Arc::new(AtomicBool::new(false));
            // Reduce avoidable resident pressure before full-resolution export.
            self.trim_images(true);
            self.export_next();
        }
    }
}

fn export_label(ui: &mut egui::Ui, label: &str, width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 28.),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.add(egui::Label::new(label).truncate())
                .on_hover_text(label);
        },
    );
}

pub(in crate::ui) fn profile_controls(
    ui: &mut egui::Ui,
    space: &mut tr_core::export_spaces::ColorSpace,
) -> bool {
    let before = *space;
    ui.label("Profilo colore di destinazione");
    egui::ComboBox::from_id_salt("destination-profile").width(ui.available_width()).selected_text(space.label()).show_ui(ui, |ui| {
        for value in tr_core::export_spaces::ColorSpace::ALL {
            ui.selectable_value(space, value, value.label());
        }
        ui.add_enabled(false, egui::Button::new("CMYK · non disponibile"))
            .on_disabled_hover_text("Conversione CMYK non ancora disponibile. Serve un profilo ICC specifico per carta e processo di stampa.");
    });
    ui.small("Si applica alla copia esportata. L’anteprima e il profilo del monitor non cambiano.")
        .on_hover_text("I calcoli interni restano in Rec.2020 lineare a 32 bit; i colori vengono convertiti nel profilo scelto durante l’export.");
    before != *space
}
