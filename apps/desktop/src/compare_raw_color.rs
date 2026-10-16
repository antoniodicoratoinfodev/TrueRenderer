//! Opt-in differential RAW diagnostics. Full decodes use the confined broker;
//! private pixels/reports belong to a new isolated campaign directory.
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path, time::Instant};
use tr_core::{color::LinearImage, decoder::RawEngine};

mod registration;
use registration::register as registration;

const EDGE: usize = 320;
const MARGIN: usize = 32;
const RADIUS: i32 = 24;
const STRIDE: usize = 8;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    source: std::path::PathBuf,
    sha256: String,
    camera: String,
    kind: String,
    original_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    sources: Vec<Source>,
    purpose: String,
    #[serde(default)]
    exposure_ev: Vec<f32>,
}

// Exercise the actual photo graph at identical EV offsets. Preserve the
// unedited floats too: SDR PNGs alone conceal both headroom and pink plateaus.
fn exposure_series(
    output: &Path,
    stem: &str,
    image: &LinearImage,
    steps: &[f32],
) -> Result<Vec<Value>> {
    let mut rows = vec![];
    for (index, &ev) in steps.iter().enumerate() {
        let recipe = tr_core::editing::EditRecipe {
            exposure_ev: ev,
            ..tr_core::editing::EditRecipe::neutral(RawEngine::TrueRendererExperimental)
        };
        let mut edited = image.clone();
        recipe.apply(&mut edited)?;
        let factor = ev.exp2();
        ensure!(
            image.pixels.iter().zip(&edited.pixels).all(|(a, b)| {
                (0..3).all(|c| b[c].to_bits() == (a[c] * factor).to_bits()) && a[3] == b[3]
            }),
            "photo exposure differs from the declared common RGB gain"
        );
        let name = format!("{stem}-ev-{index}.png");
        image::save_buffer(
            output.join(&name),
            &edited.to_display(),
            edited.width,
            edited.height,
            image::ColorType::Rgba8,
        )?;
        rows.push(json!({"ev":ev,"gain":factor,"png":name,"linear_gain_exact":true}));
    }
    Ok(rows)
}

struct Patch {
    origin: [usize; 2],
    rgb: Vec<[f32; 3]>,
    y: Vec<f64>,
}

fn luminance(rgb: [f32; 3]) -> f64 {
    f64::from(rgb[0]) * 0.2627002120112671
        + f64::from(rgb[1]) * 0.6779980715188708
        + f64::from(rgb[2]) * 0.0593017164698620
}

fn patches(image: &LinearImage) -> Result<Vec<Patch>> {
    let (w, h) = (image.width as usize, image.height as usize);
    ensure!(
        w >= EDGE && h >= EDGE,
        "comparison needs full-resolution RAW geometry"
    );
    let mut out = vec![];
    for fy in [0.2, 0.5, 0.8] {
        for fx in [0.2, 0.5, 0.8] {
            let origin = [
                ((fx * w as f64).round() as usize)
                    .saturating_sub(EDGE / 2)
                    .min(w - EDGE),
                ((fy * h as f64).round() as usize)
                    .saturating_sub(EDGE / 2)
                    .min(h - EDGE),
            ];
            let mut rgb = Vec::with_capacity(EDGE * EDGE);
            for y in 0..EDGE {
                for x in 0..EDGE {
                    let p = image.pixels[(origin[1] + y) * w + origin[0] + x];
                    ensure!(
                        p[3] == 1. && p.iter().all(|v| v.is_finite()),
                        "invalid RAW pixel"
                    );
                    rgb.push([p[0], p[1], p[2]]);
                }
            }
            let y = rgb.iter().copied().map(luminance).collect();
            out.push(Patch { origin, rgb, y });
        }
    }
    Ok(out)
}

