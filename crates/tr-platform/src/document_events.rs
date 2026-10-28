use std::path::PathBuf;
#[cfg(target_os = "macos")]
static OPEN: std::sync::Mutex<std::collections::VecDeque<PathBuf>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

pub fn install() {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn tr_document_events_install(callback: unsafe extern "C" fn(*const std::ffi::c_char));
        }
        tr_document_events_install(receive);
    }
}
#[cfg(target_os = "macos")]
unsafe extern "C" fn receive(path: *const std::ffi::c_char) {
    use std::os::unix::ffi::OsStrExt;
    if path.is_null() {
        return;
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes();
    if let Ok(mut queue) = OPEN.lock()
        && queue.len() < 32
    {
        queue.push_back(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
    }
}
pub fn next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        OPEN.lock().ok()?.pop_front()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
