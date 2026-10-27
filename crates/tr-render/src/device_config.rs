//! Device capabilities are separate from the application's memory quotas.
use eframe::wgpu;

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
