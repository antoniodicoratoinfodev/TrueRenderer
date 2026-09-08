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
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
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
    /// Metadata relative to the already opened directory, without opening every
    /// payload or following symlinks. Actual reads still validate an owned FD.
    pub fn file_info(&self, name: &str) -> Result<(u64, std::time::SystemTime)> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            ensure!(
                !name.contains('/') && !name.contains('\\') && name != "." && name != "..",
                "Nome cache non valido"
            );
            let name = CString::new(name)?;
            let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
            let status = unsafe {
                libc::fstatat(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    info.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if status != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let info = unsafe { info.assume_init() };
            ensure!(
                info.st_mode & libc::S_IFMT == libc::S_IFREG
                    && info.st_nlink == 1
                    && info.st_size >= 0,
                "Cache: link o file speciale rifiutato"
            );
            ensure!(
                (0..1_000_000_000).contains(&info.st_mtime_nsec),
                "Data cache non valida"
            );
            let seconds = std::time::Duration::from_secs(info.st_mtime.unsigned_abs());
            let epoch = std::time::UNIX_EPOCH;
            let modified = if info.st_mtime >= 0 {
                epoch.checked_add(seconds)
            } else {
                epoch.checked_sub(seconds)
            }
            .and_then(|t| t.checked_add(std::time::Duration::from_nanos(info.st_mtime_nsec as u64)))
            .ok_or_else(|| anyhow::anyhow!("Data cache fuori intervallo"))?;
            Ok((info.st_size as u64, modified))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let file = self.open_file(name, false, false, false)?;
            let metadata = file.metadata()?;
            Ok((metadata.len(), metadata.modified()?))
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
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
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
