//! Native quota/cancellation/lifetime checks on the admitted job API.
use super::{admission::*, *};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tr_core::budget::MemoryBudget;

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub lifecycle_cases: usize,
    pub pixel_cases: usize,
    pub failures: usize,
    pub maximum_display_error: u8,
    pub maximum_error: f32,
    pub admitted_memory_peak: u64,
    pub admitted_gpu_peak: u64,
    pub final_memory_reserved: u64,
    pub final_gpu_reserved: u64,
}

pub(super) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let memory = MemoryBudget::new(512 * 1024 * 1024);
    let gpu = MemoryBudget::new(512 * 1024 * 1024);
    let source = reconstruction::source([517, 269], 8)?;
    let request = Request {
        n: 16,
        amounts: [0.25, 0.35],
        threshold: 0.25,
        levels: 3,
        origin: [119, 35],
        size: [257, 193],
    };
    let pipeline = multiscale::Multiscale::new(device);
    let mut result = Check::default();
    let unchanged = |bytes: u64| -> Result<()> {
        ensure!(
            memory.usage().reserved == bytes && gpu.usage().reserved == bytes,
            "GPU job leaked or prematurely released admission credits"
        );
        Ok(())
    };
    let poor = MemoryBudget::new(1);
    ensure!(
        Upload::prepare(device, &source, &poor, &gpu, &|| false).is_err(),
        "RAM quota accepted"
    );
    unchanged(0)?;
    result.lifecycle_cases += 1;
    ensure!(
        Upload::prepare(device, &source, &memory, &poor, &|| false).is_err(),
        "GPU quota accepted"
    );
    unchanged(0)?;
    result.lifecycle_cases += 1;
    ensure!(
        Upload::prepare(device, &source, &memory, &gpu, &|| true).is_err(),
        "Cancelled upload accepted"
    );
    unchanged(0)?;
    result.lifecycle_cases += 1;
    let visits = AtomicUsize::new(0);
    ensure!(
        Upload::prepare(device, &source, &memory, &gpu, &|| visits
            .fetch_add(1, Ordering::Relaxed)
            >= 3)
        .is_err(),
        "Mid-upload cancellation ignored"
    );
    unchanged(0)?;
    result.lifecycle_cases += 1;
    let upload = Upload::prepare(device, &source, &memory, &gpu, &|| false)?;
    ensure!(
        upload.submit(queue, &|| true).is_err(),
        "Cancelled upload submitted"
    );
    unchanged(0)?;
    result.lifecycle_cases += 1;
    let uploaded =
        Upload::prepare(device, &source, &memory, &gpu, &|| false)?.submit(queue, &|| false)?;
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    let resident_bytes = memory.usage().reserved;
    ensure!(resident_bytes > 0, "Resident source lost credits");
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    ensure!(
        Pending::prepare(
            device,
            &pipeline,
            uploaded.clone(),
            request,
            &poor,
            &gpu,
            &|| false
        )
        .is_err(),
        "Job RAM quota accepted"
    );
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    ensure!(
        Pending::prepare(
            device,
            &pipeline,
            uploaded.clone(),
            request,
            &memory,
            &poor,
            &|| false
        )
        .is_err(),
        "Job GPU quota accepted"
    );
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    ensure!(
        Pending::prepare(
            device,
            &pipeline,
            uploaded.clone(),
            request,
            &memory,
            &gpu,
            &|| true
        )
        .is_err(),
        "Cancelled job encoded"
    );
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    let visits = AtomicUsize::new(0);
    ensure!(
        Pending::prepare(
            device,
            &pipeline,
            uploaded.clone(),
            request,
            &memory,
            &gpu,
            &|| visits.fetch_add(1, Ordering::Relaxed) >= 8
        )
        .is_err(),
        "Mid-job cancellation ignored"
    );
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    let pending = Pending::prepare(
        device,
        &pipeline,
        uploaded.clone(),
        request,
        &memory,
        &gpu,
        &|| false,
    )?;
    drop(pending);
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    let pending = Pending::prepare(
        device,
        &pipeline,
        uploaded.clone(),
        request,
        &memory,
        &gpu,
        &|| false,
    )?;
    ensure!(
        pending.submit(queue, &|| true).is_err(),
        "Cancelled job submitted"
    );
    unchanged(resident_bytes)?;
    result.lifecycle_cases += 1;
    let weak = Arc::downgrade(&uploaded);
    let pending = Pending::prepare(
        device,
        &pipeline,
        uploaded.clone(),
        request,
        &memory,
        &gpu,
        &|| false,
    )?;
    drop(uploaded);
    ensure!(
        weak.upgrade().is_some(),
        "Pending job did not retain its source"
    );
    result.lifecycle_cases += 1;
    let output = pending.submit(queue, &|| false)?;
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    ensure!(
        weak.upgrade().is_none(),
        "Completed job did not release its source"
    );
    let output_bytes = u64::from(request.size[0]) * u64::from(request.size[1]) * 16 + 65536;
    unchanged(output_bytes)?;
    result.lifecycle_cases += 1;
    let expected = tr_core::editing::detail::multiscale_region_reference(
        &source,
        1.,
        request.origin,
        request.size,
        request.amounts,
        request.threshold,
    )?;
    let mut pixels = benchmark::Check::default();
    benchmark::compare(device, queue, &output.buffer, &expected, &mut pixels)?;
    result.pixel_cases = pixels.correctness_cases;
    result.failures = pixels.failures;
    result.maximum_display_error = pixels.maximum_display_error;
    result.maximum_error = pixels.maximum_error;
    let alias = output.clone();
    drop(output);
    unchanged(output_bytes)?;
    result.lifecycle_cases += 1;
    drop(alias);
    unchanged(0)?;
    result.lifecycle_cases += 1;
    result.admitted_memory_peak = memory.usage().peak;
    result.admitted_gpu_peak = gpu.usage().peak;
    result.final_memory_reserved = memory.usage().reserved;
    result.final_gpu_reserved = gpu.usage().reserved;
    Ok(result)
}
