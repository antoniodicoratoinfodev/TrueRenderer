//! Explicit local qualification on generated fixtures, through the real XPC boundary.
use anyhow::{Result, ensure};
use std::{path::Path, time::Instant};
use tr_core::color::from_encoded_srgb;
use tr_platform::Broker;

pub fn run(root: &Path, worker: &Path) -> Result<()> {
    let folder = root.join("var/format-fixtures");
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json"))?)?;
    let mut broker = Broker::with_slot(worker.into(), 0);
    ensure!(
        tr_platform::external_decoding_available(worker),
        "La verifica dei formati esterni richiede il bundle XPC macOS"
    );
    let mut checks = Vec::new();
    let mut passed = true;
    for fixture in fixtures {
        let name = fixture["file"].as_str().unwrap();
        let path = folder.join(name);
        let (_, digest) = tr_platform::snapshot(&path)?;
        let start = Instant::now();
        match broker.decode(&path, &digest, 0) {
            Ok(decoded) => {
                let expected = [
                    fixture["width"].as_u64().unwrap() as u32,
                    fixture["height"].as_u64().unwrap() as u32,
                ];
                let dimensions = [decoded.raster.width, decoded.raster.height];
                let raw = name.ends_with(".dng");
                let mut error = 0.0f32;
                if !raw {
                    let orientation = fixture["orientation"].as_u64().unwrap();
                    let [w, h] = if orientation >= 5 {
                        [expected[1], expected[0]]
                    } else {
                        expected
                    };
                    for y in [dimensions[1] / 4, dimensions[1] * 3 / 4] {
                        for x in [dimensions[0] / 4, dimensions[0] * 3 / 4] {
                            let (sx, sy) = match orientation {
                                2 => (w - 1 - x, y),
                                3 => (w - 1 - x, h - 1 - y),
                                4 => (x, h - 1 - y),
                                5 => (y, x),
                                6 => (y, h - 1 - x),
                                7 => (w - 1 - y, h - 1 - x),
                                8 => (w - 1 - y, x),
                                _ => (x, y),
                            };
                            let mut rgb = [0.; 4];
                            rgb[3] = 1.;
                            if sy < h / 2 {
                                rgb[usize::from(sx >= w / 2)] = 1.;
                            } else if sx < w / 2 {
                                rgb[2] = 1.;
                            } else {
                                let v = 20000 + (sx + sy) % 1000;
                                let value = if fixture["source_bits"] == 16 {
                                    v as f32 / 65535.
                                } else {
                                    (v >> 8) as f32 / 255.
                                };
                                rgb = [value, value, value, 1.];
                            }
                            let reference = from_encoded_srgb(rgb);
                            let pixel = decoded.raster.pixels[(y * dimensions[0] + x) as usize];
                            for c in 0..4 {
                                error = error.max((reference[c] - pixel[c]).abs());
                            }
                        }
                    }
                }
                let min = decoded
                    .raster
                    .pixels
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::INFINITY, f32::min);
                let max = decoded
                    .raster
                    .pixels
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::NEG_INFINITY, f32::max);
                let accepted = dimensions == expected
                    && if raw {
                        decoded.info.format == "RAW"
                            && decoded.info.decoder.contains("full")
                            && max - min > 0.01
                    } else {
                        error <= 0.025
                    };
                passed &= accepted;
                checks.push(serde_json::json!({"file":name,"passed":accepted,"dimensions":dimensions,"expected_dimensions":expected,"info":decoded.info,"quadrant_error_linear":if raw {None} else {Some(error)},"green_range":[min,max],"seconds":start.elapsed().as_secs_f64(),"digest":digest,"transport":decoded.transport,"worker_pid":decoded.worker_pid}));
                println!(
                    "{name}: {} · {}x{} · error {error}",
                    if accepted { "passed" } else { "FAILED" },
                    dimensions[0],
                    dimensions[1]
                );
            }
            Err(error) => {
                passed = false;
                println!("{name}: FAILED · {error:#}");
                checks.push(
                    serde_json::json!({"file":name,"passed":false,"error":format!("{error:#}")}),
                );
            }
        }
    }
    let rejection_folder = root.join("var/format-rejections");
    std::fs::create_dir_all(&rejection_folder)?;
    let valid_path = folder.join("02-png.png");
    let (png, digest) = tr_platform::snapshot(&valid_path)?;
    let valid = broker.decode(&valid_path, &digest, 0)?;
    let pid = valid.worker_pid;
    let mut rejection_checks = Vec::new();
    for (name, bytes) in [
        (
            "unknown.jpg",
            b"TrueRenderer invalid image fixture".as_slice(),
        ),
        ("truncated.png", &png[..16]),
        ("truncated.tiff", b"II*\0\x08\0\0\0".as_slice()),
    ] {
        let path = rejection_folder.join(name);
        std::fs::write(&path, bytes)?;
        let (_, bad_digest) = tr_platform::snapshot(&path)?;
        let rejected = broker.decode(&path, &bad_digest, 0).is_err();
        let recovered = broker.decode(&valid_path, &digest, 0)?;
        let accepted = rejected
            && recovered.worker_pid == pid
            && recovered.raster.pixels == valid.raster.pixels;
        passed &= accepted;
        rejection_checks.push(serde_json::json!({"case":name,"passed":accepted,"rejected":rejected,"same_worker_pid":recovered.worker_pid==pid,"valid_image_recovered":recovered.raster.pixels==valid.raster.pixels}));
    }
    let renamed = rejection_folder.join("png-content.jpg");
    std::fs::write(&renamed, &png)?;
    let sniffed = broker.decode(&renamed, &digest, 0)?;
    let sniff_ok = sniffed.info.format == "PNG" && sniffed.raster.pixels == valid.raster.pixels;
    passed &= sniff_ok;
    rejection_checks.push(serde_json::json!({"case":"content signature takes precedence over extension","passed":sniff_ok}));
    let oversized = rejection_folder.join("oversized.tiff");
    std::fs::File::create(&oversized)?.set_len(tr_core::protocol::MAX_SOURCE as u64 + 1)?;
    let source_quota = tr_platform::snapshot(&oversized).is_err();
    let stale_token = broker.decode(&valid_path, "unverified:stale", 0).is_err();
    passed &= source_quota && stale_token;
    rejection_checks
        .push(serde_json::json!({"case":"source exceeds 256 MiB","passed":source_quota}));
    rejection_checks
        .push(serde_json::json!({"case":"stale source observation","passed":stale_token}));
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"passed":passed,"checks":checks,"rejection_checks":rejection_checks,"statistics":broker.statistics(),"scope":"Own generated external fixtures through actual XPC: JPEG/PNG/TIFF/GIF/BMP/HEIC/WebP, 16-bit, EXIF 1-8, 12 MP, Bayer DNG without embedded preview. No universal camera/ICC/display qualification."});
    std::fs::write(
        root.join("reports/formats-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(
        passed,
        "Alcuni formati non hanno superato la verifica: reports/formats-macos.json"
    );
    Ok(())
}
