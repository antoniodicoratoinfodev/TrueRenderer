use super::{Controls, color_wheel, slider};
use crate::i18n::Language;
use eframe::egui;
use tr_core::editing::layers::{
    Colorize, ExposureGamma, SelectiveColor, SelectiveMethod, TonalAdjustments,
};

impl Controls {
    pub(super) fn selective_color(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut SelectiveColor,
        commit: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            for (i, name) in [
                "Rossi", "Gialli", "Verdi", "Ciani", "Blu", "Magenta", "Bianchi", "Neutri", "Neri",
            ]
            .into_iter()
            .enumerate()
            {
                let edited = g.adjustments[i].iter().any(|v| *v != 0.);
                let label = if edited {
                    format!("{} •", lang.text(name))
                } else {
                    lang.text(name).into()
                };
                ui.selectable_value(&mut self.selective_family, i, label);
            }
        });
        ui.horizontal(|ui| {
            *commit |= ui
                .selectable_value(
                    &mut g.method,
                    SelectiveMethod::Relative,
                    lang.text("Relativo"),
                )
                .changed();
            *commit |= ui
                .selectable_value(
                    &mut g.method,
                    SelectiveMethod::Absolute,
                    lang.text("Assoluto"),
                )
                .changed();
        });
        ui.small(lang.text(match g.method {
            SelectiveMethod::Relative => "Relativo: percentuale del segnale RGB",
            SelectiveMethod::Absolute => "Assoluto: offset nel segnale RGB lineare",
        }));
        let family = &mut g.adjustments[self.selective_family];
        for (name, value) in ["Ciano (C)", "Magenta (M)", "Giallo (Y)", "Nero (K)"]
            .into_iter()
            .zip(family.iter_mut())
        {
            slider(ui, lang, name, value, -100. ..=100., 0., commit);
        }
        if ui.button(lang.text("Ripristina famiglia")).clicked() {
            *family = [0.; 4];
            *commit = true;
        }
        ui.small(lang.text("Regolazione creativa RGB"));
    }

    pub(super) fn colorize(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut Colorize,
        commit: &mut bool,
    ) {
        slider(
            ui,
            lang,
            "Quantità Colorizza",
            &mut g.amount,
            0. ..=100.,
            0.,
            commit,
        );
        let compact = ui.available_width() >= 300.;
        let layout = if compact {
            egui::Layout::left_to_right(egui::Align::Min)
        } else {
            egui::Layout::top_down(egui::Align::Min)
        };
        ui.with_layout(layout, |ui| {
            let side = if compact { 112. } else { 132. };
            ui.allocate_ui(egui::vec2(side, side), |ui| {
                let wheel = color_wheel(ui, lang, &mut g.hue, &mut g.chroma);
                *commit |= wheel.drag_stopped() || (wheel.changed() && !wheel.dragged());
            });
            ui.vertical(|ui| {
                slider(
                    ui,
                    lang,
                    "Tonalità (°)",
                    &mut g.hue,
                    0. ..=360.,
                    30.,
                    commit,
                );
                slider(ui, lang, "Cromia", &mut g.chroma, 0. ..=100., 50., commit);
            });
        });
        slider(
            ui,
            lang,
            "Luminosità Colorizza (EV)",
            &mut g.exposure,
            -2. ..=2.,
            0.,
            commit,
        );
        ui.small(lang.text("Luminanza conservata prima della luminosità"));
    }

    pub(super) fn tonal(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut TonalAdjustments,
        commit: &mut bool,
    ) {
        slider(
            ui,
            lang,
            "Esposizione",
            &mut g.exposure,
            -10. ..=10.,
            0.,
            commit,
        );
        for (name, value) in [
            ("Luminosità", &mut g.brightness),
            ("Contrasto", &mut g.contrast),
            ("Ombre", &mut g.shadows),
            ("Alte luci", &mut g.highlights),
            ("Neri", &mut g.blacks),
            ("Bianchi", &mut g.whites),
        ] {
            slider(ui, lang, name, value, -100. ..=100., 0., commit);
        }
        egui::CollapsingHeader::new(lang.text("Transizioni tonali")).show(ui, |ui| {
            slider(
                ui,
                lang,
                "Pivot tonale",
                &mut g.pivot,
                0.01..=4.,
                0.18,
                commit,
            );
            ui.small(lang.text("Luminosità: mezzitoni · esposizione: tutto il segnale"));
        });
    }

    pub(super) fn exposure_gamma(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        g: &mut ExposureGamma,
        commit: &mut bool,
    ) {
        slider(
            ui,
            lang,
            "Esposizione",
            &mut g.exposure,
            -10. ..=10.,
            0.,
            commit,
        );
        slider(ui, lang, "Offset", &mut g.offset, -4. ..=4., 0., commit);
        slider(ui, lang, "Gamma", &mut g.gamma, 0.25..=4., 1., commit);
        ui.small(lang.text("Ordine: esposizione → offset → gamma"));
        ui.small(lang.text("Gamma con segno · valori negativi conservati"));
    }
}
