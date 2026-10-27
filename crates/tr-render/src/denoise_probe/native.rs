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
    let backend = match std::env::var("TR_GPU_NATIVE_BACKEND").as_deref() {
        Ok("dx12") => wgpu::Backends::DX12,
        Ok("vulkan") | Err(_) => wgpu::Backends::VULKAN,
        Ok(other) => panic!("Unsupported diagnostic backend: {other}"),
    };
    // Match the release app even though this Rust diagnostic uses a test build.
    // In particular DEBUG makes FXC skip shader optimization, hiding both its
    // compilation cost and possible numerical differences in the shipped app.
    let flags = if std::env::var("TR_GPU_NATIVE_DEBUG").is_ok_and(|v| v == "1") {
        wgpu::InstanceFlags::debugging()
    } else {
        wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL
    };
    println!("instance_flags={flags:?}");
    let mut descriptor = wgpu::InstanceDescriptor {
        backends: backend,
        flags,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    if let Ok(path) = std::env::var("TR_GPU_NATIVE_DXC") {
        assert!(std::path::Path::new(&path).is_absolute());
        assert!(std::path::Path::new(&path).is_file());
        descriptor.backend_options.dx12.shader_compiler =
            wgpu::Dx12Compiler::DynamicDxc { dxc_path: path };
    }
    println!(
        "dx12_compiler={:?}",
        descriptor.backend_options.dx12.shader_compiler
    );
    let compiler_policy = crate::device_config::CompilerPolicy::from_descriptor(&descriptor);
    let instance = wgpu::Instance::new(descriptor);
    let adapter = wait(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .unwrap();
    println!("adapter={:?}", adapter.get_info());
    println!("adapter_limits={:?}", adapter.limits());
    let (device, queue) = wait(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("TR experimental chain native test"),
        required_features: crate::timings::device_features(adapter.features()),
        required_limits: crate::device_config::photographic_limits(
            &adapter.limits(),
            wgpu::Limits::default(),
        ),
        ..Default::default()
    }))
    .unwrap();
    let phase = std::env::var("TR_GPU_NATIVE_PHASE").unwrap_or_else(|_| "layers".into());
    if phase == "upload" {
        upload_check::check(&device, &queue).unwrap();
        return;
    }
    if phase == "device" {
        // A compilation failure must be a rejected optional capability, not an
        // uncaptured panic or a qualification inherited by the next check.
        let rejected = device_check::guarded(&device, || {
            let _ = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("TR intentionally invalid qualification shader"),
                source: wgpu::ShaderSource::Wgsl("invalid shader".into()),
            });
            Ok(())
        });
        assert!(rejected.is_err());
        let result = device_check::check(&device, &queue).unwrap();
        println!(
            "device_check linear_cases={} linear_failures={} display_cases={} display_failures={} max_display={} lifecycle={} final_ram={} final_gpu={} elapsed_seconds={}",
            result.checkpoint.numerical.cases,
            result.checkpoint.numerical.failures,
            result.presentation.cases,
            result.presentation.display_failures,
            result.presentation.maximum_display_error,
            result.presentation.lifecycle_cases,
            result.presentation.final_memory_reserved,
            result.presentation.final_gpu_reserved,
            result.elapsed_seconds
        );
        assert!(result.passed());
        return;
    }
    if phase == "display" {
        let p = display_check::check(&device, &queue).unwrap();
        println!(
            "presentation cases={} samples={} failures={} max_display={} lifecycle={} final_ram={} final_gpu={}",
            p.cases,
            p.samples,
            p.display_failures,
            p.maximum_display_error,
            p.lifecycle_cases,
            p.final_memory_reserved,
            p.final_gpu_reserved
        );
        assert_eq!(p.cases, 384);
        assert_eq!(p.preparation_cases, 16);
        assert_eq!(p.backpressure_cases, 3);
        println!("background_preparation cases={}", p.preparation_cases);
        assert_eq!(p.display_failures, 0);
        assert!(p.maximum_display_error <= 1);
        assert_eq!(p.final_memory_reserved, 0);
        assert_eq!(p.final_gpu_reserved, 0);
        return;
    }
    if phase == "full" {
        let result = crate::editing_compute::check(&device, &queue, compiler_policy).unwrap();
        println!(
            "point_editing cases={} failures={} max_linear={} max_display={} display_failures={} layers_passed={}",
            result.cases,
            result.failures,
            result.max_error,
            result.display_max_error,
            result.display_failures,
            result.layer_tail_passed()
        );
        assert_eq!(result.failures, 0);
        assert_eq!(result.display_failures, 0);
        assert!(result.layer_tail_passed());
        let p = result
            .denoise_probe
            .expect("TR_DENOISE_GPU_PROBE=1 is required");
        assert!(p.error.is_none(), "{:?}", p.error);
        let rows = [
            ("blocks", p.cases, p.samples, p.failures, p.maximum_error, 0),
            (
                "reconstruction",
                p.reconstruction.cases,
                p.reconstruction.samples,
                p.reconstruction.failures,
                p.reconstruction.maximum_error,
                p.reconstruction.maximum_display_error,
            ),
            (
                "multiscale",
                p.multiscale.cases,
                p.multiscale.samples,
                p.multiscale.failures,
                p.multiscale.maximum_error,
                p.multiscale.maximum_display_error,
            ),
            (
                "tiling",
                p.tiling.cases,
                p.tiling.samples,
                p.tiling.failures,
                p.tiling.maximum_error,
                p.tiling.maximum_display_error,
            ),
            (
                "point",
                p.point.cases,
                p.point.samples,
                p.point.failures,
                p.point.maximum_error,
                p.point.maximum_display_error,
            ),
            (
                "spatial",
                p.spatial.cases,
                p.spatial.samples,
                p.spatial.failures,
                p.spatial.maximum_error,
                p.spatial.maximum_display_error,
            ),
            (
                "post",
                p.post.cases,
                p.post.samples,
                p.post.failures,
                p.post.maximum_error,
                p.post.maximum_display_error,
            ),
            (
                "layers",
                p.layers.cases,
                p.layers.samples,
                p.layers.failures,
                p.layers.maximum_error,
                p.layers.maximum_display_error,
            ),
            (
                "checkpoint",
                p.checkpoint.numerical.cases,
                p.checkpoint.numerical.samples,
                p.checkpoint.numerical.failures,
                p.checkpoint.numerical.maximum_error,
                p.checkpoint.numerical.maximum_display_error,
            ),
        ];
        for (name, cases, samples, failures, linear, display) in &rows {
            println!("qualification,{name},{cases},{samples},{failures},{linear},{display}");
        }
        println!(
            "models samples={} failures={} band_mismatches={} max_linear={}",
            p.reconstruction.model_samples,
            p.reconstruction.model_failures,
            p.reconstruction.model_band_mismatches,
            p.reconstruction.model_maximum_error
        );
        println!(
            "admission lifecycle={} pixels={} failures={} final_ram={} final_gpu={}",
            p.admission.lifecycle_cases,
            p.admission.pixel_cases,
            p.admission.failures,
            p.admission.final_memory_reserved,
            p.admission.final_gpu_reserved
        );
        println!(
            "presentation cases={} samples={} failures={} max_display={} lifecycle={}",
            p.presentation.cases,
            p.presentation.samples,
            p.presentation.display_failures,
            p.presentation.maximum_display_error,
            p.presentation.lifecycle_cases
        );
        assert_eq!(p.presentation.cases, 384);
        assert_eq!(p.presentation.display_failures, 0);
        assert!(p.presentation.maximum_display_error <= 1);
        assert_eq!(p.presentation.final_memory_reserved, 0);
        assert_eq!(p.presentation.final_gpu_reserved, 0);
        assert!(
            rows.iter()
                .all(|(_, cases, samples, failures, _, display)| *cases > 0
                    && *samples > 0
                    && *failures == 0
                    && *display <= 1)
        );
        assert_eq!(p.reconstruction.model_failures, 0);
        assert_eq!(p.reconstruction.model_band_mismatches, 0);
        assert_eq!(p.admission.failures, 0);
        assert_eq!(p.admission.lifecycle_cases, 16);
        assert_eq!(p.admission.final_memory_reserved, 0);
        assert_eq!(p.admission.final_gpu_reserved, 0);
        assert_eq!(p.checkpoint.final_memory_reserved, 0);
        assert_eq!(p.checkpoint.final_gpu_reserved, 0);
        return;
    }
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
    assert_eq!(result.cases, if phase == "post" { 170 } else { 246 });
    assert_eq!(result.chain_cases, 6);
    assert_eq!(result.failures, 0);
    assert!(result.maximum_display_error <= 1 && result.failed_cases.is_empty());
}
