//! Explicit native shader checks without opening the application. This shortens
//! numerical iteration; final release and native viewport checks remain separate.
use super::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
struct WakeThread(std::thread::Thread);
impl Wake for WakeThread {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(WakeThread(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let started = Instant::now();
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "Native device request timed out"
        );
        std::thread::park_timeout(Duration::from_secs(1));
    }
}
#[test]
#[ignore = "Explicit native GPU diagnostic; needs a real adapter"]
fn experimental_chain() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = wait(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .unwrap();
    println!("adapter={:?}", adapter.get_info());
    let (device, queue) = wait(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("TR experimental chain native test"),
        ..Default::default()
    }))
    .unwrap();
    let phase = std::env::var("TR_GPU_NATIVE_PHASE").unwrap_or_else(|_| "layers".into());
    if phase == "chain_benchmark" {
        let result = chain_benchmark::check(&device, &queue).unwrap();
        println!(
            "chain cases={} failures={} max_linear={} max_display={} hits={} misses={}",
            result.correctness_cases,
            result.failures,
            result.maximum_error,
            result.maximum_display_error,
            result.checkpoint_hits,
            result.checkpoint_misses
        );
        for row in &result.measurements {
            println!(
                "sample,{},{},{},{},{},{}",
                row.control,
                row.checkpoint,
                row.schedule,
                row.cpu_us.unwrap_or(0.),
                row.gpu_encode_us,
                row.gpu_ready_us
            );
        }
        assert_eq!(result.correctness_cases, 16);
        assert_eq!(result.failures, 0);
        assert!(result.maximum_display_error <= 1);
        assert_eq!(result.final_memory_reserved, 0);
        assert_eq!(result.final_gpu_reserved, 0);
        return;
    }
    if phase == "checkpoint" {
        let result = checkpoint_check::check(&device, &queue).unwrap();
        println!(
            "checkpoint cases={} lifecycle={} hits={} misses={} failures={} max_linear={} max_display={} failed_cases={:?}",
            result.numerical.cases,
            result.lifecycle_cases,
            result.hits,
            result.misses,
            result.numerical.failures,
            result.numerical.maximum_error,
            result.numerical.maximum_display_error,
            result.numerical.failed_cases
        );
        assert_eq!(result.numerical.cases, 46);
        assert_eq!(result.lifecycle_cases, 10);
        assert_eq!(result.hits, 22);
        assert_eq!(result.numerical.failures, 0);
        assert!(
            result.numerical.maximum_display_error <= 1 && result.numerical.failed_cases.is_empty()
        );
        return;
    }
    let result = if phase == "post" {
        post_check::check(&device, &queue)
    } else {
        layers_check::check(&device, &queue)
    }
    .unwrap();
    println!(
        "phase={} cases={} chains={} samples={} failures={} max_linear={} max_display={} failed_cases={:?}",
        phase,
        result.cases,
        result.chain_cases,
        result.samples,
        result.failures,
        result.maximum_error,
        result.maximum_display_error,
        result.failed_cases
    );
    assert_eq!(result.cases, if phase == "post" { 170 } else { 126 });
    assert_eq!(result.chain_cases, 6);
    assert_eq!(result.failures, 0);
    assert!(result.maximum_display_error <= 1 && result.failed_cases.is_empty());
}
