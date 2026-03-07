//! Data types for semantic diff results.
//!
//! These mirror the JSON output of `sem diff --format json` and `sem blame --format json`,
//! plus Impulse-specific wrappers for storage and display.

use serde::{Deserialize, Serialize};

/// The kind of change detected for a code entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Moved,
    Renamed,
}

impl ChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Moved => "moved",
            Self::Renamed => "renamed",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Modified => "~",
            Self::Deleted => "-",
            Self::Moved => ">",
            Self::Renamed => "=",
        }
    }
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A code entity (function, struct, class, etc.) as identified by `sem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityInfo {
    /// Entity name (e.g. `validate_token`)
    pub name: String,
    /// Entity type (e.g. `function`, `struct`, `class`, `method`)
    pub entity_type: String,
    /// File path relative to repo root
    pub file_path: String,
    /// Start line in the file
    #[serde(default)]
    pub start_line: Option<u32>,
    /// End line in the file
    #[serde(default)]
    pub end_line: Option<u32>,
    /// Parent entity (for methods in classes, etc.)
    #[serde(default)]
    pub parent: Option<String>,
}

impl std::fmt::Display for EntityInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(parent) = &self.parent {
            write!(f, "{}::{} ({})", parent, self.name, self.entity_type)
        } else {
            write!(f, "{} ({})", self.name, self.entity_type)
        }
    }
}

/// A single entity-level change in the semantic diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChange {
    /// What kind of change
    pub kind: ChangeKind,
    /// The entity that changed
    pub entity: EntityInfo,
    /// Previous entity info (for renames/moves)
    #[serde(default)]
    pub previous: Option<EntityInfo>,
}

impl std::fmt::Display for EntityChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.previous {
            Some(prev) => write!(
                f,
                "[{}] {} -> {} in {}",
                self.kind.symbol(),
                prev,
                self.entity.name,
                self.entity.file_path
            ),
            None => write!(
                f,
                "[{}] {} in {}",
                self.kind.symbol(),
                self.entity,
                self.entity.file_path
            ),
        }
    }
}

/// Aggregated summary of semantic changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticDiffSummary {
    pub total_changes: usize,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub moved: usize,
    pub renamed: usize,
    pub files_affected: usize,
}

impl std::fmt::Display for SemanticDiffSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} changes across {} files (+{} ~{} -{} >{} ={})",
            self.total_changes,
            self.files_affected,
            self.added,
            self.modified,
            self.deleted,
            self.moved,
            self.renamed,
        )
    }
}

/// Complete semantic diff report for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDiffReport {
    /// Session ID this diff belongs to
    pub session_id: String,
    /// Git ref for the base (e.g. commit hash at session start)
    pub base_ref: String,
    /// Git ref for the head (e.g. HEAD at session end)
    pub head_ref: String,
    /// Timestamp of when this diff was computed
    pub timestamp: String,
    /// Individual entity changes
    pub changes: Vec<EntityChange>,
    /// Aggregated summary
    pub summary: SemanticDiffSummary,
}

impl SemanticDiffReport {
    /// Build a report from a list of changes.
    pub fn new(
        session_id: String,
        base_ref: String,
        head_ref: String,
        changes: Vec<EntityChange>,
    ) -> Self {
        let summary = Self::compute_summary(&changes);
        Self {
            session_id,
            base_ref,
            head_ref,
            timestamp: chrono::Utc::now().to_rfc3339(),
            changes,
            summary,
        }
    }

    fn compute_summary(changes: &[EntityChange]) -> SemanticDiffSummary {
        let mut summary = SemanticDiffSummary::default();
        let mut files = std::collections::HashSet::new();

        for change in changes {
            summary.total_changes += 1;
            files.insert(change.entity.file_path.clone());
            match change.kind {
                ChangeKind::Added => summary.added += 1,
                ChangeKind::Modified => summary.modified += 1,
                ChangeKind::Deleted => summary.deleted += 1,
                ChangeKind::Moved => summary.moved += 1,
                ChangeKind::Renamed => summary.renamed += 1,
            }
        }
        summary.files_affected = files.len();
        summary
    }

    /// Format as a human-readable summary block for injection.
    pub fn format_injection_block(&self) -> String {
        if self.changes.is_empty() {
            return "No semantic changes detected.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("## Semantic Changes ({})", self.summary));
        lines.push(String::new());

        // Group by file
        let mut by_file: std::collections::BTreeMap<&str, Vec<&EntityChange>> =
            std::collections::BTreeMap::new();
        for change in &self.changes {
            by_file
                .entry(&change.entity.file_path)
                .or_default()
                .push(change);
        }

        for (file, changes) in &by_file {
            lines.push(format!("**{}**", file));
            for change in changes {
                lines.push(format!("  {} {}", change.kind.symbol(), change.entity));
            }
        }

        lines.join("\n")
    }
}

/// Entity-level blame entry from `sem blame`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticBlameEntry {
    pub entity: EntityInfo,
    pub author: String,
    pub commit: String,
    pub date: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Impact analysis result from `sem impact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResult {
    /// The entity being analyzed
    pub target: EntityInfo,
    /// Entities that depend on / are affected by the target
    pub dependents: Vec<EntityInfo>,
    /// Total blast radius count
    pub blast_radius: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_kind_display() {
        assert_eq!(ChangeKind::Added.as_str(), "added");
        assert_eq!(ChangeKind::Modified.symbol(), "~");
        assert_eq!(ChangeKind::Deleted.symbol(), "-");
    }

    #[test]
    fn test_entity_info_display_no_parent() {
        let entity = EntityInfo {
            name: "validate_token".to_string(),
            entity_type: "function".to_string(),
            file_path: "src/auth.rs".to_string(),
            start_line: Some(10),
            end_line: Some(25),
            parent: None,
        };
        assert_eq!(entity.to_string(), "validate_token (function)");
    }

    #[test]
    fn test_entity_info_display_with_parent() {
        let entity = EntityInfo {
            name: "validate".to_string(),
            entity_type: "method".to_string(),
            file_path: "src/auth.rs".to_string(),
            start_line: None,
            end_line: None,
            parent: Some("TokenValidator".to_string()),
        };
        assert_eq!(entity.to_string(), "TokenValidator::validate (method)");
    }

    #[test]
    fn test_report_summary() {
        let changes = vec![
            EntityChange {
                kind: ChangeKind::Added,
                entity: EntityInfo {
                    name: "new_fn".to_string(),
                    entity_type: "function".to_string(),
                    file_path: "src/a.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    parent: None,
                },
                previous: None,
            },
            EntityChange {
                kind: ChangeKind::Modified,
                entity: EntityInfo {
                    name: "old_fn".to_string(),
                    entity_type: "function".to_string(),
                    file_path: "src/a.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    parent: None,
                },
                previous: None,
            },
            EntityChange {
                kind: ChangeKind::Deleted,
                entity: EntityInfo {
                    name: "gone_fn".to_string(),
                    entity_type: "function".to_string(),
                    file_path: "src/b.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    parent: None,
                },
                previous: None,
            },
        ];

        let report = SemanticDiffReport::new(
            "test-session".to_string(),
            "abc123".to_string(),
            "def456".to_string(),
            changes,
        );

        assert_eq!(report.summary.total_changes, 3);
        assert_eq!(report.summary.added, 1);
        assert_eq!(report.summary.modified, 1);
        assert_eq!(report.summary.deleted, 1);
        assert_eq!(report.summary.files_affected, 2);
    }

    #[test]
    fn test_empty_report_injection_block() {
        let report =
            SemanticDiffReport::new("s1".to_string(), "a".to_string(), "b".to_string(), vec![]);
        assert_eq!(
            report.format_injection_block(),
            "No semantic changes detected."
        );
    }

    #[test]
    fn test_injection_block_groups_by_file() {
        let changes = vec![
            EntityChange {
                kind: ChangeKind::Added,
                entity: EntityInfo {
                    name: "foo".to_string(),
                    entity_type: "function".to_string(),
                    file_path: "src/lib.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    parent: None,
                },
                previous: None,
            },
            EntityChange {
                kind: ChangeKind::Modified,
                entity: EntityInfo {
                    name: "bar".to_string(),
                    entity_type: "function".to_string(),
                    file_path: "src/lib.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    parent: None,
                },
                previous: None,
            },
        ];

        let report =
            SemanticDiffReport::new("s1".to_string(), "a".to_string(), "b".to_string(), changes);
        let block = report.format_injection_block();
        assert!(block.contains("**src/lib.rs**"));
        assert!(block.contains("+ foo (function)"));
        assert!(block.contains("~ bar (function)"));
    }
}
