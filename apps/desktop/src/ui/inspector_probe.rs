//! Native inspector layout captures on an isolated generated corpus and data root.
use super::*;

impl TrueRenderer {
    pub(super) fn inspector_layout_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(50));
        let report_path = self.root.join("reports/inspector-ui.json");
        if self.started.elapsed() > Duration::from_secs(90) || self.fatal {
            let report = serde_json::json!({
                "passed": false,
                "reason": if self.fatal { "fatal error" } else { "timeout" },
                "stage": self.smoke_stage,
                "elapsed_seconds": self.started.elapsed().as_secs_f64(),
                "scanning": self.scanning,
                "status": self.status,
                "decode_errors": self.errors,
                "presentation_errors": self.presenter.has_errors(),
                "gpu_verified": self.gpu_passed,
            });
            let _ = std::fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let Some(item) = self
            .state
            .items
            .iter()
            .find(|item| item.name == "02_Paesaggio_analitico.png")
            .cloned()
        else {
            return;
        };
        let index = usize::from(self.smoke_stage / 10);
        let phase = self.smoke_stage % 10;
        if index == 10 {
            let layouts: Vec<serde_json::Value> = (0..10)
                .filter_map(|case| {
                    let bytes = std::fs::read(
                        self.root
                            .join(format!("reports/inspector-{case:02}-layout.json")),
                    )
                    .ok()?;
                    serde_json::from_slice(&bytes).ok()
                })
                .collect();
            let source_baseline = std::fs::read(self.root.join("reports/inspector-source.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            let final_digest = tr_platform::snapshot(&item.path)
                .ok()
                .map(|(_, digest)| digest);
            let source_unchanged = source_baseline.as_ref().is_some_and(|baseline| {
                baseline["name"] == item.name
                    && baseline["digest"]
                        .as_str()
                        .is_some_and(|digest| final_digest.as_deref() == Some(digest))
            });
            let screenshots: Vec<_> = (0..10)
                .map(|case| format!("inspector-{case:02}"))
                .filter(|name| self.screenshots.contains(name))
                .collect();
            let report = serde_json::json!({
                "passed": screenshots.len() == 10
                    && layouts.len() == 10
                    && layouts.iter().all(|layout| layout["passed"] == true)
                    && source_unchanged
                    && !self.fatal
                    && self.errors.is_empty()
                    && !self.presenter.has_errors()
                    && self.gpu_passed,
                "screenshots": screenshots,
                "layouts": layouts,
                "source": item.name,
                "source_digest": final_digest,
                "source_unchanged": source_unchanged,
                "gpu_verified": self.gpu_passed,
                "decode_errors": self.errors,
                "presentation_errors": self.presenter.has_errors(),
                "scope": "Native macOS generated corpus: Information/Develop inspector in IT/EN, plus compact grid Information in both languages. Requested wide window 1440x940, 100% UI; macOS may constrain it to the display, actual viewport recorded per case. Compact window 1100x720, 200% UI (550x360 logical). Captures cover the header, tabs and initial scroll position; they do not qualify screen readers, all scrolled content, Windows or display accuracy.",
            });
            let _ = std::fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let compact = index >= 4;
        let language = if (index < 8 && index % 4 < 2) || index == 8 {
            Language::English
        } else {
            Language::Italian
        };
        let grid = index >= 8;
        let page = if grid || index.is_multiple_of(2) {
            InspectorPage::Information
        } else {
            InspectorPage::Develop
        };
        let size = if compact {
            [1100., 720.]
        } else {
            [1440., 940.]
        };
        let zoom = if compact { 2. } else { 1. };
        if phase == 0 {
            if self.scanning {
                return;
            }
            if index == 0 {
                let source = tr_platform::snapshot(&item.path)
                    .ok()
                    .map(|(_, digest)| digest);
                let baseline = serde_json::json!({ "name": item.name, "digest": source });
                let _ = std::fs::write(
                    self.root.join("reports/inspector-source.json"),
                    serde_json::to_vec_pretty(&baseline).unwrap(),
                );
                self.command(Command::Select {
                    id: item.id.clone(),
                    extend: false,
                });
                self.state.transform = ViewTransform::default();
            }
            self.set_language(language);
            self.state.view = if grid {
                ViewMode::Grid
            } else {
                ViewMode::Preview
            };
            self.show_settings = false;
            self.show_help = false;
            self.show_inspector = true;
            self.browser.session.visible = false;
            self.inspector_pages[usize::from(!grid)] = page;
            // Resize once per physical layout before changing zoom. Repeating
            // InnerSize at 200% would enlarge the native window for later cases.
            if index == 0 || index == 4 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    size[0], size[1],
                )));
                ctx.set_zoom_factor(zoom);
            }
            self.smoke_layout = None;
            self.smoke_stage += 1;
            return;
        }

        self.ensure_edit_loaded(&item);
        let edit_ready = self.editing.entries.get(&item.id).is_some_and(|entry| {
            entry.loaded.is_some() && !entry.loading && !entry.pending && entry.error.is_none()
        });
        let source_ready = self
            .demand
            .iter()
            .any(|key| key.0 == item.id && self.cache.contains_key(key));
        if self.scanning
            || !edit_ready
            || !source_ready
            || !self.presenter.is_idle()
            || self.demand.iter().any(|key| !self.cache.contains_key(key))
        {
            self.smoke_layout = None;
            return;
        }
        if phase < 5 {
            self.smoke_stage += 1;
            return;
        }
        let name = format!("inspector-{index:02}");
        if self.screenshots.contains(&name) {
            self.smoke_stage = ((index + 1) * 10) as u8;
            self.smoke_layout = None;
            return;
        }
        let viewport = ctx.content_rect();
        let geometry = format!(
            "{:?}:{:?}:{}:{}",
            viewport,
            self.presenter
                .captures()
                .iter()
                .map(|capture| { (capture.source, capture.rect, capture.clip, capture.region) })
                .collect::<Vec<_>>(),
            zoom,
            self.show_inspector,
        );
        if !self
            .smoke_layout
            .as_ref()
            .is_some_and(|(stage, previous, _)| *stage == self.smoke_stage && previous == &geometry)
        {
            self.smoke_layout = Some((self.smoke_stage, geometry, Instant::now()));
            return;
        }
        if self
            .smoke_layout
            .as_ref()
            .is_some_and(|(_, _, since)| since.elapsed() < Duration::from_millis(400))
        {
            return;
        }
        let layout = serde_json::json!({
            "case": index,
            "language": if language == Language::English { "en" } else { "it" },
            "page": if page == InspectorPage::Information { "information" } else { "develop" },
            "view": if grid { "grid" } else { "preview" },
            "viewport_logical": [viewport.width(), viewport.height()],
            "requested_physical_size": size,
            "zoom": ctx.zoom_factor(),
            "scroll": "initial position",
            "passed": (if compact {
                    (viewport.width() - size[0] / zoom).abs() <= 3.
                        && (viewport.height() - size[1] / zoom).abs() <= 3.
                } else {
                    (1000. ..=size[0] + 3.).contains(&viewport.width())
                        && (700. ..=size[1] + 3.).contains(&viewport.height())
                })
                && (ctx.zoom_factor() - zoom).abs() <= 0.01
                && self.show_inspector
                && !self.presenter.captures().is_empty(),
        });
        let _ = std::fs::write(
            self.root.join(format!("reports/{name}-layout.json")),
            serde_json::to_vec_pretty(&layout).unwrap(),
        );
        self.capture_screenshot(ctx, &name);
    }
}
