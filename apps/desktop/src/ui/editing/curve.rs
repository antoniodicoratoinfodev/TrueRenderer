use super::{CurvePoint, adjustment_default};
use crate::i18n::Language;
use eframe::egui;

const GAP: f32 = 1e-6;
const IDENTITY: [CurvePoint; 2] = [CurvePoint { x: 0., y: 0. }, CurvePoint { x: 1., y: 1. }];

#[derive(Clone, Default)]
struct Selection {
    point: Option<usize>,
}

fn points(curve: &[CurvePoint]) -> &[CurvePoint] {
    if curve.is_empty() { &IDENTITY } else { curve }
}

fn insert(curve: &mut Vec<CurvePoint>, p: CurvePoint) -> Option<usize> {
    let current = points(curve);
    if current.len() >= 32 {
        return None;
    }
    let i = current
        .windows(2)
        .position(|w| p.x >= w[0].x + GAP && p.x <= w[1].x - GAP)?
        + 1;
    let y = p.y.clamp(current[i - 1].y, current[i].y);
    if curve.is_empty() {
        curve.extend(IDENTITY);
    }
    curve.insert(i, CurvePoint { x: p.x, y });
    Some(i)
}

fn bounds(curve: &[CurvePoint], i: usize) -> (CurvePoint, CurvePoint) {
    let current = points(curve);
    let low = if i == 0 {
        CurvePoint { x: 0., y: 0. }
    } else {
        CurvePoint {
            x: (current[i - 1].x + GAP).min(current[i].x),
            y: current[i - 1].y,
        }
    };
    let high = if i + 1 == current.len() {
        CurvePoint { x: 1., y: 1. }
    } else {
        CurvePoint {
            x: (current[i + 1].x - GAP).max(current[i].x),
            y: current[i + 1].y,
        }
    };
    (low, high)
}

fn move_point(curve: &mut Vec<CurvePoint>, i: usize, p: CurvePoint) {
    if i >= points(curve).len() {
        return;
    }
    let (low, high) = bounds(curve, i);
    let next = CurvePoint {
        x: p.x.clamp(low.x, high.x),
        y: p.y.clamp(low.y, high.y),
    };
    if next != points(curve)[i] {
        if curve.is_empty() {
            curve.extend(IDENTITY);
        }
        curve[i] = next;
    }
}

pub(super) fn midtone(
    curve: &mut Vec<CurvePoint>,
    ui: &mut egui::Ui,
    lang: Language,
) -> (bool, bool) {
    let before = curve.clone();
    let current = points(curve);
    let pair = current.windows(2).find(|w| w[0].x <= 0.5 && w[1].x >= 0.5);
    let existing = current.iter().position(|p| (p.x - 0.5).abs() < GAP);
    let (low, high) = if let Some(i) = existing {
        let (low, high) = bounds(current, i);
        (low.y, high.y)
    } else {
        pair.map_or((0., 1.), |p| (p[0].y, p[1].y))
    };
    let mut mid = tr_core::editing::tone_curve_value(0.5, current);
    let r = ui
        .add_enabled_ui(
            existing.is_some() || (pair.is_some() && current.len() < 32),
            |ui| {
                adjustment_default(
                    ui,
                    lang.text("Mezzitoni curva"),
                    &mut mid,
                    low..=high,
                    "",
                    None,
                    0.5,
                )
                .0
            },
        )
        .inner;
    if r.changed() {
        if let Some(i) = existing {
            curve[i].y = mid;
        } else {
            insert(curve, CurvePoint { x: 0.5, y: mid });
        }
    }
    (
        *curve != before,
        r.drag_stopped() || (r.changed() && !r.dragged()),
    )
}

fn position(rect: egui::Rect, p: CurvePoint) -> egui::Pos2 {
    egui::pos2(
        rect.left() + rect.width() * p.x,
        rect.bottom() - rect.height() * p.y,
    )
}

fn coordinates(rect: egui::Rect, p: egui::Pos2) -> CurvePoint {
    CurvePoint {
        x: ((p.x - rect.left()) / rect.width()).clamp(0., 1.),
        y: ((rect.bottom() - p.y) / rect.height()).clamp(0., 1.),
    }
}

