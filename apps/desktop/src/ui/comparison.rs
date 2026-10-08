//! Session-only comparison bindings. Menus target the clicked photo, not the selection.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    A,
    B,
    BeforeAfter,
    Focus,
}

#[derive(Default)]
pub(super) struct Comparison {
    pub slots: [Option<Item>; 2],
    pub before_after: bool,
    queued: Option<(PathBuf, Action)>,
    expected: Option<Item>,
    pub pending: Option<(PathBuf, Action)>,
    pub pending_transform: Option<ViewTransform>,
}

impl TrueRenderer {
    pub(super) fn known_items(&self) -> impl Iterator<Item = &Item> {
        self.state
            .items
            .iter()
            .chain(self.photo_menu.extra.values())
            .chain(
                self.comparison
                    .slots
                    .iter()
                    .flatten()
                    .filter(|item| !self.state.items.iter().any(|current| current.id == item.id)),
            )
    }

    pub(super) fn comparison_items(&self) -> [Option<Item>; 2] {
        let current = self.state.current_item().cloned();
        let first = self.comparison.slots[0].clone().or(current);
        let second = self.comparison.slots[1].clone().or_else(|| {
            let id = first.as_ref().map(|item| &item.id);
            self.state
                .selected
                .iter()
                .filter(|selected| Some(*selected) != id)
                .find_map(|selected| self.state.items.iter().find(|item| &item.id == selected))
                .or_else(|| {
                    let position = self
                        .state
                        .visible
                        .iter()
                        .position(|i| Some(&self.state.items[*i].id) == id)?;
                    self.state
                        .visible
                        .get(position + 1)
                        .or_else(|| {
                            position
                                .checked_sub(1)
                                .and_then(|p| self.state.visible.get(p))
                        })
                        .map(|i| &self.state.items[*i])
                })
                .cloned()
        });
        [first, second].map(|item| {
            item.map(|pinned| {
                self.state
                    .items
                    .iter()
                    .find(|current| current.id == pinned.id)
                    .cloned()
                    .unwrap_or(pinned)
            })
        })
    }

    pub(super) fn comparison_menu(
        &mut self,
        ui: &mut egui::Ui,
        path: &std::path::Path,
        expected: Option<&Item>,
    ) {
        let lang = self.cache_settings.language;
        for (label, action) in [
            ("Usa come confronto A", Action::A),
            ("Usa come confronto B", Action::B),
            ("Confronta Prima/Dopo", Action::BeforeAfter),
        ] {
            if ui.button(lang.text(label)).clicked() {
                self.comparison.queued = Some((path.to_owned(), action));
                self.comparison.expected = expected.cloned();
                ui.close();
            }
        }
    }

    pub(super) fn photo_context_menu(&mut self, response: &egui::Response, item: &Item) {
        self.photo_menu_for(response, item, false);
    }

    pub(super) fn apply_comparison(&mut self, item: Item, action: Action) {
        let transform = self.state.transform;
        if action != Action::Focus {
            let mut slots = self.comparison_items();
            if matches!(action, Action::A | Action::B) {
                let target = usize::from(action == Action::B);
                let other = 1 - target;
                if slots[other]
                    .as_ref()
                    .is_some_and(|photo| photo.id == item.id)
                    && slots[target]
                        .as_ref()
                        .is_some_and(|photo| photo.id != item.id)
                {
                    slots[other] = slots[target].clone();
                }
            }
            match action {
                Action::A => slots[0] = Some(item.clone()),
                Action::B => slots[1] = Some(item.clone()),
                Action::BeforeAfter => slots = [Some(item.clone()), Some(item.clone())],
                Action::Focus => unreachable!(),
            }
            self.comparison.slots = slots;
            self.comparison.before_after = action == Action::BeforeAfter;
            self.editing.show_original = false;
            self.editing.advanced = Default::default();
        }
        // Explicit targeting also works when the clicked file was filtered out.
        if self.state.items.iter().any(|current| current.id == item.id) {
            if !self
                .state
                .visible
                .iter()
                .any(|index| self.state.items[*index].id == item.id)
            {
                self.state.targeted = Some(item.id.clone());
                self.state.refilter();
            }
            self.command(Command::Select {
                id: item.id.clone(),
                extend: false,
            });
        }
        self.state.view = ViewMode::Compare;
        if action == Action::Focus {
            self.state.transform = transform;
        }
        self.sample = None;
        self.sample_item_id = None;
        self.sample_level = None;
        self.sample_from_current_render = false;
        self.ensure_edit_loaded(&item);
        self.context.request_repaint();
    }

