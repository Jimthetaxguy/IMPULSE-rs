//! `ReplTool` registry for the ion REPL (TUI_SPEC.md T7).
//!
//! Owns the set of tools available to the chat loop, powers `/tools`
//! (listing), and lets `/verify` dispatch through the registry (per
//! TUI_SPEC.md section 2.3: "`/verify` calls the `ion_verify` ReplTool
//! directly") instead of a hardcoded call.
//!
//! Registers two capability universes side by side (TUI_SPEC.md section
//! 2.3's "Scope clarification"): `ion_verify`, the read-only spec-a gate
//! tool, and write-capable tools bridged from the existing
//! `src/tooling::Tool` registry (`file_read`, `file_write`, `bash_exec` --
//! see `tool_bridge::DynamicToolBridge`). The verify gate's closed
//! read-only allowlist and the REPL's full coding-agent tool surface are
//! kept conceptually separate; this registry simply holds both.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::tooling::ToolRegistry;

use super::tool_bridge::DynamicToolBridge;
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

    /// Builds the default ion REPL tool set: `ion_verify` plus `file_read`,
    /// `file_write`, and `bash_exec` bridged from
    /// `src/tooling::ToolRegistry::with_defaults()`.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(IonVerifyTool));

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
            dynamic,
            "bash_exec",
            "bash_exec {\"command\": \"...\", \"cwd\": \"...\", \"timeout_secs\": 30} \
             -- run a shell command",
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
        assert_eq!(registry.len(), 4);
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
