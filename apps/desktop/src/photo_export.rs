use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Job {
    pub item: tr_core::Item,
    pub options: tr_core::export::Options,
    pub engine: tr_core::decoder::RawEngine,
    pub destination: PathBuf,
    pub cancel: Arc<AtomicBool>,
}
pub struct ScientificJob {
    pub item: tr_core::Item,
    pub x: u32,
    pub y: u32,
    pub generation: u64,
}
pub struct Completed {
    pub path: PathBuf,
    pub info: tr_core::export::Info,
}

/// A temporary file on the destination volume, then exclusive publication.
/// Never truncate/replace an existing file, including a symlink or an original.
pub fn publish(
    destination: &Path,
    name: &str,
    format: tr_core::export::Format,
    bytes: &[u8],
    cancelled: impl Fn() -> bool,
) -> Result<PathBuf> {
    ensure!(destination.is_dir(), "Destinazione non disponibile");
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let stem: String = stem
        .chars()
        .filter(|c| c.is_alphanumeric() || ['-', '_', ' '].contains(c))
        .take(100)
        .collect();
    let stem = if stem.is_empty() { "image" } else { &stem };
    let suffix = match format {
        tr_core::export::Format::DngRaw => "raw",
        tr_core::export::Format::DngLinear16 => "linear",
        _ => "export",
    };
    let mut temporary = tempfile::Builder::new()
        .prefix(".tr-export-")
        .tempfile_in(destination)?;
    for block in bytes.chunks(64 * 1024) {
        ensure!(!cancelled(), "Esportazione annullata");
        temporary.write_all(block)?;
    }
    temporary.as_file().sync_all()?;
    for collision in 0..10000 {
        ensure!(!cancelled(), "Esportazione annullata");
        let number = if collision == 0 {
            String::new()
        } else {
            format!("-{collision}")
        };
        let path = destination.join(format!("{stem}-{suffix}{number}.{}", format.extension()));
        match temporary.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temporary = error.file
            }
            Err(error) => return Err(error.error).context("Pubblicazione export"),
        }
    }
    anyhow::bail!("Troppi nomi esistenti: scegliere una destinazione diversa")
}
impl Job {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::export::Format;
    #[test]
    fn originals_and_collisions_are_never_overwritten() {
        let folder = tempfile::tempdir().unwrap();
        let first = publish(folder.path(), "test.nef", Format::Png16, b"first", || false).unwrap();
        let second = publish(folder.path(), "test.nef", Format::Png16, b"second", || {
            false
        })
        .unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(first).unwrap(), b"first");
        assert_eq!(std::fs::read(second).unwrap(), b"second");
        assert!(
            publish(
                folder.path(),
                "../../escape.nef",
                Format::Png16,
                b"no",
                || true
            )
            .is_err()
        );
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 2);
    }
}
