use super::*;
use tr_core::{editing::WB_PICKER_SIDES, raw_wb::Analysis};

#[derive(Default)]
pub(super) struct Picker {
    photo: String,
    target: Option<Target>,
    expanded: bool,
    pub(super) error: Option<String>,
}

struct Target {
    raw: bool,
    recipe: EditRecipe,
    generation: u64,
}

impl Picker {
    pub(super) fn cancel(&mut self) -> bool {
        self.target.take().is_some()
    }

    fn bind(&mut self, photo: &str) {
        if self.photo != photo {
            *self = Self {
                photo: photo.into(),
                ..Default::default()
            };
        }
    }
}

/// Convert a current edited native sample back to the oriented RAW raster.
fn raw_point(recipe: &EditRecipe, point: [u32; 2], native: [u32; 2]) -> Option<[u32; 2]> {
    if native.contains(&0) {
        return None;
    }
    let Some(a) = &recipe.advanced else {
        return (point[0] < native[0] && point[1] < native[1]).then_some(point);
    };
    let output = a.geometry.output_size(native);
    if point[0] >= output[0] || point[1] >= output[1] {
        return None;
    }
    let p = a.geometry.source_point(
        [
            (point[0] as f64 + 0.5) / output[0] as f64,
            (point[1] as f64 + 0.5) / output[1] as f64,
        ],
        native,
    )?;
    p.iter().all(|v| (0. ..1.).contains(v)).then(|| {
        [
            (p[0] * native[0] as f64).floor() as u32,
            (p[1] * native[1] as f64).floor() as u32,
        ]
    })
}

fn area_controls(ui: &mut egui::Ui, lang: Language, side: &mut u32) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(lang.text("Area")).truncate());
        let width = (ui.available_width() - 3. * ui.spacing().item_spacing.x) / 4.;
        for value in WB_PICKER_SIDES {
            if panels::segment(ui, *side == value, &format!("{value}×{value}"), width).clicked() {
                *side = value;
            }
        }
    });
}

impl TrueRenderer {
    pub(in crate::ui) fn cancel_wb_picker(&mut self) -> bool {
        self.editing.wb_picker.cancel()
    }

    pub(in crate::ui) fn wb_picker_active(&self, id: &str) -> bool {
        let picker = &self.editing.wb_picker;
        picker.photo == id
            && self.state.current.as_deref() == Some(id)
            && !self.editing.show_original
            && !self.output_proof_for(id)
            && !self.editing.layers.view_layers
            && !self.editing.layers.view_tools
            && !self.crop_active(id)
            && picker.target.as_ref().is_some_and(|t| {
                t.generation == self.generation
                    && self.editing.entries.get(id).and_then(|e| e.draft.as_ref())
                        == Some(&t.recipe)
            })
    }

    pub(in crate::ui) fn start_wb_picker(&mut self, item: &Item, raw: bool, recipe: &EditRecipe) {
        self.editing.wb_picker.bind(&item.id);
        self.editing.wb_picker.expanded = true;
        self.editing.wb_picker.error = None;
        self.editing.wb_error = None;
        self.editing.layers.open_tools(false);
        self.editing.layers.view_tools = false;
        self.editing.advanced.paint = false;
        self.editing.advanced.horizon = false;
        if self.state.view == ViewMode::Grid {
            self.command(Command::SetView(ViewMode::Preview));
        }
        self.request_final_preview(&item.id, false);
        self.editing.wb_picker.target = Some(Target {
            raw,
            recipe: recipe.clone(),
            generation: self.generation,
        });
    }

    pub(super) fn wb_picker_controls(
        &mut self,
        ui: &mut egui::Ui,
        item: &Item,
        raw: bool,
        recipe: &EditRecipe,
    ) {
        let lang = self.cache_settings.language;
        self.editing.wb_picker.bind(&item.id);
        let active = self.wb_picker_active(&item.id);
        let ready = self.editing.entries.get(&item.id).is_some_and(|e| {
            !e.loading && !e.pending && !e.dirty() && e.loaded.is_some() && e.error.is_none()
        }) && !self.editing.wb_pending;
        let response = ui.add_enabled(
            ready,
            egui::Button::selectable(active, lang.text("WB automatico")).truncate(),
        );
        if help::attach(ui, response, lang, "WB automatico").clicked() {
            if active {
                self.cancel_wb_picker();
            } else {
                self.start_wb_picker(item, raw, recipe);
            }
        }
        if self.editing.wb_picker.expanded {
            area_controls(ui, lang, &mut self.editing.picker_side);
            if self.wb_picker_active(&item.id) {
                ui.add(
                    egui::Label::new(lang.text("Scegli un punto neutro · Esc annulla")).truncate(),
                )
                .on_hover_text(lang.text(
                    "Clicca una zona bianca o grigia non bruciata. Il WB si applica al clic.",
                ));
            }
        }
        if let Some(error) = &self.editing.wb_picker.error {
            ui.colored_label(AMBER, lang.text(error));
        }
    }

