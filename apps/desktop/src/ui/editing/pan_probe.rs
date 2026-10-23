//! Opt-in continuous pan probe on the native presentation path. Private roots
//! and development smoke provide the edited photograph and final pixel check.
use super::*;

#[derive(Default)]
pub(super) struct Probe {
    source: Option<u64>,
    started: Option<Instant>,
    timing_start: u64,
    last_step: Option<Instant>,
    steps: u32,
    frames: Vec<serde_json::Value>,
    pixels: Vec<serde_json::Value>,
    capture: Option<(String, ProofCapture)>,
    failed: bool,
    done: bool,
}
impl Probe {
    pub(super) fn active(&self) -> bool {
        self.source.is_some() && !self.done
    }
    pub(super) fn done(&self) -> bool {
        self.done
    }
}
impl TrueRenderer {
    pub(super) fn pan_continuity_tick(&mut self, item: &Item) -> bool {
        let performance = std::env::args().any(|a| a == "--interaction-performance-smoke");
        let Some(preview) = self.editing.previews.get(&item.id) else {
            return false;
        };
        let image = preview.image.clone();
        let coverage = self
            .presenter
            .coverage()
            .iter()
            .find(|c| c.lane.starts_with("view:single:"));
        let exact = coverage.is_some_and(|c| c.exact && c.displayed_source == Some(image.id()));
        let covered = coverage.map_or(0., |c| c.fraction);
        if self.editing.pan.source.is_none() {
            if !exact || covered < 0.999 || image.base_level() != 0 {
                return false;
            }
            self.editing.pan.source = Some(image.id());
            self.editing.pan.started = Some(Instant::now());
            self.editing.pan.timing_start = tr_render::timings::mark();
        }
        let region = coverage.map(|c| c.region);
        self.editing.pan.frames.push(serde_json::json!({
            "frame":self.frame_number,"step":self.editing.pan.steps,
            "elapsed_ms":self.editing.pan.started.unwrap().elapsed().as_secs_f64()*1000.,
            "coverage":covered,"exact":exact,"base_level":image.base_level(),
            "presenter_idle":self.presenter.is_idle(),"compute":self.presenter.statistics(),
            "origin":region.map(|r|r.origin),"size":region.map(|r|r.size),
            "sample_step":region.map(|r|r.step),
            "displayed_region":coverage.and_then(|c|c.displayed_region).map(|r|format!("{r:?}")),
        }));
        // Check every draw, including the first after each movement, before
        // waiting for screenshot receipt or admitting another movement.
        self.editing.pan.failed |= !exact
            || covered < 0.999
            || image.base_level() != 0
            || self.editing.pan.source != Some(image.id());
        if !performance && !self.editing.pan.failed && self.check_pan_pixels(&image) {
            return false;
        }
        if !self.editing.pan.failed && self.editing.pan.steps < 120 {
            self.context
                .request_repaint_after(Duration::from_millis(16));
            if self
                .editing
                .pan
                .last_step
                .is_some_and(|t| t.elapsed() < Duration::from_millis(16))
            {
                return false;
            }
            self.editing.pan.last_step = Some(Instant::now());
            self.editing.pan.steps += 1;
            let step = self.editing.pan.steps;
            let direction = if step <= 40 || step > 100 { 1. } else { -1. };
            let region = region.unwrap();
            for (a, delta) in [24., 8.].into_iter().enumerate() {
                self.state.transform.center[a] +=
                    (direction * delta * region.step[a] / f64::from(image.source_size()[a])) as f32;
            }
            return false;
        }
        let passed = !self.editing.pan.failed
            && self.editing.pan.steps == 120
            && (performance || self.editing.pan.pixels.len() == 3)
            && self.editing.pan.pixels.iter().all(|p| p["passed"] == true);
        let report = serde_json::json!({
            "performance_only":performance,"passed":passed,"steps":self.editing.pan.steps,"frames":self.editing.pan.frames,
            "surface_checks":self.editing.pan.pixels,"compute":self.presenter.statistics(),
            "renderer_timings":tr_render::timings::samples_since(self.editing.pan.timing_start),
            "zoom":self.state.transform.zoom,"source_size":image.source_size(),
            "engine":format!("{:?}",self.editing.entries[&item.id].draft.as_ref().unwrap().raw_engine),
            "scope":"120 pans, 24 x 8 backing pixels per movement, at least 16 ms apart, with two direction reversals. Every draw must retain full native detail and complete coverage. Correctness mode compares three native readbacks to independent CPU resampling; performance mode omits readbacks and references during the gesture. Timing describes software draws, not physical display latency."
        });
        let _ = std::fs::write(
            self.root.join("reports/pan-continuity.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        self.editing.pan.done = true;
        if !passed {
            self.capture_screenshot(&self.context.clone(), "pan-failure");
            self.status = "Pan continuity probe failed".into();
            self.fatal = true;
            return false;
        }
        true
    }

    fn check_pan_pixels(&mut self, image: &Arc<ImageLevels>) -> bool {
        let step = self.editing.pan.steps;
        if ![30, 60, 90].contains(&step) || self.editing.pan.pixels.len() >= (step / 30) as usize {
            return false;
        }
        if let Some((name, _)) = &self.editing.pan.capture {
            if !self.screenshots.contains(name) {
                return true;
            }
            let (name, (expected, size, rect, compute)) = self.editing.pan.capture.take().unwrap();
            let result = (|| -> anyhow::Result<_> {
                let screen = image::open(self.root.join(format!("reports/{name}.png")))?.to_rgba8();
                let screen = egui::ColorImage::from_rgba_unmultiplied(
                    [screen.width() as usize, screen.height() as usize],
                    screen.as_raw(),
                );
                navigation::compare(&screen, &expected, size, rect)
            })();
            let check = match result {
                Ok((maximum, differing)) => {
                    serde_json::json!({"step":step,"passed":maximum<=1,"maximum_error_u8":maximum,"differing_channels":differing,"size":size,"compute":compute})
                }
                Err(error) => {
                    serde_json::json!({"step":step,"passed":false,"error":error.to_string()})
                }
            };
            self.editing.pan.pixels.push(check);
            return false;
        }
        let Some(capture) = self
            .presenter
            .captures()
            .iter()
            .find(|c| c.source == image.id() && c.clip.contains_rect(c.rect))
        else {
            return true;
        };
        let Ok(expected) = image.render(capture.region) else {
            return true;
        };
        let ppp = self.context.pixels_per_point();
        let name = format!("pan-{step}");
        self.editing.pan.capture = Some((
            name.clone(),
            (
                expected.to_display(),
                capture.region.size,
                [
                    capture.rect.min.x * ppp,
                    capture.rect.min.y * ppp,
                    capture.rect.max.x * ppp,
                    capture.rect.max.y * ppp,
                ],
                capture.compute,
            ),
        ));
        self.capture_screenshot(&self.context.clone(), &name);
        true
    }
}
