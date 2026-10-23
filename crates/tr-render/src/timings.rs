//! Opt-in bounded diagnostics. No pixel readback, file I/O or GPU waits.
use eframe::wgpu;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::Instant,
};
use tr_core::budget::{Lease, MemoryBudget};

const CAPACITY: usize = 16384;
static EPOCH: OnceLock<Instant> = OnceLock::new();
static LOG: Mutex<VecDeque<Sample>> = Mutex::new(VecDeque::new());

#[derive(Clone, serde::Serialize)]
pub struct Sample {
    pub phase: &'static str,
    pub started_us: u64,
    pub duration_us: f64,
    pub bytes: u64,
}

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("TR_RENDER_TIMINGS").is_ok_and(|v| v == "1"))
}

pub fn mark() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_micros() as u64
}

pub fn samples_since(started: u64) -> Vec<Sample> {
    LOG.lock()
        .unwrap()
        .iter()
        .filter(|s| s.started_us >= started)
        .cloned()
        .collect()
}

fn record(phase: &'static str, started_us: u64, duration_us: f64, bytes: u64) {
    let mut log = LOG.lock().unwrap();
    if log.len() == CAPACITY {
        log.pop_front();
    }
    log.push_back(Sample {
        phase,
        started_us,
        duration_us,
        bytes,
    });
}

pub(crate) struct CpuSpan {
    phase: &'static str,
    started: Option<(Instant, u64)>,
    bytes: u64,
}
impl CpuSpan {
    pub fn new(phase: &'static str) -> Self {
        Self {
            phase,
            started: enabled().then(|| (Instant::now(), mark())),
            bytes: 0,
        }
    }
    pub fn bytes(mut self, bytes: u64) -> Self {
        self.bytes = bytes;
        self
    }
}
impl Drop for CpuSpan {
    fn drop(&mut self) {
        if let Some((instant, started)) = self.started {
            record(
                self.phase,
                started,
                instant.elapsed().as_secs_f64() * 1e6,
                self.bytes,
            );
        }
    }
}

pub(crate) struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    phases: Vec<&'static str>,
    started: u64,
    _memory: Lease,
    _gpu: Lease,
}
impl GpuTimer {
    pub fn features() -> wgpu::Features {
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
    }
    pub fn new(device: &wgpu::Device, memory: &MemoryBudget, gpu: &MemoryBudget) -> Option<Self> {
        if !enabled() || !device.features().contains(Self::features()) {
            return None;
        }
        // Eight timestamps plus two buffers, conservatively aligned to 256 B.
        let memory = memory.try_reserve(768)?;
        let gpu = gpu.try_reserve(768)?;
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("TR diagnostic timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 8,
        });
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TR diagnostic timestamps"),
                size: 64,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            queries,
            resolve: buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            readback: buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
            phases: Vec::with_capacity(4),
            started: mark(),
            _memory: memory,
            _gpu: gpu,
        })
    }
    pub fn begin(&mut self, encoder: &mut wgpu::CommandEncoder, phase: &'static str) {
        encoder.write_timestamp(&self.queries, (self.phases.len() * 2) as u32);
        self.phases.push(phase);
    }
    pub fn end(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.write_timestamp(&self.queries, (self.phases.len() * 2 - 1) as u32);
    }
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let count = (self.phases.len() * 2) as u32;
        encoder.resolve_query_set(&self.queries, 0..count, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, u64::from(count) * 8);
    }
    pub fn submitted(self, queue: &wgpu::Queue) {
        let period = f64::from(queue.get_timestamp_period()) / 1000.;
        let buffer = self.readback.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_ok() {
                if let Ok(bytes) = self.readback.get_mapped_range(..) {
                    for (phase, pair) in self.phases.iter().zip(bytes.as_chunks::<16>().0) {
                        let a = u64::from_le_bytes(pair[..8].try_into().unwrap());
                        let b = u64::from_le_bytes(pair[8..].try_into().unwrap());
                        if b >= a {
                            record(phase, self.started, (b - a) as f64 * period, 0);
                        }
                    }
                }
                self.readback.unmap();
            }
            // Captured leases survive submission through mapping completion.
            drop(self);
        });
    }
}

/// Preserve all existing requirements, adding only supported diagnostic features.
pub fn device_features(available: wgpu::Features) -> wgpu::Features {
    if enabled() && available.contains(GpuTimer::features()) {
        GpuTimer::features()
    } else {
        wgpu::Features::empty()
    }
}
