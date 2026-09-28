//! Bounded registry of explicitly visited caches. Never searches a disk recursively.
use super::{Folder, MIB, Manager, Settings};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::MutexGuard,
    time::{Duration, Instant},
};

const MAX_FOLDERS: usize = 256;
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

#[derive(Default, Clone)]
pub struct Summary {
    pub bytes: u64,
    pub folders: usize,
    pub unavailable: usize,
    pub measured: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Registry {
    folders: Vec<PathBuf>,
    #[serde(skip)]
    file: Option<PathBuf>,
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    summary: Summary,
}
impl Registry {
    fn save(&self) -> Result<()> {
        if let Some(file) = &self.file {
            let bytes = serde_json::to_vec(self)?;
            ensure!(
                bytes.len() as u64 <= MAX_BYTES,
                "Registro cache troppo grande"
            );
            let mut temp = tempfile::NamedTempFile::new_in(file.parent().unwrap())?;
            temp.write_all(&bytes)?;
            temp.as_file().sync_all()?;
            temp.persist(file)?;
        }
        Ok(())
    }
    pub(super) fn register(&mut self, folder: &Path) -> Result<()> {
        ensure!(
            self.error.is_none(),
            "Registro cache non disponibile: {}",
            self.error.as_deref().unwrap_or_default()
        );
        let folder = folder.canonicalize()?;
        if !self.folders.contains(&folder) {
            ensure!(
                self.folders.len() < MAX_FOLDERS,
                "Registro cache pieno (256 cartelle): persistenza saltata"
            );
            self.folders.push(folder);
            if let Err(error) = self.save() {
                self.folders.pop();
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) fn sweep(
        &mut self,
        settings: &Settings,
        reserved: u64,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        ensure!(self.error.is_none(), "Registro cache non disponibile");
        let started = Instant::now();
        let mut summary = Summary {
            folders: self.folders.len(),
            measured: true,
            ..Default::default()
        };
        let mut candidates = Vec::new();
        let quota = settings.disk_mib * MIB;
        let deadline = || cancelled() || started.elapsed() > Duration::from_secs(10);
        for (index, folder) in self.folders.iter().enumerate() {
            if deadline() {
                summary.unavailable += self.folders.len() - index;
                break;
            }
            let scan = (|| -> Result<()> {
                let cache = Folder::open(folder, false, true)?;
                // The legacy per-folder collector deliberately skips protected
                // files. Aggregate admission must not mistake them for free space.
                for (directory, suffix) in [(&cache.entries, ".tvc"), (&cache.tmp, ".part")] {
                    for (count, child) in std::fs::read_dir(&directory.path)?.enumerate() {
                        ensure!(!deadline(), "Manutenzione cache annullata");
                        ensure!(count < MAX_ENTRIES, "Indice cache oltre quota");
                        let name = child?.file_name().to_string_lossy().into_owned();
                        if super::managed(&name, suffix) {
                            directory.file_info(&name)?;
                        }
                    }
                }
                let (entries, _) = cache.trim_cancellable(
                    if settings.global_disk_quota {
                        u64::MAX
                    } else {
                        quota
                    },
                    settings.retention_days(),
                    quota / 5,
                    &deadline,
                )?;
                ensure!(
                    candidates.len() + entries.len() <= MAX_ENTRIES,
                    "Indice globale cache oltre quota"
                );
                for entry in entries {
                    if entry.modified.elapsed().unwrap_or_default()
                        > Duration::from_secs(settings.retention_days() as u64 * 86400)
                    {
                        summary.unavailable += 1;
                    }
                    summary.bytes = summary.bytes.saturating_add(entry.bytes);
                    candidates.push((index, entry));
                }
                Ok(())
            })();
            if scan.is_err() {
                summary.unavailable += 1;
            }
        }
        // Oldest first across every available registered folder. Recheck each
        // candidate under its exclusive folder lock before deleting it.
        candidates.sort_by_key(|(_, entry)| entry.modified);
        let limit = if settings.global_disk_quota {
            quota.saturating_sub(reserved)
        } else {
            u64::MAX
        };
        for (index, entry) in candidates {
            if summary.bytes <= limit {
                break;
            }
            if deadline() {
                summary.unavailable += 1;
                break;
            }
            let removed = (|| -> Result<bool> {
                let cache = Folder::open(&self.folders[index], false, true)?;
                let (bytes, modified) = cache.entries.file_info(&entry.name)?;
                if bytes != entry.bytes || modified != entry.modified {
                    return Ok(false);
                }
                cache.entries.remove(&entry.name)?;
                Ok(true)
            })();
            if matches!(removed, Ok(true)) {
                summary.bytes = summary.bytes.saturating_sub(entry.bytes);
            } else {
                summary.unavailable += 1;
            }
        }
        self.summary = summary;
        ensure!(
            self.summary.unavailable == 0,
            "Pulizia cache parziale: cartelle occupate/non disponibili o limite di scansione"
        );
        ensure!(
            self.summary.bytes <= limit,
            "Quota totale cache non applicabile; persistenza saltata"
        );
        Ok(())
    }
}
impl Manager {
    pub fn load_registry(&self, data: &Path) {
        let file = data.join("cache-folders.json");
        let loaded = (|| -> Result<Registry> {
            if !file.exists() {
                return Ok(Registry::default());
            }
            let metadata = std::fs::symlink_metadata(&file)?;
            ensure!(
                metadata.is_file() && metadata.len() <= MAX_BYTES,
                "Registro cache non valido"
            );
            let registry: Registry = serde_json::from_slice(&std::fs::read(&file)?)?;
            ensure!(
                registry.folders.len() <= MAX_FOLDERS
                    && registry.folders.iter().all(|p| p.is_absolute()),
                "Registro cache fuori intervallo"
            );
            let unique: std::collections::HashSet<_> = registry.folders.iter().collect();
            ensure!(
                unique.len() == registry.folders.len(),
                "Registro cache duplicato"
            );
            Ok(registry)
        })();
        let mut registry = match loaded {
            Ok(registry) => registry,
            Err(error) => {
                self.note(format!(
                    "Registro cache conservato, manutenzione sospesa: {error:#}"
                ));
                Registry {
                    error: Some(error.to_string()),
                    ..Default::default()
                }
            }
        };
        registry.file = Some(file);
        *self.disk_policy.lock().unwrap() = registry;
    }
    pub fn known_cache_summary(&self) -> Option<Summary> {
        self.disk_policy
            .try_lock()
            .ok()
            .map(|registry| registry.summary.clone())
    }
    pub fn maintain_known(&self, cancelled: &impl Fn() -> bool) -> Result<()> {
        self.wait_for_readers(cancelled)?;
        let mut registry = self.disk_policy.lock().unwrap();
        registry.sweep(&self.settings(), 0, cancelled)
    }
    pub(super) fn reserve_disk(
        &self,
        folder: &Path,
        bytes: u64,
        cancelled: &impl Fn() -> bool,
    ) -> Result<MutexGuard<'_, Registry>> {
        let settings = self.settings();
        ensure!(
            bytes <= settings.disk_mib * MIB && bytes <= settings.temporary_mib * MIB,
            "Derivato oltre quota disco/temporanei"
        );
        self.wait_for_readers(cancelled)?;
        let mut registry = self.disk_policy.lock().unwrap();
        // Validate ownership before persisting a path, never adopt another folder.
        drop(Folder::open(folder, true, true)?);
        registry.register(folder)?;
        if settings.global_disk_quota {
            registry.sweep(&settings, bytes, cancelled)?;
        }
        Ok(registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::FileTimes, time::SystemTime};

    fn entry(folder: &Path, key: char, bytes: u64, age_days: u64) -> PathBuf {
        std::fs::create_dir_all(folder).unwrap();
        let cache = Folder::open(folder, true, true).unwrap();
        let name = format!("{}.tvc", key.to_string().repeat(64));
        let file = cache.entries.open_file(&name, true, true, true).unwrap();
        file.set_len(bytes).unwrap();
        file.set_times(
            FileTimes::new()
                .set_modified(SystemTime::now() - Duration::from_secs(age_days * 86400)),
        )
        .unwrap();
        cache.entries.path.join(name)
    }

    #[test]
    fn cancellation_between_removals_stops_expiry_within_one_folder() {
        let root = tempfile::tempdir().unwrap();
        let a = entry(root.path(), 'a', 10, 33);
        let b = entry(root.path(), 'b', 10, 32);
        let c = entry(root.path(), 'c', 10, 31);
        let cache = Folder::open(root.path(), false, true).unwrap();
        // Cancellation becomes observable as soon as the first removal finishes.
        assert!(
            cache
                .trim_cancellable(u64::MAX, 30, 0, &|| !a.exists())
                .is_err()
        );
        assert!(!a.exists());
        assert!(b.exists() && c.exists());
    }

    #[test]
    fn cancellation_during_folder_scan_preserves_remaining_entries() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("photos");
        let old = entry(&folder, 'a', 10, 31);
        let other = entry(&folder, 'b', 10, 32);
        let manager = Manager::new(Settings {
            expire_unused: false,
            ..Default::default()
        });
        manager.maintain(&folder, false).unwrap();
        manager.configure(Settings::default());
        let checks = std::cell::Cell::new(0);
        let cancelled = || {
            checks.set(checks.get() + 1);
            checks.get() >= 3
        };
        assert!(manager.maintain_known(&cancelled).is_err());
        assert!(old.exists() && other.exists());
        assert!(manager.known_cache_summary().unwrap().unavailable > 0);
    }

    #[test]
    fn global_quota_uses_oldest_across_folders_and_survives_restart() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        let old = entry(&a, 'a', 40 * MIB, 3);
        let recent = entry(&b, 'b', 40 * MIB, 1);
        let original = b.join("original.nef");
        std::fs::write(&original, b"original").unwrap();
        let manager = Manager::new(Settings {
            disk_mib: 64,
            global_disk_quota: false,
            expire_unused: false,
            ..Default::default()
        });
        manager.load_registry(&data);
        manager.maintain(&a, false).unwrap();
        manager.maintain(&b, false).unwrap();
        assert!(old.exists() && recent.exists());
        let manager = Manager::new(Settings {
            disk_mib: 64,
            global_disk_quota: true,
            expire_unused: false,
            ..Default::default()
        });
        manager.load_registry(&data);
        manager.maintain_known(&|| false).unwrap();
        assert!(!old.exists() && recent.exists());
        assert_eq!(std::fs::read(original).unwrap(), b"original");
        let summary = manager.known_cache_summary().unwrap();
        assert_eq!(
            (summary.folders, summary.bytes, summary.unavailable),
            (2, 40 * MIB, 0)
        );
        // A pending new artifact must fit before its first byte is published.
        drop(manager.reserve_disk(&b, 30 * MIB, &|| false).unwrap());
        assert!(!recent.exists());
    }

