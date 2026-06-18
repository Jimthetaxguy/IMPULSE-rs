//! `impulse-workspace` — workspace registry: per-folder project roots that
//! Impulse-RS sessions can attach to.
//!
//! Like git worktrees, but for "what folder is the agent working in right now".
//! The registry stores a [`WorkspaceHandle`] per registered path, lets callers
//! look them up by id or path, and tracks the most recently used timestamp so
//! the orchestrator can surface the active workspace to MCP and the Dioxus
//! host.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use impulse_contracts::workspace::workspace_label_from_path;
use impulse_contracts::{WorkspaceHandle, WorkspaceId, WorkspacePath};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, instrument};

/// A registered workspace plus a small amount of ephemeral state the
/// orchestrator tracks (currently just the recent pane count).
#[derive(Clone, Debug)]
pub struct WorkspaceEntry {
    /// The stable handle for the workspace.
    pub handle: WorkspaceHandle,
    /// Number of panes that have recently used this workspace.
    pub recent_pane_count: u32,
}

/// Errors returned by [`WorkspaceRegistry`].
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// The path is empty or otherwise malformed.
    #[error("workspace path {path:?} is empty or invalid: {reason}")]
    InvalidPath { path: String, reason: String },
    /// The path is relative; absolute paths are required.
    #[error("workspace path {path:?} is relative; must be absolute")]
    RelativePath { path: String },
    /// The canonical path is already registered.
    #[error("workspace {path:?} is already registered with id {existing}")]
    AlreadyRegistered { path: String, existing: WorkspaceId },
    /// The given workspace id is not in the registry.
    #[error("workspace id {0:?} not found")]
    NotFound(WorkspaceId),
    /// Underlying filesystem error during registration or `touch`.
    #[error("io error touching {path:?}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Internal state: a by-id map and a by-path index, both guarded by one lock.
#[derive(Default)]
struct State {
    by_id: HashMap<WorkspaceId, WorkspaceEntry>,
    by_path: HashMap<PathBuf, WorkspaceId>,
}

