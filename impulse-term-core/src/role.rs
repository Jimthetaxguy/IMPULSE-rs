//! PaneRole — first-principles rule #6: Supervisor is Privileged.
//!
//! Every pane spawned by Impulse has a role. The role determines what
//! environment variables, filesystem mounts, and IPC handles are exposed
//! to the child process.
//!
//! - `Worker` (default): ordinary agent pane. Receives `IMPULSE_PANE_ROLE=worker`
//!   and nothing else role-specific. `EnvGuard` sanitizes `CLAUDECODE`-style
//!   parent-agent env vars before spawn.
//!
//! - `Supervisor`: privileged orchestrator pane. Receives `IMPULSE_SUPERVISOR=1`,
//!   `IMPULSE_CMD_SOCKET=<path>` (when a socket path is provided), and
//!   `IMPULSE_PANE_ROLE=supervisor`. `EnvGuard` sanitization is SKIPPED so the
//!   supervisor can intentionally see the ambient Impulse env (impulse-skills
//!   mounts, daemon socket path, etc.).
//!
//! This is the foundation for the Dioxus supervisor terminal planned in Phase 8.

use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Role assigned to a pane at spawn time.
///
/// Role is immutable for the lifetime of a panel — a worker pane cannot be
/// promoted to a supervisor, and vice versa. If a role change is needed,
/// kill the pane and spawn a replacement.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaneRole {
    /// Normal worker pane (default). Standard PTY, no cross-pane visibility.
    #[default]
    Worker,
    /// Supervisor pane. Privileged: receives IMPULSE_CMD_SOCKET env var,
    /// impulse-skills/ mount, direct daemon socket handle, cross-pane visibility.
    Supervisor,
}

impl PaneRole {
    /// Env vars injected at PTY spawn time for this role.
    ///
    /// All panes that get a `socket_path` receive `IMPULSE_CMD_SOCKET=<path>`
    /// — workers need it to send `@impulse` commands; supervisor needs it to
    /// receive them. Workers also get `IMPULSE_WORKER_PANE_ID=<uuid>` when a
    /// `pane_id` is provided, so the daemon can identify which worker emitted
    /// a given command. Only `Supervisor` gets the additional
    /// `IMPULSE_SUPERVISOR=1` privilege flag.
    pub fn spawn_env_vars(
        &self,
        socket_path: Option<&Path>,
        pane_id: Option<Uuid>,
    ) -> Vec<(String, String)> {
        let mut vars = vec![("IMPULSE_PANE_ROLE".to_string(), self.as_str().to_string())];
        if let Some(path) = socket_path {
            vars.push(("IMPULSE_CMD_SOCKET".to_string(), path.display().to_string()));
        }
        match self {
            PaneRole::Supervisor => {
                if socket_path.is_some() {
                    vars.push(("IMPULSE_SUPERVISOR".to_string(), "1".to_string()));
                }
            }
            PaneRole::Worker => {
                if let Some(id) = pane_id {
                    vars.push(("IMPULSE_WORKER_PANE_ID".to_string(), id.to_string()));
                }
            }
        }
        vars
    }

