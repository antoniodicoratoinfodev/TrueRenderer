//! A confirmed composition is prepared offscreen and published only when complete.
use super::*;
#[derive(Clone, Debug)]
pub(in crate::ui) enum PhotoAction {
    Copy,
    Empty,
    Image(PathBuf),
    Selection(Vec<tr_core::editing::retouch::Selection>),
}
impl PhotoAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Copy => "Copia immagine",
            Self::Empty => "Livello raster vuoto",
            Self::Image(_) => "Inserisci immagine",
            Self::Selection(_) => "Copia selezione",
        }
    }
}
pub(super) struct Conversion {
    pub action: PhotoAction,
    pub output: PathBuf,
    pub base: Option<Document>,
}
pub(super) struct Publishing {
    output: PathBuf,
    result: mpsc::Receiver<Result<Document>>,
}
impl Editor {
    pub(in crate::ui) fn busy(&self) -> bool {
        self.import.is_some() || self.publishing.is_some()
    }
    pub fn begin_conversion(&mut self, output: PathBuf, action: PhotoAction) {
        if self.import_photo.is_some() {
            self.conversion = Some(Conversion {
                action,
                output,
                base: None,
            });
        }
    }
    pub(super) fn conversion_result(&mut self, imported: Imported) {
        let mut conversion = self.conversion.take().unwrap();
        let result = (|| -> Result<Document> {
            let imported = imported?;
            if let Some(mut base) = conversion.base.take() {
                base.insert(imported.layer)?;
                return Ok(base);
            }
            let layer = imported.layer;
            let base = Document {
                version: 2,
                width: layer.width,
                height: layer.height,
                ppi: imported.resolution.map_or(300., |r| r.x),
                resolution: imported.resolution,
                unit: Unit::Pixels,
                color_space: ColorSpace::Srgb,
                layers: vec![layer],
                selection: vec![],
            };
            let growth =
                base.storage_bytes() as u64 + u64::from(base.width) * u64::from(base.height) * 4;
            let retained = self.doc.as_ref().map_or(0, |d| d.storage_bytes() as u64);
            ensure!(
                self.admit_bytes(tr_core::document::admission_bytes(
                    retained + growth * 2,
                    base.largest_pixels()
                )),
                "{}",
                self.error
            );
            Ok(base)
        })();
        match result {
            Ok(base) => {
                if let PhotoAction::Image(path) = &conversion.action
                    && base.layers.len() == 1
                {
                    let path = path.clone();
                    conversion.base = Some(base);
                    self.conversion = Some(conversion);
                    if self.queue_import(
                        path,
                        tr_core::editing::EditRecipe::neutral(Default::default()),
                        None,
                    ) {
                        return;
                    }
                    self.conversion = None;
                } else {
                    // Native copies, selection replay and file I/O must not block the UI.
                    let output = conversion.output;
                    let destination = output.clone();
                    let admission = self.admission.clone();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _admission = admission;
                        let result = prepare(base, conversion.action).and_then(|doc| {
                            tr_store::document::create(&destination, &doc)?;
                            Ok(doc)
                        });
                        let _ = tx.send(result);
                    });
                    self.publishing = Some(Publishing { output, result: rx });
                    return;
                }
            }
            Err(e) => self.error = e.to_string(),
        }
        self.import_photo = None;
        self.import_path = None;
    }
    pub(super) fn poll_publishing(&mut self) {
        let Some(job) = &self.publishing else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(anyhow::anyhow!("Creazione progetto interrotta"))
            }
        };
        let output = self.publishing.take().unwrap().output;
        match result {
            Ok(doc) => {
                self.replace(doc);
                self.select_layer(1);
                self.select_tab(Tab::Layers);
                self.saved(output, Ok(()), true);
            }
            Err(e) => self.error = e.to_string(),
        }
        self.import_photo = None;
        self.import_path = None;
    }
}
fn prepare(mut base: Document, action: PhotoAction) -> Result<Document> {
    match action {
        PhotoAction::Copy => base.duplicate(0)?,
        PhotoAction::Empty => base.insert(RasterLayer::new(
            "Livello vuoto".into(),
            LinearImage::new(
                base.width,
                base.height,
                vec![[0.; 4]; base.width as usize * base.height as usize],
            )?,
        ))?,
        PhotoAction::Selection(selection) => {
            // Restrict the visible result after replay, so strokes cannot restore
            // pixels outside the copied selection. The original recipe stays on 0.
            let source = &base.layers[0];
            let native = [source.width, source.height];
            let mut rendered =
                LinearImage::new(source.width, source.height, source.pixels.clone())?;
            source.recipe.apply(&mut rendered)?;
            let width = rendered.width;
            let height = rendered.height;
            let coverage = (0..width as usize * height as usize)
                .map(|i| {
                    source
                        .recipe
                        .source_point(
                            [
                                ((i % width as usize) as f64 + 0.5) / width as f64,
                                ((i / width as usize) as f64 + 0.5) / height as f64,
                            ],
                            native,
                        )
                        .map_or(0., |p| {
                            tr_core::editing::retouch::selection_weight(
                                &selection,
                                p.map(|v| v as f32),
                                native[1] as f32 / native[0] as f32,
                            )
                        })
                })
                .collect();
            let mut copy = RasterLayer::new("Copia selezione".into(), rendered);
            copy.mask = coverage;
            base.insert(copy)?;
        }
        PhotoAction::Image(_) => {}
    }
    base.validate()?;
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        Editor::new(
            mpsc::sync_channel(1).0,
            tr_core::budget::MemoryBudget::new(16 * 1024 * 1024 * 1024),
        )
    }
    fn settle(e: &mut Editor) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while e.publishing.is_some() {
            e.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn imported() -> Imported {
        Ok(crate::decode_pool::ImportedDocumentImage {
            layer: RasterLayer::new(
                "base.nef".into(),
                LinearImage::new(16, 12, vec![[0.2, 0.1, 0., 1.]; 192]).unwrap(),
            ),
            resolution: Some(tr_core::protocol::Resolution { x: 300., y: 240. }),
        })
    }
    #[test]
    fn confirmed_copy_creates_a_separate_project_and_selects_new_layer() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = editor();
        let output = dir.path().join("base.truerenderer");
        e.conversion = Some(Conversion {
            action: PhotoAction::Copy,
            output: output.clone(),
            base: None,
        });
        e.import_path = Some(dir.path().join("base.nef"));
        e.conversion_result(imported());
        settle(&mut e);
        assert!(e.error.is_empty(), "{}", e.error);
        let doc = tr_store::document::open(&output).unwrap();
        assert_eq!(doc.layers.len(), 2);
        assert_eq!(doc.layers[0].pixels, doc.layers[1].pixels);
        assert_eq!(e.selected, 1);
        assert_eq!(e.reveal, Some(output));
        assert!(!e.dirty);
        assert!(e.source_path.is_none());
    }
    #[test]
    fn failed_or_colliding_conversion_preserves_existing_project_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = editor();
        let old = Document::new(8, 6, 300., [0.; 4]).unwrap();
        e.replace(old.clone());
        let output = dir.path().join("occupied.truerenderer");
        std::fs::write(&output, b"private existing file").unwrap();
        for result in [Err(anyhow::anyhow!("decode failed")), imported()] {
            e.conversion = Some(Conversion {
                action: PhotoAction::Empty,
                output: output.clone(),
                base: None,
            });
            e.conversion_result(result);
            settle(&mut e);
            assert!(!e.error.is_empty());
            assert_eq!(e.doc.as_ref(), Some(&old));
            assert_eq!(std::fs::read(&output).unwrap(), b"private existing file");
            assert!(e.reveal.is_none());
        }
    }
}
