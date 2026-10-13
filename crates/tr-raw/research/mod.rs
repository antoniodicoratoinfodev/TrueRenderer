//! Laboratory only. Never linked into a production tr-raw build.
mod fixtures;
mod fuji;
mod quality;
mod xtrans_e8;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, hint::black_box, path::PathBuf, time::Instant};

#[test]
fn analytic_zero_streams_and_payload_mutations() {
    let cfa = crate::cfa::XTRANS.map(usize::from);
    for (width, quantizers) in [
        (768, None),
        (792, None),
        (1536, Some(vec![0, 1, 7, 255, 0])),
    ] {
        let groups = quantizers.as_ref().map_or(2, Vec::len);
        let bytes = fixtures::zero_payload(width, groups, quantizers.as_deref());
        let reference = super::codec::Codec::parse(&bytes, width, groups * 6)
            .unwrap()
            .unpack(cfa)
            .unwrap();
        let actual = fuji::Codec::parse(&bytes, width, groups * 6)
            .unwrap()
            .unpack(cfa)
            .unwrap();
        // The golden is an analytic constant, NOT an answer produced by the
        // reference codec. The two implementations are checked separately.
        assert!(reference.iter().all(|&v| v == 0));
        assert_eq!(actual, vec![0; width * groups * 6]);
        for end in 0..bytes.len() {
            assert!(fuji::Codec::parse(&bytes[..end], width, groups * 6).is_err());
        }
    }
    let valid = fixtures::zero_payload(768, 1, Some(&[5]));
    // Shorten the declared stripe itself so that parsing succeeds and the bit
    // reader, rather than merely the container length check, sees each EOF.
    let mut successful_tails = 0;
    for len in 1..valid.len() - 48 {
        let mut truncated = valid[..48 + len].to_vec();
        truncated[16..20].copy_from_slice(&(len as u32).to_be_bytes());
        let a = super::codec::Codec::parse(&truncated, 768, 6)
            .unwrap()
            .unpack(cfa);
        let b = fuji::Codec::parse(&truncated, 768, 6).unwrap().unpack(cfa);
        match (a, b) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a, b);
                successful_tails += 1;
            }
            (Err(_), Err(_)) => {}
            _ => panic!("stripe truncation mismatch at {len}"),
        }
    }
    assert!(successful_tails <= 16);
    let mut seed = 0x483721a5u32;
    let mut accepted = 0;
    for _ in 0..10_000 {
        let mut changed = valid.clone();
        // Exercise the compressed payload, not just the RAF metadata header.
        for _ in 0..4 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = 48 + seed as usize % (changed.len() - 48);
            changed[at] ^= (seed >> 24) as u8;
        }
        let baseline = super::codec::Codec::parse(&changed, 768, 6)
            .unwrap()
            .unpack(cfa);
        let candidate = fuji::Codec::parse(&changed, 768, 6).unwrap().unpack(cfa);
        match (baseline, candidate) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a, b);
                accepted += 1;
            }
            (Err(_), Err(_)) => {}
            _ => panic!("different payload acceptance at seed {seed}"),
        }
    }
    assert!(
        accepted < 10_000,
        "negative corpus must contain rejected payloads"
    );
    println!(
        "payload mutations: {accepted} accepted, {} rejected, equal outcomes",
        10_000 - accepted
    );
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn sample_hash(samples: &[u16]) -> String {
    let mut sha = Sha256::new();
    for sample in samples {
        sha.update(sample.to_le_bytes());
    }
    format!("{sha:x}", sha = sha.finalize())
}
fn timing_summary(values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p50 = (sorted[(sorted.len() - 1) / 2] + sorted[sorted.len() / 2]) / 2.;
    let mut deviations: Vec<_> = sorted.iter().map(|v| (v - p50).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    json!({"p50_ms":p50,"p95_ms":sorted[(sorted.len() as f64 * 0.95).ceil() as usize - 1],
        "min_ms":sorted[0],"max_ms":sorted[sorted.len()-1],"mad_ms":(deviations[(deviations.len()-1)/2]+deviations[deviations.len()/2])/2.,"samples_ms":values})
}

/// Explicit offline laboratory invocation; never accesses a user file in the
/// ordinary unit suite. Manifest/output must be supplied by the operator.
#[test]
#[ignore = "offline research campaign; needs TR_FUJI_LAB_MANIFEST"]
fn codec_research_campaign() -> Result<()> {
    let manifest = PathBuf::from(
        std::env::var("TR_FUJI_LAB_MANIFEST").context("TR_FUJI_LAB_MANIFEST is required")?,
    );
    let config: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    let source = fs::read(config["source"].as_str().context("source")?)?;
    ensure!(hash(&source) == config["source_sha256"], "source changed");
    let raw = super::Raf::parse(&source)?;
    let tiff = crate::tiff::Tiff::parse(super::slice(
        &source,
        super::be32(&source, 100)?,
        super::be32(&source, 104)?,
    )?)?;
    let (ifd, _, _) = tiff.directory(tiff.ifds[0].integer(0xf000, 0)? as usize)?;
    let payload = tiff.slice(
        ifd.integer(0xf007, 0)? as usize,
        ifd.integer(0xf008, 0)? as usize,
    )?;
    let candidate = fuji::Codec::parse(payload, raw.width, raw.height)?;
    let pattern = crate::cfa::XTRANS.map(usize::from);
    let a = raw.codec.unpack(pattern)?;
    let b = candidate.unpack(pattern)?;
    ensure!(a == b, "candidate differs from E8 including sensor margins");
    let ref_bytes = fs::read(
        config["reference_u16le"]
            .as_str()
            .context("reference_u16le")?,
    )?;
    ensure!(
        hash(&ref_bytes) == config["reference_sha256"],
        "reference changed"
    );
    let [left, top, width, height] = raw.active;
    let rw = config["reference_width"]
        .as_u64()
        .context("reference_width")?;
    let rh = config["reference_height"]
        .as_u64()
        .context("reference_height")?;
    ensure!(
        (rw as usize, rh as usize) == (width, height),
        "reference crop"
    );
    ensure!(ref_bytes.len() == width * height * 2, "reference length");
    let independent: Vec<_> = ref_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    for y in 0..height {
        ensure!(
            b[(y + top) * raw.width + left..(y + top) * raw.width + left + width]
                == independent[y * width..(y + 1) * width],
            "Adobe mismatch row {y}"
        );
    }
    let output_hash = sample_hash(&b);
    let full_count = b.len();
    drop((a, b, independent));
    let protocol: Value = serde_json::from_str(include_str!("../tests/fixtures/research-v1.json"))?;
    let warmups = protocol["codec"]["warmups_per_implementation"]
        .as_u64()
        .unwrap();
    let iterations = protocol["codec"]["repetitions_per_implementation"]
        .as_u64()
        .unwrap();
    for _ in 0..warmups {
        black_box(raw.codec.unpack(pattern)?);
        black_box(candidate.unpack(pattern)?);
    }
    let mut timing: [Vec<f64>; 2] = std::array::from_fn(|_| vec![]);
    for iteration in 0..iterations {
        for implementation in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
            let start = Instant::now();
            let image = if implementation == 0 {
                raw.codec.unpack(pattern)?
            } else {
                candidate.unpack(pattern)?
            };
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            timing[implementation].push(elapsed);
            black_box(&image);
            // Hash is deliberately outside the timed section. Every repeat
            // must still be identical, not merely the first comparison.
            ensure!(
                sample_hash(&image) == output_hash,
                "nondeterministic unpack"
            );
        }
    }
    let baseline = timing_summary(&timing[0]);
    let proposed = timing_summary(&timing[1]);
    let ratio = proposed["p50_ms"].as_f64().unwrap() / baseline["p50_ms"].as_f64().unwrap();
    // A lower bound is enough to prove that the candidate's tracked live
    // buffers do not grow: E8 also has table/reader metadata beyond this sum.
    let baseline_buffer_lower_bound = full_count * 2 + 18 * 514 * 4 + 4 * 6 * 41 * 2 * 4;
    let report = json!({"protocol":"TRExp-research-v1","candidate_source_sha256":hash(include_bytes!("fuji.rs")),"source_sha256":hash(&source),"payload_sha256":hash(payload),
        "reference_sha256":hash(&ref_bytes),"full_sensor_samples_compared":full_count,"adobe_active_samples_compared":width*height,
        "sample_mismatches":0,"output_u16le_sha256":output_hash,"baseline":baseline,"candidate":proposed,"p50_ratio":ratio,
        "speed_target_passed":ratio<=0.8,"warmups":warmups,"repetitions":iterations,
        "candidate_live_buffer_bound_bytes":candidate.live_buffer_bound(),
        "baseline_live_buffer_lower_bound_bytes":baseline_buffer_lower_bound,
        "tracked_buffer_target_passed":candidate.live_buffer_bound()<=baseline_buffer_lower_bound,
        "candidate_row_storage_bytes":3*3*514*2,"baseline_row_storage_bytes":18*514*4,
        "provenance_qualified":false,"production_selected":false,"photographic_files":1,
        "limits":["same-session format knowledge includes CDDL source","no cold-start or thermal control","no lossless photographic corpus","Adobe excludes inactive sensor margins","memory is buffer accounting, not process RSS"]});
    let destination = config["output"].as_str().context("output")?;
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    println!(
        "full samples={full_count}; Adobe={}; p50 ratio={ratio:.4}; report={destination}",
        width * height
    );
    Ok(())
}
