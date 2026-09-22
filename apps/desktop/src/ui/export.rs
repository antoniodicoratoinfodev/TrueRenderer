use super::*;
use std::sync::atomic::AtomicBool;
use tr_core::export::{Compression, Format, Options};

#[derive(Default)]
pub(super) struct ExportUi {
    pub open: bool,
    options: Options,
    destination: Option<PathBuf>,
    remaining: VecDeque<Item>,
    total: usize,
    completed: usize,
    succeeded: usize,
    active: bool,
    current: String,
    cancel: Arc<AtomicBool>,
    engine: tr_core::decoder::RawEngine,
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
        let Some(item) = state.remaining.pop_front() else {
            return;
        };
        state.current = item.name.clone();
        state.active = true;
        let job = crate::photo_export::Job {
            item,
            options: state.options,
            engine: state.engine,
            destination: state.destination.clone().unwrap(),
            cancel: state.cancel.clone(),
        };
        if !self.request(Request::ExportPhoto(job)) {
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
        let selected: Vec<_> = self
            .state
            .items
            .iter()
            .filter(|item| self.state.selected.contains(&item.id))
            .cloned()
            .collect();
        let state = &mut self.photo_export;
        let mut open = true;
        let mut start = false;
        egui::Window::new(lang.text("Esporta fotografie")).id(egui::Id::new("photo-export")).open(&mut open)
            .default_width(540.).resizable(true).vscroll(true)
            .max_height((ctx.content_rect().height() - 70.).max(180.)).show(ctx, |ui| {
                ui.label(lang.text("Nuovi file: nessun originale o export esistente viene sostituito."));
                ui.add_enabled_ui(!state.active && state.remaining.is_empty(), |ui| {
                    ui.label(localized_format!(lang, "{} fotografie selezionate · sviluppo nativo, non miniature", "{} selected photos · native development, not thumbnails", selected.len()));
                    egui::ComboBox::from_id_salt("export-format").selected_text(lang.text(state.options.format.label())).show_ui(ui, |ui| {
                        for format in Format::ALL { ui.selectable_value(&mut state.options.format, format, lang.text(format.label())); }
                    });
                    match state.options.format {
                        Format::Jpeg => {
                            ui.add(egui::Slider::new(&mut state.options.jpeg_quality, 1..=100).text(lang.text("Qualità JPEG")));
                            ui.label(lang.text("sRGB 8 bit, compressione con perdita. Trasparenza composta su bianco."));
                        }
                        Format::Png8 | Format::Png16 => {
                            ui.label(lang.text("sRGB, alpha conservata. La compressione non cambia la qualità."));
                            ui.horizontal(|ui| { for (value, label) in [(Compression::Fast,"Veloce"),(Compression::Balanced,"Bilanciata"),(Compression::Best,"Compatta")] {
                                ui.selectable_value(&mut state.options.compression, value, lang.text(label));
                            }});
                        }
                        Format::DngLinear16 => { ui.label(lang.text("RGB Rec.2020 lineare già sviluppato, 16 bit interi. Non conserva negativi o valori oltre 1. Trasparenza non supportata.")); ui.colored_label(AMBER, lang.text("Riapertura: scegliere LibRaw bilineare/AHD. Apple RAW e motore mosaico TrueRenderer non supportano questo DNG lineare.")); }
                        Format::Tiff16 => {ui.label(lang.text("sRGB ICC, 16 bit interi, alpha conservata; non compresso, clamp [0,1]."));}
                        Format::TiffFloat32 => {ui.label(lang.text("Rec.2020 lineare ICC, RGBA float32 con alpha associata. Conserva negativi e valori oltre 1 del render; non compresso. Non è un RAW sensore né un export FITS."));}
                        Format::DngRaw => {
                            state.options.long_edge = 0;
                            ui.colored_label(AMBER, lang.text("Mosaico area attiva: Nikon D750/D40. Nessun demosaic, WB applicato o ridimensionamento."));
                            ui.label(lang.text("Conserva campioni, CFA, nero/bianco, WB e orientamento; matrice D65 LibRaw. Non include margini ottici, MakerNotes, EXIF/GPS o NEF compresso: conservare l'originale. Altre camere vengono rifiutate."));
                            ui.colored_label(AMBER, lang.text("Per riaprire i DNG esportati scegliere LibRaw bilineare/AHD. Apple RAW e il motore mosaico TrueRenderer non sono compatibili con questi file."));
                        }
                    }
                    if state.options.format != Format::DngRaw {
                        ui.horizontal(|ui| {
                            ui.label(lang.text("Lato lungo (0 = originale)"));
                            ui.add(egui::DragValue::new(&mut state.options.long_edge).range(0..=16384));
                        });
                        ui.label(localized_format!(lang, "Motore: {:?} · clamp nei formati interi, nessun dither", "Engine: {:?} · integer formats clamp, no dither", self.cache_settings.raw_engine));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(lang.text("Cartella destinazione…")).clicked() && let Some(folder) = rfd::FileDialog::new().pick_folder() { state.destination = Some(folder); }
                        if let Some(folder) = &state.destination { ui.label(folder.display().to_string()); }
                    });
                    start = ui.add_enabled(!selected.is_empty() && selected.len() <= 1000 && state.destination.is_some(), egui::Button::new(lang.text("Esporta selezione"))).clicked();
                });
                if state.total > 0 {
                    ui.separator();
                    ui.add(egui::ProgressBar::new(state.completed as f32 / state.total as f32)
                        .text(localized_format!(lang, "{} / {} completati · {} salvati", "{} / {} completed · {} saved", state.completed, state.total, state.succeeded)));
                    if state.active {
                        ui.label(&state.current);
                        if ui.button(lang.text("Annulla export")).clicked() { state.cancel.store(true, Ordering::Release); state.remaining.clear(); }
                    } else if state.cancel.load(Ordering::Acquire) { ui.label(lang.text("Annullato. I file già salvati restano nella destinazione.")); }
                    egui::ScrollArea::vertical().max_height(180.).show(ui, |ui| { for message in &state.messages { ui.label(message); ui.separator(); } });
                }
            });
        state.open = open;
        if start {
            state.remaining = selected.into();
            state.total = state.remaining.len();
            state.completed = 0;
            state.succeeded = 0;
            state.messages.clear();
            state.cancel = Arc::new(AtomicBool::new(false));
            state.engine = self.cache_settings.raw_engine;
            // Reduce avoidable resident pressure before full-resolution export.
            self.trim_images(true);
            self.export_next();
        }
    }
}
