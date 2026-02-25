//! Monty integration module for Impulse
//!
//! This module provides computed routing and dynamic context injection capabilities
//! using pydantic-monty, a minimal Python interpreter written in Rust.
//!
//! ## Features
//!
//! - **Computed Routing**: LLM-generated routing logic evaluated in a sandbox
//! - **Dynamic Injection**: Intelligent context bundle selection
//! - **KDB Integration**: Extract and query knowledge database
//! - **Resource Limits**: Memory, time, and stack depth controls
//!
//! ## Usage
//!
//! Enable Monty support in Cargo.toml:
//! ```toml
//! [features]
//! monty-support = ["dep:pyo3"]
//! ```
//!
//! Install the Python dependency:
//! ```bash
//! pip install pydantic-monty
//! ```
//!
//! ## Architecture
//!
//! The module provides external functions that Monty's sandboxed Python code
//! can call to access Impulse's context:
//!
//! - `route_to(tool_name)` - Route to a specific tool
//! - `search_history(query, limit)` - Search session history
//! - `get_genome_decisions(topic)` - Get genome decisions
//! - `inject(context, priority)` - Mark context for injection
//! - `extract_findings(content)` - Extract findings from content
//! - `search_similar(query, limit)` - Search for similar sessions

mod datafusion;
mod kdb;
mod routing;
mod swarm;

pub mod kdb_extraction {
    //! KDB extraction helpers
    pub use super::kdb::*;
}

pub mod swarm_coordination {
    //! SWARM coordination helpers
    pub use super::swarm::*;
}

pub mod analytics {
    //! DataFusion analytics helpers
    pub use super::datafusion::*;
}

use serde::{Deserialize, Serialize};

mod python;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingTarget {
    ClaudeCode,
    Codex,
    OpenCode,
    Gemini,
    ChatGPT,
}

impl RoutingTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoutingTarget::ClaudeCode => "claude-code",
            RoutingTarget::Codex => "codex",
            RoutingTarget::OpenCode => "opencode",
            RoutingTarget::Gemini => "gemini",
            RoutingTarget::ChatGPT => "chatgpt",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedRoute {
    pub target: RoutingTarget,
    pub confidence: f64,
    pub reasoning: String,
    pub functions_called: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionDecision {
    pub context_type: String,
    pub priority: String,
    pub reasoning: String,
}

/// Result of executing Monty code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub functions_called: Vec<String>,
    pub resource_usage: ResourceUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    pub memory_bytes: u64,
    pub execution_time_ms: u64,
    pub stack_depth: u32,
}

/// Monty configuration
#[derive(Debug, Clone)]
pub struct MontyConfig {
    /// Maximum memory allocation in bytes
    pub max_memory_bytes: u64,
    /// Maximum execution time in milliseconds
    pub max_execution_time_ms: u64,
    /// Maximum stack depth
    pub max_stack_depth: u32,
    /// Enable external functions (search_history, get_genome_decisions, etc.)
    pub enable_external_functions: bool,
}

impl Default for MontyConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 10 * 1024 * 1024, // 10MB
            max_execution_time_ms: 1000,        // 1 second
            max_stack_depth: 100,
            enable_external_functions: true,
        }
    }
}

/// Primary computed routing entry point.
///
/// Uses keyword-based routing. Future: `#[cfg(feature = "monty-support")]`
/// enables PyO3 computed routing.
pub fn execute_computed_routing(
    context: &str,
    _config: &MontyConfig,
) -> Result<ComputedRoute, String> {
    routing::route_by_keywords(context)
}

/// Primary injection selection entry point.
///
/// Uses keyword-based selection. Future: `#[cfg(feature = "monty-support")]`
/// enables PyO3 computed injection.
pub fn execute_injection_selection(
    context: &str,
    _config: &MontyConfig,
) -> Result<Vec<InjectionDecision>, String> {
    routing::select_injection_by_keywords(context)
}

/// Check if PyO3 Monty support is compiled in
pub fn is_monty_available() -> bool {
    cfg!(feature = "monty-support")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_target_as_str() {
        assert_eq!(RoutingTarget::ClaudeCode.as_str(), "claude-code");
        assert_eq!(RoutingTarget::Codex.as_str(), "codex");
        assert_eq!(RoutingTarget::OpenCode.as_str(), "opencode");
    }

    #[test]
    fn test_default_config() {
        let config = MontyConfig::default();
        assert_eq!(config.max_memory_bytes, 10 * 1024 * 1024);
        assert_eq!(config.max_execution_time_ms, 1000);
        assert!(config.enable_external_functions);
    }

    #[test]
    fn test_keyword_routing() {
        let result = execute_computed_routing("task: architecture review", &MontyConfig::default());
        assert!(result.is_ok());
    }
}
