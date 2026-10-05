//! Rust build artifact management and cleanup.
//!
//! Provides discovery, measurement, and cleaning of Rust build artifacts
//! across projects. Wraps cargo-sweep, cargo-wipe, cargo-clean-all, and sccache.

pub mod clean_all;
pub mod discovery;
pub mod measurement;
pub mod native;
pub mod sccache;
pub mod sweep;
pub mod wipe;

#[cfg(test)]
mod tests;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// Re-export key types
pub use clean_all::clean_all_projects;
pub use discovery::{discover_rust_projects, RustProject};
pub use measurement::{measure_project, measure_total_target_size, DiskUsageReport};
pub use native::{native_sweep, native_sweep_paths, native_wipe};
pub use sccache::{sccache_setup, sccache_status, SccacheStatus};
pub use sweep::{run_sweep, SweepOptions};
pub use wipe::{run_wipe, WipeOptions};

/// Configuration for build hygiene auto-sweep rules.
/// Stored in `.impulse/config.json` and surfaced via the Config struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildHygieneConfig {
    /// Whether build hygiene features are enabled
    pub enabled: bool,
    /// Directories to scan for Rust projects (supports ~ expansion)
    pub scan_paths: Vec<String>,
    /// Auto-sweep when total target/ size exceeds this (GB)
    pub size_threshold_gb: f64,
    /// Sweep artifacts older than this many days
    pub age_threshold_days: u32,
    /// Run sweep when an impulse session ends
    pub sweep_on_session_end: bool,
    /// Run sweep when a toolchain version change is detected
    pub sweep_on_toolchain_update: bool,
    /// Default to dry-run for destructive operations
    pub dry_run_default: bool,
}

impl Default for BuildHygieneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scan_paths: vec!["~/projects".to_string(), "~/Desktop".to_string()],
            size_threshold_gb: 10.0,
            age_threshold_days: 30,
            sweep_on_session_end: false,
            sweep_on_toolchain_update: true,
            dry_run_default: true,
        }
    }
}

impl BuildHygieneConfig {
    /// Expand ~ in scan paths to the user's home directory
    pub fn expanded_scan_paths(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        self.scan_paths
            .iter()
            .map(|p| {
                if p == "~" {
                    home.clone()
                } else if let Some(rest) = p.strip_prefix("~/") {
                    home.join(rest)
                } else {
                    PathBuf::from(p)
                }
            })
            .filter(|p| p.exists())
            .collect()
    }
}

/// Result of a sweep/clean operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanResult {
    /// Bytes freed by this operation
    pub bytes_freed: u64,
    /// Number of files removed
    pub files_removed: u32,
    /// Number of projects cleaned
    pub projects_cleaned: u32,
    /// Errors encountered (non-fatal)
    pub errors: Vec<String>,
    /// Whether this was a dry-run
    pub was_dry_run: bool,
    /// Human-readable summary
    pub summary: String,
}

impl CleanResult {
    pub fn dry_run_summary(projects: u32, estimated_bytes: u64) -> Self {
        Self {
            bytes_freed: estimated_bytes,
            files_removed: 0,
            projects_cleaned: projects,
            errors: vec![],
            was_dry_run: true,
            summary: format!(
                "[DRY RUN] Would clean {} projects, freeing ~{}",
                projects,
                format_bytes(estimated_bytes)
            ),
        }
    }
}

/// Format bytes into human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// The signature that starts a `CACHEDIR.TAG` file, which cargo writes into
/// each build directory it creates.
const CACHEDIR_TAG_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Whether `target` carries cargo's `CACHEDIR.TAG`. Cargo itself refuses to
/// clean a directory without one, to avoid deleting unrelated files. Only a
/// regular file is read, so a FIFO in its place cannot block the caller.
fn is_cargo_build_dir(target: &Path) -> bool {
    let tag = target.join("CACHEDIR.TAG");
    if !std::fs::metadata(&tag).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut start = [0u8; CACHEDIR_TAG_SIGNATURE.len()];
    std::fs::File::open(&tag)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut start))
        .is_ok_and(|()| start.as_slice() == CACHEDIR_TAG_SIGNATURE)
}

