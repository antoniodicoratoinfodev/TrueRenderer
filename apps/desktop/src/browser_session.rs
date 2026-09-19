use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
};
use tr_core::location::Location;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelMode {
    #[default]
    Library,
    Explorer,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Session {
    pub schema: u32,
    pub mode: PanelMode,
    pub visible: bool,
    pub library_width: f32,
    pub explorer_width: f32,
    pub library_scroll: f32,
    pub explorer_scroll: f32,
    pub recent: Vec<Location>,
    pub roots: Vec<Location>,
    pub expanded: Vec<Location>,
    pub remember_recent: bool,
    pub show_hidden: bool,
    pub entry_filter: u8,
    pub name_filter: String,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            schema: 1,
            mode: PanelMode::Library,
            visible: true,
            library_width: 208.,
            explorer_width: 264.,
            library_scroll: 0.,
            explorer_scroll: 0.,
            recent: vec![],
            roots: vec![],
            expanded: vec![],
            remember_recent: true,
            show_hidden: false,
            entry_filter: 0,
            name_filter: String::new(),
        }
    }
}
impl Session {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 1, "Unsupported browser session version");
        ensure!(
            self.roots.len() <= 128
                && self.expanded.len() <= 128
                && self.recent.len() <= 20
                && self.name_filter.len() <= 1024
                && self.entry_filter <= 2,
            "Browser session limits"
        );
        ensure!(
            (192. ..=270.).contains(&self.library_width)
                && (200. ..=420.).contains(&self.explorer_width)
                && self.library_scroll.is_finite()
                && self.explorer_scroll.is_finite(),
            "Invalid browser geometry"
        );
        for location in self.roots.iter().chain(&self.recent).chain(&self.expanded) {
            location.validate()?;
        }
        Ok(())
    }
    pub fn load(data: &Path) -> (Self, Option<String>) {
        let path = data.join("browser-state.json");
        let read = || -> Result<Self> {
            let file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
                Err(e) => return Err(e.into()),
            };
            let mut bytes = Vec::new();
            file.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 2 * 1024 * 1024,
                "Browser session exceeds 2 MiB"
            );
            let session: Self = serde_json::from_slice(&bytes)?;
            session.validate()?;
            Ok(session)
        };
        match read() {
            Ok(session) => (session, None),
            Err(error) => (Self::default(), Some(format!("Browser session: {error:#}"))),
        }
    }
    pub fn save(&self, data: &Path) -> Result<()> {
        self.validate()?;
        std::fs::create_dir_all(data)?;
        // Preserve malformed/future originals before any replacement. A failed
        // backup fails the save and leaves the original in place.
        if Self::load(data).1.is_some() {
            let mut original = std::fs::File::open(data.join("browser-state.json"))?;
            let mut backup = tempfile::Builder::new()
                .prefix("browser-state-preserved-")
                .suffix(".json")
                .tempfile_in(data)?;
            std::io::copy(&mut original, &mut backup)?;
            backup.as_file().sync_all()?;
            backup.keep()?;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(data)?;
        temporary.write_all(&serde_json::to_vec(self)?)?;
        temporary.as_file().sync_all()?;
        temporary.persist(data.join("browser-state.json"))?;
        Ok(())
    }
    pub fn visit(&mut self, path: &Path) {
        if !self.remember_recent {
            return;
        }
        let location = Location::from_path(path);
        self.recent.retain(|l| *l != location);
        self.recent.insert(0, location);
        self.recent.truncate(20);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_and_future_sessions_are_preserved_before_atomic_recovery() {
        let data = tempfile::tempdir().unwrap();
        for original in [b"not json".as_slice(), b"{\"schema\":99}".as_slice()] {
            std::fs::write(data.path().join("browser-state.json"), original).unwrap();
            assert!(Session::load(data.path()).1.is_some());
            assert_eq!(
                std::fs::read(data.path().join("browser-state.json")).unwrap(),
                original
            );
            let session = Session {
                mode: PanelMode::Explorer,
                ..Default::default()
            };
            session.save(data.path()).unwrap();
            assert_eq!(Session::load(data.path()).0, session);
            assert!(std::fs::read_dir(data.path()).unwrap().flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("browser-state-preserved-")
                    && std::fs::read(e.path()).unwrap() == original
            }));
        }
    }
    #[test]
    fn recent_limit_and_privacy_option() {
        let mut session = Session::default();
        for n in 0..100 {
            session.visit(Path::new(&format!("/synthetic/{n}")));
        }
        assert_eq!(session.recent.len(), 20);
        session.remember_recent = false;
        let before = session.recent.clone();
        session.visit(Path::new("/new"));
        assert_eq!(session.recent, before);
    }
}