    pub(super) fn process_comparison_action(&mut self) {
        if let Some((path, action)) = self.comparison.queued.take() {
            let item = self.known_items().find(|item| item.path == path).cloned();
            if let Some(expected) = self.comparison.expected.take()
                && !item.as_ref().is_some_and(|p| {
                    p.id == expected.id
                        && p.digest == expected.digest
                        && p.observation == expected.observation
                })
            {
                self.status = self
                    .cache_settings
                    .language
                    .text("Foto o revisione cambiata")
                    .into();
                return;
            }
            if let Some(item) = item.filter(|item| {
                action != Action::Focus || self.state.items.iter().any(|i| i.id == item.id)
            }) {
                self.apply_comparison(item, action);
            } else {
                let transform = self.state.transform;
                if action != Action::Focus {
                    self.comparison.slots = self.comparison_items();
                }
                // Resolve and scan on the existing filesystem/service lanes. Never
                // fabricate an approved Item or perform filesystem I/O in the UI.
                self.navigate(path.clone(), Some(ViewMode::Compare));
                if self.browser.pending.is_some() {
                    self.comparison.pending = Some((path, action));
                    self.comparison.pending_transform =
                        (action == Action::Focus).then_some(transform);
                }
            }
        }
        if let Some((path, action)) = self.comparison.pending.clone() {
            let item = self
                .state
                .items
                .iter()
                .find(|item| item.path == path)
                .cloned();
            if let Some(item) = item {
                self.comparison.pending = None;
                if let Some(transform) = self.comparison.pending_transform.take() {
                    self.state.transform = transform;
                }
                self.apply_comparison(item, action);
            } else if !self.scanning && self.browser.pending.is_none() {
                self.comparison.pending = None;
                self.comparison.pending_transform = None;
            }
        }
    }

