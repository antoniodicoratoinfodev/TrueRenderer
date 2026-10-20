//! Parameter diagrams only; no invented image statistics.
use super::{egui, palette};
use crate::i18n::Language;

pub(super) fn color_field(
    ui: &mut egui::Ui,
    lang: Language,
    hue: &mut f32,
    chroma: &mut f32,
) -> egui::Response {
    let width = ui.available_width().clamp(40., 180.);
    let (outer, mut response) = ui
        .allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 112.),
            egui::Layout::top_down(egui::Align::Center),
            |ui| ui.allocate_exact_size(egui::vec2(width, 112.), egui::Sense::click_and_drag()),
        )
        .inner;
    let rect = outer.shrink(7.);
    let before = (*hue, *chroma);
    if ui.is_enabled() {
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if (response.clicked() || response.dragged())
            && let Some(p) = response.interact_pointer_pos()
        {
            *hue = (360. * (p.x - rect.left()) / rect.width()).clamp(0., 360.);
            *chroma = (100. * (rect.bottom() - p.y) / rect.height()).clamp(0., 100.);
        }
        if response.double_clicked() {
            *hue = 0.;
            *chroma = 0.;
        }
        if response.has_focus() {
            for (key, dh, dc) in [
                (egui::Key::ArrowLeft, -1., 0.),
                (egui::Key::ArrowRight, 1., 0.),
                (egui::Key::ArrowDown, 0., -1.),
                (egui::Key::ArrowUp, 0., 1.),
            ] {
                let step = ui.input_mut(|i| {
                    10 * i.count_and_consume_key(egui::Modifiers::SHIFT, key)
                        + i.count_and_consume_key(egui::Modifiers::NONE, key)
                }) as f32;
                if step != 0. {
                    *hue = (*hue + dh * step).rem_euclid(360.);
                    *chroma = (*chroma + dc * step).clamp(0., 100.);
                }
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Home)) {
                *hue = 0.;
                *chroma = 0.;
            }
        }
    }
    if before != (*hue, *chroma) {
        response.mark_changed();
    }
    if ui.is_rect_visible(outer) {
        let mut mesh = egui::Mesh::default();
        for row in 0..=12 {
            for col in 0..=48 {
                let x = col as f32 / 48.;
                let y = row as f32 / 12.;
                let color = egui::ecolor::Hsva::new(
                    x,
                    (1. - y) * 0.85,
                    if ui.is_enabled() { 0.82 } else { 0.35 },
                    1.,
                );
                mesh.colored_vertex(
                    egui::pos2(
                        rect.left() + x * rect.width(),
                        rect.top() + y * rect.height(),
                    ),
                    color.into(),
                );
                if row > 0 && col > 0 {
                    let n = row * 49 + col;
                    mesh.add_triangle(n - 50, n - 49, n);
                    mesh.add_triangle(n - 50, n, n - 1);
                }
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        let at = egui::pos2(
            rect.left() + rect.width() * (*hue / 360.).clamp(0., 1.),
            rect.bottom() - rect.height() * (*chroma / 100.).clamp(0., 1.),
        );
        ui.painter()
            .circle_stroke(at, 5., egui::Stroke::new(3., egui::Color32::BLACK));
        ui.painter()
            .circle_stroke(at, 5., egui::Stroke::new(1.4, egui::Color32::WHITE));
        if response.has_focus() {
            ui.painter().rect_stroke(
                outer,
                3,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Slider,
            ui.is_enabled(),
            lang.text("Campo tonalità e cromia"),
        )
    });
    response.on_hover_text(lang.text("Orizzontale: tonalità. Verticale: cromia. Frecce: regolazione fine; Maiusc: passo 10; Home o doppio clic: azzera."))
}

pub(super) fn filter_swatch(
    ui: &mut egui::Ui,
    lang: Language,
    hue: f32,
    saturation: f32,
    density: f32,
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.), egui::Sense::hover());
    let selected: egui::Color32 =
        egui::ecolor::Hsva::new(hue / 360., saturation / 100., 0.8, 1.).into();
    let gray = egui::Color32::from_gray(150);
    // Swatch shows the chosen UI color and density, not a prediction of photographed pixels.
    let amount = (density / 100.).clamp(0., 1.);
    let mix = egui::Color32::from_rgb(
        (150. * (1. - amount) + selected.r() as f32 * amount) as u8,
        (150. * (1. - amount) + selected.g() as f32 * amount) as u8,
        (150. * (1. - amount) + selected.b() as f32 * amount) as u8,
    );
    palette::strip(ui, rect.shrink2(egui::vec2(0., 4.)), &[gray, mix]);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.right_top() - egui::vec2(26., 0.), egui::vec2(26., 30.)),
        4,
        selected,
    );
    response.on_hover_text(lang.text(
        "Campione del colore scelto e della densità. Non è un’anteprima dei pixel della foto.",
    ));
}

