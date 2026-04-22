//! Pane registry + privileged-spawn contract.
//!
//! STATUS: SCAFFOLD (Loop 151). This models the registry that the Dioxus shell
//! will read. The actual PTY spawning lives in `impulse-term` and is bridged
//! through `impulse-supervisor::state::ShellState` once the runtime lands.

use crate::{PaneIdentity, PaneRoleRef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// In-memory registry of currently-live panes.
///
/// Invariant: at most one `Supervisor` pane. Enforced by `add()`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaneRegistry {
    panes: HashMap<uuid::Uuid, PaneIdentity>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("pane with id {0} already exists")]
    DuplicateId(uuid::Uuid),
    #[error("a supervisor pane already exists (id={0}); only one is allowed")]
    DuplicateSupervisor(uuid::Uuid),
    #[error("pane {0} not found in registry")]
    NotFound(uuid::Uuid),
}

impl PaneRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, pane: PaneIdentity) -> Result<(), RegistryError> {
        if self.panes.contains_key(&pane.id) {
            return Err(RegistryError::DuplicateId(pane.id));
        }
        if pane.role == PaneRoleRef::Supervisor {
            if let Some(existing) = self.supervisor() {
                return Err(RegistryError::DuplicateSupervisor(existing.id));
            }
        }
        self.panes.insert(pane.id, pane);
        Ok(())
    }

    pub fn remove(&mut self, id: uuid::Uuid) -> Result<PaneIdentity, RegistryError> {
        self.panes.remove(&id).ok_or(RegistryError::NotFound(id))
    }

    pub fn supervisor(&self) -> Option<&PaneIdentity> {
        self.panes.values().find(|p| p.role == PaneRoleRef::Supervisor)
    }

    pub fn workers(&self) -> impl Iterator<Item = &PaneIdentity> {
        self.panes.values().filter(|p| p.role == PaneRoleRef::Worker)
    }

    pub fn worker_count(&self) -> usize {
        self.workers().count()
    }

    pub fn get(&self, id: uuid::Uuid) -> Option<&PaneIdentity> {
        self.panes.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrono_ish::Timestamp;
    use std::path::PathBuf;

    fn make_pane(role: PaneRoleRef) -> PaneIdentity {
        PaneIdentity {
            id: uuid::Uuid::new_v4(),
            role,
            project: None,
            cwd: PathBuf::from("/tmp"),
            spawned_at: Timestamp::now(),
        }
    }

    #[test]
    fn test_empty_registry() {
        let r = PaneRegistry::new();
        assert_eq!(r.worker_count(), 0);
        assert!(r.supervisor().is_none());
    }

    #[test]
    fn test_add_worker() {
        let mut r = PaneRegistry::new();
        let pane = make_pane(PaneRoleRef::Worker);
        r.add(pane.clone()).unwrap();
        assert_eq!(r.worker_count(), 1);
        assert!(r.supervisor().is_none());
    }

    #[test]
    fn test_add_supervisor() {
        let mut r = PaneRegistry::new();
        let pane = make_pane(PaneRoleRef::Supervisor);
        r.add(pane.clone()).unwrap();
        assert!(r.supervisor().is_some());
        assert_eq!(r.supervisor().unwrap().id, pane.id);
        assert_eq!(r.worker_count(), 0);
    }

    #[test]
    fn test_duplicate_supervisor_rejected() {
        let mut r = PaneRegistry::new();
        r.add(make_pane(PaneRoleRef::Supervisor)).unwrap();
        let second = make_pane(PaneRoleRef::Supervisor);
        let err = r.add(second).unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateSupervisor(_)));
    }

    #[test]
    fn test_duplicate_id_rejected() {
        let mut r = PaneRegistry::new();
        let pane = make_pane(PaneRoleRef::Worker);
        r.add(pane.clone()).unwrap();
        let err = r.add(pane).unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateId(_)));
    }

    #[test]
    fn test_remove_not_found() {
        let mut r = PaneRegistry::new();
        let err = r.remove(uuid::Uuid::nil()).unwrap_err();
        assert!(matches!(err, RegistryError::NotFound(_)));
    }

    #[test]
    fn test_remove_returns_pane() {
        let mut r = PaneRegistry::new();
        let pane = make_pane(PaneRoleRef::Worker);
        r.add(pane.clone()).unwrap();
        let removed = r.remove(pane.id).unwrap();
        assert_eq!(removed.id, pane.id);
        assert_eq!(r.worker_count(), 0);
    }

    #[test]
    fn test_multiple_workers_one_supervisor() {
        let mut r = PaneRegistry::new();
        r.add(make_pane(PaneRoleRef::Supervisor)).unwrap();
        for _ in 0..5 {
            r.add(make_pane(PaneRoleRef::Worker)).unwrap();
        }
        assert_eq!(r.worker_count(), 5);
        assert!(r.supervisor().is_some());
    }

    #[test]
    fn test_error_display() {
        let err = RegistryError::DuplicateSupervisor(uuid::Uuid::nil());
        assert!(format!("{err}").contains("supervisor"));
        let err = RegistryError::NotFound(uuid::Uuid::nil());
        assert!(format!("{err}").contains("not found"));
    }
}