    pub(super) fn comparison_view(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let items = self.comparison_items();
        let before_after = self.comparison.before_after;
        let bottom = ui.available_rect_before_wrap().bottom();
        ui.columns(2, |columns| {
            for (index, (column, item)) in columns.iter_mut().zip(items).enumerate() {
                column.set_max_height((bottom - column.cursor().min.y).max(20.));
                let lane = if index == 0 { "A" } else { "B" };
                let Some(item) = item else {
                    column.label(lane);
                    column.label(lang.text("Tasto destro su una foto per assegnarla ad A o B."));
                    continue;
                };
                self.ensure_edit_loaded(&item);
                let title = if before_after {
                    format!(
                        "{lane} · {} · {}",
                        lang.text(if index == 0 { "Prima" } else { "Dopo" }),
                        item.name
                    )
                } else {
                    format!("{lane} · {}", item.name)
                };
                let header =
                    column.selectable_label(self.state.current.as_ref() == Some(&item.id), title);
                if header.clicked() {
                    self.comparison.queued = Some((item.path.clone(), Action::Focus));
                }
                self.photo_context_menu(&header, &item);
                // All existing RAW, geometry, preview and sampling paths resolve
                // the mode synchronously. Restore it before rendering any other
                // surface. Menu actions are deferred until both lanes are drawn.
                let previous = self.editing.show_original;
                if before_after {
                    self.editing.show_original = index == 0;
                }
                self.paint_view(column, &item, lane);
                self.editing.show_original = previous;
            }
        });
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::super::settings_regressions::{app, settle};
    use super::*;

    fn frame(
        app: &mut TrueRenderer,
        ctx: &egui::Context,
        item: &Item,
        viewer: bool,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        fn text(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::epaint::Shape::Vec(shapes) => {
                    for s in shapes {
                        text(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(800., 600.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    if viewer {
                        app.paint_view(ui, item, "test");
                    } else {
                        app.thumbnail(ui, item, Vec2::new(220., 200.), true);
                    }
                });
            },
        );
        output.textures_delta.clear();
        let mut out = vec![];
        for shape in output.shapes {
            text(&shape.shape, &mut out);
        }
        app.process_comparison_action();
        out
    }

    fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    #[test]
    fn right_click_actions_use_the_clicked_photo_without_a_prior_selection() {
        for viewer in [false, true] {
            for lang in [Language::Italian, Language::English] {
                for (action, label) in [
                    (Action::A, "Usa come confronto A"),
                    (Action::B, "Usa come confronto B"),
                    (Action::BeforeAfter, "Confronta Prima/Dopo"),
                ] {
                    let (_dir, ctx, mut app) = app();
                    settle(&mut app, &ctx, true);
                    app.cache_settings.language = lang;
                    let target = app.state.items[1].clone();
                    let previous = app.state.current.clone();
                    assert_ne!(previous.as_ref(), Some(&target.id));
                    for _ in 0..3 {
                        frame(&mut app, &ctx, &target, viewer, vec![]);
                    }
                    let pos = egui::pos2(90., 80.);
                    for pressed in [true, false] {
                        frame(
                            &mut app,
                            &ctx,
                            &target,
                            viewer,
                            pointer(pos, egui::PointerButton::Secondary, pressed),
                        );
                    }
                    let mut labels = vec![];
                    for _ in 0..3 {
                        labels = frame(&mut app, &ctx, &target, viewer, vec![]);
                    }
                    assert_eq!(
                        app.state.current, previous,
                        "Opening a menu must not select a different photo"
                    );
                    let pos = labels
                        .iter()
                        .find(|(text, _)| text == lang.text(label))
                        .unwrap_or_else(|| panic!("Missing {label}: {labels:?}"))
                        .1
                        .center();
                    for pressed in [true, false] {
                        frame(
                            &mut app,
                            &ctx,
                            &target,
                            viewer,
                            pointer(pos, egui::PointerButton::Primary, pressed),
                        );
                    }
                    assert_eq!(app.state.view, ViewMode::Compare);
                    let slots = app.comparison_items();
                    let index = usize::from(action == Action::B);
                    assert_eq!(slots[index].as_ref().unwrap().id, target.id);
                    assert_eq!(app.comparison.before_after, action == Action::BeforeAfter);
                    if action == Action::BeforeAfter {
                        assert_eq!(slots[1].as_ref().unwrap().id, target.id);
                    } else {
                        assert_ne!(slots[0].as_ref().unwrap().id, slots[1].as_ref().unwrap().id);
                    }
                    assert!(
                        app.state.targeted.is_none(),
                        "Visible photos must not suspend filters"
                    );
                    assert!(app.state.pending.is_empty());
                    assert!(app.editing.entries.values().all(|e| e.draft.as_ref()
                        == e.loaded.as_ref().map(|saved| &saved.recipe)
                        && !e.pending));
                }
            }
        }
    }

    #[test]
    fn assignments_survive_selection_filters_and_folder_navigation() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let a = app.state.items[1].clone();
        let b = app.state.items[0].clone();
        app.apply_comparison(a.clone(), Action::A);
        app.apply_comparison(b.clone(), Action::B);
        app.command(Command::Select {
            id: a.id.clone(),
            extend: false,
        });
        assert_eq!(
            app.comparison_items().map(|i| i.unwrap().id),
            [a.id.clone(), b.id.clone()]
        );
        app.state.query = "no visible photos".into();
        app.state.targeted = None;
        app.state.refilter();
        assert!(app.state.visible.is_empty());
        assert!(app.comparison_items().iter().all(Option::is_some));
        app.full_for_current();
        assert_eq!(app.quality(&a), PreviewQuality::Full);
        assert_eq!(app.quality(&b), PreviewQuality::Full);

        let other = dir.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let path = other.join("other.png");
        std::fs::copy(&b.path, &path).unwrap();
        app.comparison.queued = Some((path.clone(), Action::B));
        app.process_comparison_action();
        assert!(app.comparison.pending.is_some());
        settle(&mut app, &ctx, true);
        app.process_comparison_action();
        assert!(app.comparison.pending.is_none());
        let slots = app.comparison_items();
        assert_eq!(slots[0].as_ref().unwrap().id, a.id);
        assert_eq!(
            slots[1].as_ref().unwrap().path,
            path.canonicalize().unwrap()
        );
        assert_eq!(app.state.view, ViewMode::Compare);
        assert!(app.known_items().any(|item| item.id == a.id));
        app.apply_source_changes(vec![crate::source_monitor::Change {
            id: a.id.clone(),
            observation: "missing".into(),
            bytes: 0,
            available: false,
        }]);
        assert!(!app.comparison_items()[0].as_ref().unwrap().approved);
        assert!(app.source_status.contains_key(&a.id));
    }

    #[test]
    fn pending_comparison_is_cancelled_by_later_navigation() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let first = app.state.items[0].clone();
        app.apply_comparison(first.clone(), Action::A);
        app.comparison.queued = Some((dir.path().join("missing.png"), Action::B));
        app.process_comparison_action();
        assert!(app.comparison.pending.is_some());
        app.navigate(first.path.clone(), Some(ViewMode::Preview));
        assert!(app.comparison.pending.is_none());
        settle(&mut app, &ctx, true);
        app.process_comparison_action();
        assert_eq!(app.state.view, ViewMode::Preview);
        assert_eq!(app.comparison_items()[0].as_ref().unwrap().id, first.id);
    }

