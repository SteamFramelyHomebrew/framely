use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{path::Path, process::Command};

pub fn collect(state: &Path) -> Result<Value> {
    let output = Command::new("python3")
        .args([
            "-c",
            include_str!("../tools/export-diagnostics.py"),
            "--state",
        ])
        .arg(state)
        .output()
        .context("Could not collect diagnostics")?;
    ensure!(
        output.status.success(),
        "Diagnostic export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).context("Invalid diagnostic archive")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::io::{Cursor, Read};

    #[test]
    fn exports_without_a_running_daemon_and_with_corrupt_state() {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(state.path().join("logs")).unwrap();
        std::fs::write(state.path().join("state.json"), "corrupt").unwrap();
        std::fs::write(
            state.path().join("logs/demo.log"),
            "useful error\npassword=not-public",
        )
        .unwrap();
        let result = collect(state.path()).unwrap();
        let bytes = STANDARD.decode(result["data"].as_str().unwrap()).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut log = String::new();
        archive
            .by_name("logs/demo.log")
            .unwrap()
            .read_to_string(&mut log)
            .unwrap();
        assert!(log.contains("useful error"));
        assert!(!log.contains("not-public"));
        assert!(archive.by_name("report.json").is_ok());
        assert!(archive.by_name("renderer/gpu.json").is_ok());
        assert_eq!(
            std::fs::read_to_string(state.path().join("state.json")).unwrap(),
            "corrupt"
        );
    }
}
