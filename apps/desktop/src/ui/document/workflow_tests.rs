use super::*;
fn editor() -> Editor {
    Editor::new(
        mpsc::sync_channel(1).0,
        tr_core::budget::MemoryBudget::new(4 * 1024 * 1024 * 1024),
    )
}
fn frame(editor: &mut Editor, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
    fn text(shape: &egui::Shape, labels: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => labels.push(t.galley.job.text.clone()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| text(s, labels)),
            _ => {}
        }
    }
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            editor.poll();
            egui::Panel::right("test-inspector")
                .default_size(344.)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| editor.inspector_contents(ui));
                });
            egui::CentralPanel::default().show(ui, |ui| editor.canvas(ui, &mut Default::default()));
        },
    );
    let mut labels = vec![];
    for shape in &out.shapes {
        text(&shape.shape, &mut labels);
    }
    out.textures_delta.clear();
    labels
}
#[test]
fn four_tabs_share_document_selection_and_never_mutate_on_paint() {
    let mut e = editor();
    let ctx = egui::Context::default();
    let mut doc = Document::new(64, 48, 300., [0.2, 0.1, 0., 1.]).unwrap();
    doc.duplicate(0).unwrap();
    e.replace(doc.clone());
    e.select_layer(1);
    for tab in [Tab::Develop, Tab::Layers, Tab::Tools, Tab::Export] {
        e.select_tab(tab);
        for _ in 0..3 {
            frame(&mut e, &ctx, vec![]);
        }
        let labels = frame(&mut e, &ctx, vec![]);
        for title in ["Sviluppo", "Livelli", "Tools", "Export"] {
            assert!(
                labels.iter().any(|s| s == title),
                "{tab:?}: {title} missing"
            );
        }
        assert_eq!(e.doc.as_ref(), Some(&doc));
        assert_eq!(e.selected, 1);
        assert!(e.undo.is_empty());
    }
}
#[test]
fn copy_selected_piece_move_undo_save_reopen_preserves_base_and_recipe() {
    let mut e = editor();
    let mut doc = Document::new(8, 6, 300., [0.2, 0.1, 0., 1.]).unwrap();
    doc.layers[0].recipe.exposure_ev = 0.5;
    doc.select_polygon(
        &[[2., 1.], [5., 1.], [5., 4.], [2., 4.]],
        SelectionMode::Replace,
        0.,
    )
    .unwrap();
    let base = doc.layers[0].clone();
    e.replace(doc.clone());
    e.edit(|d, i| d.copy_selection(i));
    e.select_layer(1);
    e.begin_stroke();
    e.doc.as_mut().unwrap().layers[1].transform.x = 2.;
    e.end_stroke();
    let moved = e.doc.clone().unwrap();
    assert_eq!(e.undo.len(), 2);
    assert_eq!(moved.layers[0], base);
    assert_eq!(moved.layers[1].mask.iter().sum::<f32>(), 9.);
    assert_eq!(moved.layers[1].recipe, base.recipe);
    e.undo_edit();
    assert_eq!(e.doc.as_ref().unwrap().layers[1].transform.x, 0.);
    e.redo_edit();
    assert_eq!(e.doc.as_ref(), Some(&moved));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("livelli città.truerenderer");
    tr_store::document::save(&path, &moved).unwrap();
    let mut reopened = tr_store::document::open(&path).unwrap();
    assert_eq!(reopened, moved);
    reopened.layers[1].transform.y = 1.;
    tr_store::document::save(&path, &reopened).unwrap();
    assert_eq!(tr_store::document::open(&path).unwrap(), reopened);
    assert_eq!(reopened.layers[0], base);
}
#[test]
fn photo_import_keeps_native_canvas_and_crop_recipe_on_base_layer() {
    let mut e = editor();
    let ctx = egui::Context::default();
    let mut layer = RasterLayer::new(
        "Foto".into(),
        LinearImage::new(20, 12, vec![[0.2, 0.1, 0., 1.]; 240]).unwrap(),
    );
    layer
        .recipe
        .advanced
        .get_or_insert_with(Default::default)
        .geometry
        .crop = [0.1, 0., 0.9, 1.];
    layer.recipe.require_process(3);
    layer.recipe.exposure_ev = 0.7;
    let (tx, rx) = mpsc::channel();
    e.import = Some(rx);
    e.import_photo = Some("source-id".into());
    e.tab = Tab::Layers;
    tx.send(Ok(crate::decode_pool::ImportedDocumentImage {
        layer: layer.clone(),
        resolution: Some(tr_core::protocol::Resolution { x: 240., y: 300. }),
    }))
    .unwrap();
    frame(&mut e, &ctx, vec![]);
    let doc = e.doc.as_ref().unwrap();
    assert_eq!([doc.width, doc.height], [20, 12]);
    assert_eq!(
        doc.physical_resolution(),
        Some(tr_core::protocol::Resolution { x: 240., y: 300. })
    );
    assert_eq!(doc.layers, vec![layer]);
    assert_eq!(e.source_id.as_deref(), Some("source-id"));
    assert_eq!(e.tab, Tab::Layers);
}
#[test]
fn failed_import_keeps_previous_unsaved_project() {
    let mut e = editor();
    let ctx = egui::Context::default();
    let doc = Document::new(8, 6, 300., [0.; 4]).unwrap();
    e.replace(doc.clone());
    let (tx, rx) = mpsc::channel();
    e.import = Some(rx);
    e.import_photo = Some("new-photo".into());
    tx.send(Err(anyhow::anyhow!("decode failed"))).unwrap();
    frame(&mut e, &ctx, vec![]);
    assert_eq!(e.doc.as_ref(), Some(&doc));
    assert!(e.dirty);
    assert!(e.import_photo.is_none());
    assert_eq!(e.error, "decode failed");
}
#[test]
fn png8_export_uses_composite_alpha_and_requested_dimensions() {
    let mut doc = Document::new(8, 4, 300., [0.; 4]).unwrap();
    doc.duplicate(0).unwrap();
    doc.layers[1]
        .pixels
        .fill(tr_core::color::from_encoded_srgb([1., 0.5, 0.25, 0.5]));
    doc.layers[1].transform.x = 2.;
    let original = doc.clone();
    let mut bytes = vec![];
    export::encode_image(&doc, export::ImageFormat::Png8, 95, 4, &mut bytes).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (4, 2));
    for (actual, expected) in image
        .get_pixel(3, 1)
        .0
        .into_iter()
        .zip([255u8, 128, 64, 128])
    {
        assert!(actual.abs_diff(expected) <= 1);
    }
    assert_eq!(image.get_pixel(3, 1).0[3], 128);
    assert_eq!(doc, original);
}
#[test]
fn jpeg_export_composites_transparency_on_white() {
    let doc = Document::new(8, 8, 300., [0.; 4]).unwrap();
    let mut bytes = vec![];
    export::encode_image(&doc, export::ImageFormat::Jpeg, 100, 0, &mut bytes).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    assert!(image.pixels().all(|p| p.0.iter().all(|v| *v >= 254)));
}

