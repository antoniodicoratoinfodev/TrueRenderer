//! Canonical editing results share the existing lossless cache and its quotas.
//! Callers must first validate the neutral source through the normal broker path.
use super::{Lookup, Manager, writer::Writer};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};
use tr_core::{
    editing::EditRecipe,
    preview::PreviewRequest,
    protocol::{RasterInfo, ReferenceMip},
    provider::ImageLevels,
};

pub struct Artifact {
    cache: Arc<Manager>,
    folder: PathBuf,
    identity: String,
    request: PreviewRequest,
    size: [u32; 2],
    base: u32,
}
impl Artifact {
    pub fn new(
        cache: Arc<Manager>,
        folder: PathBuf,
        digest: &str,
        input_request: PreviewRequest,
        input: &ImageLevels,
        index: usize,
        recipe: &EditRecipe,
    ) -> Self {
        let input_base = input.base_level() + index as u32;
        let size = recipe.advanced.as_ref().map_or(input.source_size(), |a| {
            a.geometry.output_size(input.source_size())
        });
        let edge = size[0].max(size[1]);
        let base = input_base.min(31 - edge.leading_zeros() + u32::from(!edge.is_power_of_two()));
        let mut level = size;
        for _ in 0..base {
            level = [level[0].div_ceil(2), level[1].div_ceil(2)];
        }
        let request = PreviewRequest {
            edge: if base == 0 {
                0
            } else {
                level[0].max(level[1]).div_ceil(2)
            },
            raw_engine: recipe.raw_engine,
            raw_wb: recipe.raw_wb,
            ..PreviewRequest::full()
        };
        // Input base is essential: editing a reduced input is not equivalent to
        // reducing a fully edited image, especially for nonlinear/spatial tools.
        let identity = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    "tr-canonical-edit-cache-v1",
                    digest,
                    input_request,
                    input.source_size(),
                    input_base,
                    [input.levels()[index].width, input.levels()[index].height],
                    recipe,
                    tr_core::resample::VERSION,
                ))
                .unwrap()
            )
        );
        Self {
            cache,
            folder,
            identity,
            request,
            size,
            base,
        }
    }

    pub fn load(&self, cancelled: &impl Fn() -> bool) -> Option<Arc<ImageLevels>> {
        if tr_core::protocol::mip_geometry(self.size, self.request.maximum_level_edge()).1
            != self.base
        {
            return None;
        }
        let started = std::time::Instant::now();
        loop {
            if cancelled() {
                return None;
            }
            match self.cache.load_preview(
                &self.folder,
                &self.identity,
                self.request,
                &self.cache.memory,
                cancelled,
            ) {
                Lookup::Hit(_, preview)
                    if preview.image.source_size() == self.size
                        && preview.image.base_level() == self.base =>
                {
                    return Some(preview.image);
                }
                // Same bounded, cancellable contention policy as neutral reads.
                // A concurrent thumbnail write should not force canonical work.
                Lookup::Busy if started.elapsed() < std::time::Duration::from_millis(200) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                _ => return None,
            }
        }
    }

    pub fn store(&self, writer: &Writer, image: Arc<ImageLevels>, generation: u64) {
        if image.source_size() != self.size
            || image.base_level() != self.base
            || tr_core::protocol::mip_geometry(self.size, self.request.maximum_level_edge()).1
                != self.base
        {
            return;
        }
        let source = image.source();
        let info = RasterInfo {
            resolution: None,
            shooting: None,
            scientific: None,
            reference_mip: Some(ReferenceMip {
                base: self.base,
                opaque: image.opaque(),
            }),
            width: source.width,
            height: source.height,
            source_width: self.size[0],
            source_height: self.size[1],
            native_bits: 32,
            format: "EditedPreview".into(),
            decoder: "canonical-edit-v1".into(),
            input_color: "premultiplied linear Rec2020".into(),
            filter: tr_core::resample::VERSION.into(),
            orientation: "applied".into(),
        };
        writer.submit_edited(
            self.folder.clone(),
            self.identity.clone(),
            self.request,
            info,
            image,
            generation,
        );
    }
}

#[cfg(test)]
mod tests;
