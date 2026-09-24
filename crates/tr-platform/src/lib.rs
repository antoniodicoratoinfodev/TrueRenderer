//! R0 private-pipe protocol, with native XPC leases in a macOS app bundle.
//! Pipe workers retain the corpus gate; external previews require the macOS XPC bundle.
pub mod isolation;
mod resources;
#[cfg(target_os = "macos")]
mod xpc;
use crate::isolation::{Isolation, Worker};
use anyhow::{Context, Result, bail, ensure};
pub use resources::{MemoryPressure, memory_pressure};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
pub use tr_core::corpus::CorpusPolicy;
/// Read once at service startup; other resource adapters remain unqualified.
pub fn physical_memory_mib() -> u64 {
    #[cfg(target_os = "macos")]
    if let Ok(output) = Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        && let Ok(bytes) = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
    {
        return bytes / (1024 * 1024);
    }
    8192
}
/// Queried off the UI thread; an unavailable adapter preserves the selected profile.
pub fn on_battery() -> bool {
    #[cfg(target_os = "macos")]
    return Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .is_ok_and(|o| {
            o.status.success() && String::from_utf8_lossy(&o.stdout).contains("Battery Power")
        });
    #[cfg(not(target_os = "macos"))]
    false
}
use tr_core::{
    color::LinearImage,
    protocol::{self, DecodeRequest, MAX_SOURCE, RasterInfo},
};

