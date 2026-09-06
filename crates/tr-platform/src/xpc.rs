//! Internal macOS R0 adapter. Native lease pins the task, never just a PID.
use anyhow::{Result, bail, ensure};
use std::{
    ffi::{CStr, c_char, c_void},
    fs::File,
    os::fd::FromRawFd,
    ptr::NonNull,
    time::Duration,
};

unsafe extern "C" {
    fn tr_xpc_open(
        slot: u32,
        timeout: u32,
        input: *mut i32,
        output: *mut i32,
        error: *mut c_char,
        size: usize,
    ) -> *mut c_void;
    fn tr_xpc_usage(lease: *mut c_void, rss: *mut u64, footprint: *mut u64, pid: *mut i32) -> i32;
    fn tr_xpc_suspend_for_test(lease: *mut c_void) -> i32;
    fn tr_xpc_inject_growth_for_test(lease: *mut c_void) -> i32;
    fn tr_xpc_stop(lease: *mut c_void) -> i32;
    fn tr_xpc_release(lease: *mut c_void);
}
pub struct Session(NonNull<c_void>);
// SAFETY: the lease has one Rust owner and is only used on that owner's thread.
// XPC callbacks own separate ARC-managed state. This type is intentionally not Sync.
unsafe impl Send for Session {}
impl Session {
    pub fn inject_growth_for_test(&mut self) -> Result<()> {
        // SAFETY: live authenticated lease. Only a separately compiled qualification service accepts this message.
        ensure!(
            unsafe { tr_xpc_inject_growth_for_test(self.0.as_ptr()) } == 0,
            "Il servizio non è una build di fault injection"
        );
        Ok(())
    }
    pub fn open(slot: u32, timeout: Duration) -> Result<(Self, File, File)> {
        let mut input = -1;
        let mut output = -1;
        let mut error = [0 as c_char; 512];
        // SAFETY: valid writable out-pointers for the duration of a synchronous call.
        let raw = unsafe {
            tr_xpc_open(
                slot,
                timeout.as_millis().min(12000) as u32,
                &mut input,
                &mut output,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        let Some(handle) = NonNull::new(raw) else {
            // SAFETY: zero-initialized buffer, native snprintf always leaves a NUL.
            bail!(
                "XPC: {}",
                unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
            );
        };
        let session = Self(handle);
        ensure!(
            input >= 0 && output >= 0 && input != output,
            "Descrittori XPC non validi"
        );
        // SAFETY: native adapter transfers unique ownership of these two pipe descriptors.
        Ok((session, unsafe { File::from_raw_fd(input) }, unsafe {
            File::from_raw_fd(output)
        }))
    }
    pub fn usage(&self) -> Result<(u64, u64, u32)> {
        let (mut rss, mut footprint, mut pid) = (0, 0, 0);
        // SAFETY: lease is live; all out-pointers are writable for the call.
        let status = unsafe { tr_xpc_usage(self.0.as_ptr(), &mut rss, &mut footprint, &mut pid) };
        ensure!(
            status == 0 && pid > 0,
            "Servizio XPC terminato o misura memoria non disponibile ({status})"
        );
        Ok((rss, footprint, pid as u32))
    }
    pub fn suspend_for_test(&mut self) -> Result<()> {
        // SAFETY: only this authenticated, live lease is affected by the test hook.
        ensure!(
            unsafe { tr_xpc_suspend_for_test(self.0.as_ptr()) } == 0,
            "Sospensione di prova XPC non riuscita"
        );
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: single owner releases the native retain exactly once; stop uses an audit token.
        unsafe {
            tr_xpc_stop(self.0.as_ptr());
            tr_xpc_release(self.0.as_ptr());
        }
    }
}
