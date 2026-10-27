//! Bounded startup self-check, separate from the exhaustive release campaign.
//! Shader failures reject this optional graph without disabling verified point
//! editing or allowing an uncaptured validation error to terminate the app.
use super::*;
use std::{future::Future, task::Context, task::Poll, task::Waker, time::Duration, time::Instant};

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub error: Option<String>,
    pub checkpoint: checkpoint_check::Check,
    pub presentation: display_check::Check,
    pub elapsed_seconds: f64,
}

impl Check {
    pub(crate) fn passed(&self) -> bool {
        self.error.is_none()
            && self.checkpoint.numerical.cases == 46
            && self.checkpoint.numerical.failures == 0
            && self.checkpoint.numerical.maximum_display_error <= 1
            && self.checkpoint.lifecycle_cases == 10
            && self.checkpoint.final_memory_reserved == 0
            && self.checkpoint.final_gpu_reserved == 0
            && self.presentation.cases == 36
            && self.presentation.lifecycle_cases == 24
            && self.presentation.display_failures == 0
            && self.presentation.maximum_display_error <= 1
            && self.presentation.final_memory_reserved == 0
            && self.presentation.final_gpu_reserved == 0
    }
}

fn error_result(
    device: &wgpu::Device,
    future: impl Future<Output = Option<wgpu::Error>>,
) -> Result<Option<wgpu::Error>> {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    let started = Instant::now();
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Ok(value);
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "GPU error scope timed out"
        );
        device.poll(wgpu::PollType::Poll)?;
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(crate) fn guarded<T>(device: &wgpu::Device, run: impl FnOnce() -> Result<T>) -> Result<T> {
    // wgpu 30 scopes are thread-local. Qualification and its submissions stay
    // on the caller, before entering the UI event loop or during device rebind.
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = run();
    // Pop every scope even if an earlier error is found.
    let errors = [
        error_result(device, validation.pop()),
        error_result(device, internal.pop()),
        error_result(device, oom.pop()),
    ];
    for error in errors {
        if let Some(error) = error? {
            anyhow::bail!("GPU graph qualification rejected: {error}");
        }
    }
    result
}

pub(crate) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    let started = Instant::now();
    guarded(device, || {
        let checkpoint = checkpoint_check::check(device, queue)?;
        let presentation = display_check::runtime_check(device, queue)?;
        Ok(Check {
            error: None,
            checkpoint,
            presentation,
            elapsed_seconds: started.elapsed().as_secs_f64(),
        })
    })
}
