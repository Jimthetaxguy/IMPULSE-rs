//! Agent Tool Discovery - Capabilities Manifest
//!
//! This module provides a mechanism for coding agents (Claude Code, Codex,
//! OpenCode) to discover the extra tools and capabilities available in Impulse.
//!
//! The capabilities are exposed via a manifest file that can be:
//! 1. Read directly by agents
//! 2. Injected into agent context
//! 3. Used for tool discovery
//!
//! ## How Agents Discover Impulse Tools
//!
//! When an agent runs inside Impulse, it can discover available tools by:
//!
//! 1. Reading `~/.impulse/impulse-capabilities.json`
//! 2. Reading the `IMPULSE_CAPABILITIES_PATH` environment variable
//! 3. Looking for injected context that includes tool info
//!
//! ## Manifest Format
//!
//! ```json
//! {
//!   "version": "1.0",
//!   "impulse_version": "0.1.0",
//!   "generated_at": "2026-02-24T18:30:00Z",
//!   "capabilities": {
//!     "tools": [...],
//!     "features": [...],
//!     "context_injection": {...}
//!   },
//!   "routing": {
//!         "claude_code": {...},
//!         "codex": {...},
//!         "opencode": {...}
//!   }
//! }
//! ```

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Version of the capabilities manifest format
pub const MANIFEST_VERSION: &str = "1.0";

/// The capabilities manifest - describes all Impulse capabilities for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesManifest {
    /// Manifest format version
    pub version: String,
    /// Impulse version
    pub impulse_version: String,
    /// When this manifest was generated
    pub generated_at: String,
    /// Available capabilities
    pub capabilities: Capabilities,
    /// Platform routing information
    pub routing: RoutingInfo,
    /// Agent interaction patterns
    pub patterns: AgentPatterns,
}

/// Available capabilities in Impulse
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    /// Dynamic tools available to agents
    pub tools: Vec<ToolCapability>,
    /// Feature flags
    pub features: Vec<FeatureCapability>,
    /// Context injection settings
    pub context_injection: ContextInjectionCapability,
    /// Session management
    pub sessions: SessionCapability,
    /// Storage capabilities
    pub storage: StorageCapability,
}

/// A tool capability available to agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    /// Tool ID
    pub id: String,
    /// Tool name
    pub name: String,
    /// Description
    pub description: String,
    /// Category
    pub category: String,
    /// Parameters
    pub params: Vec<ParamInfo>,
    /// Example usage
    pub example: Option<String>,
}

/// Parameter information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub description: String,
}

/// A feature capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

/// Context injection capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInjectionCapability {
    pub enabled: bool,
    pub modes: Vec<String>,
    pub max_chars: usize,
    pub sources: Vec<String>,
}

/// Session capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCapability {
    pub supported: bool,
    pub platforms: Vec<String>,
    pub tracking: TrackingInfo,
}

/// Tracking information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingInfo {
    pub files: bool,
    pub tools: bool,
    pub activity: bool,
}

/// Storage capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCapability {
    pub backend: String,
    pub semantic_search: bool,
    pub history_retention_days: i64,
}

/// Routing information for platform selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingInfo {
    /// When to use Claude Code
    pub claude_code: RoutingRule,
    /// When to use Codex
    pub codex: RoutingRule,
    /// When to use OpenCode
    pub opencode: RoutingRule,
}

/// Routing rule for a platform
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub description: String,
    pub keywords: Vec<String>,
    pub example_tasks: Vec<String>,
}

/// Agent interaction patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPatterns {
    /// How to invoke Impulse tools from an agent
    pub tool_invocation: String,
    /// How to get context injected
    pub context_injection: String,
    /// How to trigger handoff
    pub handoff: String,
    /// How to query session history
    pub history_query: String,
}

