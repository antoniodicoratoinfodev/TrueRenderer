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
    /// Already validated resident levels, kept alive by their existing lease.
    /// Derivation runs on the I/O lane, never in the UI or a native decoder.
    pub resident: Option<PreviewDecoded>,
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
    _snapshot_slot: Lease,
}
const MAX_SOURCE_SNAPSHOTS: u64 = 2;
#[derive(Default)]
struct Queues {
    sample: Option<crate::photo_export::ScientificJob>,
    export: Option<crate::photo_export::Job>,
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
            w.iter().any(|(id, request, generation)| {
                id == &job.item.id
                    && *generation == job.generation
                    && request.raw_engine == job.request.raw_engine
            })
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
        let snapshot_slots = tr_core::budget::MemoryBudget::new(MAX_SOURCE_SNAPSHOTS);
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
            let writer = writer.clone();
            let snapshot_slots = snapshot_slots.clone();
            workers.push(thread::spawn(move || {
                let broker = Broker::new(binary);
                let snapshots = tr_core::budget::MemoryBudget::new(cache.memory.usage().limit / 3);
                // Count snapshots across lookup, ready AND active/awaiting decode.
                // Bounding only q.decode still let two decoder threads and the
                // lookup lane pin five RAW copies, starving a 2 GiB grid.
                loop {
                    let job = {
                        let mut q = queues.0.lock().unwrap();
                        while q.lookup.is_empty() && !stop.load(Ordering::Acquire) {
                            q = queues.1.wait(q).unwrap();
                        }
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        // Resident derivation needs no source snapshot or native
                        // admission and must not wait behind a long RAW decode.
                        let index = q
                            .lookup
                            .iter()
                            .position(|job| job.resident.is_some())
                            .unwrap_or(0);
                        q.lookup.remove(index).unwrap()
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
                        if let Some(resident) = &job.resident {
                            {
                                let mut q = queues.0.lock().unwrap();
                                if q.claimed.contains(&job.key())
                                    || !q.pending.contains_key(&job.key())
                                {
                                    return Ok(None);
                                }
                                q.claimed.insert(job.key());
                            }
                            let image = &resident.prepared.image;
                            let base = image.requested_base(job.request);
                            let bytes = image.levels()[base..]
                                .iter()
                                .map(|level| level.pixels.len() as u64 * 16)
                                .sum();
                            let credits = reserve(&cache, bytes, &events, &ctx, &cancelled)?;
                            let tail = Arc::new(image.detach(job.request, credits)?);
                            let histogram = tail.source().histogram();
                            return Ok(Some(PreviewDecoded {
                                prepared: tr_render::PreparedPreview {
                                    image: tail,
                                    histogram,
                                },
                                ..resident.clone()
                            }));
                        }
                        let maximum = job.item.bytes.min(tr_core::protocol::MAX_SOURCE as u64);
                        let required = maximum.saturating_mul(2) + 1024 * 1024;
                        snapshots.configure(cache.memory.usage().limit / 3);
                        anyhow::ensure!(
                            required <= snapshots.usage().limit,
                            "Snapshot oltre quota: aumentare la memoria complessiva"
                        );
                        let (snapshot_lane, snapshot_slot) = loop {
                            anyhow::ensure!(!cancelled(), "Richiesta sostituita");
                            let q = queues.0.lock().unwrap();
                            // A decoder can coalesce this consumer while lookup
                            // waits. Do not remove it from pending or read it twice.
                            if q.claimed.contains(&job.key()) || !q.pending.contains_key(&job.key())
                            {
                                return Ok(None);
                            }
                            drop(q);
                            if let Some(leases) = snapshots
                                .try_reserve(required)
                                .zip(snapshot_slots.try_reserve(1))
                            {
                                break leases;
                            }
                            let mut q = queues.0.lock().unwrap();
                            if q.lookup.iter().any(|other| other.resident.is_some()) {
                                insert(&mut q.lookup, job.clone());
                                return Ok(None);
                            }
                            drop(q);
                            thread::sleep(Duration::from_millis(25));
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
                            _snapshot_slot: snapshot_slot,
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
                    {
                        let mut q = queues.0.lock().unwrap();
                        q.pending.remove(&job.key());
                        if job.resident.is_some() {
                            q.claimed.remove(&job.key());
                        }
                    }
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
                        let persist = job
                            .resident
                            .is_some()
                            .then(|| result.as_ref().ok().cloned())
                            .flatten();
                        if !deliver(
                            &events,
                            Event::Image {
                                id: job.item.id.clone(),
                                request: job.request,
                                generation: job.generation,
                                result: Box::new(result),
                            },
                            &ctx,
                            &stop,
                        ) {
                            break;
                        }
                        if let Some(decoded) = persist
                            && let Some(folder) = job.item.path.parent()
                        {
                            writer.submit(
                                folder.into(),
                                decoded.digest,
                                job.request,
                                decoded.info,
                                decoded.prepared,
                                job.generation,
                            );
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
            let snapshot_slots = snapshot_slots.clone();
            workers.push(thread::spawn(move || {
                let mut broker = Broker::with_slot(binary, slot);
                let mut last_generation = None;
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let sample = if slot == 0 {
                        queues.0.lock().unwrap().sample.take()
                    } else {
                        None
                    };
                    if let Some(job) = sample {
                        let cancelled = || {
                            stop.load(Ordering::Acquire)
                                || job.generation != generation.load(Ordering::Acquire)
                        };
                        let result = (|| -> anyhow::Result<tr_core::science::Sample> {
                            let _slot = snapshot_slots.try_reserve(1).ok_or_else(|| {
                                anyhow::anyhow!("Snapshot occupati: riprovare il campionamento")
                            })?;
                            let length = job.item.bytes.min(tr_core::protocol::MAX_SOURCE as u64);
                            let _memory = reserve(
                                &cache,
                                length * 2 + 16 * 1024 * 1024,
                                &events,
                                &ctx,
                                &cancelled,
                            )?;
                            let source = broker.prepare_snapshot_bounded(
                                &job.item.path,
                                &job.item.digest,
                                length,
                                &cancelled,
                            )?;
                            broker.scientific_sample(source, job.x, job.y, cancelled)
                        })()
                        .map_err(|e| format!("{e:#}"));
                        deliver(
                            &events,
                            Event::ScientificSample {
                                id: job.item.id,
                                result,
                            },
                            &ctx,
                            &stop,
                        );
                        continue;
                    }
                    let export = if slot == 0 {
                        queues.0.lock().unwrap().export.take()
                    } else {
                        None
                    };
                    if let Some(job) = export {
                        let cancelled = || stop.load(Ordering::Acquire) || job.cancelled();
                        let result = (|| -> anyhow::Result<crate::photo_export::Completed> {
                            job.options.validate()?;
                            let start = Instant::now();
                            let _permit = loop {
                                anyhow::ensure!(!cancelled(), "Esportazione annullata");
                                match decode_admission.try_lock() {
                                    Ok(permit) => break permit,
                                    Err(std::sync::TryLockError::Poisoned(_)) => {
                                        anyhow::bail!("Decoder interrotto")
                                    }
                                    Err(std::sync::TryLockError::WouldBlock) => {}
                                }
                                anyhow::ensure!(
                                    start.elapsed() < Duration::from_secs(90),
                                    "Decoder occupato: riprovare export"
                                );
                                thread::sleep(Duration::from_millis(25));
                            };
                            // Never wait holding admission while two queued previews
                            // own the snapshot credits: they need this same lane.
                            let _slot = snapshot_slots.try_reserve(1).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Anteprime ancora in lettura: attendere e riprovare export"
                                )
                            })?;
                            let source_limit =
                                job.item.bytes.min(tr_core::protocol::MAX_SOURCE as u64);
                            let _source_lease =
                                reserve(&cache, source_limit * 2, &events, &ctx, &cancelled)?;
                            let source = broker.prepare_snapshot_bounded(
                                &job.item.path,
                                &job.item.digest,
                                source_limit,
                                &cancelled,
                            )?;
                            broker.set_raw_engine(job.engine);
                            let probe_lease =
                                reserve(&cache, 128 * 1024 * 1024, &events, &ctx, &cancelled)?;
                            let metadata = broker.probe_snapshot(&source, cancelled)?;
                            drop(probe_lease);
                            let pixels =
                                metadata.source_width as u64 * metadata.source_height as u64;
                            let bytes_per_pixel = match job.options.format {
                                tr_core::export::Format::DngRaw => 2,
                                tr_core::export::Format::DngLinear16 => 6,
                                tr_core::export::Format::Png8 => 5,
                                tr_core::export::Format::Png16 => 9,
                                tr_core::export::Format::Tiff16 => 8,
                                tr_core::export::Format::TiffFloat32 => 16,
                                tr_core::export::Format::Jpeg => 16,
                            };
                            let output_limit = (pixels * bytes_per_pixel + 1024 * 1024)
                                .min(tr_core::export::MAX_ENCODED);
                            // Native decoder handles are closed before encoding; output
                            // transfer starts after the working raster has been freed.
                            let peak = decode_working_bytes(
                                metadata.source_width,
                                metadata.source_height,
                            )?
                            .max(pixels * 20 + output_limit * 2);
                            let _working = reserve(&cache, peak, &events, &ctx, &cancelled)?;
                            let (info, bytes) = broker.export_snapshot_bounded(
                                source,
                                job.options,
                                output_limit,
                                cancelled,
                            )?;
                            let path = crate::photo_export::publish(
                                &job.destination,
                                &job.item.name,
                                job.options.format,
                                &bytes,
                                cancelled,
                            )?;
                            Ok(crate::photo_export::Completed { path, info })
                        })()
                        .map_err(|e| format!("{e:#}"));
                        deliver(&events, Event::PhotoExport(result), &ctx, &stop);
                        continue;
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
                        _snapshot_slot,
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
                        broker.set_raw_engine(job.request.raw_engine);
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
                        // Capture the finest current consumer before crossing IPC.
                        // Later requests for more detail remain queued; a reduced
                        // response can never satisfy a native-detail request.
                        let maximum_edge = {
                            let q = queues.0.lock().unwrap();
                            q.pending
                                .keys()
                                .filter(|(id, request, g)| {
                                    id == &job.item.id
                                        && *g == job.generation
                                        && request.raw_engine == job.request.raw_engine
                                })
                                .filter(|key| q.wanted.as_ref().is_none_or(|w| w.contains(*key)))
                                .map(|(_, request, _)| request.maximum_level_edge())
                                .max()
                                .unwrap_or(job.request.maximum_level_edge())
                        };
                        let decoded = if maximum_edge >= info.width.max(info.height) {
                            broker.decode_snapshot_bounded(source, 0, pixels * 16, revoked)?
                        } else {
                            let (size, _) = tr_core::protocol::mip_geometry(
                                [info.width, info.height],
                                maximum_edge,
                            );
                            broker.decode_reference_snapshot_bounded(
                                source,
                                maximum_edge,
                                u64::from(size[0]) * u64::from(size[1]) * 16,
                                cache.effective_threads(),
                                revoked,
                            )?
                        };
                        anyhow::ensure!(
                            [decoded.info.source_width, decoded.info.source_height]
                                == [info.width, info.height],
                            "Dimensioni sorgente diverse dal probe di ammissione"
                        );
                        anyhow::ensure!(!cancelled(), "Richiesta sostituita");
                        // Coalesce only consumers covered by the received part
                        // of the reference graph. Finer requests stay queued.
                        let consumers = {
                            let mut q = queues.0.lock().unwrap();
                            let mut consumers: Vec<_> = q
                                .pending
                                .keys()
                                .filter(|(id, request, g)| {
                                    id == &job.item.id
                                        && *g == job.generation
                                        && request.raw_engine == job.request.raw_engine
                                        && tr_core::protocol::mip_geometry(
                                            [info.width, info.height],
                                            request.maximum_level_edge(),
                                        )
                                        .1 >= decoded.info.reference_mip.map_or(0, |m| m.base)
                                })
                                .filter(|key| q.wanted.as_ref().is_none_or(|w| w.contains(*key)))
                                .filter(|key| !q.claimed.contains(*key))
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
                            let claimed: HashSet<_> = consumers.iter().map(Job::key).collect();
                            q.lookup.retain(|j| !claimed.contains(&j.key()));
                            q.decode.retain(|r| !claimed.contains(&r.job.key()));
                            consumers
                        };
                        cache.decoded(consumers.len(), broker.statistics());
                        let finest = consumers
                            .iter()
                            .map(|j| j.request)
                            .max_by_key(|r| r.maximum_level_edge())
                            .unwrap_or(job.request);
                        let mut image = if decoded.info.scientific.is_some() {
                            let (source, base) = if let Some(mip) = decoded.info.reference_mip {
                                (decoded.raster, mip.base)
                            } else {
                                let (source, base, _) = tr_core::provider::reduce_scientific_mip(
                                    decoded.raster,
                                    finest.maximum_level_edge(),
                                )?;
                                (source, base)
                            };
                            ImageLevels::from_scientific_mip(
                                source,
                                [decoded.info.source_width, decoded.info.source_height],
                                base,
                            )?
                        } else if let Some(mip) = decoded.info.reference_mip {
                            ImageLevels::from_reference_mip(
                                decoded.raster,
                                [decoded.info.source_width, decoded.info.source_height],
                                mip.base,
                                mip.opaque,
                            )?
                        } else {
                            ImageLevels::from_source(decoded.raster, finest)?
                        };
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
                    drop(_snapshot_slot);
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
                                .filter(|(id, request, g)| {
                                    id == &job.item.id
                                        && *g == job.generation
                                        && request.raw_engine == job.request.raw_engine
                                })
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
    pub fn submit_export(&self, job: crate::photo_export::Job) -> bool {
        let mut q = self.queues.0.lock().unwrap();
        if q.export.is_some() || self.stop.load(Ordering::Acquire) {
            return false;
        }
        q.export = Some(job);
        self.queues.1.notify_all();
        true
    }
    pub fn submit_sample(&self, job: crate::photo_export::ScientificJob) -> bool {
        let mut q = self.queues.0.lock().unwrap();
        if q.sample.is_some() || self.stop.load(Ordering::Acquire) {
            return false;
        }
        q.sample = Some(job);
        self.queues.1.notify_all();
        true
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

#[cfg(test)]
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
            resident: None,
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
                raw_engine: tr_core::decoder::RawEngine::default(),
                quality: tr_core::preview::PreviewQuality::Full,
                edge: 128,
            },
            priority: PreviewPriority::Background,
            generation: 1,
        }
    }
    #[test]
    fn another_engine_cannot_keep_an_abandoned_development_alive() {
        let old = navigation_job("same-source");
        let new = Job {
            request: PreviewRequest {
                raw_engine: tr_core::decoder::RawEngine::TrueRenderer,
                ..old.request
            },
            ..old.clone()
        };
        let queues = Queues {
            wanted: Some(HashSet::from([new.key()])),
            ..Queues::default()
        };
        assert!(!queues.wants_source(&old));
        assert!(queues.wants_source(&new));
    }
    #[test]
    fn resident_thumbnail_needs_no_file_read_or_decoder_and_releases_credits() {
        let cache = Arc::new(crate::cache::Manager::new(crate::cache::Settings {
            enabled: false,
            ..Default::default()
        }));
        let mut job = navigation_job("resident");
        job.item.path = PathBuf::from("nonexistent-resident-source");
        job.request.edge = 32;
        job.priority = PreviewPriority::SecondaryVisible;
        let source = tr_core::color::LinearImage::new(
            513,
            257,
            (0..513 * 257)
                .map(|i| [-0.2, i as f32 / 20000., 1.2, 1.])
                .collect(),
        )
        .unwrap();
        let mut image = ImageLevels::from_source(source, PreviewRequest::full()).unwrap();
        image.attach_lease(cache.memory.try_reserve(image.byte_len() as u64).unwrap());
        let image = Arc::new(image);
        let expected = image.levels()[image.requested_base(job.request)]
            .pixels
            .clone();
        job.resident = Some(PreviewDecoded {
            digest: "resident-digest".into(),
            info: tr_core::protocol::RasterInfo {
                scientific: None,
                reference_mip: None,
                width: 513,
                height: 257,
                source_width: 513,
                source_height: 257,
                native_bits: 32,
                format: "test".into(),
                decoder: "test".into(),
                input_color: "Rec2020".into(),
                filter: "reference".into(),
                orientation: "applied".into(),
            },
            prepared: tr_render::PreparedPreview {
                image: image.clone(),
                histogram: [[0; 256]; 3],
            },
            transport: "resident",
            worker_pid: None,
        });
        let (tx, rx) = mpsc::sync_channel(8);
        let pool = DecodePool::start(
            PathBuf::from("nonexistent-worker"),
            Arc::new(AtomicU64::new(1)),
            tx,
            egui::Context::default(),
            cache.clone(),
        );
        assert!(pool.submit(job).is_ok());
        let Event::Image { result, .. } = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
            panic!("No resident result");
        };
        let result = result.unwrap();
        assert_eq!(result.prepared.image.source().pixels, expected);
        assert_eq!(
            result.prepared.histogram,
            result.prepared.image.source().histogram()
        );
        assert_eq!(cache.stats().decode_jobs, 0);
        assert_ne!(result.prepared.image.id(), image.id());
        drop(pool);
        drop(result);
        drop(image);
        assert_eq!(cache.memory.usage().reserved, cache.baseline_bytes);
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

    /// Drives the pool through a shell wrapper that pauses the worker, so it
    /// is Unix-only until an equivalent exists for Windows.
    #[cfg(unix)]
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
            raw_engine: tr_core::decoder::RawEngine::default(),
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
    #[cfg(unix)]
    #[ignore = "requires built worker; scripts/verify.sh runs this explicitly"]
    fn navigation_finishes_native_probe_but_skips_obsolete_development() {
        change_view_during_native_probe(false);
    }

    #[test]
    #[cfg(unix)]
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
    fn snapshots_in_active_and_ready_lanes_leave_room_for_a_d750_grid() {
        use tr_core::budget::MemoryBudget;
        const MIB: u64 = 1024 * 1024;
        let memory = MemoryBudget::new(2048 * MIB);
        let baseline = memory.try_reserve(384 * MIB).unwrap();
        let visible = memory.try_reserve(160 * MIB).unwrap();
        let slots = MemoryBudget::new(MAX_SOURCE_SNAPSHOTS);
        let active = (
            slots.try_reserve(1).unwrap(),
            memory.try_reserve(64 * MIB).unwrap(),
        );
        let ready = (
            slots.try_reserve(1).unwrap(),
            memory.try_reserve(64 * MIB).unwrap(),
        );
        assert!(
            slots.try_reserve(1).is_none(),
            "Lookup must not read another source while both snapshots are retained"
        );
        // Moving out of the ready queue must not release the source slot.
        let second_decoder_waiting = ready;
        assert!(slots.try_reserve(1).is_none());
        let working = memory
            .try_reserve(decode_working_bytes(6032, 4032).unwrap())
            .unwrap();
        // The old two-active + two-ready + lookup policy starved this same grid.
        assert!(memory.try_reserve(3 * 64 * MIB).is_none());
        drop(active);
        assert!(slots.try_reserve(1).is_some());
        drop((second_decoder_waiting, working, visible, baseline));
        assert_eq!(slots.usage().reserved, 0);
        assert_eq!(memory.usage().reserved, 0);
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
                        raw_engine: tr_core::decoder::RawEngine::default(),
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::{process::Command, time::Instant};

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
                resident: None,
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
            resident: None,
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
                raw_engine: tr_core::decoder::RawEngine::default(),
                quality: tr_core::preview::PreviewQuality::Standard,
                edge: 0,
            },
            PreviewRequest {
                raw_engine: tr_core::decoder::RawEngine::default(),
                quality: tr_core::preview::PreviewQuality::Full,
                edge: 64,
            },
        ];
        {
            // Make the simultaneous consumer set deterministic before releasing I/O.
            let mut q = pool.queues.0.lock().unwrap();
            for request in requests {
                let job = Job {
                    resident: None,
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
    #[cfg(unix)]
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
                resident: None,
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
                    raw_engine: tr_core::decoder::RawEngine::default(),
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