fn block(patch: &Patch, x0: usize, y0: usize) -> Value {
    let (mut mean, mut low, mut high) = ([0.; 3], f32::INFINITY, f32::NEG_INFINITY);
    let (mut y_mean, mut y_m2, mut n) = (0., 0., 0usize);
    for y in y0..y0 + 16 {
        for x in x0..x0 + 16 {
            let i = y * EDGE + x;
            for (sum, v) in mean.iter_mut().zip(patch.rgb[i]) {
                *sum += f64::from(v) / 256.;
                low = low.min(v);
                high = high.max(v);
            }
            n += 1;
            let delta = patch.y[i] - y_mean;
            y_mean += delta / n as f64;
            y_m2 += delta * (patch.y[i] - y_mean);
        }
    }
    json!({"rgb":mean,"minimum":low,"maximum":high,"y_mean":y_mean,"y_stddev":(y_m2 / 255.).sqrt()})
}

fn compare(a: &Patch, b: &Patch) -> Value {
    let alignment = registration(&a.y, &b.y, EDGE, RADIUS, STRIDE);
    let dx = alignment["dx"].as_i64().unwrap() as isize;
    let dy = alignment["dy"].as_i64().unwrap() as isize;
    let mut blocks = vec![];
    for y in (MARGIN..EDGE - MARGIN).step_by(16) {
        for x in (MARGIN..EDGE - MARGIN).step_by(16) {
            blocks.push(json!({"xy":[x,y],"anchor":block(a,x,y),
                "comparison":block(b,(x as isize + dx) as usize,(y as isize + dy) as usize)}));
        }
    }
    let (mut mae, mut squared) = ([0.; 3], [0.; 3]);
    let (mut gradient, mut count) = ([0.; 2], 0usize);
    for y in MARGIN..EDGE - MARGIN {
        for x in MARGIN..EDGE - MARGIN {
            let i = y * EDGE + x;
            let j = ((y as isize + dy) * EDGE as isize + x as isize + dx) as usize;
            count += 1;
            for c in 0..3 {
                let difference = f64::from(b.rgb[j][c]) - f64::from(a.rgb[i][c]);
                mae[c] += difference.abs();
                squared[c] += difference * difference;
            }
            gradient[0] +=
                (a.y[i + 1] - a.y[i - 1]).powi(2) + (a.y[i + EDGE] - a.y[i - EDGE]).powi(2);
            gradient[1] +=
                (b.y[j + 1] - b.y[j - 1]).powi(2) + (b.y[j + EDGE] - b.y[j - EDGE]).powi(2);
        }
    }
    json!({"anchor_origin":a.origin,"comparison_origin":b.origin,"registration":alignment,
        "pixels":count,"linear_rgb_mae":mae.map(|v|v / count as f64),
        "linear_rgb_rmse":squared.map(|v|(v / count as f64).sqrt()),
        "gradient_energy":gradient,"blocks":blocks})
}

