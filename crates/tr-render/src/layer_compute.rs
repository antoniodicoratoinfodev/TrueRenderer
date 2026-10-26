//! Qualified GPU layer arithmetic. Source, masks and intermediates are admitted
//! separately; this kernel never submits or reads back UI frames.
use anyhow::{Result, ensure};
use eframe::wgpu::{self, util::DeviceExt};
use tr_core::editing::layers::{LayerStack, Operator, TonalLevel};

pub(crate) struct LayerCompute {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    geometry_pipeline: wgpu::ComputePipeline,
}
impl LayerCompute {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries: Vec<_> = (0..4)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: binding != 3,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("TR photographic layer tail"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TR layer arithmetic"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layer_compute.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("TR layer arithmetic"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("layers"),
            compilation_options: Default::default(),
            cache: None,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TR canonical layer geometry"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layer_geometry.wgsl").into()),
        });
        let geometry_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("TR canonical layer geometry"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("geometry"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            layout,
            pipeline,
            geometry_pipeline,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        output: &wgpu::Buffer,
        mask: &wgpu::Buffer,
        stack: &LayerStack,
        count: u32,
    ) -> Result<()> {
        let words = parameters(stack)?;
        let bytes: Vec<_> = words
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TR layer parameters"),
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let buffers = [&params, source, mask, output];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(count.div_ceil(256), 1, 1);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn geometry(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        output: &wgpu::Buffer,
        map: &wgpu::Buffer,
        params: &wgpu::Buffer,
        count: u32,
    ) {
        let buffers = [params, source, map, output];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.geometry_pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(count.div_ceil(256), 1, 1);
    }
}

fn hue_chroma(rgb: [f32; 3]) -> (f32, f32) {
    let hi = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let lo = rgb.into_iter().fold(f32::INFINITY, f32::min);
    let c = hi - lo;
    if c <= 1e-8 {
        return (0., 0.);
    }
    let h = if hi == rgb[0] {
        ((rgb[1] - rgb[2]) / c).rem_euclid(6.)
    } else if hi == rgb[1] {
        (rgb[2] - rgb[0]) / c + 2.
    } else {
        (rgb[0] - rgb[1]) / c + 4.
    };
    (h * 60., c)
}

pub(crate) fn parameters(stack: &LayerStack) -> Result<Vec<[f32; 4]>> {
    stack.validate()?;
    // Keep enabled slots across opacity/fill zero and neutral operator values.
    // The first slot's immutable guide/mask then stays reusable during a drag.
    let layers: Vec<_> = stack
        .layers
        .iter()
        .filter(|l| l.enabled && !l.operators.is_empty())
        .collect();
    let mut words = vec![[
        layers.len() as f32,
        u32::from(!stack.is_neutral()) as f32,
        0.,
        0.,
    ]];
    for (i, layer) in layers.into_iter().enumerate() {
        ensure!(
            i == 0 || layer.mask.root.is_none(),
            "Maschera dipendente da un livello precedente"
        );
        let operators: Vec<_> = layer.operators.iter().filter(|o| !o.is_neutral()).collect();
        words.push([
            operators.len() as f32,
            layer.opacity * layer.fill,
            u32::from(layer.mask.root.is_some()) as f32,
            0.,
        ]);
        for operator in operators {
            match operator {
                Operator::SampleColor(s) => {
                    let (h, c) = hue_chroma(s.range.reference);
                    let y = 0.2627 * s.range.reference[0]
                        + 0.678 * s.range.reference[1]
                        + 0.0593 * s.range.reference[2];
                    words.extend([
                        [1., 5., 0., 0.],
                        [h, c, y, (c * 0.01).max(1e-6)],
                        [
                            s.range.width[0],
                            s.range.width[1],
                            s.range.width[2],
                            s.range.softness,
                        ],
                        [
                            s.correction[0],
                            s.correction[1],
                            s.correction[2],
                            (s.correction[2] / 100.).exp2(),
                        ],
                        [
                            s.uniformity[0],
                            s.uniformity[1],
                            s.uniformity[2],
                            1. - s.range.softness,
                        ],
                    ]);
                }
                Operator::Light {
                    exposure,
                    temperature,
                    tint,
                    saturation,
                } => {
                    let (t, m) = (temperature / 100., tint / 100.);
                    words.extend([
                        [2., 3., 0., 0.],
                        [
                            (0.18 * t + 0.07 * m).exp(),
                            (-0.14 * m).exp(),
                            (-0.18 * t + 0.07 * m).exp(),
                            exposure.exp2(),
                        ],
                        [1. + saturation / 100., 0., 0., 0.],
                    ]);
                }
                Operator::Levels(_) | Operator::TonalLevels(_) => {
                    let levels = level_channels(operator);
                    words.push([3., 9., 0., 0.]);
                    for level in levels {
                        words.push([
                            level.black,
                            level.white - level.black,
                            level.output[0],
                            1. / level.gamma,
                        ]);
                        words.push([
                            level.output[1] - level.output[0],
                            u32::from(*level == TonalLevel::default()) as f32,
                            0.,
                            0.,
                        ]);
                    }
                }
                _ => anyhow::bail!("Operatore del livello richiede CPU"),
            }
        }
    }
    Ok(words)
}
fn level_channels(operator: &Operator) -> &[TonalLevel; 4] {
    match operator {
        Operator::Levels(levels) => levels,
        Operator::TonalLevels(levels) => &levels.channels,
        _ => unreachable!("Only tonal level operators"),
    }
}

#[derive(Default, serde::Serialize)]
pub struct Check {
    pub error: Option<String>,
    pub cases: usize,
    pub composed_cases: usize,
    pub mapped_linear_cases: usize,
    pub reuse_cases: usize,
    pub resident_cases: usize,
    pub resident_miss_cases: usize,
    pub invalidation_cases: usize,
    pub lifetime_cases: usize,
    pub samples: usize,
    pub failures: usize,
    pub maximum_error: f32,
    pub worst_ratio: f32,
    pub display_failures: usize,
    pub display_maximum_error: u8,
}
impl Check {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.cases > 0
            && self.composed_cases > 0
            && self.mapped_linear_cases > 0
            && self.resident_cases > 0
            && self.resident_miss_cases > 0
            && self.lifetime_cases > 0
            && self.failures == 0
            && self.display_failures == 0
    }
}

