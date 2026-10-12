//! Export Impulse `ToolRegistry` schemas as ElevenLabs client-tool configs.
//!
//! Client tools are registered on the agent (dashboard / API) with case-sensitive
//! names matching Impulse tool ids. This module builds that registration payload
//! from the live registry + voice exposure policy — same source of truth as MCP
//! `tools/list`.

use serde::{Deserialize, Serialize};

use crate::tooling::{ToolRegistry, ToolSource};

use super::policy::{classify_tool_risk, VoicePolicy, VoiceToolRisk};

/// ElevenLabs client-tool registration shape (subset used for config export).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElevenLabsClientToolSchema {
    /// Case-sensitive tool name (Impulse tool id).
    pub name: String,
    pub description: String,
    /// JSON Schema object for parameters.
    pub parameters: serde_json::Value,
    /// Whether the agent should wait for a tool result in context.
    pub wait_for_response: bool,
    /// Impulse risk class for operators (not an EL wire field).
    pub impulse_risk: String,
    pub impulse_source: String,
    pub impulse_capabilities: Vec<String>,
}

/// Build client-tool schemas for tools the voice policy exposes.
pub fn elevenlabs_client_tool_schemas(
    registry: &ToolRegistry,
    policy: &VoicePolicy,
) -> Vec<ElevenLabsClientToolSchema> {
    let registry_schemas = registry.schema_json();
    let mut out = Vec::new();

    for tool_json in registry_schemas {
        let name = tool_json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        if let Some(exposed) = &policy.exposed_tools {
            if !exposed.iter().any(|t| t == &name) {
                continue;
            }
        }

        let Some(tool) = registry.get(&name) else {
            continue;
        };
        let risk = classify_tool_risk(tool);
        let risk_label = match risk {
            VoiceToolRisk::ReadOnly => "read_only",
            VoiceToolRisk::Mutating => "mutating",
        };
        let source = registry
            .source(&name)
            .unwrap_or(ToolSource::Builtin)
            .as_str()
            .to_string();
        let caps = tool_json
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let description = tool_json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let parameters = tool_json
            .get("input_schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));

        // Mutating tools still export so the agent can request them; the
        // voice policy denies execution unless confirmed.
        let desc = if risk == VoiceToolRisk::Mutating {
            format!("{description} [Impulse: mutating — requires confirmation on voice path]")
        } else {
            description
        };

        out.push(ElevenLabsClientToolSchema {
            name,
            description: desc,
            parameters,
            wait_for_response: true,
            impulse_risk: risk_label.into(),
            impulse_source: source,
            impulse_capabilities: caps,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooling::ToolRegistry;

    #[test]
    fn schemas_include_system_info_with_wait_for_response() {
        let registry = ToolRegistry::with_defaults();
        let policy = VoicePolicy::default();
        let schemas = elevenlabs_client_tool_schemas(&registry, &policy);
        let sys = schemas
            .iter()
            .find(|s| s.name == "system_info")
            .expect("system_info schema");
        assert!(sys.wait_for_response);
        assert_eq!(sys.impulse_risk, "read_only");
        assert!(sys.parameters.is_object());
    }

    #[test]
    fn bash_exec_exported_as_mutating() {
        let registry = ToolRegistry::with_defaults();
        let policy = VoicePolicy::default();
        let schemas = elevenlabs_client_tool_schemas(&registry, &policy);
        let bash = schemas
            .iter()
            .find(|s| s.name == "bash_exec")
            .expect("bash_exec schema");
        assert_eq!(bash.impulse_risk, "mutating");
        assert!(bash.description.contains("mutating"));
    }
}
