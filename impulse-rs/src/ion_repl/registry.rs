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

use super::tool_blackboard::{BlackboardFetchTool, BlackboardStoreTool};
use super::tool_bridge::DynamicToolBridge;
use super::tool_claim::GovernedSubmitClaimTool;
#[cfg(feature = "office-support")]
use super::tool_document::DocumentReadTool;
#[cfg(feature = "photon-subagent")]
use super::tool_photon::PhotonTool;
use super::tool_search::{SearchToolsTool, ToolDescriptor};
use super::tool_verify::IonVerifyTool;
use super::tools::ReplTool;

/// The orchestrator's whole advertised tool surface (ADR-0023): discover
/// tools, park and page results off-context, hand work to a worker, and pass
/// an approval gate. Everything else is reached through `search_tools`.
/// `search_tools`, `blackboard_store` and `blackboard_fetch` are registered
/// in [`ReplToolRegistry::with_defaults`]; `delegate_task` and
/// `approve_gate` are reserved names for the orchestrator role, which is not
/// built yet.
pub const ORCHESTRATOR_TOOL_SURFACE: [&str; 5] = [
    "search_tools",
    "blackboard_store",
    "blackboard_fetch",
    "delegate_task",
    "approve_gate",
];

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

    /// Register a tool by its own `name()`. Duplicate names are an error --
    /// same contract as `src/tooling::ToolRegistry::register` -- so a later
    /// plugin cannot silently steal `governed_submit_claim` or `file_write`.
    pub fn register(&mut self, tool: Box<dyn ReplTool>) -> Result<(), String> {
        let name = tool.name();
        if self.tools.contains_key(name) {
            return Err(format!("duplicate ReplTool name {name}"));
        }
        self.tools.insert(name, tool);
        Ok(())
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
    /// `genome_read` bridged from `src/tooling::ToolRegistry::with_defaults()`,
    /// then ADR-0023's `blackboard_store` and `blackboard_fetch`, and last
    /// `search_tools`, whose catalog is a snapshot of everything before it.
    ///
    /// `memory_search`/`genome_read` are read-only (`Capability::FileSystemRead`
    /// only) and stay outside `CONFIRMATION_REQUIRED_TOOLS` like `file_read`
    /// and `document_read`. Bridging them through `DynamicToolBridge` (rather
    /// than a bespoke `ReplTool`) is most of what gives them the sandbox for
    /// free: both declare their `impulse_dir` parameter as
    /// `ParamType::FilePath`, so `ToolRegistry::execute`'s generic
    /// `validate_paths` step (`src/tooling/executor.rs`) already checks an
    /// EXPLICITLY-supplied `impulse_dir` against `ctx.sandbox_tool_context()`
    /// the same way it checks `file_read`'s `path`, with no extra code
    /// needed for that case.
    ///
    /// **Correction (review round 1, P2-2/P2-3 on PR #54):** this doc
    /// comment originally also claimed the OMITTED case needed no new code.
    /// It did: `validate_paths` only checks parameters a caller actually
    /// supplied, so when `impulse_dir` was omitted the sandbox check simply
    /// did not run, and each tool's own fallback -- the bare literal
    /// `".impulse"`, resolved relative to the process's own working
    /// directory -- had no relationship to `IMPULSE_HOME`/the sandbox at
    /// all. Both tools were changed to default from `ctx.impulse_dir`
    /// instead (see their own `execute` methods), and
    /// `ReplContext::sandbox_tool_context` now sets that field from
    /// `history::impulse_home()` and adds it to the read roots explicitly
    /// -- so the default now actually resolves to a directory this context
    /// grants, rather than to an unchecked, possibly-wrong one.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry
            .register(Box::new(IonVerifyTool))
            .expect("default ion_verify");
        registry
            .register(Box::new(GovernedSubmitClaimTool))
            .expect("default governed_submit_claim");
        #[cfg(feature = "office-support")]
        registry
            .register(Box::new(DocumentReadTool))
            .expect("default document_read");
        #[cfg(feature = "photon-subagent")]
        registry
            .register(Box::new(PhotonTool::from_env()))
            .expect("default photon");

        let dynamic = Arc::new(ToolRegistry::with_defaults());
        registry
            .register(Box::new(DynamicToolBridge::new(
                Arc::clone(&dynamic),
                "file_read",
                "file_read {\"path\": \"...\", \"start_line\": 1, \"max_lines\": 200} \
             -- read a file",
            )))
            .expect("default file_read");
        registry
            .register(Box::new(DynamicToolBridge::new(
                Arc::clone(&dynamic),
                "file_write",
                "file_write {\"path\": \"...\", \"content\": \"...\"} \
             -- atomically write (create/overwrite) a file",
            )))
            .expect("default file_write");
        registry
            .register(Box::new(DynamicToolBridge::new(
                Arc::clone(&dynamic),
                "bash_exec",
                "bash_exec {\"command\": \"...\", \"cwd\": \"...\", \"timeout_secs\": 30} \
             -- run a shell command",
            )))
            .expect("default bash_exec");
        registry
            .register(Box::new(DynamicToolBridge::new(
                Arc::clone(&dynamic),
                "memory_search",
                "memory_search {\"query\": \"...\", \"scope\": \"all\", \"mode\": \"keyword\", \
             \"limit\": 5} -- search GENOME decisions and session history",
            )))
            .expect("default memory_search");
        registry
            .register(Box::new(DynamicToolBridge::new(
                dynamic,
                "genome_read",
                "genome_read {\"section\": \"...\"} -- read permanent project decisions \
             and preferences from GENOME.md",
            )))
            .expect("default genome_read");
        registry
            .register(Box::new(BlackboardStoreTool))
            .expect("default blackboard_store");
        registry
            .register(Box::new(BlackboardFetchTool))
            .expect("default blackboard_fetch");

        // Last, so its catalog snapshot covers every tool registered above.
        let catalog = registry
            .list()
            .into_iter()
            .map(ToolDescriptor::of)
            .collect();
        registry
            .register(Box::new(SearchToolsTool::new(catalog)))
            .expect("default search_tools");

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
    use async_trait::async_trait;
    use serde_json::Value;

    use super::super::tools::{ReplTool, ToolOutcome};
    use super::super::ReplContext;

    struct NamedUsageTool {
        name: &'static str,
        usage: &'static str,
    }

    #[async_trait]
    impl ReplTool for NamedUsageTool {
        fn name(&self) -> &'static str {
            self.name
        }
        fn usage(&self) -> &'static str {
            self.usage
        }
        fn json_schema(&self) -> Value {
            serde_json::json!({"name": self.name})
        }
        async fn run(&self, _args: Value, _ctx: &ReplContext) -> anyhow::Result<ToolOutcome> {
            Ok(ToolOutcome {
                rendered: String::new(),
                payload: Value::Null,
                ok: true,
            })
        }
    }

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
        assert!(registry.get("blackboard_store").is_some());
        assert!(registry.get("blackboard_fetch").is_some());
        assert!(registry.get("search_tools").is_some());
        assert_eq!(
            registry.get("document_read").is_some(),
            cfg!(feature = "office-support")
        );
        assert_eq!(
            registry.get("photon").is_some(),
            cfg!(feature = "photon-subagent")
        );
        let expected = 10
            + usize::from(cfg!(feature = "office-support"))
            + usize::from(cfg!(feature = "photon-subagent"));
        assert_eq!(registry.len(), expected);
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
    fn test_default_tools_schema_name_matches_name() {
        let registry = ReplToolRegistry::with_defaults();
        for tool in registry.list() {
            let schema = tool.json_schema();
            let schema_name = schema.get("name").and_then(|v| v.as_str());
            assert_eq!(
                schema_name,
                Some(tool.name()),
                "dispatch name and schema name must be the same identity"
            );
        }
    }

    #[test]
    fn test_orchestrator_surface_stays_at_five_tools() {
        assert_eq!(ORCHESTRATOR_TOOL_SURFACE.len(), 5);
        let unique: std::collections::BTreeSet<&str> =
            ORCHESTRATOR_TOOL_SURFACE.iter().copied().collect();
        assert_eq!(unique.len(), 5, "no duplicate names");
        let registry = ReplToolRegistry::with_defaults();
        for name in ["search_tools", "blackboard_store", "blackboard_fetch"] {
            assert!(ORCHESTRATOR_TOOL_SURFACE.contains(&name));
            assert!(registry.get(name).is_some(), "{name} is registered");
        }
    }

    #[tokio::test]
    async fn test_search_tools_catalog_covers_every_default_tool() {
        let registry = ReplToolRegistry::with_defaults();
        let search = registry.get("search_tools").expect("search_tools");
        let outcome = search
            .run(
                serde_json::json!({"query": "", "limit": 20}),
                &ReplContext::default(),
            )
            .await
            .expect("list all");
        let found: Vec<&str> = outcome.payload["results"]
            .as_array()
            .expect("results")
            .iter()
            .filter_map(|entry| entry["name"].as_str())
            .collect();
        for tool in registry.list() {
            assert!(
                found.contains(&tool.name()),
                "{} is searchable",
                tool.name()
            );
        }
    }

    #[test]
    fn test_register_rejects_duplicate_name() {
        let mut registry = ReplToolRegistry::new();
        registry
            .register(Box::new(IonVerifyTool))
            .expect("first ion_verify");
        let err = registry
            .register(Box::new(IonVerifyTool))
            .expect_err("second ion_verify must not steal the name");
        assert!(
            err.contains("ion_verify"),
            "duplicate error should name the tool, got {err}"
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_register_keeps_first_instance_on_duplicate_name() {
        let mut registry = ReplToolRegistry::new();
        registry
            .register(Box::new(NamedUsageTool {
                name: "dup",
                usage: "first",
            }))
            .expect("first dup");
        let err = registry
            .register(Box::new(NamedUsageTool {
                name: "dup",
                usage: "second",
            }))
            .expect_err("second dup must not steal the name");
        assert!(err.contains("dup"), "got {err}");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get("dup").expect("first remains").usage(), "first");
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