    #[test]
    fn expiry_switch_and_periodic_switch_are_independent() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("photos");
        let old = entry(&folder, 'a', 10, 31);
        let manager = std::sync::Arc::new(Manager::new(Settings {
            expire_unused: false,
            clean_known_folders: false,
            ..Default::default()
        }));
        manager.maintain(&folder, false).unwrap();
        manager.maintain_known(&|| false).unwrap();
        assert!(old.exists());
        let mut settings = manager.settings();
        settings.expire_unused = true;
        manager.configure(settings.clone());
        // Disabled background option performs no sweep, even though expiry is enabled.
        let writer = super::super::writer::Writer::start(
            manager.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        std::thread::sleep(Duration::from_millis(100));
        drop(writer);
        assert!(old.exists());
        settings.clean_known_folders = true;
        manager.configure(settings);
        let writer = super::super::writer::Writer::start(
            manager.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        let start = Instant::now();
        while old.exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(writer);
        assert!(!old.exists());
    }

    #[test]
    fn inaccessible_or_unrecognized_cache_blocks_global_writes_without_adoption() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        entry(&a, 'a', 10, 1);
        entry(&b, 'b', 10, 1);
        let manager = Manager::new(Settings {
            global_disk_quota: true,
            ..Default::default()
        });
        manager.maintain(&a, false).unwrap();
        manager.maintain(&b, false).unwrap();
        let held = Folder::open(&a, false, false).unwrap();
        assert!(manager.reserve_disk(&b, 10, &|| false).is_err());
        drop(held);
        std::fs::write(a.join(super::super::NAME).join("OWNER"), b"not ours").unwrap();
        assert!(manager.maintain_known(&|| false).is_err());
        assert_eq!(
            std::fs::read(a.join(super::super::NAME).join("OWNER")).unwrap(),
            b"not ours"
        );
        assert!(manager.reserve_disk(&b, 10, &|| false).is_err());
        assert!(manager.known_cache_summary().unwrap().unavailable > 0);
    }

