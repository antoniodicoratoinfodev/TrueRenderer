use super::{CurvePoint, adjustment};
use crate::i18n::Language;
use eframe::egui;

pub(super) fn controls(
    ui: &mut egui::Ui,
    lang: Language,
    curve: &mut Vec<CurvePoint>,
) -> (bool, bool) {
    let before = curve.clone();
    let mut commit = false;
    egui::CollapsingHeader::new(lang.text("Curva a punti"))
        .id_salt("point-curve")
        .show(ui, |ui| {
            if ui
                .add_enabled(
                    curve.len() < 32,
                    egui::Button::new(lang.text("Aggiungi punto")),
                )
                .clicked()
            {
                if curve.is_empty() {
                    *curve = vec![CurvePoint { x: 0., y: 0. }, CurvePoint { x: 1., y: 1. }];
                }
                let i = curve
                    .windows(2)
                    .enumerate()
                    .max_by(|(_, a), (_, b)| (a[1].x - a[0].x).total_cmp(&(b[1].x - b[0].x)))
                    .unwrap()
                    .0;
                curve.insert(
                    i + 1,
                    CurvePoint {
                        x: (curve[i].x + curve[i + 1].x) * 0.5,
                        y: (curve[i].y + curve[i + 1].y) * 0.5,
                    },
                );
                commit = true;
            }
            let mut remove = None;
            for i in 1..curve.len().saturating_sub(1) {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} {i}", lang.text("Punto")));
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                    let low = curve[i - 1];
                    let high = curve[i + 1];
                    let min = (low.x + 1e-6).min(curve[i].x);
                    let max = (high.x - 1e-6).max(curve[i].x);
                    let r = adjustment(
                        ui,
                        lang.text("Ingresso"),
                        &mut curve[i].x,
                        min..=max,
                        "",
                        None,
                    );
                    commit |= r.drag_stopped() || (r.changed() && !r.dragged());
                    let r = adjustment(
                        ui,
                        lang.text("Uscita"),
                        &mut curve[i].y,
                        low.y..=high.y,
                        "",
                        None,
                    );
                    commit |= r.drag_stopped() || (r.changed() && !r.dragged());
                });
            }
            if let Some(i) = remove {
                curve.remove(i);
                commit = true;
            }
            // A small graph shows the actual interpolation, including custom points.
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2, egui::Color32::from_gray(24));
            let point = |p: CurvePoint| {
                egui::pos2(
                    rect.left() + rect.width() * p.x,
                    rect.bottom() - rect.height() * p.y,
                )
            };
            let points = if curve.is_empty() {
                vec![
                    point(CurvePoint { x: 0., y: 0. }),
                    point(CurvePoint { x: 1., y: 1. }),
                ]
            } else {
                curve.iter().copied().map(point).collect()
            };
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(1.5, egui::Color32::LIGHT_GRAY),
            ));
        });
    (*curve != before, commit)
}
