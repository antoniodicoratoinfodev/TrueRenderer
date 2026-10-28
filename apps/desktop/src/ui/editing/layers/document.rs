//! Adapt the photographic controls to a raster layer's canvas coordinates.
use super::*;
use crate::ui::document::CanvasCoordinates;
use sha2::{Digest, Sha256};
use tr_core::{color::LinearImage, provider::ImageLevels};

impl Controls {
    pub fn invalidate_document_input(&mut self) {
        self.document_input = None;
    }
    pub fn stop_document_gesture(&mut self) {
        self.cancel_analysis();
        self.paint = false;
        self.pick = false;
        self.overlay = false;
        self.anchor = None;
        self.gesture_start = None;
    }
    pub fn document_canvas(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &CanvasCoordinates<'_>,
        recipe: &mut EditRecipe,
    ) -> bool {
        if !self.view_layers {
            return false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stop_document_gesture();
            return true;
        }
        self.refresh_analysis(recipe);
        let mut input_recipe = recipe.clone();
        self.preview_input(&self.photo, &mut input_recipe);
        if self.analysis_operator.is_some() || self.pick || self.overlay {
            let key = serde_json::to_string(&input_recipe).expect("validated recipe");
            if self.document_input.as_ref().is_none_or(|(k, _)| *k != key) {
                let mut hash = Sha256::new();
                hash.update(map.layer.width.to_le_bytes());
                hash.update(map.layer.height.to_le_bytes());
                for p in &map.layer.pixels {
                    for v in p {
                        hash.update(v.to_le_bytes());
                    }
                }
                for v in &map.layer.mask {
                    hash.update(v.to_le_bytes());
                }
                self.analysis_source = format!("{:x}", hash.finalize());
                let result = (|| -> anyhow::Result<ImageLevels> {
                    let l = map.layer;
                    let mut image = LinearImage::new(l.width, l.height, l.pixels.clone())?;
                    for (p, m) in image.pixels.iter_mut().zip(&l.mask) {
                        for v in p {
                            *v *= m;
                        }
                    }
                    input_recipe.apply(&mut image)?;
                    let size = [image.width, image.height];
                    ImageLevels::from_reference_mip(image, size, 0, false)
                })();
                match result {
                    Ok(image) => self.document_input = Some((key, image)),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
        // Move the cache out temporarily so its image can be shared with the
        // existing analysis methods without aliasing the controls.
        let input = self.document_input.take();
        if let Some((_, image)) = &input {
            self.prepare_analysis(recipe, image);
        }
        self.apply_auto_request(recipe);
        let active = self.gesture_active(&self.photo, recipe);
        if self.pick && response.clicked() && !map.layer.locked {
            let result = (|| -> anyhow::Result<()> {
                let image = &input
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Ingresso non disponibile"))?
                    .1;
                let point = response
                    .interact_pointer_pos()
                    .and_then(|p| map.output_point(p))
                    .ok_or_else(|| anyhow::anyhow!("Campionare all'interno del livello"))?;
                let raster = image.source();
                let xy = [point[0].floor() as u32, point[1].floor() as u32];
                anyhow::ensure!(
                    xy[0] < raster.width && xy[1] < raster.height,
                    "Campione fuori livello"
                );
                let p = raster.pixels[xy[1] as usize * raster.width as usize + xy[0] as usize];
                anyhow::ensure!(p[3] > 0.01, "Campione trasparente");
                let rgb = std::array::from_fn(|c| p[c] / p[3]);
                if self.level_pick.is_some() {
                    return self.accept_level_sample(recipe, image, rgb, xy);
                }
                let layer = recipe
                    .layers
                    .as_mut()
                    .and_then(|s| {
                        s.layers
                            .iter_mut()
                            .find(|l| Some(l.id) == self.selected && !l.locked)
                    })
                    .ok_or_else(|| anyhow::anyhow!("Regolazione non modificabile"))?;
                if self.pick_component {
                    if let Some(MaskKind::Color(c)) = layer
                        .mask
                        .nodes
                        .iter_mut()
                        .find(|n| Some(n.id) == self.component)
                        .map(|n| &mut n.kind)
                    {
                        c.reference = rgb;
                    }
                } else if let Some(Operator::SampleColor(s)) =
                    layer.operators.get_mut(self.sample_operator)
                {
                    s.range.reference = rgb;
                }
                self.pick = false;
                Ok(())
            })();
            match result {
                Ok(()) => self.error = None,
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        if self.overlay
            && let Some(layer) = recipe
                .layers
                .as_ref()
                .and_then(|s| s.layers.iter().find(|l| Some(l.id) == self.selected))
        {
            super::super::mask_overlay::paint(
                ui,
                response.rect,
                response.rect,
                [40, 180, 240],
                85,
                |uv| {
                    let pos = response.rect.min
                        + egui::vec2(
                            uv[0] as f32 * response.rect.width(),
                            uv[1] as f32 * response.rect.height(),
                        );
                    let Some(xy) = map.source_uv(pos) else {
                        return 0.;
                    };
                    let Some(p) = map.output_point(pos) else {
                        return 0.;
                    };
                    let Some((_, image)) = &input else {
                        return 0.;
                    };
                    let raster = image.source();
                    let x = p[0].floor() as usize;
                    let y = p[1].floor() as usize;
                    if x >= raster.width as usize || y >= raster.height as usize {
                        return 0.;
                    }
                    let p = raster.pixels[y * raster.width as usize + x];
                    if p[3] <= 0.01 {
                        return 0.;
                    }
                    self.overlay_weight(
                        layer,
                        xy,
                        std::array::from_fn(|c| p[c] / p[3]),
                        map.aspect(),
                    )
                },
            );
        }
        self.document_input = input;
        if !active || !self.paint || map.layer.locked {
            return active;
        }
        let count = recipe
            .advanced
            .as_ref()
            .map_or(0, |a| a.masks.iter().map(|m| m.points.len()).sum::<usize>())
            + recipe.layers.as_ref().map_or(0, |s| s.point_count());
        let Some(MaskKind::Shape(mask)) = recipe
            .layers
            .as_mut()
            .and_then(|s| {
                s.layers
                    .iter_mut()
                    .find(|l| Some(l.id) == self.selected && !l.locked)
            })
            .and_then(|l| {
                l.mask
                    .nodes
                    .iter_mut()
                    .find(|n| Some(n.id) == self.component)
            })
            .map(|n| &mut n.kind)
        else {
            return active;
        };
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
        if let Some(p) = origin {
            self.anchor = Some(p);
            self.new_stroke = true;
        }
        if response.dragged() || response.clicked() || response.drag_stopped() {
            let mut remaining = 512_usize.saturating_sub(count);
            for p in [origin, point].into_iter().flatten() {
                if mask.shape == Shape::Brush {
                    if remaining == 0 {
                        self.error = Some("Massimo 512 punti di pennello per ricetta".into());
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
                } else if let Some(start) = self.anchor {
                    mask.center = start;
                    let dx = p[0] - start[0];
                    let dy = (p[1] - start[1]) * map.aspect();
                    let distance = dx.hypot(dy);
                    if distance > 0.005 {
                        mask.radius = distance.min(1.);
                        if mask.shape == Shape::Linear {
                            mask.angle = dy.atan2(dx).to_degrees();
                        }
                    }
                }
            }
        }
        if response.drag_stopped() || response.clicked() {
            self.anchor = None;
        }
        active
    }
}
