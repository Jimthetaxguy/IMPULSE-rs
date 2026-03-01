//! Global Impulse configuration — stored at ~/.impulse/config.json.
//!
//! Tracks recent projects and application-level preferences.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT_PROJECTS: usize = 10;

/// Application-level configuration stored at `~/.impulse/config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub recent_projects: Vec<PathBuf>,
    /// The last-used project directory — restored on next launch.
    #[serde(default)]
    pub last_project: Option<PathBuf>,
    /// Settings key-value store (used by SettingsView).
    #[serde(default)]
    pub settings: HashMap<String, String>,
}

impl GlobalConfig {
    /// Add a project to the recent list (MRU order, deduplicating).
    pub fn add_recent_project(&mut self, path: PathBuf) {
        self.recent_projects.retain(|p| p != &path);
        self.recent_projects.insert(0, path);
        self.recent_projects.truncate(MAX_RECENT_PROJECTS);
    }

    /// Load from `<dir>/config.json`. Returns default if file doesn't exist.
    pub fn load(impulse_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = impulse_dir.join("config.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save to `<dir>/config.json` using atomic write (temp + rename).
    pub fn save(&self, impulse_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(impulse_dir)?;
        let path = impulse_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;

        // Atomic write: temp file + rename.
        let tmp_path = impulse_dir.join(format!(
            ".config.json.tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// The global impulse directory path.
    pub fn impulse_home() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".impulse")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = GlobalConfig::default();
        assert!(config.recent_projects.is_empty());
    }

    #[test]
    fn test_add_recent_project() {
        let mut config = GlobalConfig::default();
        config.add_recent_project("/tmp/project-a".into());
        config.add_recent_project("/tmp/project-b".into());
        assert_eq!(config.recent_projects.len(), 2);
        // Most recent first
        assert_eq!(config.recent_projects[0], PathBuf::from("/tmp/project-b"));
    }

    #[test]
    fn test_add_recent_project_deduplicates() {
        let mut config = GlobalConfig::default();
        config.add_recent_project("/tmp/project-a".into());
        config.add_recent_project("/tmp/project-b".into());
        config.add_recent_project("/tmp/project-a".into());
        assert_eq!(config.recent_projects.len(), 2);
        // Re-added project moves to front
        assert_eq!(config.recent_projects[0], PathBuf::from("/tmp/project-a"));
    }

    #[test]
    fn test_max_recent_projects() {
        let mut config = GlobalConfig::default();
        for i in 0..15 {
            config.add_recent_project(format!("/tmp/project-{}", i).into());
        }
        assert_eq!(config.recent_projects.len(), 10);
    }

    #[test]
    fn test_load_save_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut config = GlobalConfig::default();
        config.add_recent_project("/tmp/test-proj".into());
        config.save(dir.path()).unwrap();

        let loaded = GlobalConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.recent_projects.len(), 1);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let config = GlobalConfig::load(dir.path()).unwrap();
        assert!(config.recent_projects.is_empty());
    }

    #[test]
    fn test_last_project_persists() {
        let dir = TempDir::new().unwrap();
        let mut config = GlobalConfig::default();
        config.last_project = Some("/tmp/my-project".into());
        config.save(dir.path()).unwrap();

        let loaded = GlobalConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.last_project, Some(PathBuf::from("/tmp/my-project")));
    }

    #[test]
    fn test_last_project_defaults_to_none() {
        let config = GlobalConfig::default();
        assert!(config.last_project.is_none());
    }
}
