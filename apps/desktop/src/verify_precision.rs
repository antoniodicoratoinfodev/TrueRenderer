//! Native numerical readback, not a photograph of the physical display.
use anyhow::{Result, ensure};
use eframe::{egui, wgpu};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tr_core::{
    color::{self, LinearImage},
    preview::PreviewRequest,
    provider::ImageLevels,
    resample::Region,
};

fn pattern() -> LinearImage {
    LinearImage::new(
        1024,
        64,
        (0..64)
            .flat_map(|y| {
                (0..1024).map(move |x| {
                    let v = x as f32 / 1023.;
                    if y < 32 {
                        color::from_encoded_srgb([v, v, v, 1.])
                    } else {
                        color::from_encoded_srgb([v, 1. - v, 0.3, 0.5])
                    }
                })
            })
            .collect(),
    )
    .unwrap()
}
fn native_value(format: wgpu::TextureFormat, bytes: &[u8], channel: usize) -> (f32, u32) {
    match format {
        wgpu::TextureFormat::Rgb10a2Unorm => {
            let v = (u32::from_le_bytes(bytes.try_into().unwrap()) >> (channel * 10)) & 1023;
            (v as f32 / 1023., v)
        }
        wgpu::TextureFormat::Rgba16Float => {
            let bits = u16::from_le_bytes([bytes[channel * 2], bytes[channel * 2 + 1]]);
            let exp = (bits >> 10) & 31;
            let m = (bits & 1023) as f32;
            let v = if exp == 0 {
                m * 2f32.powi(-24)
            } else {
                (1. + m / 1024.) * 2f32.powi(exp as i32 - 15)
            };
            (if bits & 0x8000 != 0 { -v } else { v }, bits as u32)
        }
        _ => {
            let c = if format == wgpu::TextureFormat::Bgra8Unorm {
                2 - channel
            } else {
                channel
            };
            (bytes[c] as f32 / 255., bytes[c] as u32)
        }
    }
}
struct Probe {
    presenter: tr_render::presenter::Presenter,
    image: Arc<ImageLevels>,
    results: Arc<Mutex<Vec<serde_json::Value>>>,
    diagnostics: String,
    requested: String,
    started: Instant,
    phase: u8,
    requested_capture: bool,
    rect: egui::Rect,
}
impl eframe::App for Probe {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.presenter.poll(&ctx);
        ui.label(format!(
            "TrueRenderer · SDR precision probe · {} · {}",
            self.requested,
            if self.phase == 0 { "CPU" } else { "GPU" }
        ));
        let ppp = ctx.pixels_per_point();
        let origin = ui.cursor().min;
        let origin = egui::pos2((origin.x * ppp).ceil() / ppp, (origin.y * ppp).ceil() / ppp);
        self.rect = egui::Rect::from_min_size(origin, egui::vec2(1024. / ppp, 64. / ppp));
        self.presenter.paint(
            ui,
            "precision".into(),
            &self.image,
            self.rect,
            Region {
                size: [1024, 64],
                origin: [0., 0.],
                step: [1., 1.],
            },
        );
        let stats = self.presenter.statistics();
        if !self.requested_capture
            && (if self.phase == 0 {
                stats.cpu_frames > 0
            } else {
                stats.gpu_frames > 0
            })
            && self.started.elapsed() > Duration::from_millis(300)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.requested_capture = true;
        }
        if let Some(capture) = eframe::egui_wgpu::capture::take_native_capture() {
            let bpp = if capture.format == wgpu::TextureFormat::Rgba16Float {
                8
            } else {
                4
            };
            let mut levels = std::collections::BTreeSet::new();
            let mut max_error = 0f32;
            let source = pattern();
            for y in [16usize, 48] {
                for x in 0..1024usize {
                    let at = (((self.rect.min.y * ppp).round() as usize + y)
                        * capture.size[0] as usize
                        + (self.rect.min.x * ppp).round() as usize
                        + x)
                        * bpp;
                    let expected = color::display_float(source.pixels[y * 1024 + x], 119. / 255.);
                    for (channel, expected) in expected[..3].iter().enumerate() {
                        let (value, code) =
                            native_value(capture.format, &capture.bytes[at..at + bpp], channel);
                        max_error = max_error.max((value - expected).abs());
                        if channel == 1 && y == 16 {
                            levels.insert(code);
                        }
                    }
                }
            }
            let high = matches!(
                capture.format,
                wgpu::TextureFormat::Rgb10a2Unorm | wgpu::TextureFormat::Rgba16Float
            );
            let passed = levels.len() >= if high { 800 } else { 250 }
                && max_error <= if high { 0.0011 } else { 0.004 };
            self.results.lock().unwrap().push(serde_json::json!({"requested":self.requested,"compute":if self.phase==0 {"CPU"} else {"GPU"},"format":format!("{:?}",capture.format),"levels":levels.len(),"max_srgb_error":max_error,"passed":passed,"diagnostics":self.diagnostics,"size":capture.size,"native_readback_bytes":capture.bytes.len(),"compute_statistics":stats}));
            if self.phase == 0 {
                self.phase = 1;
                self.presenter.clear();
                self.presenter.configure_compute(true, 2);
                self.started = Instant::now();
                self.requested_capture = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1160., 280.)));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.started.elapsed() > Duration::from_secs(20) {
            self.results.lock().unwrap().push(serde_json::json!({"requested":self.requested,"passed":false,"timeout":true,"statistics":stats}));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(30));
    }
}
pub fn run(root: &Path) -> Result<()> {
    let results = Arc::new(Mutex::new(Vec::new()));
    eframe::egui_wgpu::capture::enable_native_capture(true);
    for requested in [
        None,
        Some(wgpu::TextureFormat::Rgb10a2Unorm),
        Some(wgpu::TextureFormat::Rgba16Float),
    ] {
        let mut options = eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            dithering: false,
            run_and_return: true,
            viewport: egui::ViewportBuilder::default().with_inner_size([1100., 240.]),
            ..Default::default()
        };
        options.wgpu_options.surface.preferred_format = requested;
        let results = results.clone();
        eframe::run_native(
            "TrueRenderer precision probe",
            options,
            Box::new(move |cc| {
                let mut presenter = tr_render::presenter::Presenter::new(
                    cc.egui_ctx.clone(),
                    tr_core::budget::MemoryBudget::new(512 * 1024 * 1024),
                    cc.wgpu_render_state.clone(),
                );
                presenter.configure_compute(true, 1);
                Ok(Box::new(Probe {
                    presenter,
                    image: Arc::new(ImageLevels::from_source(pattern(), PreviewRequest::full())?),
                    results,
                    diagnostics: cc
                        .wgpu_render_state
                        .as_ref()
                        .unwrap()
                        .surface_diagnostics
                        .clone(),
                    requested: format!("{requested:?}"),
                    started: Instant::now(),
                    phase: 0,
                    requested_capture: false,
                    rect: egui::Rect::NOTHING,
                }))
            }),
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    eframe::egui_wgpu::capture::enable_native_capture(false);
    let checks = results.lock().unwrap();
    let passed = checks.len() == 6 && checks.iter().all(|c| c["passed"] == true);
    std::fs::write(
        root.join("reports/presentation-precision-macos.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":passed,"checks":*checks,"scope":"Synthetic full-range grayscale and translucent-color ramps through production CPU/GPU presenter and egui native-format capture, SDR sRGB, resize between phases. No physical display/link-depth, HDR, multi-monitor, Windows or statistical performance qualification."}),
        )?,
    )?;
    ensure!(
        passed,
        "Precisione di presentazione: controllare il rapporto"
    );
    Ok(())
}
