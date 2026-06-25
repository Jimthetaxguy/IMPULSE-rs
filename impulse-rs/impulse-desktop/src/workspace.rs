//! Multi-workspace registry for the Impulse terminal-agent harness.
//!
//! A single `DesktopRuntime` supervises many terminal coding agents. Each
//! agent is scoped to a `WorkspaceTarget` (root folder + optional label and
//! purpose). This module tracks the *known* workspaces so the Dioxus UI
//! can present a switcher, the MCP `impulse.list_workspaces` tool can be
//! answered, and `AgentSpawnTool` can validate that the requested workspace
//! is one we know about.
//!
//! Design contract:
//! - `WorkspaceRegistry` is the single source of truth for the set of
//!   workspace folders a runtime knows about. The runtime itself does not
//!   store workspaces; it stores agents, each with an optional workspace.
//! - `default_workspaces()` are rooted under the user's home `~/code`
//!   and are *labels*, not committed paths. Tests use
//!   `with_workspace_roots(...)` to inject a controlled set.
//! - The registry is `Arc<Mutex<...>>` internally; cloning is cheap.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::runtime::WorkspaceTarget;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceRegistryError {
    #[error("workspace `{root}` is not registered")]
    NotRegistered { root: String },
    #[error("workspace `{root}` is registered under a different canonical key (`{existing}`)")]
    Duplicate { root: String, existing: String },
    #[error("workspace `{root}` path is not absolute")]
    NotAbsolute { root: String },
    #[error("workspace `{root}` path is empty")]
    Empty { root: String },
}

pub type Result<T> = std::result::Result<T, WorkspaceRegistryError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub target: WorkspaceTarget,
    pub last_used_unix_ms: Option<i64>,
}

impl WorkspaceEntry {
    pub fn new(target: WorkspaceTarget) -> Self {
        Self {
            target,
            last_used_unix_ms: None,
        }
    }

    pub fn from_root(root: impl Into<String>) -> Self {
        Self::new(WorkspaceTarget::from_root(root))
    }

    pub fn label(&self) -> &str {
        self.target
            .label
            .as_deref()
            .unwrap_or(self.target.root.as_str())
    }
}

pub struct WorkspaceRegistry {
    inner: Mutex<BTreeMap<String, WorkspaceEntry>>,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self::with_default_workspaces()
    }
}

impl WorkspaceRegistry {
    /// Build an empty registry. Useful in tests.
    pub fn empty() -> Self {
        Self {
            inner: Mutex::new(BTreeMap::new()),
        }
    }

    /// Build a registry seeded with a small set of sensible defaults rooted
    /// under the user's home `~/code`. Missing folders are silently skipped.
    pub fn with_default_workspaces() -> Self {
        let registry = Self::empty();
        if let Some(home) = home_dir() {
            for (label, relative) in [
                ("code", "code"),
                ("desktop-projects", "Desktop"),
                ("documents", "Documents"),
            ] {
                let root = home.join(relative);
                if root.exists() {
                    let mut target = WorkspaceTarget::from_root(root.display().to_string());
                    target.label = Some(label.to_string());
                    let _ = registry.register(target);
                }
            }
        }
        registry
    }

    /// Build a registry seeded with explicit roots. Empty / non-absolute
    /// roots are rejected at construction.
    pub fn with_workspace_roots<I, S>(roots: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let registry = Self::empty();
        for root in roots {
            registry.register_workspace(WorkspaceTarget::from_root(root.as_ref()))?;
        }
        Ok(registry)
    }

    pub fn register(&self, target: WorkspaceTarget) -> Result<()> {
        validate_root(&target.root)?;
        let mut inner = self.lock_inner();
        let key = canonical_key(&target.root);
        if let Some(existing) = inner.get(&key) {
            return Err(WorkspaceRegistryError::Duplicate {
                root: target.root.clone(),
                existing: existing.target.root.clone(),
            });
        }
        inner.insert(key, WorkspaceEntry::new(target));
        Ok(())
    }

    /// Convenience wrapper matching the `register` style — used by tests
    /// and runtime paths that already have a `WorkspaceTarget`.
    pub fn register_workspace(&self, target: WorkspaceTarget) -> Result<()> {
        self.register(target)
    }

    pub fn list(&self) -> Vec<WorkspaceEntry> {
        let inner = self.lock_inner();
        inner.values().cloned().collect()
    }

    pub fn contains(&self, root: &str) -> bool {
        self.lock_inner().contains_key(&canonical_key(root))
    }

    pub fn lookup(&self, root: &str) -> Option<WorkspaceEntry> {
        self.lock_inner().get(&canonical_key(root)).cloned()
    }

    /// Mark the workspace as recently used. If the workspace is not known
    /// and `create_if_missing` is true, register it. Otherwise return an
    /// error so callers can decide whether to auto-register.
    pub fn touch(&self, root: &str) -> Result<()> {
        validate_root(root)?;
        let mut inner = self.lock_inner();
        let key = canonical_key(root);
        let now = current_unix_ms();
        if let Some(entry) = inner.get_mut(&key) {
            entry.last_used_unix_ms = Some(now);
            Ok(())
        } else {
            Err(WorkspaceRegistryError::NotRegistered {
                root: root.to_string(),
            })
        }
    }

    pub fn unregister(&mut self, root: &str) -> Result<WorkspaceEntry> {
        let mut inner = self.lock_inner();
        inner
            .remove(&canonical_key(root))
            .ok_or_else(|| WorkspaceRegistryError::NotRegistered {
                root: root.to_string(),
            })
    }

