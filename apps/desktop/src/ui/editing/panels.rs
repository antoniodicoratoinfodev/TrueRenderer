//! Compact photographic panel chrome; view state never belongs to a recipe.
use super::{egui, help, palette};
use crate::{i18n::Language, ui::style};

/// The inspector uses one rhythm for buttons, fields and parameter rows.
pub(super) fn control_style(ui: &mut egui::Ui) {
    let compact = ui.ctx().content_rect().height() < 500.;
    ui.spacing_mut().item_spacing = egui::vec2(6., if compact { 4. } else { 6. });
    ui.spacing_mut().button_padding = egui::vec2(8., 5.);
    ui.spacing_mut().interact_size.y = 28.;
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.bg_stroke = egui::Stroke::new(1., egui::Color32::from_gray(66));
    widgets.inactive.weak_bg_fill = egui::Color32::from_gray(37);
    widgets.hovered.weak_bg_fill = egui::Color32::from_gray(49);
    widgets.active.weak_bg_fill = egui::Color32::from_gray(55);
}

pub(super) fn segment(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    width: f32,
) -> egui::Response {
    style::segment(ui, selected, label, width)
}

fn focus_outline(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            4,
            egui::Stroke::new(1., style::SELECTION_EDGE),
            egui::StrokeKind::Inside,
        );
    }
}

pub(super) fn save_state(ui: &mut egui::Ui, lang: Language, pending: bool, dirty: bool) {
    let label = lang.text(if pending {
        "Salvataggio…"
    } else if dirty {
        "Modifiche in corso"
    } else {
        "Ricetta salvata nella libreria"
    });
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22., 28.), egui::Sense::hover());
    let center = rect.center();
    let stroke = egui::Stroke::new(1.3, style::MUTED);
    if pending || dirty {
        ui.painter().circle_stroke(center, 4., stroke);
        if pending {
            ui.painter()
                .line_segment([center, center + egui::vec2(0., -3.)], stroke);
        }
    } else {
        ui.painter().add(egui::Shape::line(
            vec![
                center + egui::vec2(-4., 0.),
                center + egui::vec2(-1., 3.),
                center + egui::vec2(5., -4.),
            ],
            stroke,
        ));
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    response.on_hover_text(label);
}

pub(super) fn tool_button(
    ui: &mut egui::Ui,
    lang: Language,
    tool: &tr_core::editing::layers::Tool,
    selected: bool,
) -> egui::Response {
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding.x = 32.;
            ui.add_sized(
                [ui.available_width(), 32.],
                egui::Button::new(())
                    .left_text(lang.text(tool.name))
                    .selected(selected)
                    .frame_when_inactive(selected)
                    .stroke(egui::Stroke::NONE)
                    .truncate(),
            )
        })
        .inner;
    if ui.is_rect_visible(response.rect) {
        focus_outline(ui, &response);
        let icon = egui::Rect::from_center_size(
            egui::pos2(response.rect.left() + 15., response.rect.center().y),
            egui::vec2(20., 20.),
        );
        palette::paint_icon(ui, icon, tool.id);
        let center = egui::pos2(response.rect.right() - 13., response.rect.center().y);
        let stroke = egui::Stroke::new(1., ui.visuals().weak_text_color());
        ui.painter().line_segment(
            [center - egui::vec2(3., 0.), center + egui::vec2(3., 0.)],
            stroke,
        );
        ui.painter().line_segment(
            [center - egui::vec2(0., 3.), center + egui::vec2(0., 3.)],
            stroke,
        );
    }
    response
        .on_hover_text(help::tool_text(lang, tool.id).unwrap_or(lang.text(tool.description)))
        .on_disabled_hover_text(
            help::tool_text(lang, tool.id).unwrap_or(lang.text(tool.description)),
        )
}

pub(super) fn layer_state(ui: &mut egui::Ui, lang: Language, masked: bool, locked: bool) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30., 28.), egui::Sense::hover());
    let p = ui.painter();
    let color = if masked {
        style::TEXT
    } else {
        style::MUTED.gamma_multiply(0.55)
    };
    let stroke = egui::Stroke::new(1., color);
    let mask = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8., rect.center().y),
        egui::vec2(12., 14.),
    );
    p.rect_stroke(mask, 2, stroke, egui::StrokeKind::Inside);
    if masked {
        p.line_segment(
            [
                mask.left_bottom() + egui::vec2(2., -2.),
                mask.right_top() + egui::vec2(-2., 2.),
            ],
            stroke,
        );
    }
    if locked {
        let center = egui::pos2(rect.right() - 5., rect.center().y + 2.);
        let lock = egui::Rect::from_center_size(center, egui::vec2(7., 6.));
        p.rect_stroke(
            lock,
            1,
            egui::Stroke::new(1., style::MUTED),
            egui::StrokeKind::Inside,
        );
        p.add(egui::Shape::line(
            vec![
                center + egui::vec2(-2., -3.),
                center + egui::vec2(-2., -6.),
                center + egui::vec2(2., -6.),
                center + egui::vec2(2., -3.),
            ],
            egui::Stroke::new(1., style::MUTED),
        ));
    }
    let label = format!(
        "{}{}",
        lang.text(if masked {
            "Con maschera"
        } else {
            "Foto intera"
        }),
        if locked {
            format!(" · {}", lang.text("Bloccato"))
        } else {
            String::new()
        }
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &label)
    });
    response.on_hover_text(label);
}

pub(super) struct Section<'a> {
    lang: Language,
    id: &'a str,
    title: &'a str,
    icon: Option<&'a str>,
    modified: bool,
    default_open: bool,
    switches: Option<&'a mut std::collections::BTreeSet<String>>,
}

