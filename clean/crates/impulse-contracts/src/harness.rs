//! Backend harness descriptors — what coding agents Impulse-RS can drive.

use crate::error::ContractsResult;
use crate::id::SessionId;
use crate::tool::{RiskClass, ToolSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// Identifier for the platform a coding agent belongs to.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentPlatformKind {
    /// Anthropic's Claude Code CLI.
    ClaudeCode,
    /// OpenAI's Codex CLI.
    Codex,
    /// Google's Gemini CLI.
    GeminiCli,
    /// Legacy OpenCode CLI (compatibility only).
    OpenCode,
    /// Generic CLI subprocess; the orchestrator treats it as opaque.
    GenericCli,
}

impl AgentPlatformKind {
    /// Stable, lower-snake-case string for use in config and JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::GeminiCli => "gemini_cli",
            Self::OpenCode => "opencode",
            Self::GenericCli => "generic_cli",
        }
    }

    /// Parse from the canonical string. Inverse of [`Self::as_str`].
    ///
    /// # Errors
    /// Returns [`HarnessError::UnknownPlatform`] when the string is not recognized.
    pub fn parse(value: &str) -> Result<Self, HarnessError> {
        match value {
            "claude_code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "gemini_cli" => Ok(Self::GeminiCli),
            "opencode" => Ok(Self::OpenCode),
            "generic_cli" => Ok(Self::GenericCli),
            other => Err(HarnessError::UnknownPlatform {
                value: other.to_owned(),
            }),
        }
    }

    /// Default CLI binary name (the user can still override per-workspace).
    #[must_use]
    pub fn default_binary(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::GeminiCli => "gemini",
            Self::OpenCode => "opencode",
            Self::GenericCli => "",
        }
    }

    /// Whether this platform should be considered an active primary backend.
    #[must_use]
    pub fn is_primary(self) -> bool {
        matches!(self, Self::ClaudeCode | Self::Codex | Self::GeminiCli)
    }
}

impl fmt::Display for AgentPlatformKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Capabilities a backend advertises. These are the things the orchestrator
/// can ask the backend to do without leaving its native protocol.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BackendCapabilities {
    /// Emits structured JSONL events on stdout in addition to the human-facing TUI.
    EmitsJsonl,
    /// Supports being resumed from a prior session id.
    SupportsResume,
    /// Supports per-workspace configuration via a project-local config file.
    PerWorkspaceConfig,
    /// Streams tool-call events in real time.
    StreamsToolEvents,
}

impl BackendCapabilities {
    /// Convenience: a backend that does the bare minimum.
    #[must_use]
    pub fn baseline() -> Self {
        Self::PerWorkspaceConfig
    }
}

/// Descriptor for a single CLI subprocess (the "actor" the orchestrator will spawn).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct CliSubprocessSpec {
    /// Absolute path to the binary, or a name resolvable via `PATH`.
    pub program: String,
    /// Default arguments (the orchestrator may append more).
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional environment overrides merged with the parent env.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory; defaults to the active workspace root.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

impl CliSubprocessSpec {
    /// Create a new spec that just calls `program` with no args.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            working_dir: None,
        }
    }
}

/// Full description of a backend the orchestrator can drive.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BackendDescriptor {
    /// Stable UUID.
    pub id: Uuid,
    /// Platform kind.
    pub kind: AgentPlatformKind,
    /// Human-readable display name (e.g. "Claude Code 1.2.3").
    pub display_name: String,
    /// Default CLI spec.
    pub default_cli: CliSubprocessSpec,
    /// Capabilities the orchestrator may rely on.
    pub capabilities: Vec<BackendCapabilities>,
    /// The native tools this backend exposes via its MCP surface.
    pub native_tools: Vec<ToolSpec>,
    /// Default risk floor for native tool calls (the orchestrator will not lower it).
    pub risk_floor: RiskClass,
}

/// Registry of backend descriptors. The orchestrator reads this at startup
/// to know what harnesses are available.
#[derive(Clone, Debug, Default)]
pub struct BackendRegistry {
    by_id: BTreeMap<Uuid, Arc<BackendDescriptor>>,
    by_kind: BTreeMap<AgentPlatformKind, Uuid>,
}

