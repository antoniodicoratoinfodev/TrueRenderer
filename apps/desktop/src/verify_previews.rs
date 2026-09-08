//! Reproducible navigation/cache checks on generated sources. No private photos.
use crate::{
    cache::{Manager, Settings},
    decode_pool::{DecodePool, Job},
    service::{Event, PreviewDecoded},
};
use anyhow::{Result, ensure};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicU64, mpsc},
    time::{Duration, Instant},
};
use tr_core::{
    Annotation, Item,
    preview::{PreviewQuality, PreviewRequest},
};

fn request(
    pool: &DecodePool,
    events: &mpsc::Receiver<Event>,
    item: &Item,
    request: PreviewRequest,
) -> Result<(PreviewDecoded, f64)> {
    let start = Instant::now();
    ensure!(
        pool.submit(Job {
            item: item.clone(),
            request,
            priority: tr_core::preview::PreviewPriority::Immediate,
            generation: 1
        })
        .is_ok(),
        "Queue refused visible work"
    );
    loop {
        ensure!(
            start.elapsed() < Duration::from_secs(60),
            "Preview timed out"
        );
        if let Event::Image {
            id,
            request: actual,
            result,
            ..
        } = events.recv_timeout(Duration::from_secs(60))?
        {
            ensure!(
                id == item.id && actual == request,
                "Late/incompatible result"
            );
            return Ok((
                (*result).map_err(anyhow::Error::msg)?,
                start.elapsed().as_secs_f64(),
            ));
        }
    }
}
pub fn run(root: &Path, worker: &Path) -> Result<()> {
    let folder = tempfile::Builder::new()
        .prefix("preview-qualification-")
        .tempdir_in(root.join("var"))?;
    let manager = Arc::new(Manager::new(Settings {
        free_mib: 0,
        ..Settings::default()
    }));
    let generation = Arc::new(AtomicU64::new(1));
    let (tx, events) = mpsc::sync_channel(16);
    let pool = DecodePool::start(
        worker.into(),
        generation,
        tx,
        eframe::egui::Context::default(),
        manager.clone(),
    );
    let mut checks = Vec::new();
    for name in ["12mp-jpeg.jpg", "07-bayer.dng", "16bit.png"] {
        let path = folder.path().join(name);
        std::fs::copy(root.join("var/format-fixtures").join(name), &path)?;
        let (bytes, digest) = tr_platform::snapshot(&path)?;
        let item = Item {
            id: name.into(),
            path: path.clone(),
            name: name.into(),
            bytes: bytes.len() as u64,
            digest: digest.clone(),
            approved: true,
            annotation: Annotation::default(),
            revision: 0,
        };
        drop(bytes);
        for quality in [PreviewQuality::Full, PreviewQuality::Standard] {
            let requested = PreviewRequest { quality, edge: 256 };
            let writes = manager.stats().writes;
            let (cold, cold_seconds) = request(&pool, &events, &item, requested)?;
            let expected: Vec<_> = cold
                .prepared
                .image
                .levels()
                .iter()
                .flat_map(|l| l.pixels.iter().flatten().map(|v| v.to_bits()))
                .collect();
            let resident_bytes = cold.prepared.image.byte_len();
            let base = cold.prepared.image.base_level();
            let source_size = cold.prepared.image.source_size();
            drop(cold);
            let start = Instant::now();
            while manager.stats().writes == writes && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(20));
            }
            ensure!(
                manager.stats().writes > writes,
                "No asynchronous cache write: {}",
                manager.stats().message
            );
            let mut warm_seconds = Vec::new();
            for _ in 0..5 {
                let (warm, seconds) = request(&pool, &events, &item, requested)?;
                ensure!(
                    warm.transport.starts_with("Cache"),
                    "Unexpected RAW decode on cache reuse"
                );
                let actual: Vec<_> = warm
                    .prepared
                    .image
                    .levels()
                    .iter()
                    .flat_map(|l| l.pixels.iter().flatten().map(|v| v.to_bits()))
                    .collect();
                ensure!(actual == expected, "fp32 bits changed through cache");
                warm_seconds.push(seconds);
            }
            ensure!(tr_platform::snapshot(&path)?.1 == digest, "Source changed");
            checks.push(serde_json::json!({"source":name,"quality":quality,"requested_edge":256,"source_dimensions":source_size,"first_resident_level":base,"resident_bytes":resident_bytes,"cold_delivery_seconds":cold_seconds,"warm_seconds":warm_seconds,"fp32_bit_exact":true,"warm_raw_decodes":0}));
        }
    }
    drop(pool);
    ensure!(
        manager.memory.usage().reserved == manager.baseline_bytes,
        "Credits leaked after shutdown"
    );
    let usage = manager.memory.usage();
    let mut stats = manager.stats();
    stats.folder = "generated temporary test folder".into();
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":true,"checks":checks,
        "cache":stats,"admission_limit_bytes":usage.limit,"peak_reserved_bytes":usage.peak,"working_credits_after_shutdown":usage.reserved-manager.baseline_bytes,"baseline_allowance_bytes":manager.baseline_bytes,
        "scope":"Generated JPEG 12 MP, Bayer DNG 1024x768 and 16-bit PNG. Five warm trials; OS cache not flushed. End-to-end request to CPU artifact delivery includes queue, source read/hash, cache verification or probe/decode/filter; excludes presentation. Not p95, total RSS/GPU qualification or 1000 real RAWs."});
    std::fs::write(
        root.join("reports/preview-cache-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}

/// Catalog and explicit preparation harness on the project's distinct synthetic
/// Bayer signals. Source reads/hash remain included; no real-photo claim.
pub fn navigation(root: &Path, worker: &Path) -> Result<()> {
    let folder = root.join("var/preview-corpus");
    let start = Instant::now();
    let mut paths = std::fs::read_dir(&folder)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "dng"))
        .collect::<Vec<_>>();
    paths.sort();
    ensure!(
        paths.len() == 1000,
        "Generate the 1000 distinct synthetic DNGs first"
    );
    let items = paths
        .iter()
        .enumerate()
        .map(|(i, path)| -> Result<Item> {
            let metadata = path.metadata()?;
            Ok(Item {
                id: i.to_string(),
                name: format!("generated-{i}"),
                path: path.clone(),
                bytes: metadata.len(),
                digest: tr_platform::observation_token(&metadata),
                approved: true,
                annotation: Default::default(),
                revision: 0,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let listing_seconds = start.elapsed().as_secs_f64();
    let manager = Arc::new(Manager::new(Settings {
        free_mib: 0,
        ..Default::default()
    }));
    manager.maintain(&folder, true)?;
    let startup_cleanup_evictions = manager.stats().evictions;
    let start_pool = || {
        let (tx, rx) = mpsc::sync_channel(16);
        (
            DecodePool::start(
                worker.into(),
                Arc::new(AtomicU64::new(1)),
                tx,
                eframe::egui::Context::default(),
                manager.clone(),
            ),
            rx,
        )
    };
    let (pool, events) = start_pool();
    let requested = PreviewRequest {
        quality: PreviewQuality::Full,
        edge: 256,
    };
    let mut cold = Vec::new();
    let mut digests = std::collections::HashSet::new();
    let preparation = Instant::now();
    for (i, item) in items.iter().enumerate() {
        let writes = manager.stats().writes;
        let (decoded, seconds) = request(&pool, &events, item, requested)?;
        ensure!(
            digests.insert(decoded.digest.clone()),
            "Sources must have distinct contents"
        );
        cold.push(seconds);
        drop(decoded);
        let wait = Instant::now();
        while manager.stats().writes == writes && wait.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        ensure!(
            manager.stats().writes > writes,
            "Background cache write failed: {}",
            manager.stats().message
        );
        if (i + 1) % 100 == 0 {
            eprintln!("Prepared {} / 1000 synthetic RAWs", i + 1);
        }
    }
    let preparation_seconds = preparation.elapsed().as_secs_f64();
    drop(pool);
    drop(events);
    let (pool, events) = start_pool();
    let before = manager.stats().decode_jobs;
    let mut warm = Vec::new();
    // Evenly distributed returns, a distant jump and repeated alternation.
    let trace = (0..100).map(|i| i * 10).chain([10, 900, 10, 900, 10, 900]);
    for index in trace {
        let (decoded, seconds) = request(&pool, &events, &items[index], requested)?;
        ensure!(
            decoded.transport.starts_with("Cache"),
            "Unexpected decode on populated cache"
        );
        ensure!(digests.contains(&decoded.digest), "Changed source revision");
        warm.push(seconds);
    }
    drop(pool);
    let usage = manager.memory.usage();
    ensure!(
        usage.reserved == manager.baseline_bytes,
        "Working credits leaked"
    );
    let mut stats = manager.stats();
    stats.folder = "generated 1000-DNG test folder".into();
    stats.evictions = stats.evictions.saturating_sub(startup_cleanup_evictions);
    let warm_decodes = stats.decode_jobs - before;
    ensure!(warm_decodes == 0, "Warm path decoded RAW");
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":true,"distinct_synthetic_raws":1000,"source_dimensions":[512,384],"quality":"Full","requested_edge":256,"metadata_listing_seconds":listing_seconds,"explicit_preparation_seconds_including_async_writes":preparation_seconds,"cold_delivery_seconds":cold,"reopened_cache_delivery_seconds":warm,"warm_raw_decodes":warm_decodes,"cache":stats,"peak_reserved_bytes":usage.peak,"baseline_allowance_bytes":manager.baseline_bytes,"working_credits_after_shutdown":usage.reserved-manager.baseline_bytes,"scope":"1000 different generated Bayer signals without embedded previews; sequential explicit preparation and 106 returns through a newly started pool. Includes source/hash/cache/decode to CPU artifact delivery, excludes GUI/surface presentation. OS cache not flushed. No event-to-frame p95, real camera/12-24-45 MP, total GPU/RSS or R0 qualification."});
    let mut report = report;
    report["startup_cleanup_evictions"] = startup_cleanup_evictions.into();
    report["cache_evictions_scope"] =
        "Only evictions during this run, excluding explicit startup cleanup".into();
    std::fs::write(
        root.join("reports/preview-navigation-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "1000 distinct synthetic RAWs prepared; 106 reopened-cache returns, no new RAW decode"
    );
    Ok(())
}
