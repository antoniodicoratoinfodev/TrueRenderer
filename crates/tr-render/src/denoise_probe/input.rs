//! Full photographic sources may exceed one storage-buffer binding. A sampled
//! RGBA32F texture uses integer loads, without filtering or quantization.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Input<'a> {
    Buffer(&'a wgpu::Buffer),
    Texture(&'a wgpu::TextureView),
}
impl<'a> Input<'a> {
    pub fn kind(self) -> usize {
        match self {
            Self::Buffer(_) => 0,
            Self::Texture(_) => 1,
        }
    }
    pub fn entry(self) -> wgpu::BindGroupEntry<'a> {
        wgpu::BindGroupEntry {
            binding: 1,
            resource: match self {
                Self::Buffer(b) => b.as_entire_binding(),
                Self::Texture(v) => wgpu::BindingResource::TextureView(v),
            },
        }
    }
}
pub(super) fn binding_type(kind: usize) -> wgpu::BindingType {
    if kind == 0 {
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        }
    } else {
        wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        }
    }
}
pub(super) fn shader(text: &str, field: &str, width: &str, kind: usize) -> String {
    if kind == 0 {
        return text.to_owned();
    }
    let declaration =
        format!("@group(0) @binding(1) var<storage, read> {field}: array<vec4<f32>>;");
    let getter = format!("fn {field}_pixel(index: u32) -> vec4<f32> {{ return {field}[index]; }}");
    assert_eq!(text.matches(&declaration).count(), 1);
    assert_eq!(text.matches(&getter).count(), 1);
    text.replacen(&declaration, &format!("@group(0) @binding(1) var {field}: texture_2d<f32>;"), 1)
        .replacen(&getter, &format!("fn {field}_pixel(index: u32) -> vec4<f32> {{ let width = u32({width}); return textureLoad({field}, vec2<i32>(i32(index % width), i32(index / width)), 0); }}"), 1)
}

pub(super) fn upload(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    image: &LinearImage,
) -> Result<wgpu::Texture> {
    upload_cancellable(device, encoder, image, &|| false)
}

pub(super) fn upload_cancellable(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    image: &LinearImage,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<wgpu::Texture> {
    ensure!(!cancelled(), "Editing annullato");
    let format = wgpu::TextureFormat::Rgba32Float;
    let usage = wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING;
    ensure!(
        format
            .guaranteed_format_features(device.features())
            .allowed_usages
            .contains(usage)
            && image.width <= device.limits().max_texture_dimension_2d
            && image.height <= device.limits().max_texture_dimension_2d,
        "Native spectral texture unsupported"
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("TR experimental native spectral source"),
        size: wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let pitch = (image.width * 16).div_ceil(256) * 256;
    // Bounded staging allocations also support sources larger than max_buffer_size.
    let rows = (device.limits().max_buffer_size / u64::from(pitch)).min(128) as u32;
    ensure!(rows > 0, "Native spectral upload row exceeds device limit");
    for first in (0..image.height).step_by(rows as usize) {
        ensure!(!cancelled(), "Editing annullato");
        let height = rows.min(image.height - first);
        let mut bytes = vec![0_u8; pitch as usize * height as usize];
        for y in 0..height {
            let start = (first + y) as usize * image.width as usize;
            let source = bytemuck::cast_slice(&image.pixels[start..start + image.width as usize]);
            let offset = y as usize * pitch as usize;
            bytes[offset..offset + source.len()].copy_from_slice(source);
        }
        let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: None,
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: first,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: image.width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(texture)
}