    #[test]
    fn first_assignment_from_another_folder_preserves_a_and_header_focus_preserves_zoom() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let a = app.state.current_item().unwrap().clone();
        let other = dir.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let path = other.join("other.png");
        std::fs::copy(&a.path, &path).unwrap();
        app.comparison.queued = Some((path.clone(), Action::B));
        app.process_comparison_action();
        settle(&mut app, &ctx, true);
        app.process_comparison_action();
        let slots = app.comparison_items();
        assert_eq!(slots[0].as_ref().unwrap().id, a.id);
        assert_eq!(
            slots[1].as_ref().unwrap().path,
            path.canonicalize().unwrap()
        );
        app.state.transform.set_zoom(1.75);
        app.state.transform.center = [0.2, 0.7];
        app.comparison.queued = Some((a.path.clone(), Action::Focus));
        app.process_comparison_action();
        settle(&mut app, &ctx, true);
        app.process_comparison_action();
        assert_eq!(app.state.current.as_ref(), Some(&a.id));
        assert_eq!(app.state.transform.zoom, Some(1.75));
        assert_eq!(app.state.transform.center, [0.2, 0.7]);
        assert_eq!(
            app.comparison_items()[1].as_ref().unwrap().path,
            path.canonicalize().unwrap()
        );
    }

    #[test]
    fn before_after_keeps_native_wb_geometry_and_pixels_separate_without_mutating_recipe() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let mut recipe =
            tr_core::editing::EditRecipe::neutral(tr_core::decoder::RawEngine::TrueRenderer);
        recipe.exposure_ev = 1.;
        recipe.process_version = 3;
        recipe.raw_wb.red = 1500;
        recipe.raw_wb.blue = 800;
        let mut advanced = tr_core::editing::Advanced::default();
        advanced.geometry.crop = [0., 0., 0.5, 1.];
        recipe.advanced = Some(Box::new(advanced));
        app.editing.entries.insert(
            item.id.clone(),
            editing::EditEntry {
                loaded: Some(tr_store::LoadedEdit {
                    asset_id: item.id.clone(),
                    source_digest: "comparison".into(),
                    generation: 1,
                    revision: 1,
                    recipe: recipe.clone(),
                    can_undo: true,
                    can_redo: false,
                }),
                draft: Some(recipe.clone()),
                ..Default::default()
            },
        );
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(16, 8, vec![[0.2, 0.2, 0.2, 1.]; 128]).unwrap(),
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        for original in [true, false] {
            let request = app.preview_request_for_mode(&item, 0, original);
            app.cache.insert(
                (item.id.clone(), request),
                CachedImage {
                    digest: "comparison".into(),
                    info: RasterInfo {
                        shooting: None,
                        scientific: None,
                        reference_mip: None,
                        width: 16,
                        height: 8,
                        source_width: 16,
                        source_height: 8,
                        native_bits: 32,
                        format: "test".into(),
                        decoder: "test".into(),
                        input_color: "Rec2020".into(),
                        filter: "reference".into(),
                        orientation: "applied".into(),
                    },
                    histogram: source.source().histogram(),
                    pyramid: source.clone(),
                    touched: 0,
                    transport: "test",
                    worker_pid: None,
                },
            );
        }
        app.apply_comparison(item.clone(), Action::BeforeAfter);
        app.state.transform.zoom = Some(1.);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            app.poll_edit_preview();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800., 600.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| app.comparison_view(ui));
                },
            );
            output.textures_delta.clear();
            assert!(
                !app.editing.show_original,
                "Before mode must not leak into thumbnails or the next lane"
            );
            let a = app.preview_request_for_mode(&item, 0, true);
            let b = app.preview_request_for_mode(&item, 0, false);
            assert_ne!(a.raw_wb, b.raw_wb);
            assert_eq!(a.raw_wb, Default::default());
            assert!(app.demand.contains(&(item.id.clone(), a)));
            assert!(app.demand.contains(&(item.id.clone(), b)));
            if let Some(after) = app.edited_preview(&item.id, "comparison", source.clone()) {
                assert_eq!([after.source().width, after.source().height], [8, 8]);
                assert_eq!(after.source().pixels[0], [0.4, 0.4, 0.4, 1.]);
                assert_eq!(source.source_size(), [16, 8]);
                assert_eq!(source.source().pixels[0], [0.2, 0.2, 0.2, 1.]);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "After must converge while Before stays unedited: {:?}",
                app.edited_preview_error(&item.id)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let entry = &app.editing.entries[&item.id];
        assert_eq!(entry.draft.as_ref(), Some(&recipe));
        assert_eq!(entry.loaded.as_ref().unwrap().revision, 1);
        assert!(!entry.pending);
    }
}
