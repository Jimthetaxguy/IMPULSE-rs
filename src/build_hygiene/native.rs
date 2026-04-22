// Native — pure-Rust build artifact cleaning without external tool dependencies
//
// Provides filesystem-based sweep (stale artifact removal by mtime) and wipe
// (full target/ removal) using only std::fs. No cargo-sweep or cargo-wipe needed.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::build_hygiene::{format_bytes, CleanResult};

/// A file identified as stale (older than the age threshold)
#[derive(Debug, Clone)]
pub struct StaleFile {
    pub path: PathBuf,
    pub size: u64,
    pub age_days: f64,
}

/// Result of collecting stale files before deletion
#[derive(Debug)]
pub struct StaleInventory {
    pub files: Vec<StaleFile>,
    pub total_bytes: u64,
    pub scan_errors: Vec<String>,
}

/// Native sweep: walk target/ directories and remove files older than `max_age_days`.
///
/// This replaces `cargo-sweep` for systems where it isn't installed.
/// Algorithm:
/// 1. Walk target/ recursively (does not follow symlinks)
/// 2. Check each file's mtime against cutoff
/// 3. Delete stale files (or report in dry-run)
/// 4. Remove empty directories bottom-up
pub fn native_sweep(target_dir: &Path, max_age_days: u32, dry_run: bool) -> Result<CleanResult> {
    if !target_dir.exists() {
        return Ok(CleanResult {
            bytes_freed: 0,
            files_removed: 0,
            projects_cleaned: 0,
            errors: vec![],
            was_dry_run: dry_run,
            summary: format!("target/ does not exist: {}", target_dir.display()),
        });
    }

    let now = SystemTime::now();
    let cutoff = now
        .checked_sub(Duration::from_secs(max_age_days as u64 * 86400))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let inventory = walk_and_collect_stale(target_dir, cutoff, now);

    if dry_run {
        return Ok(CleanResult {
            bytes_freed: inventory.total_bytes,
            files_removed: inventory.files.len() as u32,
            projects_cleaned: if inventory.files.is_empty() { 0 } else { 1 },
            errors: inventory.scan_errors,
            was_dry_run: true,
            summary: format!(
                "[DRY RUN] Would remove {} stale files ({}) older than {} days from {}",
                inventory.files.len(),
                format_bytes(inventory.total_bytes),
                max_age_days,
                target_dir.display()
            ),
        });
    }

    // Live mode: delete stale files
    let mut freed: u64 = 0;
    let mut removed: u32 = 0;
    let mut errors = inventory.scan_errors;

    for stale in &inventory.files {
        match std::fs::remove_file(&stale.path) {
            Ok(()) => {
                freed += stale.size;
                removed += 1;
            }
            Err(e) => {
                errors.push(format!("Failed to remove {}: {}", stale.path.display(), e));
            }
        }
    }

    // Clean up empty directories after file removal
    if removed > 0 {
        let cleanup_errors = remove_empty_dirs(target_dir);
        errors.extend(cleanup_errors);
    }

    Ok(CleanResult {
        bytes_freed: freed,
        files_removed: removed,
        projects_cleaned: if removed > 0 { 1 } else { 0 },
        errors,
        was_dry_run: false,
        summary: format!(
            "Removed {}/{} stale files, freed {} from {}",
            removed,
            inventory.files.len(),
            format_bytes(freed),
            target_dir.display()
        ),
    })
}

/// Native sweep across multiple project paths.
/// Discovers projects and sweeps each one's target/ directory.
pub fn native_sweep_paths(
    paths: &[PathBuf],
    max_age_days: u32,
    dry_run: bool,
) -> Result<CleanResult> {
    let projects = crate::build_hygiene::discover_rust_projects(paths);

    let mut total_freed: u64 = 0;
    let mut total_removed: u32 = 0;
    let mut total_projects: u32 = 0;
    let mut all_errors: Vec<String> = Vec::new();

    for project in &projects {
        let target_dir = project.path.join("target");
        match native_sweep(&target_dir, max_age_days, dry_run) {
            Ok(result) => {
                total_freed += result.bytes_freed;
                total_removed += result.files_removed;
                if result.projects_cleaned > 0 {
                    total_projects += 1;
                }
                all_errors.extend(result.errors);
            }
            Err(e) => {
                all_errors.push(format!("Failed to sweep {}: {}", target_dir.display(), e));
            }
        }
    }

    let summary = if dry_run {
        format!(
            "[DRY RUN] Would remove {} stale files ({}) across {} projects (artifacts older than {} days)",
            total_removed,
            format_bytes(total_freed),
            total_projects,
            max_age_days
        )
    } else {
        format!(
            "Native sweep removed {} files, freed {} across {} projects (artifacts older than {} days)",
            total_removed,
            format_bytes(total_freed),
            total_projects,
            max_age_days
        )
    };

    Ok(CleanResult {
        bytes_freed: total_freed,
        files_removed: total_removed,
        projects_cleaned: total_projects,
        errors: all_errors,
        was_dry_run: dry_run,
        summary,
    })
}