#[test]
fn new_document_is_a_file_in_current_folder_and_creation_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = editor();
    e.directory = dir.path().into();
    e.name = "Prova città".into();
    e.width = 24.;
    e.height = 16.;
    e.create_in_folder().unwrap();
    let path = dir.path().join("Prova città.truerenderer");
    assert!(e.matches(&path));
    assert!(!e.dirty);
    assert_eq!(e.reveal.as_ref(), Some(&path));
    assert_eq!(e.published.pop_front(), Some(path.clone()));
    assert_eq!(
        tr_store::document::open(&path).unwrap(),
        *e.doc.as_ref().unwrap()
    );
    let saved = std::fs::read(&path).unwrap();
    e.width = 32.;
    assert!(e.create_in_folder().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    e.name = "../outside".into();
    assert!(e.create_in_folder().is_err());
}
#[test]
fn browsing_photos_retains_dirty_document_and_history_without_showing_it_as_another_photo() {
    let mut e = editor();
    let path = PathBuf::from("/album/photo.png");
    e.replace(Document::new(8, 6, 300., [0.; 4]).unwrap());
    e.source_path = Some(path.clone());
    e.source_id = Some("photo".into());
    e.edit(|d, i| d.duplicate(i));
    let original = e.doc.clone();
    assert!(!e.select(PathBuf::from("/album/other.png").as_path()));
    assert_eq!(e.doc, original);
    assert!(e.dirty);
    assert_eq!(e.undo.len(), 1);
    assert!(e.select(&path));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("saved.truerenderer");
    tr_store::document::save(&project, e.doc.as_ref().unwrap()).unwrap();
    e.saved(project.clone(), Ok(()), true);
    assert!(!e.matches(&path));
    assert!(e.matches(&project));
    assert_eq!(e.doc, original);
}
#[test]
fn malformed_project_does_not_replace_current_document_or_retry_every_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ok.truerenderer");
    let invalid = dir.path().join("invalid.truerenderer");
    let doc = Document::new(8, 6, 300., [0.; 4]).unwrap();
    tr_store::document::save(&path, &doc).unwrap();
    std::fs::write(&invalid, b"broken").unwrap();
    let mut e = editor();
    assert!(e.open(path.clone()));
    assert!(!e.select(&invalid));
    assert!(e.matches(&path));
    assert!(!e.matches(&invalid));
    assert_eq!(e.doc.as_ref(), Some(&doc));
    assert_eq!(e.attempted.as_ref(), Some(&invalid));
    assert!(!e.select(&invalid));
}

