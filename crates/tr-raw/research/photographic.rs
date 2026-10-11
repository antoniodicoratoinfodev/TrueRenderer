//! Verification-only remosaic of frozen linearized public JPEG crops.
use super::{
    Metrics, Scene, confidence, constrained, evaluate, gate, hash, random, refined, xtrans,
};
use crate::cfa::{Cfa, XTRANS};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, f64::consts::TAU, fs, path::PathBuf};

#[test]
#[ignore = "offline photographic research campaign; needs TR_PHOTO_LAB_MANIFEST"]
fn photographic_quality_research_campaign() -> Result<()> {
    let path =
        PathBuf::from(std::env::var("TR_PHOTO_LAB_MANIFEST").context("TR_PHOTO_LAB_MANIFEST")?);
    let bytes = fs::read(&path)?;
    let config: Value = serde_json::from_slice(&bytes)?;
    let candidate = std::env::var("TR_PHOTO_CANDIDATE").unwrap_or_else(|_| "DR4".into());
    ensure!(
        matches!(candidate.as_str(), "DR4" | "DR5" | "DR6"),
        "photo candidate"
    );
    ensure!(
        config["protocol"] == "TRExp-photo-remosaic-v1",
        "photo protocol"
    );
    let border = config["border"].as_u64().context("border")? as usize;
    let root = path.parent().context("manifest parent")?;
    let mut aggregates: BTreeMap<String, [Metrics; 2]> = BTreeMap::new();
    let mut phases: BTreeMap<(String, usize), [Metrics; 2]> = BTreeMap::new();
    let mut details = vec![];
    for (tile_id, tile) in config["tiles"]
        .as_array()
        .context("tiles")?
        .iter()
        .enumerate()
    {
        let name = tile["file"].as_str().context("file")?;
        let data = fs::read(root.join(name))?;
        ensure!(hash(&data) == tile["sha256"], "tile hash");
        let w = tile["width"].as_u64().context("width")? as usize;
        let h = tile["height"].as_u64().context("height")? as usize;
        ensure!(
            w > 2 * border && h > 2 * border && w <= 512 && h <= 512 && data.len() == w * h * 12,
            "tile dimensions"
        );
        let truth: Vec<[f32; 3]> = data
            .as_chunks::<12>()
            .0
            .iter()
            .map(|p| {
                std::array::from_fn(|c| f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap()))
            })
            .collect();
        ensure!(
            truth.iter().flatten().all(|v| v.is_finite()),
            "finite reference"
        );
        for (noise_id, sigma) in config["noise_sigma"]
            .as_array()
            .context("noise_sigma")?
            .iter()
            .enumerate()
        {
            let sigma = sigma.as_f64().context("sigma")?;
            let category = format!("photo-{}-noise{noise_id}", tile["source_id"]);
            let scene = Scene {
                category: "photographic",
                name: name.into(),
                truth: truth.clone(),
                sigma,
                noise_correlation: 0.,
            };
            for phase in 0..36 {
                let cfa = Cfa::new(6, &XTRANS)?.shifted(phase % 6, phase / 6);
                let mut seed = config["seed"].as_u64().context("seed")? as u32
                    ^ (tile_id as u32 * 1877 + phase as u32 * 8191 + noise_id as u32 * 131071);
                let mosaic: Vec<_> = truth
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let noise = (-2. * random(&mut seed).ln()).sqrt()
                            * (TAU * random(&mut seed)).cos()
                            * sigma;
                        (f64::from(p[cfa.color(i % w, i / w)]) + noise) as f32
                    })
                    .collect();
                let baseline = xtrans::CameraRgb::new(&mosaic, w, h, cfa);
                let a: Vec<_> = (0..w * h).map(|i| baseline.at(i % w, i / w)).collect();
                let b = match candidate.as_str() {
                    "DR4" => confidence::reconstruct(&mosaic, w, h, cfa),
                    "DR5" => refined::reconstruct(&mosaic, w, h, cfa),
                    "DR6" => constrained::reconstruct(&mosaic, w, h, cfa),
                    _ => unreachable!(),
                };
                let pair = [
                    evaluate(&a, &scene, &mosaic, w, h, cfa, border),
                    evaluate(&b, &scene, &mosaic, w, h, cfa, border),
                ];
                for (i, m) in pair.iter().enumerate() {
                    ensure!(
                        m.mismatches == 0 && m.nonfinite == 0,
                        "measured/nonfinite photo {name}"
                    );
                    aggregates.entry(category.clone()).or_default()[i].merge(m);
                    phases.entry((category.clone(), phase)).or_default()[i].merge(m);
                }
                details.push(json!({"tile":name,"category":category,"phase":phase,
                    "baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate(&pair,"photographic",&config)}));
            }
        }
        if (tile_id + 1) % 9 == 0 {
            println!("photographic: {} tiles measured", tile_id + 1);
        }
    }
    let per_phase:Vec<_>=phases.iter().map(|((category,phase),pair)|json!({"category":category,"phase":phase,
        "baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate(pair,"photographic",&config)})).collect();
    let all_passed = per_phase.iter().all(|p| p["gate"]["passed"] == true);
    let categories:Vec<_>=aggregates.iter().map(|(category,pair)|json!({"category":category,
        "baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate(pair,"photographic",&config)})).collect();
    let report = json!({"protocol":config["protocol"],"manifest_sha256":hash(&bytes),"candidate":candidate,
        "role":if candidate=="DR4" {"held-out photographic surrogate"} else {"regression on photographs already measured with DR4"},
        "case_count":details.len(),"photographic_surrogate_gates_passed":all_passed,"production_promoted":false,
        "camera_ground_truth_available":false,"categories":categories,"per_phase":per_phase,"cases":details,"limits":config["limits"]});
    fs::write(
        root.join(match candidate.as_str() {
            "DR4" => "photographic-quality.json",
            "DR5" => "photographic-quality-dr5.json",
            "DR6" => "photographic-quality-dr6.json",
            _ => unreachable!(),
        }),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "photographic surrogate: {} cases; gates={all_passed}",
        details.len()
    );
    Ok(())
}
