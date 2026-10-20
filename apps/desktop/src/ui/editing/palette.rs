//! Semantic UI cues only: these colours never enter a photographic recipe.
use eframe::egui::{self, Color32};

pub(super) const RED: Color32 = Color32::from_rgb(235, 112, 113);
pub(super) const GREEN: Color32 = Color32::from_rgb(113, 202, 148);
pub(super) const BLUE: Color32 = Color32::from_rgb(116, 169, 238);
const CYAN: Color32 = Color32::from_rgb(99, 204, 211);
const MAGENTA: Color32 = Color32::from_rgb(218, 132, 203);
const YELLOW: Color32 = Color32::from_rgb(223, 200, 112);
const VIOLET: Color32 = Color32::from_rgb(178, 152, 228);
const WARM: Color32 = Color32::from_rgb(227, 173, 112);
const GRAY: Color32 = Color32::from_gray(182);

pub(super) fn family(label: &str) -> Option<Color32> {
    Some(match label {
        "Rosso" | "Rossi" | "Red" | "Reds" | "Rosso RAW" | "RAW red" => RED,
        "Verde" | "Verdi" | "Green" | "Greens" => GREEN,
        "Blu" | "Blue" | "Blues" | "Blu RAW" | "RAW blue" => BLUE,
        "Ciano (C)" | "Cyan (C)" | "Ciani" | "Cyans" | "Acquamarina" | "Aqua" => CYAN,
        "Magenta" | "Magentas" | "Magenta (M)" => MAGENTA,
        "Giallo" | "Gialli" | "Yellow" | "Yellows" | "Giallo (Y)" | "Yellow (Y)" => YELLOW,
        "Arancio" | "Orange" => WARM,
        "Viola" | "Purple" => VIOLET,
        "Bianchi" | "Whites" | "Luci" | "Highlights" => Color32::from_gray(230),
        "Neutri" | "Neutrals" | "Mezzitoni" | "Midtones" | "RGB" | "Globale" | "Global" => GRAY,
        "Neri" | "Blacks" | "Nero (K)" | "Black (K)" | "Ombre" | "Shadows" => {
            Color32::from_gray(120)
        }
        _ => return None,
    })
}

pub(super) fn ramp(label: &str) -> Option<Vec<Color32>> {
    match label {
        "Temperatura RGB" | "RGB warmth" | "Temperatura" | "Temperature" | "Temperatura RAW"
        | "RAW temperature" => Some(vec![BLUE, GRAY, WARM]),
        "Tinta" | "Tint" | "Tinta RGB" | "RGB tint" | "Tinta RAW" | "RAW tint" => {
            Some(vec![GREEN, GRAY, MAGENTA])
        }
        "Tonalità (°)" | "Hue (°)" | "Tonalità" | "Hue" => {
            Some(vec![RED, YELLOW, GREEN, CYAN, BLUE, MAGENTA, RED])
        }
        _ => family(label)
            .filter(|c| c.r() != c.g() || c.g() != c.b())
            .map(|c| vec![c.gamma_multiply(0.35), c]),
    }
}

fn enabled(ui: &egui::Ui, color: Color32) -> Color32 {
    if ui.is_enabled() {
        color
    } else {
        Color32::from_gray(85)
    }
}

