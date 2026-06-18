//! Tool descriptors — typed tool calls with concurrency classification.

use crate::error::ContractsResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Concurrency class for a tool. Mirrors the `read-parallel` / `write-serial`
/// distinction in the agent-harness-patterns corpus.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Deserialize,
    Serialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyClass {
    /// Pure read; safe to run in parallel with anything.
    #[default]
    ReadParallel,
    /// Read but order-sensitive (e.g. `check_rate_limit`).
    ReadSerial,
    /// Mutates state; must be serialized across the whole orchestrator.
    WriteSerial,
    /// Long-running; needs a dedicated slot and a timeout.
    Special,
}

impl ConcurrencyClass {
    /// Default for tools whose schema does not declare one.
    #[must_use]
    pub fn default_for(name: &str) -> Self {
        if name.starts_with("read_") || name.starts_with("list_") || name.starts_with("search_") {
            Self::ReadParallel
        } else if name.starts_with("write_")
            || name.starts_with("delete_")
            || name.starts_with("update_")
            || name.starts_with("patch_")
        {
            Self::WriteSerial
        } else {
            Self::Special
        }
    }
}

impl fmt::Display for ConcurrencyClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::ReadParallel => "read_parallel",
            Self::ReadSerial => "read_serial",
            Self::WriteSerial => "write_serial",
            Self::Special => "special",
        };
        f.write_str(s)
    }
}

/// Risk classification. The orchestrator can **raise** the floor but never lower it.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// No filesystem or network mutation, no spend.
    Low,
    /// Local mutation only; reversible.
    Medium,
    /// Network egress, secrets, or partial spend.
    High,
    /// Spend, destructive, or credentialed actions.
    Special,
}

impl RiskClass {
    /// Maximum of two risk classes (the higher wins).
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

impl fmt::Display for RiskClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Special => "special",
        };
        f.write_str(s)
    }
}

/// A tool exposed by a backend, ready to be advertised on the MCP surface.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct ToolSpec {
    /// Stable, snake_case tool name (e.g. `read_file`).
    pub name: String,
    /// One-line description shown to the agent.
    pub description: String,
    /// JSON Schema for the input shape.
    pub input_schema: serde_json::Value,
    /// Concurrency class — defaults to [`ConcurrencyClass::default_for`] for the name.
    #[serde(default)]
    pub concurrency: ConcurrencyClass,
    /// Risk floor.
    pub risk: RiskClass,
}

impl ToolSpec {
    /// Build a test/placeholder tool spec with empty input schema.
    #[must_use]
    pub fn dummy(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            concurrency: ConcurrencyClass::default_for("dummy"),
            risk: RiskClass::Low,
        }
    }

    /// Validate the spec is well-formed.
    ///
    /// # Errors
    /// Returns [`ContractsError::InvalidToolSpec`] if the name is empty or the
    /// input schema is not a JSON object.
    pub fn validate(&self) -> ContractsResult<()> {
        if self.name.is_empty() {
            return Err(crate::error::ContractsError::InvalidToolSpec {
                reason: "tool name is empty".to_owned(),
            });
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(crate::error::ContractsError::InvalidToolSpec {
                reason: format!("tool name {:?} must be snake_case ascii", self.name),
            });
        }
        if self.name.starts_with('_') || self.name.ends_with('_') {
            return Err(crate::error::ContractsError::InvalidToolSpec {
                reason: format!(
                    "tool name {:?} must not start or end with underscore",
                    self.name
                ),
            });
        }
        if !self.input_schema.is_object() {
            return Err(crate::error::ContractsError::InvalidToolSpec {
                reason: "input_schema must be a JSON object".to_owned(),
            });
        }
        Ok(())
    }
}

/// A descriptor paired with a function that executes the tool.
/// We intentionally do **not** ship any default implementations of
/// `execute`; the runtime crate binds these.
#[derive(Clone, Debug)]
pub struct ToolDescriptor {
    /// The spec advertised to the agent.
    pub spec: ToolSpec,
}

/// Errors raised by tool dispatch.
#[derive(Debug, Error)]
pub enum ToolError {
    /// Tool was not found.
    #[error("tool {0:?} not registered")]
    NotFound(String),

    /// Tool args failed schema validation.
    #[error("invalid args for tool {tool}: {reason}")]
    InvalidArgs {
        /// Tool name.
        tool: String,
        /// Why the args are invalid.
        reason: String,
    },

    /// Tool was denied by the permission pipeline.
    #[error("permission denied for tool {tool}: {reason}")]
    PermissionDenied {
        /// Tool name.
        tool: String,
        /// Why the permission denied.
        reason: String,
    },

    /// Tool exceeded its timeout.
    #[error("tool {tool} timed out after {ms} ms")]
    Timeout {
        /// Tool name.
        tool: String,
        /// Timeout in ms.
        ms: u64,
    },

    /// Tool returned a permanent error (no retry recommended).
    #[error("tool {tool} failed permanently: {reason}")]
    Permanent {
        /// Tool name.
        tool: String,
        /// Failure reason.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn concurrency_class_default_picks_writes_serially() {
        assert_eq!(
            ConcurrencyClass::default_for("read_file"),
            ConcurrencyClass::ReadParallel
        );
        assert_eq!(
            ConcurrencyClass::default_for("write_file"),
            ConcurrencyClass::WriteSerial
        );
        assert_eq!(
            ConcurrencyClass::default_for("search_history"),
            ConcurrencyClass::ReadParallel
        );
        assert_eq!(
            ConcurrencyClass::default_for("mystery_tool"),
            ConcurrencyClass::Special
        );
    }

    #[test]
    fn risk_class_max_picks_higher() {
        assert_eq!(RiskClass::Low.max(RiskClass::High), RiskClass::High);
        assert_eq!(RiskClass::High.max(RiskClass::Low), RiskClass::High);
        assert_eq!(RiskClass::Medium.max(RiskClass::Medium), RiskClass::Medium);
    }

    #[test]
    fn tool_spec_rejects_empty_name() {
        let spec = ToolSpec::dummy("");
        assert!(spec.validate().is_err());
    }

    #[test]
    fn tool_spec_rejects_non_snake_case() {
        let spec = ToolSpec::dummy("NotSnake");
        assert!(spec.validate().is_err());
    }

    #[test]
    fn tool_spec_requires_object_schema() {
        let mut spec = ToolSpec::dummy("ok");
        spec.input_schema = json!("not an object");
        assert!(spec.validate().is_err());
    }

    #[test]
    fn tool_spec_round_trips_through_json() {
        let spec = ToolSpec::dummy("search_history");
        let s = serde_json::to_string(&spec).unwrap();
        let back: ToolSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(spec, back);
    }
}
