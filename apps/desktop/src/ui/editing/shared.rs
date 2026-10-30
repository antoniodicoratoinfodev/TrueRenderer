//! Shared photographic sections for source photos and raster layers.
use super::*;
pub(in crate::ui) struct BasicResponse {
    pub changed: bool,
    pub commit: bool,
    pub native_requested: bool,
    pub auto_result: Option<Result<(), String>>,
}
pub(in crate::ui) fn basic_controls(
    ui: &mut egui::Ui,
    lang: crate::i18n::Language,
    draft: &mut EditRecipe,
    switches: &mut std::collections::BTreeSet<String>,
    mut auto_source: Option<impl FnMut(bool, &mut EditRecipe) -> anyhow::Result<()>>,
) -> BasicResponse {
    let mut changed = false;
    let mut commit = false;
    let mut native_requested = false;
    let mut auto_result = None;
    let mut reset = draft.clone();
    ResetGroup::Light.apply(&mut reset);
    panels::Section::new(lang, "light-panel", "Luce")
        .switches(switches)
        .icon("tonal")
        .modified(*draft != reset)
        .show(ui, |ui| {
            for (label, value, min, max, suffix) in [
                ("Esposizione", &mut draft.exposure_ev, -10., 10., " EV"),
                ("Luminosità", &mut draft.brightness, -100., 100., ""),
                ("Contrasto", &mut draft.contrast, -100., 100., ""),
                ("Alte luci", &mut draft.highlights, -100., 100., ""),
                ("Ombre", &mut draft.shadows, -100., 100., ""),
                ("Bianchi", &mut draft.whites, -100., 100., ""),
                ("Neri", &mut draft.blacks, -100., 100., ""),
            ] {
                let response =
                    adjustment(ui, lang, lang.text(label), value, min..=max, suffix, None);
                changed |= response.changed();
                commit |= response.drag_stopped() || (response.changed() && !response.dragged());
            }
            ui.horizontal_wrapped(|ui| {
                if auto_source.is_none()
                    && help::button(ui, lang, "Carica nativo per Auto").clicked()
                {
                    native_requested = true;
                }
                for (label, rgb) in [
                    ("Auto esposizione", false),
                    ("Auto RGB · grigio medio", true),
                ] {
                    if help::button_enabled(ui, lang, label, auto_source.is_some()).clicked() {
                        let result = auto_source.as_mut().unwrap()(rgb, draft);
                        if result.is_ok() {
                            changed = true;
                            commit = true;
                        }
                        auto_result = Some(result.map_err(|error| error.to_string()));
                    }
                }
            });
            ui.label(
                RichText::new(
                    lang.text("Auto RGB assume una scena mediamente neutra; non cambia il WB RAW."),
                )
                .small()
                .color(MUTED),
            );
            if reset_group_button(ui, lang.text("Azzera luce"), ResetGroup::Light, draft) {
                changed = true;
                commit = true;
            }
        });
    let mut reset = draft.clone();
    ResetGroup::Curve.apply(&mut reset);
    panels::Section::new(lang, "curve-panel", "Curva tonale")
        .switches(switches)
        .icon("curves")
        .modified(*draft != reset)
        .show(ui, |ui| {
            let (mid_changed, mid_commit) = curve::midtone(&mut draft.curve, ui, lang);
            changed |= mid_changed;
            commit |= mid_commit;
            let (curve_changed, curve_commit) = ui
                .push_id("shared-curve-editor", |ui| {
                    curve::controls(ui, lang, &mut draft.curve)
                })
                .inner;
            changed |= curve_changed;
            commit |= curve_commit;
            if (curve_changed || mid_changed)
                && tr_core::editing::curve_has_adjusted_endpoints(&draft.curve)
            {
                draft.require_process(4);
            }
            if reset_group_button(ui, lang.text("Azzera curva"), ResetGroup::Curve, draft) {
                changed = true;
                commit = true;
            }
        });
    let mut reset = draft.clone();
    ResetGroup::Color.apply(&mut reset);
    panels::Section::new(lang, "color-panel", "Colore RGB")
        .switches(switches)
        .icon("mixer")
        .modified(*draft != reset)
        .show(ui, |ui| {
            {
                let (label, value) = ("Saturazione", &mut draft.saturation);
                let response =
                    adjustment(ui, lang, lang.text(label), value, -100. ..=100., "", None);
                changed |= response.changed();
                commit |= response.drag_stopped() || (response.changed() && !response.dragged());
            }
            let response = adjustment(
                ui,
                lang,
                lang.text("Vividezza"),
                &mut draft.vibrance,
                -100. ..=100.,
                "",
                None,
            );
            let protection =
                help::checkbox(ui, lang, "Proteggi toni caldi", &mut draft.protect_warm);
            if response.changed() || protection.changed() {
                draft.require_process(2);
                changed = true;
            }
            commit |= response.drag_stopped()
                || (response.changed() && !response.dragged())
                || protection.changed();
            ui.small(
                lang.text("Vividezza: processo 2 · protezione indicativa, non rileva la pelle"),
            );
            ui.label(
                RichText::new(lang.text("La correzione RGB non cambia il WB RAW del decoder."))
                    .small()
                    .color(MUTED),
            );
            if reset_group_button(ui, lang.text("Azzera colore"), ResetGroup::Color, draft) {
                changed = true;
                commit = true;
            }
        });

    BasicResponse {
        changed,
        commit,
        native_requested,
        auto_result,
    }
}
