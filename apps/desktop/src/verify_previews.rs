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

/// Explicitly authorized folder only. Work on private temporary copies so even
/// cache maintenance never writes beside the user's originals. No source paths,
/// filenames, hashes or pixel data are included in the report.
pub fn real_raws(root: &Path, worker: &Path, folder: &Path, memory_mib: u64) -> Result<()> {
    let path = root.join("reports/preview-real-raw-macos.json");
    std::fs::create_dir_all(root.join("reports"))?;
    let mut status = serde_json::json!({
        "application": "TrueRenderer", "version": env!("CARGO_PKG_VERSION"),
        "passed": false, "status": "running", "requested_memory_mib": memory_mib,
        "scope": "Current authorized RAW verification. An incomplete or failed run does not certify originals, cache or memory. Detailed errors remain in the local command log."
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&status)?)?;
    let result = real_raws_inner(root, worker, folder, memory_mib);
    if result.is_err() {
        // Cover every exit, including warm-cache, detail and original-integrity
        // failures. Never leave a previous successful run as the current report.
        // Error chains may contain private filenames; keep them in stderr only.
        status["status"] = "failed".into();
        std::fs::write(path, serde_json::to_vec_pretty(&status)?)?;
    }
    result
}

fn real_raws_inner(root: &Path, worker: &Path, folder: &Path, memory_mib: u64) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Real RAW verification requires the sandboxed XPC bundle"
    );
    let mut paths: Vec<_> = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|ext| {
                    matches!(
                        ext.to_string_lossy().to_ascii_lowercase().as_str(),
                        "nef" | "dng" | "arw" | "cr2" | "cr3" | "raf" | "orf" | "rw2"
                    )
                })
        })
        .collect();
    paths.sort();
    ensure!(
        !paths.is_empty() && paths.len() <= 1000,
        "Expected 1–1000 authorized RAW files"
    );
    let temporary = tempfile::Builder::new()
        .prefix("real-raw-check-")
        .tempdir_in(root.join("var"))?;
    let settings = Settings {
        memory_mib,
        ..Settings::default()
    };
    settings.validate()?;
    let manager = Arc::new(Manager::new(settings));
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
    let mut cases = Vec::new();
    let mut originals = Vec::new();
    let mut items = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let before = tr_platform::snapshot(path)?.1;
        let copy = temporary.path().join(format!(
            "asset-{index}.{}",
            path.extension().unwrap().to_string_lossy()
        ));
        std::fs::copy(path, &copy)?;
        ensure!(
            tr_platform::snapshot(&copy)?.1 == before,
            "Source changed during copy"
        );
        let metadata = copy.metadata()?;
        items.push(Item {
            id: index.to_string(),
            name: format!("asset-{index}"),
            path: copy,
            bytes: metadata.len(),
            digest: before.clone(),
            observation: tr_platform::observation_token(&metadata),
            approved: true,
            annotation: Annotation::default(),
            revision: 0,
        });
        originals.push(before);
    }
    let (pool, events) = start_pool();
    for (index, item) in items.iter().enumerate() {
        for quality in [PreviewQuality::Standard, PreviewQuality::Full] {
            let requested = PreviewRequest { quality, edge: 256 };
            let writes = manager.stats().writes;
            let (cold, seconds) = match request(&pool, &events, item, requested) {
                Ok(result) => result,
                Err(error) => {
                    drop(pool);
                    let unchanged = paths.iter().zip(&originals).all(|(path, digest)| {
                        tr_platform::snapshot(path).is_ok_and(|(_, actual)| actual == *digest)
                    });
                    let usage = manager.memory.usage();
                    let report = serde_json::json!({"passed":false,"application":"TrueRenderer",
                        "version":env!("CARGO_PKG_VERSION"),"completed_cases":cases,"failed_asset_index":index,
                        "failure":format!("{error:#}"),"requested_memory_mib":memory_mib,
                        "admission_limit_bytes":usage.limit,"peak_reserved_bytes":usage.peak,
                        "working_credits_after_shutdown":usage.reserved-manager.baseline_bytes,
                        "originals_unchanged":unchanged,"scope":"Failed cold-delivery qualification on authorized copies; not a throughput or p95 claim."});
                    std::fs::write(
                        root.join("reports/preview-real-raw-failure-macos.json"),
                        serde_json::to_vec_pretty(&report)?,
                    )?;
                    return Err(error);
                }
            };
            let size = cold.prepared.image.source_size();
            ensure!(
                cold.info.format == "RAW" && cold.info.decoder.starts_with("Apple RAW "),
                "Real RAW was decoded as an embedded bitmap: asset {index}"
            );
            ensure!(
                size[0] as u64 * size[1] as u64 >= 1_000_000,
                "Unexpected thumbnail dimensions for authorized camera RAW: asset {index}"
            );
            let resident = cold.prepared.image.byte_len();
            let expected: Vec<_> = cold
                .prepared
                .image
                .levels()
                .iter()
                .flat_map(|l| l.pixels.iter().flatten().map(|v| v.to_bits()))
                .collect();
            drop(cold);
            let wait = Instant::now();
            while manager.stats().writes == writes && wait.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(20));
            }
            ensure!(
                manager.stats().writes > writes,
                "Cache persistence unavailable"
            );
            let (warm, warm_seconds) = request(&pool, &events, item, requested)?;
            ensure!(
                warm.transport.starts_with("Cache"),
                "Unexpected decode on warm lookup"
            );
            ensure!(
                warm.prepared
                    .image
                    .levels()
                    .iter()
                    .flat_map(|l| l.pixels.iter().flatten().map(|v| v.to_bits()))
                    .eq(expected.into_iter()),
                "RAW derivative changed through cache"
            );
            cases.push(
                serde_json::json!({"asset_index":index,"quality":quality,"edge":256,
                "source_dimensions":size,"resident_bytes":resident,"cold_delivery_seconds":seconds,
                "warm_delivery_seconds":warm_seconds,"fp32_bit_exact":true}),
            );
        }
        eprintln!("Real RAW verified {} / {}", index + 1, items.len());
    }
    drop(pool);
    drop(events);
    // Two cold visible requests must serialize within budget instead of failing
    // merely because another decoder still owns its temporary allocations.
    let burst_folder = temporary.path().join("concurrent");
    std::fs::create_dir(&burst_folder)?;
    let (pool, events) = start_pool();
    let burst_request = PreviewRequest {
        quality: PreviewQuality::Full,
        edge: 256,
    };
    let burst_count = items.len().min(2);
    for item in items.iter().take(burst_count) {
        let mut item = item.clone();
        let path = burst_folder.join(item.path.file_name().unwrap());
        std::fs::copy(&item.path, &path)?;
        item.path = path;
        pool.submit(Job {
            item,
            request: burst_request,
            priority: tr_core::preview::PreviewPriority::Immediate,
            generation: 1,
        })
        .map_err(|_| anyhow::anyhow!("Concurrent visible request refused"))?;
    }
    let mut burst_completed = std::collections::HashSet::new();
    let burst_started = Instant::now();
    while burst_completed.len() < burst_count {
        ensure!(
            burst_started.elapsed() < Duration::from_secs(90),
            "Concurrent RAW requests timed out"
        );
        if let Event::Image {
            id,
            request,
            result,
            ..
        } = events.recv_timeout(Duration::from_secs(60))?
        {
            let decoded = (*result).map_err(anyhow::Error::msg)?;
            ensure!(
                request == burst_request && burst_completed.insert(id),
                "Duplicate concurrent result"
            );
            ensure!(
                decoded.info.format == "RAW" && decoded.prepared.image.sufficient_for(request),
                "Invalid concurrent RAW result"
            );
        }
    }
    drop(pool);
    drop(events);
    ensure!(
        manager.memory.usage().reserved == manager.baseline_bytes,
        "Concurrent requests leaked credits"
    );
    let (pool, events) = start_pool();
    let before = manager.stats().decode_jobs;
    let mut reopened = Vec::new();
    for item in &items {
        let (decoded, seconds) = request(
            &pool,
            &events,
            item,
            PreviewRequest {
                quality: PreviewQuality::Standard,
                edge: 256,
            },
        )?;
        ensure!(
            decoded.transport.starts_with("Cache"),
            "Unexpected decode after reopening pool"
        );
        reopened.push(seconds);
    }
    let warm_decodes = manager.stats().decode_jobs - before;
    // Viewer/detail checks are separate from thumbnail cache timings. Heavy
    // optional artifacts may exceed the 64 MiB writer retention limit.
    let mut detail = Vec::new();
    for item in items.iter().take(3) {
        for requested in [
            PreviewRequest {
                quality: PreviewQuality::Standard,
                edge: 2048,
            },
            PreviewRequest::full(),
        ] {
            let (decoded, seconds) = request(&pool, &events, item, requested)?;
            ensure!(
                decoded.prepared.image.sufficient_for(requested),
                "Insufficient native detail"
            );
            detail.push(
                serde_json::json!({"asset_index":item.id,"request":requested,
                "source_dimensions":decoded.prepared.image.source_size(),
                "base_level":decoded.prepared.image.base_level(),
                "resident_bytes":decoded.prepared.image.byte_len(),"delivery_seconds":seconds}),
            );
        }
    }
    drop(pool);
    for (path, expected) in paths.iter().zip(&originals) {
        ensure!(
            tr_platform::snapshot(path)?.1 == *expected,
            "Original changed during verification"
        );
    }
    let usage = manager.memory.usage();
    ensure!(
        usage.reserved == manager.baseline_bytes && warm_decodes == 0,
        "Leaked credits or unexpected warm decode"
    );
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),
        "passed":true,"authorized_raw_count":items.len(),"distinct_sources":originals.iter().collect::<std::collections::HashSet<_>>().len(),
        "originals_unchanged":true,"cases":cases,"reopened_standard_seconds":reopened,
        "concurrent_cold_visible_requests":burst_count,"concurrent_requests_passed":true,
        "reopened_raw_decodes":warm_decodes,"detail_checks":detail,
        "peak_reserved_bytes":usage.peak,"admission_limit_bytes":usage.limit,"requested_memory_mib":memory_mib,"working_credits_after_shutdown":0,
        "scope":"Authorized real RAWs on private temporary copies; Standard/Full thumbnails and three viewer/native-detail examples. CPU artifact delivery includes source/hash/cache or decode, excludes GUI presentation. OS cache not flushed; one cold and one warm sample per quality/source, not p95/p99, 1000 real RAWs or driver/RSS qualification. Originals verified with SHA-256 before/after; no personal paths, names, hashes or pixels published."});
    std::fs::write(
        root.join("reports/preview-real-raw-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "Verified {} real RAWs, originals unchanged, zero reopened-cache decodes",
        items.len()
    );
    Ok(())
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
            observation: String::new(),
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
                observation: String::new(),
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

#[cfg(test)]
mod report_tests {
    #[test]
    fn failed_raw_verification_replaces_a_previous_success_without_private_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("reports")).unwrap();
        let report = root.path().join("reports/preview-real-raw-macos.json");
        std::fs::write(&report, br#"{"passed":true,"originals_unchanged":true}"#).unwrap();
        let result = super::real_raws(
            root.path(),
            &root.path().join("tr-worker"),
            &root.path().join("private-photo-folder"),
            2048,
        );
        assert!(result.is_err());
        let text = std::fs::read_to_string(report).unwrap();
        let status: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(status["passed"], false);
        assert_eq!(status["status"], "failed");
        assert!(status.get("originals_unchanged").is_none());
        assert!(!text.contains("private-photo-folder"));
    }
}
