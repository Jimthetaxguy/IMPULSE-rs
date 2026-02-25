// Sweep — incremental artifact cleaning (stale files by mtime)
//
// Prefers cargo-sweep when installed. Falls back to native filesystem
// walk when cargo-sweep is not available — no external dependencies required.

use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::build_hygiene::{is_cargo_tool_installed, CleanResult};

/// Options for running cargo-sweep
#[derive(Debug, Clone)]
pub struct SweepOptions {
    /// Remove artifacts not accessed in this many days
    pub days: u32,
    /// Only report what would be cleaned, don't delete
    pub dry_run: bool,
    /// Paths to scan (project roots or parent directories)
    pub paths: Vec<PathBuf>,
    /// Recurse into subdirectories to find projects
    pub recursive: bool,
    /// Show verbose output
    pub verbose: bool,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            days: 30,
            dry_run: true,
            paths: vec![],
            recursive: true,
            verbose: false,
        }
    }
}

/// Run sweep with the given options.
///
/// Prefers cargo-sweep when installed for its cargo-aware file fingerprinting.
/// Falls back to native filesystem sweep (mtime-based) when cargo-sweep is missing.
pub fn run_sweep(opts: &SweepOptions) -> Result<CleanResult> {
    if !is_cargo_tool_installed("sweep") {
        tracing::info!("cargo-sweep not installed — using native filesystem sweep");
        return crate::build_hygiene::native::native_sweep_paths(
            &opts.paths,
            opts.days,
            opts.dry_run,
        );
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
        cmd.arg("sweep");

        // Set the time threshold
        cmd.args(["--time", &opts.days.to_string()]);

        if opts.recursive {
            cmd.arg("--recursive");
        }

        if opts.verbose {
            cmd.arg("--verbose");
        }

        if opts.dry_run {
            cmd.arg("--dry-run");
        }

        cmd.current_dir(path);

        tracing::debug!(
            "Running: cargo sweep --time {} in {}",
            opts.days,
            path.display()
        );

        let output = cmd.output().map_err(|e| {
            anyhow::anyhow!("Failed to run cargo sweep in {}: {}", path.display(), e)
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            errors.push(format!(
                "cargo sweep failed in {}: {}",
                path.display(),
                stderr.trim()
            ));
            continue;
        }

        // Parse output for freed bytes
        // cargo-sweep outputs lines like: "Cleaned 1.23 GiB from ..."
        let freed = parse_sweep_output(&stdout);
        total_freed += freed;
        total_projects += 1;

        if opts.verbose {
            tracing::info!(
                "Sweep in {}: freed {}",
                path.display(),
                crate::build_hygiene::format_bytes(freed)
            );
        }
    }

    let summary = if opts.dry_run {
        format!(
            "[DRY RUN] cargo-sweep would free ~{} across {} paths (artifacts older than {} days)",
            crate::build_hygiene::format_bytes(total_freed),
            total_projects,
            opts.days
        )
    } else {
        format!(
            "cargo-sweep freed {} across {} paths (artifacts older than {} days)",
            crate::build_hygiene::format_bytes(total_freed),
            total_projects,
            opts.days
        )
    };

    Ok(CleanResult {
        bytes_freed: total_freed,
        files_removed: 0, // cargo-sweep doesn't report file count
        projects_cleaned: total_projects,
        errors,
        was_dry_run: opts.dry_run,
        summary,
    })
}

/// Parse cargo-sweep output for freed bytes.
/// Looks for patterns like "1.23 GiB", "456 MiB", "789 KiB"
fn parse_sweep_output(output: &str) -> u64 {
    let mut total: u64 = 0;

    for line in output.lines() {
        if let Some(bytes) = parse_size_from_line(line) {
            total += bytes;
        }
    }

    total
}

fn parse_size_from_line(line: &str) -> Option<u64> {
    // Look for patterns: "N.NN GiB", "N.NN MiB", "N.NN KiB", "N B"
    let parts: Vec<&str> = line.split_whitespace().collect();
    for window in parts.windows(2) {
        if let Ok(num) = window[0].parse::<f64>() {
            let multiplier = match window[1] {
                "GiB" | "GB" => Some(1_073_741_824u64),
                "MiB" | "MB" => Some(1_048_576u64),
                "KiB" | "KB" => Some(1024u64),
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
    fn test_parse_sweep_output_gib() {
        let output = "Cleaned 1.5 GiB from /path/to/project\n";
        let freed = parse_sweep_output(output);
        assert_eq!(freed, (1.5 * 1_073_741_824.0) as u64);
    }

    #[test]
    fn test_parse_sweep_output_mib() {
        let output = "Cleaned 256 MiB from /path\n";
        let freed = parse_sweep_output(output);
        assert_eq!(freed, 256 * 1_048_576);
    }

    #[test]
    fn test_parse_sweep_output_empty() {
        let freed = parse_sweep_output("");
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_parse_sweep_output_no_match() {
        let freed = parse_sweep_output("No old build artifacts found.\n");
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_parse_size_from_line() {
        assert_eq!(
            parse_size_from_line("Freed 2.0 GiB"),
            Some(2 * 1_073_741_824)
        );
        assert_eq!(parse_size_from_line("Freed 512 MiB"), Some(512 * 1_048_576));
        assert_eq!(parse_size_from_line("No artifacts"), None);
    }

    #[test]
    fn test_sweep_options_default() {
        let opts = SweepOptions::default();
        assert_eq!(opts.days, 30);
        assert!(opts.dry_run);
        assert!(opts.recursive);
    }
}
