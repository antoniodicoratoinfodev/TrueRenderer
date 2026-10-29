//! Canonical photographic editing of a visible footprint. Coordinates, spatial
//! support and mip construction belong to the full image, never to the crop.
//! Checkpoints are optional, bounded and credited; eviction changes cost only.
use super::{
    EditRecipe, Parameters,
    color::Color,
    detail::{self, Denoise, Detail},
    geometry::Geometry,
    layers::{Layer, LayerStack},
    masks,
};
use crate::{
    budget::{Lease, MemoryBudget},
    color::{LinearImage, Pixel},
    provider::ImageLevels,
    resample::{self, Region},
};
use anyhow::{Result, ensure};
use rayon::prelude::*;
mod point;
mod spatial;
pub mod tail;

// Cancellation is checked per small batch, never by locking the UI or by
// abandoning a partially edited raster as a displayable result.
pub(super) fn visit_mut<T: Send>(
    items: &mut [T],
    cancelled: &(impl Fn() -> bool + Sync),
    apply: impl Fn(usize, &mut T) + Sync,
) -> Result<()> {
    let visit = |(batch, items): (usize, &mut [T])| -> Result<()> {
        ensure!(!cancelled(), "Editing annullato");
        for (i, item) in items.iter_mut().enumerate() {
            apply(batch * 2048 + i, item);
        }
        Ok(())
    };
    if items.len() >= 32768 {
        crate::compute::install(|| items.par_chunks_mut(2048).enumerate().try_for_each(visit))?;
    } else {
        items.chunks_mut(2048).enumerate().try_for_each(visit)?;
    }
    ensure!(!cancelled(), "Editing annullato");
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub(crate) origin: [u32; 2],
    pub(crate) size: [u32; 2],
}
impl Rect {
    pub(crate) fn full(size: [u32; 2]) -> Self {
        Self {
            origin: [0; 2],
            size,
        }
    }
    pub(crate) fn expand(self, radius: u32, full: [u32; 2]) -> Self {
        let origin = self.origin.map(|v| v.saturating_sub(radius));
        let size = std::array::from_fn(|a| {
            (self.origin[a] + self.size[a])
                .saturating_add(radius)
                .min(full[a])
                - origin[a]
        });
        Self { origin, size }
    }
    fn pixels(self) -> u64 {
        u64::from(self.size[0]) * u64::from(self.size[1])
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Stage {
    Window(Rect),
    RegionalNoise(Detail, Rect),
    Base(Box<EditRecipe>, [f32; 3], f32),
    Detail(Detail),
    Defringe(f32),
    Color(Color, Vec<masks::Mask>),
    Straight,
    Layer(Layer),
    Premultiply,
}
struct Checkpoint {
    index: usize,
    image: LinearImage,
    _lease: Lease,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    source: u64,
    rect: Rect,
}
struct LayerDelta {
    identity: Identity,
    prefix: Vec<Stage>,
    layer: Layer,
    delta: Vec<Pixel>,
    lease: Lease,
}

#[derive(Default, Clone, Debug, serde::Serialize)]
pub struct Statistics {
    /// Window, base, spatial, color/layer, opacity, geometry, mip, checkpoints.
    pub stage_us: [u64; 8],
    /// Global noise model, regional reconstruction, coarse guide, chroma merge.
    /// Branch wall times may overlap; their sum is not the frame latency.
    pub noise_us: [u64; 4],
    pub renders: u64,
    pub stage_evaluations: u64,
    pub stages_reused: u64,
    pub input_pixels: u64,
    pub full_frame_fallbacks: u64,
    pub retained_bytes: u64,
    pub opacity_reuses: u64,
    pub layer_mask_reuses: u64,
    pub spatial_reuses: u64,
    pub fused_source_frames: u64,
}

/// One bounded worker cache. It never owns a source pyramid or crosses source
/// identities. A comparison view can safely evict it without changing pixels.
pub struct Processor {
    memory: MemoryBudget,
    identity: Option<Identity>,
    stages: Vec<Stage>,
    checkpoints: Vec<Checkpoint>,
    opacity: Option<(Geometry, [u32; 2], [u32; 2], bool)>,
    mapping: Option<Mapping>,
    layer_delta: Option<LayerDelta>,
    spatial: Option<spatial::Checkpoint>,
    vignette: Option<point::Vignette>,
    pub statistics: Statistics,
}
impl Processor {
    pub fn new(memory: MemoryBudget) -> Self {
        Self {
            memory,
            identity: None,
            stages: Vec::new(),
            checkpoints: Vec::new(),
            opacity: None,
            mapping: None,
            layer_delta: None,
            spatial: None,
            vignette: None,
            statistics: Statistics::default(),
        }
    }
    pub fn clear(&mut self) {
        self.identity = None;
        self.stages.clear();
        self.checkpoints.clear();
        self.mapping = None;
        self.layer_delta = None;
        self.spatial = None;
        self.vignette = None;
        self.opacity = None;
        self.statistics.retained_bytes = 0;
    }

    fn cache_bytes(&self) -> u64 {
        self.checkpoints
            .iter()
            .map(|c| c._lease.bytes())
            .sum::<u64>()
            + self
                .mapping
                .as_ref()
                .and_then(|m| m.lease.as_ref())
                .map_or(0, Lease::bytes)
            + self.layer_delta.as_ref().map_or(0, |d| d.lease.bytes())
            + self.spatial.as_ref().map_or(0, |s| s.lease.bytes())
            + self.vignette.as_ref().map_or(0, |v| v.lease.bytes())
    }

    fn reserve_cache(&self, bytes: u64, class_limit: u64) -> Option<Lease> {
        let usage = self.memory.usage();
        (bytes <= class_limit
            && bytes <= (usage.limit / 4).saturating_sub(self.cache_bytes())
            && bytes <= usage.limit.saturating_sub(usage.reserved))
        .then(|| self.memory.try_reserve(bytes))
        .flatten()
    }

    pub fn render(
        &mut self,
        image: &ImageLevels,
        recipe: &EditRecipe,
        gains: [f32; 3],
        region: Region,
    ) -> Result<LinearImage> {
        self.render_cancellable(image, recipe, gains, region, &|| false)
    }
    pub fn render_cancellable(
        &mut self,
        image: &ImageLevels,
        recipe: &EditRecipe,
        gains: [f32; 3],
        region: Region,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        let result = self.render_inner(image, recipe, gains, region, cancelled);
        if result.is_err() {
            self.clear();
        }
        result
    }
    fn render_inner(
        &mut self,
        image: &ImageLevels,
        recipe: &EditRecipe,
        gains: [f32; 3],
        region: Region,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        ensure!(!cancelled(), "Editing annullato");
        recipe.validate()?;
        ensure!(
            gains
                .iter()
                .all(|v| v.is_finite() && (0.05..=20.).contains(v)),
            "Guadagni anteprima non validi"
        );
        if self.cache_bytes() > self.memory.usage().limit / 4 {
            self.clear();
        }
        if recipe.is_neutral() && gains == [1.; 3] {
            return image.render(region);
        }
        if recipe.has_retouch() {
            // Strokes sample their frozen full-frame donor. Until a regional
            // dependency planner exists, replay the canonical native graph.
            let mut raster = image.source().clone();
            visit_mut(&mut raster.pixels, cancelled, |_, p| {
                for c in 0..3 {
                    p[c] *= gains[c];
                }
            })?;
            let output = recipe.apply_preview_cancellable(
                &mut raster,
                image.source_size(),
                image.base_level(),
                cancelled,
            )?;
            let opaque = raster.pixels.iter().all(|p| p[3] == 1.);
            let rendered = ImageLevels::from_reference_mip_cancellable(
                raster,
                output,
                image.base_level(),
                opaque,
                cancelled,
            )?;
            return rendered.render(region);
        }
        let input = image.source();
        let native = image.source_size();
        let full = [input.width, input.height];
        let geometry = recipe
            .advanced
            .as_ref()
            .map(|a| a.geometry.clone())
            .unwrap_or_default();
        let output_native = geometry.output_size(native);
        let output_full = output_native.map(|v| v.div_ceil(1 << image.base_level()));
        let opaque = if !image.opaque() {
            false
        } else if !geometry.has_mapping() {
            true
        } else {
            if self
                .opacity
                .as_ref()
                .is_none_or(|(g, n, f, _)| g != &geometry || *n != native || *f != output_full)
            {
                let value = output_opaque(&geometry, native, output_full, cancelled)?;
                self.opacity = Some((geometry.clone(), native, output_full, value));
            }
            self.opacity.as_ref().unwrap().3
        };
        // Reconstruct exactly the same sequence of edited mips as ImageLevels.
        // Applying a nonlinear graph to a smaller neutral mip is not equivalent.
        let mut dimensions = vec![output_full];
        while {
            let last = *dimensions.last().unwrap();
            (0..2).any(|a| region.step[a] * f64::from(last[a]) / f64::from(output_native[a]) > 2.)
                && last != [1, 1]
        } {
            dimensions.push(dimensions.last().unwrap().map(|v| v.div_ceil(2)));
        }
        let mut at = dimensions.len() - 1;
        let mut filter_region = Region {
            size: region.size,
            origin: std::array::from_fn(|a| {
                region.origin[a] * f64::from(dimensions[at][a]) / f64::from(output_native[a])
            }),
            step: std::array::from_fn(|a| {
                region.step[a] * f64::from(dimensions[at][a]) / f64::from(output_native[a])
            }),
        };
        let mut filters = Vec::new();
        let needed = loop {
            let (origin, size) = resample::footprint(dimensions[at], filter_region, opaque)?;
            let rect = Rect { origin, size };
            filters.push((dimensions[at], rect, filter_region));
            if at == 0 {
                break rect;
            }
            let ratio: [f64; 2] = std::array::from_fn(|a| {
                f64::from(dimensions[at - 1][a]) / f64::from(dimensions[at][a])
            });
            filter_region = Region {
                size,
                origin: std::array::from_fn(|a| f64::from(origin[a]) * ratio[a]),
                step: ratio,
            };
            at -= 1;
        };
        let mut raster = self.edit_region(
            image,
            recipe,
            gains,
            &geometry,
            needed,
            native,
            full,
            output_full,
            cancelled,
        )?;
        ensure!(!cancelled(), "Editing annullato");
        if let Some(a) = &recipe.advanced {
            a.detail.grain.apply_region(
                &mut raster,
                output_native,
                needed.origin,
                output_full,
                cancelled,
            )?;
        }
        ensure!(
            raster.samples_are_finite(),
            "Regolazione fotografica: risultato non finito"
        );
        for (full, rect, filter_region) in filters.into_iter().rev() {
            ensure!(!cancelled(), "Editing annullato");
            let started = std::time::Instant::now();
            raster = resample::filter_tile(&raster, rect.origin, full, filter_region, opaque)?;
            self.statistics.stage_us[6] += started.elapsed().as_micros() as u64;
        }
        ensure!(!cancelled(), "Editing annullato");
        self.statistics.renders += 1;
        Ok(raster)
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_region(
        &mut self,
        image: &ImageLevels,
        recipe: &EditRecipe,
        gains: [f32; 3],
        geometry: &Geometry,
        output: Rect,
        native: [u32; 2],
        full: [u32; 2],
        output_full: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        let mapped = geometry.has_mapping();
        let source = if mapped {
            if self.mapping.as_ref().is_none_or(|m| {
                m.geometry != *geometry
                    || m.output != output
                    || m.native != native
                    || m.full != full
                    || m.output_full != output_full
            }) {
                self.mapping = None;
                let bytes = output.pixels() * std::mem::size_of::<[Sample; 3]>() as u64;
                // The caller covers transient mapping in its working lease.
                // Persistent reuse is admitted separately and stays optional.
                let lease = self.reserve_cache(bytes, self.memory.usage().limit / 16);
                self.mapping = Some(Mapping::new(
                    geometry.clone(),
                    output,
                    native,
                    full,
                    output_full,
                    lease,
                    cancelled,
                )?);
            }
            self.mapping.as_ref().unwrap().source
        } else {
            output
        };
        let (radius, global) = spatial_support(recipe, native, full);
        let input_rect = if global {
            Rect::full(full)
        } else {
            source.expand(radius, full)
        };
        self.statistics.full_frame_fallbacks += u64::from(global);
        self.statistics.input_pixels += input_rect.pixels();
        let amount = geometry.vignette;
        if self
            .vignette
            .as_ref()
            .is_some_and(|v| v.source != image.id() || v.amount != amount)
        {
            self.vignette = None;
        }
        if global && amount != 0. && self.vignette.is_none() {
            let bytes = u64::from(full[0]) * u64::from(full[1]) * 4;
            if let Some(lease) = self.reserve_cache(
                bytes,
                (self.memory.usage().limit / 64).min(128 * 1024 * 1024),
            ) {
                let mut gains = vec![0.; full[0] as usize * full[1] as usize];
                visit_mut(&mut gains, cancelled, |i, v| {
                    *v = geometry.optical_gain(
                        ((i % full[0] as usize) as f32 + 0.5) / full[0] as f32,
                        ((i / full[0] as usize) as f32 + 0.5) / full[1] as f32,
                    );
                })?;
                self.vignette = Some(point::Vignette {
                    source: image.id(),
                    amount,
                    gains,
                    lease,
                });
            }
        }
        let mut stages = stages(recipe, gains);
        if let Some(index) = stages.iter().rposition(|s| matches!(s, Stage::Detail(d) if
            d.dehaze != 0. || ((d.luminance_noise != 0. || d.chroma_noise != 0.) && d.denoise != Denoise::Legacy))) {
            if let Stage::Detail(d) = &stages[index] && d.dehaze == 0. {
                stages[index] = Stage::RegionalNoise(d.clone(), source.expand(radius, full));
            } else {stages.insert(index + 1, Stage::Window(source.expand(radius, full)));}
        }
        let mut stage_rect = input_rect;
        let rectangles: Vec<_> = stages
            .iter()
            .map(|s| {
                if let Stage::Window(rect) | Stage::RegionalNoise(_, rect) = s {
                    stage_rect = *rect;
                }
                stage_rect
            })
            .collect();
        let identity = Identity {
            source: image.id(),
            rect: input_rect,
        };
        let new_source = self.identity != Some(identity);
        let prefix = if self.identity == Some(identity) {
            stages
                .iter()
                .zip(&self.stages)
                .take_while(|(a, b)| a == b)
                .count()
        } else {
            0
        };
        self.checkpoints.retain(|c| c.index < prefix);
        let checkpoint = self.checkpoints.iter().max_by_key(|c| c.index);
        let (mut raster, start, fused) = if let Some(c) = checkpoint {
            self.statistics.stages_reused += (c.index + 1) as u64;
            (c.image.clone(), c.index + 1, false)
        } else if !new_source
            && let (
                Some(Stage::Base(base, gains, vignette)),
                Some(Stage::RegionalNoise(noise, rect)),
            ) = (stages.first(), stages.get(1))
        {
            // Re-evaluate global statistics and guides from the point transform
            // directly. Each sampled value uses the same fp32 operations; the
            // full native intermediate is neither cloned nor written/read back.
            let started = std::time::Instant::now();
            let input = point::Input::new(
                image.source(),
                base,
                *gains,
                *vignette,
                self.vignette.as_ref().map(|v| v.gains.as_slice()),
            );
            let raster = noise.regional_noise(
                &input,
                native,
                *rect,
                &mut self.statistics.noise_us,
                cancelled,
            )?;
            self.statistics.stage_us[2] += started.elapsed().as_micros() as u64;
            self.statistics.stage_evaluations += 2;
            self.statistics.fused_source_frames += 1;
            (raster, 2, true)
        } else {
            (extract(image.source(), input_rect)?, 0, false)
        };
        self.identity = Some(identity);
        let cap = (self.memory.usage().limit / 8).min(512 * 1024 * 1024);
        let mut priorities = Vec::new();
        if prefix > 0 && prefix < stages.len() {
            priorities.push(prefix - 1);
        }
        // With only one native-frame slot, keep the expensive input of the
        // layer stack, rather than letting the cheap base stage fill the quota.
        if let Some(i) = stages.iter().position(|s| matches!(s, Stage::Layer(_))) {
            priorities.push(i - 1);
        }
        priorities.extend(
            stages
                .iter()
                .enumerate()
                .filter_map(|(i, s)| matches!(s, Stage::Window(_)).then_some(i.saturating_sub(1))),
        );
        priorities.push(stages.len() - 1);
        priorities.extend((0..stages.len()).rev());
        let mut retain = Vec::new();
        let mut retained = 0;
        for i in priorities {
            let bytes = rectangles[i].pixels() * 16;
            if retained + bytes <= cap && !retain.contains(&i) {
                retain.push(i);
                retained += bytes;
            }
        }
        self.checkpoints.retain(|c| retain.contains(&c.index));
        // A stable upstream prefix still seeds the checkpoint after fusion,
        // so changing from exposure to a downstream slider does not lose reuse.
        if fused && prefix > 1 && retain.contains(&1) {
            let bytes = raster.pixels.len() as u64 * 16;
            let retained: u64 = self.checkpoints.iter().map(|c| c._lease.bytes()).sum();
            if retained + bytes <= cap
                && let Some(lease) = self.reserve_cache(bytes, cap)
            {
                self.checkpoints.push(Checkpoint {
                    index: 1,
                    image: raster.clone(),
                    _lease: lease,
                });
            }
        }
        let mut origin = if start == 0 {
            input_rect.origin
        } else {
            rectangles[start - 1].origin
        };
        let layer_target = if matches!(stages.get(prefix), Some(Stage::Layer(_))) {
            Some(prefix)
        } else {
            stages.iter().position(|s| matches!(s, Stage::Layer(_)))
        };
        let spatial_target = if stages.get(prefix).is_some_and(spatial::supported) {
            Some(prefix)
        } else if new_source {
            stages.iter().position(spatial::supported)
        } else {
            None
        };
        for (index, stage) in stages.iter().enumerate().skip(start) {
            ensure!(!cancelled(), "Editing annullato");
            let started = std::time::Instant::now();
            let bucket = match stage {
                Stage::Window(_) => 0,
                Stage::Base(..) => 1,
                Stage::Detail(_) | Stage::RegionalNoise(..) | Stage::Defringe(_) => 2,
                Stage::Layer(_) if Some(index) == layer_target => 4,
                _ => 3,
            };
            if Some(index) == layer_target
                && let Stage::Layer(layer) = stage
            {
                self.apply_layer(
                    layer,
                    &stages[..index],
                    identity,
                    &mut raster,
                    origin,
                    full,
                    native,
                    cancelled,
                )?;
            } else if Some(index) == spatial_target
                && let Stage::Detail(detail) = stage
            {
                self.apply_spatial(
                    detail,
                    &stages[..index],
                    identity,
                    &mut raster,
                    full,
                    native,
                    cancelled,
                )?;
            } else {
                apply_stage(
                    stage,
                    &mut raster,
                    origin,
                    full,
                    native,
                    &mut self.statistics.noise_us,
                    cancelled,
                )?;
            }
            origin = rectangles[index].origin;
            self.statistics.stage_us[bucket] += started.elapsed().as_micros() as u64;
            self.statistics.stage_evaluations += 1;
            // Favor expensive prefixes and layer inputs. A last-stage result is
            // useful too: pure geometry/grain drags then reuse the whole graph.
            let retained: u64 = self.checkpoints.iter().map(|c| c._lease.bytes()).sum();
            let bytes = raster.pixels.len() as u64 * 16;
            if retain.contains(&index)
                && (new_source || index < prefix)
                && retained + bytes <= cap
                && let Some(lease) = self.reserve_cache(bytes, cap)
            {
                let started = std::time::Instant::now();
                self.checkpoints.push(Checkpoint {
                    index,
                    image: raster.clone(),
                    _lease: lease,
                });
                self.statistics.stage_us[7] += started.elapsed().as_micros() as u64;
            }
        }
        ensure!(!cancelled(), "Editing annullato");
        self.stages = stages;
        self.statistics.retained_bytes = self.cache_bytes();
        if mapped {
            let started = std::time::Instant::now();
            let result = self
                .mapping
                .as_ref()
                .unwrap()
                .apply(&raster, origin, cancelled)?;
            self.statistics.stage_us[5] += started.elapsed().as_micros() as u64;
            if self.mapping.as_ref().unwrap().lease.is_none() {
                self.mapping = None;
            }
            Ok(result)
        } else {
            extract(
                &raster,
                Rect {
                    origin: std::array::from_fn(|a| output.origin[a] - origin[a]),
                    size: output.size,
                },
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_layer(
        &mut self,
        layer: &Layer,
        prefix: &[Stage],
        identity: Identity,
        image: &mut LinearImage,
        origin: [u32; 2],
        full: [u32; 2],
        native: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        let mut normalized = layer.clone();
        normalized.opacity = 1.;
        normalized.fill = 1.;
        let hit = self.layer_delta.as_ref().is_some_and(|d| {
            d.identity == identity
                && d.prefix == prefix
                && d.layer == normalized
                && d.delta.len() == image.pixels.len()
        });
        let mask_hit = self.layer_delta.as_ref().is_some_and(|d| {
            d.identity == identity
                && d.prefix == prefix
                && d.layer.mask == normalized.mask
                && d.delta.len() == image.pixels.len()
        });
        if !hit && mask_hit {
            let stack = LayerStack {
                base_process: 3,
                layers: vec![normalized.clone()],
            };
            let prepared = stack.prepare(native[1] as f32 / native[0] as f32);
            let cached = self.layer_delta.as_mut().unwrap();
            visit_mut(&mut cached.delta, cancelled, |i, d| {
                let p = image.pixels[i];
                if p[3] > 0. {
                    *d = prepared[0].difference_with_weight([p[0], p[1], p[2]], d[3]);
                }
            })?;
            cached.layer = normalized;
            self.statistics.layer_mask_reuses += 1;
        } else if !hit {
            self.layer_delta = None;
            let bytes = image.pixels.len() as u64 * 16;
            if let Some(lease) = self.reserve_cache(
                bytes,
                (self.memory.usage().limit / 32).min(128 * 1024 * 1024),
            ) {
                let stack = LayerStack {
                    base_process: 3,
                    layers: vec![normalized.clone()],
                };
                let prepared = stack.prepare(native[1] as f32 / native[0] as f32);
                let mut delta = vec![[0.; 4]; image.pixels.len()];
                let width = image.width as usize;
                visit_mut(&mut delta, cancelled, |i, d| {
                    let p = image.pixels[i];
                    if p[3] > 0. {
                        let xy = [
                            ((origin[0] as usize + i % width) as f32 + 0.5) / full[0] as f32,
                            ((origin[1] as usize + i / width) as f32 + 0.5) / full[1] as f32,
                        ];
                        *d = prepared[0].difference([p[0], p[1], p[2]], xy);
                    }
                })?;
                self.layer_delta = Some(LayerDelta {
                    identity,
                    prefix: prefix.to_vec(),
                    layer: normalized,
                    delta,
                    lease,
                });
            }
        } else {
            self.statistics.opacity_reuses += 1;
        }
        if let Some(d) = &self.layer_delta {
            let opacity = layer.opacity * layer.fill;
            visit_mut(&mut image.pixels, cancelled, |i, p| {
                if p[3] > 0. {
                    let w = opacity * d.delta[i][3];
                    if w == 0. {
                        return;
                    }
                    for (c, v) in p[..3].iter_mut().enumerate() {
                        *v += w * d.delta[i][c];
                    }
                }
            })?;
        } else {
            // No cache credit: the same canonical operator remains available.
            apply_stage(
                &Stage::Layer(layer.clone()),
                image,
                origin,
                full,
                native,
                &mut [0; 4],
                cancelled,
            )?;
        }
        Ok(())
    }
}

pub(crate) fn extract(source: &LinearImage, rect: Rect) -> Result<LinearImage> {
    let mut pixels = vec![[0.; 4]; rect.pixels() as usize];
    crate::compute::install(|| {
        pixels
            .par_chunks_mut(rect.size[0] as usize)
            .enumerate()
            .for_each(|(y, row)| {
                let start =
                    (y + rect.origin[1] as usize) * source.width as usize + rect.origin[0] as usize;
                row.copy_from_slice(&source.pixels[start..start + row.len()]);
            })
    });
    LinearImage::new(rect.size[0], rect.size[1], pixels)
}

fn stages(recipe: &EditRecipe, gains: [f32; 3]) -> Vec<Stage> {
    let mut base = recipe.clone();
    base.advanced = None;
    base.retouch = None;
    base.layers = None;
    // Process semantics in transform_prepared depend on the historical process.
    base.process_version = recipe
        .layers
        .as_ref()
        .map_or(recipe.process_version, |s| s.base_process);
    let vignette = recipe.advanced.as_ref().map_or(0., |a| a.geometry.vignette);
    let mut result = vec![Stage::Base(Box::new(base), gains, vignette)];
    if let Some(a) = &recipe.advanced {
        let d = &a.detail;
        if d.luminance_noise != 0. || d.chroma_noise != 0. {
            result.push(Stage::Detail(Detail {
                luminance_noise: d.luminance_noise,
                chroma_noise: d.chroma_noise,
                denoise: d.denoise,
                threshold: d.threshold,
                ..Default::default()
            }));
        }
        for detail in [
            Detail {
                texture: d.texture,
                threshold: d.threshold,
                ..Default::default()
            },
            Detail {
                clarity: d.clarity,
                threshold: d.threshold,
                ..Default::default()
            },
            Detail {
                sharpen: d.sharpen,
                radius: d.radius,
                threshold: d.threshold,
                ..Default::default()
            },
            Detail {
                dehaze: d.dehaze,
                ..Default::default()
            },
        ] {
            result.push(Stage::Detail(detail));
        }
        if a.geometry.defringe != 0. {
            result.push(Stage::Defringe(a.geometry.defringe));
        }
        result.push(Stage::Color(a.color.clone(), a.masks.clone()));
    }
    if let Some(stack) = &recipe.layers
        && !stack.is_neutral()
    {
        result.push(Stage::Straight);
        result.extend(
            stack
                .layers
                .iter()
                .filter(|l| l.active())
                .cloned()
                .map(Stage::Layer),
        );
        result.push(Stage::Premultiply);
    }
    result
}

fn apply_stage(
    stage: &Stage,
    image: &mut LinearImage,
    origin: [u32; 2],
    full: [u32; 2],
    native: [u32; 2],
    noise_us: &mut [u64; 4],
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<()> {
    let width = image.width as usize;
    let xy = |i: usize| {
        [
            ((origin[0] as usize + i % width) as f32 + 0.5) / full[0] as f32,
            ((origin[1] as usize + i / width) as f32 + 0.5) / full[1] as f32,
        ]
    };
    match stage {
        Stage::RegionalNoise(detail, rect) => {
            *image = detail.regional_noise(image, native, *rect, noise_us, cancelled)?;
        }
        Stage::Window(rect) => {
            *image = extract(
                image,
                Rect {
                    origin: std::array::from_fn(|a| rect.origin[a] - origin[a]),
                    size: rect.size,
                },
            )?;
        }
        Stage::Base(recipe, gains, vignette) => {
            let prepared = point::Prepared::new(recipe, *gains, *vignette, full);
            visit_mut(&mut image.pixels, cancelled, |i, p| {
                *p = prepared.apply(
                    origin[0] as usize + i % width,
                    origin[1] as usize + i / width,
                    *p,
                );
            })?;
        }
        Stage::Detail(detail) => detail.apply_domain(image, native, full, cancelled)?,
        Stage::Defringe(amount) => detail::defringe(image, *amount),
        Stage::Color(color, masks) => {
            let prepared: Vec<_> = masks
                .iter()
                .filter(|m| m.active())
                .map(|m| masks::Prepared::new(m, native[1] as f32 / native[0] as f32))
                .collect();
            let active = *color != Color::default();
            visit_mut(&mut image.pixels, cancelled, |i, p| {
                if p[3] <= 0. || (!active && prepared.is_empty() && p[3] == 1.) {
                    return;
                }
                let guide = detail::straight(*p);
                let mut rgb = if active {
                    color.apply_active(guide)
                } else {
                    guide
                };
                for mask in &prepared {
                    mask.apply(&mut rgb, guide, xy(i));
                }
                for c in 0..3 {
                    p[c] = rgb[c] * p[3];
                }
            })?;
        }
        Stage::Straight => visit_mut(&mut image.pixels, cancelled, |_, p| {
            if p[3] > 0. {
                let rgb = detail::straight(*p);
                p[..3].copy_from_slice(&rgb);
            }
        })?,
        Stage::Layer(layer) => {
            let stack = LayerStack {
                base_process: 3,
                layers: vec![layer.clone()],
            };
            let prepared = stack.prepare(native[1] as f32 / native[0] as f32);
            visit_mut(&mut image.pixels, cancelled, |i, p| {
                if p[3] <= 0. {
                    return;
                }
                let rgb = prepared[0].apply([p[0], p[1], p[2]], xy(i));
                p[..3].copy_from_slice(&rgb);
            })?;
        }
        Stage::Premultiply => visit_mut(&mut image.pixels, cancelled, |_, p| {
            if p[3] > 0. {
                for c in 0..3 {
                    p[c] *= p[3];
                }
            }
        })?,
    }
    Ok(())
}

fn spatial_support(recipe: &EditRecipe, native: [u32; 2], full: [u32; 2]) -> (u32, bool) {
    let Some(a) = &recipe.advanced else {
        return (0, false);
    };
    let d = &a.detail;
    let scale = full[0].max(full[1]) as f32 / native[0].max(native[1]) as f32;
    let noise = d.luminance_noise != 0. || d.chroma_noise != 0.;
    let global = d.dehaze != 0. || (noise && d.denoise != Denoise::Legacy);
    let mut halo = u32::from(a.geometry.defringe != 0.);
    if noise && d.denoise == Denoise::Legacy {
        halo += (2. * scale).round().max(1.) as u32;
    }
    // Stable support across a slider crossing zero (or changing sharpness
    // radius). This modest extra halo prevents invalidating the noise prefix.
    halo += (2. * scale).round().max(1.) as u32;
    halo += (native[0].max(native[1]) as f32 * 0.008 * scale)
        .round()
        .clamp(1., 512.) as u32;
    halo += ((3. * scale).max(0.15) * 2.).ceil() as u32;
    (halo, global)
}

#[derive(Clone, Copy, Default)]
struct Sample {
    xy: [u32; 2],
    fraction: [f32; 2],
    valid: bool,
}
impl Sample {
    fn new(p: [f64; 2], full: [u32; 2]) -> Self {
        if p.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v)) {
            return Self::default();
        }
        let at: [f64; 2] = std::array::from_fn(|a| {
            (p[a] * f64::from(full[a]) - 0.5).clamp(0., f64::from(full[a] - 1))
        });
        Self {
            xy: at.map(|v| v.floor() as u32),
            fraction: at.map(|v| v.fract() as f32),
            valid: true,
        }
    }
    fn read(self, image: &LinearImage, origin: [u32; 2], full: [u32; 2]) -> Pixel {
        if !self.valid {
            return [0.; 4];
        }
        let get = |x: u32, y: u32| {
            image.pixels[((y.min(full[1] - 1) - origin[1]) * image.width + x.min(full[0] - 1)
                - origin[0]) as usize]
        };
        let [x, y] = self.xy;
        let [fx, fy] = self.fraction;
        let (a, b, c, d) = (get(x, y), get(x + 1, y), get(x, y + 1), get(x + 1, y + 1));
        std::array::from_fn(|i| {
            (a[i] + (b[i] - a[i]) * fx) * (1. - fy) + (c[i] + (d[i] - c[i]) * fx) * fy
        })
    }
}
struct Mapping {
    geometry: Geometry,
    output: Rect,
    native: [u32; 2],
    full: [u32; 2],
    output_full: [u32; 2],
    source: Rect,
    samples: Vec<[Sample; 3]>,
    lease: Option<Lease>,
}
fn points(geometry: &Geometry, p: [f64; 2], native: [u32; 2]) -> [[f64; 2]; 3] {
    let mut colors = [p; 3];
    for (c, amount) in [(0, geometry.ca[0]), (2, geometry.ca[1])] {
        let gain = (1. + 2. * amount as f64 / native[0].max(native[1]) as f64).max(0.05);
        colors[c] = p.map(|v| 0.5 + (v - 0.5) * gain);
    }
    colors
}
impl Mapping {
    fn new(
        geometry: Geometry,
        output: Rect,
        native: [u32; 2],
        full: [u32; 2],
        output_full: [u32; 2],
        lease: Option<Lease>,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Self> {
        let rotation = (geometry.angle as f64).to_radians().sin_cos();
        let mut samples = vec![[Sample::default(); 3]; output.pixels() as usize];
        visit_mut(&mut samples, cancelled, |i, samples| {
            let p = [
                (output.origin[0] as usize + i % output.size[0] as usize) as f64 + 0.5,
                (output.origin[1] as usize + i / output.size[0] as usize) as f64 + 0.5,
            ];
            if let Some(p) = geometry.source_point_with_rotation(
                [
                    p[0] / f64::from(output_full[0]),
                    p[1] / f64::from(output_full[1]),
                ],
                native,
                rotation,
            ) {
                let colors = if geometry.ca == [0.; 2] {
                    [p; 3]
                } else {
                    points(&geometry, p, native)
                };
                *samples = colors.map(|p| Sample::new(p, full));
            }
        })?;
        let mut lo = full;
        let mut hi = [0; 2];
        for batch in samples.chunks(2048) {
            ensure!(!cancelled(), "Editing annullato");
            for sample in batch.iter().flatten().filter(|s| s.valid) {
                for a in 0..2 {
                    lo[a] = lo[a].min(sample.xy[a]);
                    hi[a] = hi[a].max((sample.xy[a] + 2).min(full[a]));
                }
            }
        }
        let source = if hi == [0; 2] {
            Rect {
                origin: [0; 2],
                size: [1; 2],
            }
        } else {
            Rect {
                origin: lo,
                size: std::array::from_fn(|a| hi[a] - lo[a]),
            }
        };
        Ok(Self {
            geometry,
            output,
            native,
            full,
            output_full,
            source,
            samples,
            lease,
        })
    }
    fn apply(
        &self,
        image: &LinearImage,
        origin: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<LinearImage> {
        let mut pixels = vec![[0.; 4]; self.output.pixels() as usize];
        visit_mut(&mut pixels, cancelled, |i, target| {
            let samples = self.samples[i];
            *target = samples[1].read(image, origin, self.full);
            if self.geometry.ca != [0.; 2] && target[3] > 0. {
                let colors = [
                    samples[0].read(image, origin, self.full),
                    *target,
                    samples[2].read(image, origin, self.full),
                ];
                let alpha = colors.iter().map(|p| p[3]).fold(1_f32, f32::min);
                for c in 0..3 {
                    target[c] = if colors[c][3] > 0. {
                        colors[c][c] / colors[c][3] * alpha
                    } else {
                        0.
                    };
                }
                target[3] = alpha;
            }
        })?;
        LinearImage::new(self.output.size[0], self.output.size[1], pixels)
    }
}
fn output_opaque(
    geometry: &Geometry,
    native: [u32; 2],
    full: [u32; 2],
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<bool> {
    if geometry.angle == 0.
        && geometry.perspective == [0.; 2]
        && geometry.distortion == 0.
        && geometry.moustache == 0.
        && geometry.ca == [0.; 2]
    {
        return Ok(true);
    }
    let rotation = (geometry.angle as f64).to_radians().sin_cos();
    let opaque = crate::compute::install(|| {
        (0..u64::from(full[0]) * u64::from(full[1]))
            .into_par_iter()
            .all(|i| {
                if i % 2048 == 0 && cancelled() {
                    return false;
                }
                let xy = [
                    ((i % u64::from(full[0])) as f64 + 0.5) / f64::from(full[0]),
                    ((i / u64::from(full[0])) as f64 + 0.5) / f64::from(full[1]),
                ];
                geometry
                    .source_point_with_rotation(xy, native, rotation)
                    .is_some_and(|p| {
                        points(geometry, p, native)
                            .iter()
                            .flatten()
                            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                    })
            })
    });
    ensure!(!cancelled(), "Editing annullato");
    Ok(opaque)
}

#[cfg(test)]
mod tests;
