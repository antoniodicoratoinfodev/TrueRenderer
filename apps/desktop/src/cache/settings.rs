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
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum Prefetch {
    Disabled,
    #[default]
    Automatic,
    Extended,
}

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
    pub temporary_mib: u64,
    pub unused_days: u32,
    pub free_mib: u64,
    pub quality: PreviewQuality,
    /// Zero selects automatic. Reusable cache uses Option: Some(0) disables retention.
    pub memory_mib: u64,
    pub reusable_mib: Option<u64>,
    pub gpu_mib: u64,
    pub profile: PerformanceProfile,
    pub adapt_on_battery: bool,
    pub cpu_threads: usize,
    pub prefetch: Prefetch,
    pub folder_loading: FolderLoading,
    pub compute: tr_core::preview::ImageCompute,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            presentation: Default::default(),
            language: crate::i18n::Language::default(),
            raw_engine: tr_core::decoder::RawEngine::default(),
            schema: 2,
            enabled: true,
            disk_mib: 4096,
            temporary_mib: 2048,
            unused_days: 30,
            free_mib: 512,
            quality: PreviewQuality::Standard,
            memory_mib: 0,
            reusable_mib: None,
            gpu_mib: 0,
            profile: PerformanceProfile::Performance,
            adapt_on_battery: true,
            cpu_threads: 0,
            prefetch: Prefetch::Automatic,
            folder_loading: FolderLoading::Background,
            compute: tr_core::preview::ImageCompute::Automatic,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 2, "Versione impostazioni non supportata");
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
            (self.memory_mib == 0 || (512..=786432).contains(&self.memory_mib))
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
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let legacy = value.get("schema").is_none();
        let mut settings: Self = serde_json::from_value(value)?;
        if legacy {
            settings.quality = PreviewQuality::Full;
        }
        Ok(settings)
    }
    pub fn load_or_recover(data: &Path) -> (Self, Option<String>) {
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
    pub fn effective_memory_mib(&self, physical_mib: u64) -> u64 {
        let maximum = physical_mib.saturating_mul(3) / 4;
        if self.memory_mib == 0 {
            2048.min(physical_mib / 4)
        } else {
            self.memory_mib.min(maximum)
        }
    }
    pub fn reusable_bytes(&self, physical_mib: u64) -> u64 {
        let total = self.effective_memory_mib(physical_mib);
        self.reusable_mib.unwrap_or(total * 40 / 100).min(total) * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
