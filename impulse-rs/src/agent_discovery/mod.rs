//! Agent Tool Discovery - Capabilities Manifest
//!
//! Registry-derived manifest describing Impulse capabilities for agents.

use chrono::Utc;
use impulse_ops::agent_registry::AgentRegistry;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::storage::Storage;
use crate::tooling::{ManifestTool, ToolRegistry};

/// Version of the capabilities manifest format.
pub const MANIFEST_VERSION: &str = "2.0";
pub const DEFAULT_MANIFEST_FILE: &str = "impulse-capabilities.json";

/// The capabilities manifest — describes Impulse capabilities for agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesManifest {
    pub version: String,
    pub impulse_version: String,
    pub generated_at: String,
    pub tools: Vec<ManifestTool>,
    pub features: Vec<String>,
    /// Coding-agent platforms Impulse can monitor and drive. `#[serde(default)]`
    /// keeps manifests written before this field was added loadable.
    #[serde(default)]
    pub supported_agents: Vec<String>,
}

impl Default for CapabilitiesManifest {
    fn default() -> Self {
        Self::from_registries(&ToolRegistry::with_defaults(), &AgentRegistry::builtin())
    }
}

impl CapabilitiesManifest {
    pub fn from_registry(registry: &ToolRegistry) -> Self {
        Self::from_registries(registry, &AgentRegistry::builtin())
    }

    pub fn from_registries(registry: &ToolRegistry, agent_registry: &AgentRegistry) -> Self {
        let tools = registry.manifest_tools();
        let mut features = vec![
            "retrieval".to_string(),
            "context_injection".to_string(),
            "stewardship".to_string(),
            "build_hygiene".to_string(),
        ];
        if tools.iter().any(|tool| tool.category == "document") {
            features.push("office".to_string());
        }

        Self {
            version: MANIFEST_VERSION.to_string(),
            impulse_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: Utc::now().to_rfc3339(),
            tools,
            features,
            supported_agents: agent_registry
                .agents()
                .iter()
                .map(|agent| agent.id.to_string())
                .collect(),
        }
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        use anyhow::Context;
        let content = std::fs::read_to_string(path).with_context(|| {
            format!("failed to read capabilities manifest at {}", path.display())
        })?;
        serde_json::from_str(&content).with_context(|| {
            format!(
                "failed to parse capabilities manifest at {}",
                path.display()
            )
        })
    }
}

pub fn write_capabilities_manifest(
    base_path: &Path,
    registry: &ToolRegistry,
) -> anyhow::Result<std::path::PathBuf> {
    let agent_registry = AgentRegistry::registry_for_runtime()?;
    let manifest = CapabilitiesManifest::from_registries(registry, &agent_registry);
    let path = base_path.join(DEFAULT_MANIFEST_FILE);
    if let Ok(existing) = CapabilitiesManifest::load(&path) {
        let mut existing_semantics = serde_json::to_value(&existing)?;
        let mut refreshed_semantics = serde_json::to_value(&manifest)?;
        if let Some(object) = existing_semantics.as_object_mut() {
            object.remove("generated_at");
        }
        if let Some(object) = refreshed_semantics.as_object_mut() {
            object.remove("generated_at");
        }
        if existing_semantics == refreshed_semantics {
            // `generated_at` describes when this semantic manifest was first
            // produced, not daemon startup time. Keeping the existing file
            // avoids dirtying a governed Git subject with a timestamp-only
            // rewrite on every launch.
            return Ok(path);
        }
    }
    Storage::atomic_write_path(&path, manifest.to_json()?.as_bytes())?;
    Ok(path)
}

/// Generate a capabilities summary for context injection.
pub fn generate_capabilities_summary() -> String {
    if let Ok(path) = std::env::var("IMPULSE_CAPABILITIES_PATH") {
        if let Ok(manifest) = CapabilitiesManifest::load(Path::new(&path)) {
            return summary_from_manifest(&manifest);
        }
    }

    summary_from_manifest(&CapabilitiesManifest::default())
}