    #[test]
    fn malformed_registry_is_preserved_and_cancelled_scan_does_not_delete() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("cache-folders.json");
        std::fs::write(&file, b"broken").unwrap();
        let manager = Manager::new(Settings::default());
        manager.load_registry(root.path());
        let folder = root.path().join("photos");
        let old = entry(&folder, 'a', 10, 31);
        assert!(manager.maintain(&folder, false).is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"broken");
        assert!(old.exists());
        manager.maintain(&folder, true).unwrap();
        assert!(!old.exists());
        let old = entry(&folder, 'a', 10, 31);
        let manager = Manager::new(Settings {
            expire_unused: false,
            ..Default::default()
        });
        manager.maintain(&folder, false).unwrap();
        manager.configure(Settings::default());
        assert!(manager.maintain_known(&|| true).is_err());
        assert!(old.exists());
    }

    #[cfg(unix)]
    #[test]
    fn global_cleanup_preserves_hardlinks_and_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a");
        let old = entry(&a, 'a', 10, 31);
        let external = root.path().join("protected");
        std::fs::hard_link(&old, &external).unwrap();
        let manager = Manager::new(Settings {
            expire_unused: false,
            ..Default::default()
        });
        manager.maintain(&a, false).unwrap();
        manager.configure(Settings {
            global_disk_quota: true,
            ..Default::default()
        });
        assert!(manager.maintain_known(&|| false).is_err());
        assert!(old.exists() && external.exists());
        let modified = std::fs::metadata(&old).unwrap().modified().unwrap();
        assert_eq!(
            std::fs::metadata(&external).unwrap().modified().unwrap(),
            modified
        );
        let link = root.path().join("alias");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert!(manager.maintain(&link, false).is_err());
        // Ensure the external link still names the same nonempty file.
        assert_eq!(
            std::fs::OpenOptions::new()
                .read(true)
                .open(external)
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            10
        );
    }
}
