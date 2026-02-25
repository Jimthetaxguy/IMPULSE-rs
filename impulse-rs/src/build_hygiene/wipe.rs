// Wipe — aggressive target/ directory removal
//
// Prefers cargo-wipe when installed. Falls back to native remove_dir_all
// when cargo-wipe is not available — no external dependencies required.

use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::build_hygiene::{is_cargo_tool_installed, CleanResult};

/// Options for running cargo-wipe
#[derive(Debug, Clone)]
pub struct WipeOptions {
    /// Only report what would be wiped, don't delete
    pub dry_run: bool,
    /// Paths to scan for target/ directories
    pub paths: Vec<PathBuf>,
    /// Also wipe node_modules (cargo-wipe supports this)
    pub include_node_modules: bool,
}

impl Default for WipeOptions {
    fn default() -> Self {
        Self {
            dry_run: true,
            paths: vec![],
            include_node_modules: false,
        }
    }
}

/// Run wipe with the given options.
///
/// Prefers cargo-wipe when installed. Falls back to native remove_dir_all
/// when cargo-wipe is not available.
pub fn run_wipe(opts: &WipeOptions) -> Result<CleanResult> {
    if !is_cargo_tool_installed("wipe") {
        tracing::info!("cargo-wipe not installed — using native filesystem wipe");
        return crate::build_hygiene::native::native_wipe(&opts.paths, opts.dry_run);
    }

    let mut total_freed: u64 = 0;
    let mut total_projects: u32 = 0;
    let mut errors: Vec<String> = Vec::new();

    for path in &opts.paths {
        if !path.exists() {
            errors.push(format!("Path does not exist: {}", path.display()));
            continue;
        }

        let mut cmd = Command::new("cargo");
        cmd.arg("wipe");

        // cargo-wipe uses "rust" to target Rust target/ dirs
        cmd.arg("rust");

        if opts.dry_run {
            // cargo-wipe is dry-run by default, but we pass --wipe to actually delete
            // So NOT passing --wipe means dry-run
        } else {
            cmd.arg("--wipe");
        }

        cmd.current_dir(path);

        tracing::debug!("Running: cargo wipe rust in {}", path.display());

        let output = cmd.output().map_err(|e| {
            anyhow::anyhow!("Failed to run cargo wipe in {}: {}", path.display(), e)
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            errors.push(format!(
                "cargo wipe failed in {}: {}",
                path.display(),
                stderr.trim()
            ));
            continue;
        }

        let freed = parse_wipe_output(&stdout);
        total_freed += freed;
        total_projects += 1;
    }

    // Also wipe node_modules if requested
    if opts.include_node_modules && !opts.dry_run {
        for path in &opts.paths {
            let mut cmd = Command::new("cargo");
            cmd.args(["wipe", "node_modules", "--wipe"]);
            cmd.current_dir(path);
            let _ = cmd.output(); // Best-effort
        }
    }

    let summary = if opts.dry_run {
        format!(
            "[DRY RUN] cargo-wipe would free ~{} across {} paths",
            crate::build_hygiene::format_bytes(total_freed),
            total_projects
        )
    } else {
        format!(
            "cargo-wipe freed {} across {} paths",
            crate::build_hygiene::format_bytes(total_freed),
            total_projects
        )
    };

    Ok(CleanResult {
        bytes_freed: total_freed,
        files_removed: 0,
        projects_cleaned: total_projects,
        errors,
        was_dry_run: opts.dry_run,
        summary,
    })
}

/// Parse cargo-wipe output for total freed bytes
fn parse_wipe_output(output: &str) -> u64 {
    // cargo-wipe outputs something like:
    // "Total: 5.23 GiB can be freed" (dry run)
    // or individual lines with sizes
    let mut total: u64 = 0;

    for line in output.lines() {
        let lower = line.to_lowercase();
        if lower.contains("total") || lower.contains("freed") || lower.contains("can be") {
            if let Some(bytes) = parse_wipe_size(line) {
                total = bytes; // Take the total line, not individual
            }
        }
    }

    // If no total line found, sum individual lines
    if total == 0 {
        for line in output.lines() {
            if let Some(bytes) = parse_wipe_size(line) {
                total += bytes;
            }
        }
    }

    total
}

fn parse_wipe_size(line: &str) -> Option<u64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    for window in parts.windows(2) {
        if let Ok(num) = window[0].parse::<f64>() {
            let multiplier = match window[1].to_uppercase().as_str() {
                "GIB" | "GB" => Some(1_073_741_824u64),
                "MIB" | "MB" => Some(1_048_576u64),
                "KIB" | "KB" => Some(1024u64),
                "B" => Some(1u64),
                _ => None,
            };
            if let Some(m) = multiplier {
                return Some((num * m as f64) as u64);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wipe_output_total() {
        let output = "Found 3 directories\nTotal: 2.5 GiB can be freed\n";
        let freed = parse_wipe_output(output);
        assert_eq!(freed, (2.5 * 1_073_741_824.0) as u64);
    }

    #[test]
    fn test_parse_wipe_output_empty() {
        assert_eq!(parse_wipe_output(""), 0);
    }

    #[test]
    fn test_wipe_options_default() {
        let opts = WipeOptions::default();
        assert!(opts.dry_run);
        assert!(!opts.include_node_modules);
    }
}
