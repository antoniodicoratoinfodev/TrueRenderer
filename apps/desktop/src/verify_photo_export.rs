//! Opt-in numerical parity probe. Photographs and exports stay in an isolated var/.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::path::Path;
use tr_core::{
    color::{self, LinearImage},
    decoder::RawEngine,
    editing::EditRecipe,
    export::{Format, MAX_ENCODED, Options},
    preview::PreviewRequest,
    provider::{ImageLevels, reduce_reference_mip},
    resample::Region,
};
use tr_platform::Broker;

struct Difference {
    histogram: [u64; 256],
}
impl Default for Difference {
    fn default() -> Self {
        Self {
            histogram: [0; 256],
        }
    }
}
impl Difference {
    fn sample(&mut self, a: [u8; 4], b: [u8; 4]) {
        for c in 0..3 {
            self.histogram[a[c].abs_diff(b[c]) as usize] += 1;
        }
    }
    fn report(&self) -> Value {
        let count: u64 = self.histogram.iter().sum();
        let maximum = self.histogram.iter().rposition(|n| *n > 0).unwrap_or(0);
        let sum: u64 = self
            .histogram
            .iter()
            .enumerate()
            .map(|(i, n)| i as u64 * n)
            .sum();
        let mut cumulative = 0;
        let p99 = self
            .histogram
            .iter()
            .position(|n| {
                cumulative += n;
                cumulative as f64 >= count as f64 * 0.99
            })
            .unwrap_or(0);
        json!({"rgb_channels":count,"maximum_error_u8":maximum,"mean_absolute_error_u8":sum as f64/count.max(1) as f64,
            "p99_error_u8":p99,"differing_channels":count-self.histogram[0],"channels_over_one":count-self.histogram[0]-self.histogram[1],
            "within_one_code":maximum<=1})
    }
}
fn compare(a: &LinearImage, b: &LinearImage) -> Result<Value> {
    ensure!(
        (a.width, a.height) == (b.width, b.height),
        "Presentation dimensions differ"
    );
    let mut difference = Difference::default();
    for (a, b) in a.pixels.iter().zip(&b.pixels) {
        difference.sample(
            color::display_pixel(*a, 119. / 255.),
            color::display_pixel(*b, 119. / 255.),
        );
    }
    Ok(difference.report())
}
fn saved_view(folder: &Path, name: &str, image: &LinearImage) -> Result<()> {
    image::save_buffer(
        folder.join(name),
        &image.to_display(),
        image.width,
        image.height,
        image::ColorType::Rgba8,
    )?;
    Ok(())
}
fn case(
    folder: &Path,
    worker: &Path,
    source: &Path,
    digest: &str,
    engine: RawEngine,
    edited: bool,
    format: Format,
) -> Result<Value> {
    let mut broker = Broker::with_slot(worker.into(), 0);
    broker.set_raw_engine(engine);
    let mut recipe = EditRecipe::neutral(engine);
    if edited {
        recipe.exposure_ev = 0.75;
        recipe.contrast = 12.;
        recipe.highlights = -20.;
        recipe.shadows = 15.;
        recipe.temperature = 25.;
        recipe.tint = -8.;
        recipe.saturation = 10.;
    }
    let name = format!("{engine:?}-{}", if edited { "edited" } else { "neutral" });
    println!("{name}: export");
    let snapshot = broker.prepare_snapshot(source, digest, || false)?;
    let (export_info, bytes) = broker.export_edited_snapshot_bounded(
        snapshot,
        Options {
            format,
            ..Options::default()
        },
        Some(recipe.clone()),
        MAX_ENCODED,
        || false,
    )?;
    let path = crate::photo_export::publish(folder, &name, format, &bytes, || false)?;
    // The independent Rust reader checks integer codes in our newly generated PNG/TIFF16.
    // The production isolated bitmap decoder is checked separately below.
    let encoded = image::load_from_memory(&bytes)?.to_rgba16();
    drop(bytes);
    println!("{name}: native development");
    let decoded = broker.decode(source, digest, 0)?;
    let provenance = decoded.info;
    let mut native = decoded.raster;
    let size = [native.width, native.height];
    ensure!(
        size == [encoded.width(), encoded.height()],
        "Native/export dimensions differ"
    );
    ensure!(
        native.pixels.iter().all(|p| p[3] == 1.),
        "This RAW probe requires opaque samples"
    );
    // Reproduce both viewer paths: rapid edit on a reduced mip and native verification.
    let (mut quick, base, opaque) = reduce_reference_mip(native.clone(), 1024)?;
    recipe.apply(&mut quick)?;
    let quick = ImageLevels::from_reference_mip(quick, size, base, opaque)?;
    recipe.apply(&mut native)?;
    let mut unequal = 0u64;
    let mut max_error = 0u16;
    for (working, png) in native.pixels.iter().zip(encoded.pixels()) {
        let rgb = color::rec2020_to_linear_srgb([working[0], working[1], working[2]])
            .map(color::linear_to_srgb);
        let expected =
            [rgb[0], rgb[1], rgb[2], 1.].map(|v| (v.clamp(0., 1.) * 65535.).round() as u16);
        for (actual, expected) in png.0.iter().zip(expected) {
            let d = actual.abs_diff(expected);
            unequal += u64::from(d != 0);
            max_error = max_error.max(d);
        }
    }
    drop(encoded);
    drop(broker);
    println!("{name}: reopen {format:?}");
    let (_, output_digest) = tr_platform::snapshot(&path)?;
    let mut reader = Broker::with_slot(worker.into(), 1);
    let readback = reader.decode(&path, &output_digest, 0)?;
    let readback_info = readback.info;
    let reopened = readback.raster;
    let full = compare(&native, &reopened)?;
    let mut projected = native.clone();
    tr_core::export::proof_srgb16(&mut projected);
    let proof_full = compare(&projected, &reopened)?;
    let proof = ImageLevels::from_source(projected, PreviewRequest::full())?;
    let reference = ImageLevels::from_source(native, PreviewRequest::full())?;
    let output = ImageLevels::from_source(reopened, PreviewRequest::full())?;
    let mut fits = vec![];
    for edge in [1400, 600] {
        let scale = edge as f64 / size[0].max(size[1]) as f64;
        let target = size.map(|s| (s as f64 * scale).round().max(1.) as u32);
        let region = Region::fitted(size, target);
        let a = reference.render(region)?;
        let b = output.render(region)?;
        let rapid = quick.render(region)?;
        let projected = proof.render(region)?;
        let proof_check = compare(&projected, &b)?;
        let check = compare(&a, &b)?;
        let quick_check = compare(&rapid, &b)?;
        saved_view(folder, &format!("{name}-native-fit-{edge}.png"), &a)?;
        saved_view(folder, &format!("{name}-export-fit-{edge}.png"), &b)?;
        saved_view(folder, &format!("{name}-proof-fit-{edge}.png"), &projected)?;
        fits.push(json!({"size":target,"native_vs_reopened_export":check,"provisional_vs_reopened_export":if edited {Some(quick_check)} else {None},"output_proof_vs_reopened_export":proof_check}));
    }
    Ok(
        json!({"engine":engine,"edited":edited,"recipe":recipe,"dimensions":size,"native_provenance":provenance,
        "export":export_info,"file":path.file_name().unwrap().to_string_lossy(),"readback_provenance":readback_info,
        "native_vs_encoded16":{"exact":unequal==0,"differing_rgba_channels":unequal,"maximum_error_u16":max_error,"rgba_channels":size[0] as u64*size[1] as u64*4},
        "native_vs_reopened_export_1to1":full,"output_proof_vs_reopened_export_1to1":proof_full,"fitted_views":fits}),
    )
}
pub fn run(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Requires isolated production decoder bundle"
    );
    std::fs::create_dir_all(root.join("var"))?;
    let folder = tempfile::Builder::new()
        .prefix("photo-export-parity-")
        .tempdir_in(root.join("var"))?
        .keep();
    let (_, digest) = tr_platform::snapshot(source)?;
    let mut checks = vec![];
    let format = if std::env::args().any(|arg| arg == "--parity-tiff16") {
        Format::Tiff16
    } else {
        Format::Png16
    };
    for engine in RawEngine::choices() {
        for edited in [false, true] {
            let result = case(&folder, worker, source, &digest, engine, edited, format);
            let row = match result {
                Ok(row) => row,
                Err(e) => json!({"engine":engine,"edited":edited,"error":format!("{e:#}")}),
            };
            println!(
                "{}",
                json!({"engine":engine,"edited":edited,"native_vs_encoded16":row["native_vs_encoded16"],"readback_1to1":row["native_vs_reopened_export_1to1"],"error":row["error"]})
            );
            checks.push(row);
            std::fs::write(
                folder.join("report.json"),
                serde_json::to_vec_pretty(&json!({"completed":false,"checks":checks}))?,
            )?;
        }
    }
    let (_, after) = tr_platform::snapshot(source)?;
    let completed = checks.iter().all(|c| c.get("error").is_none());
    let exact = completed
        && checks
            .iter()
            .all(|c| c["native_vs_encoded16"]["exact"] == true);
    let proof_passed = completed
        && checks.iter().all(|c| {
            c["output_proof_vs_reopened_export_1to1"]["within_one_code"] == true
                && c["fitted_views"].as_array().is_some_and(|fits| {
                    fits.iter()
                        .all(|f| f["output_proof_vs_reopened_export"]["within_one_code"] == true)
                })
        });
    let report = json!({"completed":completed,"source_unchanged":digest==after,"source_digest":digest,"format":format,
        "native_export_exact":exact,"output_proof_within_one_code":proof_passed,"checks":checks,
        "scope":"One authorized RAW, all available engines, neutral and explicit edited recipes. Native PNG16 or TIFF16, no resize. Every native pixel compared to decoded integer file codes, then isolated production readback through the CPU viewer pipeline at 1:1 and fit. Provisional mip edit measured separately. No OS compositor, physical display, JPEG, GPU or other cameras qualification."});
    std::fs::write(
        folder.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", folder.join("report.json").display());
    ensure!(
        completed && digest == after && exact && proof_passed,
        "Photo parity failed: inspect report"
    );
    Ok(())
}

