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
    #[cfg(target_os = "macos")]
    {
        // Use fonts supplied by macOS; no system font is bundled or redistributed.
        static FONTS: std::sync::OnceLock<egui::FontDefinitions> = std::sync::OnceLock::new();
        ctx.set_fonts(
            FONTS
                .get_or_init(|| {
                    let mut fonts = egui::FontDefinitions::default();
                    for (name, path, family) in [
                        (
                            "system-ui",
                            "/System/Library/Fonts/SFNS.ttf",
                            egui::FontFamily::Proportional,
                        ),
                        (
                            "system-mono",
                            "/System/Library/Fonts/SFNSMono.ttf",
                            egui::FontFamily::Monospace,
                        ),
                    ] {
                        if let Ok(bytes) = std::fs::read(path) {
                            fonts
                                .font_data
                                .insert(name.into(), egui::FontData::from_owned(bytes).into());
                            fonts
                                .families
                                .entry(family)
                                .or_default()
                                .insert(0, name.into());
                        }
                    }
                    fonts
                })
                .clone(),
        );
    }
    // Cmd +/-/0 belong to the photograph; UI scale has its own control.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    // Chrome labels are controls and descriptions, not selectable document text.
    // TextEdit and numeric entry keep their own editing/selection behavior.
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| s.interaction.selectable_labels = false);
    }
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
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., Color32::from_gray(66));
    visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(156);
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
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
        s.spacing.item_spacing = Vec2::new(8., 6.);
        s.spacing.button_padding = Vec2::new(10., 6.);
        s.spacing.interact_size = Vec2::new(28., 28.);
        s.spacing.window_margin = egui::Margin::same(16);
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
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(17.));
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

/// Equal-width, single-line tabs with visible keyboard focus.
pub fn segment(ui: &mut egui::Ui, selected: bool, label: &str, width: f32) -> egui::Response {
    let response = ui.add_sized(
        [width.max(0.), 28.],
        egui::Button::selectable(selected, label)
            .truncate()
            .stroke(egui::Stroke::NONE)
            .corner_radius(4)
            .frame_when_inactive(selected)
            .min_size(egui::vec2(0., 28.)),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            4,
            egui::Stroke::new(1., SELECTION_EDGE),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(label)
}

/// Matching top-level actions, whether they open a menu or a window.
pub fn toolbar_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(label)
        .min_size(egui::vec2(0., 28.))
        .corner_radius(5)
}

/// Quiet chrome surfaces; hover, focus and active feedback stay native to egui.
pub fn toolbar_controls(ui: &mut egui::Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = SURFACE;
    widgets.inactive.bg_stroke = egui::Stroke::new(1., LINE);
}

#[derive(Clone, Copy)]
pub enum Icon {
    Back,
    Forward,
    Up,
    Refresh,
    SidePanel { open: bool, left: bool },
    Filmstrip { open: bool },
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
            Icon::SidePanel { open, left } => {
                let direction = if left { -1. } else { 1. };
                let rect = egui::Rect::from_min_max(point(-9., -7.), point(9., 7.));
                if open {
                    painter.rect_filled(
                        egui::Rect::from_two_pos(
                            point(direction * 3., -7.),
                            point(direction * 9., 7.),
                        ),
                        1,
                        color.gamma_multiply(0.35),
                    );
                }
                painter.rect_stroke(rect, 2, stroke, egui::StrokeKind::Inside);
                painter.line_segment(
                    [point(direction * 3., -7.), point(direction * 3., 7.)],
                    stroke,
                );
            }
            Icon::Filmstrip { open } => {
                let rect = egui::Rect::from_min_max(point(-9., -7.), point(9., 7.));
                if open {
                    painter.rect_filled(
                        egui::Rect::from_min_max(point(-9., 2.), point(9., 7.)),
                        1,
                        color.gamma_multiply(0.35),
                    );
                }
                painter.rect_stroke(rect, 2, stroke, egui::StrokeKind::Inside);
                painter.line_segment([point(-9., 2.), point(9., 2.)], stroke);
                for x in [-3., 3.] {
                    painter.line_segment([point(x, 2.), point(x, 7.)], stroke);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_text_keeps_the_arrow_and_cannot_be_selected_but_fields_still_edit() {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let ctx = egui::Context::default();
            apply(&ctx);
            ctx.set_theme(theme);
            let mut value = String::from("Layer name");
            let mut label_rect = egui::Rect::NOTHING;
            let mut field_rect = egui::Rect::NOTHING;
            let mut run = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400., 250.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        label_rect = ui.label("Layer properties and adjustments").rect;
                        field_rect = ui.text_edit_singleline(&mut value).rect;
                    },
                );
                output.textures_delta.clear();
                (output.platform_output, label_rect, field_rect)
            };
            let (_, label, field) = run(vec![]);
            let start = label.left_center() + egui::vec2(2., 0.);
            let end = label.right_center() - egui::vec2(2., 0.);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [
                vec![egui::Event::PointerMoved(start)],
                vec![button(start, true)],
                vec![egui::Event::PointerMoved(end)],
                vec![button(end, false)],
                vec![egui::Event::Copy],
            ] {
                let (output, _, _) = run(events);
                assert_eq!(output.cursor_icon, egui::CursorIcon::Default);
                assert!(
                    !output
                        .commands
                        .iter()
                        .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
                );
                assert!(
                    !ctx.plugin::<egui::text_selection::LabelSelectionState>()
                        .lock()
                        .has_selection()
                );
            }
            let point = field.center();
            run(vec![egui::Event::PointerMoved(point)]);
            let (output, _, _) = run(vec![button(point, true), button(point, false)]);
            assert_eq!(output.cursor_icon, egui::CursorIcon::Text);
            run(vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Copy,
            ]);
            let (output, _, _) = run(vec![egui::Event::Copy]);
            assert!(output.commands.iter().any(|command| matches!(command,
                egui::OutputCommand::CopyText(text) if text == "Layer name")));
            run(vec![egui::Event::Text("Subject".into())]);
            assert_eq!(value, "Subject");
        }
    }
}
