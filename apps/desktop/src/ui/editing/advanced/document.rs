use super::*;
use crate::ui::document::CanvasCoordinates;

impl Controls {
    pub fn document_canvas(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &CanvasCoordinates<'_>,
        recipe: &mut EditRecipe,
    ) -> bool {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.paint = false;
            self.horizon = false;
            self.overlay = false;
            self.horizon_start = None;
            return false;
        }
        if map.layer.locked {
            return false;
        }
        if self.horizon && !recipe.suspended.contains_key("geometry") {
            if response.drag_started()
                && let Some(pos) = ui.input(|i| i.pointer.press_origin())
                && let Some(p) = map.output_point(pos)
            {
                self.horizon_start = Some((String::new(), p, pos));
            }
            if let Some((_, _, start)) = &self.horizon_start
                && let Some(end) = response.interact_pointer_pos()
            {
                ui.painter().line_segment(
                    [*start, end],
                    egui::Stroke::new(2., egui::Color32::LIGHT_BLUE),
                );
            }
            if response.drag_stopped() {
                if let Some((_, start, _)) = self.horizon_start.take()
                    && let Some(p) = response
                        .interact_pointer_pos()
                        .and_then(|p| map.output_point(p))
                {
                    let dx = p[0] - start[0];
                    let dy = p[1] - start[1];
                    if dx.hypot(dy) > 4. {
                        let correction = (dy.atan2(dx).to_degrees() + 45.).rem_euclid(90.) - 45.;
                        let a = recipe.advanced.get_or_insert_with(Default::default);
                        a.geometry.angle = (a.geometry.angle - correction).clamp(-45., 45.);
                        recipe.require_process(3);
                    }
                }
                self.horizon = false;
            }
            return true;
        }
        if recipe.suspended.contains_key("local-masks") {
            return false;
        }
        let Some(a) = recipe.advanced.as_mut() else {
            return false;
        };
        let count = a.masks.iter().map(|m| m.points.len()).sum::<usize>()
            + recipe.layers.as_ref().map_or(0, |s| s.point_count());
        let Some(mask) = a.masks.get_mut(self.mask) else {
            return false;
        };
        if self.overlay {
            super::super::mask_overlay::paint(
                ui,
                response.rect,
                response.rect,
                [230, 65, 75],
                65,
                |uv| {
                    let pos = response.rect.min
                        + egui::vec2(
                            uv[0] as f32 * response.rect.width(),
                            uv[1] as f32 * response.rect.height(),
                        );
                    let Some(xy) = map.source_uv(pos) else {
                        return 0.;
                    };
                    let p = map.layer.sample([
                        xy[0] * map.layer.width as f32,
                        xy[1] * map.layer.height as f32,
                    ]);
                    let rgb = if p[3] > 0.01 {
                        std::array::from_fn(|c| p[c] / p[3])
                    } else {
                        [0.; 3]
                    };
                    mask.weight(xy, rgb, map.aspect())
                },
            );
        }
        if !self.paint || !matches!(mask.shape, Shape::Brush | Shape::Linear | Shape::Radial) {
            return false;
        }
        let origin = (response.drag_started() || response.clicked())
            .then(|| {
                ui.input(|i| i.pointer.press_origin())
                    .or(response.interact_pointer_pos())
            })
            .flatten()
            .and_then(|p| map.source_uv(p));
        let point = response
            .interact_pointer_pos()
            .and_then(|p| map.source_uv(p));
        if origin.is_some() {
            self.new_stroke = true;
        }
        if response.dragged() || response.drag_stopped() || response.clicked() {
            let mut remaining = 512_usize.saturating_sub(count);
            for p in [origin.filter(|_| mask.shape == Shape::Brush), point]
                .into_iter()
                .flatten()
            {
                if mask.shape == Shape::Brush {
                    if remaining == 0 {
                        break;
                    }
                    if self.new_stroke
                        || mask
                            .points
                            .last()
                            .is_none_or(|q| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() > 0.001)
                    {
                        if self.new_stroke && !mask.points.is_empty() {
                            mask.breaks.push(mask.points.len());
                        }
                        mask.points.push(p);
                        self.new_stroke = false;
                        remaining -= 1;
                    }
                } else {
                    mask.center = p;
                }
            }
        }
        true
    }
}
