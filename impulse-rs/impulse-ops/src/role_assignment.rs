//! Product-role assignment and runtime enforcement compatibility.
//!
//! These types describe behavioral product roles. They do not replace the
//! legacy coordinator/worker pane topology represented by [`crate::AgentRole`].

use crate::agent_registry::AgentPlatformId;
use crate::governed_task::WorldScope;
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

    /// Construct an audited static capability id owned by trusted crate code.
    ///
    /// This bypasses dynamic validation and must never receive operator or wire
    /// input; untrusted values must use [`Self::try_new`].
    pub(crate) fn builtin(value: &'static str) -> Self {
        Self(value.to_string())
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

pub const GOVERNED_BUILDER_ROLE_ID: &str = "builder";

/// Canonical first product-role contract for a profiled governed launch.
///
/// Keeping this in `impulse-ops` makes Desktop preview, daemon registration,
/// and raw IPC validation compare the same requirements instead of allowing
/// the UI to be the only source of truth.
pub fn canonical_governed_builder_assignment() -> AgentRoleAssignment {
    AgentRoleAssignment {
        role: AgentRoleId(GOVERNED_BUILDER_ROLE_ID.to_string()),
        requirements: vec![
            RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::builtin("workspace.target"),
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            },
            RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::builtin("process.lifecycle"),
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            },
            RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::builtin("filesystem.scoped"),
                minimum_enforcement: EnforcementStrength::Structural,
                mandatory: false,
            },
        ],
    }
}

/// Capability id whose enforcement a world scope can raise.
pub const FILESYSTEM_SCOPED_CAPABILITY: &str = "filesystem.scoped";

/// Enforcement that a world scope contributes to `filesystem.scoped`.
///
/// A staged worktree is a *Git-level* boundary: the Builder writes into a
/// separate checkout and only a promotion makes those bytes canonical. Nothing
/// stops the process from writing outside that checkout, so this is
/// [`EnforcementStrength::Mediated`] and never
/// [`EnforcementStrength::Structural`]. Reporting it as structural would
/// promise OS-level containment that ADR-0019 explicitly does not deliver.
pub fn world_scope_filesystem_enforcement(scope: WorldScope) -> EnforcementStrength {
    match scope {
        WorldScope::StagedAuthoritative => EnforcementStrength::Mediated,
        WorldScope::ReadOnlySnapshot
        | WorldScope::DisposableScratch
        | WorldScope::Authoritative => EnforcementStrength::Unsupported,
    }
}

