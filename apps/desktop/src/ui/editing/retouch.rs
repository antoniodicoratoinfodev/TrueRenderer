//! Photo-local vector gestures; persistence and history use the ordinary recipe.
use super::*;
use tr_core::document::SelectionMode;
use tr_core::editing::retouch::{Kind, Selection, Stroke};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tool {
    Move,
    Stroke(Kind),
    Free,
    Polygon,
    Rectangle,
}
#[derive(Default)]
pub(in crate::ui) struct Controls {
    photo: String,
    tool: Option<Tool>,
    selection: Vec<Selection>,
    points: Vec<[f32; 2]>,
    source: Option<[f32; 2]>,
    gesture: Option<EditRecipe>,
    anchor: Option<[f32; 2]>,
    selected: Option<usize>,
    radius: f32,
    hardness: f32,
    opacity: f32,
    color: [f32; 4],
    mode: Option<SelectionMode>,
    feather: f32,
    error: String,
}
impl Controls {
    pub fn bind(&mut self, id: &str) {
        if self.photo != id {
            *self = Self {
                photo: id.into(),
                radius: 30.,
                hardness: 0.7,
                opacity: 1.,
                color: [1., 0., 0., 1.],
                ..Default::default()
            };
        }
    }
    pub fn cancel(&mut self) -> Option<EditRecipe> {
        let before = self.gesture.take();
        self.points.clear();
        self.anchor = None;
        if before.is_none() {
            self.tool = None;
        }
        before
    }
    pub fn stop(&mut self) {
        self.tool = None;
        self.points.clear();
        self.gesture = None;
        self.anchor = None;
    }
    pub fn active(&self, id: &str) -> bool {
        self.photo == id && self.tool.is_some()
    }
    pub fn selection(&self) -> Vec<Selection> {
        self.selection.clone()
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        recipe: &mut EditRecipe,
        native: Option<[u32; 2]>,
        tools: bool,
    ) -> (bool, bool, bool) {
        let before = recipe.clone();
        let mut commit = false;
        let mut copy = false;
        ui.collapsing("Sposta / trasforma foto",|ui| {
            if ui.selectable_label(self.tool==Some(Tool::Move),"Sposta foto").clicked() {self.tool=Some(Tool::Move);}
            ui.small("Trasformazione reversibile del livello 0; la tela conserva le dimensioni della foto.");
            let mut transform=recipe.retouch.as_ref().map_or(Default::default(),|r|r.transform);
            for (axis,label) in ["X · px","Y · px"].into_iter().enumerate() {
                let scale=native.map_or(1.,|n|n[axis] as f32);
                let mut pixels=transform.offset[axis]*scale;
                let response=ui.add_enabled(native.is_some(),egui::DragValue::new(&mut pixels).speed(1.).range(-2.*scale..=2.*scale).prefix(format!("{label} ")));
                if response.changed() { transform.offset[axis]=pixels/scale; }
                commit|=response.drag_stopped() || (response.changed() && !response.dragged());
            }
            for (v,label,range) in [(&mut transform.scale,"Scala",0.01..=100.),(&mut transform.angle,"Rotazione °",-360. ..=360.)] {
                let response=ui.add(egui::DragValue::new(v).speed(0.1).range(range).prefix(format!("{label} ")));
                commit|=response.drag_stopped() || (response.changed() && !response.dragged());
            }
            if ui.button("Azzera trasformazione foto").clicked() {transform=Default::default();commit=true;}
            if transform!=recipe.retouch.as_ref().map_or(Default::default(),|r|r.transform) {let r=recipe.retouch_mut();r.transform=transform;r.enabled=true;}
        });
        if tools {
            egui::CollapsingHeader::new("Ritocco e selezione sul RAW").default_open(true).show(ui,|ui| {
                ui.small("Modifiche reversibili sulla foto. Alt+clic sceglie la sorgente di Clona e Ripara.");
                ui.horizontal_wrapped(|ui| {
                    for (tool,label) in [(Tool::Stroke(Kind::Paint),"Pennello"),(Tool::Stroke(Kind::Erase),"Gomma"),(Tool::Stroke(Kind::Clone),"Clona"),(Tool::Stroke(Kind::Heal),"Ripara"),(Tool::Stroke(Kind::AutoHeal),"Ripara rapido"),(Tool::Free,"Libera"),(Tool::Polygon,"Poligonale"),(Tool::Rectangle,"Rettangolo")] {
                        if ui.selectable_label(self.tool==Some(tool),label).clicked() { self.tool=Some(tool);self.points.clear();self.selected=None; }
                    }
                    if ui.selectable_label(self.tool.is_none(),"Naviga").clicked() {self.stop();}
                });
                if matches!(self.tool,Some(Tool::Stroke(_))) {
                    ui.add(egui::Slider::new(&mut self.radius,0.5..=1024.).text("Raggio · px"));
                    ui.add(egui::Slider::new(&mut self.hardness,0. ..=1.).text("Durezza"));
                    ui.add(egui::Slider::new(&mut self.opacity,0. ..=1.).text("Intensità"));
                    if self.tool==Some(Tool::Stroke(Kind::Paint)) {ui.color_edit_button_rgba_unmultiplied(&mut self.color);}
                }
                if matches!(self.tool,Some(Tool::Free|Tool::Polygon|Tool::Rectangle)) {
                    ui.horizontal_wrapped(|ui| {for (mode,label) in [(SelectionMode::Replace,"Sostituisci"),(SelectionMode::Add,"Aggiungi"),(SelectionMode::Subtract,"Sottrai"),(SelectionMode::Intersect,"Interseca")] {
                        if ui.selectable_label(self.mode.unwrap_or(SelectionMode::Replace)==mode,label).clicked(){self.mode=Some(mode);}
                    }});
                    ui.add(egui::Slider::new(&mut self.feather,0. ..=100.).text("Sfumatura · px"));
                    if self.tool==Some(Tool::Polygon) {ui.small("Clic aggiunge vertici, doppio clic o Invio chiude. Esc annulla.");}
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.selection.is_empty(),egui::Button::new("Deseleziona")).clicked(){self.selection.clear();}
                    if ui.add_enabled(!self.selection.is_empty(),egui::Button::new("Selezione → regolazione")).clicked(){
                        let mut mask=tr_core::editing::layers::MaskGraph::default();
                        for s in &self.selection {
                            use tr_core::editing::{layers::{MaskKind,Combine},masks::{Mask,Shape}};
                            if s.mode==SelectionMode::Replace {mask=Default::default();}
                            mask.append(MaskKind::Shape(Mask {shape:Shape::Polygon,points:s.points.clone(),feather:s.feather,..Default::default()}),match s.mode {SelectionMode::Subtract=>Combine::Subtract,SelectionMode::Intersect=>Combine::Intersect,_=>Combine::Add});
                        }
                        let mut layer=tr_core::editing::layers::Layer::empty("Regolazione dalla selezione");layer.mask=mask;
                        recipe.layer_stack().layers.push(layer);recipe.schema_version=3;commit=true;
                        self.tool=None;
                    }
                    copy=ui.add_enabled(!self.selection.is_empty(),egui::Button::new("Selezione → copia immagine…")).clicked();
                });
            });
        }
        if let Some(retouch) = &mut recipe.retouch {
            ui.collapsing(
                format!("Ritocchi applicati · {}", retouch.strokes.len()),
                |ui| {
                    commit |= ui
                        .checkbox(&mut retouch.enabled, "Attiva ritocchi e trasformazione")
                        .changed();
                    for (i, s) in retouch.strokes.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            commit |= ui.checkbox(&mut s.enabled, "").changed();
                            if ui
                                .selectable_label(
                                    self.selected == Some(i),
                                    format!("{} · {}", i + 1, s.kind.label()),
                                )
                                .clicked()
                            {
                                self.selected = Some(i);
                                self.tool = None;
                            }
                        });
                    }
                    if let Some(i) = self.selected.filter(|i| *i < retouch.strokes.len()) {
                        let s = &mut retouch.strokes[i];
                        for (v, label) in [
                            (&mut s.opacity, "Intensità tratto"),
                            (&mut s.hardness, "Durezza tratto"),
                        ] {
                            let r = ui.add(egui::Slider::new(v, 0. ..=1.).text(label));
                            commit |= r.drag_stopped() || (r.changed() && !r.dragged());
                        }
                        let width = native.map_or(1., |n| n[0] as f32);
                        let mut radius = s.radius * width;
                        let r = ui.add_enabled(
                            native.is_some(),
                            egui::Slider::new(&mut radius, 0.5..=1024.).text("Raggio tratto · px"),
                        );
                        if r.changed() {
                            s.radius = radius / width;
                        }
                        commit |= r.drag_stopped() || (r.changed() && !r.dragged());
                        if s.kind == Kind::Paint {
                            commit |= ui
                                .color_edit_button_rgba_unmultiplied(&mut s.color)
                                .changed();
                        }
                        if matches!(s.kind, Kind::Clone | Kind::Heal) {
                            for axis in 0..2 {
                                let scale = native.map_or(1., |n| n[axis] as f32);
                                let mut offset = s.offset[axis] * scale;
                                let r = ui.add_enabled(
                                    native.is_some(),
                                    egui::DragValue::new(&mut offset)
                                        .range(-scale..=scale)
                                        .prefix(if axis == 0 {
                                            "Sorgente X · px "
                                        } else {
                                            "Sorgente Y · px "
                                        }),
                                );
                                if r.changed() {
                                    s.offset[axis] = offset / scale;
                                }
                                commit |= r.drag_stopped() || (r.changed() && !r.dragged());
                            }
                        }
                        if ui.button("Elimina tratto").clicked() {
                            retouch.strokes.remove(i);
                            self.selected = None;
                            commit = true;
                        }
                    }
                },
            );
        }
        if let Err(e) = recipe.validate() {
            *recipe = before.clone();
            self.error = e.to_string();
            commit = false;
        }
        if !self.error.is_empty() {
            ui.colored_label(AMBER, &self.error);
        }
        (*recipe != before, commit, copy)
    }
    fn finish_selection(&mut self, native: [u32; 2]) {
        if self.points.len() < 3 {
            self.points.clear();
            return;
        }
        let selection = Selection {
            points: std::mem::take(&mut self.points),
            mode: self.mode.unwrap_or(SelectionMode::Replace),
            feather: (self.feather / native[0] as f32).min(0.25),
        };
        if let Err(e) = selection.validate() {
            self.error = e.to_string();
            return;
        }
        if selection.mode == SelectionMode::Replace {
            self.selection.clear();
        }
        if self.selection.len() < 16 {
            self.selection.push(selection);
            self.error.clear();
        } else {
            self.error = "Massimo 16 componenti di selezione".into();
        }
    }
}
impl TrueRenderer {
    pub(super) fn retouch_controls(
        &mut self,
        ui: &mut egui::Ui,
        item: &Item,
        recipe: &mut EditRecipe,
        native: Option<[u32; 2]>,
    ) -> (bool, bool) {
        self.editing.retouch.bind(&item.id);
        let previous_layers = recipe.layers.as_ref().map_or(0, |s| s.layers.len());
        let (changed, commit, copy) =
            self.editing
                .retouch
                .show(ui, recipe, native, self.editing.layers.view_tools);
        if recipe.layers.as_ref().map_or(0, |s| s.layers.len()) > previous_layers {
            self.editing.layers.view_tools = false;
            self.editing.layers.view_layers = true;
            self.editing.layers.selected = recipe
                .layers
                .as_ref()
                .and_then(|s| s.layers.last())
                .map(|l| l.id);
        }
        if self.editing.retouch.active(&item.id) {
            self.cancel_wb_picker();
            self.editing.layers.paint = false;
            self.editing.layers.pick = false;
            self.editing.layers.overlay = false;
            self.editing.advanced.paint = false;
            self.editing.advanced.horizon = false;
        }
        if copy {
            self.request_conversion(
                item,
                document::PhotoAction::Selection(self.editing.retouch.selection()),
            );
        }
        (changed, commit)
    }
    pub(in crate::ui) fn retouch_interaction(
        &mut self,
        ui: &egui::Ui,
        item: &Item,
        response: &egui::Response,
        native: [u32; 2],
    ) {
        if !self.photo_edit_ready(&item.id)
            || !self.editing.retouch.active(&item.id)
            || ui.input(|i| i.key_down(egui::Key::Space))
        {
            return;
        }
        let mut recipe = self.editing.entries[&item.id].draft.clone().unwrap();
        let before = recipe.clone();
        let output = self.edited_source_size(&item.id, native);
        let ppp = ui.ctx().pixels_per_point();
        let size = output.map(|v| v as f32);
        let scale =
            self.state
                .transform
                .scale(size, [response.rect.width(), response.rect.height()], ppp);
        let physical = egui::vec2(
            (size[0] * scale * ppp).round().max(1.),
            (size[1] * scale * ppp).round().max(1.),
        ) / ppp;
        let top = response.rect.center()
            - egui::vec2(
                physical.x * self.state.transform.center[0],
                physical.y * self.state.transform.center[1],
            );
        let top = egui::pos2((top.x * ppp).round() / ppp, (top.y * ppp).round() / ppp);
        let rect = egui::Rect::from_min_size(top, physical);
        let c = &mut self.editing.retouch;
        let tool = c.tool.unwrap();
        let coordinate_recipe = before.clone();
        let map = |p: egui::Pos2| {
            if !rect.contains(p) || !response.rect.contains(p) {
                return None;
            }
            let uv = [
                ((p.x - top.x) / physical.x) as f64,
                ((p.y - top.y) / physical.y) as f64,
            ];
            let point = if tool == Tool::Move {
                coordinate_recipe
                    .advanced
                    .as_ref()
                    .map_or(Some(uv), |a| a.geometry.source_point(uv, native))
            } else {
                coordinate_recipe.source_point(uv, native)
            }?;
            point
                .iter()
                .all(|v| (0. ..=1.).contains(v))
                .then(|| point.map(|v| v as f32))
        };
        if !c.selection.is_empty() {
            super::mask_overlay::paint(ui, rect, response.rect, [60, 170, 230], 70, |uv| {
                coordinate_recipe.source_point(uv, native).map_or(0., |p| {
                    tr_core::editing::retouch::selection_weight(
                        &c.selection,
                        p.map(|v| v as f32),
                        native[1] as f32 / native[0] as f32,
                    )
                })
            });
        }
        let point = response.interact_pointer_pos().and_then(map);
        let start = (response.drag_started() || response.clicked())
            .then(|| {
                ui.input(|i| i.pointer.press_origin())
                    .or(response.interact_pointer_pos())
            })
            .flatten()
            .and_then(map);
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut commit = false;
        if escape {
            if let Some(old) = c.gesture.take() {
                recipe = old;
            }
            c.points.clear();
            c.anchor = None;
            commit = true;
        } else if tool == Tool::Polygon {
            if response.clicked()
                && let Some(p) = point
                && c.points.len() < 256
                && c.points.last() != Some(&p)
            {
                c.points.push(p);
            }
            if response.double_clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                c.finish_selection(native);
            }
        } else if matches!(tool, Tool::Free | Tool::Rectangle) {
            if let Some(p) = start {
                c.points = vec![p];
            }
            if let Some(p) = point
                && !c.points.is_empty()
            {
                if tool == Tool::Rectangle {
                    let a = c.points[0];
                    c.points = vec![a, [p[0], a[1]], p, [a[0], p[1]]];
                } else if c.points.len() < 256
                    && c.points.last().is_none_or(|last| {
                        (p[0] - last[0]).hypot(p[1] - last[1]) > 1. / native[0] as f32
                    })
                {
                    c.points.push(p);
                }
            }
            if response.drag_stopped() || response.clicked() {
                c.finish_selection(native);
            }
        } else if ui.input(|i| i.modifiers.alt)
            && matches!(tool, Tool::Stroke(Kind::Clone | Kind::Heal))
        {
            if response.clicked() {
                c.source = point;
                c.error.clear();
            }
        } else {
            if let Some(p) = start {
                c.gesture = Some(before.clone());
                c.anchor = Some(p);
                if let Tool::Stroke(kind) = tool {
                    if matches!(kind, Kind::Clone | Kind::Heal) && c.source.is_none() {
                        c.error = "Alt+clic sulla foto per scegliere la sorgente.".into();
                        c.gesture = None;
                        c.anchor = None;
                    } else {
                        let offset = c.source.map_or([0.; 2], |s| [s[0] - p[0], s[1] - p[1]]);
                        let retouch = recipe.retouch_mut();
                        retouch.enabled = true;
                        retouch.strokes.push(Stroke {
                            enabled: true,
                            kind,
                            points: vec![p],
                            radius: (c.radius / native[0] as f32).clamp(1. / 131072., 0.25),
                            hardness: c.hardness,
                            opacity: c.opacity,
                            color: c.color,
                            offset,
                            selection: c.selection.clone(),
                        });
                        c.selected = Some(retouch.strokes.len() - 1);
                    }
                }
            }
            if let (Some(p), Some(anchor), Some(original)) = (point, c.anchor, c.gesture.as_ref()) {
                if tool == Tool::Move {
                    let base = original
                        .retouch
                        .as_ref()
                        .map_or([0.; 2], |r| r.transform.offset);
                    recipe.retouch_mut().transform.offset = [
                        (base[0] + p[0] - anchor[0]).clamp(-2., 2.),
                        (base[1] + p[1] - anchor[1]).clamp(-2., 2.),
                    ];
                    recipe.retouch_mut().enabled = true;
                } else if let Some(stroke) = recipe.retouch_mut().strokes.last_mut()
                    && stroke
                        .points
                        .last()
                        .is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > 0.5 / native[0] as f32)
                {
                    stroke.points.push(p);
                }
            }
            if response.drag_stopped() || response.clicked() {
                commit = c.gesture.take().is_some();
                c.anchor = None;
            }
        }
        if let Err(e) = recipe.validate() {
            recipe = before.clone();
            c.error = e.to_string();
        }
        if !c.points.is_empty() {
            let painter = ui.painter().with_clip_rect(response.rect);
            let project = |p: [f32; 2]| {
                coordinate_recipe
                    .output_point(p.map(f64::from), native)
                    .map(|p| top + egui::vec2(p[0] as f32 * physical.x, p[1] as f32 * physical.y))
            };
            let mut points: Vec<_> = c.points.iter().copied().filter_map(project).collect();
            if tool == Tool::Polygon {
                if let Some(pos) = response.hover_pos() {
                    points.push(pos);
                }
            } else if points.len() > 2 {
                points.push(points[0]);
            }
            if points.len() > 1 {
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                ));
            }
        }
        if let Some(pointer) = response.hover_pos()
            && let Tool::Stroke(_) = tool
        {
            ui.painter().with_clip_rect(response.rect).circle_stroke(
                pointer,
                (c.radius / native[0] as f32 * physical.x).max(2.),
                egui::Stroke::new(1., Color32::WHITE),
            );
        }
        if recipe != before {
            self.apply_edit_draft(&item.id, recipe);
        }
        if commit {
            self.commit_edit(&item.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings_regressions::app;
    fn frame(app: &mut TrueRenderer, ctx: &egui::Context, item: &Item, events: Vec<egui::Event>) {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640., 480.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let response =
                        ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());
                    app.retouch_interaction(ui, item, &response, [640, 480]);
                });
            },
        );
        out.textures_delta.clear();
    }
    fn pointer(p: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }
    fn settle(app: &mut TrueRenderer, ctx: &egui::Context, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.editing.entries[id].pending {
            app.poll(ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn photo_stroke_release_saves_one_revision_and_escape_restores_the_draft() {
        let (_dir, ctx, mut app) = app();
        let item = super::super::retouch_tests::loaded(&mut app, &ctx);
        app.editing.retouch.bind(&item.id);
        app.editing.retouch.tool = Some(Tool::Stroke(Kind::Paint));
        let generation = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .generation;
        let start = egui::pos2(300., 230.);
        let end = egui::pos2(350., 240.);
        frame(&mut app, &ctx, &item, vec![]);
        frame(&mut app, &ctx, &item, pointer(start, true));
        frame(&mut app, &ctx, &item, vec![egui::Event::PointerMoved(end)]);
        assert!(!app.editing.entries[&item.id].pending);
        assert!(
            app.editing.entries[&item.id]
                .draft
                .as_ref()
                .unwrap()
                .has_retouch()
        );
        frame(&mut app, &ctx, &item, pointer(end, false));
        settle(&mut app, &ctx, &item.id);
        let saved = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .clone();
        assert_eq!(saved.generation, generation + 1);
        assert_eq!(saved.recipe.retouch.as_ref().unwrap().strokes.len(), 1);
        frame(&mut app, &ctx, &item, pointer(start, true));
        frame(&mut app, &ctx, &item, vec![egui::Event::PointerMoved(end)]);
        frame(
            &mut app,
            &ctx,
            &item,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        frame(&mut app, &ctx, &item, pointer(end, false));
        assert_eq!(
            app.editing.entries[&item.id].draft.as_ref(),
            Some(&saved.recipe)
        );
        assert!(!app.document.has_document());
        assert!(app.conversion.is_none());
    }
    #[test]
    fn rectangular_selection_stays_ephemeral_and_move_stays_in_photo_recipe() {
        let (_dir, ctx, mut app) = app();
        let item = super::super::retouch_tests::loaded(&mut app, &ctx);
        let original = app.editing.entries[&item.id].draft.clone();
        app.editing.retouch.bind(&item.id);
        app.editing.retouch.tool = Some(Tool::Rectangle);
        let start = egui::pos2(280., 210.);
        let end = egui::pos2(370., 270.);
        frame(&mut app, &ctx, &item, vec![]);
        frame(&mut app, &ctx, &item, pointer(start, true));
        frame(&mut app, &ctx, &item, vec![egui::Event::PointerMoved(end)]);
        frame(&mut app, &ctx, &item, pointer(end, false));
        assert_eq!(app.editing.retouch.selection.len(), 1);
        assert_eq!(app.editing.retouch.selection[0].points.len(), 4);
        assert_eq!(app.editing.entries[&item.id].draft, original);
        app.editing.retouch.tool = Some(Tool::Move);
        frame(&mut app, &ctx, &item, pointer(start, true));
        frame(&mut app, &ctx, &item, vec![egui::Event::PointerMoved(end)]);
        frame(&mut app, &ctx, &item, pointer(end, false));
        settle(&mut app, &ctx, &item.id);
        let r = app.editing.entries[&item.id]
            .draft
            .as_ref()
            .unwrap()
            .retouch
            .as_ref()
            .unwrap();
        assert!(r.transform.offset[0] > 0.1 && r.transform.offset[1] > 0.1);
        assert!(r.strokes.is_empty());
        assert!(!app.document.has_document());
        assert!(app.conversion.is_none());
    }
}
