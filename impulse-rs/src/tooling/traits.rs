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
    /// Run an arbitrary shell command as a child process. Separate from
    /// `FileSystemWrite` because a shell command's effects are not limited to
    /// path-scoped file writes (network, process spawn, etc.) — kept as its
    /// own deny-by-default capability so a tool that only needs file I/O
    /// never implicitly gets shell access (TUI_SPEC.md T7: `bash_exec`
    /// ReplTool bridge).
    ShellExec,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::FileSystemRead => "filesystem_read",
            Capability::FileSystemWrite => "filesystem_write",
            Capability::Network => "network",
            Capability::PythonExec => "python_exec",
            Capability::SystemInfo => "system_info",
            Capability::ShellExec => "shell_exec",
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
    /// ElevenLabs-first voice engine tool bridge (client tool / webhook).
    Voice,
    Test,
}

impl ExecutionOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Daemon => "daemon",
            Self::Mcp => "mcp",
            Self::Voice => "voice",
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
                Capability::ShellExec,
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
        assert!(ctx.has_capability(Capability::ShellExec));
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

/// Property-based / fuzz-style tests over [`secure_resolve`]/
/// [`ToolContext::is_path_allowed`] -- see
/// `docs/superpowers/specs/2026-09-12-governed-parser-property-tests.md`.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn ctx_for(root: &Path) -> ToolContext {
        ToolContext {
            allowed_write_roots: vec![root.to_path_buf()],
            allowed_read_roots: vec![root.to_path_buf()],
            ..ToolContext::with_all_capabilities()
        }
    }

    proptest! {
        /// Never panics for arbitrary path-shaped strings, with or without
        /// sandbox roots configured, whether or not the path exists.
        #[test]
        fn is_path_allowed_never_panics(
            raw in "(\\.\\./|/|~/|[a-zA-Z0-9_./-]){0,12}",
            write in any::<bool>(),
            rooted in any::<bool>(),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let ctx = if rooted { ctx_for(dir.path()) } else { ToolContext::default() };
            let _ = ctx.is_path_allowed(Path::new(&raw), write);
        }

        /// Deterministic: the same path and mode always yield the same
        /// verdict (no hidden time- or order-dependence).
        #[test]
        fn is_path_allowed_is_deterministic(
            raw in "(\\.\\./|/|[a-zA-Z0-9_./-]){0,12}",
            write in any::<bool>(),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx_for(dir.path());
            let path = dir.path().join(&raw);
            let a = ctx.is_path_allowed(&path, write);
            let b = ctx.is_path_allowed(&path, write);
            prop_assert_eq!(a, b);
        }

        /// Empty roots mean "no restriction" for that access mode --
        /// documented directly on `is_path_allowed`. `with_all_capabilities`
        /// is the constructor with empty `allowed_read_roots`/
        /// `allowed_write_roots`; `ToolContext::default()` is NOT empty --
        /// it defaults to `[cwd, .impulse]`/`[.impulse]` -- so this
        /// deliberately does not use `default()`.
        #[test]
        fn is_path_allowed_with_no_roots_configured_allows_anything(
            raw in ".{0,64}",
            write in any::<bool>(),
        ) {
            let ctx = ToolContext::with_all_capabilities();
            prop_assert!(ctx.is_path_allowed(Path::new(&raw), write));
        }
    }

    // Allowed => the resolved candidate is truly under a resolved root.
    // Built over a real filesystem tree (an existing subdirectory and an
    // existing file inside it, plus an existing sibling directory outside
    // the sandboxed root) so `canonicalize` succeeds cleanly on every
    // generated candidate -- the property is about the sandbox check, not
    // about `secure_resolve`'s not-yet-created-file fallback path (covered
    // separately below).
    proptest! {
        #[test]
        fn is_path_allowed_true_implies_resolved_path_is_under_a_resolved_root(
            segment in "[a-z][a-z0-9_]{0,8}",
            write in any::<bool>(),
            escape_levels in 0..4usize,
        ) {
            let base = tempfile::tempdir().unwrap();
            let root = base.path().join("root");
            let inside = root.join("inside");
            std::fs::create_dir_all(&inside).unwrap();
            std::fs::write(inside.join("file.txt"), b"x").unwrap();
            let ctx = ctx_for(&root);

            // A candidate that walks `escape_levels` directories up from
            // `inside` before appending a fresh segment -- at `escape_levels
            // <= 1` it stays under `root`; at higher values it (usually)
            // escapes above `root` into `base` or above.
            let mut candidate = inside.clone();
            for _ in 0..escape_levels {
                candidate.push("..");
            }
            candidate.push(&segment);

            let allowed = ctx.is_path_allowed(&candidate, write);
            if allowed {
                let resolved = secure_resolve(&candidate);
                let resolved_root = secure_resolve(&root);
                prop_assert!(
                    resolved.starts_with(&resolved_root),
                    "is_path_allowed said true for {candidate:?} (resolved {resolved:?}), \
                     which does not start with the resolved root {resolved_root:?}"
                );
            }
        }

        /// A trailing slash on an existing in-sandbox directory never
        /// changes the verdict compared to the same path without one.
        #[test]
        fn is_path_allowed_trailing_slash_does_not_change_the_verdict(
            segment in "[a-z][a-z0-9_]{0,8}",
            write in any::<bool>(),
        ) {
            let base = tempfile::tempdir().unwrap();
            let root = base.path().join("root");
            let sub = root.join(&segment);
            std::fs::create_dir_all(&sub).unwrap();
            let ctx = ctx_for(&root);

            let without_slash = ctx.is_path_allowed(&sub, write);
            let mut with_slash = sub.as_os_str().to_owned();
            with_slash.push("/");
            let with_slash = ctx.is_path_allowed(Path::new(&with_slash), write);
            prop_assert_eq!(without_slash, with_slash);
        }
    }

    /// A `..`-traversal candidate that lexically escapes the sandbox root,
    /// for a target that does not exist yet (the write-a-new-file case
    /// `secure_resolve`'s doc comment calls out): denied, never accidentally
    /// allowed because canonicalize failed open.
    #[test]
    fn is_path_allowed_denies_a_traversal_to_a_not_yet_created_file_outside_the_root() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ctx_for(&root);

        let candidate = root.join("../../etc/definitely-not-created-by-this-test");
        assert!(
            !ctx.is_path_allowed(&candidate, true),
            "a `..`-traversal to a nonexistent target outside the root must be denied"
        );
    }

    /// A symlink loop (`a` -> `b`, `b` -> `a`) must not panic or hang --
    /// `canonicalize` detects the cycle and errors, and `secure_resolve`'s
    /// ancestor fallback walk is bounded by path depth regardless.
    #[test]
    fn is_path_allowed_does_not_panic_or_hang_on_a_symlink_loop() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a");
        let b = root.join("b");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&b, &a).unwrap();
            std::os::unix::fs::symlink(&a, &b).unwrap();
        }
        #[cfg(not(unix))]
        {
            // No portable symlink-loop construction on this target; nothing
            // to probe, but the test still proves the harness itself builds.
            return;
        }
        let ctx = ctx_for(&root);
        let _ = ctx.is_path_allowed(&a, false);
        let _ = ctx.is_path_allowed(&a.join("further/nested/tail"), false);
    }

    /// A real self-referential symlink (`self -> self`) is the degenerate
    /// one-node cycle -- same panic/hang-safety property as the two-node
    /// loop above, exercised separately since it stresses
    /// `secure_resolve`'s ancestor walk differently (the symlink's own
    /// parent is real and resolves immediately).
    #[test]
    fn is_path_allowed_does_not_panic_or_hang_on_a_self_referential_symlink() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let looped = root.join("self_loop");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&looped, &looped).unwrap();
        }
        #[cfg(not(unix))]
        {
            return;
        }
        let ctx = ctx_for(&root);
        let _ = ctx.is_path_allowed(&looped, false);
    }
}
