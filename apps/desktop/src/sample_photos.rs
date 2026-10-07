//! README photographs, kept separate from the controlled decoder corpus.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn source(root: &Path, executable: &Path) -> Option<PathBuf> {
    let local = root.join("sample-photos");
    if local.join("manifest.json").is_file() {
        return Some(local);
    }
    executable
        .parent()?
        .parent()
        .map(|p| p.join("Resources/SamplePhotos"))
        .filter(|p| p.join("manifest.json").is_file())
}

pub fn destination(root: &Path, source: &Path) -> PathBuf {
    if source == root.join("sample-photos") {
        source.into()
    } else {
        root.join("var/sample-photos")
    }
}

/// Never create adjacent caches inside the signed application resources.
/// Existing copies are verified and never overwritten, including user changes.
pub fn prepare(root: &Path, source: &Path) -> Result<PathBuf> {
    use std::io::Write;
    let target = destination(root, source);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("manifest.json"))?)?;
    let photos = manifest["photographs"]
        .as_array()
        .context("Missing photograph manifest")?;
    std::fs::create_dir_all(&target)?;
    for photo in photos {
        let name = photo["filename"]
            .as_str()
            .context("Missing photograph name")?;
        ensure!(
            Path::new(name).components().count() == 1 && !name.starts_with('.'),
            "Invalid photograph name"
        );
        let existing = target.join(name);
        let bytes = std::fs::read(if existing.exists() {
            existing.clone()
        } else {
            source.join(name)
        })?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == photo["sha256"].as_str().unwrap_or(""),
            "Photograph checksum differs: {name}"
        );
        if !existing.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(existing)?
                .write_all(&bytes)?;
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_samples_are_copied_outside_signed_resources_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("App.app/Contents/Resources/SamplePhotos");
        std::fs::create_dir_all(&source).unwrap();
        let bytes = b"synthetic sample";
        std::fs::write(source.join("test.jpg"), bytes).unwrap();
        std::fs::write(source.join("manifest.json"), serde_json::to_vec(&serde_json::json!({"photographs":[{"filename":"test.jpg","sha256":format!("{:x}",Sha256::digest(bytes))}]})).unwrap()).unwrap();
        let root = temp.path().join("data");
        let path = prepare(&root, &source).unwrap();
        assert_eq!(path, root.join("var/sample-photos"));
        assert_eq!(std::fs::read(path.join("test.jpg")).unwrap(), bytes);
        assert_eq!(prepare(&root, &source).unwrap(), path);
        std::fs::write(path.join("test.jpg"), b"user change").unwrap();
        assert!(prepare(&root, &source).is_err());
        assert_eq!(
            std::fs::read(path.join("test.jpg")).unwrap(),
            b"user change"
        );
        assert_eq!(std::fs::read(source.join("test.jpg")).unwrap(), bytes);
    }
}
