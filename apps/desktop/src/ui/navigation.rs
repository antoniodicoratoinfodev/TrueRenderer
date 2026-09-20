//! Opt-in generated-corpus probe. Readback receipt is an observable software
//! boundary, not a timestamp from the compositor or the physical display.
use super::*;
use anyhow::{Result, ensure};
use tr_core::resample::Region;

const TRACE: [&str; 3] = [
    "04_Frequenze_radiali.png",
    "02_Paesaggio_analitico.png",
    "04_Frequenze_radiali.png",
];
const ACTIONS: [&str; 10] = [
    "select",
    "select",
    "return",
    "zoom-300",
    "pan",
    "fit",
    "photo-standard",
    "photo-full",
    "physical-1to1",
    "fit-standard",
];

struct Pending {
    token: String,
    source: Arc<ImageLevels>,
    region: Region,
    rect: [f32; 4],
    compute: &'static str,
    encoded_ms: f64,
}

pub(super) struct Probe {
    started: Instant,
    trace: [String; 3],
    pressure: bool,
    pressure_before: usize,
    pressure_after: usize,
    event: Option<Instant>,
    pending: Option<Pending>,
    rows: Vec<serde_json::Value>,
    before: crate::cache::Statistics,
    resident: bool,
    done: bool,
    transitions: bool,
    redraws: usize,
    reprojections: usize,
    minimum_coverage: f32,
}
impl Probe {
    pub fn new() -> Self {
        let args: Vec<_> = std::env::args().collect();
        let target = |flag: &str, fallback: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| fallback.into())
        };
        let first = target("--navigation-first", TRACE[0]);
        Self {
            trace: [
                first.clone(),
                target("--navigation-second", TRACE[1]),
                first,
            ],
            pressure: args.iter().any(|a| a == "--navigation-pressure"),
            pressure_before: 0,
            pressure_after: 0,
            started: Instant::now(),
            event: None,
            pending: None,
            rows: vec![],
            before: Default::default(),
            resident: false,
            done: false,
            transitions: std::env::args().any(|a| a == "--navigation-transitions-smoke"),
            redraws: 0,
            reprojections: 0,
            minimum_coverage: 1.,
        }
    }
    fn steps(&self) -> usize {
        if self.transitions {
            ACTIONS.len()
        } else {
            TRACE.len()
        }
    }
    fn target(&self) -> &str {
        &self.trace[self.rows.len().min(2)]
    }
}

fn compare(
    screen: &egui::ColorImage,
    expected: &[u8],
    size: [u32; 2],
    rect: [f32; 4],
) -> Result<(u8, u64)> {
    ensure!(size[0] > 0 && size[1] > 0, "Empty capture");
    ensure!(
        rect.iter()
            .all(|v| v.is_finite() && *v >= 0. && (*v - v.round()).abs() < 0.001)
            && (rect[2] - rect[0] - size[0] as f32).abs() < 0.001
            && (rect[3] - rect[1] - size[1] as f32).abs() < 0.001
            && rect[2] <= screen.width() as f32
            && rect[3] <= screen.height() as f32,
        "Capture is not aligned to the physical surface"
    );
    ensure!(
        expected.len() == size[0] as usize * size[1] as usize * 4,
        "Reference size mismatch"
    );
    let mut maximum = 0;
    let mut differing = 0;
    for y in 0..size[1] as usize {
        for x in 0..size[0] as usize {
            let actual = screen.pixels
                [(rect[1] as usize + y) * screen.width() + rect[0] as usize + x]
                .to_array();
            for c in 0..4 {
                let difference = actual[c].abs_diff(expected[(y * size[0] as usize + x) * 4 + c]);
                maximum = maximum.max(difference);
                differing += u64::from(difference != 0);
            }
        }
    }
    Ok((maximum, differing))
}

