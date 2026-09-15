//! Shared chrome styling. Image pixels and the renderer's neutral surround are independent.
use eframe::egui::{self, Color32, Vec2};

pub const TEXT: Color32 = Color32::from_gray(232);
pub const MUTED: Color32 = Color32::from_gray(170);
pub const AMBER: Color32 = Color32::from_rgb(217, 164, 65);
pub const CANVAS: Color32 = Color32::from_gray(20);
pub const PANEL: Color32 = Color32::from_gray(28);
pub const SURFACE: Color32 = Color32::from_gray(38);
pub const LINE: Color32 = Color32::from_gray(56);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = CANVAS;
    visuals.faint_bg_color = SURFACE;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = Color32::from_gray(62);
    visuals.selection.stroke = egui::Stroke::new(1.5, TEXT);
    visuals.widgets.inactive.bg_fill = Color32::from_gray(42);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., Color32::from_gray(124));
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.hovered.bg_fill = Color32::from_gray(56);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(48);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., MUTED);
    visuals.widgets.active.bg_fill = Color32::from_gray(66);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, TEXT);
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

pub fn panel() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(16)
}
