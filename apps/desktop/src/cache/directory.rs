//! Directory-relative operations. Never follow links or recursively delete a folder.
//!
//! Unix pins the folder with a descriptor and works through `openat`, so an
//! ancestor renamed mid-operation cannot redirect a write. Windows has no
//! documented equivalent without `NtCreateFile`, so the folder handle is held
//! open without share-delete, which stops anyone renaming or deleting the cache
//! directory itself while it is in use, and every child is reached by path from
//! that pinned folder. Ancestors above the cache root are not pinned: that is
//! the one guarantee this platform does not reproduce, and it is stated rather
//! than implied. The properties that do carry over are refusing a reparse point
//! at the final component, refusing mutation of hard links and special files, and
//! publishing without clobbering an existing entry.
use anyhow::{Result, ensure};
#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
};
use std::{
    fs::{File, FileTimes, OpenOptions},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[cfg(windows)]
mod flags {
    pub const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    pub const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    pub const ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    /// For the folder: readers and writers are allowed, deletion is not, so
    /// the path a `Directory` names cannot be renamed away while it is held.
    pub const SHARE_READ_WRITE: u32 = 0x0000_0001 | 0x0000_0002;
    /// For payloads: deletion is shared as well, so an entry can be published
    /// or collected while a reader still holds it. Windows then keeps the file
    /// alive for that handle and removes it at the last close, which is the
    /// unlink-while-open behaviour the cache design already assumes.
    pub const SHARE_ALL: u32 = SHARE_READ_WRITE | 0x0000_0004;
}

pub struct Directory {
    pub path: PathBuf,
    /// The open folder. Unix reads it on every call, as the descriptor each
    /// `*at` operation is relative to. Windows never reads it: holding it
    /// without share-delete is the whole point, because that is what stops the
    /// folder being renamed or removed while this value lives.
    #[cfg_attr(windows, allow(dead_code))]
    pub file: File,
}
impl Directory {
    /// Rejects a name that could escape the folder or address something other
    /// than a plain entry in it. Windows adds `:`, which would name an
    /// alternate data stream on an existing file.
    fn validate(name: &str) -> Result<()> {
        ensure!(
            !name.contains('/')
                && !name.contains('\\')
                && !name.contains(':')
                && name != "."
                && name != ".."
                && !name.is_empty(),
            "Nome cache non valido"
        );
        Ok(())
    }

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
        #[cfg(windows)]
        {
            let file = OpenOptions::new()
                .read(true)
                .share_mode(flags::SHARE_READ_WRITE)
                .custom_flags(flags::BACKUP_SEMANTICS | flags::OPEN_REPARSE_POINT)
                .open(path)?;
            let metadata = file.metadata()?;
            ensure!(metadata.is_dir(), "Cache: non è una directory");
            ensure!(
                metadata.file_attributes() & flags::ATTRIBUTE_REPARSE_POINT == 0,
                "Cache: punto di reparse rifiutato"
            );
            Ok(Self {
                path: path.into(),
                file,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            anyhow::bail!("Cache persistente non qualificata su questa piattaforma")
        }
    }
    pub fn child(&self, name: &str, create: bool) -> Result<(Self, bool)> {
        Self::validate(name)?;
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
        #[cfg(windows)]
        {
            let path = self.path.join(name);
            // An existing folder is not a failure; only a fresh one is reported
            // as created, which is what the Unix branch reports too.
            let created = create && std::fs::create_dir(&path).is_ok();
            Ok((Self::open(&path)?, created))
        }
        #[cfg(not(any(unix, windows)))]
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
            Self::validate(name)?;
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
        #[cfg(windows)]
        {
            Self::validate(name)?;
            // No handle is opened, so a quota scan still does not touch every
            // payload. The link count is checked at `open_file`, before any read.
            let metadata = std::fs::symlink_metadata(self.path.join(name))?;
            ensure!(
                metadata.is_file()
                    && metadata.file_attributes() & flags::ATTRIBUTE_REPARSE_POINT == 0,
                "Cache: link o file speciale rifiutato"
            );
            Ok((metadata.len(), metadata.modified()?))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
        {
            let file = self.open_file(name, false, false, false)?;
            let metadata = file.metadata()?;
            Ok((metadata.len(), metadata.modified()?))
        }
    }
    /// Whether an extra hard link on this entry is acceptable for this open.
    ///
    /// The refusal exists so a write cannot be redirected through a link that also
    /// names a file outside the cache. That risk is real only when an entry we did
    /// not just create is opened for modification: a file created exclusively has
    /// one link by construction, and a read cannot corrupt anything a later link
    /// points at.
    ///
    /// The narrowing is not theoretical. A cloud sync client hard-links files while
    /// it uploads them, so the strict rule made the cache unusable inside a synced
    /// folder, intermittently and with no explanation. Refusing every extra link
    /// bought nothing there and cost the whole feature.
    fn links_allowed(write: bool, create: bool, exclusive: bool) -> bool {
        !write || (create && exclusive)
    }

    pub fn open_file(
        &self,
        name: &str,
        write: bool,
        create: bool,
        exclusive: bool,
    ) -> Result<File> {
        Self::validate(name)?;
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
            ensure!(metadata.is_file(), "Cache: non è un file regolare");
            ensure!(
                Self::links_allowed(write, create, exclusive) || metadata.nlink() == 1,
                "Cache: il file ha {} collegamenti, atteso 1",
                metadata.nlink()
            );
            Ok(file)
        }
        #[cfg(windows)]
        {
            let mut options = OpenOptions::new();
            options.read(true).write(write);
            match (create, exclusive) {
                // CREATE_NEW: an existing entry, reparse point included, fails.
                (true, true) => {
                    options.create_new(true).write(true);
                }
                (true, false) => {
                    options.create(true).write(true);
                }
                _ => {}
            }
            let file = options
                .share_mode(flags::SHARE_ALL)
                .custom_flags(flags::OPEN_REPARSE_POINT)
                .open(self.path.join(name))?;
            let metadata = file.metadata()?;
            // Separate messages on purpose: these refusals are the cache's
            // safety net, and one lumped condition cannot be diagnosed when it
            // fires on a machine that is not this one.
            ensure!(metadata.is_file(), "Cache: non è un file regolare");
            ensure!(
                metadata.file_attributes() & flags::ATTRIBUTE_REPARSE_POINT == 0,
                "Cache: punto di reparse rifiutato"
            );
            if !Self::links_allowed(write || create, create, exclusive) {
                let links = links(&file)?;
                ensure!(
                    links == 1,
                    "Cache: {name} ha {links} collegamenti, atteso 1"
                );
            }
            Ok(file)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (write, create, exclusive);
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
    /// Refresh a validated cache hit without reopening a potentially replaced
    /// path. Extra links may be read, but must never receive metadata writes.
    pub fn touch(file: &File) -> Result<()> {
        #[cfg(unix)]
        ensure!(
            file.metadata()?.nlink() == 1,
            "Cache: timestamp di hard link non modificato"
        );
        #[cfg(windows)]
        let file = {
            use std::os::windows::io::{AsRawHandle, FromRawHandle};
            use windows_sys::Win32::{
                Foundation::INVALID_HANDLE_VALUE,
                Storage::FileSystem::{FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, ReOpenFile},
            };
            ensure!(
                links(file)? == 1,
                "Cache: timestamp di hard link non modificato"
            );
            // SAFETY: the original handle is live; ReOpenFile pins the same
            // object, with attributes rights only. The owned File closes it.
            let handle = unsafe {
                ReOpenFile(
                    file.as_raw_handle(),
                    FILE_WRITE_ATTRIBUTES | FILE_READ_ATTRIBUTES,
                    flags::SHARE_ALL,
                    flags::OPEN_REPARSE_POINT,
                )
            };
            ensure!(
                handle != INVALID_HANDLE_VALUE,
                "Cache: aggiornamento timestamp: {}",
                std::io::Error::last_os_error()
            );
            let refreshed = unsafe { File::from_raw_handle(handle) };
            ensure!(
                links(&refreshed)? == 1,
                "Cache: timestamp di hard link non modificato"
            );
            refreshed
        };
        file.set_times(FileTimes::new().set_modified(SystemTime::now()))?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        Self::validate(name)?;
        // A readable payload is not necessarily ours to remove: readers permit
        // extra hard links (e.g. while a sync client uploads an entry). Check
        // again at every deletion, including replacement of invalid v1/v2 data
        // and cleanup of temporary files, rather than relying on an earlier scan.
        #[cfg(any(unix, windows))]
        let file = self.open_file(name, false, false, false)?;
        #[cfg(unix)]
        ensure!(
            file.metadata()?.nlink() == 1,
            "Cache: hard link non rimosso"
        );
        #[cfg(windows)]
        ensure!(links(&file)? == 1, "Cache: hard link non rimosso");
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
        #[cfg(windows)]
        {
            // The validated handle shares deletion, so a regular cache entry
            // can be unlinked while the handle stays alive until this returns.
            std::fs::remove_file(self.path.join(name))
                .map_err(|e| anyhow::anyhow!("Rimozione cache: {e}"))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = name;
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
    pub fn publish(&self, name: &str, destination: &Self, target: &str) -> Result<()> {
        Self::validate(name)?;
        Self::validate(target)?;
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
        #[cfg(windows)]
        {
            // Without MOVEFILE_REPLACE_EXISTING the move fails when the target
            // exists, which is the no-clobber rule the Unix branch relies on.
            // The temporary disappears in the same operation.
            let from = wide(&self.path.join(name));
            let to = wide(&destination.path.join(target));
            let moved = unsafe {
                windows_sys::Win32::Storage::FileSystem::MoveFileExW(from.as_ptr(), to.as_ptr(), 0)
            };
            ensure!(
                moved != 0,
                "Pubblicazione cache: {}",
                std::io::Error::last_os_error()
            );
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (name, destination, target);
            anyhow::bail!("Cache persistente non disponibile")
        }
    }
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Hard-link count of an open file. `std` exposes this only on nightly, so it
/// is asked of the handle directly rather than left unchecked.
#[cfg(windows)]
fn links(file: &File) -> Result<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    ensure!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0,
        "Cache: metadati del file non leggibili: {}",
        std::io::Error::last_os_error()
    );
    Ok(info.nNumberOfLinks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the narrowing, because it trades a refusal for usability and the
    /// reasoning has to survive the next person who reads the condition.
    #[test]
    fn extra_links_are_refused_only_where_a_write_could_be_redirected() {
        // Reading cannot corrupt what another link points at.
        assert!(Directory::links_allowed(false, false, false));
        // A file created here has one link at creation, by construction.
        assert!(Directory::links_allowed(true, true, true));
        assert!(!Directory::links_allowed(true, true, false));
        // Writing into an entry we did not create is the case the rule exists
        // for, and the only one that still refuses.
        assert!(!Directory::links_allowed(true, false, false));
    }

    #[test]
    fn nonexclusive_create_refuses_existing_hard_link_without_changing_target() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("original");
        std::fs::write(&target, b"untouched").unwrap();
        let cache_path = folder.path().join("cache");
        std::fs::create_dir(&cache_path).unwrap();
        std::fs::hard_link(&target, cache_path.join("cache.lock")).unwrap();
        let dir = Directory::open(&cache_path).unwrap();
        assert!(dir.open_file("cache.lock", true, true, false).is_err());
        assert!(dir.open_file("cache.lock", false, false, false).is_ok());
        assert!(dir.open_file("fresh", true, true, true).is_ok());
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    }

    /// Names that could leave the folder or address something other than a
    /// plain entry in it. `:` is Windows-specific and names a stream on an
    /// existing file, which would slip past a separator check alone.
    #[test]
    fn names_that_could_escape_the_folder_are_refused() {
        for bad in ["", ".", "..", "a/b", r"a\b", "a:stream"] {
            assert!(Directory::validate(bad).is_err(), "accettato: {bad:?}");
        }
        assert!(Directory::validate("0123abcd.tvc").is_ok());
    }

    #[test]
    fn touch_refreshes_read_handle_but_preserves_hard_link_targets() {
        use std::{io::Write, time::Duration};
        let folder = tempfile::tempdir().unwrap();
        let dir = Directory::open(folder.path()).unwrap();
        let old = SystemTime::now() - Duration::from_secs(86400 * 2);
        let mut writer = dir.open_file("entry", true, true, true).unwrap();
        writer.write_all(b"pixels").unwrap();
        writer.set_modified(old).unwrap();
        drop(writer);
        let mut reader = dir.open_file("entry", false, false, false).unwrap();
        Directory::touch(&reader).unwrap();
        assert!(reader.metadata().unwrap().modified().unwrap() > old + Duration::from_secs(86400));
        assert!(reader.write_all(b"corrupt").is_err());
        let before = reader.metadata().unwrap().modified().unwrap();
        std::fs::hard_link(folder.path().join("entry"), folder.path().join("other")).unwrap();
        assert!(Directory::touch(&reader).is_err());
        assert_eq!(reader.metadata().unwrap().modified().unwrap(), before);
        assert_eq!(
            std::fs::read(folder.path().join("entry")).unwrap(),
            b"pixels"
        );
    }

    #[test]
    fn removal_rechecks_links_added_after_a_directory_scan() {
        let folder = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let dir = Directory::open(folder.path()).unwrap();
        let path = folder.path().join("entry");
        std::fs::write(&path, b"cached pixels").unwrap();
        dir.file_info("entry").unwrap();
        let original = outside.path().join("retained");
        std::fs::hard_link(&path, &original).unwrap();
        let modified = std::fs::metadata(&original).unwrap().modified().unwrap();
        assert!(dir.open_file("entry", false, false, false).is_ok());
        assert!(dir.remove("entry").is_err());
        assert!(path.exists());
        assert_eq!(std::fs::read(&original).unwrap(), b"cached pixels");
        assert_eq!(
            std::fs::metadata(&original).unwrap().modified().unwrap(),
            modified
        );
        // Once the external link is gone, ordinary cache collection still works.
        std::fs::remove_file(&original).unwrap();
        dir.remove("entry").unwrap();
        assert!(!path.exists());
    }
}
