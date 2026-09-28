//! Explicit-source, isolated WB diagnostics; never publishes source bytes.
use anyhow::{Result, ensure};
use std::{path::Path, time::Instant};
use tr_core::{decoder::RawEngine, raw_wb::Analysis};
use tr_platform::Broker;
pub fn run(root: &Path, binary: &Path, path: &Path) -> Result<()> {
    let (_, digest) = tr_platform::snapshot(path)?;
    let mut cases = Vec::new();
    let mut cancellations = Vec::new();
    for engine in RawEngine::choices() {
        let mut broker = Broker::new(binary.into());
        broker.set_raw_engine(engine);
        let decoded = broker.decode(path, &digest, 0)?;
        let (w, h) = (decoded.raster.width, decoded.raster.height);
        let mut patch = None;
        for j in 1..8 {
            for i in 1..8 {
                let (x, y) = (w * i / 8, h * j / 8);
                if patch.is_none()
                    && tr_core::editing::sample_rgb_area(&decoded.raster, x, y, 5).is_ok()
                {
                    patch = Some(Analysis::Patch { x, y, side: 5 });
                }
            }
        }
        drop(decoded);
        if patch.is_none() {
            cases.push(serde_json::json!({"engine":engine,"analysis":"Patch",
                "passed":false,"error":"Nessuna area 5×5 valida: prova contagocce non eseguita"}));
        }
        let source = broker.prepare_snapshot(path, &digest, || false)?;
        let calls = std::cell::Cell::new(0);
        let stops = broker.statistics().forced_stops;
        let start = Instant::now();
        let cancelled = broker.estimate_raw_wb(source, Analysis::Auto, || {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        });
        ensure!(
            cancelled.is_err() && broker.statistics().forced_stops == stops + 1,
            "Il worker WB non è stato interrotto"
        );
        cancellations.push(serde_json::json!({"engine":engine,"cancelled":true,
            "forced_stops":1,"seconds":start.elapsed().as_secs_f64()}));
        // The following analyses must recover on this same broker after cancellation.
        for analysis in std::iter::once(Analysis::Auto).chain(patch) {
            let start = Instant::now();
            broker.set_raw_white_balance(Default::default());
            let source = broker.prepare_snapshot(path, &digest, || false)?;
            let result = broker.estimate_raw_wb(source, analysis, || false);
            let record = match result {
                Ok(wb) => {
                    wb.validate_for(engine)?;
                    broker.set_raw_white_balance(wb);
                    let rendered = broker.decode(path, &digest, 0)?;
                    ensure!(
                        rendered.raster.width == w && rendered.raster.height == h,
                        "Geometria cambiata"
                    );
                    serde_json::json!({"engine":engine,"analysis":analysis,"passed":true,"wb":wb,"size":[w,h],"seconds":start.elapsed().as_secs_f64()})
                }
                Err(error) => {
                    serde_json::json!({"engine":engine,"analysis":analysis,"passed":false,"error":format!("{error:#}"),"seconds":start.elapsed().as_secs_f64()})
                }
            };
            println!("{record}");
            cases.push(record);
        }
    }
    let (_, after) = tr_platform::snapshot(path)?;
    write_report(root, &digest, digest == after, cases, cancellations)
}

fn write_report(
    root: &Path,
    digest: &str,
    source_unchanged: bool,
    cases: Vec<serde_json::Value>,
    cancellations: Vec<serde_json::Value>,
) -> Result<()> {
    let engines = RawEngine::choices().count();
    let passed = source_unchanged
        && cases.len() == engines * 2
        && cases.iter().all(|case| case["passed"] == true)
        && cancellations.len() == engines
        && cancellations.iter().all(|case| case["cancelled"] == true);
    let report = serde_json::json!({"passed":passed,"source_digest":digest,"source_unchanged":source_unchanged,"cases":cases,"cancellations":cancellations,"scope":"Native WB IPC and final decode; no camera colour calibration certification"});
    std::fs::create_dir_all(root.join("reports"))?;
    std::fs::write(
        root.join("reports/native-wb.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(
        passed,
        "Verifica WB RAW fallita o incompleta: vedere reports/native-wb.json"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_or_incomplete_campaigns_save_a_failed_report_and_return_error() {
        let root = tempfile::tempdir().unwrap();
        let cases: Vec<_> = RawEngine::choices().flat_map(|engine| {
            ["Auto", "Patch"].map(|analysis| serde_json::json!({"engine":engine,"analysis":analysis,"passed":true}))
        }).collect();
        let cancellations: Vec<_> = RawEngine::choices()
            .map(|engine| serde_json::json!({"engine":engine,"cancelled":true}))
            .collect();
        let read = || -> serde_json::Value {
            serde_json::from_slice(
                &std::fs::read(root.path().join("reports/native-wb.json")).unwrap(),
            )
            .unwrap()
        };
        write_report(
            root.path(),
            "test",
            true,
            cases.clone(),
            cancellations.clone(),
        )
        .unwrap();
        assert_eq!(read()["passed"], true);
        let mut failed = cases.clone();
        failed[0]["passed"] = false.into();
        assert!(write_report(root.path(), "test", true, failed, cancellations.clone()).is_err());
        assert_eq!(read()["cases"][0]["passed"], false);
        assert_eq!(read()["passed"], false);
        assert!(
            write_report(
                root.path(),
                "test",
                true,
                cases[1..].to_vec(),
                cancellations.clone()
            )
            .is_err()
        );
        assert_eq!(read()["passed"], false);
        assert!(write_report(root.path(), "test", true, cases.clone(), vec![]).is_err());
        assert_eq!(read()["passed"], false);
        assert!(write_report(root.path(), "test", false, cases, cancellations).is_err());
        assert_eq!(read()["source_unchanged"], false);
        assert_eq!(read()["passed"], false);
    }
}