/// Projects under `paths` that a destructive operation may act on, and a
/// note for each one skipped. A project is left alone when its `target` is a
/// symbolic link, since acting through the link changed files outside the
/// scanned roots, possibly a directory other projects share; or when its
/// `target` lacks cargo's `CACHEDIR.TAG`, as cargo would refuse it too.
pub fn projects_to_clean(paths: &[PathBuf]) -> (Vec<RustProject>, Vec<String>) {
    let mut skipped = Vec::new();
    let projects = discover_rust_projects(paths)
        .into_iter()
        .filter(|project| {
            let target = project.path.join("target");
            let reason = if std::fs::symlink_metadata(&target)
                .is_ok_and(|meta| meta.file_type().is_symlink())
            {
                Some("target is a symbolic link")
            } else if !is_cargo_build_dir(&target) {
                Some("target has no CACHEDIR.TAG, so it is not treated as a cargo build directory")
            } else {
                None
            };
            if let Some(reason) = reason {
                skipped.push(format!("{}: {reason}; skipped", project.path.display()));
            }
            reason.is_none()
        })
        .collect();
    (projects, skipped)
}

/// Check if a cargo tool is installed
pub fn is_cargo_tool_installed(tool_name: &str) -> bool {
    std::process::Command::new("cargo")
        .args([tool_name, "--help"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get the install command for a cargo tool
pub fn cargo_tool_install_cmd(tool_name: &str) -> String {
    format!("cargo install {}", tool_name)
}

/// Evaluate auto-sweep rules and return whether a sweep should run.
/// Returns (should_sweep, reason)
pub fn evaluate_auto_sweep_rules(
    config: &BuildHygieneConfig,
    total_size_bytes: u64,
    trigger: AutoSweepTrigger,
) -> (bool, Option<String>) {
    if !config.enabled {
        return (false, None);
    }

    match trigger {
        AutoSweepTrigger::SessionEnd => {
            if config.sweep_on_session_end {
                (
                    true,
                    Some("Session ended — running configured sweep".to_string()),
                )
            } else {
                (false, None)
            }
        }
        AutoSweepTrigger::ToolchainUpdate => {
            if config.sweep_on_toolchain_update {
                (
                    true,
                    Some("Toolchain update detected — sweeping stale artifacts".to_string()),
                )
            } else {
                (false, None)
            }
        }
        AutoSweepTrigger::SizeThreshold => {
            let total_gb = total_size_bytes as f64 / 1_073_741_824.0;
            if total_gb > config.size_threshold_gb {
                (
                    true,
                    Some(format!(
                        "Build artifacts at {:.1} GB (threshold: {:.0} GB)",
                        total_gb, config.size_threshold_gb
                    )),
                )
            } else {
                (false, None)
            }
        }
        AutoSweepTrigger::Manual => (true, Some("Manual sweep requested".to_string())),
    }
}

/// Triggers for auto-sweep evaluation
#[derive(Debug, Clone, Copy)]
pub enum AutoSweepTrigger {
    SessionEnd,
    ToolchainUpdate,
    SizeThreshold,
    Manual,
}

/// Run the full auto-sweep pipeline:
/// 1. Discover projects
/// 2. Measure total size
/// 3. Evaluate rules
/// 4. Sweep if triggered
pub async fn run_auto_sweep(
    config: &BuildHygieneConfig,
    trigger: AutoSweepTrigger,
) -> Result<Option<CleanResult>> {
    let paths = config.expanded_scan_paths();
    if paths.is_empty() {
        return Ok(None);
    }

    let projects = discover_rust_projects(&paths);
    let total_size: u64 = projects.iter().map(|p| p.target_size_bytes).sum();

    let (should_sweep, reason) = evaluate_auto_sweep_rules(config, total_size, trigger);

    if !should_sweep {
        return Ok(None);
    }

    tracing::info!(
        "Auto-sweep triggered: {}",
        reason.as_deref().unwrap_or("unknown")
    );

    let opts = SweepOptions {
        days: config.age_threshold_days,
        dry_run: config.dry_run_default,
        paths: paths.clone(),
        recursive: true,
        verbose: false,
    };

    let result = run_sweep(&opts)?;
    Ok(Some(result))
}
