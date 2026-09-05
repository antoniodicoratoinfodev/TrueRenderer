//! R0 process boundary: private pipe copies, limited messages, absolute timeout.
//! No OS sandbox is claimed. Compiled corpus digest allowlist is mandatory.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use tr_core::{
    color::LinearImage,
    protocol::{self, DecodeRequest, MAX_SOURCE, RasterInfo},
};

#[derive(Deserialize)]
struct Manifest {
    images: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    sha256: String,
}
pub struct CorpusPolicy {
    approved: HashSet<String>,
}
impl Default for CorpusPolicy {
    fn default() -> Self {
        let manifest: Manifest =
            serde_json::from_str(include_str!("../../../corpus/manifest.json"))
                .expect("Trusted built-in corpus manifest");
        Self {
            approved: manifest.images.into_iter().map(|i| i.sha256).collect(),
        }
    }
}
impl CorpusPolicy {
    pub fn approves(&self, digest: &str) -> bool {
        self.approved.contains(digest)
    }
}
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
}
struct Work {
    id: u64,
    bytes: Vec<u8>,
    edge: u32,
    result: mpsc::SyncSender<Result<(RasterInfo, LinearImage)>>,
}
struct Process {
    child: Child,
    work: mpsc::SyncSender<Work>,
    jobs: u32,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub struct Broker {
    binary: PathBuf,
    process: Option<Process>,
    next_id: u64,
    policy: CorpusPolicy,
    timeout: Duration,
}
impl Broker {
    pub fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            process: None,
            next_id: 0,
            policy: CorpusPolicy::default(),
            timeout: Duration::from_secs(12),
        }
    }
    fn spawn(&self) -> Result<Process> {
        let mut child = Command::new(&self.binary)
            .env_clear()
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Avvio tr-worker; compilare entrambi i binari")?;
        let mut input = BufWriter::new(child.stdin.take().context("stdin worker")?);
        let mut output = BufReader::new(child.stdout.take().context("stdout worker")?);
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
            child,
            work: tx,
            jobs: 0,
        })
    }
    pub fn decode(&mut self, path: &Path, expected_digest: &str, edge: u32) -> Result<Decoded> {
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
        if self.process.as_ref().is_some_and(|p| p.jobs >= 32) {
            self.process.take();
        }
        if self.process.is_none() {
            self.process = Some(self.spawn()?);
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
        match rx.recv_timeout(self.timeout) {
            Ok(Ok((info, raster))) => Ok(Decoded {
                info,
                raster,
                digest,
            }),
            Ok(Err(error)) => {
                self.process.take();
                Err(error)
            }
            Err(_) => {
                self.process.take();
                bail!("Worker interrotto: timeout assoluto o arresto del processo")
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
        broker.process.as_mut().unwrap().child.kill().unwrap();
        broker.process.as_mut().unwrap().child.wait().unwrap();
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
