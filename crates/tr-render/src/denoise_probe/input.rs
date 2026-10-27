//! Full photographic sources may exceed one storage-buffer binding. A sampled
//! RGBA32F texture uses integer loads, without filtering or quantization.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Input<'a> {
    Buffer(&'a wgpu::Buffer),
    Texture(&'a wgpu::TextureView),
    Point {
        source: &'a wgpu::TextureView,
        parameters: &'a wgpu::Buffer,
        optics: &'a wgpu::Buffer,
    },
}
impl<'a> Input<'a> {
    pub fn kind(self) -> usize {
        match self {
            Self::Buffer(_) => 0,
            Self::Texture(_) => 1,
            Self::Point { .. } => 2,
        }
    }
    pub fn entry(self) -> wgpu::BindGroupEntry<'a> {
        wgpu::BindGroupEntry {
            binding: 1,
            resource: match self {
                Self::Buffer(b) => b.as_entire_binding(),
                Self::Texture(v) => wgpu::BindingResource::TextureView(v),
                Self::Point { source, .. } => wgpu::BindingResource::TextureView(source),
            },
        }
    }
    pub fn extend(self, entries: &mut Vec<wgpu::BindGroupEntry<'a>>) {
        entries.push(self.entry());
        if let Self::Point {
            parameters, optics, ..
        } = self
        {
            for (binding, buffer) in [(5, parameters), (6, optics)] {
                entries.push(wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                });
            }
        }
    }
}

pub(super) fn extend_layout(kind: usize, entries: &mut Vec<wgpu::BindGroupLayoutEntry>) {
    if kind == 2 {
        for binding in [5, 6] {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 5 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: true }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
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
    let load =
        format!("textureLoad({field}, vec2<i32>(i32(index % width), i32(index / width)), 0)");
    let value = if kind == 2 {
        format!(
            "point_input(spectral_source_pixel({field}, index, width, point_params.data[6].z != 0.), index)"
        )
    } else {
        load
    };
    let mut result = text.replacen(&declaration, &format!("@group(0) @binding(1) var {field}: texture_2d<f32>;"), 1)
        .replacen(&getter, &format!("fn {field}_pixel(index: u32) -> vec4<f32> {{ let width = u32({width}); return {value}; }}"), 1);
    if kind == 2 {
        result.push_str(&point::input_shader());
    }
    result
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
    upload_point(device, encoder, image, false, cancelled)
}

/// Opaque input needs only its unchanged RGB words. Pack the linear stream into
/// RGBA texels; the shader restores alpha=1 without resampling or arithmetic.
pub(super) fn texture_height(size: [u32; 2], packed: bool) -> u32 {
    if packed {
        (u64::from(size[1]) * 3).div_ceil(4) as u32
    } else {
        size[1]
    }
}

fn copy_rgb_words(pixels: &[[f32; 4]], first: usize, destination: &mut [u8]) {
    for (output, word) in destination
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(first..pixels.len() * 3)
    {
        output.copy_from_slice(&pixels[word / 3][word % 3].to_le_bytes());
    }
}

pub(super) const SOURCE_SHADER: &str = r#"
fn spectral_source_pixel(source: texture_2d<f32>, index: u32, width: u32, packed: bool) -> vec4<f32> {
    if !packed {
        return textureLoad(source, vec2<i32>(i32(index % width), i32(index / width)), 0);
    }
    let word = index * 3u;
    let texel = word / 4u;
    let a = textureLoad(source, vec2<i32>(i32(texel % width), i32(texel / width)), 0);
    switch word % 4u {
        case 0u: { return vec4(a.xyz, 1.); }
        case 1u: { return vec4(a.yzw, 1.); }
        default: {}
    }
    let next = texel + 1u;
    let b = textureLoad(source, vec2<i32>(i32(next % width), i32(next / width)), 0);
    if word % 4u == 2u { return vec4(a.zw, b.x, 1.); }
    return vec4(a.w, b.xy, 1.);
}
"#;

pub(super) fn upload_point(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    image: &LinearImage,
    packed: bool,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<wgpu::Texture> {
    ensure!(!cancelled(), "Editing annullato");
    let texture = point_texture(device, image, packed)?;
    upload_point_rows(
        device,
        encoder,
        image,
        packed,
        &texture,
        0,
        texture_height([image.width, image.height], packed),
        cancelled,
    )?;
    Ok(texture)
}

pub(super) fn point_texture(
    device: &wgpu::Device,
    image: &LinearImage,
    packed: bool,
) -> Result<wgpu::Texture> {
    let texture_height = texture_height([image.width, image.height], packed);
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
    Ok(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("TR experimental native spectral source"),
        size: wgpu::Extent3d {
            width: image.width,
            height: texture_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn upload_point_rows(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    image: &LinearImage,
    packed: bool,
    texture: &wgpu::Texture,
    first_row: u32,
    end_row: u32,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<()> {
    ensure!(
        first_row < end_row && end_row <= texture_height([image.width, image.height], packed),
        "Invalid source upload rows"
    );
    let pitch = (image.width * 16).div_ceil(256) * 256;
    // Bounded staging allocations also support sources larger than max_buffer_size.
    let rows = (device.limits().max_buffer_size / u64::from(pitch)).min(128) as u32;
    ensure!(rows > 0, "Native spectral upload row exceeds device limit");
    for first in (first_row..end_row).step_by(rows as usize) {
        ensure!(!cancelled(), "Editing annullato");
        let height = rows.min(end_row - first);
        let mut bytes = vec![0_u8; pitch as usize * height as usize];
        for y in 0..height {
            let offset = y as usize * pitch as usize;
            let start = (first + y) as usize * image.width as usize;
            if packed {
                copy_rgb_words(
                    &image.pixels,
                    start * 4,
                    &mut bytes[offset..offset + image.width as usize * 16],
                );
            } else {
                let source =
                    bytemuck::cast_slice(&image.pixels[start..start + image.width as usize]);
                bytes[offset..offset + source.len()].copy_from_slice(source);
            }
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
                texture,
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
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn packed_rgb_preserves_bits_across_rows_and_partial_texels() {
        for width in [1, 2, 3, 7, 133] {
            for height in [1, 2, 3, 4, 5, 131] {
                let pixels: Vec<_> = (0..width * height)
                    .map(|i| [f32::from_bits(0x3e800001 + i), -(i as f32), -0., 1.])
                    .collect();
                let rows = super::texture_height([width, height], true);
                let mut packed = vec![0_u8; width as usize * rows as usize * 16];
                for (y, row) in packed.chunks_exact_mut(width as usize * 16).enumerate() {
                    super::copy_rgb_words(&pixels, y * width as usize * 4, row);
                }
                for (i, pixel) in pixels.iter().enumerate() {
                    for (c, value) in pixel[..3].iter().enumerate() {
                        let offset = (3 * i + c) * 4;
                        assert_eq!(&packed[offset..offset + 4], &value.to_le_bytes());
                    }
                }
                assert!(packed[pixels.len() * 12..].iter().all(|v| *v == 0));
                assert_eq!(super::texture_height([width, height], false), height);
            }
        }
    }
}