/// Launch capabilities a world scope contributes on top of the runtime's own
/// declared support. An unsupported contribution is omitted rather than
/// declared, so it can never raise a runtime's reported enforcement.
pub fn world_scope_capability_support(scope: WorldScope) -> Vec<RuntimeCapabilitySupport> {
    let enforcement = world_scope_filesystem_enforcement(scope);
    if enforcement == EnforcementStrength::Unsupported {
        return Vec::new();
    }
    vec![RuntimeCapabilitySupport {
        capability: RuntimeCapabilityId::builtin(FILESYSTEM_SCOPED_CAPABILITY),
        enforcement,
    }]
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

/// Compare a role assignment against a runtime's declared capabilities *plus*
/// the mediation a world scope adds. The strongest declaration wins per
/// capability, exactly as when a runtime declares the same capability twice.
pub fn evaluate_role_compatibility_in_world(
    platform: &AgentPlatformId,
    declared_support: &[RuntimeCapabilitySupport],
    assignment: &AgentRoleAssignment,
    scope: WorldScope,
) -> Result<RoleCompatibility, RoleAssignmentError> {
    let scoped = world_scope_capability_support(scope);
    if scoped.is_empty() {
        return evaluate_role_compatibility(platform, declared_support, assignment);
    }
    let mut merged = declared_support.to_vec();
    for support in scoped {
        match merged
            .iter_mut()
            .find(|declared| declared.capability == support.capability)
        {
            Some(declared) => declared.enforcement = declared.enforcement.max(support.enforcement),
            None => merged.push(support),
        }
    }
    evaluate_role_compatibility(platform, &merged, assignment)
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
    fn test_runtime_capability_id_builtin_constructor_preserves_audited_static_literal() {
        let id = RuntimeCapabilityId::builtin("workspace.target");

        assert_eq!(id.as_str(), "workspace.target");
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

    fn ion_platform() -> AgentPlatformId {
        AgentPlatformId::try_new("ion").unwrap()
    }

    #[test]
    fn test_world_scope_filesystem_enforcement_is_mediated_never_structural() {
        assert_eq!(
            world_scope_filesystem_enforcement(WorldScope::StagedAuthoritative),
            EnforcementStrength::Mediated
        );
        assert!(
            world_scope_filesystem_enforcement(WorldScope::StagedAuthoritative)
                < EnforcementStrength::Structural,
            "a Git-level staged worktree must never be reported as structural containment"
        );
        for scope in [
            WorldScope::ReadOnlySnapshot,
            WorldScope::DisposableScratch,
            WorldScope::Authoritative,
        ] {
            assert_eq!(
                world_scope_filesystem_enforcement(scope),
                EnforcementStrength::Unsupported
            );
            assert!(world_scope_capability_support(scope).is_empty());
        }
    }

    #[test]
    fn test_world_scope_capability_support_declares_only_filesystem_scoped() {
        let support = world_scope_capability_support(WorldScope::StagedAuthoritative);
        assert_eq!(support.len(), 1);
        assert_eq!(support[0].capability, FILESYSTEM_SCOPED_CAPABILITY);
        assert_eq!(support[0].enforcement, EnforcementStrength::Mediated);
    }

    #[test]
    fn test_staged_world_scope_raises_filesystem_scoped_to_mediated() {
        let assignment = canonical_governed_builder_assignment();
        let declared = vec![
            RuntimeCapabilitySupport {
                capability: RuntimeCapabilityId::builtin("workspace.target"),
                enforcement: EnforcementStrength::Mediated,
            },
            RuntimeCapabilitySupport {
                capability: RuntimeCapabilityId::builtin("process.lifecycle"),
                enforcement: EnforcementStrength::Mediated,
            },
        ];

        let plain = evaluate_role_compatibility(&ion_platform(), &declared, &assignment).unwrap();
        let scoped = evaluate_role_compatibility_in_world(
            &ion_platform(),
            &declared,
            &assignment,
            WorldScope::StagedAuthoritative,
        )
        .unwrap();

        let filesystem = |compatibility: &RoleCompatibility| {
            compatibility
                .checks
                .iter()
                .find(|check| check.capability == FILESYSTEM_SCOPED_CAPABILITY)
                .expect("the canonical Builder role requires filesystem.scoped")
                .available
        };
        assert_eq!(filesystem(&plain), EnforcementStrength::Unsupported);
        assert_eq!(filesystem(&scoped), EnforcementStrength::Mediated);
        assert!(scoped.launch_allowed());
        // Still degraded: the role asks for structural, the staged worktree
        // honestly supplies only Git-level mediation.
        assert!(scoped.is_degraded());
    }

    #[test]
    fn test_authoritative_world_scope_leaves_compatibility_untouched() {
        let assignment = canonical_governed_builder_assignment();
        let declared = vec![RuntimeCapabilitySupport {
            capability: RuntimeCapabilityId::builtin("workspace.target"),
            enforcement: EnforcementStrength::Mediated,
        }];
        assert_eq!(
            evaluate_role_compatibility_in_world(
                &ion_platform(),
                &declared,
                &assignment,
                WorldScope::Authoritative,
            )
            .unwrap(),
            evaluate_role_compatibility(&ion_platform(), &declared, &assignment).unwrap()
        );
    }

    #[test]
    fn test_world_scope_never_lowers_a_stronger_declared_enforcement() {
        let assignment = canonical_governed_builder_assignment();
        let declared = vec![RuntimeCapabilitySupport {
            capability: RuntimeCapabilityId::builtin("filesystem.scoped"),
            enforcement: EnforcementStrength::Structural,
        }];
        let scoped = evaluate_role_compatibility_in_world(
            &ion_platform(),
            &declared,
            &assignment,
            WorldScope::StagedAuthoritative,
        )
        .unwrap();
        assert_eq!(
            scoped
                .checks
                .iter()
                .find(|check| check.capability == FILESYSTEM_SCOPED_CAPABILITY)
                .unwrap()
                .available,
            EnforcementStrength::Structural
        );
    }

    #[test]
    fn test_world_scope_evaluation_preserves_duplicate_capability_errors() {
        let assignment = canonical_governed_builder_assignment();
        let declared = vec![
            RuntimeCapabilitySupport {
                capability: RuntimeCapabilityId::builtin("workspace.target"),
                enforcement: EnforcementStrength::Mediated,
            },
            RuntimeCapabilitySupport {
                capability: RuntimeCapabilityId::builtin("workspace.target"),
                enforcement: EnforcementStrength::Advisory,
            },
        ];
        assert!(matches!(
            evaluate_role_compatibility_in_world(
                &ion_platform(),
                &declared,
                &assignment,
                WorldScope::StagedAuthoritative,
            ),
            Err(RoleAssignmentError::DuplicateRuntimeCapability(_))
        ));
    }
}