/// Conservative interval bound. Reject unsupported/explosive combinations
/// before allocating or dispatching; the canonical CPU path handles them.
pub(crate) fn output_bound(stack: &LayerStack, mut bound: f64) -> Result<f64> {
    ensure!(
        bound.is_finite() && bound <= 10.,
        "Ingresso livelli fuori dominio GPU"
    );
    let level = |bound: f64, l: &TonalLevel| -> Result<f64> {
        if *l == TonalLevel::default() {
            return Ok(bound);
        }
        let span = f64::from(l.white - l.black);
        let x = (bound + f64::from(l.black.abs())) / span;
        let power = x.powf(f64::from(1. / l.gamma));
        ensure!(
            power.is_finite() && power <= 1e30,
            "Potenza livelli fuori dominio GPU"
        );
        Ok(f64::from(l.output[0].abs()) + power * f64::from(l.output[1] - l.output[0]))
    };
    for layer in stack.layers.iter().filter(|l| l.active()) {
        let before = bound;
        for op in layer.operators.iter().filter(|o| !o.is_neutral()) {
            bound = match op {
                Operator::SampleColor(s) => {
                    let reference = s
                        .range
                        .reference
                        .iter()
                        .map(|v| f64::from(v.abs()))
                        .fold(0_f64, f64::max);
                    ensure!(reference <= 10., "Campione fuori dominio GPU");
                    let y = bound.max(reference);
                    let c =
                        (2. * bound).max(2. * reference) * (1. + f64::from(s.correction[1]) / 100.);
                    bound.max((y + c) * f64::from((s.correction[2] / 100.).exp2()))
                }
                Operator::Light {
                    exposure,
                    temperature,
                    tint,
                    saturation,
                } => {
                    let (t, m) = (temperature / 100., tint / 100.);
                    let gain = [
                        (0.18 * t + 0.07 * m).exp(),
                        (-0.14 * m).exp(),
                        (-0.18 * t + 0.07 * m).exp(),
                    ]
                    .into_iter()
                    .fold(0_f32, f32::max);
                    let s = f64::from(1. + saturation / 100.);
                    bound
                        * f64::from(exposure.exp2())
                        * f64::from(gain)
                        * ((1. - s).abs() + s.abs())
                }
                Operator::Levels(_) | Operator::TonalLevels(_) => {
                    let levels = level_channels(op);
                    let all = level(bound, &levels[0])?;
                    levels[1..]
                        .iter()
                        .map(|l| level(all, l))
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .fold(0_f64, f64::max)
                }
                _ => anyhow::bail!("Operatore livelli CPU"),
            } * 1.00001;
            ensure!(
                bound.is_finite() && bound <= 1000.,
                "Risultato livelli fuori dominio GPU"
            );
        }
        bound = before.max(bound) * 1.00001;
        ensure!(bound <= 1000., "Composizione livelli fuori dominio GPU");
    }
    Ok(bound)
}

