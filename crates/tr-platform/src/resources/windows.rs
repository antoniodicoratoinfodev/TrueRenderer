//! Native capacity and nonblocking, system-wide memory resource signals.
use super::MemoryPressure;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::System::{
    Memory::{
        CreateMemoryResourceNotification, HighMemoryResourceNotification,
        LowMemoryResourceNotification, QueryMemoryResourceNotification,
    },
    SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
};

pub(crate) fn physical_memory_mib() -> Option<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: initialized structure of the exact ABI size, writable for this
    // call; no pointer or allocation is retained by GlobalMemoryStatusEx.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    let mib = status.ullTotalPhys / (1024 * 1024);
    (mib > 0).then_some(mib)
}

struct Signals {
    low: OwnedHandle,
    high: OwnedHandle,
}
impl Signals {
    fn new() -> Option<Self> {
        let create = |kind| {
            // SAFETY: a valid notification type; a successful handle is owned
            // exactly once and closed on thread exit or partial failure.
            let handle = unsafe { CreateMemoryResourceNotification(kind) };
            (!handle.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(handle) })
        };
        Some(Self {
            low: create(LowMemoryResourceNotification)?,
            high: create(HighMemoryResourceNotification)?,
        })
    }
    fn pressure(&self) -> Option<MemoryPressure> {
        fn query(handle: &OwnedHandle) -> Option<bool> {
            let mut signaled = 0;
            // SAFETY: the owned handle remains live; the output is writable.
            (unsafe { QueryMemoryResourceNotification(handle.as_raw_handle(), &mut signaled) } != 0)
                .then_some(signaled != 0)
        }
        classify(query(&self.low), query(&self.high))
    }
}

fn classify(low: Option<bool>, high: Option<bool>) -> Option<MemoryPressure> {
    // The two queries are not atomic. A low signal always takes precedence.
    match (low, high) {
        (Some(true), _) => Some(MemoryPressure::Critical),
        (Some(false), Some(true)) => Some(MemoryPressure::Normal),
        // Microsoft defines the middle range as a request to hold usage steady.
        (Some(false), Some(false)) => Some(MemoryPressure::Warning),
        _ => None,
    }
}

pub(super) fn pressure() -> Option<MemoryPressure> {
    thread_local! {
        // Service polling reuses two handles; no handle creation or waiting in
        // the hot path. Ownership also avoids a leaked process-global handle.
        static SIGNALS: Option<Signals> = Signals::new();
    }
    SIGNALS.with(|signals| signals.as_ref()?.pressure())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capacity_and_pressure_are_available() {
        let physical = physical_memory_mib().expect("Windows physical memory query failed");
        assert_eq!(crate::physical_memory_mib(), physical);
        assert!(physical > 0);
        let first = pressure().expect("Windows memory resource notifications unavailable");
        // Keep the native capacity in the log; do not assume this host has 8 GB.
        eprintln!("native_memory_mib={physical} pressure={first:?}");
        assert!(pressure().is_some());
    }

    #[test]
    fn intermediate_low_and_unavailable_signals_never_claim_safe_growth() {
        assert_eq!(
            classify(Some(false), Some(true)),
            Some(MemoryPressure::Normal)
        );
        assert_eq!(
            classify(Some(false), Some(false)),
            Some(MemoryPressure::Warning)
        );
        for high in [Some(false), Some(true), None] {
            assert_eq!(classify(Some(true), high), Some(MemoryPressure::Critical));
        }
        assert_eq!(classify(None, Some(true)), None);
        assert_eq!(classify(Some(false), None), None);
    }
}
