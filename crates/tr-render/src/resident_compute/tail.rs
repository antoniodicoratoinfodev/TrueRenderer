//! Retained CPU prefix and canonical inverse map; per-gesture layer arithmetic,
//! geometry and resampling stay on the GPU. Only the UI submits these commands.
use super::*;
use tr_core::editing::{
    EditRecipe,
    layers::MaskGraph,
    regional::{Processor, tail::LayerInputPlan},
};

#[derive(PartialEq)]
struct Key {
    image: u64,
    prefix: EditRecipe,
    gains: [f32; 3],
    region: Region,
    mask: MaskGraph,
}
impl Key {
    fn new(image: &ImageLevels, edit: &crate::live_edit::LiveEdit, region: Region) -> Result<Self> {
        edit.recipe.validate()?;
        let stack = edit.recipe.layers.as_ref().context("Nessun livello")?;
        crate::layer_compute::parameters(stack)?;
        let first = stack
            .layers
            .iter()
            .find(|l| l.enabled && !l.operators.is_empty())
            .context("Livelli neutri")?;
        let mut prefix = edit.recipe.clone();
        prefix.layers.as_mut().unwrap().layers.clear();
        Ok(Self {
            image: image.id(),
            prefix,
            gains: edit.input_gains,
            region,
            mask: first.mask.clone(),
        })
    }
}
pub(super) struct LayerSource {
    key: Key,
    input: wgpu::Buffer,
    mask: wgpu::Buffer,
    geometry: Option<(wgpu::Buffer, wgpu::Buffer)>,
    input_size: [u32; 2],
    mapped_size: [u32; 2],
    bound: f64,
    coefficients: [wgpu::Buffer; 5],
    rows: u32,
    _memory: Lease,
    _gpu: Lease,
}
impl GpuFilter {
    #[allow(clippy::too_many_arguments)]
    fn layer_source(
        &self,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        processor: &mut Processor,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Arc<LayerSource>> {
        ensure!(!cancelled(), "Editing annullato");
        let key = Key::new(image, edit, region)?;
        let stack = edit.recipe.layers.as_ref().unwrap();
        let cached = self
            .layer_sources
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| s.key == key)
            .cloned();
        if let Some(source) = cached {
            if memory.usage().reserved > memory.usage().limit
                || gpu.usage().reserved > gpu.usage().limit
                || source._memory.bytes() > cache_limit(memory, gpu)
            {
                let evicted = self.layer_sources.lock().unwrap().take();
                drop(evicted);
                anyhow::bail!("Quota prefisso GPU ridotta");
            }
            crate::layer_compute::output_bound(stack, source.bound)?;
            return Ok(source);
        }
        // An immutable pending frame owns its leases independently of eviction.
        let old = self.layer_sources.lock().unwrap().take();
        drop(old);
        let plan = LayerInputPlan::new(image, &edit.recipe, region, cancelled)?;
        let input_bytes = u64::from(plan.input_size[0]) * u64::from(plan.input_size[1]) * 16;
        let coordinates = plan.geometry_samples();
        let map_bytes = coordinates.len() as u64 * 16;
        let has_mask = key.mask.root.is_some();
        let mask_bytes = if has_mask { input_bytes / 4 } else { 4 };
        let (mut xs, mut ys) =
            tr_core::resample::coefficients(plan.output_full, plan.display_region, plan.opaque)?;
        for (taps, origin) in [
            (&mut xs, plan.mapped_origin[0]),
            (&mut ys, plan.mapped_origin[1]),
        ] {
            for (index, _) in taps.iter_mut().flatten() {
                *index -= origin as usize;
            }
        }
        let min_y = ys.iter().flatten().map(|(y, _)| *y).min().unwrap() as u32;
        let max_y = ys.iter().flatten().map(|(y, _)| *y).max().unwrap() as u32;
        let rows = max_y - min_y + 1;
        let (xr, xt) = crate::preview_compute::taps(&xs);
        let (yr, yt) = crate::preview_compute::taps(&ys);
        let coefficient_bytes = (xr.len() + xt.len() + yr.len() + yt.len()) as u64 + 32;
        let retained = input_bytes + mask_bytes + map_bytes + 32 + coefficient_bytes + 128 * 1024;
        let maximum = cache_limit(memory, gpu);
        ensure!(
            retained <= maximum
                && [input_bytes, mask_bytes, map_bytes]
                    .iter()
                    .all(|v| *v <= self.device.limits().max_storage_buffer_binding_size),
            "Prefisso livelli GPU oltre quota"
        );
        let source_memory = memory
            .try_reserve_within_limit(retained)
            .context("Quota prefisso livelli")?;
        let source_gpu = gpu
            .try_reserve_within_limit(retained)
            .context("Quota GPU livelli")?;
        let upload = memory
            .try_reserve_within_limit(input_bytes + mask_bytes + map_bytes)
            .context("Quota upload livelli")?;
        let raster = plan.render_input(processor, image, edit.input_gains, cancelled)?;
        let mut bound = 0_f64;
        for batch in raster.pixels.chunks(2048) {
            ensure!(!cancelled(), "Editing annullato");
            for p in batch {
                for c in &p[..3] {
                    let value = if p[3] > 0. { *c / p[3] } else { *c };
                    ensure!(value.is_finite(), "Ingresso livelli non finito");
                    bound = bound.max(f64::from(value.abs()));
                }
            }
        }
        crate::layer_compute::output_bound(stack, bound)?;
        let mask = plan.first_mask(&raster, stack, cancelled)?;
        let storage = |bytes: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("TR retained layer prefix"),
                    contents: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        // Supported native hosts are little endian; reject elsewhere in dispatch.
        let input = storage(bytemuck::cast_slice(&raster.pixels));
        let mask = storage(bytemuck::cast_slice(&mask));
        let geometry = if coordinates.is_empty() {
            None
        } else {
            let params = [
                plan.input_size[0] as f32,
                plan.input_origin[0] as f32,
                plan.input_origin[1] as f32,
                0.,
                plan.source_full[0] as f32,
                plan.source_full[1] as f32,
                u32::from(plan.chromatic_aberration()) as f32,
                0.,
            ];
            Some((
                storage(bytemuck::cast_slice(&coordinates)),
                storage(bytemuck::cast_slice(&params)),
            ))
        };
        let params = [
            plan.mapped_size[0],
            region.size[0],
            region.size[1],
            min_y,
            rows,
            u32::from(plan.opaque),
            0,
            0,
        ];
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let coefficients = [
            uniform,
            storage(&xr),
            storage(&xt),
            storage(&yr),
            storage(&yt),
        ];
        drop(upload);
        ensure!(!cancelled(), "Editing annullato");
        let source = Arc::new(LayerSource {
            key,
            input,
            mask,
            geometry,
            input_size: plan.input_size,
            mapped_size: plan.mapped_size,
            bound,
            coefficients,
            rows,
            _memory: source_memory,
            _gpu: source_gpu,
        });
        let old = self.layer_sources.lock().unwrap().replace(source.clone());
        drop(old);
        Ok(source)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_layers(
        &self,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        processor: &mut Processor,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        warm: bool,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<Option<GpuFrame>> {
        ensure!(
            cfg!(target_endian = "little"),
            "Endian livelli GPU non qualificato"
        );
        ensure!(!image.scientific(), "Layer tail needs photographic input");
        let source = self.layer_source(image, edit, region, processor, memory, gpu, cancelled)?;
        let pipeline = self
            .layer_compute
            .get_or_init(|| crate::layer_compute::LayerCompute::new(&self.device));
        if warm {
            return Ok(None);
        }
        self.render_layer_source(
            source, edit, region, pipeline, memory, gpu, working, false, cancelled,
        )
        .map(Some)
    }
    /// Encode from an already prepared prefix without waiting for its worker,
    /// compiling a pipeline, evaluating pixels or uploading an image on the UI.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_resident_layers(
        &self,
        image: &ImageLevels,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
    ) -> Result<GpuFrame> {
        let _span = crate::timings::CpuSpan::new("resident_layer_prepare_cpu");
        ensure!(
            cfg!(target_endian = "little") && !image.scientific(),
            "Dominio livelli GPU"
        );
        let key = Key::new(image, edit, region)?;
        let source = self
            .layer_sources
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Prefisso livelli occupato"))?
            .as_ref()
            .filter(|s| s.key == key)
            .cloned()
            .context("Prefisso livelli non residente")?;
        ensure!(
            memory.usage().reserved <= memory.usage().limit
                && gpu.usage().reserved <= gpu.usage().limit
                && source._memory.bytes() <= cache_limit(memory, gpu),
            "Quota prefisso livelli ridotta"
        );
        crate::layer_compute::output_bound(edit.recipe.layers.as_ref().unwrap(), source.bound)?;
        let pipeline = self
            .layer_compute
            .get()
            .context("Pipeline livelli non preparata")?;
        self.render_layer_source(
            source,
            edit,
            region,
            pipeline,
            memory,
            gpu,
            working,
            true,
            &|| false,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn render_layer_source(
        &self,
        source: Arc<LayerSource>,
        edit: &crate::live_edit::LiveEdit,
        region: Region,
        pipeline: &crate::layer_compute::LayerCompute,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        working: &mut Lease,
        resident_only: bool,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<GpuFrame> {
        let stack = edit.recipe.layers.as_ref().unwrap();
        let params = crate::layer_compute::parameters(stack)?;
        let input_bytes = u64::from(source.input_size[0]) * u64::from(source.input_size[1]) * 16;
        let mapped_bytes = if source.geometry.is_some() {
            u64::from(source.mapped_size[0]) * u64::from(source.mapped_size[1]) * 16
        } else {
            0
        };
        let horizontal_bytes = u64::from(region.size[0]) * u64::from(source.rows) * 16;
        let scratch_bytes =
            input_bytes + mapped_bytes + horizontal_bytes + params.len() as u64 * 16 + 16;
        let limits = self.device.limits();
        ensure!(
            [input_bytes, mapped_bytes, horizontal_bytes]
                .iter()
                .all(|v| *v <= limits.max_storage_buffer_binding_size)
                && region
                    .size
                    .iter()
                    .all(|n| *n <= limits.max_texture_dimension_2d),
            "Frame livelli oltre capability"
        );
        let display_bytes = u64::from(region.size[0])
            * u64::from(region.size[1])
            * if self.format == wgpu::TextureFormat::Rgba16Float {
                24
            } else {
                12
            };
        ensure!(
            working.bytes() >= scratch_bytes + display_bytes,
            "Frame livelli oltre prenotazione"
        );
        let scratch_gpu = if resident_only {
            gpu.try_reserve_within_limit(scratch_bytes)
        } else {
            gpu.try_reserve(scratch_bytes)
        }
        .context("Scratch livelli GPU oltre quota")?;
        let buffer = |size| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TR layer scratch"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let edited = buffer(input_bytes);
        let mapped = (mapped_bytes != 0).then(|| buffer(mapped_bytes));
        let horizontal = buffer(horizontal_bytes);
        let dummy = buffer(16);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("TR GPU layer viewport"),
            size: wgpu::Extent3d {
                width: region.size[0],
                height: region.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let buffers = [
            &source.coefficients[0],
            mapped.as_ref().unwrap_or(&edited),
            &horizontal,
            &dummy,
            &source.coefficients[1],
            &source.coefficients[2],
            &source.coefficients[3],
            &source.coefficients[4],
        ];
        let mut entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: wgpu::BindingResource::TextureView(&view),
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut timer = crate::timings::GpuTimer::new(&self.device, memory, gpu);
        if let Some(t) = &mut timer {
            t.begin(&mut encoder, "gpu_layer_tail");
        }
        pipeline.encode(
            &self.device,
            &mut encoder,
            &source.input,
            &edited,
            &source.mask,
            stack,
            (input_bytes / 16) as u32,
        )?;
        if let Some((map, params)) = &source.geometry {
            pipeline.geometry(
                &self.device,
                &mut encoder,
                &edited,
                mapped.as_ref().unwrap(),
                map,
                params,
                (mapped_bytes / 16) as u32,
            );
        }
        for (pipeline, height) in self.pipelines.iter().zip([source.rows, region.size[1]]) {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(region.size[0].div_ceil(8), height.div_ceil(8), 1);
        }
        if let Some(t) = &timer {
            t.end(&mut encoder);
            t.resolve(&mut encoder);
        }
        ensure!(!cancelled(), "Editing annullato");
        Ok(GpuFrame {
            texture,
            size: region.size,
            pending: Some(Box::new(PendingSubmission {
                commands: vec![encoder.finish()],
                chain: None,
                scratch: working.split(scratch_bytes).unwrap(),
                scratch_gpu,
                source,
                timer,
            })),
        })
    }
}

fn cache_limit(memory: &MemoryBudget, gpu: &MemoryBudget) -> u64 {
    (memory.usage().limit / 16)
        .min(gpu.usage().limit / 8)
        .min(128 * 1024 * 1024)
}

pub(crate) fn check(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    result: &mut crate::layer_compute::Check,
) -> Result<()> {
    use std::time::Duration;
    use tr_core::{
        color::LinearImage,
        editing::{Advanced, detail::Denoise, layers::*},
    };
    let memory = MemoryBudget::new(128 * 1024 * 1024);
    let gpu = MemoryBudget::new(64 * 1024 * 1024);
    let renderer = GpuFilter::new(device.clone());
    let mut processor = Processor::new(memory.clone());
    for alpha in [false, true] {
        for base in [0, 2] {
            let source = LinearImage::new(
                129,
                97,
                (0..129 * 97)
                    .map(|i| {
                        let a = if alpha {
                            [0., 0.01, 0.5, 1.][i as usize % 4]
                        } else {
                            1.
                        };
                        [
                            ((i * 17 % 113) as f32 / 107. - 0.05) * a,
                            (i * 31 % 71) as f32 / 83. * a,
                            (i % 3) as f32 * 0.4 * a,
                            a,
                        ]
                    })
                    .collect(),
            )?;
            let image = ImageLevels::from_reference_mip(
                source,
                if base == 0 { [129, 97] } else { [515, 387] },
                base,
                !alpha,
            )?;
            for geometry in 0..4 {
                let mut recipe = EditRecipe::neutral(Default::default());
                recipe.process_version = 3;
                recipe.exposure_ev = 0.15;
                let mut advanced = Advanced::default();
                advanced.detail.texture = 12.;
                advanced.detail.luminance_noise = 15.;
                advanced.detail.chroma_noise = 25.;
                advanced.detail.denoise = Denoise::MultiscaleV2;
                if geometry > 0 {
                    advanced.geometry.crop = [0.07, 0.03, 0.93, 0.95];
                    advanced.geometry.quarter_turns = 1;
                    advanced.geometry.angle = 3.;
                    advanced.geometry.vignette = 12.;
                }
                if geometry > 1 {
                    advanced.geometry.ca = [0.7, -0.4];
                }
                if geometry > 2 {
                    advanced.geometry.angle = -17.;
                }
                recipe.advanced = Some(Box::new(advanced));
                let mut layer = Layer::new(
                    "sample",
                    Operator::SampleColor(SampleColor {
                        correction: [35., 15., -10.],
                        uniformity: [20., 30., 0.],
                        range: ColorRange {
                            width: [180., 3., 3.],
                            ..Default::default()
                        },
                    }),
                );
                layer.mask.append(
                    MaskKind::Color(ColorRange {
                        width: [120., 3., 3.],
                        ..Default::default()
                    }),
                    Combine::Add,
                );
                recipe.layer_stack().layers.push(layer);
                let mut levels = [TonalLevel::default(); 4];
                levels[0].gamma = 1.12;
                recipe.layer_stack().layers.push(Layer::new(
                    "levels",
                    if geometry % 2 == 0 {
                        Operator::Levels(levels)
                    } else {
                        Operator::TonalLevels(Box::new(LevelAdjustments {
                            channels: levels,
                            ..Default::default()
                        }))
                    },
                ));
                if geometry == 0 {
                    recipe.layers.as_mut().unwrap().layers[1].enabled = false;
                }
                for step in [0.7, 1., 1.91] {
                    let region = Region {
                        size: [31, 29],
                        origin: [-1.3, 3.7].map(|v| v * f64::from(1 << base)),
                        step: [step * f64::from(1 << base); 2],
                    };
                    let mut edit = crate::live_edit::LiveEdit {
                        recipe: recipe.clone(),
                        input_gains: [1.1, 1., 0.9],
                        max_edge: 1024,
                        revision: 0,
                    };
                    let mut previous: Option<Arc<LayerSource>> = None;
                    for change in 0..5 {
                        let first = &mut edit.recipe.layers.as_mut().unwrap().layers[0];
                        first.opacity = [0.3, 0., 1., 0.4, 0.7][change];
                        if let Operator::SampleColor(s) = &mut first.operators[0] {
                            s.correction[0] = 20. + change as f32 * 13.;
                        }
                        if change == 2 {
                            first.mask.nodes.clear();
                            first.mask.root = None;
                        }
                        if change == 3 {
                            edit.recipe.exposure_ev += 0.2;
                        }
                        let mut working = memory.try_reserve(16 * 1024 * 1024).unwrap();
                        let resident = change == 1 || change == 4;
                        let mut frame = if resident {
                            let frame = renderer.render_resident_layers(
                                &image,
                                &edit,
                                region,
                                &memory,
                                &gpu,
                                &mut working,
                            )?;
                            result.resident_cases += 1;
                            frame
                        } else {
                            let before = (memory.usage().reserved, gpu.usage().reserved);
                            ensure!(
                                renderer
                                    .render_resident_layers(
                                        &image,
                                        &edit,
                                        region,
                                        &memory,
                                        &gpu,
                                        &mut working,
                                    )
                                    .is_err(),
                                "Prefisso obsoleto riusato dal frame immediato"
                            );
                            ensure!(
                                before == (memory.usage().reserved, gpu.usage().reserved),
                                "Cache miss residente alloca pixel"
                            );
                            result.resident_miss_cases += 1;
                            renderer
                                .render_layers(
                                    &image,
                                    &edit,
                                    region,
                                    &mut processor,
                                    &memory,
                                    &gpu,
                                    &mut working,
                                    false,
                                    &|| false,
                                )?
                                .unwrap()
                        };
                        let source = renderer
                            .layer_sources
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .clone();
                        if let Some(previous) = previous {
                            ensure!(
                                Arc::ptr_eq(&source, &previous) == resident,
                                "Identita prefisso livelli non valida"
                            );
                            if resident {
                                result.reuse_cases += 1;
                            } else {
                                result.invalidation_cases += 1;
                            }
                        }
                        previous = Some(source);
                        frame.submit(queue);
                        let actual = crate::preview_compute::read_display(device, queue, &frame)?;
                        // Full canonical graph, independent of LayerInputPlan and its ROI.
                        let mut reference = image.source().clone();
                        for p in &mut reference.pixels {
                            for (c, g) in p[..3].iter_mut().zip(edit.input_gains) {
                                *c *= g;
                            }
                        }
                        let output = edit.recipe.apply_preview(
                            &mut reference,
                            image.source_size(),
                            image.base_level(),
                        )?;
                        let opaque = reference.pixels.iter().all(|p| p[3] == 1.);
                        if change == 0 {
                            check_linear(
                                device,
                                queue,
                                &renderer,
                                previous.as_ref().unwrap(),
                                edit.recipe.layers.as_ref().unwrap(),
                                &LayerInputPlan::new(&image, &edit.recipe, region, &|| false)?,
                                &reference,
                                result,
                            )?;
                        }
                        let reference = ImageLevels::from_reference_mip(
                            reference,
                            output,
                            image.base_level(),
                            opaque,
                        )?
                        .render(region)?
                        .to_display();
                        for (a, b) in actual.iter().zip(reference) {
                            let error = a.abs_diff(b);
                            result.display_maximum_error = result.display_maximum_error.max(error);
                            result.display_failures += usize::from(error > 1);
                        }
                        result.composed_cases += 1;
                    }
                }
            }
        }
    }
    renderer.clear();
    processor.clear();
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    ensure!(
        memory.usage().reserved == 0 && gpu.usage().reserved == 0,
        "Crediti livelli trattenuti"
    );
    result.lifetime_cases += 1;
    check_lifetimes(device, queue, result)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_linear(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &GpuFilter,
    source: &LayerSource,
    stack: &tr_core::editing::layers::LayerStack,
    plan: &LayerInputPlan,
    reference: &tr_core::color::LinearImage,
    result: &mut crate::layer_compute::Check,
) -> Result<()> {
    use std::time::Duration;
    let pipeline = renderer
        .layer_compute
        .get_or_init(|| crate::layer_compute::LayerCompute::new(device));
    let storage = |size| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("TR linear layer qualification"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    };
    let edited = storage(u64::from(source.input_size[0]) * u64::from(source.input_size[1]) * 16);
    let bytes = u64::from(source.mapped_size[0]) * u64::from(source.mapped_size[1]) * 16;
    let mapped = source.geometry.as_ref().map(|_| storage(bytes));
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    pipeline.encode(
        device,
        &mut encoder,
        &source.input,
        &edited,
        &source.mask,
        stack,
        source.input_size[0] * source.input_size[1],
    )?;
    if let Some((map, params)) = &source.geometry {
        pipeline.geometry(
            device,
            &mut encoder,
            &edited,
            mapped.as_ref().unwrap(),
            map,
            params,
            (bytes / 16) as u32,
        );
    }
    encoder.copy_buffer_to_buffer(mapped.as_ref().unwrap_or(&edited), 0, &readback, 0, bytes);
    let submission = queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(10)),
    })?;
    rx.recv_timeout(Duration::from_secs(10))??;
    let mapped = readback.slice(..).get_mapped_range()?;
    for (i, pixel) in mapped.as_chunks::<16>().0.iter().enumerate() {
        let x = plan.mapped_origin[0] as usize + i % source.mapped_size[0] as usize;
        let y = plan.mapped_origin[1] as usize + i / source.mapped_size[0] as usize;
        let expected = reference.pixels[y * reference.width as usize + x];
        for (channel, e) in pixel.as_chunks::<4>().0.iter().zip(expected) {
            let a = f32::from_le_bytes(*channel);
            let error = (a - e).abs();
            let tolerance = 1e-5 + 1e-4 * e.abs();
            result.samples += 1;
            result.failures += usize::from(!a.is_finite() || error > tolerance);
            result.maximum_error = result.maximum_error.max(error);
            result.worst_ratio = result.worst_ratio.max(error / tolerance);
        }
    }
    drop(mapped);
    readback.unmap();
    result.mapped_linear_cases += 1;
    Ok(())
}

fn check_lifetimes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    result: &mut crate::layer_compute::Check,
) -> Result<()> {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tr_core::{
        color::LinearImage,
        editing::layers::{Layer, Operator},
        preview::PreviewRequest,
    };
    let memory = MemoryBudget::new(128 * 1024 * 1024);
    let gpu = MemoryBudget::new(64 * 1024 * 1024);
    let renderer = GpuFilter::new(device.clone());
    let mut processor = Processor::new(memory.clone());
    let image = ImageLevels::from_source(
        LinearImage::new(129, 97, vec![[0.2, 0.3, 0.4, 1.]; 129 * 97])?,
        PreviewRequest::full(),
    )?;
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.layer_stack().layers.push(Layer::new(
        "light",
        Operator::Light {
            exposure: 0.3,
            temperature: 5.,
            tint: -2.,
            saturation: 10.,
        },
    ));
    let edit = crate::live_edit::LiveEdit {
        recipe,
        input_gains: [1.; 3],
        max_edge: 1024,
        revision: 0,
    };
    let region = Region {
        size: [47, 41],
        origin: [13.5, 17.3],
        step: [1.; 2],
    };
    let baseline = || (memory.usage().reserved, gpu.usage().reserved);
    {
        let mut working = memory.try_reserve(16 * 1024 * 1024).unwrap();
        let before = baseline();
        ensure!(
            renderer
                .render_resident_layers(&image, &edit, region, &memory, &gpu, &mut working)
                .is_err(),
            "Frame immediato prepara una sorgente assente"
        );
        ensure!(
            renderer.layer_compute.get().is_none() && baseline() == before,
            "Cache miss immediato compila o alloca"
        );
        let held = renderer.layer_sources.lock().unwrap();
        ensure!(
            renderer
                .render_resident_layers(&image, &edit, region, &memory, &gpu, &mut working)
                .is_err(),
            "Frame immediato attende il worker"
        );
        drop(held);
        ensure!(baseline() == before, "Cache occupata trattiene crediti");
        result.resident_miss_cases += 2;
    }
    ensure!(
        renderer
            .layer_source(
                &image,
                &edit,
                region,
                &mut processor,
                &memory,
                &gpu,
                &|| true
            )
            .is_err(),
        "Annullamento livelli ignorato"
    );
    ensure!(baseline() == (0, 0), "Crediti allocati dopo annullamento");
    result.lifetime_cases += 1;
    let calls = AtomicUsize::new(0);
    ensure!(
        renderer
            .layer_source(
                &image,
                &edit,
                region,
                &mut processor,
                &memory,
                &gpu,
                &|| calls.fetch_add(1, Ordering::Relaxed) >= 2
            )
            .is_err(),
        "Preparazione livelli non interrotta"
    );
    renderer.clear();
    processor.clear();
    ensure!(baseline() == (0, 0), "Crediti dopo interruzione");
    result.lifetime_cases += 1;
    let source = renderer.layer_source(
        &image,
        &edit,
        region,
        &mut processor,
        &memory,
        &gpu,
        &|| false,
    )?;
    gpu.configure(source._gpu.bytes() * 4);
    {
        let mut working = memory.try_reserve(16 * 1024 * 1024).unwrap();
        let before = baseline();
        ensure!(
            renderer
                .render_resident_layers(&image, &edit, region, &memory, &gpu, &mut working)
                .is_err(),
            "Frame immediato ignora la quota ridotta"
        );
        ensure!(baseline() == before, "Frame immediato oltre quota alloca");
        result.resident_miss_cases += 1;
    }
    ensure!(
        renderer
            .layer_source(
                &image,
                &edit,
                region,
                &mut processor,
                &memory,
                &gpu,
                &|| false
            )
            .is_err(),
        "Quota ridotta ignorata"
    );
    gpu.configure(64 * 1024 * 1024);
    drop(source);
    result.lifetime_cases += 1;
    let replacement = ImageLevels::from_source(image.source().clone(), PreviewRequest::full())?;
    let old = renderer.layer_source(
        &image,
        &edit,
        region,
        &mut processor,
        &memory,
        &gpu,
        &|| false,
    )?;
    let new = renderer.layer_source(
        &replacement,
        &edit,
        region,
        &mut processor,
        &memory,
        &gpu,
        &|| false,
    )?;
    ensure!(
        !Arc::ptr_eq(&old, &new),
        "Sorgente nuova riusa il prefisso precedente"
    );
    drop((old, new));
    result.lifetime_cases += 1;
    for submit in [false, true] {
        let mut working = memory.try_reserve(16 * 1024 * 1024).unwrap();
        renderer.render_layers(
            &image,
            &edit,
            region,
            &mut processor,
            &memory,
            &gpu,
            &mut working,
            true,
            &|| false,
        )?;
        let mut frame =
            renderer.render_resident_layers(&image, &edit, region, &memory, &gpu, &mut working)?;
        renderer.clear();
        processor.clear();
        drop(working);
        ensure!(
            baseline().0 > 0 && baseline().1 > 0,
            "Crediti frame rilasciati in anticipo"
        );
        if submit {
            frame.submit(queue);
        }
        drop(frame);
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        ensure!(baseline() == (0, 0), "Crediti trattenuti dal frame livelli");
        result.lifetime_cases += 1;
    }
    Ok(())
}
