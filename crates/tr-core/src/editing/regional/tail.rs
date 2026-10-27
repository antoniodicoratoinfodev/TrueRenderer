//! Split an admitted regional graph before its photographic layers. The CPU prefix
//! and all coordinates remain canonical; callers admit and retain the buffers.
use super::*;

pub struct LayerInputPlan {
    pub prefix: EditRecipe,
    pub input_origin: [u32; 2],
    pub input_size: [u32; 2],
    pub native: [u32; 2],
    pub source_full: [u32; 2],
    pub mapped_origin: [u32; 2],
    pub mapped_size: [u32; 2],
    pub output_full: [u32; 2],
    pub display_region: Region,
    pub opaque: bool,
    mapping: Option<Mapping>,
}

impl LayerInputPlan {
    /// First integration admits inputs without an intermediate edited mip and
    /// leaves grain on the CPU path. Rejection never changes the requested LOD.
    pub fn new(
        image: &ImageLevels,
        recipe: &EditRecipe,
        region: Region,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        ensure!(
            recipe.layers.as_ref().is_some_and(|stack| stack
                .layers
                .iter()
                .any(|l| l.enabled && !l.operators.is_empty())),
            "No enabled layer slots"
        );
        Self::for_graph(image, recipe, region, cancelled)
    }

