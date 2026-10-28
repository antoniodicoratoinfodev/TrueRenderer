//! Shared growable admission retained by the editor and in-flight import work.
use std::sync::Mutex;
use tr_core::budget::{Lease, MemoryBudget};
pub struct Admission {
    budget: MemoryBudget,
    leases: Mutex<Vec<Lease>>,
}
impl Admission {
    pub fn new(budget: MemoryBudget) -> Self {
        Self {
            budget,
            leases: Mutex::new(vec![]),
        }
    }
    pub fn bytes(&self) -> u64 {
        self.leases.lock().unwrap().iter().map(Lease::bytes).sum()
    }
    pub fn ensure(&self, bytes: u64) -> bool {
        let mut leases = self.leases.lock().unwrap();
        let held: u64 = leases.iter().map(Lease::bytes).sum();
        if bytes <= held {
            return true;
        }
        if let Some(extra) = self.budget.try_reserve(bytes - held) {
            leases.push(extra);
            true
        } else {
            false
        }
    }
    /// Only after the editor has dropped the previous document and its history,
    /// with no import sharing this admission.
    pub fn shrink(&self, bytes: u64) {
        let mut leases = self.leases.lock().unwrap();
        let mut retained = bytes;
        for lease in leases.iter_mut() {
            let keep = retained.min(lease.bytes());
            lease.shrink(keep);
            retained -= keep;
        }
        leases.retain(|l| l.bytes() > 0);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn growth_is_shared_bounded_and_released_with_last_owner() {
        let memory = MemoryBudget::new(6000);
        let admission = std::sync::Arc::new(Admission::new(memory.clone()));
        assert!(admission.ensure(3000));
        let import = admission.clone();
        assert!(import.ensure(5000));
        assert!(!admission.ensure(7000));
        assert_eq!(memory.usage().reserved, 5000);
        drop(admission);
        assert_eq!(memory.usage().reserved, 5000);
        drop(import);
        assert_eq!(memory.usage().reserved, 0);
    }
    #[test]
    fn replacing_a_large_document_releases_unused_credits() {
        let memory = MemoryBudget::new(10000);
        let admission = Admission::new(memory.clone());
        assert!(admission.ensure(5000));
        assert!(admission.ensure(9000));
        admission.shrink(3000);
        assert_eq!(admission.bytes(), 3000);
        assert_eq!(memory.usage().reserved, 3000);
        assert!(admission.ensure(6000));
        assert_eq!(memory.usage().reserved, 6000);
    }
}
