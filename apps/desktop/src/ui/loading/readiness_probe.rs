//! Opt-in native regression on an isolated catalogue and copied photographs.
use super::*;

#[derive(Default)]
pub(super) struct Probe {
    prepared: Option<serde_json::Value>,
    step: usize,
    since: Option<Instant>,
    frame: u64,
    visits: Vec<serde_json::Value>,
}

impl TrueRenderer {
    pub(in crate::ui) fn folder_readiness_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let failed = self.fatal
            || self.folder_load.errors > 0
            || self.presenter.has_errors()
            || self.started.elapsed() > Duration::from_secs(600);
        if failed || self.smoke_stage == 4 {
            let p = &self.loading_probe.readiness;
            let stats = self.service.cache.stats();
            let editing = self.edit_work_statistics();
            let prepared = p.prepared.as_ref();
            let report = serde_json::json!({
                "passed": !failed && self.smoke_stage == 4,
                "prepared":prepared,"visits":p.visits,"cache":stats,"editing":editing,
                "new_decodes_after_loading":prepared.map(|v| stats.decode_jobs.saturating_sub(v["decode_jobs"].as_u64().unwrap())),
                "new_edit_renders_after_loading":prepared.map(|v| editing["renders"].as_u64().unwrap().saturating_sub(v["editing"]["renders"].as_u64().unwrap())),
                "folder_errors":self.folder_load.errors,"errors":self.errors,
                "elapsed_seconds":self.started.elapsed().as_secs_f64(),
                "memory":{"reserved":self.service.cache.memory.usage().reserved,"limit":self.service.cache.memory.usage().limit},
                "scope":"Isolated native folder preparation followed by exact Fit, native zoom and pan for every edited photo. Software readiness times, not physical display latency; retention depends on quotas."
            });
            let _ = std::fs::write(
                self.root.join("reports/folder-readiness.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.scanning || self.state.items.is_empty() {
            return;
        }
        match self.smoke_stage {
            0 => {
                let items = self.state.items.clone();
                for item in &items {
                    self.ensure_edit_loaded(item);
                }
                if !items.iter().all(|i| {
                    self.editing
                        .entries
                        .get(&i.id)
                        .is_some_and(|e| e.loaded.is_some())
                }) {
                    return;
                }
                for item in &items {
                    let mut recipe =
                        tr_core::editing::EditRecipe::neutral(self.cache_settings.raw_engine);
                    recipe.exposure_ev = 0.35;
                    recipe.curve = vec![
                        tr_core::editing::CurvePoint { x: 0., y: 0. },
                        tr_core::editing::CurvePoint { x: 0.4, y: 0.55 },
                        tr_core::editing::CurvePoint { x: 1., y: 1. },
                    ];
                    if self.folder_edit_recipe(&item.id).as_ref() != Some(&recipe) {
                        self.apply_edit_draft(&item.id, recipe);
                        self.commit_edit(&item.id);
                    }
                }
                self.smoke_stage = 1;
            }
            1 => {
                if self
                    .editing
                    .entries
                    .values()
                    .any(|e| e.pending || e.dirty())
                    || self.editing_has_priority()
                {
                    return;
                }
                self.smoke_stage = 2;
                self.begin_folder_loading();
                self.start_folder_preparation(true);
            }
            2 if self.rebuild.is_empty()
                && !self.folder_load.finishing
                && self.pending_images.is_empty()
                && !self.service.cache.preview_writes_pending() =>
            {
                self.loading_probe.readiness.prepared = Some(serde_json::json!({
                    "seconds":self.started.elapsed().as_secs_f64(),
                    "decode_jobs":self.service.cache.stats().decode_jobs,
                    "editing":self.edit_work_statistics(),"photos":self.state.items.len(),
                    "memory":{"reserved":self.service.cache.memory.usage().reserved,"limit":self.service.cache.memory.usage().limit},
                }));
                self.smoke_stage = 3;
                self.readiness_visit();
            }
            3 => {
                let p = &self.loading_probe.readiness;
                if self.frame_number <= p.frame + 1 {
                    return;
                }
                let id = &self.state.items[p.step / 3].id;
                let Some(image) = self.ready_edited_image(id) else {
                    return;
                };
                let exact = self.presenter.coverage().iter().any(|c| {
                    c.lane.starts_with("view:single:")
                        && c.exact
                        && c.displayed_source == Some(image.id())
                });
                let expected_base = p.step.is_multiple_of(3)
                    || image.base_level() == 0
                    || self.cache_settings.quality == PreviewQuality::Standard;
                if !exact || !expected_base || !self.presenter.is_idle() {
                    return;
                }
                let elapsed = p.since.unwrap().elapsed().as_secs_f64() * 1000.;
                let step = p.step;
                self.loading_probe.readiness.visits.push(serde_json::json!({
                    "photo":step / 3,"action":(["fit","native","pan"][step % 3]),
                    "ready_ms":elapsed,"base":image.base_level(),"exact":exact,
                }));
                self.loading_probe.readiness.step += 1;
                if self.loading_probe.readiness.step == self.state.items.len() * 3 {
                    self.smoke_stage = 4;
                } else {
                    self.readiness_visit();
                }
            }
            _ => {}
        }
    }

    fn readiness_visit(&mut self) {
        let p = &mut self.loading_probe.readiness;
        self.state.current = Some(self.state.items[p.step / 3].id.clone());
        self.state.view = ViewMode::Preview;
        self.state.transform.zoom = if p.step.is_multiple_of(3) {
            None
        } else {
            Some(1.)
        };
        self.state.transform.center = if p.step % 3 == 2 {
            [0.57, 0.53]
        } else {
            [0.5, 0.5]
        };
        p.since = Some(Instant::now());
        p.frame = self.frame_number;
    }
}
