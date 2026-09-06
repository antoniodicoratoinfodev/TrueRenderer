mod decode_pool;
mod service;
mod ui;
mod verify_formats;
mod verify_resampling;
mod verify_xpc;
use anyhow::{Context, Result};
use eframe::egui;
use std::{fs::OpenOptions, path::PathBuf};

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let executable = std::env::current_exe()?;
    let root = option("--root")
        .or_else(|| {
            executable
                .ancestors()
                .skip(1)
                .take(6)
                .find(|path| path.join("corpus/manifest.json").is_file())
                .map(std::path::Path::to_path_buf)
        })
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .canonicalize()?;
    let sampling_smoke = args.iter().any(|a| a == "--sampling-smoke");
    let external_smoke = args.iter().any(|a| a == "--formats-smoke");
    let smoke = external_smoke || sampling_smoke || args.iter().any(|a| a == "--smoke-test");
    let initial_open =
        option("--open").or_else(|| external_smoke.then(|| root.join("var/format-fixtures")));
    let worker = executable
        .parent()
        .context("Cartella binario")?
        .join(if cfg!(windows) {
            "tr-worker.exe"
        } else {
            "tr-worker"
        });
    if args.iter().any(|a| a == "--verify-formats") {
        return verify_formats::run(&root, &worker);
    }
    if args.iter().any(|a| a == "--verify-sampling-screenshots") {
        return verify_resampling::screenshots(&root, &worker);
    }
    if args.iter().any(|a| a == "--verify-resampling") {
        return verify_resampling::run(&root, &worker);
    }
    if args.iter().any(|a| a == "--verify-xpc") {
        return verify_xpc::run(&root, &worker);
    }
    if args.iter().any(|a| a == "--verify-xpc-growth") {
        return verify_xpc::run_growth(&root, &worker);
    }
    let data =
        option("--data").unwrap_or_else(|| root.join(if smoke { "var/smoke" } else { "var" }));
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(root.join("reports"))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data.join("instance.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("TrueRenderer è già aperto con questa libreria")?;
    anyhow::ensure!(
        worker.is_file(),
        "Manca tr-worker accanto all'applicazione. Eseguire scripts/build-macos.sh."
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TrueRenderer")
            .with_app_id("it.truerenderer.prototype")
            .with_inner_size([1440., 940.])
            .with_min_inner_size([1100., 720.]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "TrueRenderer",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::TrueRenderer::new(
                cc,
                root,
                data,
                worker,
                ui::Startup {
                    smoke,
                    sampling_smoke,
                    external_smoke,
                    open: initial_open,
                },
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Avvio UI: {e}"))?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("TrueRenderer: {error:#}");
        if std::env::args().any(|a| {
            a.starts_with("--verify-")
                || a == "--smoke-test"
                || a == "--sampling-smoke"
                || a == "--formats-smoke"
        }) {
            std::process::exit(1);
        }
        rfd::MessageDialog::new()
            .set_title("TrueRenderer")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}
