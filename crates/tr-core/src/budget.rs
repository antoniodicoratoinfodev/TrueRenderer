//! Admission credits, not a kernel RSS limit. A lease lives until the last owner
//! of the allocation releases it, including completed results and cache writers.
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub limit: u64,
    pub reserved: u64,
    pub peak: u64,
    pub rejected: u64,
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
            ..Usage::default()
        })))
    }
    pub fn configure(&self, limit: u64) {
        self.0.lock().unwrap().limit = limit;
    }
    pub fn usage(&self) -> Usage {
        *self.0.lock().unwrap()
    }
    pub fn try_reserve(&self, bytes: u64) -> Option<Lease> {
        let mut usage = self.0.lock().unwrap();
        if bytes > usage.limit.saturating_sub(usage.reserved) {
            usage.rejected += 1;
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
