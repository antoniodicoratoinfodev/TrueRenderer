//! A lost presentation device requires a new eframe window/device, not CPU compute.
use crate::ui::{Startup, TrueRenderer};
use eframe::{App, egui};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct Host {
    app: Option<TrueRenderer>,
    retained: Rc<RefCell<Option<TrueRenderer>>>,
    lost: Arc<AtomicBool>,
    device: Option<eframe::wgpu::Device>,
    test: bool,
    fail_twice: bool,
    attempt: usize,
    started: Instant,
    root: PathBuf,
    expected_rating: Rc<Cell<i8>>,
    inject_loss: Arc<AtomicBool>,
    expected_selection: Rc<RefCell<Option<serde_json::Value>>>,
}
impl App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.lost.load(Ordering::Acquire) || self.inject_loss.load(Ordering::Acquire) {
            ui.ctx()
                .graphics_mut(|graphics| *graphics = Default::default());
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let app = self.app.as_mut().unwrap();
        app.ui(ui, frame);
        if self.test {
            if app.graphics_test_ready() {
                if self.attempt == 0 || self.fail_twice {
                    // Leave an accepted save in flight across the device reset.
                    self.expected_rating.set(app.graphics_test_save());
                    *self.expected_selection.borrow_mut() =
                        Some(app.graphics_test_state()["selected"].clone());
                    self.inject_loss.store(true, Ordering::Release);
                    if let Some(device) = &self.device {
                        device.destroy();
                        let _ = device.poll(eframe::wgpu::PollType::Poll);
                    }
                    ui.ctx()
                        .graphics_mut(|graphics| *graphics = Default::default());
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    let state = app.graphics_test_state();
                    let passed = state["rating"] == self.expected_rating.get()
                        && state["undo_available"] == true
                        && state["pending"] == 0
                        && state["gpu_verified"] == true
                        && self.expected_selection.borrow().as_ref() == Some(&state["selected"]);
                    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),
                        "passed":passed,"recreations":self.attempt,"state":state,
                        "scope":"Native device.destroy injection, recreation of entire eframe window/device, compute requalification, retained CPU artifacts/service and accepted rating plus undo. Not physical driver reset, other adapters/displays or OOM qualification."});
                    let _ = std::fs::write(
                        self.root.join("reports/preview-device-recovery-macos.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if self.started.elapsed() > Duration::from_secs(60) {
                let _ = std::fs::write(
                    self.root.join("reports/preview-device-recovery-macos.json"),
                    b"{\"passed\":false,\"reason\":\"timeout\"}",
                );
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if self.lost.load(Ordering::Acquire) {
            ui.ctx()
                .graphics_mut(|graphics| *graphics = Default::default());
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn on_exit(&mut self) {
        if !self.lost.load(Ordering::Acquire)
            && !self.inject_loss.load(Ordering::Acquire)
            && let Some(app) = &mut self.app
        {
            app.on_exit();
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if (self.lost.load(Ordering::Acquire) || self.inject_loss.load(Ordering::Acquire))
            && let Some(mut app) = self.app.take()
        {
            app.detach_graphics();
            *self.retained.borrow_mut() = Some(app);
        }
    }
}
pub fn run(root: PathBuf, data: PathBuf, worker: PathBuf, startup: Startup) -> anyhow::Result<()> {
    let retained = Rc::new(RefCell::new(None::<TrueRenderer>));
    let expected_rating = Rc::new(Cell::new(0));
    let expected_selection = Rc::new(RefCell::new(None));
    let mut startup = Some(startup);
    let test = std::env::args()
        .any(|arg| arg == "--device-loss-smoke" || arg == "--device-loss-twice-smoke");
    let fail_twice = std::env::args().any(|arg| arg == "--device-loss-twice-smoke");
    for attempt in 0..=1 {
        let lost = Arc::new(AtomicBool::new(false));
        let creator_lost = lost.clone();
        let retained_app = retained.clone();
        let expected_rating = expected_rating.clone();
        let expected_selection = expected_selection.clone();
        let inject_loss = Arc::new(AtomicBool::new(false));
        let app_root = root.clone();
        let app_data = data.clone();
        let app_worker = worker.clone();
        let initial = startup.take();
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("TrueRenderer")
                .with_app_id("it.truerenderer.prototype")
                .with_inner_size([1440., 940.])
                .with_min_inner_size([1100., 720.]),
            renderer: eframe::Renderer::Wgpu,
            run_and_return: true,
            ..Default::default()
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            eframe::run_native(
                "TrueRenderer",
                options,
                Box::new(move |cc| {
                    let app = if let Some(mut app) = retained_app.borrow_mut().take() {
                        app.rebind_graphics(cc);
                        app
                    } else if let Some(initial) = initial {
                        TrueRenderer::new(cc, app_root.clone(), app_data, app_worker, initial)
                    } else {
                        return Err(
                            "Stato della sessione non disponibile per il recupero grafico".into(),
                        );
                    };
                    let device = cc.wgpu_render_state.as_ref().map(|gpu| gpu.device.clone());
                    let lost_at_end = creator_lost.clone();
                    let injected_at_end = inject_loss.clone();
                    cc.egui_ctx.on_end_pass(
                        "TrueRenderer device loss",
                        Arc::new(move |ctx| {
                            // egui can repeat a UI pass after App::ui. Suppress all
                            // passes, including shapes added by integration plugins.
                            if lost_at_end.load(Ordering::Acquire)
                                || injected_at_end.load(Ordering::Acquire)
                            {
                                ctx.graphics_mut(|graphics| *graphics = Default::default());
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }),
                    );
                    if let Some(device) = &device {
                        let lost = creator_lost.clone();
                        let ctx = cc.egui_ctx.clone();
                        let inject_loss = inject_loss.clone();
                        device.set_device_lost_callback(move |reason, _message| {
                            // Normal window teardown destroys the device too.
                            if reason != eframe::wgpu::DeviceLostReason::Destroyed
                                || inject_loss.load(Ordering::Acquire)
                            {
                                lost.store(true, Ordering::Release);
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                    Ok(Box::new(Host {
                        app: Some(app),
                        retained: retained_app.clone(),
                        lost: creator_lost,
                        device,
                        test,
                        fail_twice,
                        attempt,
                        started: Instant::now(),
                        root: app_root,
                        expected_rating,
                        inject_loss,
                        expected_selection,
                    }))
                }),
            )
        }));
        if result.is_err() && lost.load(Ordering::Acquire) {
            // winit's macOS delegate is not reusable after unwinding a paint
            // callback. Finish accepted saves during drop and report natively.
            anyhow::bail!(
                "Dispositivo perso durante il disegno; backend finestra non recuperabile in questa sessione. Salvataggi accettati completati; riavviare TrueRenderer."
            );
        }
        if !lost.load(Ordering::Acquire) {
            return match result {
                Ok(result) => result.map_err(|error| anyhow::anyhow!("Avvio UI: {error}")),
                Err(panic) => std::panic::resume_unwind(panic),
            };
        }
        // Host has dropped and detached all old presentation resources. Keep
        // services/durable writes alive while recreating at most once.
        if attempt == 1 {
            if test {
                std::fs::write(root.join("reports/preview-device-recovery-bounded-macos.json"),
                    b"{\"passed\":true,\"injected_losses\":2,\"recreations\":1,\"stopped_after_second_loss\":true}")?;
            }
            anyhow::bail!(
                "Dispositivo grafico perso di nuovo dopo un tentativo di recupero. Sessione chiusa; salvataggi accettati completati. Riavviare TrueRenderer."
            );
        }
    }
    unreachable!()
}
