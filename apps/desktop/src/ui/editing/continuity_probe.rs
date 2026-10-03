//! Opt-in native temporal probe, used with the isolated development smoke.
use super::*;

#[derive(Default)]
pub(super) struct Probe {
    baseline: Option<EditRecipe>,
    steps: u32,
    frames: Vec<serde_json::Value>,
    done: bool,
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
    pub(super) fn edit_continuity_tick(&mut self, item: &Item) -> bool {
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
        }
        let original_returned = displayed
            .and_then(|c| c.displayed_source)
            .is_some_and(|source| self.cache.values().any(|c| c.pyramid.id() == source));
        let covered = displayed.map_or(0., |c| c.fraction);
        self.editing.continuity.frames.push(serde_json::json!({
            "frame":self.frame_number,"step":self.editing.continuity.steps,
            "coverage":covered,"exact":displayed.is_some_and(|c| c.exact),
            "displayed_source":displayed.and_then(|c| c.displayed_source),
            "original_returned":original_returned,
        }));
        let failed = covered < 0.999 || original_returned;
        if !failed && self.editing.continuity.steps < 40 {
            self.editing.continuity.steps += 1;
            let step = self.editing.continuity.steps;
            let mut recipe = self.editing.continuity.baseline.as_ref().unwrap().clone();
            recipe.exposure_ev = 0.25 + (step % 12) as f32 * 0.1;
            recipe.brightness = (step % 16) as f32 * 2. - 12.;
            self.apply_edit_draft(&item.id, recipe);
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
        let report = serde_json::json!({
            "passed":!failed && current,"steps":self.editing.continuity.steps,
            "proof":self.output_proof_for(&item.id),"frames":self.editing.continuity.frames,
            "compute":self.presenter.statistics(),
            "scope":"Native draw coverage during 40 exposure/brightness draft changes and final presentation. No blank frame, no return to the original. Not physical-display or p95 qualification. Final screenshot/export checked by the enclosing development smoke."
        });
        let _ = std::fs::write(
            self.root.join("reports/edit-continuity.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        self.editing.continuity.done = true;
        if failed {
            self.status = "Edit continuity probe failed".into();
            self.fatal = true;
            return false;
        }
        self.commit_edit(&item.id);
        self.editing.smoke_ready_at = None;
        true
    }
}
