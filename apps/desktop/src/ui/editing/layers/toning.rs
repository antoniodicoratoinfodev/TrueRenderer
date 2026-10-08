use super::{Controls, color_wheel, slider};
use crate::i18n::Language;
use eframe::egui;
use tr_core::editing::layers::{BlackAndWhite, ColorFilter, GradientMap, GradientStop, Grading};

impl Controls {
    pub(super) fn grading(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut Grading,
        commit: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            for (i, name) in ["Ombre", "Mezzitoni", "Luci", "Globale"]
                .into_iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.grade_zone, i, lang.text(name));
            }
        });
        let zone = &mut g.zones[self.grade_zone];
        let compact = ui.available_width() >= 300.;
        let layout = if compact {
            egui::Layout::left_to_right(egui::Align::Min)
        } else {
            egui::Layout::top_down(egui::Align::Min)
        };
        ui.with_layout(layout, |ui| {
            let side = if compact { 112. } else { 132. };
            ui.allocate_ui(egui::vec2(side, side), |ui| {
                let wheel = color_wheel(ui, lang, &mut zone.hue, &mut zone.amount);
                *commit |= wheel.drag_stopped() || (wheel.changed() && !wheel.dragged());
            });
            ui.vertical(|ui| {
                slider(
                    ui,
                    lang,
                    "Tonalità (°)",
                    &mut zone.hue,
                    0. ..=360.,
                    0.,
                    commit,
                );
                slider(ui, lang, "Cromia", &mut zone.amount, 0. ..=100., 0., commit);
                slider(
                    ui,
                    lang,
                    "Luminanza zona (EV)",
                    &mut zone.exposure,
                    -2. ..=2.,
                    0.,
                    commit,
                );
            });
        });
        if ui.button(lang.text("Ripristina zona")).clicked() {
            *zone = Default::default();
            *commit = true;
        }
        egui::CollapsingHeader::new(lang.text("Transizioni tonali")).show(ui, |ui| {
            slider(
                ui,
                lang,
                "Bilanciamento",
                &mut g.balance,
                -100. ..=100.,
                0.,
                commit,
            );
            ui.small(lang.text("Negativo: più ombre · positivo: più luci"));
            slider(
                ui,
                lang,
                "Sovrapposizione",
                &mut g.overlap,
                0. ..=100.,
                50.,
                commit,
            );
        });
    }
    pub(super) fn black_white(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut BlackAndWhite,
        commit: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui.button(lang.text("Converti in B&N")).clicked() {
                g.amount = 100.;
                *commit = true;
            }
            if ui.button(lang.text("Ripristina famiglie")).clicked() {
                g.bands = [0.; 8];
                *commit = true;
            }
        });
        slider(
            ui,
            lang,
            "Quantità B&N",
            &mut g.amount,
            0. ..=100.,
            0.,
            commit,
        );
        ui.small(lang.text("Luminosità delle famiglie · neutri protetti"));
        for (name, band) in [
            "Rosso",
            "Arancio",
            "Giallo",
            "Verde",
            "Acquamarina",
            "Blu",
            "Viola",
            "Magenta",
        ]
        .into_iter()
        .zip(&mut g.bands)
        {
            slider(ui, lang, name, band, -100. ..=100., 0., commit);
        }
    }
    pub(super) fn color_filter(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut ColorFilter,
        commit: &mut bool,
    ) {
        let compact = ui.available_width() >= 300.;
        let layout = if compact {
            egui::Layout::left_to_right(egui::Align::Min)
        } else {
            egui::Layout::top_down(egui::Align::Min)
        };
        ui.with_layout(layout, |ui| {
            let side = if compact { 112. } else { 132. };
            ui.allocate_ui(egui::vec2(side, side), |ui| {
                let wheel = color_wheel(ui, lang, &mut g.hue, &mut g.saturation);
                *commit |= wheel.drag_stopped() || (wheel.changed() && !wheel.dragged());
            });
            ui.vertical(|ui| {
                slider(
                    ui,
                    lang,
                    "Tonalità (°)",
                    &mut g.hue,
                    0. ..=360.,
                    40.,
                    commit,
                );
                slider(
                    ui,
                    lang,
                    "Saturazione filtro",
                    &mut g.saturation,
                    0. ..=100.,
                    100.,
                    commit,
                );
                slider(
                    ui,
                    lang,
                    "Densità filtro",
                    &mut g.density,
                    0. ..=100.,
                    0.,
                    commit,
                );
            });
        });
        *commit |= ui
            .checkbox(&mut g.preserve_luminance, lang.text("Conserva luminanza"))
            .changed();
    }
    pub(super) fn gradient_map(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut GradientMap,
        commit: &mut bool,
    ) {
        slider(
            ui,
            lang,
            "Quantità mappa gradiente",
            &mut g.amount,
            0. ..=100.,
            0.,
            commit,
        );
        *commit |= ui
            .checkbox(&mut g.reverse, lang.text("Inverti tavolozza"))
            .changed();
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.), egui::Sense::hover());
        for x in 0..80 {
            let t = (x as f32 + 0.5) / 80.;
            let c = g.color_at(if g.reverse { 1. - t } else { t });
            let color = tr_core::color::display_pixel([c[0], c[1], c[2], 1.], 0.);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left() + rect.width() * x as f32 / 80., rect.top()),
                    egui::pos2(
                        rect.left() + rect.width() * (x + 1) as f32 / 80.,
                        rect.bottom(),
                    ),
                ),
                0.,
                egui::Color32::from_rgb(color[0], color[1], color[2]),
            );
        }
        ui.horizontal_wrapped(|ui| {
            for i in 0..g.stops.len() {
                ui.selectable_value(
                    &mut self.gradient_stop,
                    i,
                    format!("{} {}", lang.text("Punto"), i + 1),
                );
            }
        });
        self.gradient_stop = self.gradient_stop.min(g.stops.len() - 1);
        let index = self.gradient_stop;
        let last = g.stops.len() - 1;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    g.stops.len() < GradientMap::MAX_STOPS,
                    egui::Button::new(lang.text("Aggiungi punto")),
                )
                .clicked()
            {
                // Split the largest interval at its current interpolated color: no visible jump.
                let i = g
                    .stops
                    .windows(2)
                    .enumerate()
                    .max_by(|(_, a), (_, b)| {
                        (a[1].position - a[0].position).total_cmp(&(b[1].position - b[0].position))
                    })
                    .unwrap()
                    .0;
                let position = (g.stops[i].position + g.stops[i + 1].position) / 2.;
                let color = g.color_at(position);
                g.stops.insert(i + 1, GradientStop { position, color });
                self.gradient_stop = i + 1;
                *commit = true;
            }
            if ui
                .add_enabled(
                    index > 0 && index < last,
                    egui::Button::new(lang.text("Elimina punto")),
                )
                .clicked()
            {
                g.stops.remove(index);
                self.gradient_stop = index - 1;
                *commit = true;
            }
        });
        let index = self.gradient_stop;
        let last = g.stops.len() - 1;
        if index > 0 && index < last {
            let min = g.stops[index - 1].position + 0.0011;
            let max = g.stops[index + 1].position - 0.0011;
            let default = (min + max) / 2.;
            slider(
                ui,
                lang,
                "Posizione",
                &mut g.stops[index].position,
                min..=max,
                default,
                commit,
            );
        }
        ui.small(lang.text("Colore del punto · RGB lineari"));
        for (name, value) in ["Rosso", "Verde", "Blu"]
            .into_iter()
            .zip(&mut g.stops[index].color)
        {
            slider(ui, lang, name, value, -1. ..=4., 0., commit);
        }
        egui::CollapsingHeader::new(lang.text("Intervallo luminanza")).show(ui, |ui| {
            let high = g.input[1];
            slider(
                ui,
                lang,
                "Nero ingresso",
                &mut g.input[0],
                -1. ..=high - 0.0011,
                0.,
                commit,
            );
            let low = g.input[0];
            slider(
                ui,
                lang,
                "Bianco ingresso",
                &mut g.input[1],
                low + 0.0011..=4.,
                1.,
                commit,
            );
            ui.small(lang.text("I toni esterni proseguono linearmente oltre i colori estremi."));
        });
    }
}