pub fn snapshot(path: &Path) -> Result<(Vec<u8>, String)> {
    snapshot_bounded(path, MAX_SOURCE as u64, &|| false)
}
fn snapshot_bounded(
    path: &Path,
    maximum: u64,
    cancelled: &impl Fn() -> bool,
) -> Result<(Vec<u8>, String)> {
    let file = File::open(path).context("Apertura in sola lettura")?;
    ensure!(
        file.metadata()?.is_file(),
        "La sorgente non è un file regolare"
    );
    ensure!(
        file.metadata()?.len() <= maximum.min(MAX_SOURCE as u64),
        "Limite sorgente: 256 MiB"
    );
    let before = file.metadata()?;
    let length = before.len() as usize;
    let mut bytes = vec![0; length];
    let mut reader = &file;
    let mut hash = Sha256::new();
    for block in bytes.chunks_mut(1024 * 1024) {
        ensure!(!cancelled(), "Snapshot annullato");
        reader.read_exact(block)?;
        hash.update(block);
    }
    ensure!(
        observation_token(path, &before) == observation_token(path, &file.metadata()?),
        "Sorgente cambiata durante lo snapshot"
    );
    let digest = format!("{:x}", hash.finalize());
    // Hash applies to the exact private bytes sent to the worker, never to a later reopening.
    Ok((bytes, digest))
}
/// Private source bytes and their verified identity, shared by cache lookup and decode.
/// Fields stay private so callers cannot substitute bytes after validation.
#[derive(Clone)]
pub struct SourceSnapshot {
    bytes: Arc<Vec<u8>>,
    digest: String,
    requested_identity: String,
}
impl SourceSnapshot {
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn byte_len(&self) -> u64 {
        self.bytes.len() as u64
    }
    /// Match a recipe only after the broker has validated the requested digest
    /// or observation token against these exact private bytes. A scan token is
    /// not a SHA-256 and is never compared directly with one.
    pub fn matches_recipe_source(&self, recorded: &str) -> bool {
        recorded == self.digest || recorded == self.requested_identity
    }
}
/// Cheap evidence that a source has not been swapped underneath a render.
///
/// Never a content hash: the prefix says so, and a real hit still verifies
/// SHA-256. What it must catch is replacement, including a file swapped for one
/// of the same length whose modification time was restored. Unix reads that
/// from the device and inode; Windows has no equivalent on `Metadata`, so the
/// path is opened briefly for its volume, file index and ChangeTime. A path that cannot be
/// opened degrades to size and time rather than failing: the caller is asking
/// for an observation, not a guarantee.
pub fn observation_token(path: &Path, metadata: &std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        let _ = path;
        use std::os::unix::fs::MetadataExt;
        format!(
            "unverified:{}:{}:{}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let identity = windows_identity(path)
            .map(|(volume, index, changed)| format!("{volume}:{index}:{changed}"))
            .unwrap_or_else(|| "sconosciuta".into());
        format!(
            "unverified:{}:{}:{}:{}",
            identity,
            metadata.len(),
            metadata.last_write_time(),
            metadata.creation_time()
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        format!(
            "unverified:{}:{:?}",
            metadata.len(),
            metadata.modified().ok()
        )
    }
}

/// Volume, file index and ChangeTime: identity plus in-place change evidence.
/// Both are only readable from an open handle, which `Metadata` does not carry.
#[cfg(windows)]
fn windows_identity(path: &Path) -> Option<(u32, u64, i64)> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    };
    let file = File::options()
        .read(true)
        // Share everything: this must never block a writer or a delete.
        .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
        // Follow the same target as path.metadata() and the read-only snapshot.
        .open(path)
        .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return None;
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&raw mut basic).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return None;
    }
    // ChangeTime also changes after in-place writes and restoring LastWriteTime.
    Some((info.dwVolumeSerialNumber, index, basic.ChangeTime))
}
/// Whether this installation can open files outside the corpus at all.
///
/// macOS requires the signed bundle, because the XPC services live inside it.
/// Windows confines the pipe worker itself, so the binary's location says
/// nothing and the answer is yes wherever it runs.
pub fn external_decoding_available(binary: &Path) -> bool {
    if cfg!(windows) {
        return true;
    }
    cfg!(target_os = "macos")
        && binary.parent().is_some_and(|p| {
            p.file_name().is_some_and(|n| n == "MacOS")
                && p.parent()
                    .and_then(Path::parent)
                    .is_some_and(|b| b.extension().is_some_and(|e| e == "app"))
        })
}
pub fn supported_extension(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()).is_some_and(|x| {
        matches!(
            x.to_ascii_lowercase().as_str(),
            "png"
                | "jpg"
                | "jpeg"
                | "jpe"
                | "tif"
                | "tiff"
                | "dng"
                | "nef"
                | "nrw"
                | "cr2"
                | "cr3"
                | "crw"
                | "arw"
                | "srf"
                | "sr2"
                | "raf"
                | "orf"
                | "rw2"
                | "rwl"
                | "pef"
                | "srw"
                | "3fr"
                | "fff"
                | "iiq"
                | "mos"
                | "mef"
                | "mrw"
                | "erf"
                | "raw"
                | "gif"
                | "bmp"
                | "webp"
                | "heic"
                | "heif"
                | "fits"
                | "fit"
                | "fts"
        )
    })
}
#[derive(Debug)]
pub struct Decoded {
    pub raster: LinearImage,
    pub info: RasterInfo,
    pub digest: String,
    pub transport: &'static str,
    pub worker_pid: Option<u32>,
}
#[derive(Debug)]
struct SourceRejected(String);
impl std::fmt::Display for SourceRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for SourceRejected {}
struct Work {
    raw_engine: tr_core::decoder::RawEngine,
    id: u64,
    bytes: Arc<Vec<u8>>,
    edge: u32,
    intent: protocol::DecodeIntent,
    edit: Option<tr_core::editing::EditRecipe>,
    maximum_output_bytes: u64,
    result: mpsc::SyncSender<Result<WorkerOutput>>,
}
enum WorkerOutput {
    ScientificSample(tr_core::science::Sample),
    Raster(Box<RasterInfo>, Option<LinearImage>),
    Export(tr_core::export::Info, Vec<u8>),
}
impl WorkerOutput {
    fn raster(self) -> Result<(RasterInfo, Option<LinearImage>)> {
        match self {
            Self::Raster(info, raster) => Ok((*info, raster)),
            _ => anyhow::bail!("Output non raster inatteso"),
        }
    }
}
enum Owner {
    /// The isolation outlives the child on purpose: dropping it terminates
    /// whatever is still inside the job, so an abandoned worker cannot survive
    /// the broker that started it.
    Pipe(Worker, Isolation),
    #[cfg(target_os = "macos")]
    Xpc(xpc::Session),
}
struct Process {
    owner: Owner,
    work: mpsc::SyncSender<Work>,
    jobs: u32,
}
impl Drop for Process {
    fn drop(&mut self) {
        match &mut self.owner {
            Owner::Pipe(worker, _) => worker.terminate(),
            #[cfg(target_os = "macos")]
            Owner::Xpc(_) => {}
        }
    }
}
#[derive(Debug, Default, Clone, Serialize)]
pub struct BrokerStatistics {
    pub worker_pid: Option<u32>,
    pub peak_rss_bytes: u64,
    pub peak_footprint_bytes: u64,
    pub completed_jobs: u64,
    pub starts: u64,
    pub forced_stops: u64,
}
pub struct Broker {
    raw_engine: tr_core::decoder::RawEngine,
    binary: PathBuf,
    process: Option<Process>,
    next_id: u64,
    policy: CorpusPolicy,
    timeout: Duration,
    memory_limit: u64,
    external_memory_limit: u64,
    active_external: bool,
    external_timeout: Duration,
    slot: u32,
    bundled_xpc: bool,
    /// Whether this transport may carry sources outside the corpus.
    ///
    /// macOS grants it only to the signed XPC bundle. Windows grants it to the
    /// confined pipe worker, which is created inside a job with a
    /// kernel-enforced ceiling and which refuses the wider policy unless the
    /// kernel confirms that confinement. Elsewhere it is never granted, because
    /// there is no isolation to put the decoding behind.
    external_transport: bool,
    statistics: BrokerStatistics,
}
impl Broker {
    pub fn new(binary: PathBuf) -> Self {
        Self::with_slot(binary, 0)
    }
    pub fn with_slot(binary: PathBuf, slot: u32) -> Self {
        let bundled_xpc = cfg!(target_os = "macos")
            && binary.parent().is_some_and(|folder| {
                folder.file_name().is_some_and(|name| name == "MacOS")
                    && folder
                        .parent()
                        .and_then(Path::parent)
                        .is_some_and(|bundle| bundle.extension().is_some_and(|ext| ext == "app"))
            });
        Self {
            raw_engine: Default::default(),
            external_transport: bundled_xpc || cfg!(windows),
            binary,
            process: None,
            next_id: 0,
            policy: CorpusPolicy::default(),
            timeout: Duration::from_secs(12),
            memory_limit: 384 * 1024 * 1024,
            external_memory_limit: 2 * 1024 * 1024 * 1024,
            active_external: false,
            external_timeout: Duration::from_secs(45),
            slot,
            bundled_xpc,
            statistics: BrokerStatistics::default(),
        }
    }
    /// Names the confinement the worker actually has, never the one intended.
    /// A build without a kernel-enforced ceiling must not read like one that
    /// has it, so the Job Object appears only while a worker is inside it.
    pub fn transport(&self) -> &'static str {
        if self.bundled_xpc {
            return "XPC / App Sandbox · R0";
        }
        match self.process.as_ref().map(|p| &p.owner) {
            Some(Owner::Pipe(_, isolation)) if isolation.kernel_enforced() => {
                "LPAC senza capacità + Job Object · pipe"
            }
            _ => "Processo su pipe · corpus R0",
        }
    }
    pub fn set_raw_engine(&mut self, engine: tr_core::decoder::RawEngine) {
        self.raw_engine = engine;
    }
    pub fn statistics(&self) -> &BrokerStatistics {
        &self.statistics
    }
    /// Internal qualification only; may only lower production limits.
    pub fn tighten_limits_for_probe(&mut self, timeout: Duration, memory_bytes: u64) {
        self.timeout = timeout.min(Duration::from_secs(12));
        self.external_timeout = timeout.min(Duration::from_secs(45));
        self.external_memory_limit = memory_bytes.min(2 * 1024 * 1024 * 1024);
        self.memory_limit = memory_bytes.min(384 * 1024 * 1024);
    }
    pub fn suspend_for_probe(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(Process {
            owner: Owner::Xpc(lease),
            ..
        }) = self.process.as_mut()
        {
            return lease.suspend_for_test();
        }
        bail!("La sospensione di prova richiede un isolato XPC attivo")
    }
    pub fn inject_memory_growth_for_probe(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(Process {
            owner: Owner::Xpc(lease),
            ..
        }) = self.process.as_mut()
        {
            return lease.inject_growth_for_test();
        }
        bail!("La prova memoria richiede un servizio XPC di qualification")
    }
    pub fn recycle(&mut self) {
        self.process.take();
    }
    pub fn supervise_idle(&mut self) -> Result<()> {
        if let Err(error) = self.observe_memory() {
            self.process.take();
            self.statistics.forced_stops += 1;
            return Err(error);
        }
        Ok(())
    }
    fn observe_memory(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(Process {
            owner: Owner::Xpc(lease),
            ..
        }) = self.process.as_ref()
        {
            let (rss, footprint, pid) = lease.usage()?;
            self.statistics.worker_pid = Some(pid);
            self.statistics.peak_rss_bytes = self.statistics.peak_rss_bytes.max(rss);
            self.statistics.peak_footprint_bytes =
                self.statistics.peak_footprint_bytes.max(footprint);
            ensure!(
                rss.max(footprint)
                    <= (if self.active_external {
                        self.external_memory_limit
                    } else {
                        self.memory_limit
                    }),
                "Quota memoria XPC superata: servizio interrotto"
            );
        }
        Ok(())
    }
    fn spawn(&self, remaining: Duration) -> Result<Process> {
        ensure!(
            self.slot < 2 && !remaining.is_zero(),
            "Configurazione isolato fuori limite"
        );
        type Input = Box<dyn Write + Send>;
        type Output = Box<dyn Read + Send>;
        let (owner, input, output): (Owner, Input, Output) = if self.bundled_xpc {
            #[cfg(target_os = "macos")]
            {
                let (lease, input, output) = xpc::Session::open(self.slot, remaining)?;
                (Owner::Xpc(lease), Box::new(input), Box::new(output))
            }
            #[cfg(not(target_os = "macos"))]
            {
                bail!("XPC non disponibile su questo target");
            }
        } else {
            // Built before the child so the ceiling is ready to apply the
            // moment it exists, and so a failure here starts nothing at all.
            let isolation = Isolation::bounded(if self.active_external {
                self.external_memory_limit
            } else {
                self.memory_limit
            })
            .context("Limiti del worker")?;
            let mut worker = isolation.spawn(&self.binary, &std::env::temp_dir())?;
            let (input, output) = worker.take_pipes()?;
            (Owner::Pipe(worker, isolation), input, output)
        };
        let mut input = BufWriter::new(input);
        let mut output = BufReader::new(output);
        let (tx, rx) = mpsc::sync_channel::<Work>(1);
        thread::spawn(move || {
            while let Ok(work) = rx.recv() {
                let result = (|| {
                    protocol::write_control(
                        &mut input,
                        protocol::REQUEST,
                        work.id,
                        &DecodeRequest {
                            raw_engine: work.raw_engine,
                            source_len: work.bytes.len(),
                            max_edge: work.edge,
                            intent: work.intent,
                            edit: work.edit,
                            maximum_output_bytes: work.maximum_output_bytes,
                        },
                    )?;
                    input.write_all(&work.bytes)?;
                    input.flush()?;
                    // The worker owns its private copy now. In a full decode no
                    // other owner needs the host's compressed snapshot. A probe
                    // deliberately retains the caller's Arc for the later decode.
                    drop(work.bytes);
                    let (kind, id, data) = protocol::read_control(&mut output)?;
                    ensure!(id == work.id, "Risposta IPC tardiva o request ID errato");
                    if kind == protocol::ERROR {
                        return Err(SourceRejected(protocol::parse::<String>(&data)?).into());
                    }
                    ensure!(kind == protocol::RESPONSE, "Tipo di risposta IPC inatteso");
                    if let protocol::DecodeIntent::ScientificSample { x, y } = work.intent {
                        let sample: tr_core::science::Sample = protocol::parse(&data)?;
                        ensure!(
                            sample.x == x
                                && sample.y == y
                                && sample.hdu < 256
                                && sample.stored.is_none_or(f64::is_finite)
                                && sample.physical.is_none_or(f64::is_finite)
                                && ["valid", "BLANK", "NaN", "+Inf", "-Inf", "scaling overflow"]
                                    .contains(&sample.validity.as_str())
                                && (sample.validity == "valid") == sample.physical.is_some(),
                            "Campione scientifico IPC incoerente"
                        );
                        return Ok(WorkerOutput::ScientificSample(sample));
                    }
                    if let protocol::DecodeIntent::Export(options) = work.intent {
                        let info: tr_core::export::Info = protocol::parse(&data)?;
                        info.validate(options, work.maximum_output_bytes)?;
                        let mut bytes = vec![0; usize::try_from(info.bytes)?];
                        for chunk in bytes.chunks_mut(64 * 1024) {
                            output.read_exact(chunk)?;
                        }
                        return Ok(WorkerOutput::Export(info, bytes));
                    }
                    let info: RasterInfo = protocol::parse(&data)?;
                    protocol::validate_info(&info)?;
                    if matches!(work.intent, protocol::DecodeIntent::ReferenceMip { .. }) {
                        let (size, base) = protocol::mip_geometry(
                            [info.source_width, info.source_height],
                            work.edge,
                        );
                        ensure!(
                            size == [info.width, info.height]
                                && info.reference_mip.is_some_and(|m| m.base == base),
                            "Il worker non ha restituito il livello di riferimento richiesto"
                        );
                    } else {
                        ensure!(
                            info.reference_mip.is_none(),
                            "Livello di riferimento non richiesto"
                        );
                    }
                    if work.edge > 0 {
                        ensure!(
                            info.width.max(info.height) <= work.edge,
                            "Il worker ha superato il budget richiesto"
                        );
                    } else {
                        ensure!(
                            info.width == info.source_width && info.height == info.source_height,
                            "Il worker ha restituito una risoluzione ridotta al posto dell'originale"
                        );
                    }
                    ensure!(
                        info.width as u64 * info.height as u64 * 16 <= work.maximum_output_bytes
                            || work.intent == protocol::DecodeIntent::Probe,
                        "Output oltre prenotazione"
                    );
                    let raster = if work.intent == protocol::DecodeIntent::Probe {
                        None
                    } else {
                        Some(protocol::read_raster(&mut output, &info)?)
                    };
                    Ok(WorkerOutput::Raster(Box::new(info), raster))
                })();
                let failed = result
                    .as_ref()
                    .is_err_and(|error| error.downcast_ref::<SourceRejected>().is_none());
                let _ = work.result.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Process {
            owner,
            work: tx,
            jobs: 0,
        })
    }
    pub fn decode(&mut self, path: &Path, expected_digest: &str, edge: u32) -> Result<Decoded> {
        self.decode_cancellable(path, expected_digest, edge, || false)
    }
    pub fn decode_cancellable(
        &mut self,
        path: &Path,
        expected_digest: &str,
        edge: u32,
        cancelled: impl Fn() -> bool,
    ) -> Result<Decoded> {
        let source = self.prepare_snapshot(path, expected_digest, &cancelled)?;
        self.decode_snapshot_cancellable(source, edge, cancelled)
    }
    pub fn prepare_snapshot(
        &self,
        path: &Path,
        expected_digest: &str,
        cancelled: impl Fn() -> bool,
    ) -> Result<SourceSnapshot> {
        self.prepare_snapshot_bounded(path, expected_digest, MAX_SOURCE as u64, &cancelled)
    }
    pub fn prepare_snapshot_bounded(
        &self,
        path: &Path,
        expected_digest: &str,
        maximum_bytes: u64,
        cancelled: &impl Fn() -> bool,
    ) -> Result<SourceSnapshot> {
        ensure!(!cancelled(), "Decodifica annullata");
        let before = observation_token(path, &path.metadata()?);
        let (bytes, digest) = snapshot_bounded(path, maximum_bytes, cancelled)?;
        ensure!(!cancelled(), "Decodifica annullata");
        ensure!(
            digest == expected_digest
                || (self.external_transport
                    && expected_digest.starts_with("unverified:")
                    && expected_digest == before
                    && observation_token(path, &path.metadata()?) == before),
            "Sorgente cambiata: ricaricare la cartella"
        );
        ensure!(
            self.policy.approves(&digest) || self.external_transport,
            "File esterni: serve un worker isolato; questo trasporto accetta solo il corpus R0"
        );
        Ok(SourceSnapshot {
            bytes: Arc::new(bytes),
            digest,
            requested_identity: expected_digest.into(),
        })
    }
    pub fn decode_snapshot_cancellable(
        &mut self,
        source: SourceSnapshot,
        edge: u32,
        cancelled: impl Fn() -> bool,
    ) -> Result<Decoded> {
        self.decode_snapshot_bounded(source, edge, protocol::MAX_SOURCE as u64 * 4, cancelled)
    }
    pub fn decode_snapshot_bounded(
        &mut self,
        source: SourceSnapshot,
        edge: u32,
        maximum_output_bytes: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<Decoded> {
        let digest = source.digest.clone();
        let intent = if edge == 0 {
            protocol::DecodeIntent::FullSource
        } else {
            protocol::DecodeIntent::LegacyRaster
        };
        let (info, raster) = self
            .process_snapshot(source, edge, intent, None, maximum_output_bytes, cancelled)?
            .raster()?;
        Ok(Decoded {
            info,
            raster: raster.context("Pixel assenti")?,
            digest,
            transport: self.transport(),
            worker_pid: self.statistics.worker_pid,
        })
    }
    pub fn decode_reference_snapshot_bounded(
        &mut self,
        source: SourceSnapshot,
        maximum_edge: u32,
        maximum_output_bytes: u64,
        cpu_threads: usize,
        cancelled: impl Fn() -> bool,
    ) -> Result<Decoded> {
        let digest = source.digest.clone();
        let (info, raster) = self
            .process_snapshot(
                source,
                maximum_edge,
                protocol::DecodeIntent::ReferenceMip { cpu_threads },
                None,
                maximum_output_bytes,
                cancelled,
            )?
            .raster()?;
        Ok(Decoded {
            info,
            raster: raster.context("Pixel assenti")?,
            digest,
            transport: self.transport(),
            worker_pid: self.statistics.worker_pid,
        })
    }
    pub fn probe_snapshot(
        &mut self,
        source: &SourceSnapshot,
        cancelled: impl Fn() -> bool,
    ) -> Result<RasterInfo> {
        Ok(self
            .process_snapshot(
                source.clone(),
                0,
                protocol::DecodeIntent::Probe,
                None,
                0,
                cancelled,
            )?
            .raster()?
            .0)
    }
    pub fn export_snapshot_bounded(
        &mut self,
        source: SourceSnapshot,
        options: tr_core::export::Options,
        maximum_output_bytes: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<(tr_core::export::Info, Vec<u8>)> {
        self.export_edited_snapshot_bounded(source, options, None, maximum_output_bytes, cancelled)
    }
    pub fn export_edited_snapshot_bounded(
        &mut self,
        source: SourceSnapshot,
        options: tr_core::export::Options,
        edit: Option<tr_core::editing::EditRecipe>,
        maximum_output_bytes: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<(tr_core::export::Info, Vec<u8>)> {
        options.validate()?;
        if let Some(recipe) = &edit {
            recipe.validate()?;
        }
        match self.process_snapshot(
            source,
            0,
            protocol::DecodeIntent::Export(options),
            edit,
            maximum_output_bytes,
            cancelled,
        )? {
            WorkerOutput::Export(info, bytes) => Ok((info, bytes)),
            _ => anyhow::bail!("Output inatteso durante export"),
        }
    }
    pub fn scientific_sample(
        &mut self,
        source: SourceSnapshot,
        x: u32,
        y: u32,
        cancelled: impl Fn() -> bool,
    ) -> Result<tr_core::science::Sample> {
        match self.process_snapshot(
            source,
            0,
            protocol::DecodeIntent::ScientificSample { x, y },
            None,
            0,
            cancelled,
        )? {
            WorkerOutput::ScientificSample(sample) => Ok(sample),
            _ => anyhow::bail!("Campione scientifico assente"),
        }
    }
    fn process_snapshot(
        &mut self,
        source: SourceSnapshot,
        edge: u32,
        intent: protocol::DecodeIntent,
        edit: Option<tr_core::editing::EditRecipe>,
        maximum_output_bytes: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<WorkerOutput> {
        ensure!(!cancelled(), "Decodifica annullata");
        let SourceSnapshot { bytes, digest, .. } = source;
        let external = !self.policy.approves(&digest);
        // A snapshot may have been prepared by another broker. Recheck this transport's gate.
        ensure!(
            !external || self.external_transport,
            "File esterni: serve un worker isolato; questo trasporto accetta solo il corpus R0"
        );
        // Windows fixes the quota on the Job Object at process creation.
        // Revoke the old process before changing authority/resource domain.
        if self.active_external != external {
            self.recycle();
        }
        self.active_external = external;
        let timeout = if external {
            self.external_timeout
        } else {
            self.timeout
        };
        let reference = matches!(intent, protocol::DecodeIntent::ReferenceMip { .. });
        ensure!(
            edge <= if reference { 8192 } else { 2048 },
            "Dimensione anteprima fuori quota"
        );
        let started = Instant::now();
        if self.process.as_ref().is_some_and(|p| p.jobs >= 32) {
            self.process.take();
        }
        if self.process.is_none() {
            self.process = Some(self.spawn(timeout)?);
            self.statistics.starts += 1;
        }
        self.next_id = self.next_id.checked_add(1).context("Request ID esauriti")?;
        let (tx, rx) = mpsc::sync_channel(1);
        let process = self.process.as_mut().context("Worker assente")?;
        process.jobs += 1;
        if process
            .work
            .send(Work {
                raw_engine: self.raw_engine,
                id: self.next_id,
                bytes,
                edge,
                intent,
                edit,
                maximum_output_bytes,
                result: tx,
            })
            .is_err()
        {
            self.process.take();
            bail!("Worker terminato; riprovare la decodifica");
        }
        loop {
            if cancelled() {
                self.process.take();
                self.statistics.forced_stops += 1;
                bail!("Decodifica annullata");
            }
            if let Err(error) = self.observe_memory() {
                self.process.take();
                self.statistics.forced_stops += 1;
                return Err(error);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                self.process.take();
                self.statistics.forced_stops += 1;
                bail!("Worker interrotto: timeout assoluto");
            }
            match rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(Ok(output)) => {
                    if let Err(error) = self.observe_memory() {
                        self.process.take();
                        self.statistics.forced_stops += 1;
                        return Err(error);
                    }
                    if intent != protocol::DecodeIntent::Probe {
                        self.statistics.completed_jobs += 1;
                    }
                    return Ok(output);
                }
                Ok(Err(error)) => {
                    if error.downcast_ref::<SourceRejected>().is_none() {
                        self.process.take();
                    }
                    return Err(error);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.process.take();
                    bail!("Worker interrotto: arresto del processo")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    #[ignore = "requires TR_WORKER_BINARY; exercises AppContainer launches"]
    fn switching_trust_recreates_worker_with_matching_job_quota() {
        let binary = PathBuf::from(std::env::var_os("TR_WORKER_BINARY").expect("TR_WORKER_BINARY"));
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/05_Trasparenza.png");
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("external.png");
        let mut bytes = std::fs::read(&corpus).unwrap();
        bytes.extend_from_slice(b"external test: distinct digest");
        std::fs::write(&external, bytes).unwrap();
        let mut broker = Broker::new(binary);
        for (index, path) in [&corpus, &external, &corpus].into_iter().enumerate() {
            let (_, digest) = snapshot(path).unwrap();
            broker.decode(path, &digest, 32).unwrap();
            assert_eq!(broker.statistics.starts, index as u64 + 1);
            let Owner::Pipe(_, isolation) = &broker.process.as_ref().unwrap().owner;
            assert_eq!(
                isolation.limit_bytes(),
                if index == 1 {
                    2 * 1024 * 1024 * 1024
                } else {
                    384 * 1024 * 1024
                }
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn observation_detects_same_size_write_with_restored_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.bin");
        std::fs::write(&path, b"before").unwrap();
        let metadata = path.metadata().unwrap();
        let before = observation_token(&path, &metadata);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, b"after!").unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
            .unwrap();
        let after = path.metadata().unwrap();
        assert_eq!(metadata.len(), after.len());
        assert_eq!(metadata.modified().unwrap(), after.modified().unwrap());
        assert_ne!(before, observation_token(&path, &after));
    }
    #[test]
    fn accelerated_sha256_matches_known_vectors_and_chunk_boundaries() {
        for (bytes, expected) in [
            (
                Vec::new(),
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc".to_vec(),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                vec![b'a'; 1_000_000],
                "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            ),
        ] {
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected);
            for chunk_size in [1, 63, 64, 65, 4096] {
                let mut hash = Sha256::new();
                for chunk in bytes.chunks(chunk_size) {
                    hash.update(chunk);
                }
                assert_eq!(format!("{:x}", hash.finalize()), expected);
            }
        }
    }
    #[test]
    fn recipe_source_matches_only_the_identity_validated_for_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.bin");
        std::fs::write(&path, b"original").unwrap();
        let token = observation_token(&path, &path.metadata().unwrap());
        let mut broker = Broker::new("/not/a/worker".into());
        broker.external_transport = true;
        let source = broker.prepare_snapshot(&path, &token, || false).unwrap();
        assert!(source.matches_recipe_source(&token));
        assert!(source.matches_recipe_source(source.digest()));
        assert!(!source.matches_recipe_source("unverified:another-source"));
        assert!(!source.matches_recipe_source(&"0".repeat(64)));
        let by_hash = broker
            .prepare_snapshot(&path, source.digest(), || false)
            .unwrap();
        assert!(
            !by_hash.matches_recipe_source(&token),
            "No unchecked observation alias"
        );
        let replacement = dir.path().join("replacement.bin");
        std::fs::write(&replacement, b"replaced").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert!(broker.prepare_snapshot(&path, &token, || false).is_err());
        assert!(
            broker
                .prepare_snapshot(&path, source.digest(), || false)
                .is_err()
        );
        assert!(
            source.matches_recipe_source(&token),
            "Already frozen bytes remain immutable"
        );
        broker.external_transport = false;
        let token = observation_token(&path, &path.metadata().unwrap());
        assert!(broker.prepare_snapshot(&path, &token, || false).is_err());
    }
    #[test]
    /// A snapshot prepared by a permitted transport carries no permission with
    /// it. The receiving broker rechecks its own gate, so a transport without
    /// isolation still refuses the source another one was allowed to read.
    fn snapshot_from_external_broker_cannot_bypass_pipe_gate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("untrusted.png");
        std::fs::write(&path, b"external source").unwrap();
        let (_, digest) = snapshot(&path).unwrap();
        let mut external = Broker::new("/not/a/worker".into());
        external.bundled_xpc = true;
        external.external_transport = true;
        let source = external.prepare_snapshot(&path, &digest, || false).unwrap();
        let mut pipe = Broker::new("/not/a/worker".into());
        pipe.external_transport = false;
        assert!(
            pipe.decode_snapshot_cancellable(source, 0, || false)
                .unwrap_err()
                .to_string()
                .contains("corpus R0")
        );
        assert!(pipe.process.is_none());
    }
    /// Quality `Piena` asks for the source at its own resolution, which is the
    /// one request that carries a full frame across the pipe. It is the path a
    /// reduced preview never exercises, so it gets its own check against a real
    /// camera file named by `TR_RAW_SAMPLE`.
    #[test]
    #[ignore = "requires TR_WORKER_BINARY and TR_RAW_SAMPLE"]
    fn full_quality_delivers_the_source_at_its_own_resolution() {
        let Some(binary) = std::env::var_os("TR_WORKER_BINARY").map(PathBuf::from) else {
            eprintln!("TR_WORKER_BINARY non impostata");
            return;
        };
        let Some(folder) = std::env::var_os("TR_RAW_SAMPLE").map(PathBuf::from) else {
            eprintln!("TR_RAW_SAMPLE non impostata");
            return;
        };
        let path = std::fs::read_dir(&folder)
            .expect("cartella campioni")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nef") || e.eq_ignore_ascii_case("dng"))
            })
            .expect("nessun RAW nella cartella");
        let mut broker = Broker::new(binary);
        let (_, digest) = snapshot(&path).unwrap();
        let source = broker
            .prepare_snapshot(&path, &digest, || false)
            .expect("snapshot");
        // Edge zero is exactly what `prepare_cached` sends for `Piena`.
        let decoded = broker
            .decode_snapshot_cancellable(source, 0, || false)
            .expect("decodifica a qualità piena");
        assert_eq!(
            [decoded.raster.width, decoded.raster.height],
            [decoded.info.source_width, decoded.info.source_height],
            "la qualità piena non ha consegnato la risoluzione della sorgente"
        );
        assert!(decoded.info.format.starts_with("RAW"), "{:?}", decoded.info);
    }

    #[test]
    #[ignore = "requires cargo build --workspace; scripts/verify.sh runs this explicitly"]
    fn cache_miss_decodes_captured_bytes_without_reopening_source() {
        let binary = std::env::var_os("TR_WORKER_BINARY")
            .map(PathBuf::from)
            .expect("TR_WORKER_BINARY");
        let corpus =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/01_Studio_cromatico.png");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("changing.png");
        std::fs::copy(corpus, &path).unwrap();
        let (_, digest) = snapshot(&path).unwrap();
        let mut broker = Broker::new(binary);
        let source = broker.prepare_snapshot(&path, &digest, || false).unwrap();
        std::fs::write(&path, b"changed after cache lookup").unwrap();
        let decoded = broker
            .decode_snapshot_cancellable(source, 320, || false)
            .unwrap();
        assert_eq!(decoded.digest, digest);
        assert_eq!(decoded.raster.width, 320);
        assert!(broker.prepare_snapshot(&path, &digest, || false).is_err());
        assert_eq!(broker.statistics.completed_jobs, 1);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires cargo build --workspace and LPAC worker"]
    fn non_bayer_raw_preview_survives_real_worker_validation() {
        let binary = std::env::var_os("TR_WORKER_BINARY")
            .map(PathBuf::from)
            .expect("TR_WORKER_BINARY");
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("synthetic.dng");
        let bytes = include_bytes!("../../tr-worker/tests/fixtures/non-bayer-preview.dng");
        std::fs::write(&path, bytes).unwrap();
        let (_, digest) = snapshot(&path).unwrap();
        let mut broker = Broker::new(binary);
        broker.set_raw_engine(tr_core::decoder::RawEngine::LibRawBilinear);
        let source = broker.prepare_snapshot(&path, &digest, || false).unwrap();
        let probed = broker.probe_snapshot(&source, || false).unwrap();
        assert_eq!((probed.width, probed.height), (8, 8));
        for edge in [0, 4] {
            let decoded = broker.decode(&path, &digest, edge).unwrap();
            assert_eq!(decoded.info.format, "RAW · anteprima");
            assert!(decoded.info.decoder.contains("CFA non Bayer"));
            assert_eq!(
                (decoded.info.source_width, decoded.info.source_height),
                (8, 8)
            );
            assert_eq!(decoded.raster.width, if edge == 0 { 8 } else { 4 });
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires cargo build --workspace and LPAC worker"]
    fn secondary_ifd_orientation_does_not_change_worker_preview() {
        let binary = std::env::var_os("TR_WORKER_BINARY")
            .map(PathBuf::from)
            .expect("TR_WORKER_BINARY");
        let folder = tempfile::tempdir().unwrap();
        let mut broker = Broker::new(binary);
        broker.set_raw_engine(tr_core::decoder::RawEngine::LibRawBilinear);
        let mut previous = None;
        for (name, bytes) in [
            (
                "a.dng",
                include_bytes!("../../tr-worker/tests/fixtures/second-ifd-orientation-1.dng")
                    .as_slice(),
            ),
            (
                "b.dng",
                include_bytes!("../../tr-worker/tests/fixtures/second-ifd-orientation-6.dng")
                    .as_slice(),
            ),
        ] {
            let path = folder.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            let (_, digest) = snapshot(&path).unwrap();
            let decoded = broker.decode(&path, &digest, 0).unwrap();
            assert_eq!((decoded.raster.width, decoded.raster.height), (8, 4));
            if let Some(pixels) = previous {
                assert_eq!(decoded.raster.pixels, pixels);
            }
            previous = Some(decoded.raster.pixels);
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }
    #[test]
    #[cfg(unix)]
    fn absolute_timeout_kills_stalled_process() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("stalled-worker");
        std::fs::write(&executable, "#!/bin/sh\nexec /bin/sleep 5\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/01_Studio_cromatico.png");
        let (_, digest) = snapshot(&path).unwrap();
        let mut broker = Broker::new(executable);
        broker.timeout = Duration::from_millis(150);
        let started = std::time::Instant::now();
        assert!(broker.decode(&path, &digest, 320).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(broker.process.is_none());
    }

    #[test]
    #[ignore = "requires cargo build --workspace; scripts/verify.sh runs this explicitly"]
    fn real_worker_is_recycled_after_crash() {
        let binary = std::env::var_os("TR_WORKER_BINARY")
            .map(PathBuf::from)
            .expect("TR_WORKER_BINARY");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/01_Studio_cromatico.png");
        let (_, digest) = snapshot(&path).unwrap();
        let mut broker = Broker::new(binary);
        assert_eq!(
            broker.decode(&path, &digest, 320).unwrap().raster.width,
            320
        );
        match &mut broker.process.as_mut().unwrap().owner {
            Owner::Pipe(worker, _) => worker.terminate(),
            #[cfg(target_os = "macos")]
            Owner::Xpc(_) => panic!("expected pipe worker in the pipe crash/recovery test"),
        }
        assert!(broker.decode(&path, &digest, 320).is_err());
        assert!(broker.process.is_none());
        assert_eq!(
            broker.decode(&path, &digest, 320).unwrap().raster.width,
            320
        );
    }
    #[test]
    /// A transport with no isolation behind it refuses anything outside the
    /// corpus, and refuses it before a worker is ever started. Where external
    /// decoding is permitted the source is accepted, and the failure that
    /// follows is the missing binary, never the gate.
    fn unknown_input_never_starts_a_worker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"not approved").unwrap();
        let (_, digest) = snapshot(&path).unwrap();
        let mut broker = Broker::new("/not/a/worker".into());
        let refusal = broker.decode(&path, &digest, 320).unwrap_err().to_string();
        if broker.external_transport {
            assert!(!refusal.contains("corpus R0"), "{refusal}");
        } else {
            assert!(refusal.contains("corpus R0"), "{refusal}");
        }
        assert!(broker.process.is_none());
    }
    #[test]
    fn source_change_is_detected_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"bytes").unwrap();
        let mut broker = Broker::new("/not/a/worker".into());
        assert!(
            broker
                .decode(&path, "old", 320)
                .unwrap_err()
                .to_string()
                .contains("cambiata")
        );
    }
}
pub mod filesystem;
