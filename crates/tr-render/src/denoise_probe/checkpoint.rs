//! One bounded denoised region. Unsubmitted work is never reused; weak source
//! identity prevents an optional checkpoint from pinning a full native texture.
use super::*;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tr_core::budget::MemoryBudget;

pub(super) struct Key {
    source: Weak<admission::Source>,
    point: Vec<u8>,
    optical: u32,
    request: [u32; 9],
}
impl Key {
    pub fn new(
        source: &Arc<admission::Source>,
        optics: &point::Optics,
        edit: &crate::live_edit::LiveEdit,
        request: admission::Request,
    ) -> Result<Self> {
        edit.recipe.validate()?;
        let amount = edit
            .recipe
            .advanced
            .as_ref()
            .map_or(0., |a| a.geometry.vignette);
        ensure!(
            optics.matches(source.size, amount),
            "Checkpoint optical identity mismatch"
        );
        ensure!(
            edit.input_gains
                .into_iter()
                .all(|v| v.is_finite() && (0.05..=20.).contains(&v)),
            "Invalid checkpoint input gains"
        );
        Ok(Self {
            source: Arc::downgrade(source),
            point: point::parameters(edit),
            optical: amount.to_bits(),
            request: [
                request.n as u32,
                request.levels,
                request.amounts[0].to_bits(),
                request.amounts[1].to_bits(),
                request.threshold.to_bits(),
                request.origin[0],
                request.origin[1],
                request.size[0],
                request.size[1],
            ],
        })
    }
    fn matches(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.source, &other.source)
            && self.point == other.point
            && self.optical == other.optical
            && self.request == other.request
    }
}
pub(super) struct Entry {
    pub buffer: wgpu::Buffer,
    key: Key,
    bytes: u64,
    submitted: AtomicBool,
    _credits: admission::Credits,
}
impl Entry {
    pub fn new(buffer: wgpu::Buffer, key: Key, bytes: u64, credits: admission::Credits) -> Self {
        Self {
            buffer,
            key,
            bytes,
            submitted: AtomicBool::new(false),
            _credits: credits,
        }
    }
    pub fn submitted(&self) {
        self.submitted.store(true, Ordering::Release);
    }
}
#[derive(Default)]
pub(super) struct Checkpoint {
    entry: Mutex<Option<Arc<Entry>>>,
    pub hits: AtomicUsize,
    pub misses: AtomicUsize,
}
impl Checkpoint {
    pub fn limit(memory: &MemoryBudget, gpu: &MemoryBudget) -> u64 {
        let m = memory.usage();
        let g = gpu.usage();
        if m.pressure || g.pressure || m.reserved > m.limit || g.reserved > g.limit {
            return 0;
        }
        (m.limit / 16).min(g.limit / 16).min(64 * 1024 * 1024)
    }
    pub fn get(&self, key: &Key, memory: &MemoryBudget, gpu: &MemoryBudget) -> Option<Arc<Entry>> {
        let limit = Self::limit(memory, gpu);
        let mut slot = self.entry.lock().unwrap();
        let hit = slot
            .as_ref()
            .filter(|e| {
                e.bytes <= limit && e.key.matches(key) && e.submitted.load(Ordering::Acquire)
            })
            .cloned();
        if hit.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            slot.take();
        }
        hit
    }
    pub fn put(&self, entry: Arc<Entry>, memory: &MemoryBudget, gpu: &MemoryBudget) {
        let admitted = entry.bytes <= Self::limit(memory, gpu);
        *self.entry.lock().unwrap() = admitted.then_some(entry);
    }
    pub fn clear(&self) {
        self.entry.lock().unwrap().take();
    }
}
