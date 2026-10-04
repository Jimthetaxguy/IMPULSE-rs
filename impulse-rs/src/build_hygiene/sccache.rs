// Sccache — setup and status for shared compilation cache
//
// sccache caches compiled artifacts across projects, so rebuilding
// after a `cargo clean` or switching between projects is much faster.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::storage::Storage;

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
    let configured = config_path.as_deref().is_some_and(is_sccache_configured);
    let stats = if installed { get_sccache_stats() } else { None };

    SccacheStatus {
        installed,
        version,
        configured,
        config_path: config_path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        stats,
    }
}

/// Set up sccache as cargo's `rustc-wrapper` in the user's cargo config
/// (`$CARGO_HOME/config.toml`, by default `~/.cargo/config.toml`).
///
/// Adds `rustc-wrapper = "sccache"` under the existing `[build]` table, or
/// appends a `[build]` table, and preserves everything else. A wrapper that is
/// already set to something other than sccache is left alone and reported as
/// an error: a second `rustc-wrapper` key would make the file invalid TOML,
/// and cargo would then refuse every build that reads it.
pub fn sccache_setup(check_only: bool) -> Result<SccacheSetupResult> {
    if !is_sccache_installed() {
        bail!(
            "sccache is not installed. Install it with:\n  cargo install sccache\n  or: brew install sccache"
        );
    }

    let config_path = cargo_config_path().context(
        "Cannot locate the cargo config: neither CARGO_HOME nor a home directory is set",
    )?;
    let existing = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", config_path.display()))
        }
    };
    let display_path = config_path.to_string_lossy().to_string();

    let new_content = match plan_wrapper_edit(&existing)
        .with_context(|| format!("Not changing {}", config_path.display()))?
    {
        WrapperEdit::AlreadyConfigured => {
            return Ok(SccacheSetupResult {
                already_configured: true,
                config_path: display_path,
                action_taken: "Already configured".to_string(),
            });
        }
        WrapperEdit::Write(content) => content,
    };

    if check_only {
        return Ok(SccacheSetupResult {
            already_configured: false,
            config_path: display_path,
            action_taken: format!(
                "Not configured. Would add sccache wrapper to {}",
                config_path.display()
            ),
        });
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    Storage::atomic_write_path(&config_path, new_content.as_bytes())
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    Ok(SccacheSetupResult {
        already_configured: false,
        config_path: display_path,
        action_taken: format!(
            "Added sccache as rustc-wrapper in {}",
            config_path.display()
        ),
    })
}

/// What [`sccache_setup`] would do to a cargo config's text.
#[derive(Debug, PartialEq, Eq)]
enum WrapperEdit {
    AlreadyConfigured,
    Write(String),
}

/// How a cargo config sets `build.rustc-wrapper`.
#[derive(Debug, PartialEq, Eq)]
enum WrapperState {
    Unset,
    Sccache,
    /// Set to something else; the raw TOML value is kept for the message.
    Other(String),
    /// `build = { ... }` at the top level, which a `[build]` table would
    /// duplicate.
    InlineBuildTable,
}

/// Plans the edit without a TOML parser. Only the shapes cargo configs use in
/// practice are recognized: `[build]` headers (with optional whitespace and a
/// trailing comment), `rustc-wrapper` keys inside that table, and the dotted
/// `build.rustc-wrapper` or an inline `build = { ... }` at the top level.
/// Comment lines never count.
fn plan_wrapper_edit(existing: &str) -> Result<WrapperEdit> {
    match wrapper_state(existing) {
        WrapperState::Sccache => return Ok(WrapperEdit::AlreadyConfigured),
        WrapperState::Other(value) => {
            bail!("build.rustc-wrapper is already set to {value}; edit it by hand to use sccache")
        }
        WrapperState::InlineBuildTable => {
            bail!("build is an inline table; add rustc-wrapper = \"sccache\" to it by hand")
        }
        WrapperState::Unset => {}
    }

    let mut out = String::with_capacity(existing.len() + CARGO_CONFIG_ENTRY.len());
    let mut inserted = false;
    for line in existing.split_inclusive('\n') {
        out.push_str(line);
        if !inserted && table_header(line) == Some("build") {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("rustc-wrapper = \"sccache\"\n");
            inserted = true;
        }
    }
    if !inserted {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(CARGO_CONFIG_ENTRY);
    }
    Ok(WrapperEdit::Write(out))
}

fn wrapper_state(config: &str) -> WrapperState {
    // The table the current line belongs to; "" is the top level. An
    // array-of-tables header (`[[...]]`) never names `build`.
    let mut table = "";
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            table = table_header(line).unwrap_or("[[array]]");
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = normalize_key(key);
        let value = value.trim();
        let wrapper_value = match (table, key.as_str()) {
            ("build", "rustc-wrapper") | ("", "build.rustc-wrapper") => Some(value),
            ("", "build") => {
                return if value.contains("sccache") {
                    WrapperState::Sccache
                } else {
                    WrapperState::InlineBuildTable
                };
            }
            _ => None,
        };
        if let Some(value) = wrapper_value {
            return if value.contains("sccache") {
                WrapperState::Sccache
            } else {
                WrapperState::Other(value.to_string())
            };
        }
    }
    WrapperState::Unset
}

/// The name of a `[name]` table header line, ignoring whitespace and a
/// trailing comment. Array-of-tables headers (`[[name]]`) return `None`.
fn table_header(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix('[')?;
    if rest.starts_with('[') {
        return None;
    }
    let (name, after) = rest.split_once(']')?;
    let after = after.trim();
    (after.is_empty() || after.starts_with('#')).then(|| name.trim())
}

