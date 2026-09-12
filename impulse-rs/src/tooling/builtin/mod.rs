//! Built-in tools — thin wrappers around existing tools/ implementations
//!
//! These wrap the existing functionality in tools::python, tools::system,
//! tools::health, tools::benchmark, and build_hygiene with the DynamicTool
//! trait interface. Each tool is a zero-cost wrapper — no code duplication.

mod bash_exec;
mod benchmarker;
mod build_health;
mod calculator;
mod config_get;
mod file_read;
mod file_write;
mod genome_read;
mod health_check;
mod memory_search;
mod python_exec;
mod session_query;
mod steward_status;
mod system_info;

pub use bash_exec::BashExecTool;
pub use benchmarker::BenchmarkerTool;
pub use build_health::{
    BuildHealthTool, CleanAllTool, SccacheSetupTool, SccacheStatusTool, SweepTool,
    ToolAvailabilityTool, WipeTool,
};
pub use calculator::CalculatorTool;
pub use config_get::ConfigGetTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use genome_read::GenomeReadTool;
pub use health_check::HealthCheckTool;
pub use memory_search::MemorySearchTool;
pub use python_exec::PythonExecTool;
pub use session_query::SessionQueryTool;
pub use steward_status::StewardStatusTool;
pub use system_info::SystemInfoTool;

use super::error::ToolError;
use super::registry::ToolRegistry;

/// Resolves and validates the `impulse_dir` parameter shared by the two
/// read-only memory tools (`memory_search`, `genome_read`) -- review
/// round 5, P1 Codex on PR #54, items 2/3; corrected review round 6,
/// MEDIUM REFUTED.
///
/// **Why not the shared `allowed_read_roots` check:** these tools used to
/// declare `impulse_dir` as `ParamType::FilePath`, routing it through
/// `ToolExecutor::validate_paths`'s generic check against `ctx.
/// allowed_read_roots` -- the SAME roots `file_read` shares (`bash_exec`
/// is NOT constrained by `allowed_read_roots` at all: it only requires the
/// `ShellExec` capability and checks its own `cwd` argument, gated instead
/// by `ion_repl::chat`'s confirmation prompt -- an earlier version of this
/// comment incorrectly also named `bash_exec` here). Making a normal
/// launch's `.impulse` correctly resolve to the project's own state
/// directory (not `$HOME/.impulse`) needed `ReplContext::
/// sandbox_tool_context` to add whatever `impulse_dir` resolves to onto
/// those shared roots -- but for an explicitly-configured `IMPULSE_HOME`
/// OUTSIDE the project, that would have widened `file_read`'s own reach to
/// include it too, authorizing that tool to read a directory the model was
/// never granted access to for anything but these two memory tools
/// specifically.
///
/// Instead, `memory_search`/`genome_read` declare `impulse_dir` as a plain
/// `ParamType::String` (invisible to the generic FilePath check) and each
/// call this helper themselves: the resolved path must be either `ctx.
/// project_impulse_dir` (the launching project's OWN directory -- always
/// allowed, regardless of what `ctx.impulse_dir` currently resolves to) or,
/// only when the `IMPULSE_HOME` environment variable is itself explicitly
/// set and non-blank, that exact directory. `ctx.impulse_dir` itself is
/// also accepted (covers the common case where it already equals one of
/// the two above, and non-`ion_repl` callers that only ever set
/// `impulse_dir`). Anything else is refused with the same
/// `ToolError::PathNotAllowed` shape `validate_paths` already uses for the
/// shared roots, so a caller sees a consistent refusal regardless of which
/// mechanism produced it. Path containment reuses `secure_resolve`
/// (canonicalize-or-lexically-collapse) so a `..`-traversal candidate
/// can't be mistaken for a path under any allowed root, matching the
/// shared sandbox's own guarantee exactly.
///
/// **Why `ctx.impulse_dir` alone was not enough (review round 6, MEDIUM
/// REFUTED, CONFIRMED):** `ion_repl::ReplContext::sandbox_tool_context`
/// sets `ctx.impulse_dir` to `IMPULSE_HOME` itself whenever that env var is
/// set. An earlier version of this function checked an explicit override
/// against `{ctx.impulse_dir, IMPULSE_HOME}` -- which, whenever
/// `IMPULSE_HOME` was set, collapsed to `{IMPULSE_HOME, IMPULSE_HOME}`,
/// losing the project's own directory as an allowed target entirely: with
/// `IMPULSE_HOME` set, `genome_read {"impulse_dir": "<repo>/.impulse"}`
/// was DENIED, while the OMITTED-parameter default silently read the home
/// genome instead. `ctx.project_impulse_dir` (set independently by
/// `sandbox_tool_context`, see that field's own doc comment) is what
/// fixes this: the project's own directory is always in the allowed set,
/// no matter what `ctx.impulse_dir` currently equals.
///
/// **`IMPULSE_HOME` is trusted verbatim, once trimmed:** this function (and
/// `sandbox_tool_context`, which resolves the same env var for the
/// DEFAULT) treats `IMPULSE_HOME` as an operator-set, trusted value, not
/// untrusted input requiring its own sandboxing -- it is read directly from
/// the process environment, never from model-supplied `params`. Both call
/// sites trim it before use (not merely before the emptiness check): a
/// padded value (e.g. a trailing newline from a sourced shell profile)
/// must not silently deny the very directory it names.
pub(super) fn resolve_and_validate_memory_dir(
    explicit: Option<&str>,
    ctx: &super::traits::ToolContext,
) -> Result<std::path::PathBuf, ToolError> {
    let Some(explicit) = explicit else {
        return Ok(ctx.impulse_dir.clone());
    };
    let candidate = ctx.resolve_path(explicit);
    let mut allowed_roots = vec![ctx.impulse_dir.clone(), ctx.project_impulse_dir.clone()];
    if let Ok(home) = std::env::var("IMPULSE_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            allowed_roots.push(std::path::PathBuf::from(trimmed));
        }
    }
    let resolved_candidate = super::traits::secure_resolve(&candidate);
    let is_allowed = allowed_roots
        .iter()
        .any(|root| resolved_candidate.starts_with(super::traits::secure_resolve(root)));
    if is_allowed {
        Ok(candidate)
    } else {
        Err(ToolError::PathNotAllowed(candidate.display().to_string()))
    }
}

/// Register all built-in tools into a registry
pub fn register_all(registry: &mut ToolRegistry) -> Result<(), ToolError> {
    registry.register(Box::new(BashExecTool))?;
    registry.register(Box::new(BenchmarkerTool))?;
    registry.register(Box::new(BuildHealthTool))?;
    registry.register(Box::new(CalculatorTool))?;
    registry.register(Box::new(CleanAllTool))?;
    registry.register(Box::new(ConfigGetTool))?;
    registry.register(Box::new(FileReadTool))?;
    registry.register(Box::new(FileWriteTool))?;
    registry.register(Box::new(GenomeReadTool))?;
    registry.register(Box::new(HealthCheckTool))?;
    registry.register(Box::new(MemorySearchTool))?;
    registry.register(Box::new(PythonExecTool))?;
    registry.register(Box::new(SccacheSetupTool))?;
    registry.register(Box::new(SccacheStatusTool))?;
    registry.register(Box::new(SessionQueryTool))?;
    registry.register(Box::new(StewardStatusTool))?;
    registry.register(Box::new(SweepTool))?;
    registry.register(Box::new(SystemInfoTool))?;
    registry.register(Box::new(ToolAvailabilityTool))?;
    registry.register(Box::new(WipeTool))?;
    Ok(())
}
