//! Exercise the two quality entry points through real pointer events and persistence.
use super::settings_regressions::{app as test_app, chrome_frame, settle};
use super::*;

fn frame(app: &mut TrueRenderer, ctx: &egui::Context, size: Vec2) -> Vec<(String, egui::Rect)> {
    let mut text = Vec::new();
    for _ in 0..12 {
        text = chrome_frame(app, ctx, size, Vec::new());
    }
    text
}

#[test]
fn engine_badge_shows_applied_default_and_only_raw_recipe_overrides() {
    use tr_core::decoder::RawEngine;
    let (_dir, ctx, mut app) = test_app();
    settle(&mut app, &ctx, true);
    app.set_language(Language::English);
    let item = app.state.current_item().unwrap().clone();
    app.edit_result(
        item.id.clone(),
        Ok(tr_store::LoadedEdit {
            asset_id: item.id.clone(),
            source_digest: item.digest.clone(),
            generation: 1,
            revision: 1,
            recipe: tr_core::editing::EditRecipe::neutral(RawEngine::TrueRendererExperimental),
            can_undo: false,
            can_redo: false,
        }),
    );
    for bitmap in [false, true] {
        if bitmap {
            app.bitmap_inputs.insert(
                item.id.clone(),
                (
                    item.observation.clone(),
                    RawEngine::TrueRendererExperimental,
                ),
            );
        }
        for engine in RawEngine::choices() {
            app.cache_settings.raw_engine = engine;
            app.apply_settings();
            // An unapplied preference must not replace the applied indicator.
            app.cache_settings.raw_engine = RawEngine::TrueRendererExperimental;
            let text = frame(&mut app, &ctx, Vec2::new(1440., 940.));
            let label = if engine == RawEngine::LibRawBilinear {
                "LibRaw bilinear"
            } else {
                engine.label()
            };
            assert!(
                text.iter().any(|(s, _)| s == label),
                "Missing {label}: {text:?}"
            );
            assert_eq!(
                text.iter().any(|(s, _)| s.starts_with("Photo: ")),
                !bitmap && engine != RawEngine::TrueRendererExperimental
            );
        }
    }
}