pub(super) fn channels(ui: &mut egui::Ui, lang: Language, matrix: &[[f32; 3]; 3]) {
    let (r, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 74.), egui::Sense::hover());
    let p = ui.painter();
    let colors = [palette::RED, palette::GREEN, palette::BLUE];
    let pos = |right: bool, i: usize| {
        egui::pos2(
            if right {
                r.right() - 15.
            } else {
                r.left() + 15.
            },
            r.top() + 12. + i as f32 * 24.,
        )
    };
    for (out, row) in matrix.iter().enumerate() {
        for (input, &weight) in row.iter().enumerate() {
            if weight.abs() < 0.0001 {
                continue;
            }
            let stroke = egui::Stroke::new(
                0.6 + weight.abs().min(2.) * 1.2,
                colors[input].gamma_multiply(0.65),
            );
            let a = pos(false, input);
            let b = pos(true, out);
            if weight < 0. {
                for n in (0..12).step_by(2) {
                    p.line_segment(
                        [a.lerp(b, n as f32 / 12.), a.lerp(b, (n + 1) as f32 / 12.)],
                        stroke,
                    );
                }
            } else {
                p.line_segment([a, b], stroke);
            }
        }
    }
    for (i, color) in colors.iter().enumerate() {
        for right in [false, true] {
            p.circle_filled(pos(right, i), 9., ui.visuals().extreme_bg_color);
            p.text(
                pos(right, i),
                egui::Align2::CENTER_CENTER,
                ["R", "G", "B"][i],
                egui::FontId::proportional(11.),
                *color,
            );
        }
    }
    response.on_hover_text(lang.text("Da ingresso a uscita: lo spessore indica il peso del canale. Le linee tratteggiate indicano coefficienti negativi."));
}

pub(super) fn levels(
    ui: &mut egui::Ui,
    lang: Language,
    black: f32,
    white: f32,
    gamma: f32,
    output: [f32; 2],
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.), egui::Sense::hover());
    for (row, values) in [
        [black, black + (white - black) * 0.5_f32.powf(gamma), white],
        [output[0], (output[0] + output[1]) / 2., output[1]],
    ]
    .into_iter()
    .enumerate()
    {
        let lo = values[0].min(0.);
        let hi = values[2].max(1.);
        let strip = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(4., row as f32 * 24. + 3.),
            rect.right_top() + egui::vec2(-4., row as f32 * 24. + 13.),
        );
        palette::strip(
            ui,
            strip,
            &[egui::Color32::from_gray(25), egui::Color32::from_gray(235)],
        );
        for (i, v) in values.into_iter().enumerate() {
            if row == 1 && i == 1 {
                continue;
            }
            let x = strip.left() + (v - lo) / (hi - lo) * strip.width();
            let y = strip.bottom() + 2.;
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x, y),
                    egui::pos2(x - 4., y + 6.),
                    egui::pos2(x + 4., y + 6.),
                ],
                [
                    egui::Color32::from_gray(25),
                    egui::Color32::GRAY,
                    egui::Color32::from_gray(235),
                ][i],
                egui::Stroke::new(0.8, egui::Color32::from_gray(150)),
            ));
        }
    }
    response.on_hover_text(lang.text("Sopra: nero, mezzitoni e bianco in ingresso. Sotto: limiti di uscita. Sono i parametri dei livelli, non un istogramma."));
}
