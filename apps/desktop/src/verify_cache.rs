//! Native cache qualification and cold/warm measurements on generated images only.
use crate::{
    cache::{Manager, NAME, Settings},
    decode_pool::prepare_cached,
};
use anyhow::{Result, ensure};
use std::{path::Path, time::Instant};
use tr_core::{Annotation, Item};
use tr_platform::Broker;

pub fn run(root: &Path, worker: &Path) -> Result<()> {
    let folder = tempfile::Builder::new()
        .prefix("cache-qualification-")
        .tempdir_in(root.join("var"))?;
    let cache = Manager::new(Settings::default());
    cache.maintain(folder.path(), false)?;
    let mut checks = vec![];
    let mut broker = Broker::with_slot(worker.into(), 0);
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
        let start = Instant::now();
        let cold = prepare_cached(&cache, &mut broker, &item, 0, &|| false)?;
        let cold_seconds = start.elapsed().as_secs_f64();
        let jobs = broker.statistics().completed_jobs;
        let mut warm_times = vec![];
        let mut exact = true;
        for _ in 0..5 {
            let start = Instant::now();
            let warm = prepare_cached(&cache, &mut broker, &item, 0, &|| false)?;
            warm_times.push(start.elapsed().as_secs_f64());
            exact &= warm.transport.starts_with("Cache")
                && warm.prepared.histogram == cold.prepared.histogram
                && warm.prepared.pyramid.level_count() == cold.prepared.pyramid.level_count();
            for (a, b) in cold
                .prepared
                .pyramid
                .levels()
                .iter()
                .zip(warm.prepared.pyramid.levels())
            {
                exact &= a
                    .pixels
                    .iter()
                    .flatten()
                    .zip(b.pixels.iter().flatten())
                    .all(|(a, b)| a.to_bits() == b.to_bits());
            }
        }
        let restarted = Manager::new(Settings::load(folder.path())?);
        ensure!(
            restarted.load(folder.path(), &digest, &|| false).is_some(),
            "Cache non riusabile in una nuova sessione"
        );
        ensure!(
            tr_platform::snapshot(&path)?.1 == digest,
            "Originale modificato"
        );
        let passed = exact && broker.statistics().completed_jobs == jobs;
        checks.push(serde_json::json!({"file":name,"passed":passed,"cold_seconds_including_write":cold_seconds,"warm_seconds":warm_times,"bit_exact_all_levels":exact,"no_warm_decoder_jobs":broker.statistics().completed_jobs==jobs,"source_unchanged":true,"reopened_cache_hit":true}));
        println!("Cache {name}: passed={passed}, cold={cold_seconds:.3}s, warm={warm_times:?}");
    }
    let before = cache.stats();
    cache.maintain(folder.path(), true)?;
    let cleaned = std::fs::read_dir(folder.path().join(NAME).join("entries"))?
        .next()
        .is_none()
        && std::fs::read_dir(folder.path().join(NAME).join("tmp"))?
            .next()
            .is_none();
    let passed = cleaned && checks.iter().all(|c| c["passed"] == true);
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":passed,"checks":checks,"cache_before_clear":before,"clear_removes_only_derivatives":cleaned,"scope":"Three generated sources; one cold application-cache run and five warm runs each; OS cache is not flushed. Bit-exact full fp32 pyramids and histogram. Not p95 or whole-machine memory qualification."});
    std::fs::write(
        root.join("reports/cache-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(passed, "Verifica cache fallita");
    Ok(())
}