fn new_json(path: &Path, value: &Value) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(&serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn expected_refusal(source: &Source, engine: RawEngine, error: &str) -> bool {
    (engine == RawEngine::TrueRenderer
        && !(source.camera == "NIKON D750" && source.kind == "original")
        && error.contains("solo Nikon D750/D40 Bayer"))
        || (source.camera == "FUJIFILM X-T5"
            && matches!(engine, RawEngine::LibRawAhd | RawEngine::LibRawBilinear)
            && error.contains("CFA non Bayer"))
}

pub fn run(root: &Path, worker: &Path, manifest: &Path) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "RAW comparison requires OS isolation"
    );
    let source_bytes = fs::read(manifest)?;
    let manifest: Manifest = serde_json::from_slice(&source_bytes)?;
    ensure!(
        manifest.schema == 1 && !manifest.sources.is_empty() && manifest.sources.len() <= 128,
        "invalid comparison manifest"
    );
    ensure!(
        manifest.exposure_ev.len() <= 9
            && manifest
                .exposure_ev
                .iter()
                .all(|v| v.is_finite() && (-10. ..=10.).contains(v)),
        "invalid exposure steps"
    );
    let mut ids = std::collections::HashSet::new();
    let mut hashes = std::collections::HashSet::new();
    for source in &manifest.sources {
        ensure!(
            !source.id.is_empty()
                && source
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "invalid source id"
        );
        ensure!(
            ids.insert(&source.id) && hashes.insert(&source.sha256),
            "duplicate comparison source"
        );
        ensure!(
            matches!(source.kind.as_str(), "original" | "repacked-control")
                && (source.kind == "repacked-control") == source.original_sha256.is_some(),
            "invalid comparison source kind"
        );
        ensure!(
            tr_platform::snapshot(&source.source)?.1 == source.sha256,
            "source hash mismatch"
        );
    }
    for source in &manifest.sources {
        if let Some(original) = &source.original_sha256 {
            ensure!(
                manifest.sources.iter().any(|s| s.sha256 == *original
                    && s.kind == "original"
                    && s.camera == source.camera),
                "unbound repack control"
            );
        }
    }
    let output = root.join("comparison");
    fs::create_dir(&output)?; // Never reuse a partial or completed campaign.
    let mut rows = vec![];
    let mut unexpected = 0usize;
    // The production broker binds the recipe to each request and recycles its
    // service after 32 jobs. Reuse those lifetimes instead of triggering the
    // OS's service-restart throttle at every engine switch.
    let mut brokers = [
        tr_platform::Broker::with_slot(worker.into(), 0),
        tr_platform::Broker::with_slot(worker.into(), 1),
    ];
    for (index, source) in manifest.sources.iter().enumerate() {
        let mut anchor: Option<Vec<Patch>> = None;
        let mut engines = vec![];
        for engine in [
            RawEngine::TrueRendererExperimental,
            RawEngine::Apple,
            RawEngine::LibRawAhd,
            RawEngine::LibRawBilinear,
            RawEngine::TrueRenderer,
        ] {
            eprintln!(
                "Color comparison {}/{} · {} · {}",
                index + 1,
                manifest.sources.len(),
                source.camera,
                engine.label()
            );
            let started = Instant::now();
            let result = (|| -> Result<Value> {
                let broker = &mut brokers[index % 2];
                broker.set_raw_engine(engine);
                broker.set_raw_white_balance(Default::default());
                let decoded = broker.decode(&source.source, &source.sha256, 0)?;
                ensure!(
                    decoded.info.format == "RAW"
                        && (engine == RawEngine::Apple
                            || decoded.info.decoder.contains(engine.recipe())),
                    "wrong RAW recipe or embedded preview"
                );
                let image = &decoded.raster;
                let regions = patches(image)?;
                let stem = format!("{}-{engine:?}", source.id);
                let reduced = image.reduced(640);
                image::save_buffer(
                    output.join(format!("{stem}-overview.png")),
                    &reduced.to_display(),
                    reduced.width,
                    reduced.height,
                    image::ColorType::Rgba8,
                )?;
                let mut file = std::io::BufWriter::new(
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(output.join(format!("{stem}-overview.f32")))?,
                );
                for pixel in &reduced.pixels {
                    for v in &pixel[..3] {
                        file.write_all(&v.to_le_bytes())?;
                    }
                }
                file.flush()?;
                let exposure_overview = exposure_series(
                    &output,
                    &format!("{stem}-overview"),
                    &reduced,
                    &manifest.exposure_ev,
                )?;
                let mut exposure_crops = vec![];
                for (i, region) in regions.iter().enumerate() {
                    let pixels: Vec<u8> = region
                        .rgb
                        .iter()
                        .flat_map(|p| tr_core::color::display_pixel([p[0], p[1], p[2], 1.], 0.))
                        .collect();
                    image::save_buffer(
                        output.join(format!("{stem}-zone-{i}.png")),
                        &pixels,
                        EDGE as u32,
                        EDGE as u32,
                        image::ColorType::Rgba8,
                    )?;
                    if !manifest.exposure_ev.is_empty() {
                        let patch = LinearImage::new(
                            EDGE as u32,
                            EDGE as u32,
                            region.rgb.iter().map(|p| [p[0], p[1], p[2], 1.]).collect(),
                        )?;
                        let mut linear = std::io::BufWriter::new(
                            fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(output.join(format!("{stem}-zone-{i}.f32")))?,
                        );
                        for p in &region.rgb {
                            for v in p {
                                linear.write_all(&v.to_le_bytes())?;
                            }
                        }
                        linear.flush()?;
                        exposure_crops.push(exposure_series(
                            &output,
                            &format!("{stem}-zone-{i}"),
                            &patch,
                            &manifest.exposure_ev,
                        )?);
                    }
                }
                let (mut low, mut high, mut below, mut above) =
                    (f32::INFINITY, f32::NEG_INFINITY, 0u64, 0u64);
                for pixel in &image.pixels {
                    for &v in &pixel[..3] {
                        low = low.min(v);
                        high = high.max(v);
                        below += u64::from(v < 0.);
                        above += u64::from(v > 1.);
                    }
                }
                let comparisons: Vec<_> = if let Some(reference) = &anchor {
                    regions
                        .iter()
                        .zip(reference)
                        .map(|(b, a)| compare(a, b))
                        .collect()
                } else {
                    vec![]
                };
                let result = json!({"engine":engine,"recipe":engine.recipe(),"accepted":true,
                    "info":decoded.info,"size":[image.width,image.height],"seconds":started.elapsed().as_secs_f64(),
                    "worker_pid":decoded.worker_pid,"slot":index%2,"range":{"minimum":low,"maximum":high,"below_zero":below,"above_one":above},
                    "overview":{"png":format!("{stem}-overview.png"),"linear":format!("{stem}-overview.f32"),"size":[reduced.width,reduced.height]},
                    "exposure_overview":exposure_overview,"exposure_crops":exposure_crops,
                    "zones":comparisons,"crop_pngs":(0..9).map(|i|format!("{stem}-zone-{i}.png")).collect::<Vec<_>>()});
                if engine == RawEngine::TrueRendererExperimental {
                    anchor = Some(regions);
                }
                Ok(result)
            })();
            engines.push(match result {
                Ok(value) => value,
                Err(error) => {
                    let error = format!("{error:#}");
                    let expected = expected_refusal(source, engine, &error);
                    unexpected += usize::from(!expected);
                    json!({"engine":engine,"accepted":false,"expected_refusal":expected,"error":error})
                }
            });
            if anchor.is_none() {
                break;
            }
        }
        ensure!(
            tr_platform::snapshot(&source.source)?.1 == source.sha256,
            "original changed during comparison"
        );
        let row = json!({"id":source.id,"camera":source.camera,"kind":source.kind,"source_sha256":source.sha256,
            "original_sha256":source.original_sha256,"original_unchanged":true,"engines":engines});
        let name = format!("{}.json", source.id);
        new_json(&output.join(&name), &row)?;
        rows.push(json!({"id":source.id,"report":name,"sha256":format!("{:x}",Sha256::digest(fs::read(output.join(&name))?))}));
    }
    new_json(
        &output.join("manifest.json"),
        &json!({"protocol":if manifest.exposure_ev.is_empty() {"TRExp-engine-color-v2"} else {"TRExp-engine-exposure-v1"},"complete":true,
        "passed":unexpected==0,"unexpected_failures":unexpected,"sources":rows,
        "input_manifest_sha256":format!("{:x}",Sha256::digest(source_bytes)),"purpose":manifest.purpose,
        "anchor":"TrueRendererExperimental","wb":"as-shot, engine-specific interpretation retained",
        "working":"linear Rec.2020 D65 f32; no additional photo graph", "edge":EDGE,"margin":MARGIN,
        "exposure_ev":manifest.exposure_ev,"exposure_method":"EditRecipe::apply; common RGB factor 2^EV, alpha retained, no normalization fit; PNGs are SDR sRGB",
        "registration_radius":RADIUS,"registration_stride":STRIDE,
        "broker_statistics":brokers.iter().map(|b|b.statistics()).collect::<Vec<_>>(),
        "limits":"Diagnostic engine differences only. Native profile, WB, baseline exposure, clipping, reconstruction and geometry can differ. No camera calibration ground truth."}),
    )?;
    ensure!(
        unexpected == 0,
        "unexpected engine failures: {unexpected}; private results retained"
    );
    println!("RAW color comparison complete: {}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_recovers_known_shift_despite_independent_fine_noise() {
        let edge = 160;
        let signal = |x: f64, y: f64| {
            0.3 * (0.11 * x + 0.03 * y).sin()
                + 0.23 * (0.13 * y - 0.02 * x).cos()
                + 0.17 * (0.05 * x - 0.07 * y).sin()
        };
        let mut noise = 0x12854376_u64;
        let mut jitter = || {
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            ((noise >> 11) as f64 / (1_u64 << 53) as f64 - 0.5) * 0.12
        };
        let a: Vec<_> = (0..edge * edge)
            .map(|i| signal((i % edge) as f64, (i / edge) as f64) + jitter())
            .collect();
        let b: Vec<_> = (0..edge * edge)
            .map(|i| {
                1.7 * (signal((i % edge) as f64 - 3., (i / edge) as f64 + 2.) + jitter()) + 0.4
            })
            .collect();
        let result = registration(&a, &b, edge, 12, 4);
        assert_eq!(result["supported"], true, "{result}");
        assert_eq!(
            (result["dx"].as_i64(), result["dy"].as_i64()),
            (Some(3), Some(-2))
        );
    }
    #[test]
    fn registration_recovers_translation_with_common_gain_and_offset() {
        let edge = 96;
        let signal = |x: f64, y: f64| {
            (0.19 * x + 0.03 * y).sin() + (0.27 * y - 0.09 * x).cos() + 0.2 * (0.015 * x * y).sin()
        };
        let a: Vec<_> = (0..edge * edge)
            .map(|i| signal((i % edge) as f64, (i / edge) as f64))
            .collect();
        let b: Vec<_> = (0..edge * edge)
            .map(|i| 2. * signal((i % edge) as f64 - 3., (i / edge) as f64 + 2.) + 0.4)
            .collect();
        let r = registration(&a, &b, edge, 6, 4);
        assert_eq!(r["dx"], 3);
        assert_eq!(r["dy"], -2);
        assert_eq!(r["supported"], true);
        assert!(r["minimum_gradient_cosine"].as_f64().unwrap() > 1. - 1e-12);
        assert_eq!(
            registration(&vec![0.; edge * edge], &b, edge, 6, 4)["supported"],
            false
        );
        let shifted: Vec<_> = (0..edge * edge)
            .map(|i| signal((i % edge) as f64 - 6., (i / edge) as f64))
            .collect();
        assert_eq!(registration(&a, &shifted, edge, 6, 4)["supported"], false);
    }
    #[test]
    fn linear_block_statistics_preserve_negative_and_extended_samples() {
        let rgb: Vec<_> = (0..EDGE * EDGE)
            .map(|i| [-0.125, 0.5, (i % 2) as f32 + 0.5])
            .collect();
        let y = rgb.iter().copied().map(luminance).collect();
        let patch = Patch {
            origin: [0, 0],
            rgb,
            y,
        };
        let b = block(&patch, 32, 32);
        assert_eq!(b["rgb"], json!([-0.125, 0.5, 1.]));
        assert_eq!(b["minimum"], -0.125);
        assert_eq!(b["maximum"], 1.5);
        let direct: Vec<_> = (32..48)
            .flat_map(|y| (32..48).map(move |x| y * EDGE + x))
            .map(|i| patch.y[i])
            .collect();
        let mean = direct.iter().sum::<f64>() / 256.;
        let variance = direct.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 255.;
        assert!((b["y_mean"].as_f64().unwrap() - mean).abs() < 1e-14);
        assert!((b["y_stddev"].as_f64().unwrap() - variance.sqrt()).abs() < 1e-14);
    }
}
