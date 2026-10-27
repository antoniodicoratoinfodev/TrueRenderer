//! End-to-end GPU graph → geometry → canonical resampling → SDR comparisons.
//! CPU references develop the whole source, independently of the regional plan.
use super::*;
use std::time::Duration;
use tr_core::{budget::MemoryBudget, provider::ImageLevels, resample::Region};

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub cases: usize,
    pub samples: usize,
    pub display_failures: usize,
    pub maximum_display_error: u8,
    pub lifecycle_cases: usize,
    pub preparation_cases: usize,
    pub backpressure_cases: usize,
    pub final_memory_reserved: u64,
    pub final_gpu_reserved: u64,
}
pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    run(device, queue, false)
}

/// Device self-check: all three spectral sizes, alpha/near-gray inputs,
/// geometry/CA, masked layers and cached edits. The full campaign above keeps
/// its broader geometry/filter/lifetime matrix and separate counters.
pub(super) fn runtime_check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    run(device, queue, true)
}

fn run(device: &wgpu::Device, queue: &wgpu::Queue, brief: bool) -> Result<Check> {
    let renderer = crate::resident_compute::GpuFilter::new(device.clone());
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let gpu = MemoryBudget::new(512 * 1024 * 1024);
    let mut result = Check::default();
    for pattern in [4, 8] {
        for &base in if brief { &[0, 1, 2][..] } else { &[0, 2] } {
            let source = reconstruction::source([265, 199], pattern)?;
            let opaque = source.pixels.iter().all(|p| p[3] == 1.);
            let image = ImageLevels::from_reference_mip(
                source,
                [264 * (1 << base) + 1, 198 * (1 << base) + 1],
                base,
                opaque,
            )?;
            for geometry in if brief { 2..3 } else { 0..4 } {
                for &with_layers in if brief { &[true][..] } else { &[false, true] } {
                    let mut recipe = post_check::recipe(8);
                    recipe.exposure_ev = 0.7;
                    recipe.temperature = 10.;
                    let a = recipe.advanced.as_mut().unwrap();
                    a.detail = spatial_check::recipe(7).advanced.unwrap().detail;
                    a.detail.luminance_noise = 25.;
                    a.detail.chroma_noise = 35.;
                    a.geometry.vignette = 15.;
                    a.geometry.defringe = 20.;
                    if geometry > 0 {
                        a.geometry.crop = [0.07, 0.03, 0.93, 0.95];
                        a.geometry.quarter_turns = 1;
                        a.geometry.angle = 3.;
                    }
                    if geometry > 1 {
                        a.geometry.ca = [0.7, -0.4];
                    }
                    if geometry > 2 {
                        a.geometry.angle = -17.;
                    }
                    if with_layers {
                        layers_check::add_layers(&mut recipe, if pattern == 8 { 15 } else { 5 });
                    }
                    let mut edit = crate::live_edit::LiveEdit {
                        recipe,
                        input_gains: [1.1, 1., 0.9],
                        max_edge: 0,
                        revision: 1,
                    };
                    for &step in if brief {
                        &[0.75, 2.01][..]
                    } else {
                        &[0.75, 1.91, 2.01, 4.1]
                    } {
                        let region = Region {
                            size: [67, 43],
                            origin: [-1.25, 17.3].map(|v| v * f64::from(1 << base)),
                            step: [step * f64::from(1 << base); 2],
                        };
                        for change in 0..3 {
                            edit.recipe.advanced.as_mut().unwrap().detail.texture =
                                [-30., 0., 60.][change];
                            if let Some(stack) = &mut edit.recipe.layers {
                                stack.layers[0].opacity = [0.5, 0., 1.][change];
                            }
                            let mut working = memory.try_reserve(32 * 1024 * 1024).unwrap();
                            if geometry == 0 && !with_layers && step == 0.75 && change == 0 {
                                use crate::resident_compute::GraphPreparation;
                                let prepare = |working: &mut _| {
                                    renderer.prepare_chain(
                                        &image,
                                        &edit,
                                        region,
                                        &memory,
                                        &gpu,
                                        working,
                                        &|| false,
                                    )
                                };
                                let pending = prepare(&mut working)?;
                                ensure!(
                                    matches!(pending, GraphPreparation::Upload(_)),
                                    "Expected cold upload"
                                );
                                drop(pending);
                                renderer.clear();
                                ensure!(
                                    gpu.usage().reserved == 0,
                                    "Unsubmitted preparation retained credits"
                                );
                                result.preparation_cases += 1;
                                // Admit exactly the cold source transfer. GPU
                                // display metadata must wait until its staging
                                // is gone, even when the native viewport is large.
                                let upload = admission::Upload::prepare_point(
                                    device,
                                    image.source(),
                                    &memory,
                                    &gpu,
                                    &|| false,
                                )?;
                                let upload_limit = gpu.usage().reserved;
                                drop(upload);
                                ensure!(gpu.usage().reserved == 0, "Upload retained credits");
                                let mut stage = 0;
                                let mut batches = 0;
                                while stage < 3 {
                                    let completed = stage == 2;
                                    gpu.configure(if stage == 0 {
                                        upload_limit
                                    } else {
                                        512 * 1024 * 1024
                                    });
                                    let pending = prepare(&mut working)?;
                                    let uploaded_source = match &pending {
                                        GraphPreparation::Upload(upload) => Some(upload.source()),
                                        _ => None,
                                    };
                                    let uploading = uploaded_source.is_some();
                                    ensure!(
                                        uploading == (stage == 0),
                                        "Unexpected source preparation stage"
                                    );
                                    ensure!(
                                        matches!(pending, GraphPreparation::Optics(_))
                                            == (stage == 1),
                                        "Unexpected optical preparation stage"
                                    );
                                    ensure!(
                                        matches!(pending, GraphPreparation::Frame(_)) == completed,
                                        "Unexpected preparation stage"
                                    );
                                    let (tx, rx) = std::sync::mpsc::channel();
                                    let reserved = gpu.usage().reserved;
                                    pending.submit(queue, move |ready| {
                                        tx.send(ready).unwrap();
                                    });
                                    device.poll(wgpu::PollType::Wait {
                                        submission_index: None,
                                        timeout: Some(Duration::from_secs(30)),
                                    })?;
                                    ensure!(
                                        rx.recv_timeout(Duration::from_secs(1))? == completed,
                                        "Preparation completion mismatch"
                                    );
                                    if stage == 1 {
                                        ensure!(
                                            reserved - gpu.usage().reserved
                                                == image.source().pixels.len() as u64 * 4,
                                            "Optical staging was not released before graph admission"
                                        );
                                    }
                                    if uploading {
                                        batches += 1;
                                        ensure!(
                                            batches < 100,
                                            "Source preparation made no progress"
                                        );
                                        // Staging may need several bounded transfers under
                                        // this deliberately narrow quota. The display phase
                                        // starts only after the final source callback.
                                        let resident = uploaded_source
                                            .unwrap()
                                            .submitted
                                            .load(std::sync::atomic::Ordering::Acquire);
                                        if resident {
                                            stage += 1;
                                            result.preparation_cases += 1;
                                        }
                                    } else {
                                        stage += 1;
                                        result.preparation_cases += 1;
                                    }
                                }
                            }
                            let mut frame = renderer.render_chain(
                                &image,
                                &edit,
                                region,
                                &memory,
                                &gpu,
                                &mut working,
                                &|| false,
                            )?;
                            // Clearing after encoding must not release the data read by
                            // an immutable pending frame or change its output.
                            if change == 2 {
                                renderer.clear();
                                result.lifecycle_cases += 1;
                            }
                            frame.submit(queue);
                            let actual =
                                crate::preview_compute::read_display(device, queue, &frame)?;
                            let mut expected = image.source().clone();
                            for p in &mut expected.pixels {
                                for (v, gain) in p[..3].iter_mut().zip(edit.input_gains) {
                                    *v *= gain;
                                }
                            }
                            let native = edit.recipe.apply_preview(
                                &mut expected,
                                image.source_size(),
                                base,
                            )?;
                            let opaque = expected.pixels.iter().all(|p| p[3] == 1.);
                            let expected =
                                ImageLevels::from_reference_mip(expected, native, base, opaque)?
                                    .render(region)?
                                    .to_display();
                            ensure!(
                                actual.len() == expected.len(),
                                "Graph display size mismatch"
                            );
                            let before = result.display_failures;
                            for (a, b) in actual.iter().zip(expected) {
                                let error = a.abs_diff(b);
                                result.samples += 1;
                                result.maximum_display_error =
                                    result.maximum_display_error.max(error);
                                result.display_failures += usize::from(error > 1);
                            }
                            if before != result.display_failures {
                                eprintln!(
                                    "Graph display difference pattern={pattern} base={base} geometry={geometry} layers={with_layers} step={step} change={change}: {}",
                                    result.display_failures - before
                                );
                            }
                            result.cases += 1;
                        }
                    }
                }
            }
            let mut recipe = post_check::recipe(8);
            recipe.advanced.as_mut().unwrap().detail =
                spatial_check::recipe(7).advanced.unwrap().detail;
            let edit = crate::live_edit::LiveEdit {
                recipe,
                input_gains: [1.; 3],
                max_edge: 0,
                revision: 1,
            };
            let region = Region {
                size: [67, 43],
                origin: [0.; 2],
                step: [f64::from(1 << base); 2],
            };
            let mut working = memory.try_reserve(32 * 1024 * 1024).unwrap();
            let pending = renderer.render_chain(
                &image,
                &edit,
                region,
                &memory,
                &gpu,
                &mut working,
                &|| false,
            )?;
            drop(pending);
            renderer.clear();
            ensure!(
                renderer
                    .render_chain(&image, &edit, region, &memory, &gpu, &mut working, &|| true)
                    .is_err(),
                "Cancelled display admitted"
            );
            drop(working);
            device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })?;
            ensure!(
                memory.usage().reserved == 0 && gpu.usage().reserved == 0,
                "Graph display leaked credits"
            );
            result.lifecycle_cases += 2;
        }
    }
    if !brief {
        result.backpressure_cases = backpressure(device, queue)?;
    }
    result.final_memory_reserved = memory.usage().reserved;
    result.final_gpu_reserved = gpu.usage().reserved;
    Ok(result)
}