/// A bare or dotted key with whitespace and quotes removed, so
/// `"build" . 'rustc-wrapper'` compares equal to `build.rustc-wrapper`.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
        .collect()
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

/// `$CARGO_HOME/config.toml`, falling back to `~/.cargo/config.toml` as
/// cargo itself does. `None` when neither location can be determined.
fn cargo_config_path() -> Option<PathBuf> {
    cargo_config_path_from(std::env::var_os("CARGO_HOME"), dirs::home_dir())
}

fn cargo_config_path_from(
    cargo_home: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let cargo_home = match cargo_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home?.join(".cargo"),
    };
    Some(cargo_home.join("config.toml"))
}

fn is_sccache_configured(config_path: &Path) -> bool {
    std::fs::read_to_string(config_path)
        .is_ok_and(|content| wrapper_state(&content) == WrapperState::Sccache)
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
    fn test_cargo_config_path_prefers_cargo_home() {
        let path =
            cargo_config_path_from(Some("/opt/cargo".into()), Some(PathBuf::from("/home/me")));
        assert_eq!(path, Some(PathBuf::from("/opt/cargo/config.toml")));
    }

    #[test]
    fn test_cargo_config_path_falls_back_to_home_dot_cargo() {
        let home = Some(PathBuf::from("/home/me"));
        let expected = Some(PathBuf::from("/home/me/.cargo/config.toml"));
        assert_eq!(cargo_config_path_from(None, home.clone()), expected);
        assert_eq!(cargo_config_path_from(Some("".into()), home), expected);
    }

    #[test]
    fn test_cargo_config_path_without_any_home_is_none() {
        assert_eq!(cargo_config_path_from(None, None), None);
    }

    #[test]
    fn test_plan_appends_a_build_table_to_an_empty_config() {
        assert_eq!(
            plan_wrapper_edit("").unwrap(),
            WrapperEdit::Write(CARGO_CONFIG_ENTRY.to_string())
        );
    }

    #[test]
    fn test_plan_appends_after_content_without_a_trailing_newline() {
        let WrapperEdit::Write(text) = plan_wrapper_edit("[net]\nretry = 3").unwrap() else {
            panic!("expected an edit");
        };
        assert_eq!(
            text,
            "[net]\nretry = 3\n[build]\nrustc-wrapper = \"sccache\"\n"
        );
    }

    #[test]
    fn test_plan_inserts_under_an_existing_build_table_once() {
        let existing = "[build] # shared settings\njobs = 8\n\n[term]\nverbose = true\n";
        let WrapperEdit::Write(text) = plan_wrapper_edit(existing).unwrap() else {
            panic!("expected an edit");
        };
        assert_eq!(
            text,
            "[build] # shared settings\nrustc-wrapper = \"sccache\"\njobs = 8\n\n[term]\nverbose = true\n"
        );
        assert_eq!(text.matches("rustc-wrapper").count(), 1);
    }

    /// The old string replace put a second `rustc-wrapper` key into
    /// `[build]`, which cargo rejects as duplicate-key TOML.
    #[test]
    fn test_plan_refuses_to_replace_a_different_wrapper() {
        let existing = "[build]\nrustc-wrapper = \"/usr/local/bin/other-cache\"\n";
        let error = plan_wrapper_edit(existing).unwrap_err().to_string();
        assert!(error.contains("other-cache"), "{error}");
    }

    #[test]
    fn test_plan_refuses_a_dotted_top_level_wrapper() {
        let existing = "build.rustc-wrapper = \"other\"\n[net]\nretry = 3\n";
        assert!(plan_wrapper_edit(existing).is_err());
    }

    #[test]
    fn test_plan_refuses_an_inline_build_table() {
        assert!(plan_wrapper_edit("build = { jobs = 8 }\n").is_err());
    }

    /// The old string replace also matched `[build]` inside a comment.
    #[test]
    fn test_plan_ignores_build_in_comments() {
        let existing = "# see [build] docs\n[net]\nretry = 3\n";
        let WrapperEdit::Write(text) = plan_wrapper_edit(existing).unwrap() else {
            panic!("expected an edit");
        };
        assert_eq!(
            text,
            "# see [build] docs\n[net]\nretry = 3\n[build]\nrustc-wrapper = \"sccache\"\n"
        );
    }

    #[test]
    fn test_plan_recognizes_an_existing_sccache_wrapper() {
        for existing in [
            "[build]\nrustc-wrapper = \"sccache\"\n",
            "[ build ]\n\"rustc-wrapper\" = \"/opt/homebrew/bin/sccache\"\n",
            "build.rustc-wrapper = \"sccache\"\n",
        ] {
            assert_eq!(
                plan_wrapper_edit(existing).unwrap(),
                WrapperEdit::AlreadyConfigured,
                "{existing}"
            );
        }
    }

    #[test]
    fn test_wrapper_in_another_table_does_not_count() {
        let existing = "[target.x86_64-unknown-linux-gnu]\nrustc-wrapper = \"sccache\"\n";
        assert_eq!(wrapper_state(existing), WrapperState::Unset);
    }

    #[test]
    fn test_sccache_status_structure() {
        let status = sccache_status();
        // Just verify the structure is valid
        assert!(!status.config_path.is_empty());
    }

    #[test]
    fn test_is_sccache_configured_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(!is_sccache_configured(&dir.path().join("config.toml")));
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
    fn test_is_sccache_configured_ignores_a_comment_mentioning_sccache() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "# sccache disabled for now\n[build]\njobs = 4\n",
        )
        .unwrap();
        assert!(!is_sccache_configured(tmp.path()));
    }

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("Cache hits: 42"), Some(42));
        assert_eq!(extract_number("No numbers here"), None);
    }
}
