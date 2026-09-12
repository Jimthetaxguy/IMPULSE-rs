//! `ReplTool` registry for the ion REPL (TUI_SPEC.md T7).
//!
//! Owns the set of tools available to the chat loop, powers `/tools`
//! (listing), and lets `/verify` dispatch through the registry (per
//! TUI_SPEC.md section 2.3: "`/verify` calls the `ion_verify` ReplTool
//! directly") instead of a hardcoded call.
//!
//! Registers two capability universes side by side (TUI_SPEC.md section
//! 2.3's "Scope clarification"): `ion_verify`, the read-only spec-a gate
//! tool, and tools bridged from the existing `src/tooling::Tool` registry
//! (`file_read`, `file_write`, `bash_exec`, and the read-only
//! `memory_search`/`genome_read` -- see `tool_bridge::DynamicToolBridge`).
//! The verify gate's closed read-only allowlist and the REPL's full
//! coding-agent tool surface are kept conceptually separate; this registry
//! simply holds both.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::tooling::ToolRegistry;

use super::tool_bridge::DynamicToolBridge;
use super::tool_claim::GovernedSubmitClaimTool;
#[cfg(feature = "office-support")]
use super::tool_document::DocumentReadTool;
use super::tool_verify::IonVerifyTool;
use super::tools::ReplTool;

/// Ordered (by name) collection of registered `ReplTool`s.
pub struct ReplToolRegistry {
    tools: BTreeMap<&'static str, Box<dyn ReplTool>>,
}

impl ReplToolRegistry {
    /// Empty registry -- used by tests that want to register a bespoke set
    /// of tools without pulling in the full default set.
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    /// Register a tool by its own `name()`. Later registrations with the
    /// same name replace earlier ones (last-write-wins), matching the
    /// permissive behavior of a small, hand-populated registry -- unlike
    /// `src/tooling::ToolRegistry::register`, which errors on a duplicate ID
    /// (that registry aggregates independently-loaded tool sources, e.g.
    /// external-process manifests, where a silent collision would be a real
    /// misconfiguration bug worth surfacing).
    pub fn register(&mut self, tool: Box<dyn ReplTool>) {
        self.tools.insert(tool.name(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn ReplTool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// All registered tools, sorted by name (BTreeMap iteration order).
    pub fn list(&self) -> Vec<&dyn ReplTool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Builds the default ion REPL tool set: `ion_verify`,
    /// `governed_submit_claim`, the read-only `document_read` (only with the
    /// default `office-support` feature, matching `src/tooling::document`),
    /// plus `file_read`, `file_write`, `bash_exec`, `memory_search`, and
    /// `genome_read` bridged from `src/tooling::ToolRegistry::with_defaults()`.
    ///
    /// `memory_search`/`genome_read` are read-only (`Capability::FileSystemRead`
    /// only) and stay outside `CONFIRMATION_REQUIRED_TOOLS` like `file_read`
    /// and `document_read`. Bridging them through `DynamicToolBridge` (rather
    /// than a bespoke `ReplTool`) is what gives them the sandbox for free:
    /// both declare their `impulse_dir` parameter as `ParamType::FilePath`,
    /// so `ToolRegistry::execute`'s generic `validate_paths` step (`src/
    /// tooling/executor.rs`) already checks it against `ctx.sandbox_tool_context()`
    /// the same way it checks `file_read`'s `path` -- no new sandboxing code
    /// needed here, and a call that tries to point `impulse_dir` outside the
    /// session's read roots is refused before either tool's own `execute`
    /// runs. Neither tool's `impulse_dir` default (`.impulse`) changes: it
    /// resolves relative to the process's own working directory, which for
    /// an `ion` session is `IMPULSE_HOME`/the repo root in ordinary use.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(IonVerifyTool));
        registry.register(Box::new(GovernedSubmitClaimTool));
        #[cfg(feature = "office-support")]
        registry.register(Box::new(DocumentReadTool));

        let dynamic = Arc::new(ToolRegistry::with_defaults());
        registry.register(Box::new(DynamicToolBridge::new(
            Arc::clone(&dynamic),
            "file_read",
            "file_read {\"path\": \"...\", \"start_line\": 1, \"max_lines\": 200} \
             -- read a file",
        )));
        registry.register(Box::new(DynamicToolBridge::new(
            Arc::clone(&dynamic),
            "file_write",
            "file_write {\"path\": \"...\", \"content\": \"...\"} \
             -- atomically write (create/overwrite) a file",
        )));
        registry.register(Box::new(DynamicToolBridge::new(
            Arc::clone(&dynamic),
            "bash_exec",
            "bash_exec {\"command\": \"...\", \"cwd\": \"...\", \"timeout_secs\": 30} \
             -- run a shell command",
        )));
        registry.register(Box::new(DynamicToolBridge::new(
            Arc::clone(&dynamic),
            "memory_search",
            "memory_search {\"query\": \"...\", \"scope\": \"all\", \"mode\": \"keyword\", \
             \"limit\": 5} -- search GENOME decisions and session history",
        )));
        registry.register(Box::new(DynamicToolBridge::new(
            dynamic,
            "genome_read",
            "genome_read {\"section\": \"...\"} -- read permanent project decisions \
             and preferences from GENOME.md",
        )));

        registry
    }
}

impl Default for ReplToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_defaults_registers_ion_verify_and_write_capable_tools() {
        let registry = ReplToolRegistry::with_defaults();
        assert!(registry.get("ion_verify").is_some());
        assert!(registry.get("file_read").is_some());
        assert!(registry.get("file_write").is_some());
        assert!(registry.get("bash_exec").is_some());
        assert!(registry.get("governed_submit_claim").is_some());
        assert!(registry.get("memory_search").is_some());
        assert!(registry.get("genome_read").is_some());
        #[cfg(feature = "office-support")]
        {
            assert!(registry.get("document_read").is_some());
            assert_eq!(registry.len(), 8);
        }
        #[cfg(not(feature = "office-support"))]
        {
            assert!(registry.get("document_read").is_none());
            assert_eq!(registry.len(), 7);
        }
    }

    #[test]
    fn test_with_defaults_registers_memory_search_and_genome_read_as_ungated_reads() {
        // Stage 1b-B: memory_search/genome_read must land alongside file_read
        // and document_read (ungated, read-only) rather than bash_exec/
        // file_write (CONFIRMATION_REQUIRED_TOOLS lives in ion_repl::chat,
        // not here, but this registry is where the tool identities are
        // established -- assert the schema names line up with what that
        // gate list expects to find).
        let registry = ReplToolRegistry::with_defaults();
        let memory_search = registry
            .get("memory_search")
            .expect("memory_search registered");
        assert_eq!(memory_search.json_schema()["name"], "memory_search");
        let genome_read = registry.get("genome_read").expect("genome_read registered");
        assert_eq!(genome_read.json_schema()["name"], "genome_read");
    }

    #[test]
    fn test_list_is_sorted_by_name() {
        let registry = ReplToolRegistry::with_defaults();
        let names: Vec<&str> = registry.list().iter().map(|t| t.name()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_get_unknown_tool_returns_none() {
        let registry = ReplToolRegistry::with_defaults();
        assert!(registry.get("does_not_exist").is_none());
    }

    #[test]
    fn test_new_registry_is_empty() {
        let registry = ReplToolRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}