    pub(in crate::ui) fn wb_picker_interaction(
        &mut self,
        ui: &egui::Ui,
        item: &Item,
        response: &egui::Response,
        native: [u32; 2],
        image: Option<&ImageLevels>,
        sample: Option<&tr_render::Sample>,
    ) {
        if !self.wb_picker_active(&item.id) || !self.photo_edit_ready(&item.id) {
            return;
        }
        if response.hovered()
            && let Some(pos) = response.hover_pos()
        {
            // The tip marks the sample; keep the pipette clear of that pixel.
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            let painter = ui.painter().with_clip_rect(response.rect);
            let points = vec![
                pos,
                pos + egui::vec2(3., -6.),
                pos + egui::vec2(13., -16.),
                pos + egui::vec2(17., -12.),
                pos + egui::vec2(7., -2.),
                pos,
            ];
            for (width, color) in [(3.5, Color32::BLACK), (1.3, Color32::WHITE)] {
                painter.add(egui::Shape::line(
                    points.clone(),
                    egui::Stroke::new(width, color),
                ));
                painter.line_segment(
                    [pos + egui::vec2(10., -17.), pos + egui::vec2(18., -9.)],
                    egui::Stroke::new(width, color),
                );
            }
        }
        if response.clicked() {
            self.accept_wb_picker_sample(item, native, image, sample);
        }
    }

