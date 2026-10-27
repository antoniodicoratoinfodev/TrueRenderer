//! Device capabilities are separate from the application's memory quotas.
use eframe::wgpu;

/// Records the explicit compiler used to create this instance. `Auto` cannot
/// establish DXC capability because wgpu may silently fall back to FXC.
#[derive(Clone, Copy, Debug, Default)]
pub struct CompilerPolicy {
    explicit_dxc: bool,
}
impl CompilerPolicy {
    pub fn from_descriptor(descriptor: &wgpu::InstanceDescriptor) -> Self {
        Self {
            explicit_dxc: matches!(
                descriptor.backend_options.dx12.shader_compiler,
                wgpu::Dx12Compiler::DynamicDxc { .. } | wgpu::Dx12Compiler::StaticDxc
            ),
        }
    }
    pub(crate) fn permits_graph(self, backend: wgpu::Backend) -> bool {
        backend != wgpu::Backend::Dx12 || self.explicit_dxc
    }
}

/// Configure before instance/device creation; carry the returned policy with
/// that instance, including a recreated device. No process-global readiness bit.
pub fn configure_instance(descriptor: &mut wgpu::InstanceDescriptor) -> CompilerPolicy {
    #[cfg(windows)]
    {
        let packaged = std::env::current_exe().ok().and_then(|exe| {
            let directory = exe.parent()?.join("runtime/dxc");
            let compiler = directory.join("dxcompiler.dll");
            (compiler.is_file() && directory.join("dxil.dll").is_file()).then_some(compiler)
        });
        descriptor.backend_options.dx12.shader_compiler = packaged_compiler(
            &descriptor.backend_options.dx12.shader_compiler,
            packaged.as_deref(),
        );
    }
    CompilerPolicy::from_descriptor(descriptor)
}

#[cfg(any(windows, test))]
fn packaged_compiler(
    requested: &wgpu::Dx12Compiler,
    path: Option<&std::path::Path>,
) -> wgpu::Dx12Compiler {
    if matches!(
        requested,
        wgpu::Dx12Compiler::Fxc | wgpu::Dx12Compiler::StaticDxc
    ) {
        return requested.clone();
    }
    path.filter(|p| p.is_absolute())
        .map_or(wgpu::Dx12Compiler::Fxc, |p| {
            wgpu::Dx12Compiler::DynamicDxc {
                dxc_path: p.to_string_lossy().into_owned(),
            }
        })
}

pub fn photographic_limits(available: &wgpu::Limits, mut requested: wgpu::Limits) -> wgpu::Limits {
    // 45 MP sources can exceed 8192 pixels on one axis. Their exact half-size
    // denoiser guide can exceed a 128 MiB binding. Request only real capability;
    // the existing admission budget still decides whether a job can run.
    requested.max_texture_dimension_2d = available.max_texture_dimension_2d.min(16384);
    requested.max_buffer_size = available.max_buffer_size.min(256 * 1024 * 1024);
    requested.max_storage_buffer_binding_size = available
        .max_storage_buffer_binding_size
        .min(requested.max_buffer_size);
    requested
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_package_and_automatic_compiler_cannot_qualify_dx12() {
        use super::*;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        for compiler in [wgpu::Dx12Compiler::Auto, wgpu::Dx12Compiler::Fxc] {
            descriptor.backend_options.dx12.shader_compiler = compiler.clone();
            let policy = CompilerPolicy::from_descriptor(&descriptor);
            assert!(!policy.permits_graph(wgpu::Backend::Dx12));
            assert!(policy.permits_graph(wgpu::Backend::Vulkan));
            assert!(matches!(
                packaged_compiler(&compiler, None),
                wgpu::Dx12Compiler::Fxc
            ));
        }
        let path = std::env::current_exe()
            .unwrap()
            .with_file_name("dxcompiler.dll");
        descriptor.backend_options.dx12.shader_compiler =
            packaged_compiler(&wgpu::Dx12Compiler::Auto, Some(&path));
        assert!(CompilerPolicy::from_descriptor(&descriptor).permits_graph(wgpu::Backend::Dx12));
        assert!(matches!(
            packaged_compiler(&wgpu::Dx12Compiler::Fxc, Some(&path)),
            wgpu::Dx12Compiler::Fxc
        ));
        assert!(matches!(
            packaged_compiler(
                &wgpu::Dx12Compiler::Auto,
                Some(std::path::Path::new("relative.dll"))
            ),
            wgpu::Dx12Compiler::Fxc
        ));
    }
    #[test]
    fn photographic_limits_never_exceed_the_adapter_or_request_unbounded_buffers() {
        use super::*;
        for (edge, bytes) in [(4096, 64 * 1024 * 1024), (16384, 1024 * 1024 * 1024)] {
            let limits = wgpu::Limits {
                max_texture_dimension_2d: edge,
                max_storage_buffer_binding_size: bytes,
                max_buffer_size: bytes,
                ..Default::default()
            };
            let requested = photographic_limits(&limits, wgpu::Limits::default());
            assert!(requested.max_texture_dimension_2d <= edge);
            assert!(requested.max_storage_buffer_binding_size <= bytes.min(256 * 1024 * 1024));
            assert!(requested.max_buffer_size <= bytes.min(256 * 1024 * 1024));
        }
    }
}
