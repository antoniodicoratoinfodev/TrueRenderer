//! Background services survive a graphics restart; repaint targets may change.
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Wake(Arc<Mutex<eframe::egui::Context>>);
impl From<eframe::egui::Context> for Wake {
    fn from(ctx: eframe::egui::Context) -> Self {
        Self(Arc::new(Mutex::new(ctx)))
    }
}
impl Wake {
    pub fn rebind(&self, ctx: eframe::egui::Context) {
        *self.0.lock().unwrap() = ctx;
    }
    pub fn request_repaint(&self) {
        self.0.lock().unwrap().request_repaint();
    }
}
