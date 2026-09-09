//! Poll only requested/resident sources off the UI and catalog threads.
//! Metadata is a change signal; the broker still hashes the private snapshot.
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub id: String,
    pub path: PathBuf,
    pub observation: String,
}
#[derive(Clone, Debug)]
pub struct Change {
    pub id: String,
    pub observation: String,
    pub bytes: u64,
    pub available: bool,
}
pub fn inspect(watch: &Watch) -> Option<Change> {
    let metadata = std::fs::metadata(&watch.path).ok().filter(|m| m.is_file());
    let observation = metadata
        .as_ref()
        .map(tr_platform::observation_token)
        .unwrap_or_default();
    (observation != watch.observation).then(|| Change {
        id: watch.id.clone(),
        observation,
        bytes: metadata.as_ref().map_or(0, |m| m.len()),
        available: metadata.is_some(),
    })
}
#[derive(Default)]
struct State {
    generation: u64,
    watches: Vec<Watch>,
    stop: bool,
}
pub struct Monitor {
    state: Arc<(Mutex<State>, Condvar)>,
    pub changes: mpsc::Receiver<(u64, Vec<Change>)>,
    worker: Option<JoinHandle<()>>,
}
impl Monitor {
    pub fn new(ctx: impl Into<crate::wake::Wake>) -> Self {
        let ctx = ctx.into();
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let shared = state.clone();
        let (tx, changes) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            loop {
                let (lock, wake) = &*shared;
                let state = lock.lock().unwrap();
                let (state, _) = wake
                    .wait_timeout(state, Duration::from_millis(500))
                    .unwrap();
                if state.stop {
                    break;
                }
                let generation = state.generation;
                let watches = state.watches.clone();
                drop(state);
                let changed: Vec<_> = watches.iter().filter_map(inspect).collect();
                if !changed.is_empty() && tx.try_send((generation, changed)).is_ok() {
                    ctx.request_repaint();
                }
            }
        });
        Self {
            state,
            changes,
            worker: Some(worker),
        }
    }
    pub fn watch(&self, generation: u64, mut watches: Vec<Watch>) {
        watches.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        let mut state = self.state.0.lock().unwrap();
        if state.generation != generation || state.watches != watches {
            state.generation = generation;
            state.watches = watches;
            self.state.1.notify_one();
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().stop = true;
        self.state.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_replacement_deletion_and_restoration_without_touching_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.raw");
        std::fs::write(&path, b"original").unwrap();
        let mut watch = Watch {
            id: "asset".into(),
            path: path.clone(),
            observation: tr_platform::observation_token(&path.metadata().unwrap()),
        };
        assert!(inspect(&watch).is_none());
        let replacement = dir.path().join("replacement");
        std::fs::write(&replacement, b"modified").unwrap();
        let modified = path.metadata().unwrap().modified().unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        let changed = inspect(&watch).expect("replacement with same size/mtime");
        assert!(changed.available);
        watch.observation = changed.observation;
        assert!(inspect(&watch).is_none());
        std::fs::remove_file(&path).unwrap();
        let missing = inspect(&watch).unwrap();
        assert!(!missing.available);
        watch.observation = missing.observation;
        assert!(inspect(&watch).is_none());
        std::fs::write(&path, b"restored").unwrap();
        assert!(inspect(&watch).unwrap().available);
    }
    #[test]
    fn monitor_reports_generation_and_shuts_down_with_full_result_queue() {
        let dir = tempfile::tempdir().unwrap();
        let monitor = Monitor::new(eframe::egui::Context::default());
        monitor.watch(
            7,
            vec![Watch {
                id: "missing".into(),
                path: dir.path().join("gone"),
                observation: "previous".into(),
            }],
        );
        let (generation, changes) = monitor
            .changes
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(generation, 7);
        assert!(!changes[0].available);
        std::thread::sleep(Duration::from_millis(600));
        drop(monitor);
    }
}