    /// Short string form used in env vars and logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Supervisor => "supervisor",
        }
    }

    /// Whether `EnvGuard` should sanitize parent-agent env vars before spawn.
    ///
    /// Worker panes sanitize (they should not see their parent's CLAUDECODE
    /// state). Supervisor panes do NOT sanitize — they intentionally inherit
    /// the ambient Impulse env so they can orchestrate.
    pub fn should_sanitize_env(&self) -> bool {
        matches!(self, Self::Worker)
    }

    /// Whether this role has cross-pane visibility / daemon orchestration
    /// privileges. Matches `IMPULSE_SUPERVISOR=1` semantics.
    pub fn is_privileged(&self) -> bool {
        matches!(self, Self::Supervisor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_default_is_worker() {
        assert_eq!(PaneRole::default(), PaneRole::Worker);
    }

    #[test]
    fn test_worker_env_with_socket_includes_cmd_socket() {
        // Workers DO receive IMPULSE_CMD_SOCKET so they can send `@impulse`
        // commands to the daemon. They do NOT receive IMPULSE_SUPERVISOR.
        let vars = PaneRole::Worker.spawn_env_vars(Some(Path::new("/tmp/sock")), None);
        let get = |k: &str| vars.iter().find(|(vk, _)| vk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("IMPULSE_PANE_ROLE"), Some("worker"));
        assert_eq!(get("IMPULSE_CMD_SOCKET"), Some("/tmp/sock"));
        assert_eq!(get("IMPULSE_SUPERVISOR"), None);
    }

    #[test]
    fn test_worker_env_without_socket() {
        let vars = PaneRole::Worker.spawn_env_vars(None, None);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].0, "IMPULSE_PANE_ROLE");
        assert_eq!(vars[0].1, "worker");
    }

    #[test]
    fn test_supervisor_env_includes_socket() {
        // Supervisor with a socket path gets 3 vars: role + socket + supervisor flag.
        let path = PathBuf::from("/tmp/impulse.sock");
        let vars = PaneRole::Supervisor.spawn_env_vars(Some(&path), None);
        assert_eq!(vars.len(), 3);

        let get = |k: &str| vars.iter().find(|(vk, _)| vk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("IMPULSE_PANE_ROLE"), Some("supervisor"));
        assert_eq!(get("IMPULSE_CMD_SOCKET"), Some("/tmp/impulse.sock"));
        assert_eq!(get("IMPULSE_SUPERVISOR"), Some("1"));
    }

    #[test]
    fn test_supervisor_without_socket_path() {
        // Graceful — supervisor without socket only gets IMPULSE_PANE_ROLE.
        // This is valid: the supervisor can spawn before the socket is ready.
        let vars = PaneRole::Supervisor.spawn_env_vars(None, None);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].0, "IMPULSE_PANE_ROLE");
        assert_eq!(vars[0].1, "supervisor");
    }

    #[test]
    fn test_serde_round_trip_worker() {
        let original = PaneRole::Worker;
        let json = serde_json::to_string(&original).unwrap();
        let recovered: PaneRole = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_serde_round_trip_supervisor() {
        let original = PaneRole::Supervisor;
        let json = serde_json::to_string(&original).unwrap();
        let recovered: PaneRole = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_as_str_values() {
        assert_eq!(PaneRole::Worker.as_str(), "worker");
        assert_eq!(PaneRole::Supervisor.as_str(), "supervisor");
    }

    #[test]
    fn test_should_sanitize_env_matches_role() {
        assert!(PaneRole::Worker.should_sanitize_env());
        assert!(!PaneRole::Supervisor.should_sanitize_env());
    }

    #[test]
    fn test_is_privileged_matches_role() {
        assert!(!PaneRole::Worker.is_privileged());
        assert!(PaneRole::Supervisor.is_privileged());
    }

    #[test]
    fn test_supervisor_env_preserves_socket_path_display() {
        // Path with spaces / unicode should round-trip via Display.
        let path = PathBuf::from("/var/run/impulse-agent.sock");
        let vars = PaneRole::Supervisor.spawn_env_vars(Some(&path), None);
        let sock = vars
            .iter()
            .find(|(k, _)| k == "IMPULSE_CMD_SOCKET")
            .unwrap();
        assert_eq!(sock.1, "/var/run/impulse-agent.sock");
    }

    #[test]
    fn test_role_is_copy() {
        // PaneRole is Copy, so assignment does not move. Proves the derive.
        let r = PaneRole::Supervisor;
        let r2 = r;
        assert_eq!(r, r2);
    }

    #[test]
    fn test_worker_with_pane_id_emits_worker_pane_id_var() {
        let id = Uuid::new_v4();
        let vars = PaneRole::Worker.spawn_env_vars(Some(Path::new("/tmp/sock")), Some(id));
        let get = |k: &str| vars.iter().find(|(vk, _)| vk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("IMPULSE_PANE_ROLE"), Some("worker"));
        assert_eq!(get("IMPULSE_CMD_SOCKET"), Some("/tmp/sock"));
        assert_eq!(get("IMPULSE_WORKER_PANE_ID"), Some(id.to_string().as_str()));
        // Worker MUST NOT receive the privilege flag.
        assert_eq!(get("IMPULSE_SUPERVISOR"), None);
    }

    #[test]
    fn test_supervisor_does_not_receive_worker_pane_id() {
        let id = Uuid::new_v4();
        let vars = PaneRole::Supervisor.spawn_env_vars(Some(Path::new("/tmp/sock")), Some(id));
        let get = |k: &str| vars.iter().find(|(vk, _)| vk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("IMPULSE_SUPERVISOR"), Some("1"));
        assert_eq!(get("IMPULSE_WORKER_PANE_ID"), None);
    }

    #[test]
    fn test_worker_without_pane_id_omits_worker_pane_id_var() {
        let vars = PaneRole::Worker.spawn_env_vars(Some(Path::new("/tmp/sock")), None);
        assert!(vars.iter().all(|(k, _)| k != "IMPULSE_WORKER_PANE_ID"));
    }

    #[test]
    fn test_role_hash_eq_consistency() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(PaneRole::Worker);
        set.insert(PaneRole::Supervisor);
        set.insert(PaneRole::Worker); // duplicate
        assert_eq!(set.len(), 2);
    }
}
