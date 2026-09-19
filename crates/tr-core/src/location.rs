//! Lossless, platform-tagged locators. Display strings are never identity keys.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub version: u32,
    pub platform: String,
    pub bytes: Vec<u8>,
}
impl Location {
    pub fn from_path(path: &Path) -> Self {
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(windows)]
        let bytes = {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        Self {
            version: 1,
            platform: if cfg!(windows) { "windows" } else { "unix" }.into(),
            bytes,
        }
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 1 && matches!(self.platform.as_str(), "unix" | "windows"),
            "Unsupported location format"
        );
        anyhow::ensure!(
            !self.bytes.is_empty() && self.bytes.len() <= 32768,
            "Location length limit"
        );
        anyhow::ensure!(
            if self.platform == "windows" {
                self.bytes.len().is_multiple_of(2)
                    && !self.bytes.as_chunks::<2>().0.contains(&[0, 0])
            } else {
                !self.bytes.contains(&0)
            },
            "Invalid native location"
        );
        Ok(())
    }
    pub fn path(&self) -> Option<PathBuf> {
        self.validate().ok()?;
        #[cfg(unix)]
        if self.platform == "unix" {
            use std::os::unix::ffi::OsStringExt;
            return Some(std::ffi::OsString::from_vec(self.bytes.clone()).into());
        }
        #[cfg(windows)]
        if self.platform == "windows" {
            use std::os::windows::ffi::OsStringExt;
            return Some(
                std::ffi::OsString::from_wide(
                    &self
                        .bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>(),
                )
                .into(),
            );
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorite {
    pub id: String,
    pub label: String,
    pub location: Location,
    pub revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn invalid_utf8_round_trips_without_display_collisions() {
        use std::os::unix::ffi::OsStringExt;
        let a = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/a\xff".to_vec()));
        let b = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/a\xfe".to_vec()));
        assert_eq!(a.to_string_lossy(), b.to_string_lossy());
        let location = Location::from_path(&a);
        assert_ne!(location, Location::from_path(&b));
        assert_eq!(
            serde_json::from_slice::<Location>(&serde_json::to_vec(&location).unwrap())
                .unwrap()
                .path(),
            Some(a)
        );
    }
}