pub(crate) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Check> {
    use std::time::Duration;
    use tr_core::{
        color::{LinearImage, display_pixel},
        editing::layers::{ColorRange, Combine, Layer, MaskKind, SampleColor},
    };
    let pipeline = LayerCompute::new(device);
    let mut result = Check::default();
    let (width, height) = (65, 33);
    let source = LinearImage::new(
        width,
        height,
        (0..width * height)
            .map(|i| {
                let alpha = [0., 0.01, 0.5, 1.][i as usize % 4];
                let rgb = match i % 11 {
                    0 => [0.; 3],
                    1 => [1e-10; 3],
                    2 => [1., 0., 0.],
                    3 => [0., 1., 0.],
                    4 => [0., 0., 1.],
                    5 => [0.3; 3],
                    6 => [-0.25, 0.2, 2.],
                    7 => [10., -10., 0.5],
                    _ => [
                        (i % 113) as f32 / 73.,
                        (i % 71) as f32 / 61.,
                        (i % 47) as f32 / 43.,
                    ],
                };
                [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
            })
            .collect(),
    )?;
    let source_bytes: Vec<_> = source
        .pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("TR layer qualification"),
        contents: &source_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: source_bytes.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: source_bytes.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for case in 0..24 {
        let mut stack = LayerStack {
            base_process: 3,
            layers: Vec::new(),
        };
        let range = ColorRange {
            reference: if case % 3 == 0 {
                [0.3; 3]
            } else {
                [0.1, 0.2, 0.6]
            },
            softness: [0.01, 0.5, 1.][case % 3],
            ..Default::default()
        };
        let sample = Operator::SampleColor(SampleColor {
            range,
            correction: [-72., 35., -21.],
            uniformity: [43., 12., 26.],
        });
        let light = Operator::Light {
            exposure: -1.3,
            temperature: 71.,
            tint: -36.,
            saturation: 27.,
        };
        let channels = [
            TonalLevel {
                black: 0.03,
                white: 0.9,
                gamma: 1.12,
                output: [0.07, 0.95],
            },
            TonalLevel::default(),
            TonalLevel {
                gamma: 0.7,
                ..Default::default()
            },
            TonalLevel {
                white: 1.2,
                ..Default::default()
            },
        ];
        let levels = if case % 2 == 0 {
            Operator::Levels(channels)
        } else {
            Operator::TonalLevels(Box::new(tr_core::editing::layers::LevelAdjustments {
                channels,
                ..Default::default()
            }))
        };
        let mut layer = Layer::new(
            "first",
            [sample.clone(), light.clone(), levels.clone()][case % 3].clone(),
        );
        if case >= 9 {
            layer.operators = vec![sample, light, levels.clone()];
        }
        layer.opacity = [0., 0.3, 0.7, 1.][case % 4];
        layer.fill = [0.25, 1.][case % 2];
        if case >= 6 {
            layer
                .mask
                .append(MaskKind::Color(ColorRange::default()), Combine::Add);
        }
        stack.layers.push(layer);
        if case >= 12 {
            stack.layers.push(Layer::new("finish", levels));
        }
        let active = stack
            .layers
            .iter()
            .find(|l| l.enabled && !l.operators.is_empty());
        let weights: Vec<_> = source
            .pixels
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let guide = if p[3] > 0. {
                    [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
                } else {
                    [0.; 3]
                };
                active.map_or(1., |layer| {
                    layer.mask.weight(
                        [
                            ((i % width as usize) as f32 + 0.5) / width as f32,
                            ((i / width as usize) as f32 + 0.5) / height as f32,
                        ],
                        guide,
                        height as f32 / width as f32,
                    )
                })
            })
            .collect();
        let bytes: Vec<_> = weights.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mask = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        pipeline.encode(
            device,
            &mut encoder,
            &input,
            &output,
            &mask,
            &stack,
            source.pixels.len() as u32,
        )?;
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, source_bytes.len() as u64);
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(10)),
        })?;
        rx.recv_timeout(Duration::from_secs(10))??;
        let mapped = readback.slice(..).get_mapped_range()?;
        for (i, (actual, original)) in mapped
            .as_chunks::<16>()
            .0
            .iter()
            .zip(&source.pixels)
            .enumerate()
        {
            let actual = std::array::from_fn::<_, 4, _>(|c| {
                f32::from_le_bytes(actual[c * 4..c * 4 + 4].try_into().unwrap())
            });
            let mut expected = *original;
            if original[3] > 0. {
                let rgb = stack.apply(
                    [
                        original[0] / original[3],
                        original[1] / original[3],
                        original[2] / original[3],
                    ],
                    [
                        ((i % width as usize) as f32 + 0.5) / width as f32,
                        ((i / width as usize) as f32 + 0.5) / height as f32,
                    ],
                    height as f32 / width as f32,
                );
                expected[..3].copy_from_slice(&rgb.map(|v| v * original[3]));
            }
            for (a, e) in actual.into_iter().zip(expected) {
                let error = (a - e).abs();
                let tolerance = 1e-5 + 1e-4 * e.abs();
                result.samples += 1;
                result.maximum_error = result.maximum_error.max(error);
                result.worst_ratio = result.worst_ratio.max(error / tolerance);
                result.failures += usize::from(!a.is_finite() || error > tolerance);
            }
            for (a, e) in display_pixel(actual, 119. / 255.)
                .into_iter()
                .zip(display_pixel(expected, 119. / 255.))
            {
                let error = a.abs_diff(e);
                result.display_maximum_error = result.display_maximum_error.max(error);
                result.display_failures += usize::from(error > 1);
            }
        }
        drop(mapped);
        readback.unmap();
        result.cases += 1;
    }
    crate::resident_compute::tail::check(device, queue, &mut result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_shaders_parse_and_validate_before_native_qualification() {
        for source in [
            include_str!("layer_compute.wgsl"),
            include_str!("layer_geometry.wgsl"),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(source).unwrap();
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }

    #[test]
    fn admitted_layer_bounds_cover_signed_scalar_results_and_reject_overflow() {
        use tr_core::editing::layers::{Layer, SampleColor};
        for input_bound in [0.01, 0.3, 1., 10.] {
            let mut stack = LayerStack {
                base_process: 3,
                layers: vec![],
            };
            for operator in [
                Operator::SampleColor(SampleColor {
                    correction: [-81., 35., 27.],
                    uniformity: [53., 76., 24.],
                    ..Default::default()
                }),
                Operator::Light {
                    exposure: -0.5,
                    temperature: 81.,
                    tint: -33.,
                    saturation: 55.,
                },
                Operator::Levels(
                    [TonalLevel {
                        gamma: 1.12,
                        ..Default::default()
                    }; 4],
                ),
            ] {
                stack.layers.push(Layer::new("bounded", operator));
                let bound = output_bound(&stack, input_bound).unwrap();
                for i in 0..2000 {
                    let rgb = std::array::from_fn(|c| {
                        ((i * [17, 31, 47][c] % 1999) as f32 / 999. - 1.) * input_bound as f32
                    });
                    let actual = stack.apply(rgb, [0.3, 0.7], 0.75);
                    assert!(
                        actual
                            .iter()
                            .all(|v| v.is_finite() && f64::from(v.abs()) <= bound)
                    );
                }
            }
        }
        let stack = LayerStack {
            base_process: 3,
            layers: vec![Layer::new(
                "extreme",
                Operator::Levels(
                    [TonalLevel {
                        black: -16.,
                        white: -15.999,
                        gamma: 0.1,
                        output: [-16., 16.],
                    }; 4],
                ),
            )],
        };
        assert!(output_bound(&stack, 10.).is_err());
        assert!(output_bound(&stack, f64::NAN).is_err());
    }
}
