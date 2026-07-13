//! Product-role assignment and runtime enforcement compatibility.
//!
//! These types describe behavioral product roles. They do not replace the
//! legacy coordinator/worker pane topology represented by [`crate::AgentRole`].

use crate::agent_registry::AgentPlatformId;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

/// Validation errors for open product-role and runtime-capability identifiers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RoleAssignmentError {
    #[error(
        "invalid agent role id `{0}`: ids must be nonempty and contain no whitespace or control characters"
    )]
    InvalidAgentRoleId(String),
    #[error(
        "invalid runtime capability id `{0}`: ids must be nonempty and contain no whitespace or control characters"
    )]
    InvalidRuntimeCapabilityId(String),
    #[error("duplicate runtime capability `{0}` in declared support")]
    DuplicateRuntimeCapability(RuntimeCapabilityId),
}

fn is_valid_open_id(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

/// Open, validated identity for a product-role contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentRoleId(String);

impl AgentRoleId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, RoleAssignmentError> {
        let value = value.into();
        if !is_valid_open_id(&value) {
            return Err(RoleAssignmentError::InvalidAgentRoleId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentRoleId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for AgentRoleId {
    type Err = RoleAssignmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_new(value)
    }
}

impl<'de> Deserialize<'de> for AgentRoleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(value).map_err(D::Error::custom)
    }
}

impl PartialEq<&str> for AgentRoleId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Open, validated identity for a runtime launch capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RuntimeCapabilityId(String);

impl RuntimeCapabilityId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, RoleAssignmentError> {
        let value = value.into();
        if !is_valid_open_id(&value) {
            return Err(RoleAssignmentError::InvalidRuntimeCapabilityId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeCapabilityId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for RuntimeCapabilityId {
    type Err = RoleAssignmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_new(value)
    }
}

impl<'de> Deserialize<'de> for RuntimeCapabilityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(value).map_err(D::Error::custom)
    }
}

impl PartialEq<&str> for RuntimeCapabilityId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Strength with which a runtime enforces a launch capability.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementStrength {
    #[default]
    Unsupported,
    Advisory,
    Mediated,
    Structural,
}

/// Enforcement that a runtime declares for one launch capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilitySupport {
    pub capability: RuntimeCapabilityId,
    pub enforcement: EnforcementStrength,
}

/// Minimum enforcement a product role requests for one launch capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleCapabilityRequirement {
    pub capability: RuntimeCapabilityId,
    pub minimum_enforcement: EnforcementStrength,
    pub mandatory: bool,
}

/// Product role plus the launch-capability requirements it carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRoleAssignment {
    pub role: AgentRoleId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<RoleCapabilityRequirement>,
}

/// One role requirement compared with the runtime's declared enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCompatibility {
    pub capability: RuntimeCapabilityId,
    pub required: EnforcementStrength,
    pub available: EnforcementStrength,
    pub mandatory: bool,
}

impl CapabilityCompatibility {
    pub fn is_satisfied(&self) -> bool {
        self.available >= self.required
    }
}

/// Pure compatibility result for one canonical platform and product role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleCompatibility {
    pub platform: AgentPlatformId,
    pub role: AgentRoleId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CapabilityCompatibility>,
}

impl RoleCompatibility {
    /// Mandatory gaps block launch; optional gaps never do.
    pub fn launch_allowed(&self) -> bool {
        self.checks
            .iter()
            .all(|check| !check.mandatory || check.is_satisfied())
    }

    pub fn is_blocked(&self) -> bool {
        !self.launch_allowed()
    }

    /// A launch is degraded only when it remains allowed with an optional gap.
    pub fn is_degraded(&self) -> bool {
        self.launch_allowed() && self.checks.iter().any(|check| !check.is_satisfied())
    }
}

