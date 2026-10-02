//! Shared chrome styling. Image pixels and the renderer's neutral surround are independent.
use eframe::egui::{self, Color32, Vec2};

pub const TEXT: Color32 = Color32::from_gray(232);
pub const MUTED: Color32 = Color32::from_gray(180);
pub const AMBER: Color32 = Color32::from_rgb(217, 164, 65);
pub const CANVAS: Color32 = Color32::from_gray(20);
pub const PANEL: Color32 = Color32::from_gray(28);
pub const SURFACE: Color32 = Color32::from_gray(38);
pub const LINE: Color32 = Color32::from_gray(56);
pub const SELECTED: Color32 = Color32::from_gray(52);
pub const SELECTION_EDGE: Color32 = Color32::from_gray(184);

pub fn apply(ctx: &egui::Context) {
    // Cmd +/-/0 belong to the photograph; UI scale has its own control.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = CANVAS;
    visuals.faint_bg_color = SURFACE;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = egui::Stroke::new(1., TEXT);
    visuals.widgets.inactive.bg_fill = Color32::from_gray(42);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., Color32::from_gray(124));
    visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(156);
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.hovered.bg_fill = Color32::from_gray(56);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(48);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., MUTED);
    visuals.widgets.hovered.fg_stroke.color = Color32::from_gray(190);
    visuals.widgets.active.bg_fill = Color32::from_gray(66);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1., SELECTION_EDGE);
    visuals.widgets.active.fg_stroke.color = TEXT;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1., LINE);
    visuals.window_corner_radius = 10.into();
    visuals.menu_corner_radius = 8.into();
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.corner_radius = 5.into();
    }
    ctx.set_visuals(visuals);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = Vec2::new(8., 8.);
        s.spacing.button_padding = Vec2::new(10., 6.);
        s.spacing.interact_size = Vec2::new(28., 28.);
        s.spacing.window_margin = egui::Margin::same(20);
        s.spacing.slider_width = 140.;
        s.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 6.,
            floating_width: 3.,
            floating_allocated_width: 4.,
            foreground_color: true,
            dormant_handle_opacity: 0.7,
            active_handle_opacity: 0.8,
            interact_handle_opacity: 0.85,
            ..egui::style::ScrollStyle::thin()
        };
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.));
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(13.));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
        s.text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.));
        s.text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.));
    });
}

/// Tabs share the same neutral selection treatment throughout the chrome.
pub fn tab_button(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    ui.add(
        egui::Button::selectable(selected, label)
            .frame_when_inactive(selected)
            .min_size(egui::vec2(0., 28.)),
    )
}

#[derive(Clone, Copy)]
pub enum Icon {
    Back,
    Forward,
    Up,
    Refresh,
}

/// Native button semantics with a consistent 20-point vector glyph.
pub fn icon_button(ui: &mut egui::Ui, enabled: bool, icon: Icon, label: &str) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new(())
                .frame_when_inactive(false)
                .min_size(egui::vec2(28., 28.)),
        )
        .on_hover_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    if ui.is_rect_visible(response.rect) {
        let color = if !response.enabled() {
            Color32::from_gray(92)
        } else if response.hovered() || response.has_focus() {
            TEXT
        } else {
            MUTED
        };
        let stroke = egui::Stroke::new(1.5, color);
        let center = response.rect.center();
        let point = |x: f32, y: f32| center + egui::vec2(x, y);
        let painter = ui.painter();
        match icon {
            Icon::Back | Icon::Forward => {
                let direction = if matches!(icon, Icon::Back) { -1. } else { 1. };
                painter.add(egui::Shape::line(
                    vec![
                        point(-direction * 2., -5.),
                        point(direction * 3., 0.),
                        point(-direction * 2., 5.),
                    ],
                    stroke,
                ));
            }
            Icon::Up => {
                painter.line_segment([point(0., 6.), point(0., -6.)], stroke);
                painter.add(egui::Shape::line(
                    vec![point(-5., -1.), point(0., -6.), point(5., -1.)],
                    stroke,
                ));
            }
            Icon::Refresh => {
                let points = (0..=24)
                    .map(|step| {
                        let angle = -0.4 + step as f32 / 24. * 5.25;
                        point(angle.cos() * 6., angle.sin() * 6.)
                    })
                    .collect();
                painter.add(egui::Shape::line(points, stroke));
                painter.add(egui::Shape::line(
                    vec![point(-3., -3.), point(0.8, -5.9), point(-2., -9.)],
                    stroke,
                ));
            }
        }
    }
    response
}

pub fn panel() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(16)
}
