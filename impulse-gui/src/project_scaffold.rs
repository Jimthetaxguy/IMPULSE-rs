//! Auto-scaffold `.impulse/` directory for new project targets.

use std::path::Path;

use crate::error::GuiError;

const GENOME_TEMPLATE: &str = r#"{
  "decisions": [],
  "preferences": [],
  "constraints": [],
  "last_updated": null
}"#;

const CONFIG_TEMPLATE: &str = "{}";

/// Check if a target directory needs scaffolding.
pub fn needs_scaffold(target: &Path) -> bool {
    !target.join(".impulse").exists()
}

/// Create `.impulse/` with starter files in a target project directory.
pub fn scaffold_impulse_dir(target: &Path) -> Result<(), GuiError> {
    let impulse_dir = target.join(".impulse");
    std::fs::create_dir_all(&impulse_dir)?;

    let genome_path = impulse_dir.join("GENOME.md");
    if !genome_path.exists() {
        atomic_write(&genome_path, GENOME_TEMPLATE)?;
    }

    let config_path = impulse_dir.join("config.json");
    if !config_path.exists() {
        atomic_write(&config_path, CONFIG_TEMPLATE)?;
    }

    let history_path = impulse_dir.join("HISTORY.jsonl");
    if !history_path.exists() {
        atomic_write(&history_path, "")?;
    }

    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<(), GuiError> {
    impulse_ops::atomic_write_path(path, content.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scaffold_creates_impulse_dir() {
        let dir = TempDir::new().unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        assert!(dir.path().join(".impulse").exists());
        assert!(dir.path().join(".impulse/GENOME.md").exists());
        assert!(dir.path().join(".impulse/config.json").exists());
    }

    #[test]
    fn test_scaffold_idempotent() {
        let dir = TempDir::new().unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        assert!(dir.path().join(".impulse").exists());
    }

    #[test]
    fn test_needs_scaffold_true() {
        let dir = TempDir::new().unwrap();
        assert!(needs_scaffold(dir.path()));
    }

    #[test]
    fn test_needs_scaffold_false_after_scaffold() {
        let dir = TempDir::new().unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        assert!(!needs_scaffold(dir.path()));
    }
}
