//! Impulse library crate — module tree shared by the `impulse-rs` CLI binary
//! and the `ion` binary (TUI_SPEC.md T5).
//!
//! This crate has no behavior of its own beyond re-exporting the module tree;
//! `src/main.rs` (the `impulse-rs` binary) and `src/bin/ion.rs` (the `ion`
//! binary) both depend on it as `impulse_rs::...`. The split exists solely so
//! a second binary can reach `handlers::ion::handle_ion_verify` without
//! duplicating handler logic (TUI_SPEC.md T5, R2) — see CLAUDE.md's
//! "Architecture" section for the workspace layout.

pub mod cli;
pub mod token_tracker;

pub mod agent;
pub mod agent_discovery;
pub mod branding;
pub mod build_hygiene;
pub mod client;
pub mod context_lifecycle;
pub mod credentials;
pub mod daemon;
pub mod delegation;
pub mod docs;
pub mod envelope;
pub mod error;
pub mod guardrail;
pub mod handlers;
pub mod injection;
pub mod integration_tests;
pub mod ion_repl;
pub mod llm_backends;
pub mod mcp;
pub mod memory;
pub mod monty;
pub mod notification;
pub mod office;
pub mod ops_workbench;
pub mod orchestration;
pub mod plugin;
pub(crate) mod process_group;
pub mod process_util;
pub mod retrieval;
pub mod semantic_diff;
pub mod state;
pub mod stewardship;
pub mod storage;
#[cfg(test)]
mod test_support;
pub mod tooling;
pub mod tools;
pub mod ui;
pub mod validate;
pub mod verify;

// Re-export CLI types at crate root for backward compatibility.
// Existing code (e.g. handlers/daemon_dispatch.rs) imports `crate::Commands`.
pub(crate) use cli::{Commands, McpCommands};

/// Resolve the data directory.
///
/// Currently a passthrough (kept as a named function, not inlined, so
/// `main.rs` and any future callers share one place to add resolution
/// logic — e.g. `IMPULSE_HOME` fallback — without touching call sites).
pub fn resolve_impulse_dir(requested: std::path::PathBuf) -> std::path::PathBuf {
    requested
}
