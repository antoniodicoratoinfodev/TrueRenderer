//! Resource notifications are advisory; admission credits remain authoritative.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[repr(i8)]
pub enum MemoryPressure {
    Normal = 0,
    Warning = 1,
    Critical = 2,
}
pub fn memory_pressure() -> Option<MemoryPressure> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn tr_memory_pressure_level() -> std::ffi::c_int;
        }
        match unsafe { tr_memory_pressure_level() } {
            0 => Some(MemoryPressure::Normal),
            1 => Some(MemoryPressure::Warning),
            2 => Some(MemoryPressure::Critical),
            _ => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn native_pressure_notification_source_is_available() {
        assert!(super::memory_pressure().is_some());
    }
}
