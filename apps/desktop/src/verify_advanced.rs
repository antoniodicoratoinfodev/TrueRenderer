//! Process-3 native/export parity on an explicit, isolated qualification root.
use anyhow::{Result, ensure};
use std::path::Path;
use tr_core::{
    decoder::RawEngine,
    editing::{
        Advanced, EditRecipe,
        masks::{Mask, Shape},
    },
    export::{Format, MAX_ENCODED, Options},
};

pub fn recipe(engine: RawEngine) -> EditRecipe {
    let mut r = EditRecipe::neutral(engine);
    r.process_version = 3;
    r.exposure_ev = 0.25;
    let mut a = Advanced::default();
    a.geometry.crop = [0.12, 0.08, 0.9, 0.92];
    a.geometry.quarter_turns = 1;
    a.geometry.angle = 3.;
    a.geometry.perspective = [8., -6.];
    a.geometry.distortion = 14.;
    a.geometry.vignette = 15.;
    a.geometry.ca = [0.7, -0.5];
    a.geometry.defringe = 20.;
    a.detail.texture = 15.;
    a.detail.clarity = 12.;
    a.detail.sharpen = 35.;
    a.detail.radius = 1.4;
    a.detail.luminance_noise = 25.;
    a.detail.chroma_noise = 35.;
    a.color.bands[0].hue = 12.;
    a.color.bands[5].saturation = -15.;
    a.color.grading[0].hue = 230.;
    a.color.grading[0].amount = 8.;
    a.masks.push(Mask {
        exposure: 0.5,
        warmth: 10.,
        ..Default::default()
    });
    a.masks.push(Mask {
        shape: Shape::Brush,
        points: vec![[0.2, 0.3], [0.3, 0.4], [0.4, 0.4]],
        exposure: -0.4,
        ..Default::default()
    });
    r.advanced = Some(Box::new(a));
    r
}

pub fn run(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    std::fs::create_dir_all(root.join("reports"))?;
    let report_path = root.join("reports/advanced-editing.json");
    std::fs::write(
        &report_path,
        br#"{"passed":false,"reason":"probe incomplete"}"#,
    )?;
    let (_, digest) = tr_platform::snapshot(source)?;
    let mut cases = Vec::new();
    for engine in RawEngine::choices() {
        for slot in 0..2 {
            let mut broker = tr_platform::Broker::with_slot(worker.into(), slot);
            broker.set_raw_engine(engine);
            let snapshot = broker.prepare_snapshot(source, &digest, || false)?;
            let decoded = broker.decode(source, &digest, 0)?;
            let native_size = [decoded.raster.width, decoded.raster.height];
            for mode in 0..3 {
                let mut r = recipe(engine);
                if mode == 1 {
                    r.advanced.as_mut().unwrap().geometry = Default::default();
                    r.advanced.as_mut().unwrap().color.monochrome = true;
                } else if mode == 2 {
                    r.advanced.as_mut().unwrap().detail.dehaze = 15.;
                    r.advanced.as_mut().unwrap().geometry.crop = [0.495, 0.495, 0.51, 0.51];
                }
                let mut expected = decoded.raster.clone();
                r.apply(&mut expected)?;
                for format in [Format::Png16, Format::Tiff16] {
                    let options = Options {
                        format,
                        ..Default::default()
                    };
                    let (info, bytes) = broker.export_edited_snapshot_bounded(
                        snapshot.clone(),
                        options,
                        Some(r.clone()),
                        MAX_ENCODED,
                        || false,
                    )?;
                    let actual = image::load_from_memory(&bytes)?.to_rgba16();
                    ensure!(
                        [expected.width, expected.height] == [info.width, info.height]
                            && actual.dimensions() == (info.width, info.height),
                        "Dimensioni diverse dopo editing"
                    );
                    let mut maximum = 0_u16;
                    for (working, encoded) in expected.pixels.iter().zip(actual.pixels()) {
                        let (reference, _) = tr_core::export::srgb16(*working);
                        for (a, b) in reference.into_iter().zip(encoded.0) {
                            maximum = maximum.max(a.abs_diff(b));
                        }
                    }
                    ensure!(
                        maximum == 0,
                        "Vista/export diversi: {engine:?}, slot {slot}, {format:?}, {maximum}"
                    );
                    cases.push(serde_json::json!({"engine":engine,"slot":slot,"mode":mode,"format":format,"source_size":native_size,"output_size":[info.width,info.height],"maximum_error_u16":maximum,"recipe":r}));
                }
            }
        }
    }
    ensure!(
        tr_platform::snapshot(source)?.1 == digest,
        "Sorgente cambiata durante la prova"
    );
    let report = serde_json::json!({"passed":true,"source_digest":digest,"source_unchanged":true,"cases":cases,"scope":"Exact PNG16/TIFF16 parity against the canonical CPU graph, all available engines and both workers, explicit source. No camera colour, display, Windows or performance qualification."});
    std::fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
    println!("Advanced editing: {} cases passed", cases.len());
    Ok(())
}