impl Default for CapabilitiesManifest {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilitiesManifest {
    /// Create a new capabilities manifest
    pub fn new() -> Self {
        Self {
            version: MANIFEST_VERSION.to_string(),
            impulse_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: Utc::now().to_rfc3339(),
            capabilities: Capabilities::default(),
            routing: RoutingInfo::default(),
            patterns: AgentPatterns::default(),
        }
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Write manifest to a file
    pub fn write_to_file(&self, path: &PathBuf) -> std::io::Result<()> {
        let json = self
            .to_json()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load manifest from a file
    pub fn load_from_file(path: &PathBuf) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            tools: vec![
                ToolCapability {
                    id: "session_query".to_string(),
                    name: "Session Query".to_string(),
                    description: "Search session history for cross-session awareness".to_string(),
                    category: "analysis".to_string(),
                    params: vec![
                        ParamInfo {
                            name: "query".to_string(),
                            param_type: "string".to_string(),
                            required: false,
                            description: "Text to search for".to_string(),
                        },
                        ParamInfo {
                            name: "limit".to_string(),
                            param_type: "integer".to_string(),
                            required: false,
                            description: "Max results (default: 10)".to_string(),
                        },
                    ],
                    example: Some("impulse-rs tooling-run --tool-id session_query --params '{\"query\": \"rust\"}'".to_string()),
                },
                ToolCapability {
                    id: "memory_search".to_string(),
                    name: "Memory Search".to_string(),
                    description: "Search Impulse's long-term memory (genome)".to_string(),
                    category: "analysis".to_string(),
                    params: vec![
                        ParamInfo {
                            name: "query".to_string(),
                            param_type: "string".to_string(),
                            required: true,
                            description: "Search query".to_string(),
                        },
                    ],
                    example: Some("impulse-rs tooling-run --tool-id memory_search --params '{\"query\": \"architecture\"}'".to_string()),
                },
                ToolCapability {
                    id: "config_get".to_string(),
                    name: "Config Get".to_string(),
                    description: "Get Impulse configuration values".to_string(),
                    category: "utility".to_string(),
                    params: vec![
                        ParamInfo {
                            name: "key".to_string(),
                            param_type: "string".to_string(),
                            required: true,
                            description: "Config key to retrieve".to_string(),
                        },
                    ],
                    example: Some("impulse-rs config log_level".to_string()),
                },
                ToolCapability {
                    id: "calculator".to_string(),
                    name: "Calculator".to_string(),
                    description: "Evaluate mathematical expressions".to_string(),
                    category: "utility".to_string(),
                    params: vec![
                        ParamInfo {
                            name: "expression".to_string(),
                            param_type: "string".to_string(),
                            required: true,
                            description: "Mathematical expression".to_string(),
                        },
                    ],
                    example: Some("impulse-rs calc --expression '2^10'".to_string()),
                },
                ToolCapability {
                    id: "python_exec".to_string(),
                    name: "Python Execute".to_string(),
                    description: "Execute Python code".to_string(),
                    category: "utility".to_string(),
                    params: vec![
                        ParamInfo {
                            name: "code".to_string(),
                            param_type: "string".to_string(),
                            required: true,
                            description: "Python code to execute".to_string(),
                        },
                    ],
                    example: Some("impulse-rs exec --code 'print(\"hello\")'".to_string()),
                },
            ],
            features: vec![
                FeatureCapability {
                    id: "retrieval".to_string(),
                    name: "Semantic Retrieval".to_string(),
                    description: "Search across session history and genome with semantic similarity".to_string(),
                    enabled: true,
                },
                FeatureCapability {
                    id: "context_injection".to_string(),
                    name: "Context Injection".to_string(),
                    description: "Automatically inject relevant context into agent sessions".to_string(),
                    enabled: true,
                },
                FeatureCapability {
                    id: "stewardship".to_string(),
                    name: "Token Stewardship".to_string(),
                    description: "Monitor and manage context window usage".to_string(),
                    enabled: true,
                },
                FeatureCapability {
                    id: "build_hygiene".to_string(),
                    name: "Build Hygiene".to_string(),
                    description: "Clean up Rust build artifacts to save disk space".to_string(),
                    enabled: true,
                },
                FeatureCapability {
                    id: "office".to_string(),
                    name: "Office Documents".to_string(),
                    description: "Parse and extract data from Excel, Word documents".to_string(),
                    enabled: true,
                },
                FeatureCapability {
                    id: "monty".to_string(),
                    name: "Monty Routing".to_string(),
                    description: "Computed routing between platforms using AI".to_string(),
                    enabled: true,
                },
            ],
            context_injection: ContextInjectionCapability {
                enabled: true,
                modes: vec!["off".to_string(), "review".to_string(), "apply".to_string()],
                max_chars: 2000,
                sources: vec!["history".to_string(), "genome".to_string()],
            },
            sessions: SessionCapability {
                supported: true,
                platforms: vec!["claude-code".to_string(), "opencode".to_string()],
                tracking: TrackingInfo {
                    files: true,
                    tools: true,
                    activity: true,
                },
            },
            storage: StorageCapability {
                backend: "sqlite".to_string(),
                semantic_search: true,
                history_retention_days: 90,
            },
        }
    }
}

impl Default for RoutingInfo {
    fn default() -> Self {
        Self {
            claude_code: RoutingRule {
                description: "Complex architecture, planning, debugging, code review".to_string(),
                keywords: vec![
                    "architecture".to_string(),
                    "design".to_string(),
                    "review".to_string(),
                    "refactor".to_string(),
                    "debug".to_string(),
                ],
                example_tasks: vec![
                    "Design a new system".to_string(),
                    "Review this PR".to_string(),
                    "Debug this crash".to_string(),
                ],
            },
            codex: RoutingRule {
                description: "Rapid execution, implementation, deployment".to_string(),
                keywords: vec![
                    "build".to_string(),
                    "implement".to_string(),
                    "deploy".to_string(),
                    "create".to_string(),
                ],
                example_tasks: vec![
                    "Build this feature".to_string(),
                    "Create a new API".to_string(),
                    "Deploy to production".to_string(),
                ],
            },
            opencode: RoutingRule {
                description: "Lightweight tasks, verification, quick fixes".to_string(),
                keywords: vec![
                    "verify".to_string(),
                    "check".to_string(),
                    "fix".to_string(),
                    "quick".to_string(),
                ],
                example_tasks: vec![
                    "Verify tests pass".to_string(),
                    "Quick bug fix".to_string(),
                    "Check for errors".to_string(),
                ],
            },
        }
    }
}

impl Default for AgentPatterns {
    fn default() -> Self {
        Self {
            tool_invocation: "Use `impulse-rs tooling-run --tool-id <tool> --params '<json>'` to invoke tools".to_string(),
            context_injection: "Context is automatically injected based on session activity. Use `impulse-rs compute-injection` to compute what would be injected.".to_string(),
            handoff: "Use `impulse-rs handoff --tool <platform> --task '<task>'` to hand off to another platform".to_string(),
            history_query: "Use `impulse-rs search-history --query '<query>'` to search past sessions".to_string(),
        }
    }
}

/// Generate a capabilities summary for context injection
pub fn generate_capabilities_summary() -> String {
    // Create a brief summary for injection
    r#"## Impulse Capabilities

### Available Tools
- **session_query**: Search session history
- **memory_search**: Search long-term memory (genome)
- **config_get**: Get configuration values
- **calculator**: Evaluate math expressions
- **python_exec**: Execute Python code

### Features
- Semantic retrieval across history and genome
- Context injection (modes: off/review/apply)
- Token stewardship for context management
- Build hygiene for Rust artifacts
- Office document parsing (Excel, Word, CSV)
- Monty AI routing between platforms

### Platform Routing
- Claude Code: architecture, design, review, refactor, debug
- Codex: build, implement, deploy, create
- OpenCode: verify, check, fix, quick tasks

### Usage
Run `impulse-rs tooling-list` to see all available tools.
Run `impulse-rs tooling-describe --tool-id <tool>` for details.
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_creation() {
        let manifest = CapabilitiesManifest::new();
        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert!(!manifest.impulse_version.is_empty());
    }

    #[test]
    fn test_manifest_json_serialization() {
        let manifest = CapabilitiesManifest::new();
        let json = manifest.to_json();
        assert!(json.is_ok());

        let deserialized: CapabilitiesManifest = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(deserialized.version, MANIFEST_VERSION);
    }

    #[test]
    fn test_capabilities_summary() {
        let summary = generate_capabilities_summary();
        assert!(summary.contains("Impulse Capabilities"));
        assert!(summary.contains("session_query"));
        assert!(summary.contains("Claude Code"));
    }
}