pub(super) fn strip(ui: &egui::Ui, rect: egui::Rect, colors: &[Color32]) {
    if colors.len() < 2 || !ui.is_rect_visible(rect) {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for (i, color) in colors.iter().enumerate() {
        let x = egui::lerp(rect.x_range(), i as f32 / (colors.len() - 1) as f32);
        mesh.colored_vertex(egui::pos2(x, rect.top()), enabled(ui, *color));
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), enabled(ui, *color));
        if i > 0 {
            let a = (i * 2) as u32;
            mesh.add_triangle(a - 2, a - 1, a);
            mesh.add_triangle(a - 1, a + 1, a);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

pub(super) fn choice(
    ui: &mut egui::Ui,
    lang: crate::i18n::Language,
    selected: bool,
    label: &str,
    color: Option<Color32>,
) -> egui::Response {
    let response = ui.selectable_label(selected, label);
    if let Some(color) = color.filter(|c| selected || c.r() != c.g() || c.g() != c.b()) {
        let rect = egui::Rect::from_min_max(
            egui::pos2(response.rect.left() + 5., response.rect.bottom() - 3.),
            egui::pos2(response.rect.right() - 5., response.rect.bottom() - 1.),
        );
        ui.painter().rect_filled(rect, 1, enabled(ui, color));
    }
    super::help::attach(ui, response, lang, label.trim_end_matches(" •"))
}

pub(super) fn icon(ui: &mut egui::Ui, lang: crate::i18n::Language, id: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::hover());
    paint_icon(ui, rect, id);
    if let Some(text) = super::help::tool_text(lang, id) {
        response.on_hover_text(text).on_disabled_hover_text(text);
    }
}

pub(super) fn paint_icon(ui: &egui::Ui, rect: egui::Rect, id: &str) {
    draw_symbol(
        ui,
        rect.shrink(2.),
        id,
        enabled(ui, Color32::from_gray(174)),
    );
}

// Each diagram describes a photographic operation; shape distinguishes tools even without color.
fn draw_symbol(ui: &egui::Ui, rect: egui::Rect, id: &str, c: Color32) {
    let p = ui.painter();
    let at = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let line = |a: [f32; 2], b: [f32; 2], color| {
        p.line_segment(
            [at(a[0], a[1]), at(b[0], b[1])],
            egui::Stroke::new(1.35, color),
        );
    };
    let path = |points: &[[f32; 2]], color| {
        p.add(egui::Shape::line(
            points.iter().map(|v| at(v[0], v[1])).collect(),
            egui::Stroke::new(1.5, color),
        ));
    };
    let dot = |x, y, r, color| {
        p.circle_filled(at(x, y), r, enabled(ui, color));
    };
    match id {
        "sample" => {
            p.circle_stroke(
                at(0.45, 0.45),
                rect.width() * 0.3,
                egui::Stroke::new(1.4, c),
            );
            line([0.45, 0.], [0.45, 0.22], c);
            line([0.45, 0.68], [0.45, 0.9], c);
            line([0., 0.45], [0.22, 0.45], c);
            line([0.68, 0.45], [0.9, 0.45], c);
            dot(0.82, 0.85, 2.6, WARM);
        }
        "light" => {
            p.circle_stroke(at(0.35, 0.35), 3., egui::Stroke::new(1.4, c));
            for n in 0..8 {
                let a = n as f32 * std::f32::consts::TAU / 8.;
                line(
                    [0.35 + 0.27 * a.cos(), 0.35 + 0.27 * a.sin()],
                    [0.35 + 0.38 * a.cos(), 0.35 + 0.38 * a.sin()],
                    c,
                );
            }
            line([0.1, 0.88], [1., 0.88], c);
            dot(0.65, 0.88, 2., c);
        }
        "curves" => {
            path(
                &[[0., 1.], [0.15, 0.7], [0.6, 0.55], [1., 0.]],
                enabled(ui, RED),
            );
            path(
                &[[0., 1.], [0.4, 0.85], [0.7, 0.25], [1., 0.]],
                enabled(ui, GREEN),
            );
            path(
                &[[0., 1.], [0.3, 0.4], [0.8, 0.2], [1., 0.]],
                enabled(ui, BLUE),
            );
        }
        "levels" => {
            strip(
                ui,
                egui::Rect::from_min_max(at(0., 0.2), at(1., 0.65)),
                &[Color32::from_gray(35), Color32::from_gray(230)],
            );
            for (x, col) in [(0., GRAY), (0.5, c), (1., Color32::WHITE)] {
                path(
                    &[[x - 0.1, 0.98], [x, 0.72], [x + 0.1, 0.98]],
                    enabled(ui, col),
                );
            }
        }
        "mixer" => {
            for (i, col) in [RED, WARM, YELLOW, GREEN, CYAN, BLUE, VIOLET, MAGENTA]
                .into_iter()
                .enumerate()
            {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                line(
                    [0.5 + 0.23 * a.cos(), 0.5 + 0.23 * a.sin()],
                    [0.5 + 0.47 * a.cos(), 0.5 + 0.47 * a.sin()],
                    enabled(ui, col),
                );
            }
            line([0.15, 0.95], [0.85, 0.95], c);
        }
        "channels" => {
            for (i, col) in [RED, GREEN, BLUE].into_iter().enumerate() {
                let y = 0.15 + i as f32 * 0.35;
                line(
                    [0.13, y],
                    [0.87, 1. - y],
                    enabled(ui, col).gamma_multiply(0.65),
                );
                line([0.13, y], [0.87, y], enabled(ui, col));
                dot(0.1, y, 2., col);
                dot(0.9, y, 2., col);
            }
        }
        "grading" => {
            for (x, y, col) in [
                (0.2, 0.25, BLUE),
                (0.8, 0.25, WARM),
                (0.2, 0.8, MAGENTA),
                (0.8, 0.8, GRAY),
            ] {
                p.circle_stroke(at(x, y), 3., egui::Stroke::new(1.4, enabled(ui, col)));
                line([x, y], [x + 0.12, y - 0.12], enabled(ui, col));
            }
        }
        "black_white" => {
            p.circle_filled(at(0.5, 0.5), rect.width() * 0.44, Color32::from_gray(215));
            let mut points = vec![at(0.5, 0.5)];
            points.extend((0..=16).map(|i| {
                let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 16.;
                at(0.5 + 0.44 * a.cos(), 0.5 + 0.44 * a.sin())
            }));
            p.add(egui::Shape::convex_polygon(
                points,
                Color32::from_gray(50),
                egui::Stroke::NONE,
            ));
            p.circle_stroke(at(0.5, 0.5), rect.width() * 0.44, egui::Stroke::new(1., c));
        }
        "filter" => {
            path(
                &[[0., 0.32], [0.5, 0.02], [1., 0.32], [0.5, 0.62], [0., 0.32]],
                c,
            );
            path(&[[0., 0.55], [0.5, 0.85], [1., 0.55]], enabled(ui, WARM));
            path(
                &[[0.15, 0.8], [0.5, 1.], [0.85, 0.8]],
                c.gamma_multiply(0.5),
            );
        }
        "gradient_map" => {
            strip(
                ui,
                egui::Rect::from_min_max(at(0., 0.25), at(1., 0.7)),
                &[BLUE, VIOLET, WARM],
            );
            for x in [0., 0.5, 1.] {
                path(&[[x - 0.08, 0.96], [x, 0.77], [x + 0.08, 0.96]], c);
            }
        }
        "selective_color" => {
            for (x, y, col) in [
                (0.25, 0.25, CYAN),
                (0.75, 0.25, MAGENTA),
                (0.25, 0.75, YELLOW),
                (0.75, 0.75, GRAY),
            ] {
                dot(x, y, 3., col);
            }
            path(&[[0.55, 0.04], [0.98, 0.04], [0.98, 0.45]], Color32::WHITE);
        }
        "colorize" => {
            strip(
                ui,
                egui::Rect::from_min_max(at(0., 0.05), at(1., 0.55)),
                &[RED, YELLOW, GREEN, CYAN, BLUE, MAGENTA],
            );
            line([0.15, 0.95], [0.8, 0.35], c);
            line([0.25, 1.], [0.9, 0.4], c);
            dot(0.18, 0.93, 2., c);
        }
        "tonal" => {
            for (i, h) in [0.25, 0.55, 0.9, 0.7, 0.4].into_iter().enumerate() {
                let x = 0.05 + i as f32 * 0.22;
                p.rect_filled(
                    egui::Rect::from_min_max(at(x, 1. - h), at(x + 0.13, 1.)),
                    1,
                    enabled(ui, Color32::from_gray(65 + i as u8 * 40)),
                );
            }
        }
        "exposure_gamma" => {
            path(
                &[[0., 0.8], [0.2, 0.74], [0.45, 0.6], [0.7, 0.36], [1., 0.]],
                c,
            );
            line([0., 1.], [1., 1.], GRAY);
            line([0., 0.8], [0., 1.], c);
            line([0.15, 0.12], [0.15, 0.45], c);
            line([0., 0.28], [0.3, 0.28], c);
        }
        "luminance_curve" => {
            line([0., 1.], [1., 0.], GRAY.gamma_multiply(0.4));
            path(&[[0., 1.], [0.25, 0.9], [0.6, 0.2], [1., 0.]], c);
            dot(0.25, 0.9, 1.7, GRAY);
            dot(0.6, 0.2, 1.7, Color32::WHITE);
        }
        "parametric_curve" => {
            for (i, shade) in [45, 65, 85, 105].into_iter().enumerate() {
                p.rect_filled(
                    egui::Rect::from_min_max(
                        at(i as f32 / 4., 0.),
                        at((i + 1) as f32 / 4. - 0.02, 1.),
                    ),
                    0,
                    Color32::from_gray(shade),
                );
            }
            path(
                &[[0., 1.], [0.25, 0.8], [0.5, 0.5], [0.75, 0.13], [1., 0.]],
                c,
            );
        }
        _ => {
            line([0., 0.5], [1., 0.5], c);
        }
    }
}