/// Compare a product-role assignment with the selected canonical platform's
/// declared launch capabilities. Missing capabilities are unsupported.
pub fn evaluate_role_compatibility(
    platform: &AgentPlatformId,
    declared_support: &[RuntimeCapabilitySupport],
    assignment: &AgentRoleAssignment,
) -> Result<RoleCompatibility, RoleAssignmentError> {
    for (index, support) in declared_support.iter().enumerate() {
        if declared_support[..index]
            .iter()
            .any(|previous| previous.capability == support.capability)
        {
            return Err(RoleAssignmentError::DuplicateRuntimeCapability(
                support.capability.clone(),
            ));
        }
    }

    let checks = assignment
        .requirements
        .iter()
        .map(|requirement| {
            let available = declared_support
                .iter()
                .filter(|support| support.capability == requirement.capability)
                .map(|support| support.enforcement)
                .max()
                .unwrap_or(EnforcementStrength::Unsupported);

            CapabilityCompatibility {
                capability: requirement.capability.clone(),
                required: requirement.minimum_enforcement,
                available,
                mandatory: requirement.mandatory,
            }
        })
        .collect();

    Ok(RoleCompatibility {
        platform: platform.clone(),
        role: assignment.role.clone(),
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_registry::AgentPlatformId;
    use crate::AgentRuntime;

    fn role(value: &str) -> AgentRoleId {
        AgentRoleId::try_new(value).expect("test role id must be valid")
    }

    fn capability(value: &str) -> RuntimeCapabilityId {
        RuntimeCapabilityId::try_new(value).expect("test capability id must be valid")
    }

    fn assignment(requirements: Vec<RoleCapabilityRequirement>) -> AgentRoleAssignment {
        AgentRoleAssignment {
            role: role("builder"),
            requirements,
        }
    }

    #[test]
    fn test_agent_role_id_validation_accepts_open_string_and_rejects_blank_or_whitespace() {
        let id = AgentRoleId::try_new("domain.builder/v2").expect("open id should be valid");
        assert_eq!(id.as_str(), "domain.builder/v2");
        assert!(
            format!("{}", AgentRoleId::try_new(" ").unwrap_err()).contains("invalid agent role id")
        );
        assert!(AgentRoleId::try_new("builder role").is_err());
    }

    #[test]
    fn test_runtime_capability_id_validation_accepts_open_string_and_rejects_control_characters() {
        let id = RuntimeCapabilityId::try_new("workspace.target/v1")
            .expect("open capability id should be valid");
        assert_eq!(id.as_str(), "workspace.target/v1");
        assert!(RuntimeCapabilityId::try_new("").is_err());
        assert!(RuntimeCapabilityId::try_new("process\nlifecycle").is_err());
    }

    #[test]
    fn test_role_and_capability_ids_serde_roundtrip_and_validate_on_deserialize() {
        let role_id = role("builder.custom-v1");
        let capability_id = capability("tools.structured/v1");

        let role_json = serde_json::to_string(&role_id).unwrap();
        let capability_json = serde_json::to_string(&capability_id).unwrap();

        assert_eq!(
            serde_json::from_str::<AgentRoleId>(&role_json).unwrap(),
            role_id
        );
        assert_eq!(
            serde_json::from_str::<RuntimeCapabilityId>(&capability_json).unwrap(),
            capability_id
        );
        assert!(serde_json::from_str::<AgentRoleId>(r#""builder role""#).is_err());
        assert!(serde_json::from_str::<RuntimeCapabilityId>(r#"" ""#).is_err());
    }

    #[test]
    fn test_enforcement_strength_orders_unsupported_advisory_mediated_structural() {
        assert!(EnforcementStrength::Unsupported < EnforcementStrength::Advisory);
        assert!(EnforcementStrength::Advisory < EnforcementStrength::Mediated);
        assert!(EnforcementStrength::Mediated < EnforcementStrength::Structural);
    }

    #[test]
    fn test_evaluate_role_compatibility_blocks_missing_mandatory_requirement() {
        let platform = AgentPlatformId::try_new("claude-code").unwrap();
        let role_assignment = assignment(vec![RoleCapabilityRequirement {
            capability: capability("filesystem.scoped"),
            minimum_enforcement: EnforcementStrength::Structural,
            mandatory: true,
        }]);

        let result = evaluate_role_compatibility(&platform, &[], &role_assignment).unwrap();

        assert!(!result.launch_allowed());
        assert!(result.is_blocked());
        assert!(!result.is_degraded());
        assert_eq!(result.platform, platform);
        assert_eq!(result.role, role("builder"));
        assert_eq!(result.checks.len(), 1);
        assert_eq!(result.checks[0].available, EnforcementStrength::Unsupported);
        assert!(!result.checks[0].is_satisfied());
    }

    #[test]
    fn test_evaluate_role_compatibility_allows_with_optional_gap_as_degraded() {
        let platform = AgentPlatformId::try_new("codex").unwrap();
        let declared_support = vec![
            RuntimeCapabilitySupport {
                capability: capability("workspace.target"),
                enforcement: EnforcementStrength::Mediated,
            },
            RuntimeCapabilitySupport {
                capability: capability("filesystem.scoped"),
                enforcement: EnforcementStrength::Advisory,
            },
        ];
        let role_assignment = assignment(vec![
            RoleCapabilityRequirement {
                capability: capability("workspace.target"),
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            },
            RoleCapabilityRequirement {
                capability: capability("filesystem.scoped"),
                minimum_enforcement: EnforcementStrength::Structural,
                mandatory: false,
            },
        ]);

        let result =
            evaluate_role_compatibility(&platform, &declared_support, &role_assignment).unwrap();

        assert!(result.launch_allowed());
        assert!(!result.is_blocked());
        assert!(result.is_degraded());
        assert_eq!(result.checks[0].capability, capability("workspace.target"));
        assert!(result.checks[0].is_satisfied());
        assert_eq!(result.checks[1].capability, capability("filesystem.scoped"));
        assert!(!result.checks[1].is_satisfied());
    }

    #[test]
    fn test_evaluate_role_compatibility_rejects_duplicate_capability_support() {
        let platform = AgentPlatformId::try_new("custom-runtime").unwrap();
        let duplicate_capability = capability("workspace.target");
        let declared_support = vec![
            RuntimeCapabilitySupport {
                capability: duplicate_capability.clone(),
                enforcement: EnforcementStrength::Unsupported,
            },
            RuntimeCapabilitySupport {
                capability: duplicate_capability.clone(),
                enforcement: EnforcementStrength::Structural,
            },
        ];
        let role_assignment = assignment(vec![RoleCapabilityRequirement {
            capability: duplicate_capability.clone(),
            minimum_enforcement: EnforcementStrength::Structural,
            mandatory: true,
        }]);

        let error = evaluate_role_compatibility(&platform, &declared_support, &role_assignment)
            .unwrap_err();

        assert_eq!(
            error,
            RoleAssignmentError::DuplicateRuntimeCapability(duplicate_capability)
        );
        assert!(error.to_string().contains("duplicate runtime capability"));
    }

    #[test]
    fn test_evaluate_role_compatibility_preserves_requirement_order() {
        let platform = AgentPlatformId::try_new("ion").unwrap();
        let role_assignment = assignment(vec![
            RoleCapabilityRequirement {
                capability: capability("z-last"),
                minimum_enforcement: EnforcementStrength::Advisory,
                mandatory: false,
            },
            RoleCapabilityRequirement {
                capability: capability("a-first"),
                minimum_enforcement: EnforcementStrength::Advisory,
                mandatory: false,
            },
        ]);

        let result = evaluate_role_compatibility(&platform, &[], &role_assignment).unwrap();

        assert_eq!(result.checks[0].capability, capability("z-last"));
        assert_eq!(result.checks[1].capability, capability("a-first"));
    }

    #[test]
    fn test_role_assignment_and_compatibility_serde_roundtrip() {
        let platform = AgentPlatformId::try_new("ion").unwrap();
        let role_assignment = assignment(vec![RoleCapabilityRequirement {
            capability: capability("process.lifecycle"),
            minimum_enforcement: EnforcementStrength::Structural,
            mandatory: true,
        }]);
        let compatibility = evaluate_role_compatibility(
            &platform,
            &[RuntimeCapabilitySupport {
                capability: capability("process.lifecycle"),
                enforcement: EnforcementStrength::Structural,
            }],
            &role_assignment,
        )
        .unwrap();

        let assignment_json = serde_json::to_string(&role_assignment).unwrap();
        let compatibility_json = serde_json::to_string(&compatibility).unwrap();

        assert_eq!(
            serde_json::from_str::<AgentRoleAssignment>(&assignment_json).unwrap(),
            role_assignment
        );
        assert_eq!(
            serde_json::from_str::<RoleCompatibility>(&compatibility_json).unwrap(),
            compatibility
        );
    }

    #[test]
    fn test_agent_runtime_old_json_deserializes_and_absent_role_fields_stay_omitted() {
        let old_json = r#"{
            "id":"agent-1",
            "label":"Agent 1",
            "backend_kind":"claude-code",
            "session_id":null,
            "ephemeral":false,
            "working_directory":"/workspace",
            "status":"idle",
            "current_task":null,
            "active":true,
            "context":{
                "tier":"healthy",
                "usage_fraction":0.0,
                "estimated_tokens":0,
                "window_tokens":0,
                "compaction_count":0,
                "injection_count":0,
                "pending_review_count":0,
                "recent_insights":[]
            },
            "recent_files":[],
            "recent_tools":[],
            "warnings":[]
        }"#;

        let runtime: AgentRuntime = serde_json::from_str(old_json).unwrap();
        assert!(runtime.role_assignment.is_none());
        assert!(runtime.role_compatibility.is_none());

        let serialized = serde_json::to_value(runtime).unwrap();
        assert!(serialized.get("role_assignment").is_none());
        assert!(serialized.get("role_compatibility").is_none());
    }

    #[test]
    fn test_agent_runtime_new_role_fields_roundtrip() {
        let platform = AgentPlatformId::try_new("ion").unwrap();
        let role_assignment = assignment(vec![RoleCapabilityRequirement {
            capability: capability("process.lifecycle"),
            minimum_enforcement: EnforcementStrength::Mediated,
            mandatory: true,
        }]);
        let role_compatibility =
            evaluate_role_compatibility(&platform, &[], &role_assignment).unwrap();
        let runtime = AgentRuntime {
            role_assignment: Some(role_assignment),
            role_compatibility: Some(role_compatibility),
            ..Default::default()
        };

        let json = serde_json::to_string(&runtime).unwrap();
        let decoded: AgentRuntime = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, runtime);
        assert!(json.contains(r#""role":"builder""#));
        assert!(json.contains(r#""capability":"process.lifecycle""#));
    }
}
