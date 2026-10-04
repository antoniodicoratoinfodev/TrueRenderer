use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};
use tr_core::preview::PreviewQuality;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum PerformanceProfile {
    Performance,
    #[default]
    Balanced,
    Saver,
}
pub const BASE_MEMORY_MIB: u64 = 8_000_000_000u64.div_ceil(1024 * 1024);

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum FolderLoading {
    #[default]
    Background,
    Foreground,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub presentation: tr_core::presentation::Precision,
    pub language: crate::i18n::Language,
    pub raw_engine: tr_core::decoder::RawEngine,
    pub schema: u32,
    pub enabled: bool,
    pub disk_mib: u64,
    pub global_disk_quota: bool,
    pub expire_unused: bool,
    pub clean_known_folders: bool,
    pub temporary_mib: u64,
    pub unused_days: u32,
    pub free_mib: u64,
    pub quality: PreviewQuality,
    /// Initial automatic budget, not a hard cap or a preallocation.
    pub memory_mib: u64,
    pub reusable_mib: Option<u64>,
    pub gpu_mib: u64,
    pub profile: PerformanceProfile,
    pub adapt_on_battery: bool,
    pub cpu_threads: usize,
    /// Isolated tests can still exercise fixed quotas and disable speculation.
    #[serde(skip)]
    pub diagnostic_fixed_memory: bool,
    #[serde(skip)]
    pub diagnostic_no_prefetch: bool,
    pub folder_loading: FolderLoading,
    pub compute: tr_core::preview::ImageCompute,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            presentation: Default::default(),
            language: crate::i18n::Language::default(),
            raw_engine: tr_core::decoder::RawEngine::default(),
            schema: 3,
            enabled: true,
            disk_mib: 4096,
            global_disk_quota: false,
            expire_unused: true,
            clean_known_folders: false,
            temporary_mib: 2048,
            unused_days: 30,
            free_mib: 512,
            quality: PreviewQuality::Standard,
            memory_mib: BASE_MEMORY_MIB,
            reusable_mib: None,
            gpu_mib: 0,
            profile: PerformanceProfile::Performance,
            adapt_on_battery: false,
            cpu_threads: 0,
            diagnostic_fixed_memory: false,
            diagnostic_no_prefetch: false,
            folder_loading: FolderLoading::Background,
            compute: tr_core::preview::ImageCompute::Automatic,
        }
    }
}
impl Settings {
    pub fn retention_days(&self) -> u32 {
        if self.expire_unused {
            self.unused_days
        } else {
            u32::MAX
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 3, "Versione impostazioni non supportata");
        ensure!(
            self.raw_engine.available() || !cfg!(any(windows, target_os = "macos")),
            "Motore RAW non disponibile su questa piattaforma: scegliere un motore nelle impostazioni"
        );
        ensure!(
            (64..=65536).contains(&self.disk_mib)
                && (16..=2048).contains(&self.temporary_mib)
                && (1..=3650).contains(&self.unused_days)
                && self.free_mib <= 65536,
            "Impostazioni cache fuori intervallo"
        );
        ensure!(
            ((self.diagnostic_fixed_memory && self.memory_mib > 0)
                || (512..=786432).contains(&self.memory_mib))
                && self.reusable_mib.is_none_or(|v| v <= 786432)
                && self.gpu_mib <= 786432
                && self.cpu_threads <= 256,
            "Impostazioni memoria/thread fuori intervallo"
        );
        Ok(())
    }
    /// Call before Catalog::open creates library.sqlite. The directory/instance
    /// lock alone does not identify an existing installation.
    pub fn load(data: &Path) -> Result<Self> {
        let settings = Self::read(data)?;
        settings.validate()?;
        Ok(settings)
    }
    fn read(data: &Path) -> Result<Self> {
        let path = data.join("settings.json");
        if !path.exists() {
            return Ok(Self {
                quality: if data.join("library.sqlite").exists() {
                    PreviewQuality::Full
                } else {
                    PreviewQuality::Standard
                },
                ..Self::default()
            });
        }
        ensure!(
            path.metadata()?.len() <= 16384,
            "Impostazioni troppo grandi"
        );
        let bytes = std::fs::read(&path)?;
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let legacy = value.get("schema").is_none();
        if legacy || value.get("schema").and_then(|v| v.as_u64()) == Some(2) {
            let object = value
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("Formato impostazioni non valido"))?;
            object.remove("prefetch");
            let base = object
                .get("memory_mib")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            object.insert("memory_mib".into(), base.max(BASE_MEMORY_MIB).into());
            object.insert("schema".into(), 3.into());
        }
        let mut settings: Self = serde_json::from_value(value)?;
        // Stored preferences always describe the automatic base. Smaller fixed
        // budgets exist only in explicitly configured in-memory diagnostics.
        settings.memory_mib = settings.memory_mib.max(BASE_MEMORY_MIB);
        if legacy {
            settings.quality = PreviewQuality::Full;
        }
        Ok(settings)
    }
    pub fn load_or_recover(data: &Path) -> (Self, Option<String>) {
        // Preserve the exact original before the normal save publishes schema 3.
        let path = data.join("settings.json");
        if path
            .metadata()
            .is_ok_and(|metadata| metadata.len() <= 16384)
            && let Ok(bytes) = std::fs::read(&path)
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && (value.get("schema").is_none()
                || value.get("schema").and_then(|v| v.as_u64()) == Some(2))
            && let Err(error) = Self::backup(data, "settings-before-automatic-")
        {
            return (
                Self::load(data).unwrap_or_else(|_| Self {
                    quality: PreviewQuality::Full,
                    ..Self::default()
                }),
                Some(format!("Migrazione non salvata: {error:#}")),
            );
        }
        // Preserve the other preferences and the original cross-platform JSON.
        if let Ok(mut settings) = Self::read(data)
            && cfg!(any(windows, target_os = "macos"))
            && !settings.raw_engine.available()
        {
            let previous = settings.raw_engine;
            settings.raw_engine = Default::default();
            if settings.validate().is_ok() {
                let saved = (|| -> Result<()> {
                    Self::backup(data, "settings-platform-")?;
                    settings.save(data)
                })();
                let suffix = saved
                    .err()
                    .map(|e| format!("; migrazione non salvata: {e:#}"))
                    .unwrap_or_default();
                return (
                    settings,
                    Some(format!(
                        "Motore {previous:?} non disponibile: selezionato il default della piattaforma; altre preferenze conservate{suffix}"
                    )),
                );
            }
        }
        match Self::load(data) {
            Ok(settings) => {
                let message = settings
                    .save(data)
                    .err()
                    .map(|e| format!("Salvataggio impostazioni fallito: {e:#}"));
                (settings, message)
            }
            Err(error) => {
                let settings = Self {
                    quality: PreviewQuality::Full,
                    ..Self::default()
                };
                let recovery = (|| -> Result<()> {
                    // Persist the original bytes before replacing anything, including
                    // unknown future schemas. Failure leaves settings.json untouched.
                    Self::backup(data, "settings-damaged-")?;
                    settings.save(data)
                })();
                let suffix = recovery
                    .err()
                    .map(|e| format!("; recupero non salvato: {e:#}"))
                    .unwrap_or_default();
                (
                    settings,
                    Some(format!(
                        "Impostazioni non valide: {error:#}. Default Piena; originale conservato{suffix}"
                    )),
                )
            }
        }
    }
    fn backup(data: &Path, prefix: &str) -> Result<()> {
        let mut backup = tempfile::Builder::new()
            .prefix(prefix)
            .suffix(".json")
            .tempfile_in(data)?;
        std::io::copy(
            &mut std::fs::File::open(data.join("settings.json"))?,
            &mut backup,
        )?;
        backup.as_file().sync_all()?;
        backup.keep()?;
        Ok(())
    }
    pub fn save(&self, data: &Path) -> Result<()> {
        self.validate()?;
        std::fs::create_dir_all(data)?;
        let mut temp = tempfile::NamedTempFile::new_in(data)?;
        temp.write_all(&serde_json::to_vec_pretty(self)?)?;
        temp.as_file().sync_all()?;
        temp.persist(data.join("settings.json"))?;
        Ok(())
    }
    pub fn threads_for_power(&self, battery: bool) -> usize {
        let threads = self.effective_threads();
        if battery && self.adapt_on_battery {
            threads.min(2)
        } else {
            threads
        }
    }
    pub fn effective_threads(&self) -> usize {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        if self.cpu_threads > 0 {
            return self.cpu_threads.min(available);
        }
        match self.profile {
            PerformanceProfile::Performance => available.saturating_sub(1).max(1),
            PerformanceProfile::Balanced => (available / 2).max(1),
            PerformanceProfile::Saver => 1,
        }
    }
    pub fn initial_memory_bytes(&self) -> u64 {
        if self.diagnostic_fixed_memory {
            self.memory_mib * 1024 * 1024
        } else {
            self.memory_mib.max(BASE_MEMORY_MIB) * 1024 * 1024
        }
    }
    pub fn reusable_bytes(&self, budget: u64) -> u64 {
        self.reusable_mib
            .map_or(budget * 3 / 5, |mib| mib * 1024 * 1024)
            .min(budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn failed_migration_backup_never_returns_invalid_preferences() {
        use std::os::unix::fs::PermissionsExt;
        for memory_mib in [2048, u64::MAX] {
            let data = tempfile::tempdir().unwrap();
            let original = serde_json::to_vec(&serde_json::json!({
                "schema": 2, "memory_mib": memory_mib, "language": "it"
            }))
            .unwrap();
            std::fs::write(data.path().join("settings.json"), &original).unwrap();
            let permissions = std::fs::metadata(data.path()).unwrap().permissions();
            std::fs::set_permissions(data.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
            let (settings, warning) = Settings::load_or_recover(data.path());
            std::fs::set_permissions(data.path(), permissions).unwrap();
            assert!(warning.unwrap().contains("Migrazione non salvata"));
            assert!(
                settings.validate().is_ok(),
                "Invalid preferences escaped recovery"
            );
            if memory_mib == 2048 {
                assert_eq!(settings.language, crate::i18n::Language::Italian);
            } else {
                assert_eq!(settings.quality, PreviewQuality::Full);
            }
            assert_eq!(
                std::fs::read(data.path().join("settings.json")).unwrap(),
                original
            );
        }
    }
    #[test]
    fn automatic_migration_preserves_preferences_and_backs_up_all_legacy_prefetch_modes() {
        for prefetch in ["Disabled", "Automatic", "Extended"] {
            let data = tempfile::tempdir().unwrap();
            let original = serde_json::to_vec(&serde_json::json!({
                "schema":2, "prefetch":prefetch, "memory_mib":2048,
                "language":"it", "disk_mib":8192, "quality":"Full",
                "folder_loading":"Foreground", "adapt_on_battery":true
            }))
            .unwrap();
            std::fs::write(data.path().join("settings.json"), &original).unwrap();
            let (settings, warning) = Settings::load_or_recover(data.path());
            assert!(warning.is_none(), "{warning:?}");
            assert_eq!(settings.schema, 3);
            assert!(settings.initial_memory_bytes() >= 8_000_000_000);
            assert_eq!(settings.language, crate::i18n::Language::Italian);
            assert_eq!(settings.disk_mib, 8192);
            assert_eq!(settings.quality, PreviewQuality::Full);
            assert_eq!(settings.folder_loading, FolderLoading::Foreground);
            assert!(settings.adapt_on_battery);
            assert!(std::fs::read_dir(data.path()).unwrap().flatten().any(|p| {
                p.file_name()
                    .to_string_lossy()
                    .starts_with("settings-before-automatic-")
                    && std::fs::read(p.path()).unwrap() == original
            }));
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(data.path().join("settings.json")).unwrap())
                    .unwrap();
            assert!(saved.get("prefetch").is_none());
            assert_eq!(Settings::load(data.path()).unwrap(), settings);
        }
        let settings = Settings::default();
        assert!(!settings.adapt_on_battery);
        assert_eq!(
            settings.threads_for_power(true),
            settings.threads_for_power(false)
        );
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), br#"{"schema":2}"#).unwrap();
        assert!(!Settings::load(data.path()).unwrap().adapt_on_battery);
    }
    #[test]
    fn cache_controls_preserve_legacy_defaults_and_roundtrip_independently() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(
            data.path().join("settings.json"),
            br#"{"schema":2,"disk_mib":8192,"unused_days":17}"#,
        )
        .unwrap();
        let mut settings = Settings::load(data.path()).unwrap();
        assert!(
            !settings.global_disk_quota && !settings.clean_known_folders && settings.expire_unused
        );
        assert_eq!(settings.retention_days(), 17);
        for global in [false, true] {
            for expiry in [false, true] {
                for periodic in [false, true] {
                    settings.global_disk_quota = global;
                    settings.expire_unused = expiry;
                    settings.clean_known_folders = periodic;
                    settings.save(data.path()).unwrap();
                    let loaded = Settings::load(data.path()).unwrap();
                    assert_eq!(loaded, settings);
                    assert_eq!((loaded.disk_mib, loaded.unused_days), (8192, 17));
                    assert_eq!(loaded.retention_days(), if expiry { 17 } else { u32::MAX });
                }
            }
        }
    }
    #[test]
    fn language_defaults_to_english_and_roundtrips_without_losing_preferences() {
        use crate::i18n::Language;
        let data = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(data.path()).unwrap().language,
            Language::English
        );
        std::fs::write(
            data.path().join("settings.json"),
            br#"{"schema":2,"disk_mib":8192,"quality":"Full"}"#,
        )
        .unwrap();
        let mut settings = Settings::load(data.path()).unwrap();
        assert_eq!(settings.language, Language::English);
        for language in [Language::Italian, Language::English] {
            settings.language = language;
            settings.save(data.path()).unwrap();
            assert_eq!(Settings::load(data.path()).unwrap(), settings);
            assert_eq!(settings.disk_mib, 8192);
            assert_eq!(settings.quality, PreviewQuality::Full);
        }
    }
    #[cfg(windows)]
    #[test]
    fn apple_preference_migrates_with_warning_and_original_backup() {
        let dir = tempfile::tempdir().unwrap();
        let original = br#"{"schema":2,"raw_engine":"Apple","disk_mib":8192,"quality":"Full"}"#;
        std::fs::write(dir.path().join("settings.json"), original).unwrap();
        let (settings, message) = Settings::load_or_recover(dir.path());
        assert_eq!(settings.raw_engine, Default::default());
        assert_eq!(settings.disk_mib, 8192);
        assert_eq!(settings.quality, PreviewQuality::Full);
        assert!(message.unwrap().contains("non disponibile"));
        let backup = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("settings-platform-")
            })
            .unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), original);
        assert_eq!(Settings::load(dir.path()).unwrap(), settings);
    }
    #[test]
    fn raw_engine_roundtrips_and_old_settings_keep_the_platform_default() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), br#"{"schema":2}"#).unwrap();
        assert_eq!(
            Settings::load(data.path()).unwrap().raw_engine,
            Default::default()
        );
        let settings = Settings {
            raw_engine: tr_core::decoder::RawEngine::TrueRenderer,
            ..Settings::default()
        };
        settings.save(data.path()).unwrap();
        assert_eq!(Settings::load(data.path()).unwrap(), settings);
    }
    #[test]
    fn new_legacy_and_corrupt_installations_preserve_the_contract() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(data.path()).unwrap().quality,
            PreviewQuality::Standard
        );
        std::fs::write(data.path().join("library.sqlite"), b"test").unwrap();
        assert_eq!(
            Settings::load(data.path()).unwrap().quality,
            PreviewQuality::Full
        );
        std::fs::write(data.path().join("settings.json"), br#"{"enabled":false,"disk_mib":8192,"temporary_mib":1024,"unused_days":17,"free_mib":99}"#).unwrap();
        let (settings, warning) = Settings::load_or_recover(data.path());
        assert!(warning.is_none());
        assert_eq!(
            (
                settings.quality,
                settings.enabled,
                settings.disk_mib,
                settings.unused_days
            ),
            (PreviewQuality::Full, false, 8192, 17)
        );
        assert_eq!(Settings::load(data.path()).unwrap(), settings);
        std::fs::write(data.path().join("settings.json"), b"broken").unwrap();
        let (_, warning) = Settings::load_or_recover(data.path());
        assert!(warning.is_some());
        assert!(std::fs::read_dir(data.path()).unwrap().flatten().any(|p| {
            p.file_name()
                .to_string_lossy()
                .starts_with("settings-damaged-")
                && std::fs::read(p.path()).unwrap() == b"broken"
        }));
        assert_eq!(
            std::fs::read(data.path().join("library.sqlite")).unwrap(),
            b"test"
        );
    }
    #[test]
    fn invalid_numbers_and_failed_writes_are_errors() {
        let data = tempfile::tempdir().unwrap();
        let bad = Settings {
            memory_mib: 7,
            ..Settings::default()
        };
        assert!(bad.save(data.path()).is_err());
        std::fs::create_dir(data.path().join("settings.json")).unwrap();
        assert!(Settings::default().save(data.path()).is_err());
        let disabled = Settings {
            reusable_mib: Some(0),
            ..Settings::default()
        };
        assert_eq!(disabled.reusable_bytes(16384), 0);
    }
}
