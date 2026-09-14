//! Sequential full-resolution development, using the actual confined broker.
//! Private crops/reports stay under var; originals are only opened for reading.
use anyhow::{Context, Result, ensure};
use std::{
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tr_core::{color, decoder::RawEngine};

pub fn run(root: &Path, worker: &Path, folder: &Path, limit: usize) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Serve il decoder isolato della piattaforma"
    );
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let output = root.join(format!("var/raw-engine-evaluation-{stamp}"));
    std::fs::create_dir_all(&output)?;
    let mut paths: Vec<_> = std::fs::read_dir(folder)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nef") || e.eq_ignore_ascii_case("dng"))
        })
        .collect();
    paths.sort();
    if limit > 0 {
        paths.truncate(limit);
    }
    ensure!(!paths.is_empty(), "Nessun NEF/DNG da verificare");
    let mut broker = tr_platform::Broker::new(worker.into());
    let mut cases = vec![];
    let mut all_passed = true;
    for (index, path) in paths.iter().enumerate() {
        let digest = tr_platform::snapshot(path)?.1;
        for engine in RawEngine::choices() {
            eprintln!("RAW {}/{} · {}", index + 1, paths.len(), engine.label());
            broker.set_raw_engine(engine);
            let started = Instant::now();
            let result = (|| -> Result<serde_json::Value> {
                let decoded = broker.decode(path, &digest, 0)?;
                ensure!(
                    decoded.info.format == "RAW",
                    "Il motore ha consegnato un'anteprima incorporata"
                );
                ensure!(
                    decoded.info.decoder.contains(engine.recipe()) || engine == RawEngine::Apple,
                    "Ricetta non corrispondente"
                );
                let raster = &decoded.raster;
                let (mut minimum, mut maximum) = (f32::INFINITY, f32::NEG_INFINITY);
                let (mut below, mut above) = (0u64, 0u64);
                for pixel in &raster.pixels {
                    for value in &pixel[..3] {
                        minimum = minimum.min(*value);
                        maximum = maximum.max(*value);
                        below += u64::from(*value < 0.);
                        above += u64::from(*value > 1.);
                    }
                }
                ensure!(maximum > minimum, "Raster costante");
                // Only the first file produces private visual artifacts. Each
                // crop is 1:1; no claimed alignment between different crops/OS.
                if index == 0 {
                    for (region, fx, fy) in [
                        ("center", 0.5, 0.5),
                        ("upper-left", 0.25, 0.25),
                        ("lower-right", 0.75, 0.75),
                    ] {
                        let edge = 512.min(raster.width).min(raster.height);
                        let x0 = ((raster.width - edge) as f32 * fx) as u32;
                        let y0 = ((raster.height - edge) as f32 * fy) as u32;
                        let mut crop = image::RgbaImage::new(edge, edge);
                        for y in 0..edge {
                            for x in 0..edge {
                                let p = raster.pixels[((y0 + y) * raster.width + x0 + x) as usize];
                                crop.put_pixel(x, y, image::Rgba(color::display_pixel(p, 0.)));
                            }
                        }
                        crop.save(output.join(format!("{engine:?}-{region}.png")))?;
                    }
                }
                Ok(
                    serde_json::json!({"passed":true,"info":decoded.info,"seconds":started.elapsed().as_secs_f64(),"working_min":minimum,"working_max":maximum,"channels_below_zero":below,"channels_above_one":above}),
                )
            })();
            let value = match result {
                Ok(v) => v,
                Err(e) => {
                    all_passed = false;
                    serde_json::json!({"passed":false,"error":format!("{e:#}")})
                }
            };
            cases.push(serde_json::json!({"source_index":index,"source_sha256":digest,"engine":engine,"result":value}));
            std::fs::write(
                output.join("report.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"passed":false,"complete":false,"cases":cases}),
                )?,
            )?;
        }
        ensure!(
            tr_platform::snapshot(path)?.1 == digest,
            "Originale cambiato durante la prova"
        );
    }
    let report = serde_json::json!({"passed":all_passed,"complete":true,"platform":std::env::consts::OS,"files":paths.len(),"originals_unchanged":true,"cases":cases,"scope":"Full-resolution functional checks through the confined worker. Sequential uncached develops; OS cache not flushed. Private 1:1 SDR crops of the first file. Numerical range is observed, not a quality verdict. No measured camera colour accuracy, Apple parity on Windows, p95 or total process-memory qualification."});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Report privato: {}", output.join("report.json").display());
    ensure!(
        all_passed,
        "Uno o più motori hanno fallito; consultare il report"
    );
    output.to_str().context("Percorso report non valido")?;
    Ok(())
}
/// The settings dialog covers the left side. Verify the visible right quarter
/// of the actual surface against the independent CPU presentation raster.
pub fn screenshots(root: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::ensure;
    let root = root.join("var");
    std::fs::write(
        root.join("raw-engine-ui-pixels.json"),
        br#"{"passed":false,"reason":"RAW surface verification started; incomplete"}"#,
    )?;
    let ui: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("raw-engine-ui.json"))?)?;
    let stages = ui["stages"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing UI stages"))?;
    ensure!(
        ui["passed"] == true && (4..=5).contains(&stages.len()),
        "UI smoke incomplete"
    );
    let mut checks = vec![];
    for (stage, result) in stages.iter().enumerate() {
        let name = format!("raw-engine-ui-{}", stage + 1);
        let records: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(root.join(format!("{name}-sampling.json")))?)?;
        ensure!(records.len() == 1, "Missing main viewport record: {name}");
        let record = &records[0];
        let rect = |field: &str| -> anyhow::Result<[u32; 4]> {
            let mut values = [0; 4];
            for (i, v) in values.iter_mut().enumerate() {
                let n = record[field][i]
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("Missing rectangle"))?;
                ensure!(n.is_finite() && n >= 0., "Invalid viewport");
                if field == "rect_physical" {
                    ensure!((n - n.round()).abs() < 0.001, "Unaligned viewport");
                    *v = n.round() as u32;
                } else {
                    *v = if i < 2 { n.ceil() } else { n.floor() } as u32;
                }
            }
            Ok(values)
        };
        let [x0, y0, x1, y1] = rect("rect_physical")?;
        let [cx0, cy0, cx1, cy1] = rect("clip_physical")?;
        let screen = image::open(root.join(format!("{name}.png")))?.to_rgba8();
        let expected = image::open(root.join(format!("{name}-expected.png")))?.to_rgba8();
        ensure!(
            x1 > x0 && y1 > y0 && expected.dimensions() == (x1 - x0, y1 - y0),
            "Viewport/reference dimensions differ"
        );
        let left = (x0 + (x1 - x0) * 3 / 4).max(cx0);
        let top = y0.max(cy0) + 4;
        let right = x1.min(cx1).min(screen.width()).saturating_sub(4);
        let bottom = y1.min(cy1).min(screen.height()).saturating_sub(4);
        ensure!(
            right > left + 100 && bottom > top + 100,
            "Visible viewport too small"
        );
        let mut maximum = 0;
        for y in top..bottom {
            for x in left..right {
                for c in 0..3 {
                    maximum = maximum.max(
                        screen.get_pixel(x, y)[c].abs_diff(expected.get_pixel(x - x0, y - y0)[c]),
                    );
                }
            }
        }
        checks.push(serde_json::json!({"stage":stage+1,"engine":result["engine"],"compute":record["compute"],"checked_pixels":(right-left)*(bottom-top),"max_channel_error_u8":maximum,"passed":maximum<=1}));
    }
    let passed = checks.iter().all(|c| c["passed"] == true);
    let report = serde_json::json!({"passed":passed,"checks":checks,"threshold_u8":1,"scope":"Right quarter of native RAW viewport vs CPU raster; excludes settings overlay, compositor and physical display."});
    std::fs::write(
        root.join("raw-engine-ui-pixels.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("RAW viewport pixels: passed={passed}");
    ensure!(passed, "RAW viewport differs from the expected CPU raster");
    Ok(())
}