fn graph(
    ui: &mut egui::Ui,
    curve: &mut Vec<CurvePoint>,
    selection: &mut Selection,
    color: egui::Color32,
) -> (egui::Rect, bool) {
    let side = ui.available_width().clamp(100., 320.);
    let graph_id = ui.make_persistent_id("curve-graph");
    let had_focus = ui.memory(|m| m.has_focus(graph_id));
    let (_, outer) = ui.allocate_space(egui::vec2(side, side));
    let response = ui.interact(outer, graph_id, egui::Sense::click_and_drag());
    // Saving briefly disables the controls. Keep ownership of the keyboard,
    // without editing while disabled or letting arrows navigate photographs.
    if had_focus && !ui.is_enabled() {
        response.request_focus();
    }
    let rect = outer.shrink(10.);
    let mut commit = false;
    if response.drag_started() || response.clicked() {
        response.request_focus();
        // Start at the press location: fast drags must grab the intended anchor.
        let origin = if response.drag_started() {
            ui.input(|i| i.pointer.press_origin())
        } else {
            response.interact_pointer_pos()
        };
        if let Some(pos) = origin {
            selection.point = points(curve)
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let d = position(rect, *p).distance(pos);
                    (d <= 10.).then_some((i, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
                .or_else(|| {
                    rect.contains(pos)
                        .then(|| insert(curve, coordinates(rect, pos)))
                        .flatten()
                });
        }
        commit |= response.clicked();
    }
    if (response.dragged() || response.drag_stopped())
        && let (Some(i), Some(pos)) = (selection.point, response.interact_pointer_pos())
    {
        move_point(curve, i, coordinates(rect, pos));
    }
    commit |= response.drag_stopped();
    if ui.is_enabled()
        && response.has_focus()
        && let Some(i) = selection.point.filter(|i| *i < points(curve).len())
    {
        let (delete, delta) = ui.input(|input| {
            let step = if input.modifiers.shift { 0.01 } else { 0.001 };
            (
                input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace),
                egui::vec2(
                    (i32::from(input.key_pressed(egui::Key::ArrowRight))
                        - i32::from(input.key_pressed(egui::Key::ArrowLeft)))
                        as f32
                        * step,
                    (i32::from(input.key_pressed(egui::Key::ArrowUp))
                        - i32::from(input.key_pressed(egui::Key::ArrowDown)))
                        as f32
                        * step,
                ),
            )
        });
        if delete && i > 0 && i + 1 < curve.len() {
            curve.remove(i);
            selection.point = None;
            commit = true;
        } else if delta != egui::Vec2::ZERO {
            let p = points(curve)[i];
            move_point(
                curve,
                i,
                CurvePoint {
                    x: p.x + delta.x,
                    y: p.y + delta.y,
                },
            );
            commit = true;
        }
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let painter = ui.painter();
    painter.rect_filled(outer, 4, egui::Color32::from_gray(22));
    for n in 0..=4 {
        let t = n as f32 / 4.;
        let stroke = egui::Stroke::new(1., egui::Color32::from_gray(48));
        painter.line_segment(
            [
                position(rect, CurvePoint { x: t, y: 0. }),
                position(rect, CurvePoint { x: t, y: 1. }),
            ],
            stroke,
        );
        painter.line_segment(
            [
                position(rect, CurvePoint { x: 0., y: t }),
                position(rect, CurvePoint { x: 1., y: t }),
            ],
            stroke,
        );
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(1., egui::Color32::from_gray(78)),
    );
    let mut line = vec![position(
        rect,
        CurvePoint {
            x: 0.,
            y: tr_core::editing::tone_curve_value(0., curve),
        },
    )];
    line.extend(points(curve).iter().map(|p| position(rect, *p)));
    line.push(position(
        rect,
        CurvePoint {
            x: 1.,
            y: tr_core::editing::tone_curve_value(1., curve),
        },
    ));
    painter.add(egui::Shape::line(
        line,
        egui::Stroke::new(
            2.,
            if ui.is_enabled() {
                color
            } else {
                egui::Color32::GRAY
            },
        ),
    ));
    for (i, p) in points(curve).iter().enumerate() {
        let selected = selection.point == Some(i);
        painter.circle_filled(
            position(rect, *p),
            if selected { 5.5 } else { 4. },
            if selected {
                ui.visuals().selection.bg_fill
            } else {
                egui::Color32::WHITE
            },
        );
        painter.circle_stroke(
            position(rect, *p),
            6.5,
            egui::Stroke::new(
                1.,
                if selected {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_gray(22)
                },
            ),
        );
    }
    (rect, commit)
}

pub(super) fn controls(
    ui: &mut egui::Ui,
    lang: Language,
    curve: &mut Vec<CurvePoint>,
) -> (bool, bool) {
    controls_with_label(ui, lang, curve, "Curva a punti · luminanza lineare")
}

pub(super) fn controls_with_label(
    ui: &mut egui::Ui,
    lang: Language,
    curve: &mut Vec<CurvePoint>,
    label: &str,
) -> (bool, bool) {
    controls_with_color(ui, lang, curve, label, egui::Color32::LIGHT_GRAY)
}

pub(super) fn controls_with_color(
    ui: &mut egui::Ui,
    lang: Language,
    curve: &mut Vec<CurvePoint>,
    label: &str,
    color: egui::Color32,
) -> (bool, bool) {
    let before = curve.clone();
    let id = ui.make_persistent_id("curve-selection");
    let mut selection = ui
        .data_mut(|d| d.get_temp::<Selection>(id))
        .unwrap_or_default();
    selection.point = selection.point.filter(|i| *i < points(curve).len());
    ui.label(lang.text(label));
    let (_, mut commit) = graph(ui, curve, &mut selection, color);
    ui.small(
        lang.text("Clic per aggiungere, trascina per modificare. I punti vicini restano ancorati."),
    );
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                points(curve).len() < 32,
                egui::Button::new(lang.text("Aggiungi punto")),
            )
            .clicked()
        {
            let w = points(curve)
                .windows(2)
                .max_by(|a, b| (a[1].x - a[0].x).total_cmp(&(b[1].x - b[0].x)))
                .unwrap();
            let p = CurvePoint {
                x: (w[0].x + w[1].x) * 0.5,
                y: (w[0].y + w[1].y) * 0.5,
            };
            selection.point = insert(curve, p);
            commit = true;
        }
        let removable = selection
            .point
            .is_some_and(|i| i > 0 && i + 1 < curve.len());
        if ui
            .add_enabled(removable, egui::Button::new(lang.text("Elimina punto")))
            .clicked()
        {
            curve.remove(selection.point.unwrap());
            selection.point = None;
            commit = true;
        }
    });
    if let Some(i) = selection.point {
        ui.label(format!("{} {}", lang.text("Punto"), i));
        let (low, high) = bounds(curve, i);
        let mut p = points(curve)[i];
        let default_x = if i == 0 {
            0.
        } else if i + 1 == points(curve).len() {
            1.
        } else {
            p.y
        };
        let default_y = if i == 0 {
            0.
        } else if i + 1 == points(curve).len() {
            1.
        } else {
            p.x
        };
        let input = adjustment_default(
            ui,
            lang.text("Ingresso"),
            &mut p.x,
            low.x..=high.x,
            "",
            None,
            default_x,
        )
        .0;
        let output = adjustment_default(
            ui,
            lang.text("Uscita"),
            &mut p.y,
            low.y..=high.y,
            "",
            None,
            default_y,
        )
        .0;
        if input.changed() || output.changed() {
            move_point(curve, i, p);
        }
        for r in [input, output] {
            commit |= r.drag_stopped() || (r.changed() && !r.dragged());
        }
        if i == 0 || i + 1 == points(curve).len() {
            ui.small(lang.text("Estremo modificabile · non eliminabile"));
        }
        ui.small(lang.text("Frecce: regolazione fine · Maiusc: passo maggiore · Canc: elimina"));
    } else {
        ui.small(lang.text("Seleziona un punto per regolare Ingresso e Uscita."));
    }
    ui.data_mut(|d| d.insert_temp(id, selection));
    (*curve != before, commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::editing::EditRecipe;

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn both_endpoints_move_from_empty_and_midtones_handle_a_restricted_domain() {
        let mut c = vec![];
        move_point(&mut c, 0, CurvePoint { x: 0.6, y: 0.2 });
        assert_eq!(c, vec![CurvePoint { x: 0.6, y: 0.2 }, IDENTITY[1]]);
        move_point(&mut c, 1, CurvePoint { x: 0.9, y: 0.8 });
        let ctx = egui::Context::default();
        let before = c.clone();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(midtone(&mut c, ui, Language::English), (false, false));
        });
        output.textures_delta.clear();
        assert_eq!(c, before);
        move_point(&mut c, 0, CurvePoint { x: 1., y: 1. });
        assert!(c[0].x < c[1].x);
        assert_eq!(c[0].y, c[1].y);
        EditRecipe {
            curve: c,
            process_version: 4,
            ..EditRecipe::neutral(Default::default())
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn curve_graph_add_drag_anchor_delete_and_commit_on_release() {
        for width in [200., 248., 328.] {
            let ctx = egui::Context::default();
            let mut curve = vec![];
            let mut selection = Selection::default();
            let mut commits = 0;
            let mut frame = |events| {
                let mut rect = egui::Rect::NOTHING;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 500.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let (r, c) =
                            graph(ui, &mut curve, &mut selection, egui::Color32::LIGHT_GRAY);
                        rect = r;
                        commits += usize::from(c);
                    },
                );
                output.textures_delta.clear();
                let recipe = EditRecipe {
                    curve: curve.clone(),
                    process_version: 4,
                    ..EditRecipe::neutral(Default::default())
                };
                recipe.validate().unwrap();
                (rect, curve.clone(), commits)
            };
            let (rect, _, _) = frame(vec![]);
            assert!(rect.left() >= 0. && rect.right() <= width);
            let a = position(rect, CurvePoint { x: 0.25, y: 0.25 });
            frame(vec![egui::Event::PointerMoved(a), button(a, true)]);
            let (_, c, _) = frame(vec![button(a, false)]);
            assert_eq!(c.len(), 3);
            let b = position(rect, CurvePoint { x: 0.75, y: 0.75 });
            frame(vec![egui::Event::PointerMoved(b), button(b, true)]);
            let (_, c, before) = frame(vec![button(b, false)]);
            let anchor = c[1];
            frame(vec![egui::Event::PointerMoved(b), button(b, true)]);
            let end = position(rect, CurvePoint { x: 0.6, y: 0.9 });
            let (_, c, during) = frame(vec![egui::Event::PointerMoved(end)]);
            assert_eq!(during, before);
            assert_eq!(c[1], anchor);
            assert!((c[2].x - 0.6).abs() < 1e-5 && (c[2].y - 0.9).abs() < 1e-5);
            // The final pointer position can arrive with release, after the last move frame.
            let released = position(rect, CurvePoint { x: 0.65, y: 0.95 });
            let (_, c, after) = frame(vec![button(released, false)]);
            assert_eq!(after, before + 1);
            assert!((c[2].x - 0.65).abs() < 1e-5 && (c[2].y - 0.95).abs() < 1e-5);
            assert_eq!(
                serde_json::from_str::<EditRecipe>(
                    &serde_json::to_string(&EditRecipe {
                        curve: c.clone(),
                        ..EditRecipe::neutral(Default::default())
                    })
                    .unwrap()
                )
                .unwrap()
                .curve,
                c
            );
            let (_, c, _) = frame(vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(c.len(), 3);
            assert_eq!(c[1], anchor);
            // The white endpoint moves without changing its neighbor, and cannot be deleted.
            let end = rect.right_top();
            frame(vec![egui::Event::PointerMoved(end), button(end, true)]);
            frame(vec![egui::Event::PointerMoved(rect.center())]);
            let (_, c, _) = frame(vec![button(rect.center(), false)]);
            assert_eq!(c[1], anchor);
            assert_eq!(*c.last().unwrap(), CurvePoint { x: 0.5, y: 0.5 });
            let (_, c, _) = frame(vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(c.len(), 3);
        }
    }

    #[test]
    fn curve_drag_from_empty_graph_and_outside_preserves_valid_recipe() {
        let ctx = egui::Context::default();
        let mut curve = vec![];
        let mut selection = Selection::default();
        let mut rect = egui::Rect::NOTHING;
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    rect = graph(ui, &mut curve, &mut selection, egui::Color32::LIGHT_GRAY).0;
                },
            );
            output.textures_delta.clear();
            (rect, curve.clone())
        };
        let (r, _) = frame(vec![]);
        let start = r.center();
        frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
        let end = r.right_top() + egui::vec2(100., -100.);
        let (_, c) = frame(vec![egui::Event::PointerMoved(end)]);
        assert_eq!(c.len(), 3);
        assert!(c[1].x < 1. && c[1].y == 1.);
        EditRecipe {
            curve: c,
            ..EditRecipe::neutral(Default::default())
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn curve_focus_survives_saving_controls_changing_before_the_graph() {
        let ctx = egui::Context::default();
        let mut curve = Vec::new();
        let mut selection = Selection::default();
        let mut frame = |events, saving, enabled| {
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    if saving {
                        ui.label("Saving…");
                    }
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.push_id("asset-curve", |ui| {
                            rect =
                                graph(ui, &mut curve, &mut selection, egui::Color32::LIGHT_GRAY).0;
                        });
                    });
                },
            );
            output.textures_delta.clear();
            (rect, points(&curve).to_vec())
        };
        let (rect, _) = frame(vec![], false, true);
        let black = rect.left_bottom();
        frame(
            vec![egui::Event::PointerMoved(black), button(black, true)],
            false,
            true,
        );
        frame(vec![button(black, false)], false, true);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![key(egui::Key::ArrowUp)], false, true);
        let (_, disabled) = frame(vec![key(egui::Key::ArrowRight)], true, false);
        assert_eq!(disabled[0], CurvePoint { x: 0., y: 0.001 });
        let (_, curve) = frame(vec![key(egui::Key::ArrowRight)], true, true);
        assert_eq!(curve[0], CurvePoint { x: 0.001, y: 0.001 });
    }

    #[test]
    fn curve_limits_and_neighbor_anchors() {
        let mut c = IDENTITY.to_vec();
        insert(&mut c, CurvePoint { x: 0.25, y: 0.2 });
        insert(&mut c, CurvePoint { x: 0.75, y: 0.8 });
        let anchor = c[2];
        move_point(&mut c, 1, CurvePoint { x: 1., y: 1. });
        assert_eq!(c[2], anchor);
        assert!(c[1].x < c[2].x);
        assert_eq!(c[1].y, c[2].y);
        assert!(insert(&mut c, CurvePoint { x: 0., y: 0.5 }).is_none());
        let mut full: Vec<_> = (0..32)
            .map(|i| {
                let x = i as f32 / 31.;
                CurvePoint { x, y: x }
            })
            .collect();
        assert!(insert(&mut full, CurvePoint { x: 0.51, y: 0.5 }).is_none());
        EditRecipe {
            curve: full,
            ..EditRecipe::neutral(Default::default())
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn curve_midtone_slider_keeps_custom_anchors_in_both_languages() {
        for lang in [Language::Italian, Language::English] {
            let ctx = egui::Context::default();
            let mut c = vec![
                IDENTITY[0],
                CurvePoint { x: 0.2, y: 0.15 },
                CurvePoint { x: 0.8, y: 0.85 },
                IDENTITY[1],
            ];
            let anchors = c.clone();
            let mut frame = |events| {
                let mut rect = egui::Rect::NOTHING;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(248., 200.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        rect = ui
                            .scope(|ui| {
                                midtone(&mut c, ui, lang);
                            })
                            .response
                            .rect;
                    },
                );
                output.textures_delta.clear();
                (rect, c.clone())
            };
            let (r, _) = frame(vec![]);
            let start = egui::pos2(r.center().x, r.bottom() - 10.);
            let end = start + egui::vec2(45., 0.);
            frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
            frame(vec![egui::Event::PointerMoved(end)]);
            let (_, c) = frame(vec![button(end, false)]);
            assert_eq!(c.len(), 5);
            assert_eq!(c[1], anchors[1]);
            assert_eq!(c[3], anchors[2]);
            assert!(c[2].y > 0.5);
            EditRecipe {
                curve: c,
                ..EditRecipe::neutral(Default::default())
            }
            .validate()
            .unwrap();
        }
    }
}
