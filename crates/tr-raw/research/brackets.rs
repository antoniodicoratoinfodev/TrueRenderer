//! Opt-in offline extraction for the photographic exposure-bracket bench.
//! This is not a product decoder entry point or a calibrated camera profile.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::PathBuf};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_new(path: PathBuf, bytes: &[u8]) -> Result<String> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(digest(bytes))
}

#[test]
#[ignore = "offline photographic extraction; needs TR_BRACKET_EXTRACT and TR_BRACKET_OUTPUT"]
fn research_campaign_bracket_extract() -> Result<()> {
    let input = PathBuf::from(std::env::var("TR_BRACKET_EXTRACT")?);
    let output = PathBuf::from(std::env::var("TR_BRACKET_OUTPUT")?);
    let manifest = fs::read(input)?;
    let config: Value = serde_json::from_slice(&manifest)?;
    ensure!(config["protocol"] == "TR-photo-bracket-extract-v1");
    let frames = config["frames"].as_array().context("frames")?;
    ensure!(!frames.is_empty() && frames.len() <= 64);
    fs::create_dir(&output)?;
    let mut records = vec![];
    for (index, frame) in frames.iter().enumerate() {
        let source = frame["source"].as_str().context("source")?;
        ensure!(fs::metadata(source)?.len() <= crate::MAX_SOURCE_BYTES as u64);
        let bytes = fs::read(source)?;
        ensure!(digest(&bytes) == frame["sha256"], "source hash mismatch");
        let info = crate::probe(&bytes)?;
        let raw = match crate::Raw::parse(&bytes)? {
            crate::Raw::Dng(d) => d.mosaic()?,
            crate::Raw::Nef(d) => d.mosaic()?,
            crate::Raw::Raf(d) => d.mosaic()?,
        };
        let mut recovered = raw.values.clone();
        let evidence = crate::highlights::recover(
            &mut recovered,
            &raw.saturation,
            raw.width,
            raw.active_width,
            raw.height,
            raw.cfa,
        );
        let pack = |values: &[f32]| -> Vec<u8> {
            values
                .chunks_exact(raw.width)
                .flat_map(|row| row[..raw.active_width].iter().flat_map(|v| v.to_le_bytes()))
                .collect()
        };
        let name = format!("frame-{index:03}");
        let values = pack(&raw.values);
        let values_hash = write_new(output.join(format!("{name}.f32")), &values)?;
        drop(values);
        let recovered_hash = write_new(
            output.join(format!("{name}.recovered.f32")),
            &pack(&recovered),
        )?;
        let state: Vec<_> = raw
            .saturation
            .chunks_exact(raw.width)
            .flat_map(|row| row[..raw.active_width].iter().copied())
            .collect();
        let state_hash = write_new(output.join(format!("{name}.state")), &state)?;
        let pattern: Vec<_> = (0..raw.cfa.period * raw.cfa.period)
            .map(|i| raw.cfa.color(i % raw.cfa.period, i / raw.cfa.period))
            .collect();
        records.push(json!({
            "id":frame["id"], "source_sha256":frame["sha256"],
            "values":format!("{name}.f32"), "values_sha256":values_hash,
            "recovered":format!("{name}.recovered.f32"), "recovered_sha256":recovered_hash,
            "state":format!("{name}.state"), "state_sha256":state_hash,
            "width":raw.active_width, "height":raw.height, "period":raw.cfa.period,
            "pattern":pattern, "crop":raw.crop, "orientation":raw.orientation,
            "camera":info.camera, "profile_sha256":info.profile_hash,
            "reconstructed_sites":evidence.reconstructed,
        }));
        ensure!(
            digest(&fs::read(source)?) == frame["sha256"],
            "source changed"
        );
    }
    let report = json!({
        "protocol":"TR-photo-bracket-planes-v1", "recipe":crate::RECIPE,
        "extract_manifest_sha256":digest(&manifest), "frames":records,
        "domain":"Linearized, black-subtracted, nominal-white-normalized camera CFA before WB; active sensor coordinates, not oriented display coordinates.",
        "limits":"Metadata calibration and digital clipping only; no measured physical saturation, noise model, radiometric certification or absolute colour truth. Offline test buffers are outside the product memory plan."
    });
    write_new(
        output.join("planes.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