impl TrueRenderer {
    fn navigation_finish(&mut self, ctx: &egui::Context, error: Option<String>) {
        let Some(probe) = self.navigation_probe.as_mut() else {
            return;
        };
        probe.done = true;
        let passed = error.is_none() && probe.rows.len() == probe.steps();
        let report = serde_json::json!({
            "application":"TrueRenderer", "version":env!("CARGO_PKG_VERSION"),
            "passed":passed, "error":error, "samples":probe.rows,
            "adapter":self.adapter, "surface":self.surface,
            "pixels_per_point":ctx.pixels_per_point(), "settings":self.service.cache.settings(),
            "compute":self.presenter.statistics(), "trace":probe.trace,
            "pressure_injected":probe.pressure, "pressure_wide_before":probe.pressure_before, "pressure_wide_after":probe.pressure_after,
            "transitions":probe.transitions,
            "actions":&ACTIONS[..probe.steps()],
            "inflight_step":probe.rows.len(), "inflight_redraws":probe.redraws,
            "inflight_minimum_draw_coverage":probe.minimum_coverage,
            "continuity_scope":"Per-redraw image draw-command coverage during same-source transitions. Partial overlap is recorded; zero coverage fails. No per-refresh compositor/display continuity claim.",
            "scope":"Synthetic selection/transform/photo-quality actions through the production viewer. Event timestamp precedes the action; encoded timestamp means the exact requested raster was added to egui, before surface submit. Readback receipt includes surface rendering, GPU readback and event delivery; excludes subsequent CPU reference verification. Neither timestamp measures physical display presentation. Listing and GPU qualification precede the trace. Prefetch, filmstrip and inspector disabled. Steps within each process are dependent, not independent p95/p99 samples."
        });
        if let Err(e) = std::fs::write(
            self.root.join("reports/navigation-surface.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        ) {
            eprintln!("Navigation report: {e}");
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    pub(super) fn navigation_receive(&mut self, ctx: &egui::Context) {
        let Some(probe) = self.navigation_probe.as_mut() else {
            return;
        };
        let received = Instant::now();
        let Some(pending) = probe.pending.as_ref() else {
            return;
        };
        let screen = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Screenshot {
                    user_data, image, ..
                } = event
                {
                    let token = user_data.data.as_ref()?.downcast_ref::<String>()?;
                    (token == &pending.token).then(|| image.clone())
                } else {
                    None
                }
            })
        });
        let Some(screen) = screen else { return };
        let pending = probe.pending.take().unwrap();
        let readback_ms = received.duration_since(probe.event.unwrap()).as_secs_f64() * 1000.;
        // Reference rendering is deliberately AFTER the endpoint timestamp.
        let check = pending.source.render(pending.region).and_then(|reference| {
            compare(
                &screen,
                &reference.to_display(),
                pending.region.size,
                pending.rect,
            )
        });
        match check {
            Ok((maximum, differing))
                if maximum <= 1
                    && (probe.rows.len() != 8
                        || !probe.transitions
                        || (maximum == 0 && pending.region.step == [1., 1.])) =>
            {
                let after = self.service.cache.stats();
                probe.rows.push(serde_json::json!({
                    "step":probe.rows.len(), "source":probe.target(), "action":ACTIONS[probe.rows.len()],
                    "event_to_encoded_ms":pending.encoded_ms, "event_to_readback_ms":readback_ms,
                    "resident_at_command":probe.resident,
                    "cache_hits":after.hits.saturating_sub(probe.before.hits),
                    "decode_jobs":after.decode_jobs.saturating_sub(probe.before.decode_jobs),
                    "compute":pending.compute, "size":pending.region.size,
                    "origin":pending.region.origin, "step_size":pending.region.step,
                    "source_dimensions":[pending.source.source().width,pending.source.source().height],
                    "source_size":pending.source.source_size(), "base_level":pending.source.base_level(),
                    "memory_reserved":self.service.cache.memory.usage().reserved,
                    "memory_peak":self.service.cache.memory.usage().peak,
                    "memory_limit":self.service.cache.memory.usage().limit,
                    "admission_rejections":self.service.cache.memory.usage().rejected,
                    "presenter":self.presenter.statistics(),
                    "max_channel_error_u8":maximum, "differing_channels":differing, "passed":true
                    ,"redraws":probe.redraws, "reprojected_redraws":probe.reprojections,
                    "minimum_draw_coverage":probe.minimum_coverage,
                    "effective_quality":self.state.current_item().map(|item| self.quality_overrides.get(&item.id).copied().unwrap_or(self.cache_settings.quality)),
                    "physical_1to1_exact": if ACTIONS[probe.rows.len()] == "physical-1to1" { Some(maximum == 0 && pending.region.step == [1.,1.]) } else { None }
                }));
                probe.event = None;
                if probe.rows.len() == probe.steps() {
                    self.navigation_finish(ctx, None);
                }
            }
            Ok((maximum, _)) => self.navigation_finish(
                ctx,
                Some(format!(
                    "Surface pixel/physical 1:1 check failed (maximum error {maximum})"
                )),
            ),
            Err(e) => self.navigation_finish(ctx, Some(format!("{e:#}"))),
        }
    }

    pub(super) fn navigation_begin(&mut self, ctx: &egui::Context) -> bool {
        let Some(probe) = self.navigation_probe.as_ref() else {
            return true;
        };
        if probe.done {
            return false;
        }
        if probe.started.elapsed() > Duration::from_secs(180)
            || self.fatal
            || !self.errors.is_empty()
            || self.presenter.has_errors()
        {
            self.navigation_finish(
                ctx,
                Some(format!(
                    "Timeout or application/render error: {:?}",
                    self.errors
                )),
            );
            return false;
        }
        if self.scanning || self.gpu_status == "Diagnostica GPU in corso…" {
            ctx.request_repaint_after(Duration::from_millis(10));
            return false; // No image demand may precede the first command.
        }
        if !self.gpu_passed
            || self.service.cache.settings().prefetch != crate::cache::Prefetch::Disabled
        {
            self.navigation_finish(
                ctx,
                Some(
                    "Probe requires verified GPU and disabled prefetch in isolated settings".into(),
                ),
            );
            return false;
        }
        if probe.event.is_none() {
            let step = probe.rows.len();
            let target = probe.target();
            let Some(item) = self.state.items.iter().find(|i| i.name == target).cloned() else {
                self.navigation_finish(ctx, Some("Generated corpus target missing".into()));
                return false;
            };
            self.show_inspector = false;
            self.show_filmstrip = false;
            self.state.view = ViewMode::Preview;
            if step < 3 {
                self.state.transform = ViewTransform::default();
            }
            let probe = self.navigation_probe.as_mut().unwrap();
            if probe.pressure && step == 4 {
                probe.pressure_before = self.presenter.statistics().wide_frames;
                self.presenter.set_pressure(true);
                probe.pressure_after = self.presenter.statistics().wide_frames;
            }
            probe.before = self.service.cache.stats();
            probe.resident = self.cache.keys().any(|(id, _)| id == &item.id);
            probe.event = Some(Instant::now());
            probe.redraws = 0;
            probe.reprojections = 0;
            probe.minimum_coverage = 1.;
            match step {
                0..=2 => self.command(Command::Select {
                    id: item.id,
                    extend: false,
                }),
                3 => self.state.transform.set_zoom(3.),
                4 => self.state.transform.center = [0.55, 0.47],
                5 => self.state.transform = ViewTransform::default(),
                6 => self.set_photo_quality(&item.id, Some(PreviewQuality::Standard)),
                7 => self.set_photo_quality(&item.id, Some(PreviewQuality::Full)),
                8 => {
                    self.state.transform.set_zoom(1.);
                    self.full_for_current();
                }
                9 => {
                    self.state.transform = ViewTransform::default();
                    self.set_photo_quality(&item.id, Some(PreviewQuality::Standard));
                }
                _ => unreachable!(),
            }
        }
        if let Some(probe) = &self.navigation_probe
            && probe.pressure
        {
            // Renderer policy injection only; do not pretend this is an OS warning.
            self.presenter
                .set_pressure((4..=5).contains(&probe.rows.len()));
        }
        ctx.request_repaint_after(Duration::from_millis(10));
        true
    }

    pub(super) fn navigation_capture(&mut self, ctx: &egui::Context) {
        let Some(probe) = self.navigation_probe.as_mut() else {
            return;
        };
        if probe.done || probe.event.is_none() {
            return;
        }
        let Some(item) = self.state.current_item() else {
            return;
        };
        if probe.transitions && probe.rows.len() >= 3 {
            let coverage = self.presenter.coverage().iter().find(|record| {
                self.cache
                    .iter()
                    .any(|((id, _), cached)| id == &item.id && cached.pyramid.id() == record.source)
            });
            probe.redraws += 1;
            let fraction = coverage.map_or(0., |c| c.fraction);
            probe.minimum_coverage = probe.minimum_coverage.min(fraction);
            probe.reprojections +=
                usize::from(coverage.is_some_and(|c| !c.exact && c.fraction > 0.));
            if fraction <= 0. {
                self.navigation_finish(
                    ctx,
                    Some("Same-source transition produced an empty image draw".into()),
                );
                return;
            }
        }
        if probe.pending.is_some() {
            return;
        }
        // Only an exact current demand is acceptable: no previous/reprojected
        // frame, lower-quality fallback or thumbnail can terminate the sample.
        let Some(cached) = self
            .demand
            .iter()
            .filter(|(id, _)| id == &item.id)
            .find_map(|key| self.cache.get(key))
        else {
            return;
        };
        let Some(capture) = self
            .presenter
            .captures()
            .iter()
            .find(|c| c.source == cached.pyramid.id() && c.clip.contains_rect(c.rect))
        else {
            return;
        };
        let ppp = ctx.pixels_per_point();
        let pending = Pending {
            token: format!("navigation-{}", probe.rows.len()),
            source: cached.pyramid.clone(),
            region: capture.region,
            rect: [
                capture.rect.min.x * ppp,
                capture.rect.min.y * ppp,
                capture.rect.max.x * ppp,
                capture.rect.max.y * ppp,
            ],
            compute: capture.compute,
            encoded_ms: probe.event.unwrap().elapsed().as_secs_f64() * 1000.,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
            pending.token.clone(),
        )));
        self.navigation_probe.as_mut().unwrap().pending = Some(pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surface_check_rejects_wrong_pixels_and_unaligned_geometry() {
        let screen = egui::ColorImage::filled([4, 4], Color32::WHITE);
        let reference = vec![255; 16];
        assert_eq!(
            compare(&screen, &reference, [2, 2], [1., 1., 3., 3.]).unwrap(),
            (0, 0)
        );
        assert!(compare(&screen, &reference, [2, 2], [1.5, 1., 3.5, 3.]).is_err());
        assert!(compare(&screen, &reference, [2, 2], [3., 3., 5., 5.]).is_err());
        assert_eq!(
            compare(&screen, &[0; 16], [2, 2], [1., 1., 3., 3.])
                .unwrap()
                .0,
            255
        );
    }
}
