use super::*;
use crate::ui::settings_regressions::{app, settle};
use tr_core::editing::retouch::{Kind, Stroke};
pub(super) fn loaded(app: &mut TrueRenderer, ctx: &egui::Context) -> Item {
    settle(app, ctx, true);
    let item = app.state.items[0].clone();
    app.state.current = Some(item.id.clone());
    app.ensure_edit_loaded(&item);
    let end = Instant::now() + Duration::from_secs(10);
    while app
        .editing
        .entries
        .get(&item.id)
        .is_none_or(|e| e.loaded.is_none())
    {
        app.poll(ctx);
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    item
}
fn draw(
    app: &mut TrueRenderer,
    ctx: &egui::Context,
    item: &Item,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(v) => v.iter().for_each(|s| labels(s, out)),
            _ => {}
        }
    }
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400., 1800.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.set_width(344.);
                app.editing_controls(ui, item);
            });
        },
    );
    let mut text = vec![];
    for s in &out.shapes {
        labels(&s.shape, &mut text);
    }
    out.textures_delta.clear();
    text
}
fn click(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}
#[test]
fn photo_tabs_and_all_adjustment_tools_do_not_create_a_document() {
    let (_dir, ctx, mut app) = app();
    let item = loaded(&mut app, &ctx);
    app.cache_settings.language = Language::Italian;
    let original = app.editing.entries[&item.id].draft.clone();
    for (title, tab) in [
        ("Tools", document::Tab::Tools),
        ("Livelli", document::Tab::Layers),
        ("Export", document::Tab::Export),
        ("Sviluppo", document::Tab::Develop),
    ] {
        let text = draw(&mut app, &ctx, &item, vec![]);
        let pos = text.iter().find(|(s, _)| s == title).unwrap().1.center();
        for pressed in [true, false] {
            draw(&mut app, &ctx, &item, click(pos, pressed));
        }
        let text = draw(&mut app, &ctx, &item, vec![]);
        assert_eq!(app.photo_editing_tab(), tab);
        assert!(!app.document.has_document());
        assert!(!app.document.loading(&item.path));
        assert!(app.conversion.is_none());
        if tab == document::Tab::Tools {
            for tool in tr_core::editing::layers::TOOLS {
                assert!(
                    text.iter().any(|(s, _)| s == tool.name),
                    "{} missing",
                    tool.name
                );
            }
        }
        if tab == document::Tab::Export {
            assert!(text.iter().any(|(s, _)| s == "Formato"));
        }
    }
    assert_eq!(app.editing.entries[&item.id].draft, original);
    app.select_photo_editing_tab(document::Tab::Tools);
    let text = draw(&mut app, &ctx, &item, vec![]);
    let pos = text
        .iter()
        .find(|(s, _)| s == "Colore a campioni")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        draw(&mut app, &ctx, &item, click(pos, pressed));
    }
    assert_eq!(
        app.editing.entries[&item.id]
            .draft
            .as_ref()
            .unwrap()
            .layers
            .as_ref()
            .unwrap()
            .layers
            .len(),
        1
    );
    assert!(!app.document.has_document());
    assert!(app.conversion.is_none());
}
#[test]
fn retouch_recipe_persists_undo_redo_and_reopen_without_project_or_original_write() {
    let (dir, ctx, mut app) = app();
    let item = loaded(&mut app, &ctx);
    let original = std::fs::read(&item.path).unwrap();
    let mut recipe = app.editing.entries[&item.id].draft.clone().unwrap();
    recipe.retouch_mut().strokes.push(Stroke {
        enabled: true,
        kind: Kind::Clone,
        points: vec![[0.5, 0.5]],
        radius: 0.02,
        hardness: 0.7,
        opacity: 0.8,
        color: [1.; 4],
        offset: [-0.1, 0.],
        selection: vec![],
    });
    recipe.retouch_mut().transform.offset = [0.05, 0.];
    app.apply_edit_draft(&item.id, recipe.clone());
    app.commit_edit(&item.id);
    let wait = |app: &mut TrueRenderer| {
        let end = Instant::now() + Duration::from_secs(10);
        while app.editing.entries[&item.id].pending {
            app.poll(&ctx);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait(&mut app);
    assert_eq!(
        app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .recipe,
        recipe
    );
    app.step_edit(&item.id, true);
    wait(&mut app);
    assert!(
        app.editing.entries[&item.id]
            .draft
            .as_ref()
            .unwrap()
            .is_neutral()
    );
    app.step_edit(&item.id, false);
    wait(&mut app);
    assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&recipe));
    assert!(!app.document.has_document());
    drop(app);
    let catalog = tr_store::Catalog::open(&dir.path().join("data")).unwrap();
    assert_eq!(
        catalog
            .load_edit(&item.id, Default::default())
            .unwrap()
            .recipe,
        recipe
    );
    assert_eq!(std::fs::read(&item.path).unwrap(), original);
    assert!(
        !std::fs::read_dir(dir.path().join("corpus"))
            .unwrap()
            .any(|p| tr_core::document::is_document_path(&p.unwrap().path()))
    );
}
#[test]
fn conversion_is_only_requested_by_raster_content_and_cancel_preserves_photo() {
    let (_dir, ctx, mut app) = app();
    let item = loaded(&mut app, &ctx);
    let recipe = app.editing.entries[&item.id].draft.clone();
    app.request_conversion(&item, document::PhotoAction::Copy);
    assert!(app.conversion.is_some());
    assert!(!app.document.has_document());
    assert!(!app.document.loading(&item.path));
    // Cancelling this review has no decoder request, file, or recipe side effect.
    app.conversion = None;
    assert_eq!(app.editing.entries[&item.id].draft, recipe);
    assert!(!app.document.has_document());
}
