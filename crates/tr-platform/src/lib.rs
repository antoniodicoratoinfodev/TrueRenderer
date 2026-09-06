//! R0 private-pipe protocol, with native XPC leases in a macOS app bundle.
//! The compiled corpus gate remains mandatory until complete OS qualification.
#[cfg(target_os = "macos")]
mod xpc;
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub use tr_core::corpus::CorpusPolicy;
use tr_core::{
    color::LinearImage,
    protocol::{self, DecodeRequest, MAX_SOURCE, RasterInfo},
};

pub fn snapshot(path: &Path) -> Result<(Vec<u8>, String)> {
    let file = File::open(path).context("Apertura in sola lettura")?;
    ensure!(
        file.metadata()?.is_file(),
        "La sorgente non è un file regolare"
    );
    ensure!(
        file.metadata()?.len() <= MAX_SOURCE as u64,
        "Limite R0: sorgenti fino a 32 MiB"
    );
    let mut bytes = vec![];
    file.take(MAX_SOURCE as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_SOURCE,
        "La sorgente è cresciuta oltre la quota"
    );
    let digest = format!("{:x}", Sha256::digest(&bytes));
    // Hash applies to the exact private bytes sent to the worker, never to a later reopening.
    Ok((bytes, digest))
}
pub fn supported_extension(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()).is_some_and(|x| {
        matches!(
            x.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "tif" | "tiff"
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
struct Work {
    id: u64,
    bytes: Vec<u8>,
    edge: u32,
    result: mpsc::SyncSender<Result<(RasterInfo, LinearImage)>>,
}
enum Owner {
    Pipe(Child),
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
            Owner::Pipe(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
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
    binary: PathBuf,
    process: Option<Process>,
    next_id: u64,
    policy: CorpusPolicy,
    timeout: Duration,
    memory_limit: u64,
    slot: u32,
    bundled_xpc: bool,
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
            binary,
            process: None,
            next_id: 0,
            policy: CorpusPolicy::default(),
            timeout: Duration::from_secs(12),
            memory_limit: 384 * 1024 * 1024,
            slot,
            bundled_xpc,
            statistics: BrokerStatistics::default(),
        }
    }
    pub fn transport(&self) -> &'static str {
        if self.bundled_xpc {
            "XPC / App Sandbox · R0"
        } else {
            "Processo su pipe · corpus R0"
        }
    }
    pub fn statistics(&self) -> &BrokerStatistics {
        &self.statistics
    }
    /// Internal qualification only; may only lower production limits.
    pub fn tighten_limits_for_probe(&mut self, timeout: Duration, memory_bytes: u64) {
        self.timeout = timeout.min(Duration::from_secs(12));
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
                rss.max(footprint) <= self.memory_limit,
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
            let mut child = Command::new(&self.binary)
                .env_clear()
                .current_dir(std::env::temp_dir())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("Avvio tr-worker; compilare entrambi i binari")?;
            let input = Box::new(child.stdin.take().context("stdin worker")?);
            let output = Box::new(child.stdout.take().context("stdout worker")?);
            (Owner::Pipe(child), input, output)
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
                            source_len: work.bytes.len(),
                            max_edge: work.edge,
                        },
                    )?;
                    input.write_all(&work.bytes)?;
                    input.flush()?;
                    let (kind, id, data) = protocol::read_control(&mut output)?;
                    ensure!(id == work.id, "Risposta IPC tardiva o request ID errato");
                    if kind == protocol::ERROR {
                        bail!("{}", protocol::parse::<String>(&data)?);
                    }
                    ensure!(kind == protocol::RESPONSE, "Tipo di risposta IPC inatteso");
                    let info: RasterInfo = protocol::parse(&data)?;
                    protocol::validate_info(&info)?;
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
                    let raster = protocol::read_raster(&mut output, &info)?;
                    Ok((info, raster))
                })();
                let failed = result.is_err();
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
        ensure!(!cancelled(), "Decodifica annullata");
        let (bytes, digest) = snapshot(path)?;
        ensure!(
            digest == expected_digest,
            "Sorgente cambiata: ricaricare la cartella"
        );
        ensure!(
            self.policy.approves(&digest),
            "Anteprima non disponibile: questo file non appartiene al corpus R0. La sandbox OS deve essere qualificata prima degli archivi esterni."
        );
        ensure!(edge <= 2048, "Dimensione anteprima fuori quota");
        let started = Instant::now();
        if self.process.as_ref().is_some_and(|p| p.jobs >= 32) {
            self.process.take();
        }
        if self.process.is_none() {
            self.process = Some(self.spawn(self.timeout)?);
            self.statistics.starts += 1;
        }
        self.next_id = self.next_id.checked_add(1).context("Request ID esauriti")?;
        let (tx, rx) = mpsc::sync_channel(1);
        let process = self.process.as_mut().context("Worker assente")?;
        process.jobs += 1;
        if process
            .work
            .send(Work {
                id: self.next_id,
                bytes,
                edge,
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
            let remaining = self.timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                self.process.take();
                self.statistics.forced_stops += 1;
                bail!("Worker interrotto: timeout assoluto");
            }
            match rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(Ok((info, raster))) => {
                    if let Err(error) = self.observe_memory() {
                        self.process.take();
                        self.statistics.forced_stops += 1;
                        return Err(error);
                    }
                    self.statistics.completed_jobs += 1;
                    return Ok(Decoded {
                        info,
                        raster,
                        digest,
                        transport: self.transport(),
                        worker_pid: self.statistics.worker_pid,
                    });
                }
                Ok(Err(error)) => {
                    self.process.take();
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
        let Owner::Pipe(child) = &mut broker.process.as_mut().unwrap().owner else {
            panic!("expected pipe worker")
        };
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(broker.decode(&path, &digest, 320).is_err());
        assert!(broker.process.is_none());
        assert_eq!(
            broker.decode(&path, &digest, 320).unwrap().raster.width,
            320
        );
    }
    #[test]
    fn unknown_input_never_starts_a_worker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"not approved").unwrap();
        let (_, digest) = snapshot(&path).unwrap();
        let mut broker = Broker::new("/not/a/worker".into());
        assert!(
            broker
                .decode(&path, &digest, 320)
                .unwrap_err()
                .to_string()
                .contains("corpus R0")
        );
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
