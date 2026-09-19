//! Two bounded I/O slots shared by navigation, tree listings and photo scans.
//! Workers own no database and are detached on shutdown: a stuck NAS syscall
//! must not keep the annotation writer or application shutdown waiting.
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tr_app::browser::{Entry, Kind, natural_cmp};
use tr_core::budget::MemoryBudget;

pub const MAX_ENTRIES: usize = 100_000;
const MAX_PATH: usize = 32768;
#[derive(Clone)]
pub struct Client(Arc<Shared>);
struct Shared {
    queue: Mutex<VecDeque<Job>>,
    changed: Condvar,
    stopped: AtomicBool,
    active: Mutex<Vec<(Key, Arc<AtomicBool>)>>,
}
#[derive(Clone, PartialEq, Eq)]
enum Key {
    Resolve,
    Scan,
    List(PathBuf),
    Locations,
    Reveal,
}
enum Work {
    Resolve {
        id: u64,
        path: PathBuf,
    },
    List {
        path: PathBuf,
        epoch: u64,
        hidden: bool,
    },
    Scan {
        path: PathBuf,
        id: u64,
        hidden: bool,
        corpus: PathBuf,
        external: bool,
    },
    Locations,
    Reveal(PathBuf),
}
struct Job {
    key: Key,
    work: Work,
    cancelled: Arc<AtomicBool>,
}
pub enum BrowserEvent {
    Resolved {
        id: u64,
        result: Result<(PathBuf, Option<PathBuf>), String>,
    },
    Listing {
        path: PathBuf,
        epoch: u64,
        entries: Vec<Arc<Entry>>,
        result: Option<Result<(), String>>,
    },
    Locations(Vec<PathBuf>),
    Revealed(Result<(), String>),
}
pub struct Observation {
    pub path: PathBuf,
    pub bytes: u64,
    pub digest: String,
    pub observation: String,
    pub approved: bool,
}
pub enum ScanEvent {
    Batch {
        id: u64,
        observations: Vec<Observation>,
    },
    Finished {
        id: u64,
        result: Result<(), String>,
    },
}
pub struct Filesystem {
    pub client: Client,
    pub events: mpsc::Receiver<BrowserEvent>,
    _credit: Arc<Option<tr_core::budget::Lease>>,
}
impl Filesystem {
    pub fn start(
        wake: crate::wake::Wake,
        budget: MemoryBudget,
    ) -> (Self, mpsc::Receiver<ScanEvent>) {
        let credit = Arc::new(budget.try_reserve(64 * 1024 * 1024));
        let budget = MemoryBudget::new(if credit.is_some() {
            40 * 1024 * 1024
        } else {
            0
        });
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            changed: Condvar::new(),
            stopped: AtomicBool::new(false),
            active: Mutex::new(vec![]),
        });
        let (tx, events) = mpsc::sync_channel(4);
        let (scan_tx, scans) = mpsc::sync_channel(4);
        for _ in 0..2 {
            let shared = shared.clone();
            let tx = tx.clone();
            let scan_tx = scan_tx.clone();
            let wake = wake.clone();
            let budget = budget.clone();
            let credit = credit.clone();
            std::thread::spawn(move || {
                loop {
                    let _keep_credit_until_io_returns = &credit;
                    let job = {
                        let mut queue = shared.queue.lock().unwrap();
                        while queue.is_empty() && !shared.stopped.load(Ordering::Acquire) {
                            queue = shared.changed.wait(queue).unwrap();
                        }
                        if shared.stopped.load(Ordering::Acquire) {
                            return;
                        }
                        let job = queue.pop_front().unwrap();
                        shared
                            .active
                            .lock()
                            .unwrap()
                            .push((job.key.clone(), job.cancelled.clone()));
                        job
                    };
                    let cancelled = || {
                        job.cancelled.load(Ordering::Acquire)
                            || shared.stopped.load(Ordering::Acquire)
                    };
                    if !cancelled() {
                        match job.work {
                            Work::Resolve { id, path } => {
                                let result = resolve(&path).map_err(|e| e.to_string());
                                send(
                                    &tx,
                                    BrowserEvent::Resolved { id, result },
                                    &cancelled,
                                    &wake,
                                );
                            }
                            Work::List {
                                path,
                                epoch,
                                hidden,
                            } => {
                                let mut entries = Vec::new();
                                let mut batch = Vec::new();
                                let mut batch_bytes = 0;
                                let result = enumerate(&path, hidden, &cancelled, |entry, kind| {
                                    let path = entry.path();
                                    let size = path.as_os_str().len().saturating_mul(8) + 1024;
                                    let credit =
                                        budget.try_reserve(size as u64).ok_or_else(|| {
                                            "Elenco parziale: limite memoria".to_string()
                                        })?;
                                    let item = Arc::new(Entry {
                                        name: entry.file_name().to_string_lossy().into(),
                                        path,
                                        kind,
                                        credit: Some(Arc::new(credit)),
                                    });
                                    batch_bytes += size;
                                    entries.push(item.clone());
                                    batch.push(item);
                                    if batch.len() >= 256 || batch_bytes >= 128 * 1024 {
                                        if !send(
                                            &tx,
                                            BrowserEvent::Listing {
                                                path: path_parent(&entries),
                                                epoch,
                                                entries: std::mem::take(&mut batch),
                                                result: None,
                                            },
                                            &cancelled,
                                            &wake,
                                        ) {
                                            return Err("Cancelled".into());
                                        }
                                        batch_bytes = 0;
                                    }
                                    Ok(())
                                });
                                if !cancelled() {
                                    entries.sort_unstable_by(|a, b| {
                                        (b.kind == Kind::Directory)
                                            .cmp(&(a.kind == Kind::Directory))
                                            .then_with(|| natural_cmp(&a.name, &b.name))
                                            .then_with(|| a.path.cmp(&b.path))
                                    });
                                    send(
                                        &tx,
                                        BrowserEvent::Listing {
                                            path,
                                            epoch,
                                            entries,
                                            result: Some(result),
                                        },
                                        &cancelled,
                                        &wake,
                                    );
                                }
                            }
                            Work::Scan {
                                path,
                                id,
                                hidden,
                                corpus,
                                external,
                            } => {
                                let controlled = path == corpus;
                                let mut observations = Vec::new();
                                let mut bytes = 0;
                                let mut total_bytes = 0;
                                let result = enumerate(&path, hidden, &cancelled, |entry, kind| {
                                    if kind != Kind::Image {
                                        return Ok(());
                                    }
                                    total_bytes +=
                                        entry.path().as_os_str().len().saturating_mul(8) + 2048;
                                    if credit.is_none() || total_bytes > 20 * 1024 * 1024 {
                                        return Err("Elenco parziale: limite memoria".into());
                                    }
                                    let path = entry.path();
                                    let metadata = entry.metadata().map_err(|e| e.to_string())?;
                                    let observation =
                                        tr_platform::observation_token(&path, &metadata);
                                    let (digest, approved) = if controlled {
                                        let (_, hash) = tr_platform::snapshot(&path)
                                            .map_err(|e| e.to_string())?;
                                        let approved =
                                            tr_platform::CorpusPolicy::default().approves(&hash);
                                        (hash, approved)
                                    } else {
                                        (
                                            observation.clone(),
                                            external
                                                && metadata.len()
                                                    <= tr_core::protocol::MAX_SOURCE as u64,
                                        )
                                    };
                                    bytes += path.as_os_str().len() * 2 + 512;
                                    observations.push(Observation {
                                        path,
                                        bytes: metadata.len(),
                                        digest,
                                        observation,
                                        approved,
                                    });
                                    if observations.len() >= 16 || bytes >= 128 * 1024 {
                                        if !send(
                                            &scan_tx,
                                            ScanEvent::Batch {
                                                id,
                                                observations: std::mem::take(&mut observations),
                                            },
                                            &cancelled,
                                            &wake,
                                        ) {
                                            return Err("Cancelled".into());
                                        }
                                        bytes = 0;
                                    }
                                    Ok(())
                                });
                                if !observations.is_empty() {
                                    send(
                                        &scan_tx,
                                        ScanEvent::Batch { id, observations },
                                        &cancelled,
                                        &wake,
                                    );
                                }
                                send(
                                    &scan_tx,
                                    ScanEvent::Finished { id, result },
                                    &cancelled,
                                    &wake,
                                );
                            }
                            Work::Locations => {
                                send(
                                    &tx,
                                    BrowserEvent::Locations(tr_platform::filesystem::locations()),
                                    &cancelled,
                                    &wake,
                                );
                            }
                            Work::Reveal(path) => {
                                send(
                                    &tx,
                                    BrowserEvent::Revealed(
                                        tr_platform::filesystem::reveal(&path)
                                            .map_err(|e| e.to_string()),
                                    ),
                                    &cancelled,
                                    &wake,
                                );
                            }
                        }
                    }
                    shared
                        .active
                        .lock()
                        .unwrap()
                        .retain(|(_, token)| !Arc::ptr_eq(token, &job.cancelled));
                }
            });
        }
        (
            Self {
                client: Client(shared),
                events,
                _credit: credit,
            },
            scans,
        )
    }
}
fn path_parent(entries: &[Arc<Entry>]) -> PathBuf {
    entries.last().unwrap().path.parent().unwrap().into()
}
fn send<T>(
    tx: &mpsc::SyncSender<T>,
    mut event: T,
    cancelled: &impl Fn() -> bool,
    wake: &crate::wake::Wake,
) -> bool {
    loop {
        if cancelled() {
            return false;
        }
        match tx.try_send(event) {
            Ok(()) => {
                wake.request_repaint();
                return true;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
            Err(mpsc::TrySendError::Full(value)) => {
                event = value;
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
fn resolve(path: &std::path::Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    if path.as_os_str().len() > MAX_PATH {
        return Err("Location length limit".into());
    }
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    let metadata = resolved.symlink_metadata().map_err(|e| e.to_string())?;
    let (folder, photo) = if metadata.is_dir() {
        if tr_platform::filesystem::opaque(&resolved, &metadata) {
            return Err("Pacchetto o placeholder: aprire nel sistema".into());
        }
        (resolved, None)
    } else if metadata.is_file()
        && tr_platform::supported_extension(&resolved)
        && !tr_platform::filesystem::opaque(&resolved, &metadata)
    {
        (
            resolved.parent().ok_or("No parent")?.to_path_buf(),
            Some(resolved),
        )
    } else {
        return Err("Anteprima non disponibile per questo tipo di file".into());
    };
    std::fs::read_dir(&folder).map_err(|e| e.to_string())?;
    Ok((folder, photo))
}
fn enumerate(
    path: &std::path::Path,
    hidden: bool,
    cancelled: &impl Fn() -> bool,
    mut visit: impl FnMut(std::fs::DirEntry, Kind) -> Result<(), String>,
) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || tr_platform::filesystem::opaque(path, &metadata)
    {
        return Err("Collegamento, pacchetto o posizione non disponibile".into());
    }
    let mut errors = 0;
    for (count, result) in std::fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if cancelled() {
            return Err("Cancelled".into());
        }
        if count >= MAX_ENTRIES {
            return Err("Elenco parziale: limite di 100.000 voci".into());
        }
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        let name = entry.file_name();
        if name == ".truerenderer-cache" || (!hidden && name.to_string_lossy().starts_with('.')) {
            continue;
        }
        if entry.path().as_os_str().len() > MAX_PATH {
            errors += 1;
            continue;
        }
        let metadata = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        if !hidden && tr_platform::filesystem::hidden(&metadata) {
            continue;
        }
        let kind =
            if metadata.file_type().is_symlink() || tr_platform::filesystem::reparse(&metadata) {
                Kind::Link
            } else if tr_platform::filesystem::opaque(&entry.path(), &metadata) {
                Kind::Package
            } else if metadata.is_dir() {
                Kind::Directory
            } else if !metadata.is_file() {
                Kind::Special
            } else if tr_platform::supported_extension(&entry.path()) {
                Kind::Image
            } else {
                Kind::File
            };
        visit(entry, kind)?;
    }
    if errors > 0 {
        Err(format!("Elenco parziale: {errors} voci non leggibili"))
    } else {
        Ok(())
    }
}
impl Client {
    fn submit(&self, key: Key, work: Work, priority: bool) -> bool {
        let mut queue = self.0.queue.lock().unwrap();
        queue.retain(|job| job.key != key);
        if queue.len() >= 64 || self.0.stopped.load(Ordering::Acquire) {
            return false;
        }
        for (active, token) in self.0.active.lock().unwrap().iter() {
            if *active == key {
                token.store(true, Ordering::Release);
            }
        }
        let job = Job {
            key,
            work,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        if priority {
            queue.push_front(job);
        } else {
            queue.push_back(job);
        }
        self.0.changed.notify_one();
        true
    }
    pub fn resolve(&self, id: u64, path: PathBuf) -> bool {
        self.submit(Key::Resolve, Work::Resolve { id, path }, true)
    }
    pub fn list(&self, path: PathBuf, epoch: u64, hidden: bool) -> bool {
        self.submit(
            Key::List(path.clone()),
            Work::List {
                path,
                epoch,
                hidden,
            },
            false,
        )
    }
    pub fn scan(
        &self,
        path: PathBuf,
        id: u64,
        hidden: bool,
        corpus: PathBuf,
        external: bool,
    ) -> bool {
        self.submit(
            Key::Scan,
            Work::Scan {
                path,
                id,
                hidden,
                corpus,
                external,
            },
            true,
        )
    }
    pub fn locations(&self) {
        self.submit(Key::Locations, Work::Locations, false);
    }
    pub fn reveal(&self, path: PathBuf) {
        self.submit(Key::Reveal, Work::Reveal(path), false);
    }
    pub fn cancel_list(&self, path: PathBuf) {
        self.cancel(Key::List(path));
    }
    pub fn cancel_scan(&self) {
        self.cancel(Key::Scan);
    }
    fn cancel(&self, key: Key) {
        self.0.queue.lock().unwrap().retain(|job| job.key != key);
        for (active, token) in self.0.active.lock().unwrap().iter() {
            if *active == key {
                token.store(true, Ordering::Release);
            }
        }
    }
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
        self.0.queue.lock().unwrap().clear();
        self.0.changed.notify_all();
    }
}
impl Drop for Filesystem {
    fn drop(&mut self) {
        self.client.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_listing_is_lazy_bounded_and_never_opens_special_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("folder")).unwrap();
        std::fs::create_dir(directory.path().join(".truerenderer-cache")).unwrap();
        for name in ["img10.png", "img2.png", "notes.txt", ".hidden.png"] {
            std::fs::write(directory.path().join(name), b"not an image").unwrap();
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.path(), directory.path().join("cycle")).unwrap();
            use std::os::unix::ffi::OsStrExt;
            let path =
                std::ffi::CString::new(directory.path().join("pipe.png").as_os_str().as_bytes())
                    .unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        }
        let mut entries = Vec::new();
        enumerate(directory.path(), false, &|| false, |e, kind| {
            entries.push((e.file_name(), kind));
            Ok(())
        })
        .unwrap();
        assert!(
            entries
                .iter()
                .any(|(name, kind)| name == "notes.txt" && *kind == Kind::File)
        );
        assert!(
            entries
                .iter()
                .any(|(name, kind)| name == "img2.png" && *kind == Kind::Image)
        );
        assert!(
            !entries
                .iter()
                .any(|(name, _)| name == ".hidden.png" || name == ".truerenderer-cache")
        );
        #[cfg(unix)]
        {
            assert!(
                entries
                    .iter()
                    .any(|(name, kind)| name == "cycle" && *kind == Kind::Link)
            );
            assert!(
                entries
                    .iter()
                    .any(|(name, kind)| name == "pipe.png" && *kind == Kind::Special)
            );
            assert!(resolve(&directory.path().join("pipe.png")).is_err());
            assert!(
                enumerate(
                    &directory.path().join("cycle"),
                    false,
                    &|| false,
                    |_, _| panic!()
                )
                .is_err()
            );
        }
        assert!(!directory.path().join("library.sqlite").exists());
        assert_eq!(
            std::fs::read_dir(directory.path().join(".truerenderer-cache"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn listing_streams_sorted_terminal_and_releases_budget_on_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        for n in 0..600 {
            std::fs::write(directory.path().join(format!("file{n}.txt")), []).unwrap();
        }
        let budget = MemoryBudget::new(128 * 1024 * 1024);
        let (fs, _) = Filesystem::start(
            crate::wake::Wake::from(eframe::egui::Context::default()),
            budget.clone(),
        );
        assert!(fs.client.list(directory.path().into(), 1, false));
        let mut batches = 0;
        loop {
            match fs.events.recv_timeout(Duration::from_secs(5)).unwrap() {
                BrowserEvent::Listing {
                    entries,
                    result: None,
                    ..
                } => {
                    assert!(entries.len() <= 256);
                    batches += 1;
                }
                BrowserEvent::Listing {
                    entries,
                    result: Some(result),
                    ..
                } => {
                    result.unwrap();
                    assert_eq!(entries.len(), 600);
                    assert_eq!(entries[2].name, "file2.txt");
                    break;
                }
                _ => panic!(),
            }
        }
        assert!(batches >= 2);
        assert!(!directory.path().join(".truerenderer-cache").exists());
        drop(fs);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while budget.usage().reserved > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(budget.usage().reserved, 0);
    }
    #[test]
    fn saturated_queue_and_cancel_remain_bounded() {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            changed: Condvar::new(),
            stopped: AtomicBool::new(false),
            active: Mutex::new(vec![]),
        });
        let client = Client(shared.clone());
        for n in 0..64 {
            assert!(client.list(PathBuf::from(format!("/synthetic/{n}")), n, false));
        }
        assert!(!client.list("/overflow".into(), 65, false));
        assert!(client.list("/synthetic/0".into(), 100, false));
        assert_eq!(shared.queue.lock().unwrap().len(), 64);
        client.cancel_list("/synthetic/1".into());
        assert_eq!(shared.queue.lock().unwrap().len(), 63);
        client.stop();
        assert!(shared.queue.lock().unwrap().is_empty());
    }
}