    fn accept_wb_picker_sample(
        &mut self,
        item: &Item,
        native: [u32; 2],
        image: Option<&ImageLevels>,
        sample: Option<&tr_render::Sample>,
    ) {
        if !self.wb_picker_active(&item.id) || !self.photo_edit_ready(&item.id) {
            return;
        }
        let Some((image, sample)) = image.filter(|i| i.base_level() == 0).zip(sample) else {
            self.editing.wb_picker.error =
                Some("Attendi il dettaglio nativo e clicca dentro la foto.".into());
            return;
        };
        let target = self.editing.wb_picker.target.as_ref().unwrap();
        let mut recipe = target.recipe.clone();
        if target.raw {
            let Some([x, y]) = raw_point(&recipe, [sample.x, sample.y], native) else {
                self.editing.wb_picker.error =
                    Some("Punto fuori dalla fotografia originale.".into());
                return;
            };
            let saved = self.editing.entries[&item.id].loaded.as_ref().unwrap();
            let revision = saved.generation;
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let requested = self.request(Request::RawWhiteBalance(Box::new(
                crate::photo_export::WbJob {
                    cancel: cancel.clone(),
                    item: item.clone(),
                    recipe,
                    analysis: Analysis::Patch {
                        x,
                        y,
                        side: self.editing.picker_side,
                    },
                    generation: self.generation,
                    revision,
                },
            )));
            if requested {
                self.editing.wb_cancel = Some(cancel);
                self.editing.wb_pending = true;
                self.editing.wb_error = None;
                self.cancel_wb_picker();
            }
        } else {
            let result =
                sample_rgb_area(image.source(), sample.x, sample.y, self.editing.picker_side)
                    .and_then(|area| recipe.neutralize_render_sample(area.working));
            match result {
                Ok(()) => {
                    self.cancel_wb_picker();
                    self.apply_edit_draft(&item.id, recipe);
                    self.commit_edit(&item.id);
                }
                Err(error) => self.editing.wb_picker.error = Some(error.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_picker_maps_the_current_pixel_through_crop_rotation_and_rejects_outside() {
        let mut recipe = EditRecipe::neutral(Default::default());
        assert_eq!(raw_point(&recipe, [13, 7], [80, 60]), Some([13, 7]));
        assert_eq!(raw_point(&recipe, [80, 7], [80, 60]), None);
        assert_eq!(raw_point(&recipe, [0, 0], [0, 0]), None);
        recipe
            .advanced
            .get_or_insert_with(Default::default)
            .geometry
            .quarter_turns = 1;
        let native = [80, 60];
        assert_eq!(raw_point(&recipe, [0, 0], native), Some([0, 59]));
        assert_eq!(raw_point(&recipe, [59, 79], native), Some([79, 0]));
        assert_eq!(raw_point(&recipe, [60, 0], native), None);
        let geometry = &mut recipe.advanced.as_mut().unwrap().geometry;
        geometry.quarter_turns = 0;
        geometry.crop = [0.25, 0.25, 0.75, 0.75];
        assert_eq!(raw_point(&recipe, [0, 0], native), Some([20, 15]));
        assert_eq!(raw_point(&recipe, [39, 29], native), Some([59, 44]));
    }

    #[test]
    fn picker_area_options_remain_on_one_row_in_both_languages() {
        for lang in [Language::Italian, Language::English] {
            for width in [220., 260., 306., 364.] {
                let ctx = egui::Context::default();
                crate::ui::style::apply(&ctx);
                let mut side = 5;
                let mut output = ctx.run_ui(Default::default(), |ui| {
                    ui.set_width(width);
                    panels::control_style(ui);
                    area_controls(ui, lang, &mut side);
                    assert!(ui.min_rect().width() <= width + 0.5);
                    assert!(ui.min_rect().height() <= 30.);
                });
                assert_eq!(side, 5);
                assert!(!output.shapes.is_empty());
                output.textures_delta.clear();
            }
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn picker_waits_for_native_pixels_applies_once_and_cancels_without_editing() {
        let (_dir, ctx, mut app) = crate::ui::settings_regressions::app();
        crate::ui::settings_regressions::settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let recipe = EditRecipe::neutral(Default::default());
        app.state.current = Some(item.id.clone());
        app.editing.entries.insert(
            item.id.clone(),
            EditEntry {
                loaded: Some(LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: item.digest.clone(),
                    generation: 0,
                    revision: 0,
                    recipe: recipe.clone(),
                    can_undo: false,
                    can_redo: false,
                }),
                draft: Some(recipe.clone()),
                ..Default::default()
            },
        );
        app.editing.layers.bind(&item.id);
        app.start_wb_picker(&item, false, &recipe);
        assert!(app.wb_picker_active(&item.id));
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
        assert!(app.cancel_wb_picker());
        assert!(!app.cancel_wb_picker());
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
        app.start_wb_picker(&item, false, &recipe);
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| app.keyboard(ui.ctx()),
        );
        assert!(!app.wb_picker_active(&item.id));
        assert_eq!(app.state.view, ViewMode::Preview);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
        app.start_wb_picker(&item, false, &recipe);
        app.generation += 1;
        assert!(!app.wb_picker_active(&item.id));
        app.start_wb_picker(&item, false, &recipe);
        let pixel = [0.30, 0.32, 0.34, 1.];
        let raster = tr_core::color::LinearImage::new(11, 11, vec![pixel; 121]).unwrap();
        let native = ImageLevels::from_source(raster.clone(), PreviewRequest::full()).unwrap();
        let reduced = ImageLevels::from_reference_mip(raster, [22, 22], 1, true).unwrap();
        let sample = tr_render::Sample {
            x: 5,
            y: 5,
            working: pixel,
            display: [0; 4],
        };
        for image in [None, Some(&reduced)] {
            app.accept_wb_picker_sample(&item, [11, 11], image, Some(&sample));
            assert!(app.wb_picker_active(&item.id));
            assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
            assert!(!app.editing.entries[&item.id].pending);
        }
        app.accept_wb_picker_sample(&item, [11, 11], Some(&native), None);
        assert!(app.wb_picker_active(&item.id));
        let edge = tr_render::Sample {
            x: 0,
            y: 0,
            ..sample
        };
        app.accept_wb_picker_sample(&item, [11, 11], Some(&native), Some(&edge));
        assert!(app.wb_picker_active(&item.id));
        assert!(app.editing.wb_picker.error.is_some());
        app.accept_wb_picker_sample(&item, [11, 11], Some(&native), Some(&sample));
        assert!(!app.wb_picker_active(&item.id));
        assert!(app.editing.entries[&item.id].pending);
        let applied = app.editing.entries[&item.id].draft.clone().unwrap();
        let neutralized = applied.apply_pixel(pixel);
        assert!((neutralized[0] - neutralized[1]).abs() < 1e-5);
        assert!((neutralized[2] - neutralized[1]).abs() < 1e-5);
        assert_eq!(applied.raw_wb, recipe.raw_wb);
        app.accept_wb_picker_sample(&item, [11, 11], Some(&native), Some(&sample));
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&applied));
    }
}
