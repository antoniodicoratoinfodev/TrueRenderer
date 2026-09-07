//! Directory-relative operations. Never follow links or recursively delete a folder.
use anyhow::{Result, ensure};
#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

pub struct Directory {
    pub path: PathBuf,
    pub file: File,
}
impl Directory {
    pub fn open(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)?;
            Ok(Self {
                path: path.into(),
                file,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            anyhow::bail!("Cache persistente non qualificata su questa piattaforma")
        }
    }
    pub fn child(&self, name: &str, create: bool) -> Result<(Self, bool)> {
        #[cfg(unix)]
        {
            let name_c = CString::new(name)?;
            let created = create
                && unsafe { libc::mkdirat(self.file.as_raw_fd(), name_c.as_ptr(), 0o700) } == 0;
            let fd = unsafe {
                libc::openat(
                    self.file.as_raw_fd(),
                    name_c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            ensure!(
                fd >= 0,
                "Cartella cache non accessibile: {}",
                std::io::Error::last_os_error()
            );
            Ok((
                Self {
                    path: self.path.join(name),
                    file: unsafe { File::from_raw_fd(fd) },
                },
                created,
            ))
        }
        #[cfg(not(unix))]
        {
            let _ = (name, create);
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
    pub fn open_file(
        &self,
        name: &str,
        write: bool,
        create: bool,
        exclusive: bool,
    ) -> Result<File> {
        ensure!(
            !name.contains('/') && !name.contains('\\') && name != "." && name != "..",
            "Nome cache non valido"
        );
        #[cfg(unix)]
        {
            let name = CString::new(name)?;
            let flags = if write { libc::O_RDWR } else { libc::O_RDONLY }
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK
                | if create { libc::O_CREAT } else { 0 }
                | if exclusive { libc::O_EXCL } else { 0 };
            let fd = unsafe { libc::openat(self.file.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            ensure!(
                fd >= 0,
                "File cache non accessibile: {}",
                std::io::Error::last_os_error()
            );
            let file = unsafe { File::from_raw_fd(fd) };
            let metadata = file.metadata()?;
            ensure!(
                metadata.is_file() && metadata.nlink() == 1,
                "Cache: link o file speciale rifiutato"
            );
            Ok(file)
        }
        #[cfg(not(unix))]
        {
            let _ = (write, create, exclusive);
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        #[cfg(unix)]
        {
            let name = CString::new(name)?;
            ensure!(
                unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) } == 0,
                "Rimozione cache: {}",
                std::io::Error::last_os_error()
            );
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = name;
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
    pub fn publish(&self, name: &str, destination: &Self, target: &str) -> Result<()> {
        #[cfg(unix)]
        {
            let name = CString::new(name)?;
            let target = CString::new(target)?;
            // Atomic publication without clobbering an existing target. The temporary
            // disappears in the same operation, including across an application crash.
            #[cfg(target_os = "macos")]
            let result = unsafe {
                libc::renameatx_np(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    destination.file.as_raw_fd(),
                    target.as_ptr(),
                    libc::RENAME_EXCL,
                )
            };
            #[cfg(target_os = "linux")]
            let result = unsafe {
                libc::renameat2(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    destination.file.as_raw_fd(),
                    target.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            };
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            let result = -1;
            ensure!(
                result == 0,
                "Pubblicazione cache: {}",
                std::io::Error::last_os_error()
            );
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (name, destination, target);
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
}