/// Native wipe: remove entire target/ directories for discovered projects.
///
/// This replaces `cargo-wipe` for systems where it isn't installed.
pub fn native_wipe(paths: &[PathBuf], dry_run: bool) -> Result<CleanResult> {
    let projects = crate::build_hygiene::discover_rust_projects(paths);

    if projects.is_empty() {
        return Ok(CleanResult {
            bytes_freed: 0,
            files_removed: 0,
            projects_cleaned: 0,
            errors: vec![],
            was_dry_run: dry_run,
            summary: "No Rust projects with target/ directories found.".to_string(),
        });
    }

    if dry_run {
        let total_bytes: u64 = projects.iter().map(|p| p.target_size_bytes).sum();
        return Ok(CleanResult {
            bytes_freed: total_bytes,
            files_removed: 0,
            projects_cleaned: projects.len() as u32,
            errors: vec![],
            was_dry_run: true,
            summary: format!(
                "[DRY RUN] Would remove target/ from {} projects, freeing ~{}",
                projects.len(),
                format_bytes(total_bytes)
            ),
        });
    }

    let mut total_freed: u64 = 0;
    let mut cleaned: u32 = 0;
    let mut errors: Vec<String> = Vec::new();

    for project in &projects {
        let target_dir = project.path.join("target");
        if !target_dir.exists() {
            continue;
        }

        let size = project.target_size_bytes;
        match std::fs::remove_dir_all(&target_dir) {
            Ok(()) => {
                total_freed += size;
                cleaned += 1;
                tracing::debug!("Wiped {} ({})", target_dir.display(), format_bytes(size));
            }
            Err(e) => {
                errors.push(format!("Failed to remove {}: {}", target_dir.display(), e));
            }
        }
    }

    Ok(CleanResult {
        bytes_freed: total_freed,
        files_removed: 0,
        projects_cleaned: cleaned,
        errors,
        was_dry_run: false,
        summary: format!(
            "Wiped target/ from {} of {} projects, freed {}",
            cleaned,
            projects.len(),
            format_bytes(total_freed)
        ),
    })
}

/// Walk a target/ directory and collect files whose mtime is older than `cutoff`.
/// Uses `symlink_metadata` to avoid following symlinks outside target/.
/// `now` is passed through to compute `age_days` consistently (avoids calling
/// `SystemTime::now()` repeatedly during the walk).
pub fn walk_and_collect_stale(dir: &Path, cutoff: SystemTime, now: SystemTime) -> StaleInventory {
    let mut files = Vec::new();
    let mut total_bytes: u64 = 0;
    let mut scan_errors = Vec::new();

    walk_recursive(
        dir,
        cutoff,
        now,
        &mut files,
        &mut total_bytes,
        &mut scan_errors,
    );

    StaleInventory {
        files,
        total_bytes,
        scan_errors,
    }
}

fn walk_recursive(
    dir: &Path,
    cutoff: SystemTime,
    now: SystemTime,
    files: &mut Vec<StaleFile>,
    total_bytes: &mut u64,
    errors: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(format!("Cannot read {}: {}", dir.display(), e));
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Use symlink_metadata to avoid following symlinks out of target/
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("Cannot stat {}: {}", path.display(), e));
                continue;
            }
        };

        // Skip symlinks entirely — don't delete or follow them
        if meta.is_symlink() {
            continue;
        }

        if meta.is_dir() {
            walk_recursive(&path, cutoff, now, files, total_bytes, errors);
        } else {
            // Check mtime against cutoff
            let mtime = match meta.modified() {
                Ok(t) => t,
                Err(_) => {
                    // If we can't read mtime, skip this file (conservative)
                    continue;
                }
            };

            if mtime < cutoff {
                let size = meta.len();
                let age_secs = now.duration_since(mtime).unwrap_or_default().as_secs();
                let age_days = age_secs as f64 / 86400.0;

                files.push(StaleFile {
                    path,
                    size,
                    age_days,
                });
                *total_bytes += size;
            }
        }
    }
}

