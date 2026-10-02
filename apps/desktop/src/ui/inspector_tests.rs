use super::settings_regressions::{app as test_app, settle};
use super::*;

type PaintedText = (String, egui::Rect, egui::Rect);

fn frame(
    app: &mut TrueRenderer,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> Vec<PaintedText> {
    fn collect(shape: &egui::Shape, clip: egui::Rect, text: &mut Vec<PaintedText>) {
        match shape {
            egui::Shape::Text(label) => text.push((
                label.galley.job.text.clone(),
                label.galley.rect.translate(label.pos.to_vec2()),
                clip,
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, text);
                }
            }
            _ => {}
        }
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            app.toolbar(ui);
            app.location_bar(ui);
            app.footer(ui);
            app.inspector(ui);
        },
    );
    output.textures_delta.clear();
    let mut text = Vec::new();
    for shape in output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut text);
    }
    text
}

fn stable_frame(app: &mut TrueRenderer, ctx: &egui::Context, size: Vec2) -> Vec<PaintedText> {
    let mut text = Vec::new();
    for _ in 0..20 {
        text = frame(app, ctx, size, Vec::new());
    }
    text
}

fn tab_rects(text: &[PaintedText], lang: Language, size: Vec2) -> [egui::Rect; 2] {
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    ["Informazioni", "Sviluppo"].map(|title| {
        let label = lang.text(title);
        let (_, rect, clip) = text
            .iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("Missing inspector tab {label}, {size:?}: {text:?}"));
        assert!(
            viewport.contains_rect(*rect) && clip.contains_rect(*rect),
            "Clipped inspector tab {label}, {size:?}: {rect:?}, clip {clip:?}"
        );
        *rect
    })
}

fn click_tab(
    app: &mut TrueRenderer,
    ctx: &egui::Context,
    size: Vec2,
    page: InspectorPage,
) -> Vec<PaintedText> {
    let text = stable_frame(app, ctx, size);
    let tabs = tab_rects(&text, app.cache_settings.language, size);
    let pos = tabs[usize::from(page == InspectorPage::Develop)].center();
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let text = stable_frame(app, ctx, size);
    tab_rects(&text, app.cache_settings.language, size);
    let context = usize::from(app.state.view != ViewMode::Grid);
    assert_eq!(
        app.inspector_pages[context], page,
        "Tab click did not route"
    );
    text
}

