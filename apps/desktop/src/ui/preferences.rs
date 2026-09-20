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
        let body_height = (ctx.content_rect().height() - 265.).clamp(48., 410.);
        egui::Window::new(lang.text("Impostazioni"))
            .id(egui::Id::new("settings-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(700.)
            .max_width((ctx.content_rect().width() - 64.).max(280.))
            .default_pos(egui::pos2(72., 64.))
            .show(ctx, |ui| {
                ui.label(RichText::new(lang.text("Il tuo spazio di lavoro, le tue preferenze.")).color(MUTED));
                ui.add_space(8.);
                ui.horizontal_wrapped(|ui| {
                    for (page, title) in SettingsPage::ALL {
                        ui.selectable_value(&mut self.settings_page, page, lang.text(title));
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
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
                                if ui.add_enabled(self.cache_action.is_none() && !self.scanning,
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
        ui.label(
            RichText::new(lang.text("La lingua viene applicata e salvata subito."))
                .small()
                .color(MUTED),
        );
        ui.add_space(24.);
        section(ui, lang.text("Vista · questa sessione"));
        ui.checkbox(&mut self.show_inspector, lang.text("Mostra ispettore"));
        ui.checkbox(
            &mut self.show_filmstrip,
            lang.text("Mostra miniature nel viewer"),
        );
        ui.add_space(24.);
        section(ui, lang.text("Dati locali"));
        ui.label(
            lang.text(
                "Gli originali rimangono intatti. Le annotazioni sono nella libreria locale.",
            ),
        );
    }

    fn preview_preferences(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Resa delle anteprime"));
        ui.horizontal_wrapped(|ui| {
            ui.label(lang.text("Qualità globale delle anteprime"));
            ui.selectable_value(
                &mut self.cache_settings.quality,
                PreviewQuality::Standard,
                "Standard",
            );
            ui.selectable_value(
                &mut self.cache_settings.quality,
                PreviewQuality::Full,
                lang.text("Piena"),
            );
        });
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
        ui.label(localized_format!(
            lang,
            "Attivo: {}",
            "Active: {}",
            lang.text(self.service.cache.settings().raw_engine.label())
        ));
        ui.label(lang.text("Applica e salva aggiorna le immagini. Ogni motore conserva le proprie anteprime in cache."));
        if self.cache_settings.raw_engine == tr_core::decoder::RawEngine::TrueRenderer {
            ui.label(lang.text("Sperimentale: Nikon D750 e D40 Bayer. Colore e superiorità rispetto agli altri motori ancora da qualificare. I RAW non supportati mostrano un errore."));
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
        ui.label(lang.text(
            "La scelta si applica dopo Applica e salva. Il popup può continuare in background.",
        ));
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
                    self.rebuild.is_empty() && !self.scanning,
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
        ui.label(lang.text("Questa scelta riguarda il ricampionamento del viewer. Il motore RAW si sceglie in Anteprime e RAW."));
        ui.checkbox(
            &mut self.cache_settings.adapt_on_battery,
            lang.text("Riduci automaticamente il lavoro a batteria"),
        );
        ui.label(localized_format!(
            lang,
            "Thread applicativi effettivi: {} · {}",
            "Effective application threads: {} · {}",
            self.service.cache.effective_threads(),
            if self.service.cache.on_battery() {
                lang.text("batteria")
            } else {
                lang.text("alimentazione esterna / non rilevata")
            }
        ));
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
        ui.add(
            egui::Slider::new(
                &mut self.cache_settings.cpu_threads,
                0..=std::thread::available_parallelism().map_or(1, usize::from),
            )
            .text(lang.text("Thread CPU (0 = automatici)")),
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
            ui.add(
                egui::Slider::new(
                    &mut self.cache_settings.memory_mib,
                    512..=(physical * 3 / 4).max(512),
                )
                .text(lang.text("Memoria richiesta (MiB)")),
            );
        }
        ui.label(localized_format!(
            lang,
            "Budget di ammissione effettivo: {} MiB",
            "Effective admission budget: {} MiB",
            self.cache_settings.effective_memory_mib(physical)
        ));
        let mut automatic = self.cache_settings.reusable_mib.is_none();
        if ui
            .checkbox(&mut automatic, lang.text("Cache RAM automatica"))
            .changed()
        {
            self.cache_settings.reusable_mib = if automatic { None } else { Some(0) };
        }
        if let Some(value) = &mut self.cache_settings.reusable_mib {
            ui.add(
                egui::Slider::new(
                    value,
                    0..=self.service.cache.memory.usage().limit / (1024 * 1024),
                )
                .text(lang.text("Cache RAM riutilizzabile (MiB; 0 = solo viste)")),
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
                    "Stima di base app/worker/device: {} MiB",
                    "App/worker/device baseline estimate: {} MiB",
                    self.service.cache.baseline_bytes / (1024 * 1024)
                ));
                ui.label(lang.text(
                    "Il budget include stime dei decoder; non è un limite RSS imposto dal sistema.",
                ));
            });
        ui.add(
            egui::Slider::new(&mut self.cache_settings.gpu_mib, 0..=1024)
                .text(lang.text("Cache GPU (MiB; 0 = automatica)")),
        );
    }

    fn cache_preferences(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        section(ui, lang.text("Archiviazione delle anteprime"));
        ui.label(RichText::new(lang.text("Le impostazioni valgono per tutte le cartelle; la quota disco si applica a ciascuna cartella separatamente.")).small().color(MUTED));
        ui.checkbox(
            &mut self.cache_settings.enabled,
            lang.text("Abilita cache su disco nella cartella delle immagini"),
        );
        ui.add(
            egui::Slider::new(&mut self.cache_settings.disk_mib, 64..=65536)
                .logarithmic(true)
                .text(lang.text("Quota per cartella (MiB)")),
        );
        ui.add(
            egui::Slider::new(&mut self.cache_settings.temporary_mib, 16..=2048)
                .logarithmic(true)
                .text(lang.text("Temporanei (MiB)")),
        );
        ui.add(
            egui::Slider::new(&mut self.cache_settings.unused_days, 1..=3650)
                .logarithmic(true)
                .text(lang.text("Scadenza senza utilizzo (giorni)")),
        );
        ui.add(
            egui::Slider::new(&mut self.cache_settings.free_mib, 0..=65536)
                .logarithmic(true)
                .text(lang.text("Spazio libero da riservare (MiB)")),
        );
        ui.label(lang.text("I temporanei rientrano nella quota disco. Le immagini troppo grandi per la cache restano visualizzabili in RAM. I file meno usati vengono rimossi per rispettare la quota."));
        ui.separator();
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
        ui.label(lang.text("entries contiene i render fp32 senza perdita; tmp contiene le scritture in corso. I temporanei abbandonati vengono rimossi alla successiva apertura. Originali, annotazioni e backup sono separati."));
    }
}
