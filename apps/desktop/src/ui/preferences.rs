use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum SettingsPage {
    General,
    #[default]
    Previews,
    Performance,
    Cache,
}
impl SettingsPage {
    pub(super) const ALL: [(Self, &'static str); 4] = [
        (Self::General, "Generale"),
        (Self::Previews, "Anteprime e RAW"),
        (Self::Performance, "Prestazioni"),
        (Self::Cache, "Cache e dati"),
    ];
}

fn preference_note(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(RichText::new(text).small().color(MUTED))
}

// Labels and values stay on one row; the track below uses the available width.
// Binary storage units are unchanged when a quota is displayed and edited in MB.
fn preference_slider<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    logarithmic: bool,
    decimal_mb: bool,
) -> egui::Response {
    ui.push_id(label, |ui| {
        ui.spacing_mut().item_spacing.y = 3.;
        ui.spacing_mut().interact_size.y = 22.;
        ui.spacing_mut().button_padding.y = 3.;
        let (label_id, number) = ui
            .horizontal(|ui| {
                let label_width =
                    (ui.available_width() - 96. - ui.spacing().item_spacing.x).max(0.);
                let label_id = ui
                    .allocate_ui_with_layout(
                        egui::vec2(label_width, 22.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(label_width);
                            ui.set_min_height(22.);
                            ui.add(egui::Label::new(label).wrap())
                        },
                    )
                    .inner
                    .id;
                let mut number = egui::DragValue::new(value).range(range.clone()).speed(1.);
                if decimal_mb {
                    number = number
                        .custom_formatter(|value, _| crate::size_units::format_mib_as_mb(value))
                        .custom_parser(crate::size_units::parse_mb_as_mib);
                }
                let number = ui.add_sized([96., 22.], number).labelled_by(label_id);
                (label_id, number)
            })
            .inner;
        ui.spacing_mut().slider_width = ui.available_width();
        let slider = ui
            .add(
                egui::Slider::new(value, range)
                    .show_value(false)
                    .logarithmic(logarithmic),
            )
            .labelled_by(label_id);
        number.union(slider)
    })
    .inner
}

