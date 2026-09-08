use crate::{
    cache::{Lookup, writer::Writer},
    service::{Event, PreviewDecoded},
};
use eframe::egui;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tr_core::{
    Item,
    budget::Lease,
    preview::{PreviewPriority, PreviewRequest},
    provider::ImageLevels,
};
use tr_platform::{Broker, SourceSnapshot};

#[derive(Clone)]
pub struct Job {
    pub item: Item,
    pub request: PreviewRequest,
    pub priority: PreviewPriority,
    pub generation: u64,
}
impl Job {
    fn key(&self) -> (String, PreviewRequest, u64) {
        (self.item.id.clone(), self.request, self.generation)
    }
}
struct Ready {
    job: Job,
    source: SourceSnapshot,
    _snapshot: Lease,
    _snapshot_lane: Lease,
}
#[derive(Default)]
struct Queues {
    lookup: VecDeque<Job>,
    decode: VecDeque<Ready>,
    pending: HashMap<(String, PreviewRequest, u64), PreviewPriority>,
    wanted: Option<HashSet<(String, PreviewRequest, u64)>>,
    deferred: Vec<Job>,
    active: HashSet<(String, u64)>,
    claimed: HashSet<(String, PreviewRequest, u64)>,
}
pub struct DecodePool {
    queues: Arc<(Mutex<Queues>, Condvar)>,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}
fn insert(queue: &mut VecDeque<Job>, job: Job) {
    let position = queue
        .iter()
        .position(|queued| queued.priority > job.priority)
        .unwrap_or(queue.len());
    queue.insert(position, job);
}
fn insert_ready(queue: &mut VecDeque<Ready>, ready: Ready) {
    let position = queue
        .iter()
        .position(|queued| queued.job.priority > ready.job.priority)
        .unwrap_or(queue.len());
    queue.insert(position, ready);
}

