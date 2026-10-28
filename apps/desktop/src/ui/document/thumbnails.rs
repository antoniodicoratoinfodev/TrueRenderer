//! One bounded background reader for native projects. No photographic decoder or catalog recipe.
use super::*;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
struct Preview {
    path: PathBuf,
    observation: String,
    result: Result<egui::TextureHandle, String>,
    _credit: Option<tr_core::budget::Lease>,
}
type Reply = (PathBuf, String, Option<Result<egui::ColorImage, String>>);
pub(super) struct Previews {
    cache: VecDeque<Preview>,
    pending: Option<mpsc::Receiver<Reply>>,
    memory: tr_core::budget::MemoryBudget,
    retry: Instant,
    invalidated: Vec<PathBuf>,
}
impl Previews {
    pub fn new(memory: tr_core::budget::MemoryBudget) -> Self {
        Self {
            cache: VecDeque::new(),
            pending: None,
            memory,
            retry: Instant::now(),
            invalidated: vec![],
        }
    }
    pub fn invalidate(&mut self, path: &Path) {
        self.cache.retain(|p| p.path != path);
        if self.pending.is_some() && !self.invalidated.iter().any(|p| p == path) {
            self.invalidated.push(path.to_path_buf());
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending else {
            return;
        };
        match rx.try_recv() {
            Ok((path, observation, result)) => {
                self.pending = None;
                let stale = self.invalidated.contains(&path);
                self.invalidated.clear();
                if stale {
                    return;
                }
                let Some(result) = result else {
                    self.retry = Instant::now() + Duration::from_millis(500);
                    return;
                };
                while self.cache.len() >= 16 {
                    self.cache.pop_front();
                }
                let credit = result
                    .as_ref()
                    .ok()
                    .and_then(|i| self.memory.try_reserve((i.pixels.len() * 8) as u64));
                if result.is_ok() && credit.is_none() {
                    return;
                }
                self.cache.push_back(Preview {
                    path,
                    observation,
                    result: result.map(|i| {
                        ctx.load_texture("project-thumbnail", i, egui::TextureOptions::LINEAR)
                    }),
                    _credit: credit,
                });
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, item: &tr_core::Item, area: egui::Rect) {
        self.poll(ui.ctx());
        if let Some(index) = self
            .cache
            .iter()
            .position(|p| p.path == item.path && p.observation == item.observation)
        {
            let cached = self.cache.remove(index).unwrap();
            match &cached.result {
                Ok(texture) => paint(ui, texture, area),
                Err(error) => {
                    ui.painter().text(
                        area.center(),
                        egui::Align2::CENTER_CENTER,
                        "Progetto non leggibile",
                        egui::FontId::proportional(12.),
                        Color32::YELLOW,
                    );
                    ui.interact(
                        area,
                        ui.id().with((&item.id, "project-error")),
                        egui::Sense::hover(),
                    )
                    .on_hover_text(error);
                }
            }
            self.cache.push_back(cached);
            return;
        }
        ui.painter().text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            "Progetto · caricamento…",
            egui::FontId::proportional(12.),
            Color32::GRAY,
        );
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        if self.pending.is_some() || Instant::now() < self.retry {
            return;
        }
        let Some(credit) = self.memory.try_reserve(3 * 1024 * 1024 * 1024) else {
            return;
        };
        let path = item.path.clone();
        let observation = item.observation.clone();
        let memory = self.memory.clone();
        let ctx = ui.ctx().clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _credit = Arc::new(credit);
            let mut retry = false;
            let result = (|| -> Result<egui::ColorImage> {
                let (storage, pixels) = tr_store::document::allocation(&path)?;
                let required = tr_core::document::admission_bytes(storage, pixels);
                let _extra = if required > _credit.bytes() {
                    Some(
                        memory
                            .try_reserve(required - _credit.bytes())
                            .ok_or_else(|| {
                                retry = true;
                                anyhow::anyhow!(
                                    "Memoria insufficiente per l'anteprima del progetto"
                                )
                            })?,
                    )
                } else {
                    None
                };
                let doc = tr_store::document::open_admitted(&path, required)?;
                let image = doc.render()?.reduced(512);
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|p| tr_core::color::display_pixel(*p, 0.18))
                    .collect();
                Ok(egui::ColorImage::from_rgba_unmultiplied(
                    [image.width as usize, image.height as usize],
                    &bytes,
                ))
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send((path, observation, if retry { None } else { Some(result) }));
            ctx.request_repaint();
        });
    }
}
pub(super) fn paint(ui: &egui::Ui, texture: &egui::TextureHandle, area: egui::Rect) {
    let size = texture.size_vec2();
    let scale = (area.width() / size.x).min(area.height() / size.y);
    ui.painter().image(
        texture.id(),
        egui::Rect::from_center_size(area.center(), size * scale),
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
        Color32::WHITE,
    );
}