impl BackendRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a descriptor. Re-registering the same kind overwrites the prior mapping.
    ///
    /// # Errors
    /// Returns [`ContractsError::DuplicateBackend`] when the same `id` is registered twice
    /// with a different `kind`.
    pub fn register(&mut self, descriptor: BackendDescriptor) -> ContractsResult<()> {
        let id = descriptor.id;
        let kind = descriptor.kind;
        if let Some(prior) = self.by_id.get(&id) {
            if prior.kind != kind {
                return Err(crate::error::ContractsError::DuplicateBackend { kind, existing: id });
            }
        }
        self.by_id.insert(id, Arc::new(descriptor));
        self.by_kind.insert(kind, id);
        Ok(())
    }

    /// Look up a backend by id.
    #[must_use]
    pub fn get(&self, id: Uuid) -> Option<Arc<BackendDescriptor>> {
        self.by_id.get(&id).cloned()
    }

    /// Look up a backend by platform kind.
    #[must_use]
    pub fn get_by_kind(&self, kind: AgentPlatformKind) -> Option<Arc<BackendDescriptor>> {
        self.by_kind
            .get(&kind)
            .and_then(|id| self.by_id.get(id).cloned())
    }

    /// Iterate over all backends, sorted by display name.
    pub fn iter(&self) -> impl Iterator<Item = Arc<BackendDescriptor>> + '_ {
        let mut items: Vec<Arc<BackendDescriptor>> = self.by_id.values().cloned().collect();
        items.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        items.into_iter()
    }

    /// Number of registered backends.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry has no backends.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Identifier alias for use by other layers.
pub type BackendId = Uuid;

/// Errors raised by the harness layer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HarnessError {
    /// The platform string was not recognized.
    #[error("unknown platform {value:?}; expected one of claude_code, codex, gemini_cli, opencode, generic_cli")]
    UnknownPlatform {
        /// The string that did not match.
        value: String,
    },

    /// The session id is not a valid UUID.
    #[error("invalid session id {0:?}")]
    InvalidSessionId(String),

    /// The CLI binary could not be found on `PATH`.
    #[error("binary {program:?} not found on PATH for platform {platform}")]
    BinaryNotFound {
        /// Platform the orchestrator was trying to drive.
        platform: AgentPlatformKind,
        /// Program that could not be located.
        program: String,
    },
}

/// Result alias for harness-layer operations.
pub type HarnessResult<T> = Result<T, HarnessError>;

/// Adapter trait for spawning a specific backend. The runtime provides
/// a default implementation per [`AgentPlatformKind`]; users can also
/// supply their own.
pub trait HarnessAdapter: Send + Sync {
    /// Platform this adapter drives.
    fn kind(&self) -> AgentPlatformKind;

    /// The CLI spec to use when spawning a fresh session.
    fn cli_for(&self, session: SessionId) -> CliSubprocessSpec;

    /// Whether the given stderr chunk indicates the backend wants tool approval.
    fn needs_approval(&self, stderr_chunk: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolSpec;

    fn fixture(id: Uuid, kind: AgentPlatformKind) -> BackendDescriptor {
        BackendDescriptor {
            id,
            kind,
            display_name: kind.as_str().to_owned(),
            default_cli: CliSubprocessSpec::new(kind.default_binary()),
            capabilities: vec![BackendCapabilities::baseline()],
            native_tools: vec![ToolSpec::dummy("echo")],
            risk_floor: RiskClass::Low,
        }
    }

    #[test]
    fn kind_round_trips_through_string() {
        for kind in [
            AgentPlatformKind::ClaudeCode,
            AgentPlatformKind::Codex,
            AgentPlatformKind::GeminiCli,
            AgentPlatformKind::OpenCode,
            AgentPlatformKind::GenericCli,
        ] {
            assert_eq!(AgentPlatformKind::parse(kind.as_str()), Ok(kind));
        }
    }

    #[test]
    fn unknown_kind_returns_error() {
        assert!(AgentPlatformKind::parse("nope").is_err());
    }

    #[test]
    fn registry_replaces_same_id_with_same_kind() {
        let mut reg = BackendRegistry::new();
        let id = Uuid::new_v4();
        reg.register(fixture(id, AgentPlatformKind::ClaudeCode))
            .unwrap();
        reg.register(fixture(id, AgentPlatformKind::ClaudeCode))
            .unwrap();
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn registry_rejects_same_id_different_kind() {
        let mut reg = BackendRegistry::new();
        let id = Uuid::new_v4();
        reg.register(fixture(id, AgentPlatformKind::ClaudeCode))
            .unwrap();
        assert!(reg.register(fixture(id, AgentPlatformKind::Codex)).is_err());
    }

    #[test]
    fn registry_iter_is_sorted_by_display_name() {
        let mut reg = BackendRegistry::new();
        reg.register(fixture(Uuid::new_v4(), AgentPlatformKind::Codex))
            .unwrap();
        reg.register(fixture(Uuid::new_v4(), AgentPlatformKind::ClaudeCode))
            .unwrap();
        let names: Vec<String> = reg.iter().map(|b| b.display_name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn is_primary_matches_three_active_backends() {
        assert!(AgentPlatformKind::ClaudeCode.is_primary());
        assert!(AgentPlatformKind::Codex.is_primary());
        assert!(AgentPlatformKind::GeminiCli.is_primary());
        assert!(!AgentPlatformKind::OpenCode.is_primary());
        assert!(!AgentPlatformKind::GenericCli.is_primary());
    }
}
