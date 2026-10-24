//! Optional native detail for the active photo, after its fitted view is ready.
use super::*;

const DWELL: Duration = Duration::from_millis(180);

#[derive(Default)]
pub(super) struct DetailPrefetch {
    targets: HashMap<(String, PreviewRequest), Target>,
    views: Vec<(Item, PreviewRequest)>,
}
struct Target {
    generation: u64,
    observation: String,
    since: Instant,
    attempted: bool,
}
impl DetailPrefetch {
    pub(super) fn begin_frame(&mut self) {
        self.views.clear();
    }
    pub(super) fn observe(&mut self, item: &Item, request: PreviewRequest) {
        let request = PreviewRequest { edge: 0, ..request };
        if self.views.len() < 2
            && !self
                .views
                .iter()
                .any(|(i, r)| i.id == item.id && *r == request)
        {
            self.views.push((item.clone(), request));
        }
    }
    pub(super) fn contains(&self, key: &(String, PreviewRequest)) -> bool {
        self.targets.contains_key(key)
    }
}

impl TrueRenderer {
    pub(super) fn reset_detail_prefetch(&mut self) {
        self.detail_prefetch.targets.clear();
        self.retain_prepared_edits(&HashSet::new());
    }

    pub(super) fn active_detail_demand(&mut self, ctx: &egui::Context) {
        let settings = self.service.cache.settings();
        if self.state.view == ViewMode::Grid
            || self.preparation_paused
            || self.scanning
            || self.folder_loading_blocks()
            || self.clearing_current_folder()
            || self.service.cache.under_pressure()
            || (settings.adapt_on_battery && self.service.cache.on_battery())
            || (self.smoke && std::env::args().any(|a| a == "--detail-prefetch-off-smoke"))
        {
            self.reset_detail_prefetch();
            return;
        }
        // At most the two actual viewer lanes, never filmstrip thumbnails or
        // every photo in the folder. Resolve the same quality/RAW recipe as UI.
        let candidates: Vec<_> = self
            .detail_prefetch
            .views
            .clone()
            .into_iter()
            .filter(|(item, _)| item.approved)
            .map(|(item, request)| {
                let key = (item.id.clone(), request);
                (item, key)
            })
            .collect();
        self.detail_prefetch.targets.retain(|key, target| {
            target.generation == self.generation
                && candidates
                    .iter()
                    .any(|(item, current)| current == key && item.observation == target.observation)
        });
        for (item, key) in &candidates {
            self.detail_prefetch
                .targets
                .entry(key.clone())
                .or_insert_with(|| Target {
                    generation: self.generation,
                    observation: item.observation.clone(),
                    since: Instant::now(),
                    attempted: false,
                });
        }
        let sources = candidates
            .iter()
            .filter_map(|(_, key)| self.cache.get(key).map(|c| (key.0.clone(), c.pyramid.id())))
            .collect();
        self.retain_prepared_edits(&sources);

        // Foreground demand promotes an existing request through the ordinary
        // scheduler. It can consume a prepared edit without redoing it.
        if self.state.transform.zoom.is_some() {
            return;
        }
        let visible_ready = self.demand.iter().all(|key| {
            self.cache.contains_key(key)
                || self.errors.contains_key(&format!("{}:{:?}", key.0, key.1))
        });
        let memory = self.service.cache.memory.usage();
        let headroom = memory.reserved
            <= memory
                .limit
                .saturating_sub(self.service.cache.background_headroom());
        for (item, key) in candidates {
            if let Some(cached) = self.cache.get(&key).cloned() {
                self.detail_prefetch
                    .targets
                    .get_mut(&key)
                    .unwrap()
                    .attempted = true;
                // Do not touch/pin this optional source, or replace the fitted
                // preview. Reclamation must be able to release both copies.
                let remaining =
                    DWELL.saturating_sub(self.detail_prefetch.targets[&key].since.elapsed());
                if !remaining.is_zero() {
                    ctx.request_repaint_after(remaining);
                } else if visible_ready
                    && headroom
                    && self.presenter.is_idle()
                    && self.preview_request(&item, 0) == key.1
                {
                    self.prefetch_edit_detail(&item.id, &cached.digest, cached.pyramid);
                }
                continue;
            }
            if self.pending_images.contains(&key) {
                // The admitted job owns credits: low remaining headroom alone
                // must not repeatedly cancel/restart that same development.
                // Also promote bulk folder work already queued for this photo.
                self.ensure_preview_request(&item, key.1, PreviewPriority::AheadRegion);
                self.detail_prefetch
                    .targets
                    .get_mut(&key)
                    .unwrap()
                    .attempted = true;
                continue;
            }
            if self.edit_interactive(&item.id) {
                self.detail_prefetch.targets.get_mut(&key).unwrap().since = Instant::now();
                continue;
            }
            let target = self.detail_prefetch.targets.get_mut(&key).unwrap();
            if target.attempted {
                continue; // Eviction/deferral must not create a decode loop.
            }
            let remaining = DWELL.saturating_sub(target.since.elapsed());
            if !remaining.is_zero() {
                ctx.request_repaint_after(remaining);
                continue;
            }
            if !visible_ready
                || !headroom
                || !self.presenter.is_idle()
                || self.pending_images.len() + self.demand_jobs.len() >= 2
                || !self
                    .editing
                    .entries
                    .get(&item.id)
                    .is_some_and(|e| e.loaded.is_some())
            {
                continue;
            }
            self.ensure_preview_request(&item, key.1, PreviewPriority::AheadRegion);
            if self
                .demand_jobs
                .iter()
                .any(|j| j.item.id == key.0 && j.request == key.1)
                || self.cache.contains_key(&key)
            {
                self.detail_prefetch
                    .targets
                    .get_mut(&key)
                    .unwrap()
                    .attempted = true;
            }
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
pub(super) mod tests;
