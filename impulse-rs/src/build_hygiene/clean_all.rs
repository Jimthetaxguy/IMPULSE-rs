// CleanAll — wrapper around cargo-clean-all for workspace-wide cleaning
//
// cargo-clean-all runs `cargo clean` across all projects in a directory tree.
// More aggressive than sweep — it removes ALL build artifacts, not just stale ones.

use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::build_hygiene::{format_bytes, projects_to_clean, CleanResult};

/// Run cargo-clean-all or equivalent across discovered projects.
///
/// If cargo-clean-all is not installed, falls back to removing each
/// discovered project's own `target/` directory, as `cargo clean` would.
pub fn clean_all_projects(paths: &[PathBuf], dry_run: bool) -> Result<CleanResult> {
    let has_clean_all = crate::build_hygiene::is_cargo_tool_installed("clean-all");

    if has_clean_all {
        return clean_all_with_tool(paths, dry_run);
    }

    // Fallback: discover projects and run cargo clean in each
    clean_all_manual(paths, dry_run)
}

/// Use cargo-clean-all tool
fn clean_all_with_tool(paths: &[PathBuf], dry_run: bool) -> Result<CleanResult> {
    let mut total_freed: u64 = 0;
    let mut total_projects: u32 = 0;
    let mut errors: Vec<String> = Vec::new();

    for path in paths {
        if !path.exists() {
            errors.push(format!("Path does not exist: {}", path.display()));
            continue;
        }

        let mut cmd = Command::new("cargo");
        cmd.arg("clean-all");

        if dry_run {
            cmd.arg("--dry-run");
        }

        cmd.current_dir(path);

        tracing::debug!("Running: cargo clean-all in {}", path.display());

        let output = cmd.output().map_err(|e| {
            anyhow::anyhow!("Failed to run cargo clean-all in {}: {}", path.display(), e)
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            errors.push(format!(
                "cargo clean-all failed in {}: {}",
                path.display(),
                stderr.trim()
            ));
            continue;
        }

        // Parse output for project count and sizes
        let (freed, projects) = parse_clean_all_output(&stdout);
        total_freed += freed;
        total_projects += projects;
    }

    let summary = if dry_run {
        format!(
            "[DRY RUN] cargo-clean-all would free ~{} across {} projects",
            format_bytes(total_freed),
            total_projects
        )
    } else {
        format!(
            "cargo-clean-all freed {} across {} projects",
            format_bytes(total_freed),
            total_projects
        )
    };

    Ok(CleanResult {
        bytes_freed: total_freed,
        files_removed: 0,
        projects_cleaned: total_projects,
        errors,
        was_dry_run: dry_run,
        summary,
    })
}

/// Fallback: remove each discovered project's own `target/`. This used to
/// run `cargo clean`, which asks cargo where the build output lives: with a
/// global `build.target-dir` or `build.build-dir` that is a directory every
/// project shares, and it was deleted once per project; a relative scan path
/// also made `--target-dir` point nowhere. `projects_to_clean` keeps only a
/// `target/` that is a real directory carrying cargo's `CACHEDIR.TAG`.
fn clean_all_manual(paths: &[PathBuf], dry_run: bool) -> Result<CleanResult> {
    let (projects, skipped) = projects_to_clean(paths);

    if projects.is_empty() {
        return Ok(CleanResult {
            bytes_freed: 0,
            files_removed: 0,
            projects_cleaned: 0,
            errors: skipped,
            was_dry_run: dry_run,
            summary: "No Rust projects found to clean.".to_string(),
        });
    }

    if dry_run {
        let total_bytes: u64 = projects.iter().map(|p| p.target_size_bytes).sum();
        return Ok(CleanResult::dry_run_summary(
            projects.len() as u32,
            total_bytes,
        ));
    }

    let mut total_freed: u64 = 0;
    let mut cleaned: u32 = 0;
    let mut errors: Vec<String> = skipped;

    for project in &projects {
        let before_size = project.target_size_bytes;

        let target_dir = project.path.join("target");
        match std::fs::remove_dir_all(&target_dir) {
            Ok(()) => {
                total_freed += before_size;
                cleaned += 1;
            }
            Err(e) => errors.push(format!("Failed to remove {}: {e}", target_dir.display())),
        }
    }

    let summary = format!(
        "Manually cleaned {} of {} projects, freeing ~{}",
        cleaned,
        projects.len(),
        format_bytes(total_freed)
    );

    Ok(CleanResult {
        bytes_freed: total_freed,
        files_removed: 0,
        projects_cleaned: cleaned,
        errors,
        was_dry_run: false,
        summary,
    })
}

fn parse_clean_all_output(output: &str) -> (u64, u32) {
    let mut total_bytes: u64 = 0;
    let mut project_count: u32 = 0;

    for line in output.lines() {
        // Count lines that mention cleaning a project
        if line.contains("Cleaning") || line.contains("cleaned") {
            project_count += 1;
        }

        // Parse size values — strip parentheses and other non-numeric chars
        let parts: Vec<&str> = line.split_whitespace().collect();
        for window in parts.windows(2) {
            let cleaned = window[0].trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
            if let Ok(num) = cleaned.parse::<f64>() {
                let unit = window[1].trim_matches(|c: char| !c.is_alphanumeric());
                let multiplier = match unit.to_uppercase().as_str() {
                    "GIB" | "GB" => Some(1_073_741_824u64),
                    "MIB" | "MB" => Some(1_048_576u64),
                    "KIB" | "KB" => Some(1024u64),
                    "B" => Some(1u64),
                    _ => None,
                };
                if let Some(m) = multiplier {
                    total_bytes += (num * m as f64) as u64;
                }
            }
        }
    }

    (total_bytes, project_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn tag(target: &Path) {
        std::fs::write(
            target.join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
    }

    /// Round 6 (reviewer C): `cargo clean` also removed cargo's configured
    /// `build.build-dir`, possibly shared, and a relative scan path made it
    /// clean nothing while reporting bytes freed. The fallback now removes
    /// the project's own tagged `target/` and leaves an untagged one alone.
    #[test]
    fn test_clean_all_manual_removes_only_a_tagged_target() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["tagged", "untagged"] {
            let project = tmp.path().join(name);
            std::fs::create_dir_all(project.join("target")).unwrap();
            std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
            std::fs::write(project.join("target/artifact"), b"data").unwrap();
        }
        tag(&tmp.path().join("tagged/target"));

        let result = clean_all_manual(&[tmp.path().to_path_buf()], false).unwrap();
        assert!(!tmp.path().join("tagged/target").exists());
        assert!(tmp.path().join("untagged/target/artifact").exists());
        assert_eq!(result.projects_cleaned, 1);
        assert!(
            result.errors.iter().any(|e| e.contains("CACHEDIR.TAG")),
            "{:?}",
            result.errors
        );
    }

    /// Round 5 (reviewer C): a `target` that links outside the root was
    /// cleaned through the link.
    #[cfg(unix)]
    #[test]
    fn test_clean_all_manual_skips_a_project_whose_target_is_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let outside = tmp.path().join("outside-target");
        std::fs::create_dir_all(root.join("p")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.bin"), b"keep").unwrap();
        std::fs::write(root.join("p/Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("p/target")).unwrap();

        let result = clean_all_manual(std::slice::from_ref(&root), false).unwrap();
        assert!(outside.join("keep.bin").exists());
        assert_eq!(result.projects_cleaned, 0);
        assert!(
            result.errors.iter().any(|e| e.contains("symbolic link")),
            "{:?}",
            result.errors
        );
    }
    use std::fs;

    #[test]
    fn test_clean_all_manual_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let result = clean_all_manual(&[tmp.path().to_path_buf()], true).unwrap();
        assert_eq!(result.projects_cleaned, 0);
        assert!(result.summary.contains("No Rust projects"));
    }

    #[test]
    fn test_clean_all_manual_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("test-proj");
        fs::create_dir_all(proj.join("target")).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]\nname=\"t\"").unwrap();
        fs::write(proj.join("target/artifact"), "data").unwrap();
        tag(&proj.join("target"));

        let result = clean_all_manual(&[tmp.path().to_path_buf()], true).unwrap();
        assert!(result.was_dry_run);
        assert_eq!(result.projects_cleaned, 1);
    }

    #[test]
    fn test_parse_clean_all_output() {
        let output = "Cleaning /tmp/a (512 MiB)\nCleaning /tmp/b (1.5 GiB)\n";
        let (bytes, count) = parse_clean_all_output(output);
        assert_eq!(count, 2);
        assert!(bytes > 0);
    }
}