fn summary_from_manifest(manifest: &CapabilitiesManifest) -> String {
    let mut out = String::new();
    out.push_str("## Impulse Capabilities\n\n");
    out.push_str("### Available Tools\n");
    if manifest.tools.is_empty() {
        out.push_str("- (no tools registered)\n");
    } else {
        for tool in &manifest.tools {
            out.push_str(&format!(
                "- **{}**: {} [{}]\n",
                tool.id, tool.description, tool.source
            ));
        }
    }

    if !manifest.features.is_empty() {
        out.push_str("\n### Features\n");
        for feature in &manifest.features {
            out.push_str(&format!("- {}\n", feature));
        }
    }

    if !manifest.supported_agents.is_empty() {
        out.push_str("\n### Monitored Agents\n");
        out.push_str(&format!(
            "Impulse can detect, monitor, and drive: {}\n",
            manifest.supported_agents.join(", ")
        ));
    }

    out.push_str("\n### Usage\n");
    out.push_str("Run `impulse-rs tooling-list` to see all available tools.\n");
    out.push_str("Run `impulse-rs tooling-describe <tool-id>` for details.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_creation() {
        let manifest = CapabilitiesManifest::default();
        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert!(!manifest.impulse_version.is_empty());
        assert!(!manifest.tools.is_empty());
        assert!(!manifest.features.is_empty());
    }

    #[test]
    fn test_manifest_json_serialization() {
        let manifest = CapabilitiesManifest::default();
        let json = manifest.to_json();
        assert!(json.is_ok());

        let deserialized: CapabilitiesManifest = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(deserialized.version, MANIFEST_VERSION);
    }

    #[test]
    fn test_capabilities_summary() {
        let summary = generate_capabilities_summary();
        assert!(summary.contains("Impulse Capabilities"));
        assert!(summary.contains("Available Tools"));
    }

    #[test]
    fn test_manifest_lists_supported_agents() {
        let manifest = CapabilitiesManifest::default();
        // The agents added across the multi-agent work (claude/codex/opencode/
        // gemini/cursor) are advertised so peers can discover interop.
        for agent in [
            "claude-code",
            "codex",
            "opencode",
            "gemini",
            "cursor",
            "ion",
        ] {
            assert!(
                manifest.supported_agents.iter().any(|a| a == agent),
                "manifest should advertise support for {agent}"
            );
        }
    }

    #[test]
    fn unchanged_manifest_preserves_generated_at_without_rewrite() {
        let temp = tempfile::TempDir::new().unwrap();
        let registry = ToolRegistry::with_defaults();
        let path = write_capabilities_manifest(temp.path(), &registry).unwrap();
        let mut existing = CapabilitiesManifest::load(&path).unwrap();
        existing.generated_at = "2026-07-14T00:00:00Z".to_string();
        Storage::atomic_write_path(&path, existing.to_json().unwrap().as_bytes()).unwrap();
        #[cfg(unix)]
        let inode_before = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&path).unwrap().ino()
        };

        let refreshed_path = write_capabilities_manifest(temp.path(), &registry).unwrap();
        let refreshed = CapabilitiesManifest::load(&refreshed_path).unwrap();

        assert_eq!(refreshed.generated_at, "2026-07-14T00:00:00Z");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode_before);
        }
    }

    #[test]
    fn test_summary_includes_monitored_agents() {
        let summary = summary_from_manifest(&CapabilitiesManifest::default());
        assert!(summary.contains("Monitored Agents"));
        assert!(summary.contains("gemini"));
        assert!(summary.contains("cursor"));
        assert!(summary.contains("ion"));
    }

    #[test]
    fn test_manifest_can_be_driven_by_a_custom_agent_registry() {
        let agents = AgentRegistry::from_toml_str(
            r#"
[[agent]]
id = "custom-agent"
label = "Custom Agent"
command = "custom-agent"
"#,
        )
        .unwrap();
        let manifest =
            CapabilitiesManifest::from_registries(&ToolRegistry::with_defaults(), &agents);

        assert_eq!(manifest.supported_agents, vec!["custom-agent"]);
    }

    #[test]
    fn test_manifest_loads_without_supported_agents_field() {
        // Backward compatibility: a manifest written before supported_agents
        // existed (the field absent) must still deserialize.
        let legacy = r#"{
            "version": "2.0",
            "impulse_version": "0.1.0",
            "generated_at": "2026-01-01T00:00:00Z",
            "tools": [],
            "features": ["retrieval"]
        }"#;
        let manifest: CapabilitiesManifest = serde_json::from_str(legacy).unwrap();
        assert!(manifest.supported_agents.is_empty());
        assert_eq!(manifest.version, "2.0");
    }
}
