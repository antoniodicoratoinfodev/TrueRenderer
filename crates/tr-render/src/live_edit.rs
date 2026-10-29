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
        let _ = image;
        0
    }
    pub fn source_size(&self, image: &ImageLevels) -> [u32; 2] {
        self.recipe
            .advanced
            .as_ref()
            .map_or(image.source_size(), |a| {
                a.geometry.output_size(image.source_size())
            })
    }
    pub fn working_bytes(&self, image: &ImageLevels, region: Region) -> u64 {
        if self.gpu_supported_for(image, region)
            && let Ok((level, scaled, _)) = image.render_input(region)
        {
            let (_, size) = crop_bounds(level, scaled);
            return u64::from(size[0]) * u64::from(size[1]) * 32
                + self.recipe.scratch_bytes(size[0], size[1])
                + 4096;
        }
        let level = image.source();
        let native = self.source_size(image);
        let full = native.map(|v| v.div_ceil(1 << image.base_level()));
        let needed: [u64; 2] = std::array::from_fn(|a| {
            ((f64::from(region.size[a]) * region.step[a] * f64::from(full[a])
                / f64::from(native[a])
                + 64.)
                .ceil() as u64)
                .min(u64::from(full[a]))
        });
        // Full-input scratch is a conservative admission for arbitrary inverse
        // geometry/global filters; the evaluator actually visits only support.
        level.pixels.len() as u64 * 32
            + self.recipe.scratch_bytes(level.width, level.height)
            + needed[0] * needed[1] * 112
            + 8 * 1024 * 1024
    }
    pub fn render(&self, image: &ImageLevels, region: Region) -> anyhow::Result<LinearImage> {
        self.render_cached(
            image,
            region,
            &mut tr_core::editing::regional::Processor::new(tr_core::budget::MemoryBudget::new(0)),
        )
    }
    pub fn render_cached(
        &self,
        image: &ImageLevels,
        region: Region,
        cache: &mut tr_core::editing::regional::Processor,
    ) -> anyhow::Result<LinearImage> {
        self.render_cached_cancellable(image, region, cache, &|| false)
    }
    pub fn render_cached_cancellable(
        &self,
        image: &ImageLevels,
        region: Region,
        cache: &mut tr_core::editing::regional::Processor,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> anyhow::Result<LinearImage> {
        anyhow::ensure!(!cancelled(), "Editing annullato");
        if self.gpu_supported_for(image, region) {
            // Point operations need only the visible samples and filter halo.
            // Keep the normal viewer LOD, including native detail at 1:1.
            let (level, mut region, opaque) = image.render_input(region)?;
            let (origin, size) = crop_bounds(level, region);
            let pixels = level
                .pixels
                .chunks_exact(level.width as usize)
                .skip(origin[1] as usize)
                .take(size[1] as usize)
                .flat_map(|row| {
                    row[origin[0] as usize..(origin[0] + size[0]) as usize]
                        .iter()
                        .copied()
                })
                .collect();
            let mut raster = LinearImage::new(size[0], size[1], pixels)?;
            for pixel in &mut raster.pixels {
                for (c, gain) in pixel[..3].iter_mut().zip(self.input_gains) {
                    *c *= gain;
                }
            }
            self.recipe.apply(&mut raster)?;
            anyhow::ensure!(!cancelled(), "Editing annullato");
            region.origin[0] -= f64::from(origin[0]);
            region.origin[1] -= f64::from(origin[1]);
            return tr_core::resample::filter(&raster, region, opaque);
        }
        cache.render_cancellable(image, &self.recipe, self.input_gains, region, cancelled)
    }
    pub fn gpu_supported_for(&self, image: &ImageLevels, region: Region) -> bool {
        self.gpu_supported()
            && image
                .render_input(region)
                .is_ok_and(|(level, _, _)| std::ptr::eq(level, image.source()))
    }
    pub fn gpu_supported(&self) -> bool {
        self.recipe.validate().is_ok()
            && !self.recipe.has_retouch()
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
/// Include every Lanczos3/Mitchell/alpha tap, preserving the original border.
/// This arithmetic-only bound can also be used for admission on the UI thread.
pub(crate) fn crop_bounds(level: &LinearImage, region: Region) -> ([u32; 2], [u32; 2]) {
    let dimensions = [level.width, level.height];
    let origin = std::array::from_fn(|axis| {
        let halo = 3. * region.step[axis].max(1.) + 2.;
        (region.origin[axis] - halo)
            .floor()
            .clamp(0., f64::from(dimensions[axis] - 1)) as u32
    });
    let size = std::array::from_fn(|axis| {
        let halo = 3. * region.step[axis].max(1.) + 2.;
        let end = (region.origin[axis] + region.step[axis] * f64::from(region.size[axis]) + halo)
            .ceil()
            .clamp(f64::from(origin[axis] + 1), f64::from(dimensions[axis]))
            as u32;
        end - origin[axis]
    });
    (origin, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_point_edits_keep_native_detail_and_filter_halos_at_every_zoom() {
        use tr_core::{decoder::RawEngine, editing::CurvePoint, preview::PreviewRequest};
        for alpha in [false, true] {
            // Odd dimensions and fine detail expose accidental mip selection.
            let size = [513, 257];
            let source = LinearImage::new(
                size[0],
                size[1],
                (0..size[0] * size[1])
                    .map(|i| {
                        let a = if alpha {
                            0.25 + (i % 4) as f32 * 0.25
                        } else {
                            1.
                        };
                        [
                            if i % 2 == 0 { 0.1 * a } else { 0.9 * a },
                            (i % 13) as f32 / 13. * a,
                            0.3 * a,
                            a,
                        ]
                    })
                    .collect(),
            )
            .unwrap();
            let image = ImageLevels::from_source(source, PreviewRequest::full()).unwrap();
            for engine in [
                RawEngine::Apple,
                RawEngine::LibRawBilinear,
                RawEngine::LibRawAhd,
                RawEngine::TrueRenderer,
                RawEngine::TrueRendererExperimental,
            ] {
                let mut recipe = EditRecipe::neutral(engine);
                recipe.exposure_ev = 0.7;
                recipe.curve = vec![
                    CurvePoint { x: 0., y: 0. },
                    CurvePoint { x: 0.4, y: 0.6 },
                    CurvePoint { x: 1., y: 1. },
                ];
                let edit = LiveEdit {
                    recipe,
                    input_gains: [1.1, 1., 0.9],
                    max_edge: 256,
                    revision: 1,
                };
                for (origin, step) in [
                    ([131., 67.], [1., 1.]),
                    ([171., 73.], [1., 1.]),
                    ([139.25, 71.5], [0.75, 0.85]),
                    ([-1.25, -0.5], [1.98, 1.9]),
                    ([499.25, 249.75], [1.1, 1.3]),
                    ([0., 0.], [4.1, 4.2]),
                ] {
                    let region = Region {
                        size: [67, 43],
                        origin,
                        step,
                    };
                    let actual = edit.render(&image, region).unwrap();
                    let mut expected = image.source().clone();
                    for pixel in &mut expected.pixels {
                        for (c, gain) in pixel[..3].iter_mut().zip(edit.input_gains) {
                            *c *= gain;
                        }
                    }
                    edit.recipe.apply(&mut expected).unwrap();
                    let expected = ImageLevels::from_source(expected, PreviewRequest::full())
                        .unwrap()
                        .render(region)
                        .unwrap();
                    let maximum = actual
                        .pixels
                        .iter()
                        .flatten()
                        .zip(expected.pixels.iter().flatten())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0_f32, f32::max);
                    assert!(maximum < 1e-6, "{engine:?}, {alpha}, {region:?}: {maximum}");
                    if step == [1., 1.] {
                        assert!(edit.working_bytes(&image, region) < image.byte_len() as u64 / 4);
                    }
                }
            }
        }
    }

    #[test]
    fn creative_grain_uses_cpu_until_disabled() {
        let mut recipe = tr_core::editing::EditRecipe::neutral(Default::default());
        recipe.process_version = 3;
        recipe.advanced = Some(Box::default());
        recipe.advanced.as_mut().unwrap().detail.grain.enabled = true;
        let mut live = super::LiveEdit {
            recipe,
            input_gains: [1.; 3],
            max_edge: 1024,
            revision: 1,
        };
        assert!(!live.gpu_supported());
        live.recipe.advanced.as_mut().unwrap().detail.grain.enabled = false;
        assert!(live.gpu_supported());
    }
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
