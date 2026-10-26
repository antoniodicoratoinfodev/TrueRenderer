//! Full CPU recipes verify cached downstream changes and every upstream invalidator.
use super::*;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tr_core::{budget::MemoryBudget, editing::CurvePoint};

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub numerical: spatial_check::Check,
    pub lifecycle_cases: usize,
    pub hits: usize,
    pub misses: usize,
    pub final_memory_reserved: u64,
    pub final_gpu_reserved: u64,
}
struct Pipelines {
    point: point::Point,
    noise: multiscale::Multiscale,
    spatial: spatial::Spatial,
    post: post::Post,
    layers: layers::Layers,
}
impl Pipelines {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            point: point::Point::new(device),
            noise: multiscale::Multiscale::new(device),
            spatial: spatial::Spatial::new(device),
            post: post::Post::new(device),
            layers: layers::Layers::new(device),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &self,
        device: &wgpu::Device,
        source: Arc<admission::Source>,
        optics: Arc<point::Optics>,
        edit: &crate::live_edit::LiveEdit,
        request: admission::Request,
        memory: &MemoryBudget,
        gpu: &MemoryBudget,
        cache: &checkpoint::Checkpoint,
    ) -> Result<admission::Pending> {
        let native = source.size;
        admission::Pending::prepare_cached(
            device,
            &self.noise,
            source,
            request,
            memory,
            gpu,
            Some((&self.point, optics, edit)),
            Some((&self.spatial, native)),
            Some((&self.post, native)),
            Some((&self.layers, native)),
            Some(cache),
            &|| false,
        )
    }
}
#[allow(clippy::too_many_arguments)]
fn compare(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &LinearImage,
    edit: &crate::live_edit::LiveEdit,
    request: admission::Request,
    output: &admission::Output,
    label: String,
    result: &mut Check,
) -> Result<()> {
    let mut expected = source.clone();
    for pixel in &mut expected.pixels {
        for (value, gain) in pixel[..3].iter_mut().zip(edit.input_gains) {
            *value *= gain;
        }
    }
    edit.recipe.apply(&mut expected)?;
    if std::env::var("TR_GPU_COMPARE_TRACE").is_ok_and(|v| v == "1") {
        eprintln!("Compare {label}");
    }
    spatial_check::compare(
        device,
        queue,
        &output.buffer,
        &spatial_check::extract(&expected, request.origin, request.size)?,
        label,
        &mut result.numerical,
    )
}
pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let pipelines = Pipelines::new(device);
    let mut result = Check::default();
    let size = [517, 269];
    for pattern in [4, 8] {
        let source = reconstruction::source(size, pattern)?;
        let memory = MemoryBudget::new(256 * 1024 * 1024);
        let gpu = MemoryBudget::new(256 * 1024 * 1024);
        let cache = checkpoint::Checkpoint::default();
        let mut uploaded = admission::Upload::prepare(device, &source, &memory, &gpu, &|| false)?
            .submit(queue, &|| false)?;
        let mut recipe = post_check::recipe(8);
        recipe.exposure_ev = 0.25;
        recipe.temperature = 8.;
        let a = recipe.advanced.as_mut().unwrap();
        a.detail = spatial_check::recipe(7).advanced.unwrap().detail;
        a.detail.luminance_noise = 25.;
        a.detail.chroma_noise = 35.;
        a.geometry.vignette = 15.;
        a.geometry.defringe = 20.;
        layers_check::add_layers(&mut recipe, 5);
        let mut optics = Arc::new(point::Optics::prepare(
            device,
            size,
            15.,
            &memory,
            &gpu,
            &|| false,
        )?);
        let mut edit = crate::live_edit::LiveEdit {
            recipe,
            input_gains: [1.; 3],
            max_edge: 0,
            revision: 1,
        };
        let mut request = admission::Request {
            n: 16,
            amounts: [0.25, 0.35],
            threshold: 0.25,
            levels: 3,
            origin: [119, 35],
            size: [257, 193],
        };
        for case in 0..21 {
            let a = edit.recipe.advanced.as_mut().unwrap();
            match case {
                1 => a.detail.texture = -30.,
                2 => a.detail.texture = 0.,
                3 => a.detail.texture = 60.,
                4 => a.detail.clarity = -50.,
                5 => a.detail.sharpen = 0.,
                6 => a.geometry.defringe = 0.,
                7 => a.color.bands[2].hue = 40.,
                8 => a.masks[0].exposure = 1.,
                9 => edit.recipe.layers.as_mut().unwrap().layers[0].opacity = 0.,
                10 => edit.recipe.layers.as_mut().unwrap().layers[0].fill = 0.4,
                11 => edit.input_gains = [1.1, 1., 0.9],
                12 => edit.recipe.exposure_ev = 0.7,
                13 => edit.recipe.temperature = 10.,
                14 => {
                    edit.recipe.curve = vec![
                        CurvePoint { x: 0., y: 0. },
                        CurvePoint { x: 0.4, y: 0.5 },
                        CurvePoint { x: 1., y: 1. },
                    ]
                }
                15 => {
                    a.geometry.vignette = 25.;
                    optics = Arc::new(point::Optics::prepare(
                        device,
                        size,
                        25.,
                        &memory,
                        &gpu,
                        &|| false,
                    )?);
                }
                16 => {
                    a.detail.luminance_noise = 40.;
                    request.amounts[0] = 0.4;
                }
                17 => {
                    a.detail.threshold = 45.;
                    request.threshold = 0.45;
                }
                18 => a.detail.radius = 2.5,
                19 => request.origin = [132, 42],
                20 => {
                    uploaded =
                        admission::Upload::prepare(device, &source, &memory, &gpu, &|| false)?
                            .submit(queue, &|| false)?
                }
                _ => (),
            }
            let hits = cache.hits.load(Ordering::Relaxed);
            let output = pipelines
                .prepare(
                    device,
                    uploaded.clone(),
                    optics.clone(),
                    &edit,
                    request,
                    &memory,
                    &gpu,
                    &cache,
                )?
                .submit(queue, &|| false)?;
            ensure!(
                cache.hits.load(Ordering::Relaxed) - hits == usize::from((1..=10).contains(&case)),
                "Unexpected checkpoint reuse for case {case}"
            );
            let failures_before = result.numerical.failures;
            compare(
                device,
                queue,
                &source,
                &edit,
                request,
                &output,
                format!("checkpoint pattern={pattern} change={case}"),
                &mut result,
            )?;
            if result.numerical.failures > failures_before
                && std::env::var("TR_GPU_COMPARE_TRACE").is_ok_and(|v| v == "1")
            {
                for stage in 0..7 {
                    let mut inspected = edit.clone();
                    let mut requested = request;
                    if stage >= 1 {
                        inspected.recipe.layers.as_mut().unwrap().layers.clear();
                    }
                    let advanced = inspected.recipe.advanced.as_mut().unwrap();
                    if (2..=4).contains(&stage) || stage == 6 {
                        advanced.color = Default::default();
                    }
                    if (2..=5).contains(&stage) {
                        advanced.masks.clear();
                    }
                    if (3..=4).contains(&stage) {
                        advanced.detail.texture = 0.;
                        advanced.detail.clarity = 0.;
                        advanced.detail.sharpen = 0.;
                        advanced.geometry.defringe = 0.;
                    }
                    if stage == 4 {
                        advanced.detail.luminance_noise = 0.;
                        advanced.detail.chroma_noise = 0.;
                        requested.amounts = [0.; 2];
                    }
                    let actual = admission::Pending::prepare_inner(
                        device,
                        &pipelines.noise,
                        uploaded.clone(),
                        requested,
                        &memory,
                        &gpu,
                        Some((&pipelines.point, optics.clone(), &inspected)),
                        Some((&pipelines.spatial, size)),
                        Some((&pipelines.post, size)),
                        (stage == 0).then_some((&pipelines.layers, size)),
                        &|| false,
                    )?
                    .submit(queue, &|| false)?;
                    let mut trace = Check::default();
                    compare(
                        device,
                        queue,
                        &source,
                        &inspected,
                        requested,
                        &actual,
                        format!("uncached change={case} stage={stage}"),
                        &mut trace,
                    )?;
                    eprintln!(
                        "Stage {stage}: failures={} max_error={} display={}",
                        trace.numerical.failures,
                        trace.numerical.maximum_error,
                        trace.numerical.maximum_display_error
                    );
                }
            }
        }
        cache.clear();
        let hits = cache.hits.load(Ordering::Relaxed);
        let pending = pipelines.prepare(
            device,
            uploaded.clone(),
            optics.clone(),
            &edit,
            request,
            &memory,
            &gpu,
            &cache,
        )?;
        drop(pending);
        let pending = pipelines.prepare(
            device,
            uploaded.clone(),
            optics.clone(),
            &edit,
            request,
            &memory,
            &gpu,
            &cache,
        )?;
        ensure!(
            cache.hits.load(Ordering::Relaxed) == hits,
            "Unsubmitted checkpoint was reused"
        );
        ensure!(
            pending.submit(queue, &|| true).is_err(),
            "Cancelled checkpoint submitted"
        );
        result.lifecycle_cases += 1;
        let first = pipelines
            .prepare(
                device,
                uploaded.clone(),
                optics.clone(),
                &edit,
                request,
                &memory,
                &gpu,
                &cache,
            )?
            .submit(queue, &|| false)?;
        let before = edit.clone();
        edit.recipe.advanced.as_mut().unwrap().detail.texture = 10.;
        // No wait or readback between these submissions: the second dispatch
        // must safely read the first dispatch's cached result in queue order.
        let second = pipelines
            .prepare(
                device,
                uploaded.clone(),
                optics.clone(),
                &edit,
                request,
                &memory,
                &gpu,
                &cache,
            )?
            .submit(queue, &|| false)?;
        ensure!(
            cache.hits.load(Ordering::Relaxed) == hits + 1,
            "Queued checkpoint was not reused"
        );
        compare(
            device,
            queue,
            &source,
            &before,
            request,
            &first,
            format!("checkpoint queued producer pattern={pattern}"),
            &mut result,
        )?;
        compare(
            device,
            queue,
            &source,
            &edit,
            request,
            &second,
            format!("checkpoint queued consumer pattern={pattern}"),
            &mut result,
        )?;
        drop((first, second));
        result.lifecycle_cases += 1;
        memory.set_pressure(true);
        let output = pipelines
            .prepare(
                device,
                uploaded.clone(),
                optics.clone(),
                &edit,
                request,
                &memory,
                &gpu,
                &cache,
            )?
            .submit(queue, &|| false)?;
        drop(output);
        memory.set_pressure(false);
        result.lifecycle_cases += 1;
        let output = pipelines
            .prepare(
                device,
                uploaded.clone(),
                optics.clone(),
                &edit,
                request,
                &memory,
                &gpu,
                &cache,
            )?
            .submit(queue, &|| false)?;
        drop(output);
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })?;
        gpu.configure(1);
        ensure!(
            pipelines
                .prepare(
                    device,
                    uploaded.clone(),
                    optics.clone(),
                    &edit,
                    request,
                    &memory,
                    &gpu,
                    &cache
                )
                .is_err(),
            "Reduced GPU quota admitted work"
        );
        gpu.configure(256 * 1024 * 1024);
        result.lifecycle_cases += 1;
        let output = pipelines
            .prepare(
                device,
                uploaded.clone(),
                optics.clone(),
                &edit,
                request,
                &memory,
                &gpu,
                &cache,
            )?
            .submit(queue, &|| false)?;
        drop(output);
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })?;
        let weak = Arc::downgrade(&uploaded);
        drop((uploaded, optics));
        ensure!(weak.upgrade().is_none(), "Checkpoint pins native source");
        ensure!(
            memory.usage().reserved > 0 && gpu.usage().reserved > 0,
            "Retained checkpoint has no credits"
        );
        cache.clear();
        result.lifecycle_cases += 1;
        result.hits += cache.hits.load(Ordering::Relaxed);
        result.misses += cache.misses.load(Ordering::Relaxed);
        result.final_memory_reserved += memory.usage().reserved;
        result.final_gpu_reserved += gpu.usage().reserved;
    }
    ensure!(
        result.final_memory_reserved == 0 && result.final_gpu_reserved == 0,
        "Checkpoint leaked admission"
    );
    Ok(result)
}
