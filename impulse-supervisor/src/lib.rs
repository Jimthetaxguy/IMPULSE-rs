//! # impulse-supervisor
//!
//! Dioxus desktop shell for the Impulse privileged supervisor terminal.
//!
//! ## Status: SCAFFOLD (Phase 8, Loop 151)
//!
//! This crate is the pure-Rust replacement for the retiring egui `impulse-gui` shell.
//! It implements the "desktop orchestrator" architecture from the Grok first-principles
//! analysis: one Dioxus window hosting a permanent supervisor terminal in the left
//! sidebar and multiple ordinary worker terminals in the main area.
//!
//! ## First-Principles Contract
//!
//! Enforced by this crate (see `.opencode/ralph-loop-state/cleanup/DECISIONS.md`):
//!
//! 1. **Ownership at Birth** — every pane receives a typed `PaneRole` + `Uuid` at spawn
//! 2. **View ≠ State** — visible scrollback may lag the durable `.impulse/` log
//! 3. **One Language** — 100% Rust; no webview, no TypeScript
//! 4. **Daemon as Library** — daemon lives for the window lifetime, not forever
//! 5. **Context Replacement** — compaction replaces the worker's context file and
//!    restarts; no in-memory mutation of a running agent
//! 6. **Supervisor is Privileged** — the supervisor pane gets `IMPULSE_CMD_SOCKET`,
//!    `IMPULSE_SUPERVISOR=1`, access to impulse-skills/, cross-pane visibility
//!
//! ## Not Yet Wired
//!
//! The runtime (`fn main`) is gated behind the `experimental-runtime` feature so the
//! workspace builds cleanly while the prototype is still in scaffold. When the wire
//! format (Loop 117+) and compaction flow (Loop 122+) land, the `launch()` function
//! below will become callable.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::all)]

pub mod layout;
pub mod panes;
pub mod state;

use serde::{Deserialize, Serialize};

/// Metadata attached to every pane at spawn time. Never inferred from strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneIdentity {
    pub id: uuid::Uuid,
    pub role: PaneRoleRef,
    pub project: Option<String>,
    pub cwd: std::path::PathBuf,
    pub spawned_at: chrono_ish::Timestamp,
}

/// Thin reference to the `PaneRole` enum living in `impulse-term::role`.
///
/// This crate re-exports it by value so UI code can match on role without depending
/// on `impulse-term` types leaking into component signatures.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PaneRoleRef {
    #[default]
    Worker,
    Supervisor,
}

impl PaneRoleRef {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Supervisor => "supervisor",
        }
    }

    pub fn is_privileged(&self) -> bool {
        matches!(self, Self::Supervisor)
    }
}

/// Sidebar/main split: the supervisor is always pinned to the left sidebar.
pub const SUPERVISOR_SIDEBAR_WIDTH_PX: u32 = 420;
pub const MIN_WORKER_PANE_WIDTH_PX: u32 = 320;

/// Single-purpose placeholder until Phase 8 runtime lands.
pub fn scaffold_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// Local ad-hoc timestamp type so we don't pull `chrono` in yet.
pub(crate) mod chrono_ish {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Timestamp(pub u64);

    impl Timestamp {
        pub fn now() -> Self {
            Self(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaffold_version_is_non_empty() {
        assert!(!scaffold_version().is_empty());
    }

    #[test]
    fn test_default_role_is_worker() {
        assert_eq!(PaneRoleRef::default(), PaneRoleRef::Worker);
    }

    #[test]
    fn test_worker_is_not_privileged() {
        assert!(!PaneRoleRef::Worker.is_privileged());
    }

    #[test]
    fn test_supervisor_is_privileged() {
        assert!(PaneRoleRef::Supervisor.is_privileged());
    }

    #[test]
    fn test_role_serde_roundtrip() {
        for role in [PaneRoleRef::Worker, PaneRoleRef::Supervisor] {
            let json = serde_json::to_string(&role).unwrap();
            let recovered: PaneRoleRef = serde_json::from_str(&json).unwrap();
            assert_eq!(role, recovered);
        }
    }

    #[test]
    fn test_pane_identity_roundtrip() {
        let id = PaneIdentity {
            id: uuid::Uuid::nil(),
            role: PaneRoleRef::Supervisor,
            project: Some("test-project".into()),
            cwd: std::path::PathBuf::from("/tmp"),
            spawned_at: chrono_ish::Timestamp::now(),
        };
        let json = serde_json::to_string(&id).unwrap();
        let recovered: PaneIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn test_layout_constants_are_sane() {
        assert!(SUPERVISOR_SIDEBAR_WIDTH_PX >= 300);
        assert!(MIN_WORKER_PANE_WIDTH_PX >= 200);
    }

    #[test]
    fn test_role_as_str_stable() {
        assert_eq!(PaneRoleRef::Worker.as_str(), "worker");
        assert_eq!(PaneRoleRef::Supervisor.as_str(), "supervisor");
    }
}
