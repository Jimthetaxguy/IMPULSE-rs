//! Core trait and types for dynamic tools

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use super::error::ToolError;

/// Lexically collapse `.` and `..` components without touching the filesystem.
/// Used as a security fallback when a path can't be canonicalized (e.g. a file
/// being created that does not exist yet) so traversal sequences can't survive
/// into the `starts_with` sandbox check.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a path to an absolute, traversal-free form for the sandbox check.
///
/// Prefers `canonicalize` (resolves symlinks and `..` for existing paths). When
/// the target itself does not exist (typical for a write of a new file), it
/// canonicalizes the nearest existing ancestor and re-attaches the remainder,
/// collapsing `..` lexically — so `<root>/../../etc/x` can never be mistaken for
/// a path under `<root>`.
fn secure_resolve(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    // The target doesn't exist (e.g. a new file). Canonicalize the nearest
    // existing ancestor — resolving symlinks (e.g. macOS /var -> /private/var)
    // and `..` in the existing prefix — then re-attach the remaining tail with
    // any `..` collapsed lexically so it can't escape the resolved ancestor.
    for ancestor in path.ancestors().skip(1) {
        if let Ok(base) = std::fs::canonicalize(ancestor) {
            return match path.strip_prefix(ancestor) {
                Ok(rest) => normalize_lexical(&base.join(rest)),
                Err(_) => base,
            };
        }
    }
    normalize_lexical(path)
}

/// Capability a tool may require — deny-by-default security model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    FileSystemRead,
    FileSystemWrite,
    Network,
    PythonExec,
    SystemInfo,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::FileSystemRead => "filesystem_read",
            Capability::FileSystemWrite => "filesystem_write",
            Capability::Network => "network",
            Capability::PythonExec => "python_exec",
            Capability::SystemInfo => "system_info",
        }
    }
}

/// Where a tool came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    Builtin,
    Document,
    ExternalProcess,
    Plugin,
    McpProxy,
}

impl ToolSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Document => "document",
            Self::ExternalProcess => "external_process",
            Self::Plugin => "plugin",
            Self::McpProxy => "mcp_proxy",
        }
    }
}

/// Origin of the current tool execution request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOrigin {
    Cli,
    Daemon,
    Mcp,
    Test,
}

impl ExecutionOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Daemon => "daemon",
            Self::Mcp => "mcp",
            Self::Test => "test",
        }
    }
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParamType {
    String,
    Integer,
    Float,
    Bool,
    FilePath,
    Json,
}

impl ParamType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Integer => "integer",
            ParamType::Float => "float",
            ParamType::Bool => "bool",
            ParamType::FilePath => "file_path",
            ParamType::Json => "json",
        }
    }
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

/// Tool metadata exported to agents and external runtimes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub params: Vec<ToolParam>,
    pub capabilities: Vec<String>,
    pub source: String,
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
    /// Where the request came from
    pub execution_origin: ExecutionOrigin,
    /// Maximum serialized output bytes before truncation
    pub max_output_bytes: usize,
    /// Maximum number of artifacts to return
    pub max_artifacts: usize,
    /// Read roots allowed for FileSystemRead tools. Empty means unrestricted.
    pub allowed_read_roots: Vec<PathBuf>,
    /// Write roots allowed for FileSystemWrite tools. Empty means unrestricted.
    pub allowed_write_roots: Vec<PathBuf>,
}

impl Default for ToolContext {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            impulse_dir: PathBuf::from(".impulse"),
            session_id: None,
            allowed_capabilities: [Capability::FileSystemRead, Capability::SystemInfo]
                .into_iter()
                .collect(),
            timeout_ms: 30_000,
            execution_origin: ExecutionOrigin::Daemon,
            max_output_bytes: 256 * 1024,
            max_artifacts: 8,
            allowed_read_roots: vec![cwd.clone(), PathBuf::from(".impulse")],
            allowed_write_roots: vec![PathBuf::from(".impulse")],
        }
    }
}

