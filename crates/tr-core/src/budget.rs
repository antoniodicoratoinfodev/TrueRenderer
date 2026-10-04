//! Admission credits, not a kernel RSS limit. A lease lives until the last owner
//! of the allocation releases it, including completed results and cache writers.
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub limit: u64,
    pub base: u64,
    pub ceiling: u64,
    pub automatic: bool,
    pub growths: u64,
    pub pressure: bool,
    pub reserved: u64,
    pub peak: u64,
    pub rejected: u64,
    /// Largest failed admission since the consumer last requested reclamation.
    pub reclaim_bytes: u64,
}
#[derive(Clone, Debug)]
pub struct MemoryBudget(Arc<Mutex<Usage>>);
#[derive(Debug)]
pub struct Lease {
    budget: MemoryBudget,
    bytes: u64,
}
impl MemoryBudget {
    pub fn new(limit: u64) -> Self {
        Self(Arc::new(Mutex::new(Usage {
            limit,
            base: limit,
            ceiling: limit,
            ..Usage::default()
        })))
    }
    /// Fixed admission is also used by isolated quota diagnostics.
    pub fn configure(&self, limit: u64) {
        let mut usage = self.0.lock().unwrap();
        usage.limit = limit;
        usage.base = limit;
        usage.ceiling = limit;
        usage.automatic = false;
    }
    pub fn automatic(base: u64, ceiling: u64) -> Self {
        let budget = Self::new(base.min(ceiling));
        budget.configure_automatic(base, ceiling);
        budget
    }
    pub fn configure_automatic(&self, base: u64, ceiling: u64) {
        let mut usage = self.0.lock().unwrap();
        let previous = if usage.automatic { usage.limit } else { base };
        usage.base = base;
        usage.ceiling = ceiling;
        usage.limit = previous.max(base).min(ceiling);
        usage.automatic = true;
    }
    pub fn set_pressure(&self, pressure: bool) {
        let mut usage = self.0.lock().unwrap();
        usage.pressure = pressure;
        if pressure && usage.automatic {
            usage.limit = usage.base.min(usage.ceiling);
        }
    }
    pub fn margin(bytes: u64) -> u64 {
        (bytes / 10).max(256_000_000)
    }
    pub fn usage(&self) -> Usage {
        *self.0.lock().unwrap()
    }
    pub fn take_reclaim_request(&self) -> u64 {
        std::mem::take(&mut self.0.lock().unwrap().reclaim_bytes)
    }
    pub fn try_reserve(&self, bytes: u64) -> Option<Lease> {
        let mut usage = self.0.lock().unwrap();
        let required = usage.reserved.checked_add(bytes)?;
        if usage.automatic && !usage.pressure && required > usage.limit {
            let expanded = required.saturating_add(Self::margin(required));
            // Expand and admit under the same lock. A second queue sees the
            // first reservation, including every still-live lease.
            if expanded <= usage.ceiling {
                usage.limit = expanded;
                usage.growths += 1;
            }
        }
        if bytes > usage.limit.saturating_sub(usage.reserved) {
            usage.rejected += 1;
            usage.reclaim_bytes = usage.reclaim_bytes.max(bytes);
            return None;
        }
        usage.reserved += bytes;
        usage.peak = usage.peak.max(usage.reserved);
        Some(Lease {
            budget: self.clone(),
            bytes,
        })
    }
}
impl Lease {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// Transfer existing credits without a release/reacquire race.
    pub fn split(&mut self, bytes: u64) -> Option<Self> {
        if bytes > self.bytes {
            return None;
        }
        self.bytes -= bytes;
        Some(Self {
            budget: self.budget.clone(),
            bytes,
        })
    }
    pub fn shrink(&mut self, bytes: u64) {
        assert!(bytes <= self.bytes);
        self.budget.0.lock().unwrap().reserved -= self.bytes - bytes;
        self.bytes = bytes;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.budget.0.lock().unwrap().reserved -= self.bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_budget_accepts_reported_work_and_grows_with_live_leases() {
        let budget = MemoryBudget::automatic(8_000_000_000, 14_000_000_000);
        assert_eq!(budget.usage().reserved, 0);
        let existing = budget.try_reserve(924_900_000).unwrap();
        let work = budget.try_reserve(1_290_000_000).unwrap();
        assert_eq!(budget.usage().rejected, 0);
        let extra = budget.try_reserve(6_000_000_000).unwrap();
        let usage = budget.usage();
        assert_eq!(usage.growths, 1);
        assert!(usage.limit >= usage.reserved + MemoryBudget::margin(usage.reserved));
        assert!(usage.limit <= usage.ceiling);
        drop((existing, work, extra));
        assert_eq!(budget.usage().reserved, 0);
        // A pressure event reduces the budget without invalidating live leases.
        budget.set_pressure(true);
        let existing = budget.try_reserve(7_000_000_000).unwrap();
        assert!(budget.try_reserve(2_000_000_000).is_none());
        budget.set_pressure(false);
        let work = budget.try_reserve(2_000_000_000).unwrap();
        let alias = Arc::new(work);
        let reader = alias.clone();
        drop((existing, alias));
        assert_eq!(budget.usage().reserved, 2_000_000_000);
        drop(reader);
        assert_eq!(budget.usage().reserved, 0);
    }
    #[test]
    fn automatic_admission_is_atomic_and_respects_physical_capacity() {
        let budget = MemoryBudget::automatic(8_000_000_000, 12_000_000_000);
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let budget = &budget;
                let barrier = &barrier;
                scope.spawn(move || {
                    let lease = budget.try_reserve(2_000_000_000);
                    barrier.wait();
                    let usage = budget.usage();
                    assert!(usage.reserved <= usage.limit && usage.limit <= usage.ceiling);
                    assert!(usage.rejected > 0);
                    drop(lease);
                });
            }
        });
        assert_eq!(budget.usage().reserved, 0);
        let small = MemoryBudget::automatic(8_000_000_000, 5_000_000_000);
        assert_eq!(small.usage().base, 8_000_000_000);
        assert!(small.try_reserve(6_000_000_000).is_none());
    }
    #[test]
    fn failed_admissions_request_only_the_largest_needed_headroom() {
        let budget = MemoryBudget::new(100);
        let held = budget.try_reserve(80).unwrap();
        assert!(budget.try_reserve(30).is_none());
        assert!(budget.try_reserve(60).is_none());
        assert!(budget.try_reserve(40).is_none());
        assert_eq!(budget.take_reclaim_request(), 60);
        assert_eq!(budget.take_reclaim_request(), 0);
        assert_eq!(budget.usage().reserved, 80);
        drop(held);
        assert!(budget.try_reserve(60).is_some());
    }
    #[test]
    fn transfer_and_last_owner_return_credits_after_resize() {
        let budget = MemoryBudget::new(100);
        let mut work = budget.try_reserve(90).unwrap();
        let resident = Arc::new(work.split(60).unwrap());
        let writer = resident.clone();
        work.shrink(10);
        assert_eq!(budget.usage().reserved, 70);
        budget.configure(50);
        assert!(budget.try_reserve(1).is_none());
        drop(work);
        drop(resident);
        assert_eq!(budget.usage().reserved, 60);
        drop(writer);
        assert_eq!(budget.usage().reserved, 0);
        assert!(budget.try_reserve(50).is_some());
    }
    #[test]
    fn concurrent_admission_never_exceeds_available_credit() {
        let budget = MemoryBudget::new(64);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let budget = budget.clone();
                scope.spawn(move || {
                    for _ in 0..1000 {
                        let _lease = budget.try_reserve(17);
                        assert!(budget.usage().reserved <= 64);
                    }
                });
            }
        });
        assert_eq!(budget.usage().reserved, 0);
        assert!(budget.usage().peak <= 64);
    }
}