/// One pending frame deliberately holds most of the quota. The next encoder
/// must sleep, resume after release, or obey cancellation without waiting for
/// a UI submission that will never happen during shutdown.
fn backpressure(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<usize> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let renderer = crate::resident_compute::GpuFilter::new(device.clone());
    let image = ImageLevels::from_reference_mip(
        reconstruction::source([265, 199], 8)?,
        [265, 199],
        0,
        true,
    )?;
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let gpu = MemoryBudget::new(512 * 1024 * 1024);
    let region = Region {
        size: [67, 43],
        origin: [37., 41.],
        step: [1.; 2],
    };
    let mut recipe = post_check::recipe(8);
    recipe.advanced.as_mut().unwrap().detail = spatial_check::recipe(7).advanced.unwrap().detail;
    recipe.advanced.as_mut().unwrap().detail.chroma_noise = 35.;
    let mut edit = crate::live_edit::LiveEdit {
        recipe,
        input_gains: [1.; 3],
        max_edge: 0,
        revision: 1,
    };
    for (cancel_wait, submit_held) in [(false, false), (true, false), (false, true)] {
        let mut working = memory.try_reserve(32 * 1024 * 1024).unwrap();
        // Establish the resident source and optics without a live scratch lease.
        let mut warm =
            renderer.render_chain(&image, &edit, region, &memory, &gpu, &mut working, &|| {
                false
            })?;
        warm.submit(queue);
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        drop(warm);
        edit.recipe.exposure_ev = 0.3;
        let mut held =
            renderer.render_chain(&image, &edit, region, &memory, &gpu, &mut working, &|| {
                false
            })?;
        gpu.configure(gpu.usage().reserved + 1024 * 1024);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        edit.recipe.exposure_ev = 0.6;
        std::thread::scope(|scope| -> Result<()> {
            scope.spawn(|| {
                let mut working = memory.try_reserve(32 * 1024 * 1024).unwrap();
                let result = renderer.render_chain(
                    &image,
                    &edit,
                    region,
                    &memory,
                    &gpu,
                    &mut working,
                    &|| cancel.load(Ordering::Acquire),
                );
                tx.send((result, working)).ok();
            });
            ensure!(
                matches!(
                    rx.recv_timeout(Duration::from_millis(60)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ),
                "Second graph bypassed quota backpressure"
            );
            if cancel_wait {
                cancel.store(true, Ordering::Release);
            } else if submit_held {
                held.submit(queue);
                drop(held);
            } else {
                drop(held);
            }
            let (result, working) = match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(value) => value,
                Err(error) => {
                    cancel.store(true, Ordering::Release);
                    return Err(error.into());
                }
            };
            if cancel_wait {
                ensure!(result.is_err(), "Waiting graph ignored cancellation");
            } else {
                let mut frame = result?;
                frame.submit(queue);
                device.poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })?;
            }
            drop(working);
            Ok(())
        })?;
        // In the cancellation case `held` is dropped at scope completion.
        renderer.clear();
        drop(working);
        gpu.configure(512 * 1024 * 1024);
        ensure!(
            memory.usage().reserved == 0 && gpu.usage().reserved == 0,
            "Backpressure retained credits"
        );
    }
    Ok(3)
}
