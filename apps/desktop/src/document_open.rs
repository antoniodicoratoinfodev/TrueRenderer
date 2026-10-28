//! Single-instance handoff. Requests are owned queue files, never photo data.
use anyhow::{Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub fn enqueue(data: &Path, path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("truerenderer")),
        "Solo documenti nativi nella coda di apertura"
    );
    let queue = data.join("document-open-requests");
    std::fs::create_dir_all(&queue)?;
    let mut file = tempfile::Builder::new()
        .prefix("tr-open-")
        .suffix(".pending")
        .tempfile_in(&queue)?;
    file.write_all(&serde_json::to_vec(&path)?)?;
    file.as_file().sync_all()?;
    let ready = file.path().with_extension("json");
    file.persist_noclobber(ready).map_err(|e| e.error)?;
    Ok(())
}
pub fn next(data: &Path) -> Result<Option<PathBuf>> {
    let queue = data.join("document-open-requests");
    if !queue.is_dir() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(queue)?.take(64) {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file()
            || !entry.file_name().to_string_lossy().starts_with("tr-open-")
            || path.extension().is_none_or(|e| e != "json")
        {
            continue;
        }
        ensure!(
            entry.metadata()?.len() <= 65536,
            "Richiesta di apertura fuori quota"
        );
        let requested: PathBuf = serde_json::from_slice(&std::fs::read(&path)?)?;
        std::fs::remove_file(&path)?;
        ensure!(
            requested
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("truerenderer")),
            "Richiesta non nativa"
        );
        return Ok(Some(requested));
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    #[test]
    fn handoff_preserves_unicode_and_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("città prova.truerenderer");
        std::fs::write(&file, b"test").unwrap();
        super::enqueue(dir.path(), &file).unwrap();
        assert_eq!(
            super::next(dir.path()).unwrap(),
            Some(file.canonicalize().unwrap())
        );
        assert!(super::next(dir.path()).unwrap().is_none());
        assert_eq!(std::fs::read(&file).unwrap(), b"test");
    }
}
