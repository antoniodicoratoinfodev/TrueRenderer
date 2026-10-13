//! Verification-only remosaic of frozen linearized public JPEG crops.
use super::{
    Metrics, Scene, confidence, constrained, evaluate, gate, hash, random, refined, xtrans,
};
use crate::cfa::{Cfa, XTRANS};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::TAU,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn validate_manifest(config: &Value, requested: Option<&str>) -> Result<String> {
    ensure!(
        config["protocol"] == "TRExp-photo-remosaic-v1",
        "photo protocol"
    );
    let candidate = config["candidate"].as_str().context("manifest candidate")?;
    ensure!(
        matches!(candidate, "DR4" | "DR5" | "DR6"),
        "photo candidate"
    );
    ensure!(
        requested.is_none_or(|value| value == candidate),
        "candidate differs from frozen manifest"
    );
    ensure!(config["phases"] == 36, "all 36 phases required");
    ensure!(
        config["border"] == 16
            && config["other_category_and_detail_rmse_max_ratio"] == 1.05
            && config["absolute_rmse_floor"] == 1e-6,
        "photo v1 gates"
    );
    ensure!(
        config["noise_sigma"] == json!([0.0, 0.012]),
        "photo v1 noise"
    );
    ensure!(
        config["seed"]
            .as_u64()
            .is_some_and(|v| u32::try_from(v).is_ok()),
        "seed out of range"
    );
    let sources = config["sources"].as_array().context("sources")?;
    let tiles = config["tiles"].as_array().context("tiles")?;
    ensure!(
        !sources.is_empty() && sources.len() <= 512 && tiles.len() == sources.len() * 9,
        "nine tiles per source required"
    );
    let mut counts = BTreeMap::new();
    let mut digests = BTreeSet::new();
    let valid_hash = |v: &Value| {
        v.as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    for source in sources {
        let id = source["id"].as_u64().context("source id")?;
        ensure!(
            counts.insert(id, 0).is_none()
                && valid_hash(&source["sha256"])
                && digests.insert(source["sha256"].as_str().unwrap().to_ascii_lowercase()),
            "duplicate source or invalid source hash"
        );
    }
    let mut names = BTreeSet::new();
    let mut coordinates = BTreeSet::new();
    for tile in tiles {
        let name = tile["file"].as_str().context("file")?;
        ensure!(
            !name.is_empty()
                && Path::new(name)
                    .components()
                    .all(|p| matches!(p, std::path::Component::Normal(_)))
                && names.insert(name),
            "duplicate or invalid tile path"
        );
        let id = tile["source_id"].as_u64().context("source id")?;
        *counts.get_mut(&id).context("unknown tile source")? += 1;
        let (x, y) = (
            tile["x"].as_u64().context("tile x")?,
            tile["y"].as_u64().context("tile y")?,
        );
        ensure!(coordinates.insert((id, x, y)), "duplicate crop");
        let (w, h) = (
            tile["width"].as_u64().context("width")?,
            tile["height"].as_u64().context("height")?,
        );
        ensure!(
            w > 32 && h > 32 && w <= 512 && h <= 512 && valid_hash(&tile["sha256"]),
            "tile dimensions/hash"
        );
    }
    ensure!(
        counts.values().all(|&count| count == 9),
        "incomplete source crops"
    );
    Ok(candidate.to_owned())
}

#[test]
#[ignore = "offline photographic research campaign; needs TR_PHOTO_LAB_MANIFEST"]
fn photographic_quality_research_campaign() -> Result<()> {
    let path =
        PathBuf::from(std::env::var("TR_PHOTO_LAB_MANIFEST").context("TR_PHOTO_LAB_MANIFEST")?);
    let bytes = fs::read(&path)?;
    let config: Value = serde_json::from_slice(&bytes)?;
    let requested = std::env::var("TR_PHOTO_CANDIDATE").ok();
    let candidate = validate_manifest(&config, requested.as_deref())?;
    let border = config["border"].as_u64().context("border")? as usize;
    let root = path.parent().context("manifest parent")?;
    let destination = root.join(match candidate.as_str() {
        "DR4" => "photographic-quality.json",
        "DR5" => "photographic-quality-dr5.json",
        "DR6" => "photographic-quality-dr6.json",
        _ => unreachable!(),
    });
    ensure!(!destination.exists(), "report already exists");
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
        let w = tile["width"].as_u64().context("width")? as usize;
        let h = tile["height"].as_u64().context("height")? as usize;
        ensure!(
            fs::metadata(root.join(name))?.len() == (w * h * 12) as u64,
            "tile byte count"
        );
        let data = fs::read(root.join(name))?;
        ensure!(hash(&data) == tile["sha256"], "tile hash");
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
    ensure!(
        details.len() == config["tiles"].as_array().unwrap().len() * 2 * 36,
        "incomplete campaign"
    );
    let all_passed = !per_phase.is_empty() && per_phase.iter().all(|p| p["gate"]["passed"] == true);
    let categories:Vec<_>=aggregates.iter().map(|(category,pair)|json!({"category":category,
        "baseline":pair[0].value(),"candidate":pair[1].value(),"gate":gate(pair,"photographic",&config)})).collect();
    let report = json!({"protocol":config["protocol"],"manifest_sha256":hash(&bytes),"candidate":candidate,
        "role":config["role"].as_str().unwrap_or(if candidate=="DR4" {"held-out photographic surrogate"} else {"regression on photographs already measured with DR4"}),
        "case_count":details.len(),"photographic_surrogate_gates_passed":all_passed,"production_promoted":false,
        "camera_ground_truth_available":false,"categories":categories,"per_phase":per_phase,"cases":details,"limits":config["limits"]});
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    println!(
        "photographic surrogate: {} cases; gates={all_passed}",
        details.len()
    );
    Ok(())
}

#[test]
fn photographic_manifest_cannot_pass_empty_partial_or_mislabeled_data() {
    let tiles: Vec<_> = (0..9)
        .map(|i| {
            json!({"file":format!("{i}.rgbf32"),"source_id":0,
        "x":i%3*20,"y":i/3*20,"width":64,"height":64,"sha256":"b".repeat(64)})
        })
        .collect();
    let valid = json!({"protocol":"TRExp-photo-remosaic-v1","candidate":"DR6","phases":36,
        "border":16,"noise_sigma":[0.0,0.012],"seed":39127651,
        "other_category_and_detail_rmse_max_ratio":1.05,"absolute_rmse_floor":1e-6,
        "sources":[{"id":0,"sha256":"a".repeat(64)}],"tiles":tiles});
    assert_eq!(validate_manifest(&valid, None).unwrap(), "DR6");
    assert!(validate_manifest(&valid, Some("DR4")).is_err());
    for (key, value) in [
        ("tiles", json!([])),
        ("sources", json!([])),
        ("phases", json!(0)),
        ("noise_sigma", json!([])),
        ("noise_sigma", json!([0., -0.1])),
        ("border", json!(0)),
        ("seed", json!(u64::MAX)),
        ("absolute_rmse_floor", json!(1.)),
        ("candidate", json!("BR5")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(validate_manifest(&invalid, None).is_err(), "{key}");
    }
    for (key, value) in [
        ("source_id", json!(7)),
        ("sha256", json!("invalid")),
        ("file", json!("../outside")),
        ("file", json!("1.rgbf32")),
        ("width", json!(u64::MAX)),
    ] {
        let mut invalid = valid.clone();
        invalid["tiles"][0][key] = value;
        assert!(validate_manifest(&invalid, None).is_err(), "{key}");
    }
    let mut invalid = valid.clone();
    invalid["tiles"][1]["x"] = json!(0);
    assert!(validate_manifest(&invalid, None).is_err());
}