    /// Geometry/filter continuation also serves a complete GPU base graph with
    /// no photographic layers. It does not choose a lower source resolution.
    pub fn for_graph(
        image: &ImageLevels,
        recipe: &EditRecipe,
        region: Region,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        ensure!(!cancelled(), "Editing annullato");
        recipe.validate()?;
        ensure!(!image.scientific(), "Layer tail needs photographic input");
        ensure!(
            recipe
                .advanced
                .as_ref()
                .is_none_or(|a| !a.detail.grain.active()),
            "Grain remains on CPU"
        );
        let native = image.source_size();
        let source_full = [image.source().width, image.source().height];
        let geometry = recipe
            .advanced
            .as_ref()
            .map(|a| a.geometry.clone())
            .unwrap_or_default();
        let output_native = geometry.output_size(native);
        let output_full = output_native.map(|v| v.div_ceil(1 << image.base_level()));
        let region = Region {
            size: region.size,
            origin: std::array::from_fn(|a| {
                region.origin[a] * f64::from(output_full[a]) / f64::from(output_native[a])
            }),
            step: std::array::from_fn(|a| {
                region.step[a] * f64::from(output_full[a]) / f64::from(output_native[a])
            }),
        };
        ensure!(
            region
                .step
                .iter()
                .all(|v| v.is_finite() && *v > 0. && *v <= 2.),
            "Layer tail needs the edited mip chain"
        );
        let opaque = image.opaque() && output_opaque(&geometry, native, output_full, cancelled)?;
        let (origin, size) = resample::footprint(output_full, region, opaque)?;
        let output = Rect { origin, size };
        let mapping = if geometry.has_mapping() {
            Some(Mapping::new(
                geometry.clone(),
                output,
                native,
                source_full,
                output_full,
                None,
                cancelled,
            )?)
        } else {
            None
        };
        let input = mapping.as_ref().map_or(output, |m| m.source);
        let mut prefix = recipe.clone();
        // Retain the process/schema pair and the historical base evaluator.
        if let Some(stack) = &mut prefix.layers {
            stack.layers.clear();
        }
        if let Some(a) = prefix.advanced.as_mut() {
            a.geometry = Geometry {
                vignette: geometry.vignette,
                defringe: geometry.defringe,
                ..Default::default()
            };
        }
        Ok(Self {
            prefix,
            input_origin: input.origin,
            input_size: input.size,
            native,
            source_full,
            mapped_origin: output.origin,
            mapped_size: output.size,
            output_full,
            display_region: region,
            opaque,
            mapping,
        })
    }
    pub fn render_input(
        &self,
        processor: &mut Processor,
        image: &ImageLevels,
        gains: [f32; 3],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        processor.render_cancellable(
            image,
            &self.prefix,
            gains,
            Region {
                size: self.input_size,
                origin: std::array::from_fn(|a| {
                    f64::from(self.input_origin[a]) * f64::from(self.native[a])
                        / f64::from(self.source_full[a])
                }),
                step: std::array::from_fn(|a| {
                    f64::from(self.native[a]) / f64::from(self.source_full[a])
                }),
            },
            cancelled,
        )
    }
    /// Global source coordinates and the exact precomputed fp32 fractions.
    /// Negative coordinates mark transparent samples. Three records per output
    /// pixel retain red/green/blue CA independently; no shader trigonometry.
    pub fn geometry_samples(&self) -> Vec<[f32; 4]> {
        self.mapping.as_ref().map_or_else(Vec::new, |mapping| {
            mapping
                .samples
                .iter()
                .flatten()
                .map(|s| {
                    if s.valid {
                        [s.xy[0] as f32, s.xy[1] as f32, s.fraction[0], s.fraction[1]]
                    } else {
                        [-1., -1., 0., 0.]
                    }
                })
                .collect()
        })
    }
    pub fn chromatic_aberration(&self) -> bool {
        self.mapping
            .as_ref()
            .is_some_and(|m| m.geometry.ca != [0.; 2])
    }
    pub fn first_mask(
        &self,
        image: &LinearImage,
        stack: &LayerStack,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Vec<f32>> {
        ensure!(
            [image.width, image.height] == self.input_size,
            "Layer input shape mismatch"
        );
        let layer = stack
            .layers
            .iter()
            .find(|l| l.enabled && !l.operators.is_empty());
        let Some(layer) = layer.filter(|l| l.mask.root.is_some()) else {
            return Ok(vec![1.]);
        };
        let mut weights = vec![0.; image.pixels.len()];
        visit_mut(&mut weights, cancelled, |i, weight| {
            let p = image.pixels[i];
            if p[3] > 0. {
                *weight = layer.mask.weight(
                    self.xy(i),
                    detail::straight(p),
                    self.native[1] as f32 / self.native[0] as f32,
                );
            }
        })?;
        Ok(weights)
    }
    fn xy(&self, i: usize) -> [f32; 2] {
        [
            ((self.input_origin[0] as usize + i % self.input_size[0] as usize) as f32 + 0.5)
                / self.source_full[0] as f32,
            ((self.input_origin[1] as usize + i / self.input_size[0] as usize) as f32 + 0.5)
                / self.source_full[1] as f32,
        ]
    }
    /// Independent scalar completion for qualification and CPU fallback.
    pub fn finish_cpu(
        &self,
        mut input: LinearImage,
        stack: &LayerStack,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        ensure!(
            [input.width, input.height] == self.input_size,
            "Layer input shape mismatch"
        );
        visit_mut(&mut input.pixels, cancelled, |i, p| {
            if p[3] > 0. {
                let rgb = stack.apply(
                    detail::straight(*p),
                    self.xy(i),
                    self.native[1] as f32 / self.native[0] as f32,
                );
                for c in 0..3 {
                    p[c] = rgb[c] * p[3];
                }
            }
        })?;
        let mapped = if let Some(mapping) = &self.mapping {
            mapping.apply(&input, self.input_origin, cancelled)?
        } else {
            input
        };
        resample::filter_tile(
            &mapped,
            self.mapped_origin,
            self.output_full,
            self.display_region,
            self.opaque,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitting_before_layers_preserves_admitted_domains_geometry_masks_and_alpha() {
        for alpha in [false, true] {
            for base in [0, 2] {
                let source = super::super::tests::source(alpha, [265, 199]);
                let source = ImageLevels::from_reference_mip(
                    source.source().clone(),
                    if base == 0 { [265, 199] } else { [1057, 793] },
                    base,
                    !alpha,
                )
                .unwrap();
                let mut recipe = super::super::tests::recipe();
                recipe.advanced.as_mut().unwrap().detail.denoise = Denoise::MultiscaleV2;
                recipe.advanced.as_mut().unwrap().geometry.crop = [0.12, 0.08, 0.9, 0.92];
                recipe.advanced.as_mut().unwrap().geometry.quarter_turns = 1;
                recipe.advanced.as_mut().unwrap().geometry.angle = 3.;
                recipe.advanced.as_mut().unwrap().geometry.ca = [0.7, -0.5];
                let expected = super::super::tests::reference(&source, &recipe, [1.1, 1., 0.9]);
                for step in [0.7, 1., 1.91] {
                    let region = Region {
                        size: [47, 43],
                        origin: [15.3, 21.7].map(|v| v * f64::from(1 << base)),
                        step: [step * f64::from(1 << base); 2],
                    };
                    let plan = LayerInputPlan::new(&source, &recipe, region, &|| false).unwrap();
                    let mut processor = Processor::new(MemoryBudget::new(128 * 1024 * 1024));
                    let input = plan
                        .render_input(&mut processor, &source, [1.1, 1., 0.9], &|| false)
                        .unwrap();
                    let actual = plan
                        .finish_cpu(input, recipe.layers.as_ref().unwrap(), &|| false)
                        .unwrap();
                    super::super::tests::compare(&actual, &expected.render(region).unwrap());
                }
            }
        }
    }
}
