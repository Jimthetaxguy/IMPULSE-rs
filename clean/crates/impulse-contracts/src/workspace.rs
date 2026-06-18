//! Workspace handle types — the orchestrator points at one of these per session.

use crate::id::WorkspaceId;
use crate::id::WorkspacePath;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A registered workspace the orchestrator can attach sessions to.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceHandle {
    /// Stable id.
    pub id: WorkspaceId,
    /// Canonical absolute path.
    pub path: WorkspacePath,
    /// Optional human label (defaults to directory basename).
    #[serde(default)]
    pub label: Option<String>,
    /// When the workspace was first registered.
    pub registered_at: DateTime<Utc>,
    /// When the workspace was last used by any session.
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Summary view of a workspace for list endpoints.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceSummary {
    /// Stable id.
    pub id: WorkspaceId,
    /// Display label (basename of path if no explicit label).
    pub label: String,
    /// Absolute path as a string for display.
    pub path_display: String,
    /// When the workspace was last used.
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<&WorkspaceHandle> for WorkspaceSummary {
    fn from(h: &WorkspaceHandle) -> Self {
        let label = h
            .label
            .clone()
            .unwrap_or_else(|| workspace_label_from_path(h.path.as_path()));
        Self {
            id: h.id,
            label,
            path_display: h.path.to_string(),
            last_used_at: h.last_used_at,
        }
    }
}

/// Derive a default label from a workspace path's basename.
#[must_use]
pub fn workspace_label_from_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn label_defaults_to_basename() {
        let h = WorkspaceHandle {
            id: WorkspaceId::new(),
            path: WorkspacePath::new_unchecked(PathBuf::from("/tmp/impulse-foo")),
            label: None,
            registered_at: Utc::now(),
            last_used_at: None,
        };
        let s: WorkspaceSummary = (&h).into();
        assert_eq!(s.label, "impulse-foo");
    }

    #[test]
    fn label_uses_explicit_label_when_provided() {
        let h = WorkspaceHandle {
            id: WorkspaceId::new(),
            path: WorkspacePath::new_unchecked(PathBuf::from("/tmp/impulse-foo")),
            label: Some("Custom Name".to_owned()),
            registered_at: Utc::now(),
            last_used_at: None,
        };
        let s: WorkspaceSummary = (&h).into();
        assert_eq!(s.label, "Custom Name");
    }
}
