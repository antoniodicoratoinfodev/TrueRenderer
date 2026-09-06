//! Internal qualification command; sources are always the compiled R0 corpus.
use anyhow::{Result, ensure};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tr_platform::Broker;

pub fn run_growth(root: &Path, binary: &Path) -> Result<()> {
    let path = root.join("corpus/01_Studio_cromatico.png");
    let (_, digest) = tr_platform::snapshot(&path)?;
    let mut broker = Broker::with_slot(binary.into(), 0);
    broker.decode(&path, &digest, 320)?;
    let pid = broker.statistics().worker_pid;
    broker.inject_memory_growth_for_probe()?;
    let start = Instant::now();
    let error = loop {
        if let Err(error) = broker.supervise_idle() {
            break error.to_string();
        }
        ensure!(
            start.elapsed() < Duration::from_secs(5),
            "La crescita di memoria non è stata contenuta"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    ensure!(
        error.contains("memoria"),
        "Errore diverso dalla quota: {error}"
    );
    let stopped = broker.statistics().clone();
    let termination_ms = start.elapsed().as_millis();
    drop(broker);
    let mut recovered = Broker::with_slot(binary.into(), 0);
    recovered.decode(&path, &digest, 320)?;
    ensure!(
        recovered.statistics().worker_pid != pid,
        "Servizio non sostituito dopo la quota"
    );
    let limit = 384u64 * 1024 * 1024;
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),
        "passed":true,"scope":"bounded 512 MiB fault in a separately compiled qualification service; not a hard memory cap",
        "threshold_bytes":limit,"sampling_interval_ms":25,"termination_ms":termination_ms,
        "maximum_observed_bytes":stopped.peak_rss_bytes.max(stopped.peak_footprint_bytes),
        "observed_overshoot_bytes":stopped.peak_rss_bytes.max(stopped.peak_footprint_bytes).saturating_sub(limit),
        "statistics":stopped,"recovered_pid":recovered.statistics().worker_pid,
        "cooperative_cancellation_ignored":true,"fault_code_in_distribution_bundle":false,
        "hard_memory_cap":false,"full_sandbox_gate_passed":false});
    let text = serde_json::to_string_pretty(&report)?;
    std::fs::write(root.join("reports/xpc-memory-growth-macos.json"), &text)?;
    println!("{text}");
    Ok(())
}

pub fn run(root: &Path, binary: &Path) -> Result<()> {
    let started = Instant::now();
    let path = root.join("corpus/01_Studio_cromatico.png");
    let (_, digest) = tr_platform::snapshot(&path)?;
    let mut first = Broker::with_slot(binary.into(), 0);
    let mut second = Broker::with_slot(binary.into(), 1);
    ensure!(
        first.transport().starts_with("XPC"),
        "Il test richiede il bundle XPC"
    );
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| first.decode(&path, &digest, 320));
        let b = scope.spawn(|| second.decode(&path, &digest, 320));
        (a.join().unwrap(), b.join().unwrap())
    });
    let (a, b) = (a?, b?);
    ensure!(
        a.raster.pixels == b.raster.pixels,
        "I due decoder producono campioni diversi"
    );
    let first_pid = first.statistics().worker_pid;
    let second_pid = second.statistics().worker_pid;
    ensure!(
        first_pid.is_some() && second_pid.is_some() && first_pid != second_pid,
        "I due isolati non sono distinti"
    );
    first.suspend_for_probe()?;
    first.tighten_limits_for_probe(Duration::from_millis(200), 384 * 1024 * 1024);
    let deadline = Instant::now();
    let failure = first.decode(&path, &digest, 320).unwrap_err().to_string();
    ensure!(
        failure.contains("timeout") && deadline.elapsed() < Duration::from_secs(2),
        "Timeout non contenuto: {failure}"
    );
    let timeout_ms = deadline.elapsed().as_millis();
    let first_stopped = first.statistics().clone();
    drop(first);
    ensure!(
        second.decode(&path, &digest, 320)?.raster.pixels == b.raster.pixels,
        "Crash ha coinvolto l'altro isolato"
    );
    let mut recovered = Broker::with_slot(binary.into(), 0);
    recovered.decode(&path, &digest, 320)?;
    ensure!(
        recovered.statistics().worker_pid != first_pid,
        "Il worker sospeso non è stato sostituito"
    );
    let recovered_pid = recovered.statistics().worker_pid;
    recovered.tighten_limits_for_probe(Duration::from_secs(12), 1);
    let memory_failure = recovered
        .decode(&path, &digest, 320)
        .unwrap_err()
        .to_string();
    ensure!(
        memory_failure.contains("memoria"),
        "Supervisione memoria non attiva: {memory_failure}"
    );
    let memory_stopped = recovered.statistics().clone();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("corpus/manifest.json"))?)?;
    let mut checked = 0;
    for entry in manifest["images"]
        .as_array()
        .expect("compiled corpus manifest")
    {
        let name = entry["file"]
            .as_str()
            .or_else(|| entry["name"].as_str())
            .expect("corpus filename");
        let path = root.join("corpus").join(name);
        let (_, digest) = tr_platform::snapshot(&path)?;
        second.decode(&path, &digest, 320)?;
        checked += 1;
    }
    let report = serde_json::json!({
        "application":"TrueRenderer", "version":env!("CARGO_PKG_VERSION"), "passed":true,
        "scope":"integrated XPC R0 corpus, two processes, timeout and memory supervision; full sandbox/release gate open",
        "first_pid":first_pid,"second_pid":second_pid,"recovered_pid":recovered_pid,
        "two_distinct_processes":true,"same_pixels":true,"suspended_worker_timeout_ms":timeout_ms,
        "other_isolate_survived":true,"corpus_images_checked":checked,
        "first_before_stop":first_stopped,"memory_before_stop":memory_stopped,"second":second.statistics(),
        "elapsed_seconds":started.elapsed().as_secs_f64(),"memory_limit_is_hard_kernel_cap":false,
        "lifecycle_api":"Mach task/audit token + dynamically resolved libproc SPI; no PID-only signal",
        "full_sandbox_gate_passed":false
    });
    std::fs::create_dir_all(root.join("reports"))?;
    let text = serde_json::to_string_pretty(&report)?;
    std::fs::write(root.join("reports/xpc-integration-macos.json"), &text)?;
    println!("{text}");
    Ok(())
}
