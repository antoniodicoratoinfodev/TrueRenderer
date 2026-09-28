//! A closed native window is not evidence that its development smoke passed.
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub fn begin(report: &Path) -> Result<()> {
    std::fs::write(
        report,
        br#"{"passed":false,"reason":"Development smoke started; incomplete"}"#,
    )
    .context("Initialize development smoke report")
}

pub fn finish(report: &Path) -> Result<()> {
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report).context("Read development smoke report")?)
            .context("Parse development smoke report")?;
    ensure!(
        result["passed"] == true,
        "Development smoke failed or incomplete: {} ({})",
        result["reason"].as_str().unwrap_or("see report"),
        report.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_close_cannot_reuse_a_previous_success() {
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("develop-ui.json");
        std::fs::write(&report, br#"{"passed":true}"#).unwrap();
        begin(&report).unwrap();
        assert!(
            finish(&report)
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );
        std::fs::write(&report, br#"{"passed":true,"saved_generation":1}"#).unwrap();
        finish(&report).unwrap();
    }

    #[test]
    fn failed_missing_and_invalid_reports_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("develop-ui.json");
        assert!(finish(&report).is_err());
        for bytes in [
            b"truncated".as_slice(),
            br#"{}"#,
            br#"{"passed":"true"}"#,
            br#"{"passed":false,"reason":"WB failed"}"#,
        ] {
            std::fs::write(&report, bytes).unwrap();
            assert!(finish(&report).is_err());
            assert_eq!(std::fs::read(&report).unwrap(), bytes);
        }
        assert!(
            finish(&report)
                .unwrap_err()
                .to_string()
                .contains("WB failed")
        );
    }
}
