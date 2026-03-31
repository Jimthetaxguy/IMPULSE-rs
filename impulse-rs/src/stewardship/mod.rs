//! Progressive Context Stewardship
//!
//! Monitors AI session context usage and progressively curates context
//! quality as the context window fills up. Provides:
//!
//! - Session JSONL analysis (token estimation, pattern detection)
//! - Progressive cleanup at configurable thresholds (30/45/60/80%)
//! - Cross-project learning via YAML-based memory
//! - User approval workflow (auto/review/off modes)
//! - Compaction assistance with refined context injection

pub mod analyzer;
pub mod approval;
pub mod cleanup;
pub mod cross_project;
pub mod monitor;
pub mod types;

pub use types::*;

/// Atomic write helper: temp file + rename for crash safety.
///
/// Delegates to [`crate::storage::Storage::atomic_write_path`] which is
/// the single canonical implementation of PID+timestamp atomic writes.
pub(crate) fn atomic_write_file(path: &std::path::Path, content: &[u8]) -> anyhow::Result<()> {
    crate::storage::Storage::atomic_write_path(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_write_file_creates_and_writes() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        atomic_write_file(&path, b"hello world").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_atomic_write_file_overwrites_existing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        atomic_write_file(&path, b"first").unwrap();
        atomic_write_file(&path, b"second").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "second");
    }

    #[test]
    fn test_atomic_write_file_no_temp_residue() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        atomic_write_file(&path, b"data").unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(
            files.len(),
            1,
            "Should only have the target file, no temp residue"
        );
    }
}
