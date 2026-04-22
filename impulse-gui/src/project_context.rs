//! Lightweight readers for `.impulse/GENOME.md` and `HISTORY.jsonl`.
//!
//! The GUI crate can't depend on `impulse-rs` (sibling workspace member),
//! so we define minimal deserialization structs that only capture the fields
//! needed for init context injection.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// GENOME types (JSON stored as GENOME.md)
// ---------------------------------------------------------------------------

/// Top-level GENOME file structure (subset).
#[derive(Deserialize)]
struct GenomeFile {
    #[serde(default)]
    decisions: Vec<GenomeDecision>,
}

/// A single standing decision from the project GENOME.
#[derive(Debug, Clone, Deserialize)]
pub struct GenomeDecision {
    pub description: String,
    #[serde(default, alias = "date")]
    pub timestamp: Option<String>,
}

// ---------------------------------------------------------------------------
// HISTORY types (JSONL — one entry per line)
// ---------------------------------------------------------------------------

/// A single session entry from HISTORY.jsonl (subset).
///
/// Some fields are deserialized for future init context injection but not yet
/// read outside of tests.
#[derive(Debug, Clone, Deserialize)]
// dead_code: the GUI keeps the full history-entry subset available for init context surfaces and tests.
#[allow(dead_code)]
pub struct SessionEntry {
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub files_touched: Vec<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load the most recent N decisions from `<impulse_dir>/GENOME.md`.
///
/// Returns an empty vec on any error (missing file, parse failure, etc.).
pub fn load_recent_decisions(impulse_dir: &Path, limit: usize) -> Vec<GenomeDecision> {
    let path = impulse_dir.join("GENOME.md");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let genome: GenomeFile = match serde_json::from_str(&content) {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };

    let decisions = genome.decisions;
    if decisions.len() <= limit {
        decisions
    } else {
        decisions[decisions.len() - limit..].to_vec()
    }
}

/// Load all session entries from `<impulse_dir>/HISTORY.jsonl`.
///
/// Returns an empty vec on any error (missing file, etc.).
// dead_code: reserved for future history views and exercised by the unit tests below.
#[allow(dead_code)]
pub fn load_all_sessions(impulse_dir: &Path) -> Vec<SessionEntry> {
    let path = impulse_dir.join("HISTORY.jsonl");
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);

    reader
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect()
}

