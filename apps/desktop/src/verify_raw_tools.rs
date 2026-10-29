//! Opt-in native RAW recipe/export parity. Inputs and reports stay under var/.
use anyhow::{Result, ensure};
use serde_json::json;
use std::path::Path;
use tr_core::{
    decoder::RawEngine,
    editing::{
        EditRecipe,
        retouch::{Kind, Stroke},
    },
    export::{Format, MAX_ENCODED, Options},
};
pub fn run(root: &Path, worker: &Path, source: &Path) -> Result<()> {
    ensure!(
        root.starts_with(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../var")
                .canonicalize()?
        ),
        "Use an isolated root under var/"
    );
    ensure!(
        tr_platform::external_decoding_available(worker),
        "Native RAW probe requires OS-isolated decoder bundle"
    );
    std::fs::create_dir_all(root.join("reports"))?;
    let (_, digest) = tr_platform::snapshot(source)?;
    let mut checks = vec![];
    for engine in [
        RawEngine::TrueRendererExperimental,
        RawEngine::LibRawBilinear,
    ] {
        let mut recipe = EditRecipe::neutral(engine);
        for (i, kind) in [
            Kind::Paint,
            Kind::Erase,
            Kind::Clone,
            Kind::Heal,
            Kind::AutoHeal,
        ]
        .into_iter()
        .enumerate()
        {
            recipe.retouch_mut().strokes.push(Stroke {
                enabled: true,
                kind,
                points: vec![[0.3 + i as f32 * 0.06, 0.5], [0.31 + i as f32 * 0.06, 0.51]],
                radius: 0.002,
                hardness: 0.7,
                opacity: 0.8,
                color: [0.7, 0.2, 0.1, 0.8],
                offset: [-0.02, 0.],
                selection: vec![],
            });
        }
        recipe.retouch_mut().transform.offset = [0.002, -0.001];
        recipe.exposure_ev = 0.25;
        recipe
            .layer_stack()
            .layers
            .push(tr_core::editing::layers::Layer::new(
                "Regolazione",
                tr_core::editing::layers::TOOLS[2].operator(),
            ));
        let serialized = serde_json::to_vec(&recipe)?;
        let reopened: EditRecipe = serde_json::from_slice(&serialized)?;
        reopened.validate()?;
        ensure!(recipe == reopened, "Recipe changed on reopen");
        let mut broker = tr_platform::Broker::with_slot(worker.into(), 0);
        broker.set_raw_engine(engine);
        let decoded = broker.decode(source, &digest, 0)?;
        let size = [decoded.raster.width, decoded.raster.height];
        let density = decoded.info.resolution;
        let mut expected = decoded.raster;
        reopened.apply(&mut expected)?;
        let snapshot = broker.prepare_snapshot(source, &digest, || false)?;
        let (_, encoded) = broker.export_edited_snapshot_bounded(
            snapshot,
            Options {
                format: Format::Png16,
                ..Default::default()
            },
            Some(reopened),
            MAX_ENCODED,
            || false,
        )?;
        let output = image::load_from_memory(&encoded)?.to_rgba16();
        ensure!(
            size == [output.width(), output.height()],
            "Export changed native pixel dimensions"
        );
        let mut differences = 0u64;
        let mut max_error = 0u16;
        for (p, q) in expected.pixels.iter().zip(output.pixels()) {
            let reference = tr_core::export::srgb16(*p).0;
            for (a, b) in reference.into_iter().zip(q.0) {
                let error = a.abs_diff(b);
                differences += u64::from(error != 0);
                max_error = max_error.max(error);
            }
        }
        let output_path = root.join(format!("{engine:?}-retouch.png"));
        std::fs::write(&output_path, &encoded)?;
        let mut phys = None;
        let mut at = 8;
        while at + 12 <= encoded.len() {
            let n = u32::from_be_bytes(encoded[at..at + 4].try_into().unwrap()) as usize;
            if at + 12 + n > encoded.len() {
                break;
            }
            if &encoded[at + 4..at + 8] == b"pHYs" && n == 9 {
                phys = Some([
                    u32::from_be_bytes(encoded[at + 8..at + 12].try_into().unwrap()),
                    u32::from_be_bytes(encoded[at + 12..at + 16].try_into().unwrap()),
                ]);
            }
            at += 12 + n;
        }
        let density_ok = match density {
            Some(r) => phys == Some([(r.x / 0.0254).round() as u32, (r.y / 0.0254).round() as u32]),
            None => phys.is_none(),
        };
        let row = json!({"engine":engine,"native_pixels":size,"recipe_roundtrip":true,"all_five_strokes_and_transform":true,"adjustment_layer_preserved":true,"native_export_exact":differences==0,"maximum_error_u16":max_error,"different_channels":differences,"physical_density_preserved":density_ok,"source_resolution":density,"export_file":output_path.file_name().unwrap().to_string_lossy()});
        println!("{row}");
        checks.push(row);
    }
    let unchanged = digest == tr_platform::snapshot(source)?.1;
    let passed = unchanged
        && checks
            .iter()
            .all(|c| c["native_export_exact"] == true && c["physical_density_preserved"] == true);
    let report = json!({"passed":passed,"source_unchanged":unchanged,"source_sha256":digest,"checks":checks,"scope":"One authorized NEF D750, Experimental and LibRaw bilinear, canonical CPU replay and isolated PNG16 export at every native pixel. No camera-wide, latency, Windows or physical-display qualification."});
    std::fs::write(
        root.join("reports/raw-tools-native.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(passed, "RAW tools native/export comparison failed");
    Ok(())
}