#[test]
fn export_overrides_density_and_dimensions_without_mutating_project() {
    use image::ImageDecoder;
    let mut doc = Document::new(20, 12, 240., [0.1, 0.2, 0.3, 1.]).unwrap();
    doc.resolution = Some(tr_core::protocol::Resolution { x: 240., y: 150. });
    let original = doc.clone();
    for custom in [false, true] {
        let density = if custom {
            Some(tr_core::protocol::Resolution { x: 72.5, y: 96.25 })
        } else {
            doc.physical_resolution()
        };
        let options = export::ImageOptions {
            format: export::ImageFormat::Png16,
            quality: 95,
            edge: if custom { 10 } else { 0 },
            space: ColorSpace::Srgb,
            resolution: density,
        };
        let mut bytes = vec![];
        export::encode_with_options(&doc, options, &mut bytes).unwrap();
        let png = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .unwrap();
        let dims = png.info().pixel_dims.unwrap();
        let density = density.unwrap();
        assert!((dims.xppu as f64 * 0.0254 - density.x).abs() < 0.02);
        assert!((dims.yppu as f64 * 0.0254 - density.y).abs() < 0.02);
        assert_eq!(
            (png.info().width, png.info().height),
            if custom { (10, 6) } else { (20, 12) }
        );
        let mut bytes = vec![];
        export::encode_with_options(
            &doc,
            export::ImageOptions {
                format: export::ImageFormat::Jpeg,
                ..options
            },
            &mut bytes,
        )
        .unwrap();
        let mut jpeg = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(jpeg.dimensions(), if custom { (10, 6) } else { (20, 12) });
        let exif = jpeg.exif_metadata().unwrap().unwrap();
        assert_eq!(&exif[..4], b"II\x2a\0");
        let rational = |tag: u16| {
            let count = u16::from_le_bytes(exif[8..10].try_into().unwrap()) as usize;
            let entry = (0..count)
                .map(|i| 10 + i * 12)
                .find(|i| u16::from_le_bytes(exif[*i..*i + 2].try_into().unwrap()) == tag)
                .unwrap();
            let offset =
                u32::from_le_bytes(exif[entry + 8..entry + 12].try_into().unwrap()) as usize;
            let n = u32::from_le_bytes(exif[offset..offset + 4].try_into().unwrap());
            let d = u32::from_le_bytes(exif[offset + 4..offset + 8].try_into().unwrap());
            n as f64 / d as f64
        };
        assert_eq!(rational(282), density.x);
        assert_eq!(rational(283), density.y);
        assert_eq!(doc, original);
    }
    doc.resolution = None;
    let mut bytes = vec![];
    export::encode_image(&doc, export::ImageFormat::Png16, 95, 0, &mut bytes).unwrap();
    assert!(
        png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap()
            .info()
            .pixel_dims
            .is_none()
    );
    let mut bytes = vec![];
    export::encode_image(&doc, export::ImageFormat::Jpeg, 95, 0, &mut bytes).unwrap();
    let mut jpeg = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(jpeg.exif_metadata().unwrap().is_none());
}
#[test]
fn canvas_and_export_controls_keep_unknown_source_density_unknown() {
    let mut e = editor();
    let ctx = egui::Context::default();
    let mut doc = Document::new(12, 8, 300., [0.; 4]).unwrap();
    doc.resolution = None;
    e.replace(doc.clone());
    e.tab = Tab::Export;
    e.dirty = false;
    for _ in 0..3 {
        frame(&mut e, &ctx, vec![]);
    }
    assert_eq!(e.doc.as_ref(), Some(&doc));
    assert!(!e.dirty);
}
#[test]
fn refused_stroke_never_modifies_pixels_or_history() {
    let mut e = Editor::new(
        mpsc::sync_channel(1).0,
        tr_core::budget::MemoryBudget::new(1024),
    );
    let doc = Document::new(12, 8, 300., [0.; 4]).unwrap();
    e.replace(doc.clone());
    e.tool = Tool::Paint;
    assert!(!e.begin_stroke());
    e.paint_at([4., 4.]);
    e.end_stroke();
    assert_eq!(e.doc.as_ref(), Some(&doc));
    assert!(e.undo.is_empty());
    assert!(!e.error.is_empty());
}