impl<'a> Section<'a> {
    pub fn new(lang: Language, id: &'a str, title: &'a str) -> Self {
        Self {
            lang,
            id,
            title,
            icon: None,
            modified: false,
            default_open: true,
            switches: None,
        }
    }
    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }
    pub fn modified(mut self, modified: bool) -> Self {
        self.modified = modified;
        self
    }
    pub fn switches(mut self, switches: &'a mut std::collections::BTreeSet<String>) -> Self {
        self.switches = Some(switches);
        self
    }
    pub fn show<R>(
        mut self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::Response {
        let id = ui.make_persistent_id(self.id);
        let compact = ui.ctx().content_rect().height() < 500.;
        egui::Frame::new()
            .fill(egui::Color32::from_gray(32))
            .corner_radius(6)
            .inner_margin(if compact { 4 } else { 6 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let mut clicked = false;
                let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    self.default_open,
                )
                .show_header(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.;
                    if let Some(icon) = self.icon.filter(|_| !compact) {
                        palette::icon(ui, self.lang, icon);
                    }
                    let reserve = 40.;
                    let response = ui.add_sized(
                        [(ui.available_width() - reserve).max(0.), 24.],
                        egui::Button::new(())
                            .left_text(egui::RichText::new(self.lang.text(self.title)).strong())
                            .frame(false)
                            .truncate(),
                    );
                    clicked = response.clicked();
                    help::attach(ui, response, self.lang, self.title);
                    if let Some(switches) = self.switches.as_deref_mut() {
                        let enabled = !switches.contains(self.id);
                        let label = match (self.lang, enabled) {
                            (Language::English, true) => "Disable tool (keep values)",
                            (Language::English, false) => "Enable tool (restore values)",
                            (_, true) => "Disattiva strumento conservando i valori",
                            (_, false) => "Riattiva strumento con i valori conservati",
                        };
                        let response = ui.add_sized(
                            [28., 28.],
                            egui::Button::new(if enabled { "●" } else { "○" })
                                .selected(enabled && self.modified),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                enabled,
                                label,
                            )
                        });
                        if response.on_hover_text(label).clicked() {
                            if enabled {
                                switches.insert(self.id.into());
                            } else {
                                switches.remove(self.id);
                            }
                        }
                    } else if self.modified {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(8., 16.), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 2.5, style::MUTED);
                        response
                            .on_hover_text(self.lang.text("Questa sezione contiene modifiche."));
                    }
                });
                if clicked {
                    header.toggle();
                }
                let (_, heading, _) = header.body_unindented(|ui| {
                    ui.add_space(2.);
                    // A modified badge appears on the first slider movement.
                    // Keep every body widget ID stable across that transition.
                    let enabled = self.switches.as_ref().is_none_or(|s| !s.contains(self.id));
                    ui.scope_builder(egui::UiBuilder::new().id(id.with("body")), |ui| {
                        ui.add_enabled_ui(enabled, body).inner
                    })
                    .inner
                });
                heading.response
            })
            .inner
    }
}

pub(super) fn tabs(ui: &mut egui::Ui, lang: Language, current: usize) -> Option<usize> {
    let mut selected = None;
    egui::Frame::new()
        .fill(style::CANVAS)
        .corner_radius(6)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            ui.spacing_mut().button_padding.x = 2.;
            let width = ((ui.available_width() - 4.) / 3.).max(0.);
            ui.horizontal(|ui| {
                for (i, title) in ["Regolazioni", "Livelli", "Strumenti"]
                    .into_iter()
                    .enumerate()
                {
                    let response = segment(ui, current == i, lang.text(title), width);
                    if help::attach(ui, response, lang, title).clicked() {
                        selected = Some(i);
                    }
                }
            });
        });
    selected
}

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Undo,
    Redo,
    Eye,
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    glyph: Glyph,
    label: &str,
    enabled: bool,
    selected: bool,
) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new(())
                .min_size(egui::vec2(28., 28.))
                .selected(selected)
                .frame_when_inactive(false),
        )
        .on_hover_text(label)
        .on_disabled_hover_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            response.enabled(),
            selected,
            label,
        )
    });
    if ui.is_rect_visible(response.rect) {
        let center = response.rect.center();
        let p = |x: f32, y: f32| center + egui::vec2(x, y);
        let color = if !response.enabled() {
            egui::Color32::from_gray(92)
        } else {
            style::MUTED
        };
        let stroke = egui::Stroke::new(1.4, color);
        let painter = ui.painter();
        match glyph {
            Glyph::Undo | Glyph::Redo => {
                let d = if matches!(glyph, Glyph::Undo) {
                    1.
                } else {
                    -1.
                };
                painter.add(egui::Shape::line(
                    vec![
                        p(5. * d, 5.),
                        p(5. * d, -1.),
                        p(2. * d, -4.),
                        p(-6. * d, -4.),
                    ],
                    stroke,
                ));
                painter.add(egui::Shape::line(
                    vec![p(-2. * d, -8.), p(-6. * d, -4.), p(-2. * d, 0.)],
                    stroke,
                ));
            }
            Glyph::Eye => {
                let points = (0..=32)
                    .map(|n| {
                        let t = n as f32 * std::f32::consts::TAU / 32.;
                        p(8. * t.cos(), 4.5 * t.sin())
                    })
                    .collect();
                painter.add(egui::Shape::closed_line(points, stroke));
                painter.circle_filled(center, 2., color);
                if !selected {
                    painter.line_segment([p(-8., 7.), p(8., -7.)], stroke);
                }
            }
        }
    }
    response
}