/// Remove empty directories bottom-up within a root directory.
/// Returns any errors encountered (non-fatal).
fn remove_empty_dirs(root: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    remove_empty_dirs_recursive(root, root, &mut errors);
    errors
}

// clippy: false positive — parameter used in recursive call
#[allow(clippy::only_used_in_recursion)]
fn remove_empty_dirs_recursive(dir: &Path, root: &Path, errors: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // Use symlink_metadata to avoid following symlinks (consistent with walk_recursive)
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() && !meta.is_symlink() {
            children.push(path);
        }
    }

    // Recurse into children first (bottom-up)
    for child in &children {
        remove_empty_dirs_recursive(child, root, errors);
    }

    // Don't remove the root target/ directory itself
    if dir == root {
        return;
    }

    // Try to remove this directory — only succeeds if empty
    match std::fs::remove_dir(dir) {
        Ok(()) => {
            tracing::trace!("Removed empty dir: {}", dir.display());
        }
        Err(_) => {
            // Not empty or permission denied — that's fine, skip it
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_file_with_mtime(path: &Path, content: &str, age_days: u64) {
        fs::write(path, content).unwrap();
        let mtime = SystemTime::now()
            .checked_sub(Duration::from_secs(age_days * 86400))
            .unwrap();
        filetime::set_file_mtime(path, filetime::FileTime::from_system_time(mtime)).unwrap();
    }

    #[test]
    fn test_walk_and_collect_stale_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let cutoff = now.checked_sub(Duration::from_secs(30 * 86400)).unwrap();
        let inventory = walk_and_collect_stale(tmp.path(), cutoff, now);
        assert!(inventory.files.is_empty());
        assert_eq!(inventory.total_bytes, 0);
    }

    #[test]
    fn test_walk_and_collect_stale_finds_old_files() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        // Create a file aged 60 days
        create_file_with_mtime(&target.join("old.o"), "old-content", 60);

        // Create a recent file
        fs::write(target.join("new.o"), "new-content").unwrap();

        let now = SystemTime::now();
        let cutoff = now.checked_sub(Duration::from_secs(30 * 86400)).unwrap();
        let inventory = walk_and_collect_stale(&target, cutoff, now);

        assert_eq!(inventory.files.len(), 1);
        assert!(inventory.files[0].path.ends_with("old.o"));
        assert!(inventory.files[0].age_days >= 59.0);
    }

    #[test]
    fn test_walk_and_collect_stale_nested_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("debug").join("deps");
        fs::create_dir_all(&nested).unwrap();

        create_file_with_mtime(&nested.join("old-dep.d"), "dep-data", 45);

        let now = SystemTime::now();
        let cutoff = now.checked_sub(Duration::from_secs(30 * 86400)).unwrap();
        let inventory = walk_and_collect_stale(tmp.path(), cutoff, now);

        assert_eq!(inventory.files.len(), 1);
        assert!(inventory.files[0].path.ends_with("old-dep.d"));
    }

    #[test]
    fn test_native_sweep_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        create_file_with_mtime(&target.join("old.o"), "stale", 60);
        fs::write(target.join("new.o"), "fresh").unwrap();

        let result = native_sweep(&target, 30, true).unwrap();

        assert!(result.was_dry_run);
        assert_eq!(result.files_removed, 1);
        assert!(result.summary.contains("DRY RUN"));
        // File should still exist
        assert!(target.join("old.o").exists());
    }

    #[test]
    fn test_native_sweep_live_removes_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        create_file_with_mtime(&target.join("old.o"), "stale-content", 60);
        fs::write(target.join("new.o"), "fresh-content").unwrap();

        let result = native_sweep(&target, 30, false).unwrap();

        assert!(!result.was_dry_run);
        assert_eq!(result.files_removed, 1);
        assert!(result.bytes_freed > 0);
        // Old file removed, new file preserved
        assert!(!target.join("old.o").exists());
        assert!(target.join("new.o").exists());
    }

    #[test]
    fn test_native_sweep_removes_empty_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        let nested = target.join("debug").join("deps");
        fs::create_dir_all(&nested).unwrap();

        create_file_with_mtime(&nested.join("old.d"), "dep", 60);

        let result = native_sweep(&target, 30, false).unwrap();

        assert_eq!(result.files_removed, 1);
        // Empty dirs should be cleaned up
        assert!(!nested.exists());
        assert!(!target.join("debug").exists());
    }

    #[test]
    fn test_native_sweep_nonexistent_target() {
        let result =
            native_sweep(Path::new("/tmp/nonexistent_xyz_12345/target"), 30, false).unwrap();
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.files_removed, 0);
    }

    #[test]
    fn test_native_sweep_preserves_recent_files() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        // All files are fresh
        fs::write(target.join("a.o"), "aaa").unwrap();
        fs::write(target.join("b.o"), "bbb").unwrap();

        let result = native_sweep(&target, 30, false).unwrap();

        assert_eq!(result.files_removed, 0);
        assert_eq!(result.bytes_freed, 0);
        assert!(target.join("a.o").exists());
        assert!(target.join("b.o").exists());
    }

    #[test]
    fn test_native_wipe_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("my-project");
        fs::create_dir_all(proj.join("target")).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
        fs::write(proj.join("target/artifact"), "data").unwrap();

        let result = native_wipe(&[tmp.path().to_path_buf()], true).unwrap();

        assert!(result.was_dry_run);
        assert_eq!(result.projects_cleaned, 1);
        assert!(result.summary.contains("DRY RUN"));
        // Directory should still exist
        assert!(proj.join("target").exists());
    }

    #[test]
    fn test_native_wipe_live() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("my-project");
        fs::create_dir_all(proj.join("target")).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
        fs::write(proj.join("target/artifact"), "data").unwrap();

        let result = native_wipe(&[tmp.path().to_path_buf()], false).unwrap();

        assert!(!result.was_dry_run);
        assert_eq!(result.projects_cleaned, 1);
        assert!(result.bytes_freed > 0);
        // target/ should be gone
        assert!(!proj.join("target").exists());
        // Cargo.toml should still exist
        assert!(proj.join("Cargo.toml").exists());
    }

    #[test]
    fn test_native_wipe_no_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let result = native_wipe(&[tmp.path().to_path_buf()], false).unwrap();
        assert_eq!(result.projects_cleaned, 0);
    }

    #[test]
    fn test_native_wipe_multiple_projects() {
        let tmp = tempfile::tempdir().unwrap();

        for name in &["proj-a", "proj-b", "proj-c"] {
            let proj = tmp.path().join(name);
            fs::create_dir_all(proj.join("target")).unwrap();
            fs::write(proj.join("Cargo.toml"), "[package]").unwrap();
            fs::write(proj.join("target/bin"), "binary").unwrap();
        }

        let result = native_wipe(&[tmp.path().to_path_buf()], false).unwrap();
        assert_eq!(result.projects_cleaned, 3);
    }

    #[test]
    fn test_remove_empty_dirs_preserves_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("target");
        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        let errors = remove_empty_dirs(&root);
        assert!(errors.is_empty());
        // Root should still exist, empty children should be gone
        assert!(root.exists());
        assert!(!root.join("a").exists());
    }

    #[test]
    fn test_remove_empty_dirs_keeps_nonempty() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("target");
        let empty_dir = root.join("empty");
        let full_dir = root.join("full");
        fs::create_dir_all(&empty_dir).unwrap();
        fs::create_dir_all(&full_dir).unwrap();
        fs::write(full_dir.join("keep.txt"), "data").unwrap();

        let errors = remove_empty_dirs(&root);
        assert!(errors.is_empty());
        assert!(!empty_dir.exists());
        assert!(full_dir.exists());
        assert!(full_dir.join("keep.txt").exists());
    }

    #[test]
    fn test_native_sweep_paths_integration() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("test-proj");
        fs::create_dir_all(proj.join("target/debug")).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();

        create_file_with_mtime(&proj.join("target/debug/old.o"), "old", 60);
        fs::write(proj.join("target/debug/new.o"), "new").unwrap();

        let result = native_sweep_paths(&[tmp.path().to_path_buf()], 30, false).unwrap();

        assert_eq!(result.files_removed, 1);
        assert!(result.projects_cleaned > 0);
        assert!(!proj.join("target/debug/old.o").exists());
        assert!(proj.join("target/debug/new.o").exists());
    }

    /// Verify that walk_and_collect_stale skips symlinks entirely.
    #[cfg(unix)]
    #[test]
    fn test_walk_and_collect_stale_skips_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        // Create a real old file
        create_file_with_mtime(&target.join("real.o"), "real-data", 60);

        // Create a symlink to a file outside target/
        let outside = tmp.path().join("outside.dat");
        fs::write(&outside, "external-data").unwrap();
        create_file_with_mtime(&outside, "external-data", 90);
        std::os::unix::fs::symlink(&outside, target.join("link.o")).unwrap();

        // Create a symlink to a directory outside target/
        let outside_dir = tmp.path().join("outside_dir");
        fs::create_dir_all(&outside_dir).unwrap();
        create_file_with_mtime(&outside_dir.join("deep.o"), "deep", 60);
        std::os::unix::fs::symlink(&outside_dir, target.join("link_dir")).unwrap();

        let now = SystemTime::now();
        let cutoff = now.checked_sub(Duration::from_secs(30 * 86400)).unwrap();
        let inventory = walk_and_collect_stale(&target, cutoff, now);

        // Should only find real.o, not the symlinked file or anything behind symlinked dir
        assert_eq!(inventory.files.len(), 1);
        assert!(inventory.files[0].path.ends_with("real.o"));
    }

    /// Verify that native_sweep_paths accumulates errors instead of aborting
    /// when one project fails mid-sweep.
    #[test]
    fn test_native_sweep_paths_error_accumulation() {
        let tmp = tempfile::tempdir().unwrap();

        // Project A: valid, with a stale file
        let proj_a = tmp.path().join("proj-a");
        fs::create_dir_all(proj_a.join("target")).unwrap();
        fs::write(proj_a.join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        create_file_with_mtime(&proj_a.join("target/old.o"), "stale", 60);

        // Project B: valid, with a stale file
        let proj_b = tmp.path().join("proj-b");
        fs::create_dir_all(proj_b.join("target")).unwrap();
        fs::write(proj_b.join("Cargo.toml"), "[package]\nname = \"b\"\n").unwrap();
        create_file_with_mtime(&proj_b.join("target/old.o"), "stale", 60);

        // Make proj-a's target/ unreadable to force an error during sweep
        // (We remove read permission so read_dir fails inside walk_recursive)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(proj_a.join("target"), fs::Permissions::from_mode(0o000)).unwrap();
        }

        let result = native_sweep_paths(&[tmp.path().to_path_buf()], 30, false).unwrap();

        // On Unix: proj-a fails (permission denied), proj-b succeeds
        // On non-Unix: both succeed (no permission trick available)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // proj-b should still have been cleaned
            assert!(!proj_b.join("target/old.o").exists());
            // There should be at least one error from proj-a
            assert!(!result.errors.is_empty());
            // Restore permissions for cleanup
            fs::set_permissions(proj_a.join("target"), fs::Permissions::from_mode(0o755)).unwrap();
        }

        #[cfg(not(unix))]
        {
            // Both should succeed on non-Unix
            assert!(result.files_removed >= 1);
        }
    }

    /// Verify that max_age_days=0 treats all files as stale.
    #[test]
    fn test_native_sweep_max_age_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        // Create files with various ages — all should be stale with max_age_days=0
        fs::write(target.join("brand_new.o"), "just-created").unwrap();
        create_file_with_mtime(&target.join("one_day.o"), "day-old", 1);
        create_file_with_mtime(&target.join("ancient.o"), "very-old", 365);

        let result = native_sweep(&target, 0, true).unwrap();

        // All 3 files should be considered stale
        assert_eq!(result.files_removed, 3);
        assert!(result.was_dry_run);
        assert!(result.bytes_freed > 0);
    }
}
