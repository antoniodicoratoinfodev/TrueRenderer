//! Metadata-only discovery. No shell thumbnails, image parsing or implicit link traversal.
use std::{
    fs::Metadata,
    path::{Path, PathBuf},
    process::Command,
};
pub fn hidden(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 2 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
pub fn reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
pub fn opaque(path: &Path, metadata: &Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        if metadata.st_flags() & 0x4000_0000 != 0 {
            return true;
        } // UF_DATALESS
        if metadata.is_dir()
            && path.extension().is_some_and(|e| {
                matches!(
                    e.to_string_lossy().to_ascii_lowercase().as_str(),
                    "app"
                        | "bundle"
                        | "framework"
                        | "photoslibrary"
                        | "photolibrary"
                        | "pages"
                        | "numbers"
                        | "key"
                        | "rtfd"
                        | "xcodeproj"
                )
            })
        {
            return true;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
            return true;
        }
    }
    let _ = (path, metadata);
    false
}
pub fn locations() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let home = PathBuf::from(home);
        roots.push(home.clone());
        for name in ["Pictures", "Desktop", "Downloads", "Documents"] {
            let path = home.join(name);
            if path.is_dir() {
                roots.push(path);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/"));
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.flatten().take(64) {
                roots.push(entry.path());
            }
        }
    }
    #[cfg(windows)]
    {
        let drives = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        for index in 0..26 {
            if drives & (1 << index) != 0 {
                roots.push(PathBuf::from(format!("{}:\\", (b'A' + index) as char)));
            }
        }
    }
    roots
}
pub fn reveal(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("/usr/bin/open").arg("-R").arg(path).status()?;
    #[cfg(windows)]
    let status = Command::new("explorer.exe")
        .arg(path.parent().unwrap_or(path))
        .status()?;
    #[cfg(not(any(target_os = "macos", windows)))]
    let status = Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .status()?;
    anyhow::ensure!(status.success(), "System reveal failed");
    Ok(())
}
