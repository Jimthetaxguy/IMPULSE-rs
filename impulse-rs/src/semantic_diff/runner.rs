//! Subprocess runner for the `sem` CLI tool.
//!
//! All interactions with `sem` go through this module. It spawns `sem` as a child
//! process, captures JSON output, and parses it into our types.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::process_util::run_with_timeout;
use crate::storage::sanitize_filename;

use super::types::*;

/// Hard timeout for any `sem` subprocess. A stuck `sem` (e.g. on a pathological
/// repo) must never hang the caller indefinitely.
const SEM_TIMEOUT: Duration = Duration::from_secs(30);

/// Check whether the `sem` CLI is available on PATH.
pub fn sem_available() -> bool {
    which::which("sem").is_ok()
}

/// Run `sem diff` between two Git refs and return parsed entity changes.
///
/// # Arguments
/// * `repo_path` — path to the Git repository
/// * `base_ref` — base Git ref (commit, branch, tag)
/// * `head_ref` — head Git ref (commit, branch, tag, or empty for working tree)
pub fn run_semantic_diff(
    repo_path: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<EntityChange>> {
    if !sem_available() {
        anyhow::bail!(
            "sem CLI not found on PATH. Install from https://github.com/Ataraxy-Labs/sem"
        );
    }

    let range = if head_ref.is_empty() {
        base_ref.to_string()
    } else {
        format!("{}..{}", base_ref, head_ref)
    };

    let mut cmd = Command::new("sem");
    cmd.arg("diff")
        .arg(&range)
        .arg("--format")
        .arg("json")
        .current_dir(repo_path);
    let output = run_with_timeout(cmd, SEM_TIMEOUT).context("failed to run `sem diff`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // sem returns non-zero when there are no changes in some versions
        if stderr.contains("no changes") || stderr.contains("No changes") {
            return Ok(Vec::new());
        }
        anyhow::bail!("sem diff failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    parse_sem_diff_output(&stdout)
}

/// Run `sem blame` on a file and return entity-level blame entries.
pub fn run_semantic_blame(repo_path: &Path, file_path: &str) -> Result<Vec<SemanticBlameEntry>> {
    if !sem_available() {
        anyhow::bail!(
            "sem CLI not found on PATH. Install from https://github.com/Ataraxy-Labs/sem"
        );
    }

    let mut cmd = Command::new("sem");
    cmd.arg("blame")
        .arg(file_path)
        .arg("--format")
        .arg("json")
        .current_dir(repo_path);
    let output = run_with_timeout(cmd, SEM_TIMEOUT).context("failed to run `sem blame`")?;

    if !output.status.success() {
        anyhow::bail!(
            "sem blame failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    let entries: Vec<SemanticBlameEntry> =
        serde_json::from_str(&stdout).context("failed to parse sem blame JSON output")?;
    Ok(entries)
}

/// Run `sem impact` for a given entity and return its blast radius.
pub fn run_semantic_impact(repo_path: &Path, entity_name: &str) -> Result<ImpactResult> {
    if !sem_available() {
        anyhow::bail!(
            "sem CLI not found on PATH. Install from https://github.com/Ataraxy-Labs/sem"
        );
    }

    let mut cmd = Command::new("sem");
    cmd.arg("impact")
        .arg(entity_name)
        .arg("--format")
        .arg("json")
        .current_dir(repo_path);
    let output = run_with_timeout(cmd, SEM_TIMEOUT).context("failed to run `sem impact`")?;

    if !output.status.success() {
        anyhow::bail!(
            "sem impact failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: ImpactResult =
        serde_json::from_str(&stdout).context("failed to parse sem impact JSON output")?;
    Ok(result)
}

/// Capture semantic diff at session end and store it.
///
/// Called during session-end to record what semantically changed during the session.
/// The result is stored in `.impulse/semantic_diffs/<session_id>.json`.
pub fn capture_semantic_diff(
    impulse_dir: &Path,
    repo_path: &Path,
    session_id: &str,
    base_ref: &str,
    head_ref: &str,
) -> Result<SemanticDiffReport> {
    let changes = run_semantic_diff(repo_path, base_ref, head_ref)?;

    let report = SemanticDiffReport::new(
        session_id.to_string(),
        base_ref.to_string(),
        head_ref.to_string(),
        changes,
    );

    // Store the report — sanitize session_id to prevent path traversal
    let safe_id = sanitize_filename(session_id);
    let diff_dir = impulse_dir.join("semantic_diffs");
    std::fs::create_dir_all(&diff_dir).context("failed to create semantic_diffs directory")?;

    let report_path = diff_dir.join(format!("{}.json", safe_id));
    let json = serde_json::to_string_pretty(&report)
        .context("failed to serialize semantic diff report")?;

    // Atomic write: temp file + rename
    let tmp_path = diff_dir.join(format!(".{}.{}.tmp", safe_id, std::process::id()));
    std::fs::write(&tmp_path, json.as_bytes())
        .context("failed to write semantic diff temp file")?;
    std::fs::rename(&tmp_path, &report_path)
        .context("failed to rename semantic diff report into place")?;

    Ok(report)
}

/// Load a previously stored semantic diff report for a session.
#[cfg(test)]
pub fn load_semantic_diff(
    impulse_dir: &Path,
    session_id: &str,
) -> Result<Option<SemanticDiffReport>> {
    let safe_id = sanitize_filename(session_id);
    let report_path = impulse_dir
        .join("semantic_diffs")
        .join(format!("{}.json", safe_id));

    if !report_path.exists() {
        return Ok(None);
    }

    let content =
        std::fs::read_to_string(&report_path).context("failed to read semantic diff report")?;
    let report: SemanticDiffReport =
        serde_json::from_str(&content).context("failed to parse semantic diff report")?;
    Ok(Some(report))
}

/// List all stored semantic diff session IDs.
#[cfg(test)]
pub fn list_semantic_diffs(impulse_dir: &Path) -> Result<Vec<String>> {
    let diff_dir = impulse_dir.join("semantic_diffs");
    if !diff_dir.exists() {
        return Ok(Vec::new());
    }

    let mut session_ids = Vec::new();
    for entry in std::fs::read_dir(&diff_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !stem.starts_with('.') {
                    session_ids.push(stem.to_string());
                }
            }
        }
    }
    session_ids.sort();
    Ok(session_ids)
}

// ============================================================================
// JSON parsing helpers
// ============================================================================

/// Parse the JSON output from `sem diff --format json`.
///
/// The sem tool outputs a JSON array of change objects. We normalize
/// the field names to our internal types.
fn parse_sem_diff_output(json_str: &str) -> Result<Vec<EntityChange>> {
    // sem outputs a JSON object with a "changes" array (or sometimes a top-level array)
    let value: serde_json::Value =
        serde_json::from_str(json_str).context("failed to parse sem diff JSON")?;

    let changes_array = if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(arr) = value.get("changes").and_then(|v| v.as_array()) {
        arr.clone()
    } else if let Some(arr) = value.get("entities").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        // Single object or unknown structure — try to parse as-is
        vec![value]
    };

    let mut results = Vec::new();
    for item in &changes_array {
        if let Ok(change) = parse_single_change(item) {
            results.push(change);
        }
    }

    Ok(results)
}

/// Parse a single change object from sem's JSON output.
fn parse_single_change(value: &serde_json::Value) -> Result<EntityChange> {
    // Try direct deserialization first
    if let Ok(change) = serde_json::from_value::<EntityChange>(value.clone()) {
        return Ok(change);
    }

    // Manual extraction for varying sem output formats
    let kind_str = value
        .get("change_type")
        .or_else(|| value.get("kind"))
        .or_else(|| value.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("modified");

    let kind = match kind_str.to_lowercase().as_str() {
        "added" | "add" | "new" => ChangeKind::Added,
        "modified" | "modify" | "changed" | "change" => ChangeKind::Modified,
        "deleted" | "delete" | "removed" | "remove" => ChangeKind::Deleted,
        "moved" | "move" => ChangeKind::Moved,
        "renamed" | "rename" => ChangeKind::Renamed,
        _ => ChangeKind::Modified,
    };

    let entity = parse_entity_info(value)?;

    let previous = value
        .get("previous")
        .or_else(|| value.get("old"))
        .and_then(|v| parse_entity_info(v).ok());

    Ok(EntityChange {
        kind,
        entity,
        previous,
    })
}

/// Extract entity info from a JSON value.
fn parse_entity_info(value: &serde_json::Value) -> Result<EntityInfo> {
    // Try nested entity object first
    let entity_obj = value.get("entity").unwrap_or(value);

    let name = entity_obj
        .get("name")
        .or_else(|| entity_obj.get("identifier"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let entity_type = entity_obj
        .get("entity_type")
        .or_else(|| entity_obj.get("type"))
        .or_else(|| entity_obj.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let file_path = entity_obj
        .get("file_path")
        .or_else(|| entity_obj.get("file"))
        .or_else(|| entity_obj.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let start_line = entity_obj
        .get("start_line")
        .or_else(|| entity_obj.get("line"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let end_line = entity_obj
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let parent = entity_obj
        .get("parent")
        .or_else(|| entity_obj.get("parent_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(EntityInfo {
        name,
        entity_type,
        file_path,
        start_line,
        end_line,
        parent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sem_available_returns_bool() {
        // Just verify it doesn't panic — sem may or may not be installed
        let _ = sem_available();
    }

    #[test]
    fn test_parse_sem_diff_array_format() {
        let json = r#"[
            {
                "change_type": "added",
                "name": "new_function",
                "type": "function",
                "file_path": "src/lib.rs",
                "start_line": 10,
                "end_line": 20
            },
            {
                "change_type": "modified",
                "name": "existing_fn",
                "type": "function",
                "file_path": "src/lib.rs",
                "start_line": 30,
                "end_line": 45
            }
        ]"#;

        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[0].entity.name, "new_function");
        assert_eq!(changes[1].kind, ChangeKind::Modified);
        assert_eq!(changes[1].entity.name, "existing_fn");
    }

    #[test]
    fn test_parse_sem_diff_object_format() {
        let json = r#"{
            "changes": [
                {
                    "kind": "deleted",
                    "entity": {
                        "name": "old_fn",
                        "entity_type": "function",
                        "file_path": "src/old.rs"
                    }
                }
            ]
        }"#;

        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Deleted);
        assert_eq!(changes[0].entity.name, "old_fn");
    }

    #[test]
    fn test_parse_rename_with_previous() {
        let json = r#"[
            {
                "change_type": "renamed",
                "name": "new_name",
                "type": "function",
                "file_path": "src/lib.rs",
                "previous": {
                    "name": "old_name",
                    "type": "function",
                    "file_path": "src/lib.rs"
                }
            }
        ]"#;

        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Renamed);
        assert_eq!(changes[0].entity.name, "new_name");
        assert!(changes[0].previous.is_some());
        assert_eq!(changes[0].previous.as_ref().unwrap().name, "old_name");
    }

    #[test]
    fn test_parse_empty_output() {
        let changes = parse_sem_diff_output("[]").unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_list_semantic_diffs_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = list_semantic_diffs(dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_nonexistent_diff() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = load_semantic_diff(dir.path(), "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_roundtrip_storage() {
        let dir = tempfile::TempDir::new().unwrap();
        let changes = vec![EntityChange {
            kind: ChangeKind::Added,
            entity: EntityInfo {
                name: "test_fn".to_string(),
                entity_type: "function".to_string(),
                file_path: "src/test.rs".to_string(),
                start_line: Some(1),
                end_line: Some(10),
                parent: None,
            },
            previous: None,
        }];

        let report = SemanticDiffReport::new(
            "test-session".to_string(),
            "aaa".to_string(),
            "bbb".to_string(),
            changes,
        );

        // Store
        let diff_dir = dir.path().join("semantic_diffs");
        std::fs::create_dir_all(&diff_dir).unwrap();
        let path = diff_dir.join("test-session.json");
        let json = serde_json::to_string_pretty(&report).unwrap();
        std::fs::write(&path, json).unwrap();

        // Load
        let loaded = load_semantic_diff(dir.path(), "test-session")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.session_id, "test-session");
        assert_eq!(loaded.changes.len(), 1);
        assert_eq!(loaded.changes[0].entity.name, "test_fn");
        assert_eq!(loaded.summary.added, 1);

        // List
        let ids = list_semantic_diffs(dir.path()).unwrap();
        assert_eq!(ids, vec!["test-session"]);
    }

    #[test]
    fn test_load_semantic_diff_path_traversal_sanitized() {
        let dir = tempfile::TempDir::new().unwrap();
        // A traversal ID like "../../etc/passwd" should be sanitized to a flat filename
        let result = load_semantic_diff(dir.path(), "../../etc/passwd").unwrap();
        assert!(result.is_none());

        // Verify the sanitized path stays inside semantic_diffs/
        let safe = crate::storage::sanitize_filename("../../etc/passwd");
        assert!(!safe.contains('/'));
        assert!(!safe.contains('\\'));
    }
}
