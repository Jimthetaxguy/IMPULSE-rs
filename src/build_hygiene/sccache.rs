// Sccache — setup and status for shared compilation cache
//
// sccache caches compiled artifacts across projects, so rebuilding
// after a `cargo clean` or switching between projects is much faster.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

const CARGO_CONFIG_ENTRY: &str = r#"[build]
rustc-wrapper = "sccache"
"#;

/// Status of sccache installation and configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SccacheStatus {
    /// Whether the sccache binary is installed
    pub installed: bool,
    /// sccache version string
    pub version: Option<String>,
    /// Whether ~/.cargo/config.toml is configured to use sccache
    pub configured: bool,
    /// Path to the cargo config file
    pub config_path: String,
    /// sccache cache stats (if running)
    pub stats: Option<SccacheStats>,
}

/// Basic sccache cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SccacheStats {
    pub cache_hits: Option<u64>,
    pub cache_misses: Option<u64>,
    pub cache_size: Option<String>,
}

/// Check the current status of sccache
pub fn sccache_status() -> SccacheStatus {
    let installed = is_sccache_installed();
    let version = if installed {
        get_sccache_version()
    } else {
        None
    };
    let config_path = cargo_config_path();
    let configured = is_sccache_configured(&config_path);
    let stats = if installed { get_sccache_stats() } else { None };

    SccacheStatus {
        installed,
        version,
        configured,
        config_path: config_path.to_string_lossy().to_string(),
        stats,
    }
}

/// Set up sccache in ~/.cargo/config.toml
///
/// This adds `[build]\nrustc-wrapper = "sccache"` to the cargo config.
/// Preserves existing content and doesn't duplicate the entry.
pub fn sccache_setup(check_only: bool) -> Result<SccacheSetupResult> {
    if !is_sccache_installed() {
        bail!(
            "sccache is not installed. Install it with:\n  cargo install sccache\n  or: brew install sccache"
        );
    }

    let config_path = cargo_config_path();

    if is_sccache_configured(&config_path) {
        return Ok(SccacheSetupResult {
            already_configured: true,
            config_path: config_path.to_string_lossy().to_string(),
            action_taken: "Already configured".to_string(),
        });
    }

    if check_only {
        return Ok(SccacheSetupResult {
            already_configured: false,
            config_path: config_path.to_string_lossy().to_string(),
            action_taken: format!(
                "Not configured. Would add sccache wrapper to {}",
                config_path.display()
            ),
        });
    }

    // Read existing config or start fresh
    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read cargo config {}", config_path.display()))?
    } else {
        String::new()
    };

    let new_content = build_sccache_config(&existing);
    write_config_atomically(&config_path, &new_content)?;

    Ok(SccacheSetupResult {
        already_configured: false,
        config_path: config_path.to_string_lossy().to_string(),
        action_taken: format!(
            "Added sccache as rustc-wrapper in {}",
            config_path.display()
        ),
    })
}

/// Result of sccache setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SccacheSetupResult {
    pub already_configured: bool,
    pub config_path: String,
    pub action_taken: String,
}

fn is_sccache_installed() -> bool {
    Command::new("sccache")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn get_sccache_version() -> Option<String> {
    Command::new("sccache")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

fn cargo_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".cargo")
        .join("config.toml")
}

fn build_sccache_config(existing: &str) -> String {
    if existing.contains("[build]") {
        return existing.replace("[build]", "[build]\nrustc-wrapper = \"sccache\"");
    }

    let sep = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{existing}{sep}{CARGO_CONFIG_ENTRY}")
}

fn write_config_atomically(config_path: &Path, new_content: &str) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create cargo config directory {}",
                parent.display()
            )
        })?;
    }

    crate::storage::Storage::atomic_write_path(config_path, new_content.as_bytes()).with_context(
        || {
            format!(
                "Failed to atomically write cargo config {}",
                config_path.display()
            )
        },
    )
}

fn is_sccache_configured(config_path: &Path) -> bool {
    if !config_path.exists() {
        return false;
    }
    match std::fs::read_to_string(config_path) {
        Ok(content) => content.contains("sccache"),
        Err(_) => false,
    }
}

fn get_sccache_stats() -> Option<SccacheStats> {
    let output = Command::new("sccache").arg("--show-stats").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut cache_hits = None;
    let mut cache_misses = None;
    let mut cache_size = None;

    for line in stdout.lines() {
        let lower = line.to_lowercase();
        if lower.contains("cache hit") {
            cache_hits = extract_number(line);
        } else if lower.contains("cache miss") {
            cache_misses = extract_number(line);
        } else if lower.contains("cache size") || lower.contains("cache_size") {
            cache_size = line.split_whitespace().last().map(|s| s.to_string());
        }
    }

    Some(SccacheStats {
        cache_hits,
        cache_misses,
        cache_size,
    })
}

fn extract_number(line: &str) -> Option<u64> {
    line.split_whitespace()
        .find_map(|word| word.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_config_path() {
        let path = cargo_config_path();
        assert!(path.to_string_lossy().contains(".cargo"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }

    #[test]
    fn test_sccache_status_structure() {
        let status = sccache_status();
        // Just verify the structure is valid
        assert!(!status.config_path.is_empty());
    }

    #[test]
    fn test_is_sccache_configured_missing_file() {
        let path = PathBuf::from("/tmp/nonexistent-cargo-config.toml");
        assert!(!is_sccache_configured(&path));
    }

    #[test]
    fn test_is_sccache_configured_without_entry() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "[profile.release]\nopt-level = 3\n").unwrap();
        assert!(!is_sccache_configured(tmp.path()));
    }

    #[test]
    fn test_is_sccache_configured_with_entry() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "[build]\nrustc-wrapper = \"sccache\"\n").unwrap();
        assert!(is_sccache_configured(tmp.path()));
    }

    #[test]
    fn test_build_sccache_config_appends_build_section_when_missing() {
        let existing = "[profile.release]\nopt-level = 3\n";
        let new_content = build_sccache_config(existing);

        assert!(new_content.starts_with(existing));
        assert!(new_content.ends_with(CARGO_CONFIG_ENTRY));
    }

    #[test]
    fn test_build_sccache_config_inserts_wrapper_into_existing_build_section() {
        let existing = "[build]\nincremental = true\n";
        let new_content = build_sccache_config(existing);

        assert_eq!(
            new_content,
            "[build]\nrustc-wrapper = \"sccache\"\nincremental = true\n"
        );
    }

    #[test]
    fn test_write_config_atomically_creates_parent_dirs_and_overwrites() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nested/.cargo/config.toml");

        write_config_atomically(&config_path, "first = true\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "first = true\n"
        );

        write_config_atomically(&config_path, "second = true\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "second = true\n"
        );
    }

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("Cache hits: 42"), Some(42));
        assert_eq!(extract_number("No numbers here"), None);
    }
}
