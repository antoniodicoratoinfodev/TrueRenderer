//! Display-only drafts. The selected source/engine stays resident; final edits,
//! sampling and export always use the canonical CPU graph separately.
use tr_core::{color::LinearImage, editing::EditRecipe, provider::ImageLevels, resample::Region};

#[derive(Clone, Debug, PartialEq)]
pub struct LiveEdit {
    pub recipe: EditRecipe,
    /// Explicit temporary RGB approximation while native RAW WB is developing.
    pub input_gains: [f32; 3],
    pub max_edge: u32,
    pub revision: u64,
}
impl LiveEdit {
    pub fn source_index(&self, image: &ImageLevels) -> usize {
        image
            .levels()
            .iter()
            .position(|l| l.width.max(l.height) <= self.max_edge.clamp(256, 2048))
            .unwrap_or(image.levels().len() - 1)
    }
    pub fn source_size(&self, image: &ImageLevels) -> [u32; 2] {
        self.recipe
            .advanced
            .as_ref()
            .map_or(image.source_size(), |a| {
                a.geometry.output_size(image.source_size())
            })
    }
    pub fn working_bytes(&self, image: &ImageLevels) -> u64 {
        let level = &image.levels()[self.source_index(image)];
        level.pixels.len() as u64 * 32 + self.recipe.scratch_bytes(level.width, level.height) + 4096
    }
    pub fn render(&self, image: &ImageLevels, region: Region) -> anyhow::Result<LinearImage> {
        let index = self.source_index(image);
        let mut raster = image.levels()[index].clone();
        if self.input_gains != [1.; 3] {
            for p in &mut raster.pixels {
                for (c, gain) in p[..3].iter_mut().zip(self.input_gains) {
                    *c *= gain;
                }
            }
        }
        let native = self.recipe.apply_preview(
            &mut raster,
            image.source_size(),
            image.base_level() + index as u32,
        )?;
        let region = scaled_region(region, [raster.width, raster.height], native);
        let opaque = raster.pixels.iter().all(|p| p[3] == 1.);
        tr_core::resample::filter(&raster, region, opaque)
    }
    pub fn gpu_supported(&self) -> bool {
        self.recipe.validate().is_ok()
            && self.recipe.layers.as_ref().is_none_or(|s| s.is_neutral())
            && self
                .input_gains
                .iter()
                .all(|g| g.is_finite() && (0.05..=20.).contains(g))
            && self.recipe.advanced.as_ref().is_none_or(|a| {
                !a.detail.active()
                    && a.geometry == Default::default()
                    && !a.masks.iter().any(|m| m.active())
            })
    }
}
pub(crate) fn scaled_region(region: Region, raster: [u32; 2], native: [u32; 2]) -> Region {
    let ratio = [
        raster[0] as f64 / native[0] as f64,
        raster[1] as f64 / native[1] as f64,
    ];
    Region {
        size: region.size,
        origin: [region.origin[0] * ratio[0], region.origin[1] * ratio[1]],
        step: [region.step[0] * ratio[0], region.step[1] * ratio[1]],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn active_new_layers_never_use_the_historical_shader() {
        use tr_core::editing::{
            EditRecipe,
            layers::{Layer, Operator},
        };
        let mut live = super::LiveEdit {
            recipe: EditRecipe::neutral(Default::default()),
            input_gains: [1.; 3],
            max_edge: 1024,
            revision: 1,
        };
        live.recipe.layer_stack().layers.push(Layer::new(
            "exposure",
            Operator::Light {
                exposure: 1.,
                temperature: 0.,
                tint: 0.,
                saturation: 0.,
            },
        ));
        assert!(!live.gpu_supported());
        live.recipe.layers.as_mut().unwrap().layers[0].enabled = false;
        assert!(live.gpu_supported());
    }
}
