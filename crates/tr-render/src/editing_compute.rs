//! Resident per-pixel photographic draft pipeline. No readback in presentation.
use crate::live_edit::LiveEdit;
use eframe::wgpu::{self, util::DeviceExt};

pub(crate) struct EditingCompute {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}
impl EditingCompute {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries: Vec<_> = (0..3)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding == 1,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("TR live editing"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TR photographic editing"),
            source: wgpu::ShaderSource::Wgsl(include_str!("editing_compute.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("TR photographic editing"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("edit"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { layout, pipeline }
    }
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        output: &wgpu::Buffer,
        edit: &LiveEdit,
        count: u32,
    ) {
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TR edit parameters"),
            contents: &parameters(edit),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let buffers = [&params, source, output];
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
    }
}
fn parameters(edit: &LiveEdit) -> Vec<u8> {
    let r = &edit.recipe;
    let mut p = [[0_f32; 4]; 52];
    let (t, tint) = (r.temperature / 100., r.tint / 100.);
    p[0] = [
        r.exposure_ev.exp2(),
        (0.18 * t + 0.07 * tint).exp(),
        (-0.14 * tint).exp(),
        (-0.18 * t + 0.07 * tint).exp(),
    ];
    p[1] = [
        r.brightness / 200.,
        r.shadows / 200.,
        r.highlights / 200.,
        r.blacks / 200.,
    ];
    p[2] = [
        r.whites / 200.,
        (r.contrast / 100.).exp(),
        1. + r.saturation / 100.,
        r.vibrance / 100.,
    ];
    let tone = r.brightness != 0.
        || r.shadows != 0.
        || r.highlights != 0.
        || r.blacks != 0.
        || r.whites != 0.
        || r.contrast != 0.
        || !r.curve.is_empty();
    let exposure_only =
        !tone && r.temperature == 0. && r.tint == 0. && r.saturation == 0. && r.vibrance == 0.;
    p[3] = [
        r.curve.len() as f32,
        u32::from(r.protect_warm) as f32,
        u32::from(tone) as f32,
        u32::from(exposure_only) as f32,
    ];
    p[5][..3].copy_from_slice(&edit.input_gains);
    for (out, point) in p[8..40].iter_mut().zip(&r.curve) {
        *out = [point.x, point.y, 0., 0.];
    }
    if let Some(a) = &r.advanced {
        p[4] = [
            u32::from(a.color != Default::default()) as f32,
            u32::from(a.color.monochrome) as f32,
            0.,
            0.,
        ];
        for (out, band) in p[40..48].iter_mut().zip(a.color.bands) {
            *out = [band.hue, band.saturation, band.luminance, 0.];
        }
        for (out, grade) in p[48..51].iter_mut().zip(a.color.grading) {
            *out = [grade.hue, grade.amount, 0., 0.];
        }
        p[51][..3].copy_from_slice(&a.color.rgb_midtones);
    }
    p.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

#[derive(Default, serde::Serialize)]
pub struct EditingCheck {
    pub cases: usize,
    pub samples: usize,
    pub failures: usize,
    pub max_error: f32,
    pub worst_ratio: f32,
    pub display_failures: usize,
    pub display_max_error: u8,
}
pub(crate) fn check(device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<EditingCheck> {
    use std::time::Duration;
    use tr_core::{
        color::LinearImage,
        editing::{Advanced, CurvePoint, EditRecipe},
        resample::Region,
    };
    let mut result = EditingCheck::default();
    let pipeline = EditingCompute::new(device);
    let source = LinearImage::new(
        65,
        33,
        (0..65 * 33)
            .map(|i| {
                let a = [0., 0.01, 0.5, 1.][i % 4];
                let rgb = match i % 9 {
                    0 => [0.2, 0.2, 0.2],
                    1 => [1., 0., 0.],
                    2 => [0., 1., 0.],
                    3 => [0., 0., 1.],
                    4 => [-0.25, 0.2, 2.],
                    5 => [1000., -1000., 0.5],
                    _ => [
                        (i % 67) as f32 / 67.,
                        (i % 31) as f32 / 31.,
                        (i % 17) as f32 / 17.,
                    ],
                };
                [rgb[0] * a, rgb[1] * a, rgb[2] * a, a]
            })
            .collect(),
    )?;
    let bytes: Vec<_> = source
        .pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("TR edit qualification"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let levels = tr_core::provider::ImageLevels::from_source(
        source.clone(),
        tr_core::preview::PreviewRequest::full(),
    )?;
    let memory = tr_core::budget::MemoryBudget::new(64 * 1024 * 1024);
    let gpu = tr_core::budget::MemoryBudget::new(64 * 1024 * 1024);
    let mut renderer = crate::resident_compute::GpuFilter::new(device.clone());
    for case in 0..12 {
        let mut recipe = EditRecipe::neutral(Default::default());
        recipe.process_version = 3;
        match case {
            1 => recipe.exposure_ev = 10.,
            2 => recipe.exposure_ev = -10.,
            3 | 4 => {
                let v = if case == 3 { 100. } else { -100. };
                recipe.brightness = v;
                recipe.shadows = v;
                recipe.highlights = v;
                recipe.whites = v;
                recipe.blacks = v;
                recipe.contrast = v;
            }
            5 => {
                recipe.temperature = 100.;
                recipe.tint = -100.;
                recipe.saturation = 100.;
            }
            6 => {
                recipe.temperature = -100.;
                recipe.tint = 100.;
                recipe.saturation = -100.;
            }
            7 | 8 => {
                recipe.vibrance = if case == 7 { 100. } else { -100. };
                recipe.protect_warm = true;
            }
            9 => {
                recipe.contrast = 23.;
                recipe.curve = (0..32)
                    .map(|i| {
                        let x = i as f32 / 31.;
                        CurvePoint { x, y: x * x }
                    })
                    .collect();
            }
            10 | 11 => {
                let mut a = Advanced::default();
                for (i, b) in a.color.bands.iter_mut().enumerate() {
                    b.hue = i as f32 * 20. - 60.;
                    b.saturation = 30.;
                    b.luminance = -40.;
                }
                for (i, g) in a.color.grading.iter_mut().enumerate() {
                    g.hue = i as f32 * 120. + 20.;
                    g.amount = 35.;
                }
                a.color.rgb_midtones = [30., -60., 100.];
                a.color.monochrome = case == 11;
                recipe.advanced = Some(Box::new(a));
                recipe.exposure_ev = 0.35;
                recipe.brightness = 24.;
            }
            _ => {}
        }
        let edit = LiveEdit {
            recipe: recipe.clone(),
            input_gains: if case == 0 { [1.2, 0.8, 1.1] } else { [1.; 3] },
            max_edge: 1024,
            revision: case,
        };
        let mut expected = source.clone();
        for p in &mut expected.pixels {
            for (c, g) in p[..3].iter_mut().zip(edit.input_gains) {
                *c *= g;
            }
        }
        recipe.apply(&mut expected)?;
        let mut encoder = device.create_command_encoder(&Default::default());
        pipeline.encode(
            device,
            &mut encoder,
            &input,
            &output,
            &edit,
            source.pixels.len() as u32,
        );
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes.len() as u64);
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
        for (actual, expected) in mapped
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| f32::from_le_bytes(*v))
            .zip(expected.pixels.iter().flatten())
        {
            let error = (actual - expected).abs();
            let tolerance = 1e-5 + 1e-4 * expected.abs();
            result.samples += 1;
            result.max_error = result.max_error.max(error);
            result.worst_ratio = result.worst_ratio.max(error / tolerance);
            if !actual.is_finite() || error > tolerance {
                result.failures += 1;
            }
        }
        drop(mapped);
        readback.unmap();
        for region in [
            Region::fitted([65, 33], [47, 23]),
            Region {
                size: [31, 17],
                origin: [8.25, 4.5],
                step: [1., 1.],
            },
        ] {
            let mut working = memory.try_reserve(8 * 1024 * 1024).unwrap();
            let mut frame =
                renderer.render_edit(&levels, region, &memory, &gpu, &mut working, Some(&edit))?;
            frame.submit(queue);
            let actual = crate::preview_compute::read_display(device, queue, &frame)?;
            let expected = edit.render(&levels, region)?.to_display();
            for (a, b) in actual.iter().zip(expected) {
                let error = a.abs_diff(b);
                result.display_max_error = result.display_max_error.max(error);
                if error > 1 {
                    result.display_failures += 1;
                }
            }
        }
        result.cases += 1;
    }
    drop(renderer);
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    anyhow::ensure!(
        memory.usage().reserved == 0 && gpu.usage().reserved == 0,
        "Editing GPU: crediti trattenuti"
    );
    Ok(result)
}
