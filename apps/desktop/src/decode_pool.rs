use crate::{
    cache::{Lookup, writer::Writer},
    service::{Event, PreviewDecoded},
};
#[cfg(test)]
use eframe::egui;
use std::{
    cell::Cell,
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
impl Queues {
    fn wants_job(&self, job: &Job) -> bool {
        self.wanted.as_ref().is_none_or(|w| w.contains(&job.key()))
    }

    // Native development is full-frame: a new quality/edge for this source can
    // still share the in-flight decode, even if its original consumer left.
    fn wants_source(&self, job: &Job) -> bool {
        self.wanted.as_ref().is_none_or(|w| {
            w.iter()
                .any(|(id, _, generation)| id == &job.item.id && *generation == job.generation)
        })
    }
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
    ctx: &crate::wake::Wake,
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
    ctx: &crate::wake::Wake,
    cancelled: &impl Fn() -> bool,
) -> anyhow::Result<Lease> {
    let start = Instant::now();
    loop {
        anyhow::ensure!(!cancelled(), "Richiesta sostituita");
        if let Some(lease) = cache.memory.try_reserve(bytes) {
            return Ok(lease);
        }
        if start.elapsed() > Duration::from_secs(2) || bytes > cache.memory.usage().limit {
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

/// Peak of successive full-decode phases, excluding separately owned snapshots.
/// Native scratch remains a conservative estimate (32 bytes/pixel + 128 MiB),
/// not a kernel limit. The native handle is closed before raster transfer.
fn decode_working_bytes(width: u32, height: u32) -> anyhow::Result<u64> {
    let pixels = u64::from(width) * u64::from(height);
    anyhow::ensure!(
        width > 0 && height > 0 && pixels <= tr_core::color::MAX_PIXELS as u64,
        "Dimensioni del decode fuori quota"
    );
    let source = pixels * 16;
    // During development: native output + two raster-sized scratch allowances.
    // During transfer: native output + the host's private raster copy.
    let mut peak = source * 3;
    let (mut w, mut h) = (width, height);
    let mut retained = source;
    let mut all_tails = source;
    let mut level = 1;
    while w > 1 || h > 1 {
        let (next_w, next_h) = (w.div_ceil(2), h.div_ceil(2));
        let next = u64::from(next_w) * u64::from(next_h) * 16;
        let horizontal = u64::from(next_w) * u64::from(h) * 16;
        // Conservatively keep the worker output during host filtering too:
        // receiving the final bytes does not synchronize its destructor.
        peak = peak.max(source + retained + horizontal + next);
        retained += next;
        level += 1;
        // Every distinct mip tail can be requested; count each owned copy.
        // Exact ceil geometry also covers odd dimensions and 1-pixel strips.
        all_tails += next * level;
        (w, h) = (next_w, next_h);
    }
    peak = peak.max(source + all_tails);
    Ok(peak + 128 * 1024 * 1024)
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
        ctx: impl Into<crate::wake::Wake>,
        cache: Arc<crate::cache::Manager>,
    ) -> Self {
        let ctx = ctx.into();
        let queues = Arc::new((Mutex::new(Queues::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let decode_admission = Arc::new(Mutex::new(()));
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
                    let revoked = || {
                        stop.load(Ordering::Acquire)
                            || job.generation != generation.load(Ordering::Acquire)
                    };
                    let obsolete = Cell::new(false);
                    let cancelled = || {
                        obsolete.set(obsolete.get() || !queues.0.lock().unwrap().wants_job(&job));
                        revoked() || obsolete.get()
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
                        // Keep descriptors in the larger scheduling queue, not a
                        // backlog of full compressed snapshots that can prevent
                        // the next visible decode from ever acquiring its credits.
                        let deferred =
                            (q.decode.len() > 2).then(|| q.decode.pop_back().unwrap().job);
                        if let Some(job) = &deferred {
                            q.pending.remove(&job.key());
                        }
                        queues.1.notify_all();
                        drop(q);
                        if let Some(job) = deferred {
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
                        }
                        Ok(None)
                    })();
                    if matches!(result, Ok(None)) {
                        continue;
                    }
                    queues.0.lock().unwrap().pending.remove(&job.key());
                    if !revoked() && cancelled() {
                        // A quick return to this request must be retryable. Do
                        // not turn a cancelled lookup into a permanent UI error.
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
                    } else if !revoked() {
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
            let decode_admission = decode_admission.clone();
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
                    let revoked = || {
                        stop.load(Ordering::Acquire)
                            || job.generation != generation.load(Ordering::Acquire)
                    };
                    let obsolete = Cell::new(false);
                    let cancelled = || {
                        obsolete
                            .set(obsolete.get() || !queues.0.lock().unwrap().wants_source(&job));
                        revoked() || obsolete.get()
                    };
                    if revoked() {
                        let mut q = queues.0.lock().unwrap();
                        q.pending.remove(&job.key());
                        q.active.remove(&(job.item.id.clone(), job.generation));
                        continue;
                    }
                    last_generation = Some(job.generation);
                    let result = (|| -> anyhow::Result<Vec<(Job, PreviewDecoded)>> {
                        // Probe admissions also wait behind a large decode. Small
                        // jobs release this permit after probing and can overlap;
                        // jobs using over half the working budget remain serial.
                        // The hash/cache lane is independent of this permit.
                        let waiting = Instant::now();
                        let mut permit = Some(loop {
                            anyhow::ensure!(!cancelled(), "Richiesta sostituita");
                            match decode_admission.try_lock() {
                                Ok(permit) => break permit,
                                Err(std::sync::TryLockError::Poisoned(_)) => {
                                    anyhow::bail!("Ammissione decoder interrotta")
                                }
                                Err(std::sync::TryLockError::WouldBlock) => {}
                            }
                            anyhow::ensure!(
                                waiting.elapsed() < Duration::from_secs(90),
                                "Attesa decoder oltre il limite; riprovare la richiesta"
                            );
                            thread::sleep(Duration::from_millis(25));
                        });
                        // Parser state is an estimated allowance; OS footprint remains
                        // supervised independently and must be qualified on real RAWs.
                        let probe_lease =
                            reserve(&cache, 128 * 1024 * 1024, &events, &ctx, &cancelled)?;
                        // Only domain revocation/shutdown may terminate a native
                        // call. View changes cancel at boundaries, retaining all
                        // leases until the actual probe/decode has completed.
                        let info = broker.probe_snapshot(&source, revoked)?;
                        drop(probe_lease);
                        anyhow::ensure!(!cancelled(), "Richiesta sostituita");
                        let pixels = info.width as u64 * info.height as u64;
                        let working_bytes = decode_working_bytes(info.width, info.height)?;
                        if working_bytes
                            <= cache
                                .memory
                                .usage()
                                .limit
                                .saturating_sub(cache.baseline_bytes)
                                / 2
                        {
                            drop(permit.take());
                        }
                        // Reserve the peak of phases, not their sum. Pixel data,
                        // native scratch allowance and fp32 precision are unchanged.
                        let mut lease = reserve(&cache, working_bytes, &events, &ctx, &cancelled)?;
                        let decoded =
                            broker.decode_snapshot_bounded(source, 0, pixels * 16, revoked)?;
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
                                .filter(|key| q.wanted.as_ref().is_none_or(|w| w.contains(*key)))
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
                    let origin_delivered = results.iter().any(|(j, _)| j.key() == job.key());
                    {
                        let mut q = queues.0.lock().unwrap();
                        q.pending.remove(&job.key());
                        q.claimed.remove(&job.key());
                        for (consumer, _) in &results {
                            q.pending.remove(&consumer.key());
                            q.claimed.remove(&consumer.key());
                        }
                        q.active.remove(&(job.item.id.clone(), job.generation));
                        queues.1.notify_all();
                    }
                    if revoked() {
                        continue;
                    }
                    if !origin_delivered {
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
                    }
                    for (consumer, result) in results {
                        if obsolete.get() || !queues.0.lock().unwrap().wants_job(&consumer) {
                            deliver(
                                &events,
                                Event::DecodeDeferred {
                                    id: consumer.item.id,
                                    request: consumer.request,
                                    generation: consumer.generation,
                                },
                                &ctx,
                                &stop,
                            );
                            continue;
                        }
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
    #[test]
    fn cancelled_admission_does_not_acquire_even_available_credits() {
        let cache = crate::cache::Manager::new(crate::cache::Settings::default());
        let (tx, _rx) = mpsc::sync_channel(4);
        let wake = egui::Context::default().into();
        assert!(reserve(&cache, 1024, &tx, &wake, &|| true).is_err());
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
    }

    fn navigation_job(id: &str) -> Job {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("corpus/01_Studio_cromatico.png");
        let (bytes, digest) = tr_platform::snapshot(&path).unwrap();
        Job {
            item: Item {
                id: id.into(),
                name: "corpus".into(),
                path,
                bytes: bytes.len() as u64,
                digest,
                observation: String::new(),
                approved: true,
                annotation: Default::default(),
                revision: 0,
            },
            request: PreviewRequest {
                quality: tr_core::preview::PreviewQuality::Full,
                edge: 128,
            },
            priority: PreviewPriority::Background,
            generation: 1,
        }
    }

    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn navigation_cancels_inflight_lookup_before_memory_becomes_available() {
        let cache = Arc::new(crate::cache::Manager::new(crate::cache::Settings {
            enabled: false,
            ..Default::default()
        }));
        let held = cache
            .memory
            .try_reserve(cache.memory.usage().limit - cache.baseline_bytes)
            .unwrap();
        let (tx, rx) = mpsc::sync_channel(16);
        let pool = DecodePool::start(
            std::env::var_os("TR_WORKER_BINARY").unwrap().into(),
            Arc::new(AtomicU64::new(1)),
            tx,
            egui::Context::default(),
            cache.clone(),
        );
        let old = navigation_job("old");
        let mut next = navigation_job("next");
        next.priority = PreviewPriority::Immediate;
        assert!(pool.submit(old.clone()).is_ok());
        // This event proves the lookup left the queue and is blocked on credits.
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::MemoryPressure
        ));
        pool.set_demand(&[(next.item.id.clone(), next.request)], 1);
        loop {
            match rx.recv_timeout(Duration::from_secs(1)).unwrap() {
                Event::DecodeDeferred { id, .. } => {
                    assert_eq!(id, old.item.id);
                    break;
                }
                Event::MemoryPressure => {}
                _ => panic!("Obsolete lookup must defer, not decode or report a memory error"),
            }
        }
        assert_eq!(cache.stats().decode_jobs, 0);
        assert!(
            !pool
                .queues
                .0
                .lock()
                .unwrap()
                .pending
                .contains_key(&old.key())
        );
        drop(held);
        assert!(pool.submit(next.clone()).is_ok());
        loop {
            if let Event::Image { id, result, .. } =
                rx.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                assert_eq!(id, next.item.id);
                assert!(result.is_ok());
                break;
            }
        }
        drop(pool);
        drop(rx);
        assert_eq!(cache.stats().decode_jobs, 1);
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
    }

    fn change_view_during_native_probe(keep_source: bool) {
        let binary = PathBuf::from(std::env::var_os("TR_WORKER_BINARY").unwrap());
        let data = tempfile::tempdir().unwrap();
        let wrapper = data.path().join("gated-worker");
        let marker = data.path().join("started");
        let gate = data.path().join("continue");
        let quote =
            |path: &std::path::Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > {}\nwhile [ ! -f {} ]; do /bin/sleep 0.01; done\nexec {}\n",
                quote(&marker), quote(&gate), quote(&binary)
            ),
        ).unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let cache = Arc::new(crate::cache::Manager::new(crate::cache::Settings {
            enabled: false,
            ..Default::default()
        }));
        let (tx, rx) = mpsc::sync_channel(16);
        let pool = DecodePool::start(
            wrapper,
            Arc::new(AtomicU64::new(1)),
            tx,
            egui::Context::default(),
            cache.clone(),
        );
        let old = navigation_job("old");
        assert!(pool.submit(old.clone()).is_ok());
        let start = Instant::now();
        while !marker.exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        }
        let native_pid = std::fs::read_to_string(&marker).unwrap();
        let mut next = if keep_source {
            old.clone()
        } else {
            navigation_job("next")
        };
        next.priority = PreviewPriority::Immediate;
        next.request = PreviewRequest {
            quality: tr_core::preview::PreviewQuality::Standard,
            edge: 64,
        };
        pool.set_demand(&[(next.item.id.clone(), next.request)], 1);
        assert!(pool.submit(next.clone()).is_ok());
        std::fs::write(&gate, b"continue").unwrap();
        let mut deferred = false;
        let mut delivered = false;
        while !deferred || !delivered {
            match rx.recv_timeout(Duration::from_secs(15)).unwrap() {
                Event::DecodeDeferred { id, request, .. } => {
                    assert_eq!((id, request), (old.item.id.clone(), old.request));
                    deferred = true;
                }
                Event::Image {
                    id,
                    request,
                    result,
                    ..
                } => {
                    assert_eq!((id, request), (next.item.id.clone(), next.request));
                    assert!(result.is_ok());
                    delivered = true;
                }
                Event::MemoryPressure => {}
                _ => panic!("Unexpected event"),
            }
        }
        assert_eq!(
            cache.stats().decode_jobs,
            1,
            "Only the current consumer needs development"
        );
        assert!(pool.queues.0.lock().unwrap().pending.is_empty());
        assert!(
            Command::new("/bin/kill")
                .args(["-0", native_pid.trim()])
                .output()
                .unwrap()
                .status
                .success(),
            "Changing view must not terminate the native call/process"
        );
        drop(pool);
        drop(rx);
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
    }

    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn navigation_finishes_native_probe_but_skips_obsolete_development() {
        change_view_during_native_probe(false);
    }

    #[test]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn quality_change_reuses_active_source_without_delivering_removed_consumer() {
        change_view_during_native_probe(true);
    }

    #[test]
    fn phased_decode_admits_24mp_and_rejects_excess_without_changing_the_limit() {
        let memory = tr_core::budget::MemoryBudget::new(2 * 1024 * 1024 * 1024);
        let baseline = memory.try_reserve(384 * 1024 * 1024).unwrap();
        let snapshot = memory.try_reserve(81 * 1024 * 1024).unwrap();
        let working = memory
            .try_reserve(decode_working_bytes(6016, 4016).unwrap())
            .expect("24 MP full-frame phases fit the unchanged 2 GiB budget");
        assert!(memory.try_reserve(working.bytes()).is_none());
        drop((working, snapshot, baseline));
        assert_eq!(memory.usage().reserved, 0);
        assert!(decode_working_bytes(0, 10).is_err());
        assert!(decode_working_bytes(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn phased_budget_covers_all_owned_mip_tails_including_thin_and_odd_images() {
        for (width, height) in [(1, 1), (1, 129), (129, 1), (257, 129)] {
            let budget = tr_core::budget::MemoryBudget::new(
                decode_working_bytes(width, height).unwrap() - 128 * 1024 * 1024,
            );
            let worker_output = budget.try_reserve(u64::from(width * height) * 16).unwrap();
            let image = ImageLevels::from_source(
                tr_core::color::LinearImage::new(
                    width,
                    height,
                    vec![[0., 0., 0., 1.]; (width * height) as usize],
                )
                .unwrap(),
                PreviewRequest::full(),
            )
            .unwrap();
            let main = budget.try_reserve(image.byte_len() as u64).unwrap();
            let mut bases = HashSet::from([0]);
            let tails: Vec<_> = image
                .levels()
                .iter()
                .skip(1)
                .filter_map(|level| {
                    let request = PreviewRequest {
                        quality: tr_core::preview::PreviewQuality::Full,
                        edge: level.width.max(level.height).div_ceil(2),
                    };
                    let start = image.requested_base(request);
                    if !bases.insert(start) {
                        return None;
                    }
                    let bytes = image.levels()[start..]
                        .iter()
                        .map(|l| l.pixels.len() as u64 * 16)
                        .sum();
                    Some(
                        image
                            .detach(request, budget.try_reserve(bytes).unwrap())
                            .unwrap(),
                    )
                })
                .collect();
            assert!(budget.usage().reserved <= budget.usage().limit);
            drop((tails, main, worker_output));
            assert_eq!(budget.usage().reserved, 0);
        }
    }
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
                    observation: String::new(),
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
                observation: String::new(),
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
            observation: String::new(),
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
                    observation: String::new(),
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
