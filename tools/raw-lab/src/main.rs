//! Offline research CLI, deliberately independent of tr-raw, tr-worker and
//! LibRaw. Accepts an extracted codec payload, not a product RAW fallback.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::PathBuf,
};

// Shared sensor geometry only; the standalone executable does not import the
// derived codec, RAF parser, compatibility profiles, or production library.
#[allow(dead_code)]
#[path = "../../../crates/tr-raw/src/cfa.rs"]
mod cfa;
#[path = "../../../crates/tr-raw/research/fuji.rs"]
mod fuji;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "Usage: tr-raw-lab PAYLOAD WIDTH HEIGHT NEW_OUTPUT_U16LE (offline research only)"
    );
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[3]);
    let width: usize = args[1].to_str().context("width")?.parse()?;
    let height: usize = args[2].to_str().context("height")?.parse()?;
    let mut bytes = vec![];
    File::open(&source)?
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 256 * 1024 * 1024,
        "payload exceeds laboratory source limit"
    );
    let codec = fuji::Codec::parse(&bytes, width, height)?;
    let samples = codec.unpack(cfa::XTRANS.map(usize::from))?;
    let mut file = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    let mut digest = Sha256::new();
    for sample in &samples {
        let bytes = sample.to_le_bytes();
        file.write_all(&bytes)?;
        digest.update(bytes);
    }
    file.flush()?;
    println!(
        "{}",
        serde_json::json!({"candidate":"Fuji circular rows v1","width":width,"height":height,
        "payload_sha256":format!("{:x}",Sha256::digest(&bytes)),"output_u16le_sha256":format!("{:x}",digest.finalize()),
        "samples":samples.len(),"live_buffer_bound_bytes":codec.live_buffer_bound(),"provenance_qualified":false,
        "production_selected":false,"runtime_libraw":false,"imports_e8_codec":false})
    );
    Ok(())
}
