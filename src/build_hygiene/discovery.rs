// Discovery — find Rust projects by locating Cargo.toml and target/ directories

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A discovered Rust project with its build artifact metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustProject {
    /// Path to the project root (contains Cargo.toml)
    pub path: PathBuf,
    /// Size of target/ directory in bytes
    pub target_size_bytes: u64,
    /// Last modification time of target/ directory
    pub last_modified: Option<SystemTime>,
    /// Whether a Cargo.lock file exists
    pub has_cargo_lock: bool,
    /// Detected toolchain versions (from rust-toolchain.toml or target dirs)
    pub toolchain_versions: Vec<String>,
}

/// Discover Rust projects under the given search paths.
/// A Rust project is any directory containing a Cargo.toml with a target/ subdirectory.
pub fn discover_rust_projects(search_paths: &[PathBuf]) -> Vec<RustProject> {
    let mut projects = Vec::new();

    for search_path in search_paths {
        if !search_path.exists() {
            continue;
        }
        discover_recursive(search_path, &mut projects, 0, 5);
    }

    // Sort by target size (largest first) for prioritized reporting
    projects.sort_by(|a, b| b.target_size_bytes.cmp(&a.target_size_bytes));
    projects
}

fn discover_recursive(dir: &Path, projects: &mut Vec<RustProject>, depth: usize, max_depth: usize) {
    if depth > max_depth {
        return;
    }

    let cargo_toml = dir.join("Cargo.toml");
    let target_dir = dir.join("target");

    if cargo_toml.exists() && target_dir.exists() {
        let target_size = dir_size(&target_dir);
        let last_modified = std::fs::metadata(&target_dir)
            .and_then(|m| m.modified())
            .ok();
        let has_cargo_lock = dir.join("Cargo.lock").exists();
        let toolchain_versions = detect_toolchains(dir);

        projects.push(RustProject {
            path: dir.to_path_buf(),
            target_size_bytes: target_size,
            last_modified,
            has_cargo_lock,
            toolchain_versions,
        });

        // Don't recurse into target/ or .git/ of a found project
        return;
    }

    // Recurse into subdirectories
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden dirs, node_modules, target dirs without Cargo.toml parents
        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "target"
            || name_str == ".git"
            || name_str == "__pycache__"
            || name_str == "venv"
            || name_str == ".venv"
        {
            continue;
        }

        discover_recursive(&path, projects, depth + 1, max_depth);
    }
}

/// Calculate total size of a directory recursively
pub fn dir_size(path: &Path) -> u64 {
    if !path.is_dir() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }

    let mut total: u64 = 0;
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += dir_size(&path);
        } else {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// Detect toolchain versions from rust-toolchain.toml or target subdirectories
fn detect_toolchains(project_dir: &Path) -> Vec<String> {
    let mut versions = Vec::new();

    // Check rust-toolchain.toml
    let toolchain_file = project_dir.join("rust-toolchain.toml");
    if toolchain_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&toolchain_file) {
            for line in content.lines() {
                if line.contains("channel") {
                    if let Some(version) = line.split('=').nth(1) {
                        let v = version.trim().trim_matches('"').to_string();
                        if !v.is_empty() {
                            versions.push(v);
                        }
                    }
                }
            }
        }
    }

    // Also check rust-toolchain (plain text)
    let toolchain_plain = project_dir.join("rust-toolchain");
    if toolchain_plain.exists() {
        if let Ok(content) = std::fs::read_to_string(&toolchain_plain) {
            let v = content.trim().to_string();
            if !v.is_empty() && !versions.contains(&v) {
                versions.push(v);
            }
        }
    }

    versions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = discover_rust_projects(&[tmp.path().to_path_buf()]);
        assert!(projects.is_empty());
    }

    #[test]
    fn test_discover_finds_project() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("my-project");
        fs::create_dir_all(proj.join("target")).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();

        let projects = discover_rust_projects(&[tmp.path().to_path_buf()]);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, proj);
    }

    #[test]
    fn test_dir_size_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(tmp.path()), 0);
    }

    #[test]
    fn test_dir_size_with_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        fs::write(tmp.path().join("b.txt"), "world!").unwrap();
        let size = dir_size(tmp.path());
        assert!(size > 0);
        assert_eq!(size, 11); // "hello" + "world!"
    }

    #[test]
    fn test_discover_skips_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let hidden = tmp.path().join(".hidden-project");
        fs::create_dir_all(hidden.join("target")).unwrap();
        fs::write(hidden.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();

        let projects = discover_rust_projects(&[tmp.path().to_path_buf()]);
        assert!(projects.is_empty());
    }

    #[test]
    fn test_discover_sorted_by_size() {
        let tmp = tempfile::tempdir().unwrap();

        // Small project
        let small = tmp.path().join("small");
        fs::create_dir_all(small.join("target")).unwrap();
        fs::write(small.join("Cargo.toml"), "[package]").unwrap();
        fs::write(small.join("target/a"), "x").unwrap();

        // Big project
        let big = tmp.path().join("big");
        fs::create_dir_all(big.join("target")).unwrap();
        fs::write(big.join("Cargo.toml"), "[package]").unwrap();
        fs::write(big.join("target/b"), "x".repeat(1000)).unwrap();

        let projects = discover_rust_projects(&[tmp.path().to_path_buf()]);
        assert_eq!(projects.len(), 2);
        assert!(projects[0].target_size_bytes >= projects[1].target_size_bytes);
    }
}
