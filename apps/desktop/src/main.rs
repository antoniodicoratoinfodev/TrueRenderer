mod cache;
mod decode_pool;
mod graphics;
mod service;
mod source_monitor;
mod ui;
mod verify_cache;
mod verify_formats;
mod verify_previews;
mod verify_resampling;
mod verify_xpc;
mod wake;
use anyhow::{Context, Result};
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
    let source_smoke = args.iter().any(|a| a == "--source-change-smoke");
    let graphics_smoke = args
        .iter()
        .any(|a| a == "--device-loss-smoke" || a == "--device-loss-twice-smoke");
    let external_smoke = args.iter().any(|a| a == "--formats-smoke");
    let settings_smoke = args.iter().any(|a| {
        a == "--settings-smoke" || a == "--preview-gpu-smoke" || a == "--preview-performance-smoke"
    });
    let smoke = source_smoke
        || settings_smoke
        || external_smoke
        || sampling_smoke
        || args.iter().any(|a| a == "--smoke-test");
    let mut initial_open =
        option("--open").or_else(|| external_smoke.then(|| root.join("var/format-fixtures")));
    if source_smoke {
        let folder = root.join("var/source-change-fixture");
        std::fs::create_dir_all(&folder)?;
        let path = folder.join("watched.png");
        std::fs::copy(root.join("corpus/01_Studio_cromatico.png"), &path)?;
        initial_open = Some(path);
    }
    let worker = executable
        .parent()
        .context("Cartella binario")?
        .join(if cfg!(windows) {
            "tr-worker.exe"
        } else {
            "tr-worker"
        });
    if args.iter().any(|a| a == "--verify-cache") {
        return verify_cache::run(&root, &worker);
    }
    if args.iter().any(|a| a == "--verify-preview-navigation") {
        return verify_previews::navigation(&root, &worker);
    }
    if args.iter().any(|a| a == "--verify-previews") {
        return verify_previews::run(&root, &worker);
    }
    if let Some(folder) = option("--verify-real-raws") {
        let memory_mib = option("--raw-memory-mib")
            .map(|value| value.to_string_lossy().parse::<u64>())
            .transpose()
            .context("--raw-memory-mib deve essere un intero")?
            .unwrap_or(0);
        return verify_previews::real_raws(&root, &worker, &folder, memory_mib);
    }
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
    let data = option("--data").unwrap_or_else(|| {
        root.join(if smoke || graphics_smoke {
            "var/smoke"
        } else {
            "var"
        })
    });
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
    graphics::run(
        root,
        data,
        worker,
        ui::Startup {
            smoke,
            sampling_smoke,
            external_smoke,
            settings_smoke,
            open: initial_open,
        },
    )?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("TrueRenderer: {error:#}");
        if std::env::args().any(|a| {
            a.starts_with("--verify-")
                || a.ends_with("-smoke")
                || a == "--smoke-test"
                || a == "--sampling-smoke"
                || a == "--formats-smoke"
                || a == "--settings-smoke"
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
