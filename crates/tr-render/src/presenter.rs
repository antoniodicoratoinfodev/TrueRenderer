//! Bounded asynchronous CPU presentation; every uploaded texel is one backing pixel.
use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, TextureOptions};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};
use tr_core::resample::{Pyramid, Region};

#[derive(Clone, Debug, PartialEq)]
struct Key {
    source: u64,
    region: Region,
}
struct Job {
    lane: String,
    key: Key,
    image: Arc<Pyramid>,
}
struct Completed {
    lane: String,
    key: Key,
    result: Result<Vec<u8>, String>,
}
#[derive(Default)]
struct Shared {
    jobs: VecDeque<Job>,
    completed: VecDeque<Completed>,
    stop: bool,
}
struct Entry {
    key: Key,
    texture: TextureHandle,
    touched: u64,
}
pub struct Capture {
    pub source: u64,
    pub rect: Rect,
    pub clip: Rect,
    pub region: Region,
}
pub struct Presenter {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    waiting: HashMap<String, Key>,
    entries: HashMap<String, Entry>,
    errors: HashMap<String, (Key, String)>,
    clock: u64,
    capturing: bool,
    captures: Vec<Capture>,
}
impl Presenter {
    pub fn new(ctx: egui::Context) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let worker_shared = shared.clone();
        let worker = thread::spawn(move || {
            loop {
                let job = {
                    let (lock, ready) = &*worker_shared;
                    let mut state = lock.lock().unwrap();
                    while state.jobs.is_empty() && !state.stop {
                        state = ready.wait(state).unwrap();
                    }
                    if state.stop {
                        break;
                    }
                    state.jobs.pop_front().unwrap()
                };
                let result = job
                    .image
                    .render(job.key.region)
                    .map(|image| image.to_display())
                    .map_err(|e| format!("{e:#}"));
                let mut state = worker_shared.0.lock().unwrap();
                if state.stop {
                    break;
                }
                state.completed.retain(|old| old.lane != job.lane);
                // At most 64 waiting lanes plus one active operation; never block shutdown.
                if state.completed.len() >= 65 {
                    state.completed.pop_front();
                }
                state.completed.push_back(Completed {
                    lane: job.lane,
                    key: job.key,
                    result,
                });
                drop(state);
                ctx.request_repaint();
            }
        });
        Self {
            shared,
            worker: Some(worker),
            waiting: HashMap::new(),
            entries: HashMap::new(),
            errors: HashMap::new(),
            clock: 0,
            capturing: false,
            captures: Vec::new(),
        }
    }
    pub fn begin_capture(&mut self) {
        self.capturing = true;
        self.captures.clear();
    }
    pub fn captures(&self) -> &[Capture] {
        &self.captures
    }
    pub fn clear(&mut self) {
        self.waiting.clear();
        self.entries.clear();
        self.errors.clear();
        let mut shared = self.shared.0.lock().unwrap();
        shared.jobs.clear();
        shared.completed.clear();
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        let completed: Vec<_> = self.shared.0.lock().unwrap().completed.drain(..).collect();
        for finished in completed {
            if self.waiting.get(&finished.lane) != Some(&finished.key) {
                continue;
            }
            self.waiting.remove(&finished.lane);
            match finished.result {
                Ok(rgba) => {
                    self.errors.remove(&finished.lane);
                    let [w, h] = finished.key.region.size;
                    let texture = ctx.load_texture(
                        &finished.lane,
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                        TextureOptions::NEAREST,
                    );
                    self.entries.insert(
                        finished.lane,
                        Entry {
                            key: finished.key,
                            texture,
                            touched: self.clock,
                        },
                    );
                }
                Err(error) => {
                    if self.errors.len() >= 64 {
                        self.errors.clear();
                    }
                    self.errors.insert(finished.lane, (finished.key, error));
                }
            }
        }
        while self.entries.len() > 64
            || self
                .entries
                .values()
                .map(|e| e.texture.size()[0] * e.texture.size()[1] * 4)
                .sum::<usize>()
                > 128 * 1024 * 1024
        {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.touched)
                .map(|(k, _)| k.clone())
                .unwrap();
            self.entries.remove(&oldest);
        }
    }
    pub fn is_idle(&self) -> bool {
        self.waiting.is_empty()
    }
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        lane: String,
        image: &Arc<Pyramid>,
        rect: Rect,
        region: Region,
    ) {
        self.clock += 1;
        let key = Key {
            source: image.id(),
            region,
        };
        if let Some(entry) = self.entries.get_mut(&lane).filter(|e| e.key == key) {
            entry.touched = self.clock;
            if self.capturing {
                self.captures.push(Capture {
                    source: image.id(),
                    rect,
                    clip: ui.clip_rect(),
                    region,
                });
            }
            ui.painter().image(
                entry.texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
            return;
        }
        if let Some((_, error)) = self.errors.get(&lane).filter(|(failed, _)| *failed == key) {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(12.),
                Color32::LIGHT_RED,
            );
            return;
        }
        self.errors.remove(&lane);
        if self.waiting.get(&lane) != Some(&key) {
            let mut shared = self.shared.0.lock().unwrap();
            shared.jobs.retain(|j| j.lane != lane);
            if shared.jobs.len() < 64
                && (self.waiting.contains_key(&lane) || self.waiting.len() < 64)
            {
                shared.jobs.push_back(Job {
                    lane: lane.clone(),
                    key: key.clone(),
                    image: image.clone(),
                });
                self.waiting.insert(lane, key);
                self.shared.1.notify_one();
            }
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(25));
    }
}
impl Drop for Presenter {
    fn drop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stop = true;
            state.jobs.clear();
        }
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Snap both image edges to physical pixels, then sample exactly that raster.
pub fn fitted_rect(area: Rect, source: [u32; 2], ppp: f32) -> Rect {
    let scale = (area.width() * ppp / source[0] as f32).min(area.height() * ppp / source[1] as f32);
    let size = egui::vec2(
        (source[0] as f32 * scale).round().max(1.),
        (source[1] as f32 * scale).round().max(1.),
    );
    let top = area.center() * ppp - size / 2.;
    Rect::from_min_size(
        egui::pos2(top.x.round() / ppp, top.y.round() / ppp),
        size / ppp,
    )
}
pub fn fitted(
    presenter: &mut Presenter,
    ui: &egui::Ui,
    lane: String,
    image: &Arc<Pyramid>,
    area: Rect,
) {
    let source = image.source();
    let rect = fitted_rect(
        area,
        [source.width, source.height],
        ui.ctx().pixels_per_point(),
    );
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    ];
    presenter.paint(
        ui,
        lane,
        image,
        rect,
        Region::fitted([source.width, source.height], size),
    );
}