/// Load the last session entry from `<impulse_dir>/HISTORY.jsonl`.
///
/// Returns None on any error (missing file, empty file, parse failure).
pub fn load_last_session(impulse_dir: &Path) -> Option<SessionEntry> {
    let path = impulse_dir.join("HISTORY.jsonl");
    let file = std::fs::File::open(&path).ok()?;
    let reader = BufReader::new(file);

    let mut last_line: Option<String> = None;
    for line in reader.lines() {
        match line {
            Ok(l) if !l.trim().is_empty() => last_line = Some(l),
            _ => {}
        }
    }

    let line = last_line?;
    serde_json::from_str(&line).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_genome(dir: &Path, content: &str) {
        std::fs::write(dir.join("GENOME.md"), content).unwrap();
    }

    fn write_history(dir: &Path, content: &str) {
        std::fs::write(dir.join("HISTORY.jsonl"), content).unwrap();
    }

    // -- GENOME tests --

    #[test]
    fn test_missing_genome_returns_empty() {
        let dir = TempDir::new().unwrap();
        let result = load_recent_decisions(dir.path(), 5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_empty_genome_returns_empty() {
        let dir = TempDir::new().unwrap();
        write_genome(
            dir.path(),
            r#"{"decisions":[],"preferences":[],"constraints":[],"last_updated":null}"#,
        );
        let result = load_recent_decisions(dir.path(), 5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_genome_returns_last_n_decisions() {
        let dir = TempDir::new().unwrap();
        write_genome(
            dir.path(),
            r#"{"decisions":[
                {"date":"2026-01-01T00:00:00Z","description":"First","rationale":null,"tags":[]},
                {"date":"2026-01-02T00:00:00Z","description":"Second","rationale":"why","tags":[]},
                {"date":"2026-01-03T00:00:00Z","description":"Third","rationale":null,"tags":[]}
            ],"preferences":[],"constraints":[],"last_updated":"2026-01-03T00:00:00Z"}"#,
        );
        let result = load_recent_decisions(dir.path(), 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].description, "Second");
        assert_eq!(result[1].description, "Third");
    }

    #[test]
    fn test_genome_limit_larger_than_count() {
        let dir = TempDir::new().unwrap();
        write_genome(
            dir.path(),
            r#"{"decisions":[
                {"date":"2026-01-01T00:00:00Z","description":"Only one","rationale":null,"tags":[]}
            ],"preferences":[],"constraints":[],"last_updated":"2026-01-01T00:00:00Z"}"#,
        );
        let result = load_recent_decisions(dir.path(), 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].description, "Only one");
    }

    #[test]
    fn test_genome_timestamp_alias() {
        let dir = TempDir::new().unwrap();
        write_genome(
            dir.path(),
            r#"{"decisions":[
                {"date":"2026-02-15T12:00:00Z","description":"With date","rationale":null,"tags":[]}
            ],"preferences":[],"constraints":[],"last_updated":null}"#,
        );
        let result = load_recent_decisions(dir.path(), 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].timestamp.as_deref(), Some("2026-02-15T12:00:00Z"));
    }

    #[test]
    fn test_malformed_genome_returns_empty() {
        let dir = TempDir::new().unwrap();
        write_genome(dir.path(), "this is not json at all {{{");
        let result = load_recent_decisions(dir.path(), 5);
        assert!(result.is_empty());
    }

    // -- HISTORY tests --

    #[test]
    fn test_missing_history_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_last_session(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_history_returns_none() {
        let dir = TempDir::new().unwrap();
        write_history(dir.path(), "");
        let result = load_last_session(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_history_returns_last_entry() {
        let dir = TempDir::new().unwrap();
        write_history(
            dir.path(),
            &format!(
                "{}\n{}",
                r#"{"session_id":"s1","session_name":"first","summary":"did stuff","files_touched":["a.rs"],"tools_used":[],"started_at":"2026-01-01T00:00:00Z","ended_at":"2026-01-01T01:00:00Z"}"#,
                r#"{"session_id":"s2","session_name":"second","summary":"more work","files_touched":["b.rs","c.rs"],"tools_used":["cargo"],"started_at":"2026-01-02T00:00:00Z","ended_at":"2026-01-02T02:00:00Z"}"#
            ),
        );
        let result = load_last_session(dir.path()).unwrap();
        assert_eq!(result.summary.as_deref(), Some("more work"));
        assert_eq!(result.files_touched, vec!["b.rs", "c.rs"]);
    }

    #[test]
    fn test_single_line_history() {
        let dir = TempDir::new().unwrap();
        write_history(
            dir.path(),
            r#"{"session_id":"s1","session_name":"only","summary":"single session","files_touched":[],"tools_used":[],"started_at":"2026-01-01T00:00:00Z","ended_at":"2026-01-01T01:00:00Z"}"#,
        );
        let result = load_last_session(dir.path()).unwrap();
        assert_eq!(result.summary.as_deref(), Some("single session"));
    }

    #[test]
    fn test_malformed_history_returns_none() {
        let dir = TempDir::new().unwrap();
        write_history(dir.path(), "not json\nalso not json\n");
        let result = load_last_session(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_history_skips_blank_lines() {
        let dir = TempDir::new().unwrap();
        write_history(
            dir.path(),
            &format!(
                "{}\n\n\n",
                r#"{"session_id":"s1","session_name":"test","summary":"works","files_touched":[],"tools_used":[],"started_at":"2026-01-01T00:00:00Z","ended_at":"2026-01-01T01:00:00Z"}"#,
            ),
        );
        let result = load_last_session(dir.path()).unwrap();
        assert_eq!(result.summary.as_deref(), Some("works"));
    }
}