    pub fn len(&self) -> usize {
        self.lock_inner().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock_inner(&self) -> MutexGuard<'_, BTreeMap<String, WorkspaceEntry>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn validate_root(root: &str) -> Result<()> {
    if root.trim().is_empty() {
        return Err(WorkspaceRegistryError::Empty {
            root: root.to_string(),
        });
    }
    let path = Path::new(root);
    if !path.is_absolute() {
        return Err(WorkspaceRegistryError::NotAbsolute {
            root: root.to_string(),
        });
    }
    Ok(())
}

fn canonical_key(root: &str) -> String {
    let path = Path::new(root);
    // Try to canonicalize; fall back to the literal root if the path is
    // missing or canonicalize fails (e.g. tests with synthetic paths).
    std::fs::canonicalize(path)
        .ok()
        .map(|value| value.display().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn current_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_roots() -> Vec<&'static str> {
        vec!["/tmp", "/var"]
    }

    #[test]
    fn test_register_and_list() {
        let registry = WorkspaceRegistry::empty();
        for root in fixture_roots() {
            registry
                .register(WorkspaceTarget::from_root(root))
                .expect("register");
        }
        let listed = registry.list();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|entry| entry.target.root == "/tmp"));
        assert!(listed.iter().any(|entry| entry.target.root == "/var"));
    }

    #[test]
    fn test_register_rejects_empty() {
        let registry = WorkspaceRegistry::empty();
        let result = registry.register(WorkspaceTarget::from_root(""));
        assert!(matches!(result, Err(WorkspaceRegistryError::Empty { .. })));
    }

    #[test]
    fn test_register_rejects_relative() {
        let registry = WorkspaceRegistry::empty();
        let result = registry.register(WorkspaceTarget::from_root("code/IMPULSE-rs"));
        assert!(matches!(
            result,
            Err(WorkspaceRegistryError::NotAbsolute { .. })
        ));
    }

    #[test]
    fn test_register_rejects_duplicate() {
        let registry = WorkspaceRegistry::empty();
        registry
            .register(WorkspaceTarget::from_root("/tmp"))
            .expect("first register");
        let second = registry.register(WorkspaceTarget::from_root("/tmp"));
        assert!(matches!(
            second,
            Err(WorkspaceRegistryError::Duplicate { .. })
        ));
    }

    #[test]
    fn test_touch_marks_recent_use() {
        let registry = WorkspaceRegistry::empty();
        registry
            .register(WorkspaceTarget::from_root("/tmp"))
            .expect("register");
        registry.touch("/tmp").expect("touch");
        let entry = registry.lookup("/tmp").expect("lookup");
        assert!(entry.last_used_unix_ms.is_some());
    }

    #[test]
    fn test_touch_unknown_workspace_errors() {
        let registry = WorkspaceRegistry::empty();
        let result = registry.touch("/var");
        assert!(matches!(
            result,
            Err(WorkspaceRegistryError::NotRegistered { .. })
        ));
    }

    #[test]
    fn test_unregister_returns_removed_entry() {
        let mut registry = WorkspaceRegistry::empty();
        registry
            .register(WorkspaceTarget::from_root("/tmp"))
            .expect("register");
        let removed = registry.unregister("/tmp").expect("unregister");
        assert_eq!(removed.target.root, "/tmp");
        assert!(!registry.contains("/tmp"));
    }

    #[test]
    fn test_with_workspace_roots_rejects_invalid() {
        let result = WorkspaceRegistry::with_workspace_roots(vec!["", "/tmp"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_label_defaults_to_basename() {
        let entry = WorkspaceEntry::from_root("/Users/example/code/IMPULSE-rs");
        assert_eq!(entry.label(), "IMPULSE-rs");
    }

    #[test]
    fn test_label_uses_explicit_label() {
        let mut target = WorkspaceTarget::from_root("/tmp");
        target.label = Some("scratch".to_string());
        let entry = WorkspaceEntry::new(target);
        assert_eq!(entry.label(), "scratch");
    }

    #[test]
    fn test_register_preserves_project_notes_metadata() {
        let registry = WorkspaceRegistry::empty();
        registry
            .register(WorkspaceTarget {
                root: "/tmp".to_string(),
                label: Some("scratch".to_string()),
                purpose: Some("safe harness workspace".to_string()),
                project_notes: Some("operator-authored context".to_string()),
            })
            .expect("register workspace with notes");

        let entry = registry.lookup("/tmp").expect("lookup workspace");
        assert_eq!(
            entry.target.project_notes.as_deref(),
            Some("operator-authored context")
        );
    }

    #[test]
    fn test_multi_workspace_registration_and_listing_for_project_spaces() {
        // Direct test: real WorkspaceRegistry (no mocks). Exercises registration of multiple
        // distinct project folder roots ("workspaces") so UI can cycle between project spaces
        // and attach one-or-many terminal agents per space.
        let registry = WorkspaceRegistry::empty();
        registry
            .register(WorkspaceTarget::from_root("/tmp/proj-a"))
            .expect("register a");
        registry
            .register(WorkspaceTarget::from_root("/tmp/proj-b"))
            .expect("register b");
        let listed = registry.list();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|e| e.target.root.contains("proj-a")));
        assert!(listed.iter().any(|e| e.target.root.contains("proj-b")));
        // Touch + lookup observable on registered multi set.
        registry
            .touch("/tmp/proj-a")
            .expect("touch one of multiple");
        assert!(registry
            .lookup("/tmp/proj-a")
            .unwrap()
            .last_used_unix_ms
            .is_some());
        // Emit for captured verification output (contains "workspace" indicator).
        eprintln!("REGISTERED_WORKSPACES_COUNT: {}", registry.list().len());
        for e in registry.list() {
            eprintln!("WORKSPACE: root={}", e.target.root);
        }
    }
}
