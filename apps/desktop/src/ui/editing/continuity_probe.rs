//! Opt-in native temporal probe, used with the isolated development smoke.
use super::*;

#[derive(Default)]
pub(super) struct Probe {
    baseline: Option<EditRecipe>,
    steps: u32,
    frames: Vec<serde_json::Value>,
    started: Option<Instant>,
    timing_start: u64,
    last_step: Option<Instant>,
    events: Vec<(u64, f64)>,
    done: bool,
    pixel_capture: Option<(String, ProofCapture, u32)>,
    pixel_checks: Vec<serde_json::Value>,
    failed: bool,
}
impl Probe {
    pub(super) fn active(&self) -> bool {
        self.baseline.is_some() && !self.done
    }
    pub(super) fn done(&self) -> bool {
        self.done
    }
}
impl TrueRenderer {
    fn check_live_edit_pixels(&mut self, item: &Item) -> bool {
        let step = self.editing.continuity.steps;
        if !std::env::args().any(|a| a == "--edit-pixel-smoke")
            || ![30, 60, 90].contains(&step)
            || self.editing.continuity.pixel_checks.len() >= (step / 30) as usize
        {
            return false;
        }
        // Hold this actual draft while the native surface capture completes.
        self.editing
            .changed_at
            .insert(item.id.clone(), Instant::now());
        if let Some((name, _, _)) = &self.editing.continuity.pixel_capture {
            if !self.screenshots.contains(name) {
                return true;
            }
            let (name, (expected, size, rect, compute), base) =
                self.editing.continuity.pixel_capture.take().unwrap();
            let detail_ready = !std::env::args().any(|a| a == "--edit-zoom-smoke") || base == 0;
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
                    serde_json::json!({"step":step,"passed":maximum<=1 && detail_ready,"input_base_level":base,"maximum_error_u8":maximum,"differing_channels":differing,"size":size,"compute":compute})
                }
                Err(error) => {
                    serde_json::json!({"step":step,"passed":false,"error":error.to_string()})
                }
            };
            self.editing.continuity.pixel_checks.push(check);
            return false;
        }
        let Some(displayed) = self.presenter.coverage().iter().find(|c| {
            c.lane.starts_with("view:single:")
                && c.displayed_revision == Some(self.editing.revision)
        }) else {
            return true;
        };
        let Some(capture) = self.presenter.captures().iter().find(|c| {
            Some(c.source) == displayed.displayed_source
                && c.region == displayed.region
                && c.clip.contains_rect(c.rect)
        }) else {
            return true;
        };
        let Some((key, cached)) = self
            .cache
            .iter()
            .find(|((id, _), cached)| id == &item.id && cached.pyramid.id() == capture.source)
        else {
            return true;
        };
        let Some(live) =
            self.live_edit_for(&item.id, &cached.digest, &cached.pyramid, key.1.raw_wb)
        else {
            return true;
        };
        let Ok(expected) = live.render(&cached.pyramid, capture.region) else {
            return true;
        };
        let ppp = self.context.pixels_per_point();
        let name = format!("live-edit-{step}");
        self.editing.continuity.pixel_capture = Some((
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
            cached.pyramid.base_level(),
        ));
        self.capture_screenshot(&self.context.clone(), &name);
        true
    }
    pub(super) fn edit_continuity_tick(&mut self, item: &Item) -> bool {
        let performance = std::env::args().any(|a| a == "--interaction-performance-smoke");
        if !performance && self.check_live_edit_pixels(item) {
            return false;
        }
        let displayed = self
            .presenter
            .coverage()
            .iter()
            .find(|c| c.lane.starts_with("view:single:"));
        if self.editing.continuity.baseline.is_none() {
            if !displayed.is_some_and(|c| c.exact && c.fraction >= 0.999) {
                return false;
            }
            self.editing.continuity.baseline = self.editing.entries[&item.id].draft.clone();
            self.editing.continuity.started = Some(Instant::now());
            self.editing.continuity.timing_start = tr_render::timings::mark();
        }
        let elapsed = self
            .editing
            .continuity
            .started
            .unwrap()
            .elapsed()
            .as_secs_f64()
            * 1000.;
        let original_returned = displayed
            .filter(|c| c.displayed_revision.is_none())
            .and_then(|c| c.displayed_source)
            .is_some_and(|source| self.cache.values().any(|c| c.pyramid.id() == source));
        let covered = displayed.map_or(0., |c| c.fraction);
        self.editing.continuity.frames.push(serde_json::json!({
            "frame":self.frame_number,"step":self.editing.continuity.steps,
            "coverage":covered,"exact":displayed.is_some_and(|c| c.exact),
            "displayed_source":displayed.and_then(|c| c.displayed_source),
            "displayed_revision":displayed.and_then(|c| c.displayed_revision),
            "region":displayed.map(|c|format!("{:?}", c.region)),
            "displayed_region":displayed.and_then(|c|c.displayed_region).map(|r|format!("{r:?}")),
            "elapsed_ms":elapsed,
            "age_ms":displayed.and_then(|c| c.displayed_revision).and_then(|revision| self.editing.continuity.events.iter().find(|(r,_)| *r==revision).map(|(_,time)| elapsed-time)),
            "original_returned":original_returned,
        }));
        self.editing.continuity.failed |= covered < 0.999 || original_returned;
        let failed = self.editing.continuity.failed;
        if failed && !self.screenshots.contains("edit-failure") {
            self.capture_screenshot(&self.context.clone(), "edit-failure");
            return false;
        }
        if !failed && self.editing.continuity.steps < 120 {
            self.context
                .request_repaint_after(Duration::from_millis(16));
            if self
                .editing
                .continuity
                .last_step
                .is_some_and(|t| t.elapsed() < Duration::from_millis(16))
            {
                return false;
            }
            self.editing.continuity.last_step = Some(Instant::now());
            self.editing.continuity.steps += 1;
            let step = self.editing.continuity.steps;
            let mut recipe = self.editing.continuity.baseline.as_ref().unwrap().clone();
            recipe.exposure_ev = 0.25 + (step % 12) as f32 * 0.1;
            recipe.brightness = (step % 16) as f32 * 2. - 12.;
            if std::env::args().any(|arg| arg == "--edit-pixel-smoke") {
                recipe.curve = vec![
                    CurvePoint { x: 0., y: 0. },
                    CurvePoint {
                        x: 0.4,
                        y: 0.25 + (step % 16) as f32 / 40.,
                    },
                    CurvePoint { x: 1., y: 1. },
                ];
            }
            if std::env::args().any(|arg| arg == "--edit-wb-continuity-smoke") {
                if recipe.raw_engine == tr_core::decoder::RawEngine::Apple {
                    recipe.raw_wb.apple_temperature = 4000 + (step % 40) as u16 * 100;
                    recipe.raw_wb.apple_tint = (step % 30) as i16 - 15;
                } else {
                    recipe.raw_wb.red = 750 + (step % 40) as u16 * 20;
                    recipe.raw_wb.blue = 1500 - (step % 40) as u16 * 15;
                }
            }
            self.apply_edit_draft(&item.id, recipe);
            self.editing
                .continuity
                .events
                .push((self.editing.revision, elapsed));
            return false;
        }
        let current = self.editing.previews.get(&item.id).is_some_and(|p| {
            self.editing.entries[&item.id].draft.as_ref() == Some(&p.recipe)
                && self
                    .presenter
                    .captures()
                    .iter()
                    .any(|c| c.source == p.image.id())
        });
        if !failed && !current {
            return false;
        }
        let distinct = self
            .editing
            .continuity
            .frames
            .iter()
            .filter(|f| f["step"].as_u64().unwrap_or(0) < 120)
            .map(|f| {
                (
                    f["displayed_source"].as_u64(),
                    f["displayed_revision"].as_u64(),
                )
            })
            .collect::<HashSet<_>>()
            .len();
        let fresh = distinct >= 20;
        let pixels_passed = performance
            || !std::env::args().any(|arg| arg == "--edit-pixel-smoke")
            || (self.editing.continuity.pixel_checks.len() == 3
                && self
                    .editing
                    .continuity
                    .pixel_checks
                    .iter()
                    .all(|check| check["passed"] == true));
        let report = serde_json::json!({
            "performance_only":performance,"passed":!failed && current && fresh && pixels_passed,"steps":self.editing.continuity.steps,
            "live_surface_checks":self.editing.continuity.pixel_checks,
            "distinct_frames_during_gesture":distinct,"events":self.editing.continuity.events,
            "quality":format!("{:?}",self.quality(item)),"engine":format!("{:?}",self.editing.continuity.baseline.as_ref().unwrap().raw_engine),
            "native_wb":std::env::args().any(|arg|arg=="--edit-wb-continuity-smoke"),
            "zoom":self.state.transform.zoom,"source_size":self.cache.values().map(|c|c.pyramid.source_size()).max(),
            "proof":self.output_proof_for(&item.id),"frames":self.editing.continuity.frames,
            "compute":self.presenter.statistics(),
            "renderer_timings":tr_render::timings::samples_since(self.editing.continuity.timing_start),
            "scope":"Native draw coverage and distinct completed drafts during 120 changes, paced at least 16 ms apart, followed by canonical convergence. Frame age measures event to draw command, not physical display latency. Minimum 20 distinct updates; no 60 fps or p95 qualification. Performance mode omits surface readbacks and CPU references during the gesture. Final screenshot/export checked separately by enclosing development smoke."
        });
        let _ = std::fs::write(
            self.root.join("reports/edit-continuity.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        self.editing.continuity.done = true;
        if failed || !fresh || !pixels_passed {
            self.status = "Edit continuity probe failed".into();
            self.fatal = true;
            return false;
        }
        self.commit_edit(&item.id);
        self.editing.smoke_ready_at = None;
        true
    }
}
