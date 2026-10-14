//! Explicit two-renderer diagnostic, with decode-only timing and optional
//! generated linear truth. All image artifacts remain in the private root.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path, time::Instant};
use tr_core::{color::LinearImage, decoder::RawEngine};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    source: std::path::PathBuf,
    sha256: String,
    camera: String,
    truth: Option<std::path::PathBuf>,
    truth_sha256: Option<String>,
    patches: Option<Vec<[usize; 4]>>,
    repetitions: usize,
}

impl Source {
    fn validate(&self) -> Result<()> {
        ensure!(
            [1, 30].contains(&self.repetitions),
            "one quality decode or thirty timed repetitions required"
        );
        ensure!(
            self.truth.is_some() == self.truth_sha256.is_some()
                && self.truth.is_some() == self.patches.is_some(),
            "truth, truth_sha256 and patches must be supplied together"
        );
        if let Some(patches) = &self.patches {
            let unique: std::collections::HashSet<_> = patches.iter().collect();
            ensure!(
                !patches.is_empty() && unique.len() == patches.len(),
                "truth patches must be nonempty and distinct"
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    sources: Vec<Source>,
}

fn digest(image: &LinearImage) -> String {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    for pixels in image.pixels.chunks(1024) {
        for (pixel, bytes) in pixels.iter().zip(buffer.as_chunks_mut::<16>().0) {
            for (v, b) in pixel.iter().zip(bytes.as_chunks_mut::<4>().0) {
                b.copy_from_slice(&v.to_le_bytes());
            }
        }
        hash.update(&buffer[..pixels.len() * 16]);
    }
    format!("{:x}", hash.finalize())
}

fn metrics(
    image: &LinearImage,
    truth: &[[f32; 3]],
    patches: &[[usize; 4]],
) -> Result<serde_json::Value> {
    let (w, h) = (image.width as usize, image.height as usize);
    ensure!(truth.len() == w * h, "truth geometry mismatch");
    let mut rows = vec![];
    for &[x, y, width, height] in patches {
        ensure!(
            width > 0
                && height > 0
                && x.checked_add(width).is_some_and(|end| end < w)
                && y.checked_add(height).is_some_and(|end| end < h),
            "truth patch bounds"
        );
        let (mut actual, mut reference, mut mse, mut maximum, mut gradient_mse) =
            ([0.; 3], [0.; 3], [0.; 3], [0f64; 3], [0.; 3]);
        let count = (width * height) as f64;
        let (mut chroma_sse, mut chroma_max, mut gradient_max) = (0f64, 0f64, 0f64);
        let mut chroma_pixels = 0usize;
        for yy in y..y + height {
            for xx in x..x + width {
                let i = yy * w + xx;
                let sa: f64 = image.pixels[i][..3].iter().map(|&v| f64::from(v)).sum();
                let sb: f64 = truth[i].iter().map(|&v| f64::from(v)).sum();
                let chroma_valid = sa > 0.01 && sb > 0.01;
                chroma_pixels += usize::from(chroma_valid);
                for c in 0..3 {
                    let a = f64::from(image.pixels[i][c]);
                    let b = f64::from(truth[i][c]);
                    actual[c] += a / count;
                    reference[c] += b / count;
                    mse[c] += (a - b).powi(2) / count;
                    maximum[c] = maximum[c].max((a - b).abs());
                    if chroma_valid {
                        let error = a / sa - b / sb;
                        chroma_sse += error * error;
                        chroma_max = chroma_max.max(error.abs());
                    }
                    for j in [i + 1, i + w] {
                        let da = f64::from(image.pixels[j][c]) - a;
                        let db = f64::from(truth[j][c]) - b;
                        gradient_mse[c] += (da - db).powi(2) / (2. * count);
                        gradient_max = gradient_max.max((da - db).abs());
                    }
                }
            }
        }
        rows.push(
            json!({"rect":[x,y,width,height],"actual":actual,"reference":reference,
            "rgb_rmse":mse.map(f64::sqrt),"maximum_error":maximum,
            "gradient_rmse":gradient_mse.map(f64::sqrt),
            "gradient_max_abs":gradient_max,"chroma_pixels":chroma_pixels,
            "chroma_rmse":(chroma_pixels>0).then(||(chroma_sse/(3*chroma_pixels) as f64).sqrt()),
            "chroma_max_abs":(chroma_pixels>0).then_some(chroma_max)}),
        );
    }
    Ok(json!(rows))
}

fn common_chroma(
    a: &[[f32; 4]],
    b: &[[f32; 4]],
    truth: &[[f32; 3]],
    width: usize,
    patches: &[[usize; 4]],
) -> serde_json::Value {
    json!(
        patches
            .iter()
            .map(|&rect| {
                let mut sse = [0f64; 2];
                let mut maximum = [0f64; 2];
                let mut count = 0usize;
                let mut excluded = [0usize; 3];
                let [x, y, w, h] = rect;
                for yy in y..y + h {
                    for xx in x..x + w {
                        let i = yy * width + xx;
                        let p = [&a[i][..3], &b[i][..3], &truth[i][..]];
                        let sums = p.map(|v| v.iter().map(|&f| f64::from(f)).sum::<f64>());
                        for k in 0..3 {
                            excluded[k] += usize::from(sums[k] <= 0.01);
                        }
                        if sums.iter().any(|&v| v <= 0.01) {
                            continue;
                        }
                        count += 1;
                        for k in 0..2 {
                            for (&measured, &reference) in p[k].iter().zip(p[2]) {
                                let error =
                                    f64::from(measured) / sums[k] - f64::from(reference) / sums[2];
                                sse[k] += error * error;
                                maximum[k] = maximum[k].max(error.abs());
                            }
                        }
                    }
                }
                json!({"rect":rect,"pixels":count,"excluded_experimental_legacy_truth":excluded,
        "experimental_legacy_rmse":sse.map(|v|(count>0).then(||(v/(3*count) as f64).sqrt())),
        "experimental_legacy_max_abs":maximum.map(|v|(count>0).then_some(v))})
            })
            .collect::<Vec<_>>()
    )
}

pub fn run(root: &Path, worker: &Path, manifest: &Path) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "comparison requires OS isolation"
    );
    let input = fs::read(manifest)?;
    let manifest: Manifest = serde_json::from_slice(&input)?;
    ensure!(
        manifest.schema == 1 && (1..=128).contains(&manifest.sources.len()),
        "invalid campaign"
    );
    let output = root.join("renderer-duel");
    fs::create_dir(&output)?;
    let mut brokers = [
        tr_platform::Broker::with_slot(worker.into(), 0),
        tr_platform::Broker::with_slot(worker.into(), 1),
    ];
    let engines = [RawEngine::TrueRendererExperimental, RawEngine::TrueRenderer];
    let mut rows = vec![];
    let mut ids = std::collections::HashSet::new();
    for (index, source) in manifest.sources.iter().enumerate() {
        ensure!(
            !source.id.is_empty()
                && source
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && ids.insert(&source.id),
            "invalid id"
        );
        source.validate()?;
        ensure!(
            tr_platform::snapshot(&source.source)?.1 == source.sha256,
            "source hash"
        );
        let truth = if let Some(path) = &source.truth {
            ensure!(fs::metadata(path)?.len() <= 16 * 1024 * 1024, "truth size");
            let bytes = fs::read(path)?;
            ensure!(
                source.truth_sha256.as_deref() == Some(&format!("{:x}", Sha256::digest(&bytes))),
                "truth hash"
            );
            let (triples, remainder) = bytes.as_chunks::<12>();
            ensure!(remainder.is_empty(), "truth byte length");
            let rgb: Vec<_> = triples
                .iter()
                .map(|p| {
                    std::array::from_fn(|c| {
                        f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap())
                    })
                })
                .collect();
            ensure!(
                rgb.iter().flatten().all(|v| v.is_finite()),
                "nonfinite truth"
            );
            Some(rgb)
        } else {
            None
        };
        let warmups = if source.repetitions == 30 { 3 } else { 0 };
        let mut times: [Vec<f64>; 2] = std::array::from_fn(|_| vec![]);
        let mut hashes: [Option<String>; 2] = [None, None];
        let mut records = vec![json!(null); 2];
        let mut attempts = vec![];
        let mut geometry = [None; 2];
        // Only known-model inputs retain images; truth is bounded to 16 MiB.
        // Keep identical usable pixels for normalized chroma comparisons.
        let mut comparison_images: [Option<Vec<[f32; 4]>>; 2] = [None, None];
        for iteration in 0..warmups + source.repetitions {
            for method in if (index + iteration) % 2 == 0 {
                [0, 1]
            } else {
                [1, 0]
            } {
                let broker = &mut brokers[index % 2];
                broker.set_raw_engine(engines[method]);
                broker.set_raw_white_balance(Default::default());
                let started = Instant::now();
                let decoded = broker.decode(&source.source, &source.sha256, 0)?;
                let seconds = started.elapsed().as_secs_f64();
                ensure!(
                    decoded.info.format == "RAW"
                        && decoded.info.decoder.contains(engines[method].recipe()),
                    "wrong renderer"
                );
                let size = (decoded.raster.width, decoded.raster.height);
                if let Some(expected) = geometry[method] {
                    ensure!(size == expected, "nonrepeatable output geometry");
                } else {
                    geometry[method] = Some(size);
                }
                if truth.is_some()
                    && let Some(other) = geometry[1 - method]
                {
                    ensure!(size == other, "quality comparison geometries differ");
                }
                let hash = digest(&decoded.raster);
                if let Some(expected) = &hashes[method] {
                    ensure!(expected == &hash, "nonrepeatable render");
                } else {
                    hashes[method] = Some(hash.clone());
                    let comparison = truth
                        .as_ref()
                        .map(|t| {
                            metrics(
                                &decoded.raster,
                                t,
                                source
                                    .patches
                                    .as_deref()
                                    .context("truth patches required")?,
                            )
                        })
                        .transpose()?;
                    records[method] = json!({"engine":engines[method],"recipe":engines[method].recipe(),"size":[decoded.raster.width,decoded.raster.height],"info":decoded.info,"sha256":hash,"truth_metrics":comparison});
                    let overview = decoded.raster.reduced(640);
                    image::save_buffer(
                        output.join(format!("{}-{}.png", source.id, method)),
                        &overview.to_display(),
                        overview.width,
                        overview.height,
                        image::ColorType::Rgba8,
                    )?;
                    if truth.is_some() {
                        comparison_images[method] = Some(decoded.raster.pixels);
                    }
                }
                attempts.push(json!({"iteration":iteration,"warmup":iteration < warmups,"method":method,"seconds":seconds,"worker_pid":decoded.worker_pid,"slot":index%2}));
                if iteration >= warmups {
                    times[method].push(seconds);
                }
            }
        }
        for method in 0..2 {
            times[method].sort_by(f64::total_cmp);
            let n = times[method].len();
            records[method]["timing"] = json!({"count":n,"median_seconds":(times[method][(n-1)/2]+times[method][n/2])*0.5,"p95_seconds":times[method][(n*95).div_ceil(100)-1],"sorted_seconds":times[method]});
        }
        let common = if let (Some(a), Some(b), Some(t), Some(patches)) = (
            comparison_images[0].as_deref(),
            comparison_images[1].as_deref(),
            truth.as_deref(),
            source.patches.as_deref(),
        ) {
            Some(common_chroma(
                a,
                b,
                t,
                geometry[0].unwrap().0 as usize,
                patches,
            ))
        } else {
            None
        };
        ensure!(
            tr_platform::snapshot(&source.source)?.1 == source.sha256,
            "source changed"
        );
        let result = json!({"id":source.id,"camera":source.camera,"source_sha256":source.sha256,"truth_sha256":source.truth_sha256,"warmups":warmups,"engines":records,"attempts":attempts,"original_unchanged":true,"common_chroma":common,
            "equal_output_geometry":geometry[0] == geometry[1],
            "comparison_scope":if truth.is_some() {"Known-model quality at identical output geometry"} else {"Native complete pipelines; compare output sizes before interpreting timing differences"}});
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(format!("{}.json", source.id)))?
            .write_all(&serde_json::to_vec_pretty(&result)?)?;
        rows.push(result);
        eprintln!(
            "Renderer duel {}/{} complete",
            index + 1,
            manifest.sources.len()
        );
    }
    let report = json!({"protocol":"TR-renderer-duel-v1","complete":true,"passed":true,"manifest_sha256":format!("{:x}",Sha256::digest(input)),"sources":rows,"broker_statistics":brokers.iter().map(|b|b.statistics()).collect::<Vec<_>>(),"limits":"Decode-only timing includes broker input hash, XPC and transfer, excludes output hash, image analysis and disk reports. Warm OS caches; no whole-machine RSS, cold-start or physical camera accuracy claim."});
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("report.json"))?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_or_duplicate_truth_cannot_be_reported_as_a_quality_comparison() {
        let complete = json!({"id":"case", "source":"input.dng", "sha256":"input",
            "camera":"Generated", "truth":"truth.f32", "truth_sha256":"reference",
            "patches":[[1,1,2,2]], "repetitions":1});
        for field in ["truth", "truth_sha256", "patches"] {
            let mut partial = complete.clone();
            partial.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<Source>(partial)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for patches in [json!([]), json!([[1, 1, 2, 2], [1, 1, 2, 2]])] {
            let mut invalid = complete.clone();
            invalid["patches"] = patches;
            assert!(
                serde_json::from_value::<Source>(invalid)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        serde_json::from_value::<Source>(complete)
            .unwrap()
            .validate()
            .unwrap();
    }

    #[test]
    fn isolated_colour_error_is_reported_and_black_is_not_normalized() {
        let truth = vec![[0.25; 3]; 64];
        let mut image = LinearImage {
            width: 8,
            height: 8,
            pixels: vec![[0.25, 0.25, 0.25, 1.]; 64],
        };
        image.pixels[3 * 8 + 3][0] = 0.75;
        let row = metrics(&image, &truth, &[[0, 0, 7, 7]]).unwrap();
        assert!((row[0]["chroma_max_abs"].as_f64().unwrap() - (0.6 - 1. / 3.)).abs() < 1e-12);
        assert!(row[0]["chroma_rmse"].as_f64().unwrap() < 0.04);
        assert_eq!(row[0]["gradient_max_abs"], json!(0.5));
        let other = vec![[0.25, 0.25, 0.25, 1.]; 64];
        image.pixels[3 * 8 + 3] = [0.; 4];
        let common = common_chroma(&image.pixels, &other, &truth, 8, &[[0, 0, 7, 7]]);
        assert_eq!(common[0]["pixels"], json!(48));
        assert_eq!(
            common[0]["excluded_experimental_legacy_truth"],
            json!([1, 0, 0])
        );
        assert_eq!(common[0]["experimental_legacy_max_abs"], json!([0., 0.]));
        image.pixels.fill([0.; 4]);
        let row = metrics(&image, &vec![[0.; 3]; 64], &[[0, 0, 7, 7]]).unwrap();
        assert_eq!(row[0]["chroma_pixels"], json!(0));
        assert!(row[0]["chroma_rmse"].is_null());
        assert!(row[0]["chroma_max_abs"].is_null());
    }

    #[test]
    fn constant_bias_and_gradient_errors_are_separate() {
        let truth: Vec<_> = (0..16).map(|i| [i as f32; 3]).collect();
        let image = LinearImage {
            width: 4,
            height: 4,
            pixels: truth
                .iter()
                .map(|p| [p[0] + 0.25, p[1] - 0.5, p[2] + 1., 1.])
                .collect(),
        };
        let result = metrics(&image, &truth, &[[0, 0, 3, 3]]).unwrap();
        assert_eq!(result[0]["rgb_rmse"], json!([0.25, 0.5, 1.]));
        assert_eq!(result[0]["gradient_rmse"], json!([0., 0., 0.]));
        assert!(metrics(&image, &truth, &[[usize::MAX, 0, 2, 2]]).is_err());
        assert!(metrics(&image, &truth, &[[0, 0, 4, 4]]).is_err());
    }
}
