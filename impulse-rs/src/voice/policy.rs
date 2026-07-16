//! Voice-path confirmation / deny-by-default policy for mutating tools.

use crate::tooling::{Capability, DynamicTool, ToolRegistry};

/// Risk class for voice exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceToolRisk {
    /// Read-oriented: SystemInfo / FileSystemRead only (or no special caps).
    ReadOnly,
    /// Write/shell/python/network — requires explicit confirmation.
    Mutating,
}

/// Decision from the voice policy gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoicePolicyDecision {
    Allow,
    Deny { reason: String },
}

/// Policy applied before any side-effecting tool runs on the voice path.
#[derive(Debug, Clone)]
pub struct VoicePolicy {
    /// When false (default), mutating tools are always denied without confirm.
    pub allow_confirmed_mutations: bool,
    /// Optional allowlist of tool ids; empty means all registered tools may be
    /// *considered*, subject to risk class.
    pub exposed_tools: Option<Vec<String>>,
}

impl Default for VoicePolicy {
    fn default() -> Self {
        Self {
            allow_confirmed_mutations: true,
            exposed_tools: Some(DEFAULT_VOICE_EXPOSED_TOOLS.iter().map(|s| (*s).into()).collect()),
        }
    }
}

/// Default tools advertised for ElevenLabs agent configuration (read-oriented
/// plus gated mutators listed for schema completeness).
pub const DEFAULT_VOICE_EXPOSED_TOOLS: &[&str] = &[
    // Read-oriented (SystemInfo / FileSystemRead):
    "system_info",
    "health_check",
    "config_get",
    "steward_status",
    "session_query",
    "memory_search",
    "file_read",
    "genome_read",
    "build_health",
    "sccache_status",
    "tool_availability",
    // Mutating — still gated (require confirmation):
    "calculator", // PythonExec
    "file_write",
    "bash_exec",
    "python_exec",
];

/// Caps that mark a tool as mutating on the voice path.
pub fn is_mutating_capability(cap: Capability) -> bool {
    matches!(
        cap,
        Capability::FileSystemWrite
            | Capability::ShellExec
            | Capability::PythonExec
            | Capability::Network
    )
}

/// Classify a tool by its required capabilities.
pub fn classify_tool_risk(tool: &dyn DynamicTool) -> VoiceToolRisk {
    if tool
        .required_capabilities()
        .into_iter()
        .any(is_mutating_capability)
    {
        VoiceToolRisk::Mutating
    } else {
        VoiceToolRisk::ReadOnly
    }
}

impl VoicePolicy {
    /// Evaluate whether a named tool may run for this call.
    pub fn evaluate(
        &self,
        registry: &ToolRegistry,
        tool_name: &str,
        confirmed: bool,
    ) -> VoicePolicyDecision {
        if let Some(exposed) = &self.exposed_tools {
            if !exposed.iter().any(|t| t == tool_name) {
                return VoicePolicyDecision::Deny {
                    reason: format!(
                        "tool `{tool_name}` is not on the voice exposure allowlist"
                    ),
                };
            }
        }

        let Some(tool) = registry.get(tool_name) else {
            // Unknown tools fail at execute time; policy does not invent tools.
            return VoicePolicyDecision::Allow;
        };

        match classify_tool_risk(tool) {
            VoiceToolRisk::ReadOnly => VoicePolicyDecision::Allow,
            VoiceToolRisk::Mutating => {
                if !confirmed {
                    return VoicePolicyDecision::Deny {
                        reason: format!(
                            "mutating tool `{tool_name}` requires confirmation on the voice path (deny-by-default)"
                        ),
                    };
                }
                if !self.allow_confirmed_mutations {
                    return VoicePolicyDecision::Deny {
                        reason: format!(
                            "mutating tool `{tool_name}` is blocked by voice policy (confirmed mutations disabled)"
                        ),
                    };
                }
                VoicePolicyDecision::Allow
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_exec_is_mutating_and_denied_without_confirm() {
        let registry = ToolRegistry::with_defaults();
        let policy = VoicePolicy::default();
        let decision = policy.evaluate(&registry, "bash_exec", false);
        assert!(matches!(decision, VoicePolicyDecision::Deny { .. }));
    }

    #[test]
    fn system_info_is_readonly_and_allowed() {
        let registry = ToolRegistry::with_defaults();
        let policy = VoicePolicy::default();
        assert_eq!(
            policy.evaluate(&registry, "system_info", false),
            VoicePolicyDecision::Allow
        );
    }
}
