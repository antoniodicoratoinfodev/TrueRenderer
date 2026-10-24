//! Native continuous scale changes, including the first draw after each input.
use super::*;

fn pixel_steps() -> &'static [u32] {
    if std::env::args().any(|a| a == "--zoom-pan-smoke") {
        &[20, 50, 80, 110, 124, 132, 144]
    } else {
        &[20, 50, 80, 110]
    }
}

struct PixelFrame {
    reference: ProofCapture,
    region: tr_core::resample::Region,
    held: bool,
    frame: u64,
}
struct PendingPixels {
    name: String,
    frames: Vec<PixelFrame>,
}

#[derive(Default)]
pub(super) struct Probe {
    started: Option<Instant>,
    timing_start: u64,
    last_step: Option<Instant>,
    steps: u32,
    frames: Vec<serde_json::Value>,
    pixels: Vec<serde_json::Value>,
    capture: Option<PendingPixels>,
    failed: bool,
    done: bool,
    cold_started: bool,
    cold_done: bool,
    cold_frames: Vec<serde_json::Value>,
    compute_before: Option<serde_json::Value>,
    detail_dwell: Option<Instant>,
    detail_before_zoom: Option<serde_json::Value>,
    first_zoom_at: Option<Instant>,
    first_zoom_ready_ms: Option<f64>,
}
impl Probe {
    pub(super) fn active(&self) -> bool {
        (self.started.is_some() || self.cold_started) && !self.done
    }
    pub(super) fn done(&self) -> bool {
        self.done
    }
}
impl TrueRenderer {
    pub(super) fn zoom_continuity_tick(&mut self, item: &Item) -> bool {
        let performance = std::env::args()
            .any(|a| a == "--zoom-performance-smoke" || a == "--interaction-performance-smoke");
        let pan_after_zoom = std::env::args().any(|a| a == "--zoom-pan-smoke");
        let total_steps = if pan_after_zoom { 144 } else { 120 };
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
        let held = coverage.is_some_and(|c| c.held);
        let requested_region = coverage.map(|c| c.region);
        if std::env::args().any(|a| a == "--zoom-from-fit-smoke") && !self.editing.zoom.cold_done {
            if !self.editing.zoom.cold_started {
                if !exact {
                    return false;
                }
                if std::env::args().any(|a| a == "--detail-prefetch-smoke") {
                    let args: Vec<_> = std::env::args().collect();
                    let dwell_ms = args
                        .iter()
                        .position(|a| a == "--detail-prefetch-dwell-ms")
                        .and_then(|i| args.get(i + 1))
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(1500);
                    let dwell = self
                        .editing
                        .zoom
                        .detail_dwell
                        .get_or_insert_with(Instant::now);
                    // Same fixed observation interval in enabled/disabled runs.
                    // Never wait for speculative completion to make it pass.
                    if dwell.elapsed() < Duration::from_millis(dwell_ms) {
                        self.context
                            .request_repaint_after(Duration::from_millis(25));
                        return false;
                    }
                    let native = self.cache.get(&self.image_key(item, 0));
                    let prepared = self.editing.prepared.get(&item.id);
                    self.editing.zoom.detail_before_zoom = Some(serde_json::json!({
                        "native_ready":native.is_some(),
                        "native_pending":self.pending_images.contains(&self.image_key(item, 0)),
                        "edit_ready":native.zip(prepared).is_some_and(|(n, p)| p.source == n.pyramid.id()),
                        "visible_fit_base":image.base_level(),
                        "decode_jobs":self.service.cache.stats().decode_jobs,
                        "reserved_bytes":self.service.cache.memory.usage().reserved,
                        "memory_limit_bytes":self.service.cache.memory.usage().limit,
                        "disabled":std::env::args().any(|a| a == "--detail-prefetch-off-smoke"),
                        "fixed_fit_observation_ms":dwell_ms,
                        "editing":self.edit_work_statistics(),
                    }));
                }
                self.editing.zoom.first_zoom_at = Some(Instant::now());
                self.editing.zoom.cold_started = true;
                self.editing.zoom.cold_frames.push(serde_json::json!({"phase":"fit","base_level":image.base_level(),"exact":exact}));
                self.state.transform.set_zoom(1.);
                self.context.request_repaint();
                return false;
            }
            self.editing.zoom.cold_frames.push(serde_json::json!({"phase":"load","base_level":image.base_level(),"exact":exact,"held":held}));
            if exact && image.base_level() == 0 {
                self.editing.zoom.cold_done = true;
                self.editing.zoom.first_zoom_ready_ms = self
                    .editing
                    .zoom
                    .first_zoom_at
                    .map(|t| t.elapsed().as_secs_f64() * 1000.);
            } else if held {
                return false;
            } else {
                let _ = std::fs::write(
                    self.root.join("reports/zoom-cold-failure.json"),
                    serde_json::to_vec_pretty(&self.editing.zoom.cold_frames).unwrap(),
                );
                self.status = "Zoom enlarged reduced input during native preparation".into();
                self.fatal = true;
                return false;
            }
        }
        if self.editing.zoom.started.is_none() {
            if !exact || image.base_level() != 0 {
                return false;
            }
            self.editing.zoom.started = Some(Instant::now());
            self.editing.zoom.timing_start = tr_render::timings::mark();
            self.editing.zoom.compute_before =
                Some(serde_json::to_value(self.presenter.statistics()).unwrap());
        }
        self.editing.zoom.frames.push(serde_json::json!({
            "frame":self.frame_number,"step":self.editing.zoom.steps,
            "elapsed_ms":self.editing.zoom.started.unwrap().elapsed().as_secs_f64()*1000.,
            "requested_zoom":self.state.transform.zoom,"exact":exact,"held":held,
            "coverage":coverage.map_or(0.,|c|c.fraction),"base_level":image.base_level(),
            "sample_step":coverage.and_then(|c|c.displayed_region).map(|r|r.step),
            "origin":coverage.and_then(|c|c.displayed_region).map(|r|r.origin),
            "viewport_pixels":coverage.map(|c|c.region.size),
        }));
        self.editing.zoom.failed |= (!exact && !held)
            || image.base_level() != 0
            || !coverage.is_some_and(|c| c.displayed_source == Some(image.id()));
        if !performance && !self.editing.zoom.failed && self.check_zoom_pixels(&image) {
            return false;
        }
        if !self.editing.zoom.failed && self.editing.zoom.steps < total_steps {
            self.context
                .request_repaint_after(Duration::from_millis(16));
            if self
                .editing
                .zoom
                .last_step
                .is_some_and(|t| t.elapsed() < Duration::from_millis(16))
            {
                return false;
            }
            self.editing.zoom.last_step = Some(Instant::now());
            self.editing.zoom.steps += 1;
            let step = self.editing.zoom.steps;
            if step > 120 {
                let direction = if step <= 132 { 1. } else { -1. };
                let region = requested_region.unwrap();
                for (a, delta) in [24., 8.].into_iter().enumerate() {
                    self.state.transform.center[a] += (direction * delta * region.step[a]
                        / f64::from(image.source_size()[a]))
                        as f32;
                }
                return false;
            }
            let zoom = if step <= 30 {
                1. + step as f32 / 30.
            } else if step <= 90 {
                2. - (step - 30) as f32 * 1.63 / 60.
            } else {
                0.37 + (step - 90) as f32 * 0.63 / 30.
            };
            self.state.transform.set_zoom(zoom);
            return false;
        }
        if !self.editing.zoom.failed && (!exact || !self.presenter.is_idle()) {
            return false;
        }
        let distinct = self
            .editing
            .zoom
            .frames
            .iter()
            .filter_map(|f| f["sample_step"][0].as_f64())
            .map(f64::to_bits)
            .collect::<HashSet<_>>()
            .len();
        let passed = !self.editing.zoom.failed
            && self.editing.zoom.steps == total_steps
            && distinct >= 20
            && (performance || self.editing.zoom.pixels.len() == pixel_steps().len())
            && self.editing.zoom.pixels.iter().all(|p| p["passed"] == true);
        let report = serde_json::json!({
            "passed":passed,"steps":self.editing.zoom.steps,"frames":self.editing.zoom.frames,
            "distinct_visible_scales":distinct,"surface_checks":self.editing.zoom.pixels,
            "cold_fit_transition":self.editing.zoom.cold_frames,
            "detail_before_zoom":self.editing.zoom.detail_before_zoom,
            "first_native_zoom_ms":self.editing.zoom.first_zoom_ready_ms,
            "decode_jobs_after_gesture":self.service.cache.stats().decode_jobs,
            "compute":self.presenter.statistics(),
            "renderer_timings":tr_render::timings::samples_since(self.editing.zoom.timing_start),"source_size":image.source_size(),
            "compute_before":self.editing.zoom.compute_before,
            "performance_only":performance,
            "pan_after_zoom":pan_after_zoom,
            "engine":format!("{:?}",self.editing.entries[&item.id].draft.as_ref().unwrap().raw_engine),
            "scope":"120 scale changes from 100% to 200%, down to 37% and back to 100%, paced at least 16 ms apart. Each draw is freshly resampled from the linear source or holds the previous complete scale without resampling. Correctness mode compares four native surface readbacks to the CPU at the displayed scale; performance mode omits CPU references and readbacks during the gesture. At least 20 distinct scales must appear. Software timestamps do not measure physical display latency."
        });
        let _ = std::fs::write(
            self.root.join("reports/zoom-continuity.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        self.editing.zoom.done = true;
        if !passed {
            self.status = "Zoom continuity probe failed".into();
            self.fatal = true;
            return false;
        }
        true
    }
    fn check_zoom_pixels(&mut self, image: &Arc<ImageLevels>) -> bool {
        let step = self.editing.zoom.steps;
        let Some(index) = pixel_steps().iter().position(|s| *s == step) else {
            return false;
        };
        if self.editing.zoom.pixels.len() > index {
            return false;
        }
        // The windowing backend may execute the screenshot one draw after the
        // request. Record the actual displayed regions until receipt, without
        // freezing presentation or substituting the requested scale. The
        // returned surface must match one of these bounded real draw records.
        let held = self
            .presenter
            .coverage()
            .iter()
            .find(|c| c.lane.starts_with("view:single:"))
            .is_some_and(|c| c.held);
        if let Some(c) = self
            .presenter
            .captures()
            .iter()
            .find(|c| c.source == image.id() && c.clip.contains_rect(c.rect))
        {
            let ppp = self.context.pixels_per_point();
            let rect = [
                c.rect.min.x * ppp,
                c.rect.min.y * ppp,
                c.rect.max.x * ppp,
                c.rect.max.y * ppp,
            ];
            let pending = self
                .editing
                .zoom
                .capture
                .get_or_insert_with(|| PendingPixels {
                    name: format!("zoom-{step}"),
                    frames: Vec::new(),
                });
            if pending.frames.len() < 4
                && !pending
                    .frames
                    .iter()
                    .any(|f| f.region == c.region && f.reference.2 == rect)
                && let Ok(expected) = image.render(c.region)
            {
                pending.frames.push(PixelFrame {
                    reference: (expected.to_display(), c.region.size, rect, c.compute),
                    region: c.region,
                    held,
                    frame: self.frame_number,
                });
                if pending.frames.len() == 1 {
                    let name = pending.name.clone();
                    self.capture_screenshot(&self.context.clone(), &name);
                }
            }
        }
        let Some(pending) = self.editing.zoom.capture.as_ref() else {
            return true;
        };
        if !self.screenshots.contains(&pending.name) {
            return true;
        }
        let pending = self.editing.zoom.capture.take().unwrap();
        let result = (|| -> anyhow::Result<_> {
            let screen =
                image::open(self.root.join(format!("reports/{}.png", pending.name)))?.to_rgba8();
            let screen = egui::ColorImage::from_rgba_unmultiplied(
                [screen.width() as usize, screen.height() as usize],
                screen.as_raw(),
            );
            let compared = pending
                .frames
                .iter()
                .enumerate()
                .map(|(index, f)| {
                    navigation::compare(&screen, &f.reference.0, f.reference.1, f.reference.2)
                        .map(|(max, differing)| (max, differing, index))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            compared
                .into_iter()
                .min()
                .ok_or_else(|| anyhow::anyhow!("Missing draw reference"))
        })();
        self.editing.zoom.pixels.push(match result {
            Ok((maximum,differing,index))=>{
                let f=&pending.frames[index];
                serde_json::json!({"step":step,"passed":maximum<=1,"maximum_error_u8":maximum,"differing_channels":differing,"size":f.reference.1,"compute":f.reference.3,"displayed_sample_step":f.region.step,"held":f.held,"matched_draw":f.frame,"draw_candidates":pending.frames.len()})
            },
            Err(e)=>serde_json::json!({"step":step,"passed":false,"error":e.to_string()}),
        });
        false
    }
}
