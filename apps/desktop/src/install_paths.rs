//! Installed Windows packages keep user data outside replaceable program files.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

pub fn installed_root(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    directory
        .join("truerenderer-installed.txt")
        .is_file()
        .then(|| directory.to_path_buf())
}

pub fn user_data(local_app_data: Option<PathBuf>) -> Result<PathBuf> {
    let base = local_app_data.context("LOCALAPPDATA non disponibile per i dati di TrueRenderer")?;
    ensure!(
        base.is_absolute(),
        "LOCALAPPDATA deve essere un percorso assoluto"
    );
    Ok(base.join("TrueRenderer").join("var"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_is_detected_beside_executable_not_working_directory() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("truerenderer.exe");
        assert!(installed_root(&executable).is_none());
        std::fs::write(temp.path().join("truerenderer-installed.txt"), b"1\n").unwrap();
        assert_eq!(installed_root(&executable).unwrap(), temp.path());
    }

    #[test]
    fn installed_data_has_a_stable_location_and_missing_profile_is_an_error() {
        let profile = tempfile::tempdir().unwrap();
        assert_eq!(
            user_data(Some(profile.path().into())).unwrap(),
            profile.path().join("TrueRenderer/var")
        );
        assert!(user_data(None).is_err());
        assert!(user_data(Some("relative".into())).is_err());
    }
}