fn click(app: &mut TrueRenderer, ctx: &egui::Context, label: &str, in_settings: bool) {
    let size = Vec2::new(1440., 940.);
    let text = frame(app, ctx, size);
    let mut matches = text.iter().filter(|(s, _)| s == label);
    let (_, rect) = if in_settings {
        matches.next_back()
    } else {
        matches.next()
    }
    .unwrap_or_else(|| panic!("Missing {label}: {text:?}"));
    let pos = rect.center();
    for pressed in [true, false] {
        chrome_frame(
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
}

#[test]
fn opening_a_file_or_showing_filters_keeps_space_for_the_photograph() {
    let (_dir, ctx, mut app) = test_app();
    settle(&mut app, &ctx, true);
    app.state.view = ViewMode::Preview;
    for lang in [Language::Italian, Language::English] {
        app.set_language(lang);
        for targeted in [true, false] {
            app.state.targeted = targeted.then(|| app.state.items[0].id.clone());
            app.state.minimum_rating = if targeted { 0 } else { 1 };
            for size in [Vec2::new(1440., 940.), Vec2::new(1100., 720.)] {
                // The extra row must not enlarge the location bar on every pass.
                for _ in 0..32 {
                    let mut remaining = 0.;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ui| {
                            app.toolbar(ui);
                            app.location_bar(ui);
                            app.footer(ui);
                            remaining = ui.available_height();
                        },
                    );
                    output.textures_delta.clear();
                    assert!(
                        remaining > size.y * 0.5,
                        "Lost photograph area: {lang:?}, targeted={targeted}, {size:?}, remaining={remaining}"
                    );
                }
            }
        }
    }
}

#[test]
fn toolbar_and_preferences_quality_clicks_share_applied_and_saved_value() {
    let (_dir, ctx, mut app) = test_app();
    settle(&mut app, &ctx, true);
    let item = app.state.items[1].clone();
    app.command(Command::Select {
        id: item.id.clone(),
        extend: true,
    });
    app.state.transform = ViewTransform {
        zoom: Some(1.75),
        center: [0.4, 0.6],
    };
    let selected = app.state.selected.clone();
    let applied = app.service.cache.settings();
    for lang in [Language::Italian, Language::English] {
        app.set_language(lang);
        for view in [ViewMode::Grid, ViewMode::Preview, ViewMode::Compare] {
            app.state.view = view;
            app.show_settings = false;
            app.open_preferences(SettingsPage::Previews);
            // A quality change must not commit, discard or display another draft.
            app.cache_settings.disk_mib = applied.disk_mib + 1024;
            app.cache_settings.raw_engine = tr_core::decoder::RawEngine::TrueRenderer;
            for (in_settings, before, after, choice) in [
                (false, "Standard", PreviewQuality::Full, lang.text("Piena")),
                (
                    true,
                    lang.text("Piena"),
                    PreviewQuality::Standard,
                    "Standard",
                ),
            ] {
                app.set_photo_quality(&item.id, Some(PreviewQuality::Full));
                click(
                    &mut app,
                    &ctx,
                    &format!("{} · {before}", lang.text("Anteprime")),
                    in_settings,
                );
                click(&mut app, &ctx, choice, false);
                assert_eq!(app.service.cache.settings().quality, after);
                assert_eq!(app.cache_settings.quality, after);
                assert!(app.quality_overrides.is_empty());
                settle(&mut app, &ctx, true);
                let saved = crate::cache::Settings::load(&app.settings_data).unwrap();
                assert_eq!(saved.quality, after);
                assert_eq!(saved.disk_mib, applied.disk_mib);
                assert_eq!(saved.raw_engine, applied.raw_engine);
                assert_eq!(app.cache_settings.disk_mib, applied.disk_mib + 1024);
                assert_eq!(
                    app.cache_settings.raw_engine,
                    tr_core::decoder::RawEngine::TrueRenderer
                );
                assert_eq!(app.state.selected, selected);
                assert_eq!(app.state.current.as_deref(), Some(item.id.as_str()));
                assert_eq!(app.state.transform.zoom, Some(1.75));
                assert_eq!(app.state.transform.center, [0.4, 0.6]);
                assert_eq!(app.folder_load.request.unwrap().quality, after);
                let text = frame(&mut app, &ctx, Vec2::new(1440., 940.));
                let expected = format!("{} · {choice}", lang.text("Anteprime"));
                assert_eq!(text.iter().filter(|(s, _)| s == &expected).count(), 2);
            }
        }
    }
    app.show_settings = false;
    app.open_preferences(SettingsPage::Previews);
    assert_eq!(app.cache_settings, app.service.cache.settings());
}

#[test]
fn quality_controls_stay_disabled_during_cache_maintenance() {
    let (_dir, ctx, mut app) = test_app();
    settle(&mut app, &ctx, true);
    app.set_language(Language::English);
    app.open_preferences(SettingsPage::Previews);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.cache_action = Some(cache_actions::CacheAction::new(rx));
    for in_settings in [false, true] {
        click(&mut app, &ctx, "Previews · Standard", in_settings);
        assert!(!egui::Popup::is_any_open(&ctx));
        assert_eq!(
            app.service.cache.settings().quality,
            PreviewQuality::Standard
        );
    }
    tx.send(Ok(())).unwrap();
    settle(&mut app, &ctx, false);
    click(&mut app, &ctx, "Previews · Standard", false);
    assert!(egui::Popup::is_any_open(&ctx));
}

#[test]
fn engine_and_quality_remain_adjacent_with_loading_and_long_names() {
    let (_dir, ctx, mut app) = test_app();
    settle(&mut app, &ctx, true);
    for lang in [Language::English, Language::Italian] {
        app.set_language(lang);
        let mut settings = app.service.cache.settings();
        settings.raw_engine = tr_core::decoder::RawEngine::TrueRendererExperimental;
        app.service.cache.configure(settings);
        for size in [
            Vec2::new(550., 360.),
            Vec2::new(760., 600.),
            Vec2::new(1100., 720.),
            Vec2::new(1319., 940.),
            Vec2::new(1320., 940.),
            Vec2::new(1440., 940.),
        ] {
            for view in [ViewMode::Grid, ViewMode::Preview] {
                app.state.view = view;
                let text = frame(&mut app, &ctx, size);
                let rect = |label: &str| {
                    text.iter()
                        .find(|(s, _)| s == label)
                        .unwrap_or_else(|| panic!("Missing {label}: {text:?}"))
                        .1
                };
                let engine = rect("trueRendererExperimental 32fp");
                let quality = rect(&format!("{} · Standard", lang.text("Anteprime")));
                assert!(quality.left() > engine.right());
                assert!(
                    (engine.center().y - quality.center().y).abs() < 2.,
                    "Vertical alignment {lang:?}, {size:?}: engine {engine:?}, quality {quality:?}"
                );
                let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for label in [
                    "Menu",
                    lang.text("Impostazioni"),
                    "trueRendererExperimental 32fp",
                    &format!("{} · Standard", lang.text("Anteprime")),
                ] {
                    assert!(
                        viewport.contains_rect(rect(label)),
                        "Overflow {label}, {size:?}: {text:?}"
                    );
                }
                assert!(
                    quality.right() < rect(lang.text("Impostazioni")).left()
                        || quality.top() > rect(lang.text("Impostazioni")).bottom()
                );
                let search = ctx
                    .read_response(egui::Id::new("catalog-search"))
                    .unwrap()
                    .rect;
                assert!(
                    viewport.contains_rect(search),
                    "Search overflow {size:?}: {search:?}"
                );
                assert!(
                    !search.intersects(quality),
                    "Search overlaps quality {size:?}: {search:?}, {quality:?}"
                );
            }
        }
    }
}

#[test]
fn viewer_status_stays_in_chrome_without_resizing_the_photo() {
    let (_dir, ctx, mut app) = test_app();
    settle(&mut app, &ctx, true);
    for language in [Language::English, Language::Italian] {
        app.set_language(language);
        for width in [550., 1100., 1440.] {
            for view in [ViewMode::Preview, ViewMode::Compare] {
                app.state.view = view;
                let text = frame(&mut app, &ctx, Vec2::new(width, 720.));
                let quality = text
                    .iter()
                    .find(|(text, _)| text.starts_with(language.text("Solo questa foto")))
                    .unwrap()
                    .1;
                let status = app.viewer_status_area.unwrap().0;
                assert!((status.center().y - quality.center().y).abs() < 3.);
                let mut previous = None;
                for detail in [
                    "",
                    "Detail limited to 1280 × 800 pixels",
                    "Refining preview…",
                ] {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 720.),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            app.toolbar(ui);
                            app.location_bar(ui);
                            let status = app.viewer_status_area.unwrap().0;
                            assert!(status.width() > 40., "{width}: {status:?}");
                            assert!(status.right() <= width);
                            egui::CentralPanel::default().show(ui, |ui| {
                                let photo = ui.available_rect_before_wrap();
                                assert!(status.bottom() < photo.top());
                                app.viewer_status(
                                    if view == ViewMode::Compare {
                                        "A"
                                    } else {
                                        "single"
                                    },
                                    detail,
                                    "",
                                );
                                if view == ViewMode::Compare {
                                    app.viewer_status("B", "Preparing 1:1 detail…", "");
                                }
                                assert_eq!(photo, ui.available_rect_before_wrap());
                                if let Some(previous) = previous {
                                    assert_eq!(photo, previous);
                                }
                                previous = Some(photo);
                            });
                        },
                    );
                    output.textures_delta.clear();
                }
            }
        }
    }
    app.state.view = ViewMode::Grid;
    frame(&mut app, &ctx, Vec2::new(1440., 940.));
    assert!(app.viewer_status_area.is_none());
}
