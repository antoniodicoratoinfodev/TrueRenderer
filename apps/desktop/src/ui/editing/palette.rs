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
        "Tinta RGB" | "RGB tint" | "Tinta RAW" | "RAW tint" => Some(vec![GREEN, GRAY, MAGENTA]),
        "Tonalità (°)" | "Hue (°)" | "Tonalità" | "Hue" => {
            Some(vec![RED, YELLOW, GREEN, CYAN, BLUE, MAGENTA, RED])
        }
        _ => family(label).map(|c| vec![c.gamma_multiply(0.35), c]),
    }
}

pub(super) fn tool(id: &str) -> (Color32, &'static str) {
    match id {
        "light" | "levels" | "tonal" | "exposure_gamma" => (WARM, "Tono"),
        "curves" | "luminance_curve" | "parametric_curve" => (GREEN, "Curve"),
        "grading" | "filter" | "gradient_map" | "colorize" => (VIOLET, "Viraggio"),
        "black_white" => (GRAY, "Bianco e nero"),
        _ => (CYAN, "Colore"),
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
    selected: bool,
    label: &str,
    color: Option<Color32>,
) -> egui::Response {
    let response = ui.selectable_label(selected, label);
    if let Some(color) = color {
        let rect = egui::Rect::from_min_max(
            egui::pos2(response.rect.left() + 5., response.rect.bottom() - 3.),
            egui::pos2(response.rect.right() - 5., response.rect.bottom() - 1.),
        );
        ui.painter().rect_filled(rect, 1, enabled(ui, color));
    }
    response
}

pub(super) fn icon(ui: &mut egui::Ui, id: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22., 22.), egui::Sense::hover());
    let (color, group) = tool(id);
    let c = enabled(ui, color);
    let p = ui.painter();
    p.rect_filled(rect, 5, c.gamma_multiply(0.12));
    let inside = rect.shrink(5.);
    match group {
        "Curve" => {
            p.line_segment(
                [inside.left_bottom(), inside.right_bottom()],
                egui::Stroke::new(1., c),
            );
            p.line_segment(
                [inside.left_bottom(), inside.left_top()],
                egui::Stroke::new(1., c),
            );
            p.add(egui::Shape::line(
                vec![inside.left_bottom(), inside.center(), inside.right_top()],
                egui::Stroke::new(2., c),
            ));
        }
        "Tono" | "Bianco e nero" => {
            for i in 0..3 {
                let x = inside.left() + i as f32 * 4.;
                p.line_segment(
                    [
                        egui::pos2(x, inside.bottom()),
                        egui::pos2(x, inside.bottom() - 4. - i as f32 * 4.),
                    ],
                    egui::Stroke::new(2., c),
                );
            }
        }
        _ => {
            for (offset, color) in [
                (egui::vec2(-3., 2.), RED),
                (egui::vec2(3., 2.), BLUE),
                (
                    egui::vec2(0., -3.),
                    if group == "Viraggio" { VIOLET } else { GREEN },
                ),
            ] {
                p.circle_filled(rect.center() + offset, 3., enabled(ui, color));
            }
        }
    }
}
