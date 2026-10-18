//! Compact photographic panel chrome; view state never belongs to a recipe.
use super::{egui, help, palette};
use crate::{i18n::Language, ui::style};

pub(super) struct Section<'a> {
    lang: Language,
    id: &'a str,
    title: &'a str,
    icon: Option<&'a str>,
    modified: bool,
    default_open: bool,
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
    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::Response {
        let id = ui.make_persistent_id(self.id);
        let compact = ui.ctx().content_rect().height() < 500.;
        egui::Frame::new()
            .fill(egui::Color32::from_gray(32))
            .stroke(egui::Stroke::new(1., style::LINE))
            .corner_radius(6)
            .inner_margin(if compact { 4 } else { 8 })
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
                    let reserve = if self.modified { 12. } else { 0. };
                    let response = ui.add_sized(
                        [(ui.available_width() - reserve).max(0.), 28.],
                        egui::Button::new(())
                            .left_text(egui::RichText::new(self.lang.text(self.title)).strong())
                            .frame(false)
                            .wrap(),
                    );
                    clicked = response.clicked();
                    help::attach(ui, response, self.lang, self.title);
                    if self.modified {
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
                    if !compact {
                        ui.add_space(4.);
                        ui.separator();
                    }
                    // A modified badge appears on the first slider movement.
                    // Keep every body widget ID stable across that transition.
                    ui.scope_builder(egui::UiBuilder::new().id(id.with("body")), body)
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
                    let response = ui.add_sized(
                        [width, 30.],
                        egui::Button::selectable(current == i, lang.text(title))
                            .wrap()
                            .frame_when_inactive(current == i),
                    );
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
    Lock,
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
                .frame_when_inactive(selected),
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
            Glyph::Lock => {
                painter.rect_stroke(
                    egui::Rect::from_min_max(p(-5., -1.), p(5., 7.)),
                    2,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                painter.add(egui::Shape::line(
                    vec![p(-3., -1.), p(-3., -5.), p(0., -7.), p(3., -5.), p(3., -1.)],
                    stroke,
                ));
                painter.circle_filled(p(0., 3.), 1., color);
            }
        }
    }
    response
}
