//! Canonical error enums for the Impulse-RS contract layer.

use thiserror::Error;
use uuid::Uuid;

/// Result alias for contract-layer operations.
pub type ContractsResult<T> = Result<T, ContractsError>;

/// Errors that can occur in the contract layer (serialization, id parsing, path validation).
#[derive(Debug, Error)]
pub enum ContractsError {
    /// A UUID-shaped identifier could not be parsed.
    #[error("invalid {kind} id {value:?}: {source}")]
    InvalidId {
        /// Human-readable identifier kind (e.g. `"session"`).
        kind: &'static str,
        /// The original input.
        value: String,
        /// Underlying parse error.
        #[source]
        source: uuid::Error,
    },

    /// A workspace path failed validation (empty or relative).
    #[error("invalid workspace path: {reason}")]
    InvalidPath {
        /// Why the path is invalid.
        reason: String,
    },

    /// A backend descriptor was registered twice.
    #[error("backend {kind:?} already registered with id {existing}")]
    DuplicateBackend {
        /// Backend kind that collided.
        kind: crate::harness::AgentPlatformKind,
        /// Existing backend id.
        existing: Uuid,
    },

    /// A tool call referenced a tool the registry does not know about.
    #[error("unknown tool {name:?} (known: {available:?})")]
    UnknownTool {
        /// Tool name that was requested.
        name: String,
        /// List of available tool names (sorted).
        available: Vec<String>,
    },

    /// A tool spec failed JSON-schema validation.
    #[error("tool spec invalid: {reason}")]
    InvalidToolSpec {
        /// Why the spec is invalid.
        reason: String,
    },
}