impl TrueRenderer {
    // Reproducible native layout capture on a caller-supplied synthetic corpus/data root.
    pub(super) fn restyle_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.started.elapsed() > Duration::from_secs(90) {
            let _ = std::fs::write(
                self.root.join("reports/restyle-ui.json"),
                b"{\"passed\":false,\"reason\":\"timeout\"}",
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.scanning
            || !self.presenter.is_idle()
            || self.demand.iter().any(|key| !self.cache.contains_key(key))
        {
            return;
        }
        let index = self.smoke_stage / 10;
        let phase = self.smoke_stage % 10;
        if index == 12 {
            let report = serde_json::json!({"passed": !self.fatal && self.errors.is_empty() && !self.presenter.has_errors() && self.gpu_passed,
                "screenshots": self.screenshots, "scope": "Four preferences tabs in English/Italian, minimum window and 200% UI zoom on synthetic corpus. Not full accessibility/display qualification."});
            let _ = std::fs::write(
                self.root.join("reports/restyle-ui.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if phase == 0 {
            self.set_language(if index < 4 || index == 8 || index == 10 {
                Language::English
            } else {
                Language::Italian
            });
            self.settings_page =
                SettingsPage::ALL[if index < 8 { index as usize % 4 } else { 1 }].0;
            // Send the physical-window resize before increasing UI zoom. Repeating it
            // at 200% would request a larger native window on the second language.
            if index == 8 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1100., 720.)));
            }
            ctx.set_zoom_factor(if index >= 10 { 2. } else { 1. });
        }
        if phase < 5 {
            self.smoke_stage += 1;
            return;
        }
        let name = format!("restyle-{index:02}");
        if self.screenshots.contains(&name) {
            self.smoke_stage = (index + 1) * 10;
        } else {
            self.capture_screenshot(ctx, &name);
        }
    }

    pub(super) fn open_preferences(&mut self, page: SettingsPage) {
        if !self.show_settings {
            self.cache_settings = self.service.cache.settings();
        }
        self.settings_page = page;
        self.show_settings = true;
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        self.poll_cache_action(ctx);
        if !self.show_settings {
            return;
        }
        let lang = self.cache_settings.language;
        let mut open = self.show_settings;
        let mut close = false;
        let mut action = 0;
        let body_height = (ctx.content_rect().height() - 235.).clamp(48., 410.);
        egui::Window::new(lang.text("Impostazioni"))
            .id(egui::Id::new("settings-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(700.)
            .max_width((ctx.content_rect().width() - 64.).max(280.))
            .default_pos(egui::pos2(72., 64.))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (page, title) in SettingsPage::ALL {
                        ui.selectable_value(&mut self.settings_page, page, lang.text(title));
                    }
                });
                ui.separator();
                ui.style_mut().spacing.scroll.floating = false;
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .id_salt(("preferences-body", self.settings_page))
                    .max_height(body_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_height(body_height);
                        ui.add_space(8.);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        match self.settings_page {
                            SettingsPage::General => self.general_preferences(ui),
                            SettingsPage::Previews => self.preview_preferences(ui),
                            SettingsPage::Performance => self.performance_preferences(ui),
                            SettingsPage::Cache => {
                                self.cache_preferences(ui);
                                ui.add_space(16.);
                                section(ui, lang.text("Manutenzione"));
                                ui.label(RichText::new(lang.text("Svuota solo le anteprime ricostruibili della cartella corrente.")).small().color(MUTED));
                                if ui.add_enabled(self.cache_action.is_none(),
                                    egui::Button::new(lang.text("Svuota cache cartella"))).clicked() { action = 2; }
                                ui.separator();
                                self.library_actions(ui);
                            }
                        }
                    });
                ui.separator();
                // This footer is outside the scroll area on every page.
                let dirty = self.cache_settings != self.service.cache.settings();
                if self.cache_action.is_some() {
                    ui.label(RichText::new(lang.text("Aggiornamento cache…")).color(AMBER));
                } else if dirty {
                    ui.label(RichText::new(lang.text("Modifiche da applicare")).small().color(AMBER));
                } else {
                    ui.add(egui::Label::new(lang.message(&self.status)).truncate())
                        .on_hover_text(lang.message(&self.status));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(dirty && self.cache_action.is_none(),
                        egui::Button::new(lang.text("Ripristina modifiche")).frame(false)).clicked() {
                        self.cache_settings = self.service.cache.settings();
                    }
                    if ui.button(lang.text("Chiudi")).clicked() { close = true; }
                    if ui.add_enabled(self.cache_action.is_none() && !self.scanning,
                        egui::Button::new(lang.text("Applica e salva")).fill(Color32::from_gray(62))).clicked() { action = 1; }
                });
            });
        self.show_settings = open && !close;
        if action > 0 {
            self.start_cache_action(action == 1);
        }
    }

    fn general_preferences(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Lingua"));
        let mut language = lang;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut language, Language::English, "English");
            ui.selectable_value(&mut language, Language::Italian, "Italiano");
        });
        if language != lang {
            self.set_language(language);
        }
        preference_note(ui, lang.text("La lingua viene applicata e salvata subito."));
        ui.add_space(24.);
        section(ui, lang.text("Vista · questa sessione"));
        ui.label(lang.text("Dimensione interfaccia"));
        ui.horizontal_wrapped(|ui| {
            let mut zoom = (ui.ctx().zoom_factor() * 100.).round() as u16;
            for percent in [100, 150, 200] {
                if ui
                    .selectable_value(&mut zoom, percent, format!("{percent}%"))
                    .changed()
                {
                    ui.ctx().set_zoom_factor(f32::from(zoom) / 100.);
                }
            }
        });
        ui.checkbox(&mut self.show_inspector, lang.text("Mostra ispettore"));
        ui.checkbox(
            &mut self.show_filmstrip,
            lang.text("Mostra miniature nel viewer"),
        );
        ui.add_space(24.);
        section(ui, lang.text("Dati locali"));
        preference_note(
            ui,
            lang.text(
                "Gli originali rimangono intatti. Le annotazioni sono nella libreria locale.",
            ),
        );
    }

    fn preview_preferences(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Resa delle anteprime"));
        self.global_quality_control(ui);
        preference_note(
            ui,
            lang.text("La qualità viene applicata e salvata subito."),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(lang.text("Motore RAW"));
            egui::ComboBox::from_id_salt("raw-engine")
                .selected_text(lang.text(self.cache_settings.raw_engine.label()))
                .show_ui(ui, |ui| {
                    for engine in tr_core::decoder::RawEngine::choices() {
                        ui.selectable_value(
                            &mut self.cache_settings.raw_engine,
                            engine,
                            lang.text(engine.label()),
                        );
                    }
                });
        });
        preference_note(
            ui,
            localized_format!(
                lang,
                "Attivo: {}",
                "Active: {}",
                lang.text(self.service.cache.settings().raw_engine.label())
            ),
        );
        preference_note(ui, lang.text("Applica e salva aggiorna le immagini. Ogni motore conserva le proprie anteprime in cache."));
        if self.cache_settings.raw_engine == tr_core::decoder::RawEngine::TrueRenderer {
            ui.label(RichText::new(lang.text("Sperimentale: Nikon D750 e D40 Bayer. Colore e superiorità rispetto agli altri motori ancora da qualificare. I RAW non supportati mostrano un errore.")).small().color(AMBER));
        }

        ui.add_space(24.);
        section(ui, lang.text("Preparazione in background"));
        ui.label(lang.text("All'apertura della cartella"));
        ui.selectable_value(
            &mut self.cache_settings.folder_loading,
            crate::cache::FolderLoading::Background,
            lang.text("Carica le anteprime in background"),
        );
        ui.selectable_value(
            &mut self.cache_settings.folder_loading,
            crate::cache::FolderLoading::Foreground,
            lang.text("Prepara prima la cartella con popup"),
        );
        preference_note(
            ui,
            lang.text(
                "La scelta si applica dopo Applica e salva. Il popup può continuare in background.",
            ),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(lang.text("Precaricamento"));
            ui.selectable_value(
                &mut self.cache_settings.prefetch,
                crate::cache::Prefetch::Disabled,
                lang.text("Disattivato"),
            );
            ui.selectable_value(
                &mut self.cache_settings.prefetch,
                crate::cache::Prefetch::Automatic,
                lang.text("Automatico"),
            );
            ui.selectable_value(
                &mut self.cache_settings.prefetch,
                crate::cache::Prefetch::Extended,
                lang.text("Esteso"),
            );
        });
        ui.checkbox(
            &mut self.preparation_paused,
            lang.text("Pausa preparazione in background"),
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.rebuild.is_empty() && !self.scanning && !self.clearing_current_folder(),
                    egui::Button::new(lang.text("Ricostruisci anteprime della cartella")),
                )
                .clicked()
            {
                self.start_folder_preparation(true);
            }
            if !self.rebuild.is_empty() && ui.button(lang.text("Annulla preparazione")).clicked() {
                self.cancel_folder_preparation();
            }
        });
        if self.rebuild_total > 0 {
            let (done, total, _) = self.folder_progress();
            ui.label(localized_format!(
                lang,
                "Anteprime elaborate: {} / {}",
                "Previews processed: {} / {}",
                done,
                total
            ));
        }
    }

    fn performance_preferences(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Precisione di presentazione SDR"));
        egui::ComboBox::from_id_salt("presentation-precision")
            .selected_text(lang.text(self.cache_settings.presentation.label()))
            .show_ui(ui, |ui| {
                for precision in [
                    tr_core::presentation::Precision::Compatible8,
                    tr_core::presentation::Precision::Sdr10,
                    tr_core::presentation::Precision::Sdr16Float,
                ] {
                    ui.selectable_value(
                        &mut self.cache_settings.presentation,
                        precision,
                        lang.text(precision.label()),
                    );
                }
            });
        ui.label(RichText::new(lang.text("Applicare e riavviare l'app. La coppia formato + sRGB deve essere supportata; altrimenti rimane SDR 8 bit. Non abilita HDR né certifica i bit del monitor.")).small());
        preference_note(ui, lang.text("16 float aumenta memoria di texture/superficie; precisione non uniforme, diversa da 16 bit interi. Working e cache restano fp32, anche con calcolo CPU."));
        egui::CollapsingHeader::new(lang.text("Superficie effettiva e capacità")).show(ui, |ui| {
            ui.label(&self.surface);
        });
        ui.add_space(20.);
        section(ui, lang.text("Elaborazione"));
        ui.horizontal_wrapped(|ui| {
            ui.label(lang.text("Profilo prestazioni"));
            ui.selectable_value(
                &mut self.cache_settings.profile,
                crate::cache::PerformanceProfile::Performance,
                lang.text("Prestazioni"),
            );
            ui.selectable_value(
                &mut self.cache_settings.profile,
                crate::cache::PerformanceProfile::Balanced,
                lang.text("Bilanciato"),
            );
            ui.selectable_value(
                &mut self.cache_settings.profile,
                crate::cache::PerformanceProfile::Saver,
                lang.text("Risparmio"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(lang.text("Calcolo immagine"));
            ui.selectable_value(
                &mut self.cache_settings.compute,
                ImageCompute::Automatic,
                lang.text("Automatico"),
            );
            ui.selectable_value(
                &mut self.cache_settings.compute,
                ImageCompute::Gpu,
                lang.text("GPU compatibile"),
            );
            ui.selectable_value(&mut self.cache_settings.compute, ImageCompute::Cpu, "CPU");
        });
        egui::CollapsingHeader::new(lang.text("Diagnostica GPU"))
            .id_salt("gpu-details")
            .show(ui, |ui| {
                ui.label(lang.message(&self.gpu_status));
                let compute = self.presenter.statistics();
                ui.label(localized_format!(
                    lang,
                    "Frame elaborati: {} GPU · {} CPU · {} ripieghi",
                    "Processed frames: {} GPU · {} CPU · {} fallbacks",
                    compute.gpu_frames,
                    compute.cpu_frames,
                    compute.fallbacks
                ));
                if !compute.last_fallback.is_empty() {
                    ui.label(localized_format!(
                        lang,
                        "Ultimo ripiego CPU: {}",
                        "Last CPU fallback: {}",
                        compute.last_fallback
                    ));
                }
            });
        preference_note(ui, lang.text("Questa scelta riguarda il ricampionamento del viewer. Il motore RAW si sceglie in Anteprime e RAW."));
        ui.checkbox(
            &mut self.cache_settings.adapt_on_battery,
            lang.text("Riduci automaticamente il lavoro a batteria"),
        );
        preference_note(
            ui,
            localized_format!(
                lang,
                "Thread applicativi effettivi: {} · {}",
                "Effective application threads: {} · {}",
                self.service.cache.effective_threads(),
                if self.service.cache.on_battery() {
                    lang.text("batteria")
                } else {
                    lang.text("alimentazione esterna / non rilevata")
                }
            ),
        );
        ui.label(localized_format!(
            lang,
            "Pressione memoria OS: {}",
            "OS memory pressure: {}",
            match self.service.cache.stats().memory_pressure {
                Some(tr_platform::MemoryPressure::Normal) => lang.text("normale"),
                Some(tr_platform::MemoryPressure::Warning) =>
                    lang.text("elevata · lavoro anticipato sospeso"),
                Some(tr_platform::MemoryPressure::Critical) =>
                    lang.text("critica · cache riutilizzabili in rilascio"),
                None => lang.text("adattatore non disponibile"),
            }
        ));
        preference_slider(
            ui,
            lang.text("Thread CPU (0 = automatici)"),
            &mut self.cache_settings.cpu_threads,
            0..=std::thread::available_parallelism().map_or(1, usize::from),
            false,
            false,
        );

        ui.add_space(24.);
        section(ui, lang.text("Memoria e GPU"));
        let physical = self.service.cache.physical_mib;
        let mut automatic = self.cache_settings.memory_mib == 0;
        if ui
            .checkbox(&mut automatic, lang.text("Memoria automatica"))
            .changed()
        {
            self.cache_settings.memory_mib = if automatic {
                0
            } else {
                self.cache_settings.effective_memory_mib(physical).max(512)
            };
        }
        if !automatic {
            preference_slider(
                ui,
                lang.text("Memoria richiesta (MB)"),
                &mut self.cache_settings.memory_mib,
                512..=(physical * 3 / 4).max(512),
                false,
                true,
            );
        }
        preference_note(
            ui,
            localized_format!(
                lang,
                "Budget di ammissione effettivo: {}",
                "Effective admission budget: {}",
                human_bytes(self.cache_settings.effective_memory_mib(physical) * 1024 * 1024)
            ),
        );
        let mut automatic = self.cache_settings.reusable_mib.is_none();
        if ui
            .checkbox(&mut automatic, lang.text("Cache RAM automatica"))
            .changed()
        {
            self.cache_settings.reusable_mib = if automatic { None } else { Some(0) };
        }
        if let Some(value) = &mut self.cache_settings.reusable_mib {
            preference_slider(
                ui,
                lang.text("Cache RAM riutilizzabile (MB; 0 = solo viste)"),
                value,
                0..=self.service.cache.memory.usage().limit / (1024 * 1024),
                false,
                true,
            );
        }
        egui::CollapsingHeader::new(lang.text("Utilizzo memoria"))
            .id_salt("memory-details")
            .show(ui, |ui| {
                let memory = self.service.cache.memory.usage();
                ui.label(localized_format!(
                    lang,
                    "Memoria prenotata (base inclusa): {} · picco crediti: {}",
                    "Reserved memory (including baseline): {} · peak credits: {}",
                    human_bytes(memory.reserved),
                    human_bytes(memory.peak)
                ));
                if memory.reserved > memory.limit {
                    ui.label(lang.text("Riduzione memoria in corso"));
                }
                ui.label(localized_format!(
                    lang,
                    "Stima di base app/worker/device: {}",
                    "App/worker/device baseline estimate: {}",
                    human_bytes(self.service.cache.baseline_bytes)
                ));
                preference_note(ui, lang.text(
                    "Il budget include stime dei decoder; non è un limite RSS imposto dal sistema.",
                ));
            });
        preference_slider(
            ui,
            lang.text("Cache GPU (MB; 0 = automatica)"),
            &mut self.cache_settings.gpu_mib,
            0..=1024,
            false,
            true,
        );
    }

    fn cache_preferences(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Archiviazione delle anteprime"));
        preference_note(ui, lang.text("Un solo limite di spazio: scegli se applicarlo per cartella o al totale delle cartelle conosciute."));
        ui.checkbox(
            &mut self.cache_settings.enabled,
            lang.text("Abilita cache su disco nella cartella delle immagini"),
        );
        ui.add_enabled_ui(self.cache_settings.enabled, |ui| {
        ui.checkbox(&mut self.cache_settings.global_disk_quota, lang.text("Limite totale tra cartelle conosciute")).on_hover_text(lang.text("Disattivato: il limite vale per ogni cartella. Attivato: lo stesso limite vale per la somma, con rimozione delle anteprime meno recenti."));
        preference_slider(
            ui,
            lang.text("Limite spazio cache (MB)"),
            &mut self.cache_settings.disk_mib,
            64..=65536,
            true,
            true,
        );
        preference_slider(
            ui,
            lang.text("Temporanei (MB)"),
            &mut self.cache_settings.temporary_mib,
            16..=2048,
            true,
            true,
        );
        ui.checkbox(&mut self.cache_settings.expire_unused, lang.text("Elimina anteprime non utilizzate"));
        ui.add_enabled_ui(self.cache_settings.expire_unused, |ui| {
            preference_slider(
                ui,
                lang.text("Scadenza senza utilizzo (giorni)"),
                &mut self.cache_settings.unused_days,
                1..=3650,
                true,
                false,
            );
        });
        preference_slider(
            ui,
            lang.text("Spazio libero da riservare (MB)"),
            &mut self.cache_settings.free_mib,
            0..=65536,
            true,
            true,
        );
        ui.checkbox(&mut self.cache_settings.clean_known_folders, lang.text("Pulisci periodicamente anche le cartelle non aperte")).on_hover_text(lang.text("Ogni minuto mentre l'app è aperta. Usa gli stessi limiti e la stessa scadenza; considera solo le cartelle registrate da questa libreria, senza cercare nei dischi. Le cartelle non disponibili vengono segnalate e riprovate."));
        });
        if let Some(summary) = self.service.cache.known_cache_summary() {
            if summary.measured {
                ui.label(localized_format!(
                    lang,
                    "Ultima pulizia: {} · {} cartelle · {} controlli non completati",
                    "Last cleanup: {} · {} folders · {} checks incomplete",
                    human_bytes(summary.bytes),
                    summary.folders,
                    summary.unavailable
                ));
            } else {
                ui.label(lang.text("Spazio totale non ancora misurato"));
            }
        } else {
            ui.label(lang.text("Aggiornamento cache…"));
        }
        let saved = self.cache_settings == self.service.cache.settings();
        if ui
            .add_enabled(
                saved && self.cache_action.is_none(),
                egui::Button::new(lang.text("Pulisci ora le cache conosciute")),
            )
            .clicked()
        {
            let cache = self.service.cache.clone();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            self.cache_action = Some(cache_actions::CacheAction::new(rx));
            std::thread::spawn(move || {
                let result = cache
                    .maintain_known(&|| false)
                    .map_err(|error| format!("Cache: {error:#}"));
                let _ = tx.send(result);
            });
        }
        egui::CollapsingHeader::new(lang.text("Dettagli cache"))
            .id_salt("cache-details")
            .show(ui, |ui| {
        preference_note(ui, lang.text("I temporanei rientrano nella quota disco. Le immagini troppo grandi per la cache restano visualizzabili in RAM. I file meno usati vengono rimossi per rispettare la quota."));
        ui.label(lang.text("Cartella corrente:"));
        ui.add(egui::Label::new(self.folder.join(crate::cache::NAME).display().to_string()).wrap());
        let stats = self.service.cache.stats();
        if stats.folder == self.folder.display().to_string() {
            ui.label(localized_format!(
                lang,
                "Cache: {} in {} file · temporanei inattivi: {}",
                "Cache: {} in {} files · inactive temporary files: {}",
                human_bytes(stats.bytes),
                stats.entries,
                human_bytes(stats.temporary_bytes)
            ));
        }
        ui.label(localized_format!(
            lang,
            "Sessione: {} riusi da disco · {} mancate corrispondenze · {} scritture",
            "Session: {} disk hits · {} misses · {} writes",
            stats.hits,
            stats.misses,
            stats.writes
        ));
        ui.add(egui::Label::new(lang.message(&stats.message)).wrap());
        preference_note(ui, lang.text("entries contiene i render fp32 senza perdita; tmp contiene le scritture in corso. I temporanei abbandonati vengono rimossi alla successiva apertura. Originali, annotazioni e backup sono separati."));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_quota_keeps_decimal_input_and_disabled_controls() {
        for lang in [Language::Italian, Language::English] {
            for width in [200., 328., 628.] {
                for enabled in [false, true] {
                    let ctx = egui::Context::default();
                    style::apply(&ctx);
                    let mut value = 2048_u64;
                    let mut frame = |events: Vec<egui::Event>| {
                        let mut rect = egui::Rect::NOTHING;
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 180.),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                ui.add_enabled_ui(enabled, |ui| {
                                    rect = preference_slider(
                                        ui,
                                        lang.text("Spazio libero da riservare (MB)"),
                                        &mut value,
                                        0..=65536,
                                        true,
                                        true,
                                    )
                                    .rect;
                                });
                            },
                        );
                        output.textures_delta.clear();
                        assert!(rect.left() >= 0. && rect.right() <= width);
                        (rect, value)
                    };
                    let (rect, initial) = frame(vec![]);
                    assert_eq!(initial, 2048, "Opening the control must keep its quota");
                    let pointer = |pos, pressed| egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    };
                    let start = egui::pos2(rect.center().x, rect.bottom() - 10.);
                    let end = egui::pos2(rect.right() - 20., start.y);
                    frame(vec![egui::Event::PointerMoved(start), pointer(start, true)]);
                    frame(vec![egui::Event::PointerMoved(end)]);
                    let (_, dragged) = frame(vec![pointer(end, false)]);
                    assert_eq!(dragged != initial, enabled, "Track enabled: {enabled}");
                    let number = egui::pos2(rect.right() - 24., rect.top() + 11.);
                    frame(vec![
                        egui::Event::PointerMoved(number),
                        pointer(number, true),
                    ]);
                    frame(vec![pointer(number, false)]);
                    let (_, typed) = frame(vec![
                        egui::Event::Key {
                            key: egui::Key::A,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers {
                                command: true,
                                ctrl: true,
                                ..Default::default()
                            },
                        },
                        egui::Event::Text("1000,00".into()),
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                    assert_eq!(
                        typed,
                        if enabled { 954 } else { 2048 },
                        "{lang:?} at {width}"
                    );
                }
            }
        }
    }
}