/// Independent analytic reconstruction of this probe's generated sRGB PNGs.
/// Isolates bitmap import from clipping/quantization before pyramid construction.
pub fn projection(folder: &Path) -> Result<()> {
    let report: Value = serde_json::from_slice(&std::fs::read(folder.join("report.json"))?)?;
    ensure!(
        report["completed"] == true,
        "Complete the RAW parity probe first"
    );
    let mut rows = Vec::new();
    for case in report["checks"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing cases"))?
    {
        let filename = case["file"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing export"))?;
        ensure!(
            Path::new(filename).components().count() == 1 && filename.ends_with(".png"),
            "Expected a generated PNG filename"
        );
        let png = image::open(folder.join(filename))?.to_rgba16();
        let size = [png.width(), png.height()];
        let working = LinearImage::new(
            size[0],
            size[1],
            png.pixels()
                .map(|p| color::from_encoded_srgb(p.0.map(|v| v as f32 / 65535.)))
                .collect(),
        )?;
        drop(png);
        let pyramid = ImageLevels::from_source(
            working,
            PreviewRequest {
                edge: 1400,
                ..PreviewRequest::full()
            },
        )?;
        let stem = filename
            .strip_suffix("-export.png")
            .ok_or_else(|| anyhow::anyhow!("Unexpected probe filename"))?;
        let mut fits = Vec::new();
        for fit in case["fitted_views"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing fit checks"))?
        {
            let target = [
                fit["size"][0].as_u64().unwrap() as u32,
                fit["size"][1].as_u64().unwrap() as u32,
            ];
            let expected = pyramid.render(Region::fitted(size, target))?.to_display();
            let edge = target[0].max(target[1]);
            let actual =
                image::open(folder.join(format!("{stem}-export-fit-{edge}.png")))?.to_rgba8();
            ensure!(
                [actual.width(), actual.height()] == target,
                "Stored presentation size differs"
            );
            let mut difference = Difference::default();
            for (expected, actual) in expected.as_chunks::<4>().0.iter().zip(actual.pixels()) {
                difference.sample(*expected, actual.0);
            }
            fits.push(
                json!({"size":target,"analytic_srgb_png_vs_production_import":difference.report()}),
            );
        }
        rows.push(json!({"engine":case["engine"],"edited":case["edited"],"fits":fits}));
    }
    let result = json!({"completed":true,"checks":rows,"scope":"Reconstruct generated PNG16 through independent Rust PNG reader and analytic sRGB-to-working transform; same CPU mip/render graph, compared with saved production isolated import. Native parity from the primary report establishes the PNG's clamp and quantization. This isolates import error from the ordering of output projection and reduction. Neutral provisional-mip diagnostics in the primary report do not represent the neutral UI bypass."});
    std::fs::write(
        folder.join("projection.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{}", folder.join("projection.json").display());
    Ok(())
}
