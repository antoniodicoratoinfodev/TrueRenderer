//! End-to-end codec/IPC checks. All generated files remain in private var/.
use anyhow::{Result, ensure};
use std::path::Path;
use tr_core::{
    decoder::RawEngine,
    export::{Format, MAX_ENCODED, Options},
};
use tr_platform::Broker;

pub fn run(root: &Path, worker: &Path, source: Option<&Path>) -> Result<()> {
    ensure!(
        tr_platform::external_decoding_available(worker),
        "La verifica export/FITS richiede il bundle isolato"
    );
    let folder = tempfile::Builder::new()
        .prefix("export-fits-")
        .tempdir_in(root.join("var"))?
        .keep();
    let default = root.join("corpus/03_Gradiente_16bit.png");
    let source = source.unwrap_or(&default);
    let (_, digest) = tr_platform::snapshot(source)?;
    let mut checks = Vec::new();
    let mut broker = Broker::with_slot(worker.into(), 0);
    let mut reader = Broker::with_slot(worker.into(), 1);
    let raw = source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nef"));
    for engine in RawEngine::choices() {
        for format in Format::ALL {
            if format == Format::DngRaw && (!raw || engine != RawEngine::default()) {
                continue;
            }
            broker.set_raw_engine(engine);
            let options = Options {
                format,
                long_edge: if format == Format::DngRaw { 0 } else { 1024 },
                ..Options::default()
            };
            let start = std::time::Instant::now();
            let result = (|| -> Result<_> {
                let snapshot = broker.prepare_snapshot(source, &digest, || false)?;
                let (info, bytes) =
                    broker.export_snapshot_bounded(snapshot, options, MAX_ENCODED, || false)?;
                let path = crate::photo_export::publish(
                    &folder,
                    &format!("{engine:?}-{format:?}.input"),
                    format,
                    &bytes,
                    || false,
                )?;
                let (_, output_digest) = tr_platform::snapshot(&path)?;
                let readback_engine = if matches!(format, Format::DngLinear16 | Format::DngRaw) {
                    RawEngine::LibRawAhd
                } else {
                    RawEngine::default()
                };
                reader.set_raw_engine(readback_engine);
                // Read the exported file through the same isolation, never a decoder in the UI.
                let readback = reader.decode(&path, &output_digest, 0);
                // Some platform RAW engines do not implement LinearRaw. Record that
                // as an interoperability failure, not a successful sample comparison.
                let (readable, readback_info, error) = match readback {
                    Ok(decoded) => (true, Some(decoded.info), None),
                    Err(error) => (false, None, Some(format!("{error:#}"))),
                };
                Ok(
                    serde_json::json!({"format":format,"engine":engine,"encoded":info,"readback_engine":readback_engine,"readable":readable,"readback":readback_info,"readback_error":error,"seconds":start.elapsed().as_secs_f64(),"passed":readable}),
                )
            })();
            let check = match result {
                Ok(check) => check,
                Err(error) => {
                    serde_json::json!({"format":format,"engine":engine,"passed":false,"error":format!("{error:#}")})
                }
            };
            println!("{check}");
            checks.push(check);
        }
    }
    // Release both authenticated slots before the dedicated two-slot FITS checks.
    drop(reader);
    drop(broker);
    if !raw {
        let mut bytes = Vec::new();
        for card in [
            "SIMPLE  =                    T",
            "BITPIX  =                   32",
            "NAXIS   =                    2",
            "NAXIS1  =                   64",
            "NAXIS2  =                   32",
            "BSCALE  =                    2",
            "BZERO   =                    7",
            "BLANK   =                   -1",
            "BUNIT   = 'ADU'",
            "END",
        ] {
            bytes.extend_from_slice(card.as_bytes());
            bytes.resize(bytes.len().div_ceil(80) * 80, b' ');
        }
        bytes.resize(2880, b' ');
        for index in 0..2048i32 {
            bytes.extend_from_slice(
                &(if index == 0 { -1 } else { 16_777_217 + index }).to_be_bytes(),
            );
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        let path = folder.join("scientific-i32.fits");
        std::fs::write(&path, bytes)?;
        let (_, digest) = tr_platform::snapshot(&path)?;
        for slot in 0..2 {
            let mut broker = Broker::with_slot(worker.into(), slot);
            let source = broker.prepare_snapshot(&path, &digest, || false)?;
            let sample = broker.scientific_sample(source.clone(), 1, 0, || false)?;
            let invalid = broker.scientific_sample(source.clone(), 0, 0, || false)?;
            let decoded =
                broker.decode_reference_snapshot_bounded(source, 16, 64 * 1024, 1, || false)?;
            let meta = decoded.info.scientific.as_ref().unwrap();
            let passed = sample.stored == Some(16_777_218.)
                && sample.physical == Some(33_554_443.)
                && invalid.validity == "BLANK"
                && meta.valid == 2047
                && meta.invalid == 1
                && decoded.raster.width == 16;
            checks.push(serde_json::json!({"case":"FITS i32 scientific sample plus masked reference mip","slot":slot,"passed":passed,"sample":sample,"invalid_sample":invalid,"info":decoded.info,"transport":decoded.transport}));
        }
    }
    let (_, after) = tr_platform::snapshot(source)?;
    let unchanged = after == digest;
    let passed = unchanged && checks.iter().all(|c| c["passed"] == true);
    let report = serde_json::json!({"passed":passed,"source_unchanged":unchanged,"checks":checks,"scope":"Codec roundtrip readability through isolated production decoder; no physical display/colour fidelity or general DNG archival qualification. Sources and generated exports private."});
    std::fs::write(
        folder.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", folder.join("report.json").display());
    ensure!(
        passed,
        "Verifica export/FITS non completamente superata: leggere il rapporto"
    );
    Ok(())
}
