//! Explicit-source, isolated WB diagnostics; never publishes source bytes.
use anyhow::{Result, ensure};
use std::{path::Path, time::Instant};
use tr_core::{decoder::RawEngine, raw_wb::Analysis};
use tr_platform::Broker;
pub fn run(root: &Path, binary: &Path, path: &Path) -> Result<()> {
    let (_, digest) = tr_platform::snapshot(path)?;
    let mut cases = Vec::new();
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
    ensure!(digest == after, "Sorgente modificata");
    let report = serde_json::json!({"source_digest":digest,"source_unchanged":true,"cases":cases,"scope":"Native WB IPC and final decode; no camera colour calibration certification"});
    std::fs::create_dir_all(root.join("reports"))?;
    std::fs::write(
        root.join("reports/native-wb.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
