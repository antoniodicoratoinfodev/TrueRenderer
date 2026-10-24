use super::*;
use std::sync::atomic::AtomicU64;

#[derive(Default)]
pub(super) struct WorkStats {
    pub hits: AtomicU64,
    pub renders: AtomicU64,
    pub cancelled: AtomicU64,
    pub renders_by_base: [AtomicU64; 32],
}

impl TrueRenderer {
    pub(in crate::ui) fn editing_has_priority(&self) -> bool {
        self.editing.crop.is_some()
            || self.editing.looks.previewing()
            || self.editing.entries.iter().any(|(id, entry)| {
                self.edit_interactive(id)
                    || entry
                        .draft
                        .as_ref()
                        .is_some_and(|recipe| self.editing.layers.gesture_active(id, recipe))
            })
    }

    pub(in crate::ui) fn ready_edited_image(&self, id: &str) -> Option<Arc<ImageLevels>> {
        let preview = self.editing.previews.get(id)?;
        (self.view_recipe(id).as_ref() == Some(&preview.recipe)).then(|| preview.image.clone())
    }

    pub(in crate::ui) fn edit_work_statistics(&self) -> serde_json::Value {
        serde_json::json!({
            "cache_hits": self.editing.work_stats.hits.load(Ordering::Relaxed),
            "renders": self.editing.work_stats.renders.load(Ordering::Relaxed),
            "cancelled": self.editing.work_stats.cancelled.load(Ordering::Relaxed),
            "renders_by_base": self.editing.work_stats.renders_by_base.iter().map(|v|v.load(Ordering::Relaxed)).collect::<Vec<_>>(),
        })
    }

    pub(in crate::ui) fn folder_edit_recipe(&self, id: &str) -> Option<EditRecipe> {
        let entry = self.editing.entries.get(id)?;
        entry
            .draft
            .clone()
            .or_else(|| entry.loaded.as_ref().map(|e| e.recipe.clone()))
    }

    pub(in crate::ui) fn cancel_folder_edit(&mut self) {
        if self.editing.folder_preparing {
            self.cancel_edit_refinement();
        }
    }

    pub(super) fn edit_artifact(
        &self,
        id: &str,
        digest: &str,
        source: &ImageLevels,
        index: usize,
        recipe: &EditRecipe,
    ) -> Option<crate::cache::edited::Artifact> {
        if !self.service.cache.settings().enabled || source.scientific() {
            return None;
        }
        let folder = self
            .known_items()
            .find(|item| item.id == id)?
            .path
            .parent()?
            .to_owned();
        let input_request = self
            .cache
            .iter()
            .find(|((photo, _), c)| {
                photo == id && c.digest == digest && c.pyramid.id() == source.id()
            })?
            .0
            .1;
        Some(crate::cache::edited::Artifact::new(
            self.service.cache.clone(),
            folder,
            digest,
            PreviewRequest {
                edge: 0,
                ..input_request
            },
            source,
            index,
            recipe,
        ))
    }

    pub(in crate::ui) fn prepare_folder_edit(
        &mut self,
        item: &Item,
        cached: &CachedImage,
    ) -> Result<bool, String> {
        let Some(recipe) = self.folder_edit_recipe(&item.id) else {
            return Ok(false);
        };
        if recipe.is_neutral() || cached.pyramid.scientific() {
            return Ok(true);
        }
        let source = cached.pyramid.id();
        if self
            .editing
            .folder_completed
            .as_ref()
            .is_some_and(|(s, r)| *s == source && *r == recipe)
        {
            return Ok(true);
        }
        if self
            .editing
            .preparation_failures
            .get(&item.id)
            .is_some_and(|(s, r)| *s == source && *r == recipe)
        {
            return Err("Preparazione delle regolazioni non riuscita".into());
        }
        // A single canonical editing lane coalesces visible, active-photo and
        // folder consumers. Never revoke the very work whose result we await.
        if self.editing.inflight.is_some() {
            return Ok(false);
        }
        let level = cached.pyramid.source();
        let bytes = preview_working_bytes(level.width, level.height)
            + recipe.scratch_bytes(level.width, level.height);
        let margin = self.service.cache.background_headroom();
        let memory = self.service.cache.memory.usage();
        if bytes + cached.pyramid.byte_len() as u64 + margin > memory.limit {
            return Err("Memoria insufficiente per anticipare le regolazioni".into());
        }
        if memory.reserved > memory.limit.saturating_sub(bytes + margin) {
            self.trim_images_for_headroom(bytes + margin);
        }
        if self
            .render_edit_preview(&item.id, &cached.digest, cached.pyramid.clone(), true, true)
            .is_some()
        {
            self.editing.folder_completed = Some((source, recipe));
            return Ok(true);
        }
        Ok(false)
    }

    pub(in crate::ui) fn prepare_folder_thumbnail(
        &mut self,
        item: &Item,
        cached: &CachedImage,
    ) -> Result<bool, String> {
        if cached.pyramid.scientific()
            || self
                .folder_edit_recipe(&item.id)
                .is_some_and(|r| r.is_neutral())
        {
            return Ok(true);
        }
        if self
            .render_edit_thumbnail(&item.id, &cached.digest, cached.pyramid.clone(), true)
            .is_some()
        {
            return Ok(true);
        }
        if let Some(error) = self.edited_thumbnail_error(cached.pyramid.id()) {
            return Err(error.into());
        }
        if self.editing.thumbnail_inflight.is_empty() {
            let bytes = self.service.cache.memory.take_reclaim_request();
            if bytes > 0 {
                self.trim_images_for_headroom(bytes);
            }
        }
        Ok(false)
    }

    pub(super) fn trim_batch_edits(&mut self) {
        // The current completion is a hand-off, even if the reusable quota is
        // tiny. Older results are optional; disk keeps the expensive work.
        let limit = self.service.cache.reusable_bytes() / 2;
        while self.editing.batch_prepared.len() > 1
            && (self.editing.batch_prepared.len() > 16
                || self
                    .editing
                    .batch_prepared
                    .values()
                    .map(|p| p.image.byte_len() as u64)
                    .sum::<u64>()
                    > limit)
        {
            let key = self
                .editing
                .batch_prepared
                .iter()
                .min_by_key(|(_, p)| (p.image.base_level() == 0, p.touched))
                .unwrap()
                .0
                .clone();
            self.editing.batch_prepared.remove(&key);
        }
    }

    pub(in crate::ui) fn optional_edit_bytes(&self, pinned: &HashSet<u64>) -> usize {
        self.editing
            .batch_prepared
            .values()
            .filter(|p| !pinned.contains(&p.source))
            .map(|p| p.image.byte_len())
            .sum()
    }

    pub(in crate::ui) fn release_optional_edit(&mut self, pinned: &HashSet<u64>) -> bool {
        let key = self
            .editing
            .batch_prepared
            .iter()
            .filter(|(_, p)| !pinned.contains(&p.source))
            .min_by_key(|(_, p)| p.touched)
            .map(|(key, _)| key.clone());
        key.is_some_and(|key| self.editing.batch_prepared.remove(&key).is_some())
    }
}