fn wait_for_edit(app: &mut TrueRenderer, ctx: &egui::Context, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        settle(app, ctx, true);
        let entry = &app.editing.entries[id];
        assert!(entry.error.is_none(), "{:?}", entry.error);
        if entry.loaded.is_some() && !entry.loading && !entry.pending {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Timed out loading inspector recipe"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn short_inspector_exposes_metadata_and_edit_actions_without_scrolling() {
    for lang in [Language::Italian, Language::English] {
        let (_dir, ctx, mut app) = test_app();
        settle(&mut app, &ctx, true);
        app.cache_settings.language = lang;
        app.show_inspector = true;
        let item = app.state.current_item().unwrap().clone();
        app.ensure_edit_loaded(&item);
        wait_for_edit(&mut app, &ctx, &item.id);
        let size = egui::vec2(550., 360.);
        stable_frame(&mut app, &ctx, size);
        // Include the actual histogram/preview branches; an empty cache hides
        // them and makes a crowded inspector look shorter in a headless test.
        let request = app
            .demand
            .iter()
            .find(|key| key.0 == item.id)
            .unwrap()
            .clone();
        let pyramid = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(16, 96, vec![[0.2, 0.3, 0.4, 1.]; 16 * 96])
                    .unwrap(),
                request.1,
            )
            .unwrap(),
        );
        app.cache.insert(
            request,
            CachedImage {
                digest: item.digest.clone(),
                info: RasterInfo {
                    shooting: None,
                    scientific: None,
                    reference_mip: None,
                    width: 16,
                    height: 96,
                    source_width: 16,
                    source_height: 96,
                    native_bits: 8,
                    format: "PNG".into(),
                    decoder: "test".into(),
                    input_color: "sRGB".into(),
                    filter: "reference".into(),
                    orientation: "applied".into(),
                },
                histogram: pyramid.source().histogram(),
                pyramid,
                touched: 0,
                transport: "test",
                worker_pid: None,
            },
        );
        let visible = |text: &[PaintedText], label: &str| {
            let (_, rect, clip) = text
                .iter()
                .find(|(text, _, _)| text == label)
                .unwrap_or_else(|| panic!("Missing {label}: {text:?}"));
            assert!(
                clip.contains_rect(*rect)
                    && egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(*rect),
                "Hidden {label} in {lang:?}: {rect:?}, clip {clip:?}"
            );
            *rect
        };
        app.state.view = ViewMode::Grid;
        let information = stable_frame(&mut app, &ctx, size);
        visible(&information, lang.text("Dimensioni"));
        visible(&information, lang.text("Formato"));
        let developed = click_tab(&mut app, &ctx, size, InspectorPage::Develop);
        visible(&developed, lang.text("Prima/Dopo"));
        visible(&developed, lang.text("Esposizione"));
        let number = developed
            .iter()
            .find(|(text, _, _)| text.ends_with(" EV"))
            .unwrap();
        let position = visible(&developed, &number.0).center();
        let click = |app: &mut TrueRenderer, position| {
            for pressed in [true, false] {
                frame(
                    app,
                    &ctx,
                    size,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        let generation = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .generation;
        click(&mut app, position);
        frame(
            &mut app,
            &ctx,
            size,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ctrl: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text("0.75".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        wait_for_edit(&mut app, &ctx, &item.id);
        let saved = app.editing.entries[&item.id].loaded.as_ref().unwrap();
        assert_eq!(saved.recipe.exposure_ev, 0.75);
        assert_eq!(saved.generation, generation + 1);
        let developed = stable_frame(&mut app, &ctx, size);
        click(&mut app, visible(&developed, lang.text("Azioni")).center());
        let menu = stable_frame(&mut app, &ctx, size);
        visible(&menu, lang.text("Copia e incolla regolazioni"));
        visible(&menu, lang.text("Verifica resa finale"));

        // A very tall portrait must leave the File fields visible at normal size.
        egui::Popup::close_all(&ctx);
        let wide = egui::vec2(1440., 940.);
        let information = click_tab(&mut app, &ctx, wide, InspectorPage::Information);
        for label in [lang.text("Dimensioni"), lang.text("Formato"), "16 × 96 px"] {
            let (_, rect, clip) = information
                .iter()
                .find(|(text, _, _)| text == label)
                .unwrap();
            assert!(
                clip.contains_rect(*rect),
                "Portrait hides {label}: {rect:?}, {clip:?}"
            );
        }
    }
}

#[test]
fn inspector_tabs_remain_reachable_and_preserve_photo_state_in_both_layouts_and_languages() {
    for lang in [Language::Italian, Language::English] {
        for size in [egui::vec2(550., 360.), egui::vec2(1440., 940.)] {
            let (dir, ctx, mut app) = test_app();
            settle(&mut app, &ctx, true);
            assert_eq!(
                app.inspector_pages,
                [InspectorPage::Information, InspectorPage::Develop],
                "Grid defaults to information; viewer defaults to development"
            );
            app.cache_settings.language = lang;
            app.show_inspector = true;
            let item = app.state.current_item().unwrap().clone();
            app.ensure_edit_loaded(&item);
            wait_for_edit(&mut app, &ctx, &item.id);
            let mut recipe = app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .recipe
                .clone();
            recipe.exposure_ev = 0.75;
            app.editing.entries.get_mut(&item.id).unwrap().draft = Some(recipe);
            app.commit_edit(&item.id);
            wait_for_edit(&mut app, &ctx, &item.id);
            let saved = app.editing.entries[&item.id].loaded.clone().unwrap();
            let mut draft = saved.recipe.clone();
            draft.contrast = 15.;
            app.editing.entries.get_mut(&item.id).unwrap().draft = Some(draft.clone());
            app.state.selected.insert(app.state.items[1].id.clone());
            app.state.transform.zoom = Some(1.75);
            app.state.transform.center = [0.3, 0.7];
            let selected = app.state.selected.clone();
            let current = app.state.current.clone();
            let transform = app.state.transform;

            app.state.view = ViewMode::Grid;
            let initial = stable_frame(&mut app, &ctx, size);
            let tabs = tab_rects(&initial, lang, size);
            if size.x > 700. {
                assert!(initial.iter().any(|(text, _, _)| text == lang.text("File")));
                assert!(
                    !initial
                        .iter()
                        .any(|(text, _, _)| text == lang.text("Prima/Dopo"))
                );
            }
            // Scroll the information body, then use the fixed header to change page.
            let pos = egui::pos2(tabs[1].right() - 2., tabs[1].bottom() + 30.);
            frame(
                &mut app,
                &ctx,
                size,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::MouseWheel {
                        phase: egui::TouchPhase::Move,
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -1200.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let scrolled = stable_frame(&mut app, &ctx, size);
            let fixed_tabs = tab_rects(&scrolled, lang, size);
            for (before, after) in tabs.into_iter().zip(fixed_tabs) {
                assert!((before.center() - after.center()).length() < 1.);
            }
            if size.x < 700. {
                let before = initial
                    .iter()
                    .find(|(text, _, _)| text == lang.text("File"));
                let after = scrolled
                    .iter()
                    .find(|(text, _, _)| text == lang.text("File"));
                assert!(
                    before.is_some()
                        && after.is_none_or(|(_, rect, _)| {
                            (rect.top() - before.unwrap().1.top()).abs() > 1.
                        }),
                    "Compact body did not scroll: {initial:?}, after {scrolled:?}"
                );
            }
            let developed = click_tab(&mut app, &ctx, size, InspectorPage::Develop);
            if size.x > 700. {
                assert!(
                    developed
                        .iter()
                        .any(|(text, _, _)| text == lang.text("Prima/Dopo"))
                );
                assert!(
                    !developed
                        .iter()
                        .any(|(text, _, _)| text == lang.text("File"))
                );
            }
            click_tab(&mut app, &ctx, size, InspectorPage::Information);

            // Viewer keeps its default until explicitly changed, independently of grid.
            app.state.view = ViewMode::Preview;
            stable_frame(&mut app, &ctx, size);
            assert_eq!(app.inspector_pages[1], InspectorPage::Develop);
            for page in [
                InspectorPage::Information,
                InspectorPage::Develop,
                InspectorPage::Information,
            ] {
                click_tab(&mut app, &ctx, size, page);
            }
            app.state.view = ViewMode::Grid;
            let text = stable_frame(&mut app, &ctx, size);
            tab_rects(&text, lang, size);
            assert_eq!(app.inspector_pages[0], InspectorPage::Information);
            assert_eq!(app.state.current, current);
            assert_eq!(app.state.selected, selected);
            assert_eq!(app.state.transform.zoom, transform.zoom);
            assert_eq!(app.state.transform.center, transform.center);
            let entry = &app.editing.entries[&item.id];
            assert_eq!(entry.draft.as_ref(), Some(&draft));
            assert!(!entry.pending && !entry.loading && entry.error.is_none());
            let after = entry.loaded.as_ref().unwrap();
            assert_eq!(after.generation, saved.generation);
            assert_eq!(after.revision, saved.revision);
            assert_eq!(after.recipe, saved.recipe);
            let catalog = tr_store::Catalog::open(&dir.path().join("data")).unwrap();
            let durable = catalog
                .load_edit(&item.id, saved.recipe.raw_engine)
                .unwrap();
            assert_eq!(durable.generation, saved.generation);
            assert_eq!(durable.recipe, saved.recipe);
        }
    }
}