fn deliver(
    events: &mpsc::SyncSender<Event>,
    mut event: Event,
    ctx: &egui::Context,
    stop: &AtomicBool,
) -> bool {
    loop {
        match events.try_send(event) {
            Ok(()) => {
                ctx.request_repaint();
                return true;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
            Err(mpsc::TrySendError::Full(e)) => {
                if stop.load(Ordering::Acquire) {
                    return false;
                }
                event = e;
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
fn reserve(
    cache: &crate::cache::Manager,
    bytes: u64,
    events: &mpsc::SyncSender<Event>,
    ctx: &egui::Context,
    cancelled: &impl Fn() -> bool,
) -> anyhow::Result<Lease> {
    let start = Instant::now();
    loop {
        if let Some(lease) = cache.memory.try_reserve(bytes) {
            return Ok(lease);
        }
        if start.elapsed() > Duration::from_secs(2)
            || bytes > cache.memory.usage().limit
            || cancelled()
        {
            anyhow::bail!(
                "Memoria richiesta {} MiB; limite {} MiB, occupata/prenotata {} MiB. Aumentare il limite o liberare le viste.",
                bytes.div_ceil(1024 * 1024),
                cache.memory.usage().limit / (1024 * 1024),
                cache.memory.usage().reserved / (1024 * 1024)
            );
        }
        if start.elapsed() < Duration::from_millis(25) {
            let _ = events.try_send(Event::MemoryPressure);
            ctx.request_repaint();
        }
        thread::sleep(Duration::from_millis(25));
    }
}
impl DecodePool {
    pub fn set_demand(&self, wanted: &[(String, PreviewRequest)], generation: u64) {
        let mut q = self.queues.0.lock().unwrap();
        let wanted: HashSet<_> = wanted.iter().cloned().collect();
        q.wanted = Some(
            wanted
                .iter()
                .map(|(id, request)| (id.clone(), *request, generation))
                .collect(),
        );
        let mut removed = Vec::new();
        q.lookup.retain(|job| {
            let keep = job.generation == generation
                && wanted.contains(&(job.item.id.clone(), job.request));
            if !keep {
                removed.push(job.key());
            }
            keep
        });
        q.decode.retain(|ready| {
            let job = &ready.job;
            let keep = job.generation == generation
                && wanted.contains(&(job.item.id.clone(), job.request));
            if !keep {
                removed.push(job.key());
            }
            keep
        });
        for key in removed {
            q.pending.remove(&key);
        }
        self.queues.1.notify_all();
    }
    pub fn take_deferred(&self) -> Vec<Job> {
        std::mem::take(&mut self.queues.0.lock().unwrap().deferred)
    }
    pub fn start(
        binary: PathBuf,
        generation: Arc<AtomicU64>,
        events: mpsc::SyncSender<Event>,
        ctx: egui::Context,
        cache: Arc<crate::cache::Manager>,
    ) -> Self {
        let queues = Arc::new((Mutex::new(Queues::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let writer = Arc::new(Writer::start(cache.clone(), generation.clone()));
        let mut workers = Vec::new();
        // One independent source/hash/cache lane: it never waits behind a RAW
        // decoder. Both decode services remain available for visible misses.
        {
            let queues = queues.clone();
            let stop = stop.clone();
            let generation = generation.clone();
            let events = events.clone();
            let ctx = ctx.clone();
            let cache = cache.clone();
            let binary = binary.clone();
            workers.push(thread::spawn(move || {
                let broker = Broker::new(binary);
                let snapshots = tr_core::budget::MemoryBudget::new(cache.memory.usage().limit / 3);
                loop {
                    let job = {
                        let mut q = queues.0.lock().unwrap();
                        while q.lookup.is_empty() && !stop.load(Ordering::Acquire) {
                            q = queues.1.wait(q).unwrap();
                        }
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        q.lookup.pop_front().unwrap()
                    };
                    if cache.under_pressure() && !job.priority.visible() {
                        queues.0.lock().unwrap().pending.remove(&job.key());
                        deliver(
                            &events,
                            Event::DecodeDeferred {
                                id: job.item.id,
                                request: job.request,
                                generation: job.generation,
                            },
                            &ctx,
                            &stop,
                        );
                        continue;
                    }
                    let cancelled = || {
                        stop.load(Ordering::Acquire)
                            || job.generation != generation.load(Ordering::Acquire)
                    };
                    let result = (|| -> anyhow::Result<Option<PreviewDecoded>> {
                        anyhow::ensure!(!cancelled(), "Richiesta sostituita");
                        job.request.validate()?;
                        let maximum = job.item.bytes.min(tr_core::protocol::MAX_SOURCE as u64);
                        let required = maximum.saturating_mul(2) + 1024 * 1024;
                        snapshots.configure(cache.memory.usage().limit / 3);
                        anyhow::ensure!(
                            required <= snapshots.usage().limit,
                            "Snapshot oltre quota: aumentare la memoria complessiva"
                        );
                        let Some(snapshot_lane) = snapshots.try_reserve(required) else {
                            queues.0.lock().unwrap().pending.remove(&job.key());
                            deliver(
                                &events,
                                Event::DecodeDeferred {
                                    id: job.item.id.clone(),
                                    request: job.request,
                                    generation: job.generation,
                                },
                                &ctx,
                                &stop,
                            );
                            return Ok(None);
                        };
                        let lease = reserve(
                            &cache,
                            maximum.saturating_mul(2) + 1024 * 1024,
                            &events,
                            &ctx,
                            &cancelled,
                        )?;
                        let source = broker.prepare_snapshot_bounded(
                            &job.item.path,
                            &job.item.digest,
                            maximum,
                            &cancelled,
                        )?;
                        let folder = job
                            .item
                            .path
                            .parent()
                            .ok_or_else(|| anyhow::anyhow!("Cartella assente"))?;
                        let reading = cache.read_demand();
                        let start = Instant::now();
                        loop {
                            match cache.load_preview(
                                folder,
                                source.digest(),
                                job.request,
                                &cache.memory,
                                &cancelled,
                            ) {
                                Lookup::Hit(info, prepared) => {
                                    return Ok(Some(PreviewDecoded {
                                        digest: source.digest().into(),
                                        info,
                                        prepared: *prepared,
                                        transport: "Cache disco · fp32 · Anteprima",
                                        worker_pid: None,
                                    }));
                                }
                                Lookup::Busy
                                    if start.elapsed() < Duration::from_millis(200)
                                        && !cancelled() =>
                                {
                                    thread::sleep(Duration::from_millis(20))
                                }
                                Lookup::Limited(error) => anyhow::bail!("{error}"),
                                _ => break,
                            }
                        }
                        drop(reading);
                        let mut q = queues.0.lock().unwrap();
                        if q.claimed.contains(&job.key()) {
                            return Ok(None);
                        }
                        if q.wanted
                            .as_ref()
                            .is_some_and(|wanted| !wanted.contains(&job.key()))
                        {
                            q.pending.remove(&job.key());
                            return Ok(None);
                        }
                        let ready = Ready {
                            job: Job {
                                priority: q
                                    .pending
                                    .get(&job.key())
                                    .copied()
                                    .unwrap_or(job.priority),
                                ..job.clone()
                            },
                            source,
                            _snapshot: lease,
                            _snapshot_lane: snapshot_lane,
                        };
                        insert_ready(&mut q.decode, ready);
                        queues.1.notify_all();
                        Ok(None)
                    })();
                    if matches!(result, Ok(None)) {
                        continue;
                    }
                    queues.0.lock().unwrap().pending.remove(&job.key());
                    if !cancelled() {
                        let result = result.map(|v| v.unwrap()).map_err(|e| format!("{e:#}"));
                        if !deliver(
                            &events,
                            Event::Image {
                                id: job.item.id,
                                request: job.request,
                                generation: job.generation,
                                result: Box::new(result),
                            },
                            &ctx,
                            &stop,
                        ) {
                            break;
                        }
                    }
                }
            }));
        }
        for slot in 0..2 {
            let queues = queues.clone();
            let stop = stop.clone();
            let generation = generation.clone();
            let events = events.clone();
            let ctx = ctx.clone();
            let cache = cache.clone();
            let binary = binary.clone();
            let writer = writer.clone();
            workers.push(thread::spawn(move || {
                let mut broker = Broker::with_slot(binary, slot);
                let mut last_generation = None;
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let current = generation.load(Ordering::Acquire);
                    if last_generation.is_some_and(|g| g != current) {
                        broker.recycle();
                        last_generation = None;
                    }
                    let ready = {
                        let mut q = queues.0.lock().unwrap();
                        if q.decode.is_empty() {
                            q = queues
                                .1
                                .wait_timeout(q, Duration::from_millis(25))
                                .unwrap()
                                .0;
                        }
                        let ready = q
                            .decode
                            .iter()
                            .position(|r| {
                                (slot == 0 || (r.job.priority.visible() && !cache.under_pressure()))
                                    && (!cache.under_pressure() || r.job.priority.visible())
                                    && !q
                                        .active
                                        .contains(&(r.job.item.id.clone(), r.job.generation))
                            })
                            .and_then(|i| q.decode.remove(i));
                        if let Some(r) = &ready {
                            q.active.insert((r.job.item.id.clone(), r.job.generation));
                        } else {
                            // Queued background or same-asset work may be ineligible
                            // for this lane. Wait rather than spinning on that queue.
                            drop(queues.1.wait_timeout(q, Duration::from_millis(25)).unwrap());
                        }
                        ready
                    };
                    let Some(Ready {
                        job,
                        source,
                        _snapshot,
                        _snapshot_lane,
                    }) = ready
                    else {
                        let _ = broker.supervise_idle();
                        continue;
                    };
                    let cancelled = || {
                        stop.load(Ordering::Acquire)
                            || job.generation != generation.load(Ordering::Acquire)
                    };
                    if cancelled() {
                        let mut q = queues.0.lock().unwrap();
                        q.pending.remove(&job.key());
                        q.active.remove(&(job.item.id.clone(), job.generation));
                        continue;
                    }
                    last_generation = Some(job.generation);
                    let result = (|| -> anyhow::Result<Vec<(Job, PreviewDecoded)>> {
                        // Parser state is an estimated allowance; OS footprint remains
                        // supervised independently and must be qualified on real RAWs.
                        let probe_lease =
                            reserve(&cache, 128 * 1024 * 1024, &events, &ctx, &cancelled)?;
                        let info = broker.probe_snapshot(&source, cancelled)?;
                        drop(probe_lease);
                        let pixels = info.width as u64 * info.height as u64;
                        // Native raster + host copy + native scratch + reference
                        // pyramid/filter intermediates. Snapshot credits are separate.
                        let mut lease = reserve(
                            &cache,
                            pixels * 64 + 128 * 1024 * 1024,
                            &events,
                            &ctx,
                            &cancelled,
                        )?;
                        let decoded =
                            broker.decode_snapshot_bounded(source, 0, pixels * 16, cancelled)?;
                        anyhow::ensure!(!cancelled(), "Richiesta sostituita");
                        // All current backends produce full source samples. Group
                        // compatible consumers before building the reference graph;
                        // a Standard reduced decoder must not enter this branch.
                        let consumers = {
                            let mut q = queues.0.lock().unwrap();
                            let mut consumers: Vec<_> = q
                                .pending
                                .keys()
                                .filter(|(id, _, g)| id == &job.item.id && *g == job.generation)
                                .filter(|key| {
                                    q.wanted.as_ref().is_none_or(|w| w.contains(*key))
                                        || **key == job.key()
                                })
                                .map(|key| Job {
                                    request: key.1,
                                    priority: q.pending[key],
                                    ..job.clone()
                                })
                                .collect();
                            consumers.sort_by_key(|job| job.priority);
                            for consumer in &consumers {
                                q.claimed.insert(consumer.key());
                            }
                            q.lookup.retain(|j| {
                                j.item.id != job.item.id || j.generation != job.generation
                            });
                            q.decode.retain(|r| {
                                r.job.item.id != job.item.id || r.job.generation != job.generation
                            });
                            consumers
                        };
                        cache.decoded(consumers.len(), broker.statistics());
                        let finest = consumers
                            .iter()
                            .map(|j| j.request)
                            .max_by_key(|r| r.maximum_level_edge())
                            .unwrap_or(job.request);
                        let mut image = ImageLevels::from_source(decoded.raster, finest)?;
                        image.attach_lease(lease.split(image.byte_len() as u64).unwrap());
                        let image = Arc::new(image);
                        let mut tails = std::collections::HashMap::new();
                        tails.insert(0, image.clone());
                        let mut results = Vec::new();
                        for consumer in consumers {
                            let base = image.requested_base(consumer.request);
                            let tail = if let Some(tail) = tails.get(&base) {
                                tail.clone()
                            } else {
                                let bytes = image.levels()[base..]
                                    .iter()
                                    .map(|l| l.pixels.len() as u64 * 16)
                                    .sum();
                                let credits = lease.split(bytes).ok_or_else(|| {
                                    anyhow::anyhow!("Derivati oltre prenotazione")
                                })?;
                                let tail = Arc::new(image.detach(consumer.request, credits)?);
                                tails.insert(base, tail.clone());
                                tail
                            };
                            let histogram = tail.source().histogram();
                            results.push((
                                consumer,
                                PreviewDecoded {
                                    digest: decoded.digest.clone(),
                                    info: decoded.info.clone(),
                                    prepared: tr_render::PreparedPreview {
                                        image: tail,
                                        histogram,
                                    },
                                    transport: decoded.transport,
                                    worker_pid: decoded.worker_pid,
                                },
                            ));
                        }
                        Ok(results)
                    })()
                    .map_err(|e| format!("{e:#}"));
                    drop(_snapshot);
                    drop(_snapshot_lane);
                    let results = match result {
                        Ok(results) => results
                            .into_iter()
                            .map(|(j, d)| (j, Ok(d)))
                            .collect::<Vec<_>>(),
                        Err(error) => {
                            let q = queues.0.lock().unwrap();
                            let mut jobs: Vec<_> = q
                                .claimed
                                .iter()
                                .filter(|(id, _, g)| id == &job.item.id && *g == job.generation)
                                .map(|(_, request, _)| Job {
                                    request: *request,
                                    ..job.clone()
                                })
                                .collect();
                            if jobs.is_empty() {
                                jobs.push(job.clone());
                            }
                            jobs.into_iter().map(|j| (j, Err(error.clone()))).collect()
                        }
                    };
                    {
                        let mut q = queues.0.lock().unwrap();
                        for (consumer, _) in &results {
                            q.pending.remove(&consumer.key());
                            q.claimed.remove(&consumer.key());
                        }
                        q.active.remove(&(job.item.id.clone(), job.generation));
                        queues.1.notify_all();
                    }
                    if cancelled() {
                        continue;
                    }
                    for (consumer, result) in results {
                        let persist = result.as_ref().ok().cloned();
                        if !deliver(
                            &events,
                            Event::Image {
                                id: consumer.item.id.clone(),
                                request: consumer.request,
                                generation: consumer.generation,
                                result: Box::new(result),
                            },
                            &ctx,
                            &stop,
                        ) {
                            break;
                        }
                        if let Some(decoded) = persist
                            && let Some(folder) = consumer.item.path.parent()
                        {
                            writer.submit(
                                folder.into(),
                                decoded.digest,
                                consumer.request,
                                decoded.info,
                                decoded.prepared,
                                consumer.generation,
                            );
                        }
                    }
                }
            }));
        }
        Self {
            queues,
            stop,
            workers,
        }
    }
    pub fn submit(&self, job: Job) -> Result<(), Box<Job>> {
        let mut q = self.queues.0.lock().unwrap();
        q.lookup.retain(|j| j.generation == job.generation);
        q.decode.retain(|j| j.job.generation == job.generation);
        q.pending.retain(|(_, _, g), _| *g == job.generation);
        if self.stop.load(Ordering::Acquire) {
            return Err(Box::new(job));
        }
        if let Some(priority) = q.pending.get_mut(&job.key()) {
            if job.priority < *priority {
                *priority = job.priority;
                if let Some(index) = q.lookup.iter().position(|j| j.key() == job.key()) {
                    q.lookup.remove(index);
                    insert(&mut q.lookup, job.clone());
                }
                if let Some(index) = q.decode.iter().position(|j| j.job.key() == job.key()) {
                    let mut ready = q.decode.remove(index).unwrap();
                    ready.job.priority = job.priority;
                    insert_ready(&mut q.decode, ready);
                }
                self.queues.1.notify_all();
            }
            return Ok(());
        }
        if q.pending.len() >= 64 {
            let worst = q
                .lookup
                .iter()
                .enumerate()
                .map(|(i, j)| (j.priority, false, i))
                .chain(
                    q.decode
                        .iter()
                        .enumerate()
                        .map(|(i, j)| (j.job.priority, true, i)),
                )
                .filter(|(p, _, _)| !p.visible() && *p > job.priority)
                .max_by_key(|(priority, _, _)| *priority);
            if job.priority.visible()
                && let Some((_, decoded, index)) = worst
            {
                let removed = if decoded {
                    q.decode.remove(index).unwrap().job
                } else {
                    q.lookup.remove(index).unwrap()
                };
                q.pending.remove(&removed.key());
                q.deferred.push(removed);
            } else {
                return Err(Box::new(job));
            }
        }
        q.pending.insert(job.key(), job.priority);
        insert(&mut q.lookup, job);
        self.queues.1.notify_all();
        Ok(())
    }
}
impl Drop for DecodePool {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.queues.1.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

pub fn prepare_cached(
    cache: &crate::cache::Manager,
    broker: &mut Broker,
    item: &Item,
    edge: u32,
    cancelled: &impl Fn() -> bool,
) -> anyhow::Result<crate::service::PreparedDecoded> {
    let folder = item
        .path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cartella sorgente assente"))?;
    let source = broker.prepare_snapshot(&item.path, &item.digest, cancelled)?;
    if edge == 0
        && cache.settings().enabled
        && let Some((_info, prepared)) = cache.load(folder, source.digest(), cancelled)
    {
        return Ok(crate::service::PreparedDecoded {
            prepared,
            transport: "Cache disco · fp32 · Anteprima",
        });
    }
    let decoded = broker.decode_snapshot_cancellable(source, edge, cancelled)?;
    let prepared = tr_render::prepare(decoded.raster)?;
    if edge == 0
        && let Err(e) = cache.store(folder, &decoded.digest, &decoded.info, &prepared, cancelled)
    {
        cache.note(
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
            {
                "Cache occupata: scrittura saltata, immagine disponibile".into()
            } else {
                format!("Cache saltata, immagine disponibile: {e:#}")
            },
        );
    }
    Ok(crate::service::PreparedDecoded {
        prepared,
        transport: decoded.transport,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::Command, time::Instant};

    #[test]
    fn priority_order_is_fifo_and_inflight_lookup_keeps_promotion() {
        let pool = DecodePool {
            queues: Arc::new((Mutex::new(Queues::default()), Condvar::new())),
            stop: Arc::new(AtomicBool::new(false)),
            workers: Vec::new(),
        };
        let priorities = [
            PreviewPriority::Background,
            PreviewPriority::NeighborPreview,
            PreviewPriority::SecondaryVisible,
            PreviewPriority::AdjacentRows,
            PreviewPriority::Refinement,
            PreviewPriority::Immediate,
            PreviewPriority::AheadRegion,
            PreviewPriority::SecondaryVisible,
        ];
        let mut jobs = Vec::new();
        for (id, priority) in priorities.into_iter().enumerate() {
            let job = Job {
                item: Item {
                    id: id.to_string(),
                    name: id.to_string(),
                    path: PathBuf::new(),
                    bytes: 1,
                    digest: "revision".into(),
                    approved: true,
                    annotation: Default::default(),
                    revision: 0,
                },
                request: PreviewRequest::full(),
                priority,
                generation: 1,
            };
            assert!(pool.submit(job.clone()).is_ok());
            jobs.push(job);
        }
        {
            let mut q = pool.queues.0.lock().unwrap();
            assert_eq!(
                q.lookup
                    .iter()
                    .map(|job| job.item.id.as_str())
                    .collect::<Vec<_>>(),
                ["5", "4", "6", "2", "7", "3", "1", "0"]
            );
            // Simulate a lookup that has left the queue but still owns pending work.
            q.lookup.pop_back();
        }
        jobs[0].priority = PreviewPriority::Immediate;
        assert!(pool.submit(jobs[0].clone()).is_ok());
        let q = pool.queues.0.lock().unwrap();
        assert_eq!(q.pending[&jobs[0].key()], PreviewPriority::Immediate);
        assert_eq!(q.lookup.len(), 7, "An active lookup must not be duplicated");
    }
    #[test]
    fn visible_work_promotes_deduplicates_and_displaces_queued_background() {
        let pool = DecodePool {
            queues: Arc::new((Mutex::new(Queues::default()), Condvar::new())),
            stop: Arc::new(AtomicBool::new(false)),
            workers: Vec::new(),
        };
        let job = |n: usize| Job {
            item: Item {
                id: n.to_string(),
                name: n.to_string(),
                path: PathBuf::new(),
                bytes: 1,
                digest: "revision".into(),
                approved: true,
                annotation: tr_core::Annotation::default(),
                revision: 0,
            },
            request: PreviewRequest::full(),
            priority: PreviewPriority::Background,
            generation: 1,
        };
        for n in 0..64 {
            assert!(pool.submit(job(n)).is_ok());
        }
        let mut promoted = job(40);
        promoted.priority = PreviewPriority::Immediate;
        assert!(pool.submit(promoted.clone()).is_ok());
        assert_eq!(pool.queues.0.lock().unwrap().pending.len(), 64);
        assert_eq!(pool.queues.0.lock().unwrap().lookup[0].item.id, "40");
        let mut visible = job(999);
        visible.priority = PreviewPriority::Immediate;
        assert!(pool.submit(visible.clone()).is_ok());
        assert_eq!(pool.take_deferred().len(), 1);
        pool.set_demand(&[(visible.item.id.clone(), visible.request)], 1);
        let q = pool.queues.0.lock().unwrap();
        assert_eq!(q.lookup.len(), 1);
        assert_eq!(q.pending.len(), 1);
        assert_eq!(q.lookup[0].item.id, "999");
    }

    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn one_source_decode_serves_full_standard_and_detached_thumbnail() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let binary = PathBuf::from(std::env::var_os("TR_WORKER_BINARY").unwrap());
        let path = root.join("corpus/04_Frequenze_radiali.png");
        let (bytes, digest) = tr_platform::snapshot(&path).unwrap();
        let item = Item {
            id: "shared-source".into(),
            name: "radial".into(),
            path,
            bytes: bytes.len() as u64,
            digest,
            approved: true,
            annotation: Default::default(),
            revision: 0,
        };
        let cache = Arc::new(crate::cache::Manager::new(crate::cache::Settings {
            enabled: false,
            ..Default::default()
        }));
        let (tx, rx) = mpsc::sync_channel(16);
        let pool = DecodePool::start(
            binary,
            Arc::new(AtomicU64::new(1)),
            tx,
            egui::Context::default(),
            cache.clone(),
        );
        let requests = [
            PreviewRequest::full(),
            PreviewRequest {
                quality: tr_core::preview::PreviewQuality::Standard,
                edge: 0,
            },
            PreviewRequest {
                quality: tr_core::preview::PreviewQuality::Full,
                edge: 64,
            },
        ];
        {
            // Make the simultaneous consumer set deterministic before releasing I/O.
            let mut q = pool.queues.0.lock().unwrap();
            for request in requests {
                let job = Job {
                    item: item.clone(),
                    request,
                    priority: PreviewPriority::Immediate,
                    generation: 1,
                };
                q.pending.insert(job.key(), job.priority);
                q.lookup.push_back(job);
            }
            pool.queues.1.notify_all();
        }
        let mut results = std::collections::HashMap::new();
        while results.len() < 3 {
            if let Event::Image {
                request, result, ..
            } = rx.recv_timeout(Duration::from_secs(20)).unwrap()
            {
                results.insert(request, (*result).unwrap());
            }
        }
        assert_eq!(cache.stats().decode_jobs, 1);
        assert_eq!(cache.stats().coalesced_consumers, 2);
        let small = results.remove(&requests[2]).unwrap();
        assert!(small.prepared.image.base_level() > 0);
        let region =
            tr_core::resample::Region::fitted(small.prepared.image.source_size(), [64, 43]);
        assert_eq!(
            small.prepared.image.render(region).unwrap().pixels,
            results[&requests[0]]
                .prepared
                .image
                .render(region)
                .unwrap()
                .pixels
        );
        results.clear();
        drop(pool);
        assert_eq!(
            cache.memory.usage().reserved,
            cache.baseline_bytes + small.prepared.image.byte_len() as u64
        );
        drop(small);
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
    }

    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn changing_domain_reaps_idle_decoder_without_a_new_job() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let binary = std::env::var_os("TR_WORKER_BINARY").expect("TR_WORKER_BINARY");
        let data = tempfile::tempdir().unwrap();
        let wrapper = data.path().join("observed-worker");
        let marker = data.path().join("pid");
        let quote =
            |path: &std::path::Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > {}\nexec {}\n",
                quote(&marker),
                quote(std::path::Path::new(&binary))
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let (events, received) = mpsc::sync_channel(4);
        let pool = DecodePool::start(
            wrapper,
            generation.clone(),
            events,
            egui::Context::default(),
            Arc::new(crate::cache::Manager::new(crate::cache::Settings {
                enabled: false,
                ..Default::default()
            })),
        );
        let path = root.join("corpus/01_Studio_cromatico.png");
        let (bytes, digest) = tr_platform::snapshot(&path).unwrap();
        assert!(
            pool.submit(Job {
                item: Item {
                    id: "idle-domain-test".into(),
                    path,
                    name: "Studio cromatico".into(),
                    bytes: bytes.len() as u64,
                    digest,
                    approved: true,
                    annotation: tr_core::Annotation::default(),
                    revision: 0,
                },
                request: PreviewRequest {
                    quality: tr_core::preview::PreviewQuality::Full,
                    edge: 320
                },
                priority: PreviewPriority::Background,
                generation: 1,
            })
            .is_ok()
        );
        let Event::Image { result, .. } = received.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected decoded corpus image")
        };
        assert!(result.is_ok());
        let pid: u32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
        // Read-only liveness observation of our fixture; no PID-only termination is used here.
        let alive = || {
            Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(
            alive(),
            "fixture must be idle and alive before changing the domain"
        );
        generation.store(2, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !alive(),
            "the old decoder retained authority without a new job"
        );
        drop(pool);
    }
}