/// Thread-safe registry of workspace roots.
///
/// `WorkspaceRegistry` is `Clone + Send + Sync`; cloning yields another handle
/// to the same underlying state. All public methods take `&self`, so a single
/// instance can be shared across the orchestrator, MCP server, and Dioxus host.
pub struct WorkspaceRegistry {
    state: Arc<RwLock<State>>,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for WorkspaceRegistry {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl WorkspaceRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(State::default())),
        }
    }

    /// Construct a registry pre-populated with the given workspace roots.
    ///
    /// Roots are registered in order. The first invalid root aborts the
    /// construction; the error is returned and any roots after the failing
    /// one are not touched. Roots registered before the failure remain in
    /// the partial state — the function does not return the registry on
    /// error, so this is observable only via the constructor's result.
    ///
    /// # Errors
    /// Returns any [`WorkspaceError`] that [`Self::register`] would return
    /// for the first invalid root.
    pub fn with_workspace_roots<I, P>(roots: I) -> Result<Self, WorkspaceError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let reg = Self::new();
        for root in roots {
            reg.register(root.as_ref())?;
        }
        Ok(reg)
    }

    /// Register a workspace at the given path. The label defaults to the
    /// path's basename.
    ///
    /// # Errors
    /// See [`Self::register_with_label`].
    #[instrument(skip(self), fields(path = %path.display()))]
    pub fn register(&self, path: &Path) -> Result<WorkspaceId, WorkspaceError> {
        self.register_inner(path, None)
    }

    /// Register a workspace at the given path with an explicit label.
    ///
    /// # Errors
    /// - [`WorkspaceError::InvalidPath`] when the path is empty or cannot
    ///   be canonicalized.
    /// - [`WorkspaceError::RelativePath`] when the path is not absolute.
    /// - [`WorkspaceError::Io`] when the path is missing or is a file.
    /// - [`WorkspaceError::AlreadyRegistered`] when the canonical path is
    ///   already in the registry.
    #[instrument(skip(self), fields(path = %path.display(), label = %label))]
    pub fn register_with_label(
        &self,
        path: &Path,
        label: String,
    ) -> Result<WorkspaceId, WorkspaceError> {
        self.register_inner(path, Some(label))
    }

    fn register_inner(
        &self,
        path: &Path,
        explicit_label: Option<String>,
    ) -> Result<WorkspaceId, WorkspaceError> {
        // 1. Non-empty.
        if path.as_os_str().is_empty() {
            return Err(WorkspaceError::InvalidPath {
                path: path.display().to_string(),
                reason: "path is empty".to_owned(),
            });
        }

        // 2. Absolute.
        if !path.is_absolute() {
            return Err(WorkspaceError::RelativePath {
                path: path.display().to_string(),
            });
        }

        // 3. Exists as a directory (not a file, not missing).
        let meta = std::fs::metadata(path).map_err(|source| WorkspaceError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if !meta.is_dir() {
            return Err(WorkspaceError::Io {
                path: path.display().to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    format!("path is not a directory: {}", path.display()),
                ),
            });
        }

        // 4. Canonicalize. A real canonical path is always absolute on
        //    Unix and Windows, so a failure here means something exotic
        //    (permissions, race with deletion, …).
        let canonical =
            std::fs::canonicalize(path).map_err(|source| WorkspaceError::InvalidPath {
                path: path.display().to_string(),
                reason: format!("cannot canonicalize: {source}"),
            })?;

        // 5. Defensive validation — `WorkspacePath::new` requires absolute.
        let workspace_path =
            WorkspacePath::new(canonical.clone()).map_err(|e| WorkspaceError::InvalidPath {
                path: canonical.display().to_string(),
                reason: e.to_string(),
            })?;

        // 6. Resolve the label: explicit wins; otherwise default to the
        //    path's basename; an empty basename falls back to `None`.
        let resolved_label = explicit_label.or_else(|| {
            let derived = workspace_label_from_path(workspace_path.as_path());
            if derived.is_empty() {
                None
            } else {
                Some(derived)
            }
        });

        // 7. Lock and insert.
        let mut state = self.state.write();
        if let Some(existing) = state.by_path.get(&canonical) {
            return Err(WorkspaceError::AlreadyRegistered {
                path: canonical.display().to_string(),
                existing: *existing,
            });
        }

        let id = WorkspaceId::new();
        let now: DateTime<Utc> = Utc::now();
        let handle = WorkspaceHandle {
            id,
            path: workspace_path,
            label: resolved_label,
            registered_at: now,
            last_used_at: None,
        };
        let entry = WorkspaceEntry {
            handle,
            recent_pane_count: 0,
        };
        state.by_id.insert(id, entry);
        state.by_path.insert(canonical, id);
        debug!(workspace_id = %id, "registered workspace");
        Ok(id)
    }

    /// Remove a workspace from the registry. Returns the prior entry if it
    /// was present, or `None` if the id was unknown.
    #[must_use]
    pub fn unregister(&self, id: WorkspaceId) -> Option<WorkspaceEntry> {
        let mut state = self.state.write();
        let entry = state.by_id.remove(&id)?;
        let path = entry.handle.path.as_path().to_path_buf();
        state.by_path.remove(&path);
        debug!(workspace_id = %id, path = %path.display(), "unregistered workspace");
        Some(entry)
    }

    /// Mark a workspace as recently used (sets `last_used_at = now`).
    ///
    /// # Errors
    /// Returns [`WorkspaceError::NotFound`] when the id is not registered.
    pub fn touch(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let mut state = self.state.write();
        let entry = state
            .by_id
            .get_mut(&id)
            .ok_or(WorkspaceError::NotFound(id))?;
        entry.handle.last_used_at = Some(Utc::now());
        debug!(workspace_id = %id, "touched workspace");
        Ok(())
    }

    /// Look up a workspace by id.
    #[must_use]
    pub fn get(&self, id: WorkspaceId) -> Option<WorkspaceEntry> {
        self.state.read().by_id.get(&id).cloned()
    }

    /// Snapshot all registered workspaces.
    #[must_use]
    pub fn list(&self) -> Vec<WorkspaceEntry> {
        self.state.read().by_id.values().cloned().collect()
    }

    /// Find a workspace by path. Accepts a non-canonical form (relative
    /// segments, `./` noise, symlinks) and canonicalizes the query before
    /// lookup. Returns `None` when the path is empty, cannot be
    /// canonicalized, or is not registered.
    #[must_use]
    pub fn find_by_path(&self, path: &Path) -> Option<WorkspaceEntry> {
        if path.as_os_str().is_empty() {
            return None;
        }
        let canonical = std::fs::canonicalize(path).ok()?;
        let state = self.state.read();
        let id = state.by_path.get(&canonical)?;
        state.by_id.get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn test_register_and_list() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id = registry.register(dir.path()).expect("register");
        let entries = registry.list();
        assert_eq!(entries.len(), 1);
        assert!(
            entries.iter().any(|e| e.handle.id == id),
            "registered id should appear in list"
        );
    }

    #[test]
    fn test_register_rejects_relative() {
        let registry = WorkspaceRegistry::new();
        let result = registry.register(Path::new("relative/path"));
        match result {
            Err(WorkspaceError::RelativePath { .. }) => {}
            other => panic!("expected RelativePath, got {other:?}"),
        }
    }

    #[test]
    fn test_register_rejects_empty() {
        let registry = WorkspaceRegistry::new();
        let result = registry.register(Path::new(""));
        match result {
            Err(WorkspaceError::InvalidPath { .. }) => {}
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn test_register_rejects_nonexistent() {
        let registry = WorkspaceRegistry::new();
        let result = registry.register(Path::new("/impulse_nonexistent_workspace_xyz_12345"));
        match result {
            Err(WorkspaceError::Io { .. }) => {}
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn test_register_rejects_file() {
        let registry = WorkspaceRegistry::new();
        let file = tempfile::NamedTempFile::new().expect("tempfile");
        let result = registry.register(file.path());
        match result {
            Err(WorkspaceError::Io { .. }) | Err(WorkspaceError::InvalidPath { .. }) => {}
            other => panic!("expected Io or InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn test_register_rejects_duplicate() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id1 = registry.register(dir.path()).expect("first register");
        let result = registry.register(dir.path());
        match result {
            Err(WorkspaceError::AlreadyRegistered { existing, .. }) => {
                assert_eq!(existing, id1, "duplicate error should name the existing id");
            }
            other => panic!("expected AlreadyRegistered, got {other:?}"),
        }
    }

    #[test]
    fn test_unregister_returns_removed_entry() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id = registry.register(dir.path()).expect("register");
        let removed = registry.unregister(id);
        let entry = removed.expect("unregister should return the prior entry");
        assert_eq!(entry.handle.id, id);
        assert_eq!(registry.list().len(), 0);
    }

    #[test]
    fn test_unregister_unknown_returns_none() {
        let registry = WorkspaceRegistry::new();
        let result = registry.unregister(WorkspaceId::new());
        assert!(result.is_none());
    }

    #[test]
    fn test_touch_marks_recent_use() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id = registry.register(dir.path()).expect("register");
        let before = Utc::now();
        registry.touch(id).expect("touch");
        let after = Utc::now();
        let entry = registry.get(id).expect("get");
        let touched = entry
            .handle
            .last_used_at
            .expect("last_used_at should be set after touch");
        assert!(
            touched >= before,
            "touched {touched} should be >= before {before}"
        );
        assert!(
            touched <= after,
            "touched {touched} should be <= after {after}"
        );
    }

    #[test]
    fn test_touch_unknown_workspace_errors() {
        let registry = WorkspaceRegistry::new();
        let result = registry.touch(WorkspaceId::new());
        match result {
            Err(WorkspaceError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_label_defaults_to_basename() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id = registry.register(dir.path()).expect("register");
        let entry = registry.get(id).expect("get");
        let expected = dir
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .expect("basename");
        assert_eq!(entry.handle.label.as_deref(), Some(expected));
    }

    #[test]
    fn test_label_uses_explicit_label() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id = registry
            .register_with_label(dir.path(), "My Custom Label".to_owned())
            .expect("register");
        let entry = registry.get(id).expect("get");
        assert_eq!(entry.handle.label.as_deref(), Some("My Custom Label"));
    }

    #[test]
    fn test_with_workspace_roots_rejects_invalid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = WorkspaceRegistry::with_workspace_roots([
            dir.path(),
            Path::new("/impulse_nonexistent_workspace_xyz_12345"),
        ]);
        assert!(result.is_err(), "expected error from invalid root");
    }

    #[test]
    fn test_find_by_path() {
        let registry = WorkspaceRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let id = registry.register(dir.path()).expect("register");

        // Non-canonical form: append a "./" segment.
        let mut non_canonical = dir.path().to_path_buf();
        non_canonical.push(".");
        let entry = registry
            .find_by_path(&non_canonical)
            .expect("expected to find workspace via non-canonical path");
        assert_eq!(entry.handle.id, id);

        // Exact path also resolves.
        let entry = registry
            .find_by_path(dir.path())
            .expect("expected to find workspace via exact path");
        assert_eq!(entry.handle.id, id);

        // Unknown path returns None.
        let entry = registry.find_by_path(Path::new("/impulse_nonexistent_xyz_99999"));
        assert!(entry.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_registers_are_thread_safe() {
        let registry = WorkspaceRegistry::new();
        let mut handles = Vec::new();
        for _ in 0..4 {
            let reg = registry.clone();
            handles.push(tokio::task::spawn(async move {
                let dir = tempfile::tempdir().expect("tempdir");
                reg.register(dir.path())
            }));
        }
        let mut ids = Vec::new();
        for h in handles {
            let id = h.await.expect("join").expect("register");
            ids.push(id);
        }
        assert_eq!(registry.list().len(), 4);
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 4, "all 4 ids should be distinct");
    }
}
