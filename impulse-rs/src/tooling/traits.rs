//! Core trait and types for dynamic tools

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::error::ToolError;

/// Capability a tool may require — deny-by-default security model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    FileSystemRead,
    FileSystemWrite,
    Network,
    PythonExec,
    SystemInfo,
}

/// Category for organizing tools in listings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCategory {
    Utility,
    Document,
    Analysis,
    System,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCategory::Utility => write!(f, "utility"),
            ToolCategory::Document => write!(f, "document"),
            ToolCategory::Analysis => write!(f, "analysis"),
            ToolCategory::System => write!(f, "system"),
        }
    }
}

/// Parameter type for tool inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamType {
    String,
    Integer,
    Float,
    Bool,
    FilePath,
    Json,
}

/// Describes a single parameter a tool accepts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub description: String,
    pub param_type: ParamType,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

/// Human-readable descriptor for a tool (for CLI --help, LLM tool schemas)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub category: ToolCategory,
    pub params: Vec<ToolParam>,
}

/// Result of executing a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Primary output data
    pub output: serde_json::Value,
    /// Files produced by the tool (if any)
    pub artifacts: Vec<PathBuf>,
    /// Arbitrary metadata (timing, row counts, etc.)
    pub metadata: HashMap<String, String>,
}

impl ToolResult {
    /// Create a simple text result
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            output: serde_json::Value::String(s.into()),
            artifacts: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a JSON result
    pub fn json(value: serde_json::Value) -> Self {
        Self {
            output: value,
            artifacts: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

/// Execution context passed to every tool — controls what it can do
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Path to .impulse/ directory
    pub impulse_dir: PathBuf,
    /// Current session ID (if any)
    pub session_id: Option<String>,
    /// Capabilities this invocation is allowed to use
    pub allowed_capabilities: HashSet<Capability>,
    /// Maximum execution time in milliseconds
    pub timeout_ms: u64,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            impulse_dir: PathBuf::from(".impulse"),
            session_id: None,
            allowed_capabilities: [Capability::FileSystemRead, Capability::SystemInfo]
                .into_iter()
                .collect(),
            timeout_ms: 30_000,
        }
    }
}

impl ToolContext {
    /// Check if a capability is allowed in this context
    pub fn has_capability(&self, cap: Capability) -> bool {
        self.allowed_capabilities.contains(&cap)
    }

    /// Create a context with all capabilities (for CLI direct invocation)
    pub fn with_all_capabilities() -> Self {
        Self {
            allowed_capabilities: [
                Capability::FileSystemRead,
                Capability::FileSystemWrite,
                Capability::Network,
                Capability::PythonExec,
                Capability::SystemInfo,
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        }
    }
}

/// The core trait every dynamic tool implements.
///
/// Follows the same pattern as `agent::LlmProvider` (async_trait, Send+Sync).
/// Tools declare their capabilities upfront and validate params before execution.
#[async_trait]
pub trait DynamicTool: Send + Sync {
    /// Unique identifier (e.g., "calculator", "xlsx_read")
    fn id(&self) -> &str;

    /// Human-readable descriptor with parameter schema
    fn descriptor(&self) -> ToolDescriptor;

    /// Validate parameters before execution — called automatically by ToolRegistry
    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError>;

    /// Execute the tool with validated parameters
    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError>;

    /// Capabilities this tool requires — checked against ToolContext before execution
    fn required_capabilities(&self) -> Vec<Capability>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_text() {
        let result = ToolResult::text("hello");
        assert_eq!(result.output, serde_json::Value::String("hello".into()));
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn test_tool_result_json() {
        let val = serde_json::json!({"key": "value"});
        let result = ToolResult::json(val.clone());
        assert_eq!(result.output, val);
    }

    #[test]
    fn test_tool_context_default() {
        let ctx = ToolContext::default();
        assert!(ctx.has_capability(Capability::FileSystemRead));
        assert!(ctx.has_capability(Capability::SystemInfo));
        assert!(!ctx.has_capability(Capability::FileSystemWrite));
        assert!(!ctx.has_capability(Capability::PythonExec));
    }

    #[test]
    fn test_tool_context_all_capabilities() {
        let ctx = ToolContext::with_all_capabilities();
        assert!(ctx.has_capability(Capability::FileSystemRead));
        assert!(ctx.has_capability(Capability::FileSystemWrite));
        assert!(ctx.has_capability(Capability::Network));
        assert!(ctx.has_capability(Capability::PythonExec));
        assert!(ctx.has_capability(Capability::SystemInfo));
    }

    #[test]
    fn test_tool_category_display() {
        assert_eq!(ToolCategory::Utility.to_string(), "utility");
        assert_eq!(ToolCategory::Document.to_string(), "document");
        assert_eq!(ToolCategory::Analysis.to_string(), "analysis");
        assert_eq!(ToolCategory::System.to_string(), "system");
    }
}