impl ToolContext {
    /// Create a context for a specific origin.
    pub fn for_origin(impulse_dir: PathBuf, execution_origin: ExecutionOrigin) -> Self {
        Self {
            impulse_dir,
            execution_origin,
            ..Default::default()
        }
    }

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
            execution_origin: ExecutionOrigin::Cli,
            allowed_read_roots: Vec::new(),
            allowed_write_roots: Vec::new(),
            ..Default::default()
        }
    }

    /// Resolve a potentially relative path against the current working directory.
    pub fn resolve_path(&self, path: &str) -> PathBuf {
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            candidate
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(candidate)
        }
    }

    /// Check whether a path is allowed for the requested access mode.
    pub fn is_path_allowed(&self, path: &std::path::Path, write: bool) -> bool {
        let roots = if write {
            &self.allowed_write_roots
        } else {
            &self.allowed_read_roots
        };

        if roots.is_empty() {
            return true;
        }

        // Resolve to a traversal-free path. The previous implementation fell
        // back to the RAW path when canonicalize failed (the common case for a
        // not-yet-created write target), which let `<root>/../../etc/x` slip
        // through the component-based `starts_with` check.
        let candidate = secure_resolve(path);

        roots.iter().any(|root| {
            let root = secure_resolve(root);
            candidate.starts_with(&root)
        })
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
    fn test_normalize_lexical_collapses_traversal() {
        assert_eq!(
            normalize_lexical(Path::new("/a/b/../../etc/passwd")),
            PathBuf::from("/etc/passwd")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/b/./c")),
            PathBuf::from("/a/b/c")
        );
        // A pop at the root stays at the root (cannot underflow).
        assert_eq!(
            normalize_lexical(Path::new("/../../x")),
            PathBuf::from("/x")
        );
    }

    #[test]
    fn test_is_path_allowed_blocks_traversal_to_nonexistent_target() {
        let tmp = tempfile::tempdir().unwrap();
        // Canonicalize the root up front so the traversal is demonstrated
        // independently of platform symlink quirks (e.g. macOS /var ->
        // /private/var): the raw escape path then genuinely shares the root's
        // prefix, which is exactly what the old component-based check allowed.
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let ctx = ToolContext {
            allowed_write_roots: vec![root.clone()],
            ..ToolContext::default()
        };

        // A new (non-existent) file UNDER the root is allowed.
        let inside = root.join("subdir/new_file.txt");
        assert!(
            ctx.is_path_allowed(&inside, true),
            "writing a new file under the sandbox root must be allowed"
        );

        // A traversal escaping the root to a non-existent target must be denied.
        // The raw path `<root>/../../../tmp/x` shares the root's leading
        // components, so the previous `starts_with` check (on the un-collapsed
        // path, since canonicalize fails for a non-existent target) wrongly
        // allowed it.
        let escape = root.join("../../../tmp/impulse_escape_test_file");
        assert!(
            !ctx.is_path_allowed(&escape, true),
            "traversal to a non-existent target outside the root must be denied"
        );
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
        assert_eq!(ctx.execution_origin, ExecutionOrigin::Daemon);
    }

    #[test]
    fn test_tool_context_all_capabilities() {
        let ctx = ToolContext::with_all_capabilities();
        assert!(ctx.has_capability(Capability::FileSystemRead));
        assert!(ctx.has_capability(Capability::FileSystemWrite));
        assert!(ctx.has_capability(Capability::Network));
        assert!(ctx.has_capability(Capability::PythonExec));
        assert!(ctx.has_capability(Capability::SystemInfo));
        assert!(ctx.allowed_read_roots.is_empty());
        assert!(ctx.allowed_write_roots.is_empty());
    }

    #[test]
    fn test_tool_category_display() {
        assert_eq!(ToolCategory::Utility.to_string(), "utility");
        assert_eq!(ToolCategory::Document.to_string(), "document");
        assert_eq!(ToolCategory::Analysis.to_string(), "analysis");
        assert_eq!(ToolCategory::System.to_string(), "system");
    }

    #[test]
    fn test_tool_source_str() {
        assert_eq!(ToolSource::Builtin.as_str(), "builtin");
        assert_eq!(ToolSource::ExternalProcess.as_str(), "external_process");
    }
}
