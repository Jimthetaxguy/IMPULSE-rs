//! Agent Tool Discovery - Capabilities Manifest
//!
//! Minimal manifest describing Impulse capabilities for agent context injection.
//! Routing rules live in `orchestration/`; this module only provides the
//! structured manifest and the summary string used by the context injector.

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Version of the capabilities manifest format.
pub const MANIFEST_VERSION: &str = "1.0";

/// The capabilities manifest — describes Impulse capabilities for agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesManifest {
    pub version: String,
    pub impulse_version: String,
    pub generated_at: String,
    pub tools: Vec<String>,
    pub features: Vec<String>,
}

impl Default for CapabilitiesManifest {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilitiesManifest {
    pub fn new() -> Self {
        Self {
            version: MANIFEST_VERSION.to_string(),
            impulse_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: Utc::now().to_rfc3339(),
            tools: vec![
                "session_query".to_string(),
                "memory_search".to_string(),
                "config_get".to_string(),
                "calculator".to_string(),
                "python_exec".to_string(),
            ],
            features: vec![
                "retrieval".to_string(),
                "context_injection".to_string(),
                "stewardship".to_string(),
                "build_hygiene".to_string(),
                "office".to_string(),
            ],
        }
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Generate a capabilities summary for context injection.
pub fn generate_capabilities_summary() -> String {
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
        assert_eq!(manifest.tools.len(), 5);
        assert_eq!(manifest.features.len(), 5);
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
    }
}
