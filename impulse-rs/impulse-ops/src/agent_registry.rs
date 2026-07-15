//! Canonical agent registry — a single, data-driven source of truth for the
//! coding agents Impulse can monitor and launch.
//!
//! Historically the workspace carried several divergent enums (`AgentKind`,
//! `AgentType`, `ImpulseHarness`, `AgentPlatformKind`, two `Platform`s …) each
//! re-encoding the same facts: a canonical slug, a human label, the launch
//! command, headless invocation args, and string aliases. Those enums drifted —
//! different variant sets, different serde casings, duplicated `parse()` alias
//! tables. This module collapses the *facts* into one [`AgentDescriptor`]
//! catalog. The legacy enums remain for wire/disk serde compatibility, but new
//! code resolves agent metadata here so there is exactly one place to add a new
//! agent.
//!
//! The builtin catalog ([`AgentRegistry::builtin`]) is the union of every legacy
//! enum's knowledge. A TOML file can extend or override it at runtime so a user
//! can register *any* CLI/TUI coding agent without a code change — mirroring the
//! existing JSON capabilities-manifest pattern but in TOML for hand-editing.
//!
//! ```toml
//! [[agent]]
//! id = "my-agent"
//! label = "My Agent"
//! command = "my-agent"
//! invocation_args = ["--headless"]
//! aliases = ["mine"]
//!
//! [agent.capabilities]
//! uses_xml_context = false
//! startup_delay_ms = 1500
//! ```

use std::path::{Path, PathBuf};

use crate::role_assignment::{
    evaluate_role_compatibility as evaluate_declared_role_compatibility, AgentRoleAssignment,
    EnforcementStrength, RoleAssignmentError, RoleCompatibility, RuntimeCapabilityId,
    RuntimeCapabilitySupport,
};
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

/// Environment variable naming a TOML file that extends/overrides the builtin
/// registry. Mirrors `IMPULSE_CAPABILITIES_PATH` for the JSON manifest.
pub const REGISTRY_PATH_ENV: &str = "IMPULSE_AGENT_REGISTRY_PATH";

/// Default startup delay (milliseconds) before context injection for an agent
/// that does not specify its own. Matches the legacy `AgentKind` default.
pub const DEFAULT_STARTUP_DELAY_MS: u64 = 2000;

fn default_startup_delay_ms() -> u64 {
    DEFAULT_STARTUP_DELAY_MS
}

/// Errors produced while building or loading an [`AgentRegistry`].
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("failed to read agent registry at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse agent registry TOML: {0}")]
    Toml(String),
    #[error("agent descriptor has an empty id")]
    EmptyId,
    #[error(
        "invalid agent platform id `{0}`: ids must not contain whitespace or control characters"
    )]
    InvalidId(String),
    #[error(
        "invalid alias `{alias}` for agent platform `{platform}`: aliases must be nonempty and contain no whitespace or control characters"
    )]
    InvalidAlias { platform: String, alias: String },
    #[error("unknown agent platform `{0}`")]
    UnknownPlatform(String),
    #[error("agent platform `{0}` has no launch command")]
    MissingLaunchCommand(String),
    #[error("agent platform `{0}` received a blank command override")]
    BlankLaunchOverride(String),
    #[error("custom agent platform `{platform}` cannot declare trusted runtime_capabilities")]
    UntrustedRuntimeCapabilities { platform: String },
    #[error("duplicate agent id in registry: {0}")]
    DuplicateId(String),
    #[error(
        "agent identity `{identity}` is claimed by both `{first_platform}` and `{second_platform}`"
    )]
    IdentityCollision {
        identity: String,
        first_platform: String,
        second_platform: String,
    },
    #[error("failed to evaluate runtime compatibility: {0}")]
    RoleAssignment(#[from] RoleAssignmentError),
}

/// Open, stable identity for an agent platform.
///
/// The wire shape remains a plain string (for backward compatibility with the
/// former desktop enum), while metadata such as label and launch command stays
/// owned by [`AgentRegistry`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentPlatformId(String);

impl AgentPlatformId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, RegistryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(RegistryError::EmptyId);
        }
        if value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(RegistryError::InvalidId(value));
        }
        Ok(Self(value))
    }

    fn builtin(value: &'static str) -> Self {
        Self(value.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentPlatformId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for AgentPlatformId {
    type Err = RegistryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_new(value)
    }
}

impl<'de> Deserialize<'de> for AgentPlatformId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(value).map_err(D::Error::custom)
    }
}

impl PartialEq<&str> for AgentPlatformId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Behavioral knobs an agent exposes to the context-injection pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapabilities {
    /// Whether this agent consumes XML-tagged context injection (Claude Code).
    #[serde(default)]
    pub uses_xml_context: bool,
    /// Milliseconds to wait after spawn before injecting context.
    #[serde(default = "default_startup_delay_ms")]
    pub startup_delay_ms: u64,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            uses_xml_context: false,
            startup_delay_ms: DEFAULT_STARTUP_DELAY_MS,
        }
    }
}

/// One canonical agent definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDescriptor {
    /// Canonical, stable, kebab-case slug. e.g. `"claude-code"`. This is the
    /// wire/disk identity new code should serialize.
    pub id: AgentPlatformId,
    /// Human-readable label. e.g. `"Claude Code"`.
    pub label: String,
    /// Default binary/command to launch. e.g. `"claude"`.
    pub command: String,
    /// Leading CLI args that put the agent into non-interactive (single-prompt,
    /// print-to-stdout) mode. The prompt is appended as the final positional
    /// argument by the caller. Empty when the agent has no known headless flag.
    #[serde(default)]
    pub invocation_args: Vec<String>,
    /// Alternate names (case-insensitive) that resolve to this agent.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Behavioral capabilities.
    #[serde(default)]
    pub capabilities: AgentCapabilities,
    /// Conservative launch capabilities enforced by the runtime wrapper.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_capabilities: Vec<RuntimeCapabilitySupport>,
}

impl AgentDescriptor {
    /// Returns true if `needle` matches this descriptor's id or any alias,
    /// case-insensitively.
    pub fn matches(&self, needle: &str) -> bool {
        if needle.trim().is_empty() {
            return false;
        }
        identity_eq(self.id.as_str(), needle)
            || self.aliases.iter().any(|alias| identity_eq(alias, needle))
    }
}

/// On-disk TOML shape: a list of `[[agent]]` tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryFile {
    #[serde(default, rename = "agent")]
    pub agents: Vec<AgentDescriptor>,
}

/// An ordered catalog of [`AgentDescriptor`]s.
///
/// Order is meaningful for [`AgentRegistry::detect_from_command`], which returns
/// the first descriptor whose command/alias appears in a command string — the
/// generic `shell` descriptor is intentionally last so it acts as a fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRegistry {
    agents: Vec<AgentDescriptor>,
}

impl AgentRegistry {
    /// The builtin catalog: the union of every legacy enum's knowledge.
    pub fn builtin() -> Self {
        Self {
            agents: vec![
                AgentDescriptor {
                    id: AgentPlatformId::builtin("claude-code"),
                    label: "Claude Code".into(),
                    command: "claude".into(),
                    invocation_args: vec!["--print".into()],
                    aliases: vec!["claude".into(), "claude_code".into(), "claudecode".into()],
                    capabilities: AgentCapabilities {
                        uses_xml_context: true,
                        startup_delay_ms: 3000,
                    },
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
                AgentDescriptor {
                    id: AgentPlatformId::builtin("codex"),
                    label: "Codex".into(),
                    command: "codex".into(),
                    invocation_args: vec!["exec".into()],
                    aliases: vec![],
                    capabilities: AgentCapabilities::default(),
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
                AgentDescriptor {
                    id: AgentPlatformId::builtin("opencode"),
                    label: "OpenCode".into(),
                    command: "opencode".into(),
                    invocation_args: vec!["run".into()],
                    aliases: vec!["open-code".into()],
                    capabilities: AgentCapabilities::default(),
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
                AgentDescriptor {
                    id: AgentPlatformId::builtin("gemini"),
                    label: "Gemini".into(),
                    command: "gemini".into(),
                    invocation_args: vec!["-p".into()],
                    aliases: vec!["antigravity".into()],
                    capabilities: AgentCapabilities::default(),
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
                AgentDescriptor {
                    id: AgentPlatformId::builtin("cursor"),
                    label: "Cursor".into(),
                    // Cursor's headless CLI binary is `cursor-agent`. No stable
                    // public single-prompt flag is wired yet, so args stay empty
                    // rather than guessing.
                    command: "cursor-agent".into(),
                    invocation_args: vec![],
                    aliases: vec!["cursor-agent".into()],
                    capabilities: AgentCapabilities::default(),
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
                AgentDescriptor {
                    id: AgentPlatformId::builtin("ion"),
                    label: "Ion".into(),
                    command: "ion".into(),
                    // Ion's bare binary is the interactive coding agent. It
                    // has no separate headless invocation contract.
                    invocation_args: vec![],
                    aliases: vec![],
                    capabilities: AgentCapabilities::default(),
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
                AgentDescriptor {
                    id: AgentPlatformId::builtin("shell"),
                    label: "Shell".into(),
                    command: "sh".into(),
                    invocation_args: vec![],
                    aliases: vec![
                        "bash".into(),
                        "zsh".into(),
                        "generic_shell".into(),
                        "generic-shell".into(),
                    ],
                    capabilities: AgentCapabilities {
                        uses_xml_context: false,
                        startup_delay_ms: 500,
                    },
                    runtime_capabilities: mediated_desktop_wrapper_capabilities(),
                },
            ],
        }
    }

    /// Build a registry from explicit descriptors, validating that every id is
    /// non-empty and unique (case-insensitive).
    pub fn from_descriptors(descriptors: Vec<AgentDescriptor>) -> Result<Self, RegistryError> {
        let mut registry = Self::default();
        for descriptor in descriptors {
            registry.insert(descriptor)?;
        }
        Ok(registry)
    }

    /// Parse a registry from a TOML document (no builtin merge).
    pub fn from_toml_str(toml_str: &str) -> Result<Self, RegistryError> {
        let file: RegistryFile =
            toml::from_str(toml_str).map_err(|e| RegistryError::Toml(e.to_string()))?;
        Self::from_registry_file(file)
    }

    /// Build from the operator-editable registry-file shape.
    ///
    /// Runtime capability evidence is trusted code-declared adapter metadata,
    /// so custom files cannot self-assert it for launch authorization.
    pub fn from_registry_file(file: RegistryFile) -> Result<Self, RegistryError> {
        if let Some(descriptor) = file
            .agents
            .iter()
            .find(|descriptor| !descriptor.runtime_capabilities.is_empty())
        {
            return Err(RegistryError::UntrustedRuntimeCapabilities {
                platform: descriptor.id.to_string(),
            });
        }
        Self::from_descriptors(file.agents)
    }

    /// Load a registry from a TOML file path (no builtin merge).
    pub fn load_from_path(path: &Path) -> Result<Self, RegistryError> {
        let contents = std::fs::read_to_string(path).map_err(|source| RegistryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&contents)
    }

    /// The builtin catalog, with an optional TOML override file (from
    /// `IMPULSE_AGENT_REGISTRY_PATH`) merged on top. Override descriptors with a
    /// matching id replace the builtin entry; new ids are appended. When the env
    /// var is unset, returns the pristine builtin catalog.
    pub fn load_with_env() -> Result<Self, RegistryError> {
        let mut registry = Self::builtin();
        if let Ok(path) = std::env::var(REGISTRY_PATH_ENV) {
            if !path.trim().is_empty() {
                let overrides = Self::load_from_path(Path::new(&path))?;
                registry.merge(overrides)?;
            }
        }
        Ok(registry)
    }

    /// Policy helper for runtime/MCP use.
    ///
    /// - env unset or empty → builtin
    /// - env points to non-existent file → builtin (per "missing file" policy)
    /// - env points to corrupt file → RegistryError (no silent fallback)
    /// - success → merged registry
    pub fn registry_for_runtime() -> Result<Self, RegistryError> {
        let path = std::env::var_os(REGISTRY_PATH_ENV).map(PathBuf::from);
        Self::registry_for_runtime_path(path.as_deref())
    }

    /// Pure path-driven form of [`Self::registry_for_runtime`]. Keeping path
    /// selection separate from loading makes callers and parallel tests able to
    /// exercise registry policy without mutating process-global environment.
    pub fn registry_for_runtime_path(path: Option<&Path>) -> Result<Self, RegistryError> {
        let mut registry = Self::builtin();
        if let Some(path) = path {
            if !path.as_os_str().to_string_lossy().trim().is_empty() {
                match path.try_exists() {
                    Ok(true) => {
                        let overrides = Self::load_from_path(path)?;
                        registry.merge(overrides)?;
                    }
                    Ok(false) => {}
                    Err(source) => {
                        return Err(RegistryError::Io {
                            path: path.to_path_buf(),
                            source,
                        });
                    }
                }
            }
            // missing file → builtin, no error
        }
        Ok(registry)
    }

    /// Merge another registry into this one: descriptors whose id already exists
    /// (case-insensitive) replace the existing entry in place; new ids append.
    pub fn merge(&mut self, other: AgentRegistry) -> Result<(), RegistryError> {
        let mut merged = self.agents.clone();
        for descriptor in other.agents {
            match merged
                .iter_mut()
                .find(|existing| identity_eq(existing.id.as_str(), descriptor.id.as_str()))
            {
                Some(existing) => *existing = descriptor,
                None => merged.push(descriptor),
            }
        }
        *self = Self::from_descriptors(merged)?;
        Ok(())
    }

    /// Insert a single descriptor, validating non-empty + unique id.
    fn insert(&mut self, descriptor: AgentDescriptor) -> Result<(), RegistryError> {
        for alias in &descriptor.aliases {
            if AgentPlatformId::try_new(alias).is_err() {
                return Err(RegistryError::InvalidAlias {
                    platform: descriptor.id.to_string(),
                    alias: alias.clone(),
                });
            }
        }
        if self
            .agents
            .iter()
            .any(|existing| identity_eq(existing.id.as_str(), descriptor.id.as_str()))
        {
            return Err(RegistryError::DuplicateId(descriptor.id.to_string()));
        }
        for identity in descriptor_identities(&descriptor) {
            if let Some(existing) = self.agents.iter().find(|existing| {
                descriptor_identities(existing).any(|candidate| identity_eq(candidate, identity))
            }) {
                return Err(RegistryError::IdentityCollision {
                    identity: identity.to_string(),
                    first_platform: existing.id.to_string(),
                    second_platform: descriptor.id.to_string(),
                });
            }
        }
        self.agents.push(descriptor);
        Ok(())
    }

    /// Resolve a string (id or alias, case-insensitive) to a descriptor.
    pub fn resolve(&self, needle: &str) -> Option<&AgentDescriptor> {
        self.agents.iter().find(|d| d.matches(needle))
    }

    /// Evaluate a role against a registered platform's launch capabilities.
    /// Alias inputs resolve through the descriptor, so compatibility always
    /// records the canonical platform identity.
    pub fn evaluate_role_compatibility(
        &self,
        platform: &AgentPlatformId,
        assignment: &AgentRoleAssignment,
    ) -> Result<RoleCompatibility, RegistryError> {
        let descriptor = self
            .resolve(platform.as_str())
            .ok_or_else(|| RegistryError::UnknownPlatform(platform.to_string()))?;
        evaluate_declared_role_compatibility(
            &descriptor.id,
            &descriptor.runtime_capabilities,
            assignment,
        )
        .map_err(RegistryError::from)
    }

    /// Look up a descriptor by exact canonical id (case-insensitive).
    pub fn get(&self, id: &str) -> Option<&AgentDescriptor> {
        self.agents
            .iter()
            .find(|descriptor| identity_eq(descriptor.id.as_str(), id))
    }

    /// Detect the agent represented by the first executable token in a command.
    /// Only the executable basename participates: parent-directory text and
    /// arguments can never impersonate an agent id. Matching is exact and
    /// case-insensitive across descriptor command, id, and aliases. The
    /// trailing `shell` descriptor therefore still recognizes plain shells.
    pub fn detect_from_command(&self, command: &str) -> Option<&AgentDescriptor> {
        let executable = command.split_whitespace().next()?;
        let token = normalized_executable_basename(executable)?;
        self.agents.iter().find(|descriptor| {
            normalized_executable_basename(&descriptor.command)
                .is_some_and(|command| identity_eq(&command, &token))
                || identity_eq(descriptor.id.as_str(), &token)
                || descriptor
                    .aliases
                    .iter()
                    .any(|alias| identity_eq(alias, &token))
        })
    }

    /// All descriptors in catalog order.
    pub fn agents(&self) -> &[AgentDescriptor] {
        &self.agents
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Whether the registry holds no agents.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

fn mediated_desktop_wrapper_capabilities() -> Vec<RuntimeCapabilitySupport> {
    ["workspace.target", "process.lifecycle"]
        .into_iter()
        .map(|capability| RuntimeCapabilitySupport {
            capability: RuntimeCapabilityId::builtin(capability),
            enforcement: EnforcementStrength::Mediated,
        })
        .collect()
}

fn descriptor_identities(descriptor: &AgentDescriptor) -> impl Iterator<Item = &str> {
    std::iter::once(descriptor.id.as_str()).chain(descriptor.aliases.iter().map(String::as_str))
}

fn identity_eq(left: &str, right: &str) -> bool {
    left.trim().to_lowercase() == right.trim().to_lowercase()
}

fn normalized_executable_basename(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(['\'', '"']);
    if value.is_empty() {
        return None;
    }
    let basename = Path::new(value).file_name()?.to_str()?;
    let basename_path = Path::new(basename);
    let basename = match basename_path
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("exe") => {
            basename_path.file_stem()?.to_str()?
        }
        _ => basename,
    };
    (!basename.is_empty()).then(|| basename.to_string())
}

/// Structured info for one agent platform (for observability / listing / launch).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPlatformInfo {
    pub id: AgentPlatformId,
    pub label: String,
    pub command: String,
    /// Trusted launch-capability evidence copied from the backend registry
    /// descriptor for compatibility preview clients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_capabilities: Vec<RuntimeCapabilitySupport>,
}

/// Pure observability report built from the registry (no I/O, no printing).
/// Used by CLI status, MCP list platforms, etc. to drive indicators for claude-code, codex, workspaces.
#[derive(Debug, Clone, Serialize)]
pub struct AgentPlatformsReport {
    pub platforms: Vec<AgentPlatformInfo>,
    pub multi_workspace_note: String,
}

impl AgentPlatformsReport {
    /// Build from a registry. Pure function — directly testable.
    pub fn from_registry(reg: &AgentRegistry) -> Self {
        let platforms = reg
            .agents()
            .iter()
            .map(|d| AgentPlatformInfo {
                id: d.id.clone(),
                label: d.label.clone(),
                command: d.command.clone(),
                runtime_capabilities: d.runtime_capabilities.clone(),
            })
            .collect();
        Self {
            platforms,
            multi_workspace_note:
                "register and cycle project folders/spaces via desktop (WorkspaceRegistry)."
                    .to_string(),
        }
    }
}

/// Resolve the command to use when launching a registered terminal agent.
/// Registry membership is required even when an override is supplied, so an
/// unknown declared platform can never silently fall back to Shell. The
/// platform remains operator-declared: a non-blank command override wins and
/// is recorded separately by the runtime for auditability.
pub fn resolve_launch_command(
    reg: &AgentRegistry,
    platform: &AgentPlatformId,
    override_cmd: Option<&str>,
) -> Result<String, RegistryError> {
    let descriptor = reg
        .resolve(platform.as_str())
        .ok_or_else(|| RegistryError::UnknownPlatform(platform.to_string()))?;
    if let Some(cmd) = override_cmd {
        if cmd.trim().is_empty() {
            return Err(RegistryError::BlankLaunchOverride(platform.to_string()));
        }
        return Ok(cmd.to_string());
    }
    if descriptor.command.trim().is_empty() {
        return Err(RegistryError::MissingLaunchCommand(platform.to_string()));
    }
    Ok(descriptor.command.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_contains_canonical_agents() {
        let registry = AgentRegistry::builtin();
        assert_eq!(registry.len(), 7);
        for id in [
            "claude-code",
            "codex",
            "opencode",
            "gemini",
            "cursor",
            "ion",
            "shell",
        ] {
            assert!(registry.get(id).is_some(), "missing builtin agent {id}");
        }
    }

    #[test]
    fn test_builtin_contains_ion_as_an_interactive_agent() {
        let registry = AgentRegistry::builtin();
        let ion = registry
            .get("ion")
            .expect("Ion must be available through the canonical agent registry");

        assert_eq!(ion.label, "Ion");
        assert_eq!(ion.command, "ion");
        assert!(
            ion.invocation_args.is_empty(),
            "Ion is an interactive coding agent; do not invent headless arguments"
        );
    }

    #[test]
    fn test_builtin_shell_is_last_for_fallback() {
        let registry = AgentRegistry::builtin();
        assert_eq!(registry.agents().last().unwrap().id, "shell");
    }

    #[test]
    fn test_resolve_by_id_and_alias_case_insensitive() {
        let registry = AgentRegistry::builtin();
        assert_eq!(registry.resolve("claude").unwrap().id, "claude-code");
        assert_eq!(registry.resolve("CLAUDE_CODE").unwrap().id, "claude-code");
        assert_eq!(registry.resolve("Antigravity").unwrap().id, "gemini");
        assert_eq!(registry.resolve("open-code").unwrap().id, "opencode");
        assert_eq!(registry.resolve("generic_shell").unwrap().id, "shell");
    }

    #[test]
    fn test_resolve_unknown_returns_none() {
        let registry = AgentRegistry::builtin();
        assert!(registry.resolve("not-an-agent").is_none());
        assert!(registry.resolve("").is_none());
        assert!(registry.resolve("   ").is_none());
    }

    #[test]
    fn test_detect_from_command_paths() {
        let registry = AgentRegistry::builtin();
        assert_eq!(
            registry
                .detect_from_command("/usr/local/bin/claude --print")
                .unwrap()
                .id,
            "claude-code"
        );
        assert_eq!(
            registry.detect_from_command("codex exec").unwrap().id,
            "codex"
        );
        assert_eq!(
            registry.detect_from_command("gemini -p").unwrap().id,
            "gemini"
        );
        assert_eq!(
            registry.detect_from_command("cursor-agent").unwrap().id,
            "cursor"
        );
        assert_eq!(
            registry.detect_from_command("/bin/zsh -l").unwrap().id,
            "shell"
        );
    }

    #[test]
    fn test_detect_from_command_empty_returns_none() {
        let registry = AgentRegistry::builtin();
        assert!(registry.detect_from_command("").is_none());
        assert!(registry.detect_from_command("   ").is_none());
    }

    #[test]
    fn test_detect_from_command_matches_only_the_executable_basename() {
        let registry = AgentRegistry::builtin();

        for command in [
            "ion",
            "/usr/local/bin/ion",
            "\"/usr/local/bin/ion\"",
            "ion.exe",
            "ion.Exe",
        ] {
            assert_eq!(
                registry
                    .detect_from_command(command)
                    .map(|agent| agent.id.as_str()),
                Some("ion"),
                "expected exact Ion executable detection for {command:?}"
            );
        }

        for command in [
            "python",
            "notification",
            "union",
            "version",
            "/tmp/ion-wrapper",
            "echo ion",
        ] {
            assert_ne!(
                registry
                    .detect_from_command(command)
                    .map(|agent| agent.id.as_str()),
                Some("ion"),
                "must not infer Ion from a substring or argument in {command:?}"
            );
        }

        assert_eq!(
            registry
                .detect_from_command("/tmp/ion/tools/codex")
                .map(|agent| agent.id.as_str()),
            Some("codex"),
            "parent-directory text must not override the executable basename"
        );
        assert_eq!(
            registry
                .detect_from_command("bash -lc ion")
                .map(|agent| agent.id.as_str()),
            Some("shell"),
            "arguments must not participate in executable detection"
        );
    }

    #[test]
    fn test_builtin_capabilities_preserve_legacy_behavior() {
        let registry = AgentRegistry::builtin();
        let claude = registry.get("claude-code").unwrap();
        assert!(claude.capabilities.uses_xml_context);
        assert_eq!(claude.capabilities.startup_delay_ms, 3000);
        assert_eq!(claude.invocation_args, vec!["--print".to_string()]);

        let codex = registry.get("codex").unwrap();
        assert!(!codex.capabilities.uses_xml_context);
        assert_eq!(
            codex.capabilities.startup_delay_ms,
            DEFAULT_STARTUP_DELAY_MS
        );
        assert_eq!(codex.invocation_args, vec!["exec".to_string()]);

        let shell = registry.get("shell").unwrap();
        assert_eq!(shell.capabilities.startup_delay_ms, 500);
        assert!(shell.invocation_args.is_empty());
    }

    #[test]
    fn test_descriptor_serde_round_trip() {
        let original = AgentRegistry::builtin().get("gemini").unwrap().clone();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AgentDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_capabilities_default() {
        let caps = AgentCapabilities::default();
        assert!(!caps.uses_xml_context);
        assert_eq!(caps.startup_delay_ms, DEFAULT_STARTUP_DELAY_MS);
    }

    #[test]
    fn test_capabilities_deserialize_applies_defaults() {
        // Missing startup_delay_ms falls back to the default.
        let caps: AgentCapabilities = serde_json::from_str("{}").unwrap();
        assert_eq!(caps, AgentCapabilities::default());
    }

    #[test]
    fn test_from_toml_str_parses_agents() {
        let toml_str = r#"
[[agent]]
id = "my-agent"
label = "My Agent"
command = "my-agent"
invocation_args = ["--headless"]
aliases = ["mine"]

[agent.capabilities]
uses_xml_context = true
startup_delay_ms = 1500
"#;
        let registry = AgentRegistry::from_toml_str(toml_str).unwrap();
        assert_eq!(registry.len(), 1);
        let agent = registry.resolve("mine").unwrap();
        assert_eq!(agent.id, "my-agent");
        assert_eq!(agent.command, "my-agent");
        assert!(agent.capabilities.uses_xml_context);
        assert_eq!(agent.capabilities.startup_delay_ms, 1500);
    }

    #[test]
    fn test_from_toml_str_minimal_uses_default_capabilities() {
        let toml_str = r#"
[[agent]]
id = "bare"
label = "Bare"
command = "bare"
"#;
        let registry = AgentRegistry::from_toml_str(toml_str).unwrap();
        let agent = registry.get("bare").unwrap();
        assert_eq!(agent.capabilities, AgentCapabilities::default());
        assert!(agent.invocation_args.is_empty());
        assert!(agent.aliases.is_empty());
    }

    #[test]
    fn test_legacy_toml_defaults_to_no_runtime_launch_capabilities() {
        let registry = AgentRegistry::from_toml_str(
            r#"
[[agent]]
id = "legacy-agent"
label = "Legacy Agent"
command = "legacy-agent"
"#,
        )
        .unwrap();
        let agent = registry.get("legacy-agent").unwrap();

        assert!(agent.runtime_capabilities.is_empty());

        let serialized = toml::to_string(&RegistryFile {
            agents: vec![agent.clone()],
        })
        .unwrap();
        assert!(!serialized.contains("runtime_capabilities"));
    }

    #[test]
    fn test_custom_toml_cannot_authorize_structural_filesystem_capability() {
        use crate::role_assignment::{
            AgentRoleAssignment, AgentRoleId, EnforcementStrength, RoleCapabilityRequirement,
            RuntimeCapabilityId,
        };

        let parsed = AgentRegistry::from_toml_str(
            r#"
[[agent]]
id = "untrusted-agent"
label = "Untrusted Agent"
command = "untrusted-agent"
runtime_capabilities = [
  { capability = "filesystem.scoped", enforcement = "structural" },
]
"#,
        );

        match parsed {
            Ok(registry) => {
                let platform = AgentPlatformId::try_new("untrusted-agent").unwrap();
                let assignment = AgentRoleAssignment {
                    role: AgentRoleId::try_new("builder").unwrap(),
                    requirements: vec![RoleCapabilityRequirement {
                        capability: RuntimeCapabilityId::try_new("filesystem.scoped").unwrap(),
                        minimum_enforcement: EnforcementStrength::Structural,
                        mandatory: true,
                    }],
                };
                let compatibility = registry
                    .evaluate_role_compatibility(&platform, &assignment)
                    .unwrap();

                assert!(
                    !compatibility.launch_allowed(),
                    "operator TOML must never authorize structural filesystem enforcement"
                );
            }
            Err(error) => assert!(error.to_string().contains("runtime_capabilities")),
        }
    }

    #[test]
    fn test_registry_file_rejects_operator_declared_runtime_capabilities_with_typed_error() {
        let file: RegistryFile = toml::from_str(
            r#"
[[agent]]
id = "untrusted-agent"
label = "Untrusted Agent"
command = "untrusted-agent"
runtime_capabilities = [
  { capability = "filesystem.scoped", enforcement = "structural" },
]
"#,
        )
        .unwrap();

        let error = AgentRegistry::from_registry_file(file).unwrap_err();

        assert!(matches!(
            error,
            RegistryError::UntrustedRuntimeCapabilities { ref platform }
                if platform == "untrusted-agent"
        ));
        assert!(error.to_string().contains("runtime_capabilities"));
    }

    #[test]
    fn test_builtin_runtime_launch_capabilities_are_conservative() {
        use crate::role_assignment::EnforcementStrength;

        let registry = AgentRegistry::builtin();
        for agent in registry.agents() {
            assert_eq!(
                agent.runtime_capabilities.len(),
                2,
                "{} must advertise only the desktop wrapper facts",
                agent.id
            );
            assert!(agent.runtime_capabilities.iter().any(|support| {
                support.capability == "workspace.target"
                    && support.enforcement == EnforcementStrength::Mediated
            }));
            assert!(agent.runtime_capabilities.iter().any(|support| {
                support.capability == "process.lifecycle"
                    && support.enforcement == EnforcementStrength::Mediated
            }));
            assert!(
                agent
                    .runtime_capabilities
                    .iter()
                    .all(|support| support.capability != "filesystem.scoped"),
                "{} must not claim scoped filesystem enforcement",
                agent.id
            );
        }
    }

    #[test]
    fn test_evaluate_role_compatibility_canonicalizes_alias_identity() {
        use crate::role_assignment::{
            AgentRoleAssignment, AgentRoleId, EnforcementStrength, RoleCapabilityRequirement,
            RuntimeCapabilityId,
        };

        let registry = AgentRegistry::builtin();
        let alias = AgentPlatformId::try_new("claude").unwrap();
        let assignment = AgentRoleAssignment {
            role: AgentRoleId::try_new("builder").unwrap(),
            requirements: vec![RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::try_new("workspace.target").unwrap(),
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            }],
        };

        let compatibility = registry
            .evaluate_role_compatibility(&alias, &assignment)
            .unwrap();

        assert_eq!(compatibility.platform, "claude-code");
        assert!(compatibility.launch_allowed());
    }

    #[test]
    fn test_evaluate_role_compatibility_unknown_platform_returns_typed_error() {
        use crate::role_assignment::{AgentRoleAssignment, AgentRoleId};

        let registry = AgentRegistry::builtin();
        let unknown = AgentPlatformId::try_new("unknown-agent").unwrap();
        let assignment = AgentRoleAssignment {
            role: AgentRoleId::try_new("builder").unwrap(),
            requirements: Vec::new(),
        };

        assert!(matches!(
            registry.evaluate_role_compatibility(&unknown, &assignment),
            Err(RegistryError::UnknownPlatform(platform)) if platform == "unknown-agent"
        ));
    }

    #[test]
    fn test_evaluate_role_compatibility_preserves_typed_evaluator_errors() {
        use crate::role_assignment::{
            AgentRoleAssignment, AgentRoleId, EnforcementStrength, RoleAssignmentError,
            RuntimeCapabilityId, RuntimeCapabilitySupport,
        };

        let duplicate_capability = RuntimeCapabilityId::try_new("workspace.target").unwrap();
        let registry = AgentRegistry::from_descriptors(vec![AgentDescriptor {
            id: AgentPlatformId::try_new("duplicate-support").unwrap(),
            label: "Duplicate Support".into(),
            command: "duplicate-support".into(),
            invocation_args: Vec::new(),
            aliases: Vec::new(),
            capabilities: AgentCapabilities::default(),
            runtime_capabilities: vec![
                RuntimeCapabilitySupport {
                    capability: duplicate_capability.clone(),
                    enforcement: EnforcementStrength::Mediated,
                },
                RuntimeCapabilitySupport {
                    capability: duplicate_capability,
                    enforcement: EnforcementStrength::Structural,
                },
            ],
        }])
        .unwrap();
        let platform = AgentPlatformId::try_new("duplicate-support").unwrap();
        let assignment = AgentRoleAssignment {
            role: AgentRoleId::try_new("builder").unwrap(),
            requirements: Vec::new(),
        };

        let error = registry
            .evaluate_role_compatibility(&platform, &assignment)
            .unwrap_err();

        assert!(matches!(
            error,
            RegistryError::RoleAssignment(
                RoleAssignmentError::DuplicateRuntimeCapability(ref capability)
            ) if capability == &RuntimeCapabilityId::try_new("workspace.target").unwrap()
        ));
        assert!(error.to_string().contains("duplicate runtime capability"));
    }

    #[test]
    fn test_from_toml_str_invalid_returns_toml_error() {
        let err = AgentRegistry::from_toml_str("this is not = valid toml [[[").unwrap_err();
        assert!(matches!(err, RegistryError::Toml(_)));
        assert!(format!("{err}").contains("parse agent registry TOML"));
    }

    #[test]
    fn test_from_toml_str_rejects_platform_ids_with_whitespace_or_controls() {
        for id in ["bad id", "bad\nid", " leading", "trailing "] {
            let toml = format!(
                r#"
[[agent]]
id = {id:?}
label = "Invalid"
command = "invalid"
"#
            );
            assert!(
                AgentRegistry::from_toml_str(&toml).is_err(),
                "invalid platform id {id:?} must fail loudly"
            );
        }
    }

    #[test]
    fn test_from_toml_str_rejects_aliases_that_cannot_cross_platform_wire() {
        for alias in ["", "bad alias", "bad\nalias"] {
            let toml = format!(
                r#"
[[agent]]
id = "valid-agent"
label = "Invalid alias"
command = "valid-agent"
aliases = [{alias:?}]
"#
            );
            assert!(
                AgentRegistry::from_toml_str(&toml).is_err(),
                "alias {alias:?} must satisfy the AgentPlatformId wire grammar"
            );
        }
    }

    #[test]
    fn test_from_descriptors_rejects_empty_id() {
        let err = AgentPlatformId::try_new("  ").unwrap_err();
        assert!(matches!(err, RegistryError::EmptyId));
        assert!(format!("{err}").contains("empty id"));
    }

    #[test]
    fn test_from_descriptors_rejects_duplicate_id() {
        let dup = AgentDescriptor {
            id: AgentPlatformId::try_new("Dup").unwrap(),
            label: "x".into(),
            command: "x".into(),
            invocation_args: vec![],
            aliases: vec![],
            capabilities: AgentCapabilities::default(),
            runtime_capabilities: vec![],
        };
        let mut other = dup.clone();
        other.id = AgentPlatformId::try_new("dup").unwrap(); // case-insensitive collision
        let err = AgentRegistry::from_descriptors(vec![dup, other]).unwrap_err();
        match err {
            RegistryError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn test_from_descriptors_rejects_unicode_casefold_identity_collision() {
        let first = AgentDescriptor {
            id: AgentPlatformId::try_new("Ågent").unwrap(),
            label: "First".into(),
            command: "first".into(),
            invocation_args: vec![],
            aliases: vec![],
            capabilities: AgentCapabilities::default(),
            runtime_capabilities: vec![],
        };
        let second = AgentDescriptor {
            id: AgentPlatformId::try_new("ågent").unwrap(),
            label: "Second".into(),
            command: "second".into(),
            invocation_args: vec![],
            aliases: vec![],
            capabilities: AgentCapabilities::default(),
            runtime_capabilities: vec![],
        };

        assert!(
            AgentRegistry::from_descriptors(vec![first, second]).is_err(),
            "collision validation and identity resolution must use the same canonical key"
        );
    }

    #[test]
    fn test_from_descriptors_rejects_alias_owned_by_another_platform() {
        let registry = AgentRegistry::from_toml_str(
            r#"
[[agent]]
id = "alpha"
label = "Alpha"
command = "shared"
aliases = ["beta"]

[[agent]]
id = "beta"
label = "Beta"
command = "shared"
"#,
        );

        assert!(
            registry.is_err(),
            "identity aliases must have one owner even when command reuse is intentional"
        );
    }

    #[test]
    fn test_merge_rejects_custom_id_that_collides_with_builtin_alias() {
        let mut registry = AgentRegistry::builtin();
        let before = registry.clone();
        let overrides = AgentRegistry::from_toml_str(
            r#"
[[agent]]
id = "zsh"
label = "Custom Zsh Agent"
command = "custom-zsh"
"#,
        )
        .unwrap();

        assert!(registry.merge(overrides).is_err());
        assert_eq!(
            registry, before,
            "a rejected extension must leave the prior registry intact"
        );
    }

    #[test]
    fn test_merge_overrides_existing_and_appends_new() {
        let mut registry = AgentRegistry::builtin();
        let overrides = AgentRegistry::from_toml_str(
            r#"
[[agent]]
id = "claude-code"
label = "Claude (custom)"
command = "claude"
invocation_args = ["-p"]

[[agent]]
id = "aider"
label = "Aider"
command = "aider"
"#,
        )
        .unwrap();
        let before = registry.len();
        registry.merge(overrides).unwrap();

        // Existing id replaced, not duplicated.
        assert_eq!(registry.len(), before + 1);
        let claude = registry.get("claude-code").unwrap();
        assert_eq!(claude.label, "Claude (custom)");
        assert_eq!(claude.invocation_args, vec!["-p".to_string()]);

        // New id appended and resolvable.
        assert!(registry.get("aider").is_some());
    }

    #[test]
    fn test_load_from_path_reads_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.toml");
        std::fs::write(
            &path,
            r#"
[[agent]]
id = "ratchet"
label = "Ratchet"
command = "ratchet"
"#,
        )
        .unwrap();
        let registry = AgentRegistry::load_from_path(&path).unwrap();
        assert_eq!(registry.get("ratchet").unwrap().label, "Ratchet");
    }

    #[test]
    fn test_load_from_path_missing_file_returns_io_error() {
        let err = AgentRegistry::load_from_path(Path::new("/nonexistent/impulse/agents.toml"))
            .unwrap_err();
        assert!(matches!(err, RegistryError::Io { .. }));
        assert!(format!("{err}").contains("read agent registry"));
    }

    #[test]
    fn test_registry_for_runtime_no_env_returns_builtin() {
        let reg = AgentRegistry::registry_for_runtime_path(None).expect("builtin");
        assert!(reg.get("claude-code").is_some());
        assert!(reg.get("codex").is_some());
    }

    #[test]
    fn test_registry_for_runtime_nonexistent_file_returns_builtin() {
        let non = Path::new("/tmp/impulse-no-such-registry-bd088a7e.toml");
        let reg =
            AgentRegistry::registry_for_runtime_path(Some(non)).expect("builtin on missing file");
        assert!(reg.get("claude-code").is_some());
    }

    #[test]
    fn test_registry_for_runtime_metadata_error_does_not_fall_back_to_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, "ordinary file").unwrap();
        let invalid_child = file.join("agents.toml");

        assert!(
            matches!(
                AgentRegistry::registry_for_runtime_path(Some(&invalid_child)),
                Err(RegistryError::Io { .. })
            ),
            "metadata errors are configuration failures, not missing optional files"
        );
    }

    #[test]
    fn test_registry_for_runtime_corrupt_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.toml");
        std::fs::write(
            &p,
            "[[agent]\nid = \"x\"\nlabel = \"bad\" toml syntax error here",
        )
        .unwrap();
        let err = AgentRegistry::registry_for_runtime_path(Some(&p)).unwrap_err();
        assert!(matches!(err, RegistryError::Toml(_)));
    }

    #[test]
    fn test_registry_file_serde_round_trip() {
        let file = RegistryFile {
            agents: AgentRegistry::builtin().agents().to_vec(),
        };
        let toml_str = toml::to_string(&file).unwrap();
        let recovered: RegistryFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(recovered.agents, file.agents);
    }

    #[test]
    fn test_descriptor_matches_ignores_blank() {
        let d = AgentRegistry::builtin().get("codex").unwrap().clone();
        assert!(d.matches("codex"));
        assert!(d.matches("CODEX"));
        assert!(!d.matches(""));
        assert!(!d.matches("   "));
    }

    #[test]
    fn test_multi_backend_agent_descriptor_listing_for_terminal_cli_logins() {
        // Direct test exercising real registry (no mocks of the unit under test).
        // Verifies observable support for multiple distinct agent CLI types (claude-code, codex, etc.)
        // that can "login"/attach as terminal agents under Impulse supervision.
        let registry = AgentRegistry::builtin();
        let agents = registry.agents();
        assert!(agents.len() >= 2, "must expose multiple backends");
        assert!(registry.get("claude-code").is_some());
        assert!(registry.get("codex").is_some());
        // Listing via public API yields the descriptors.
        let listed: Vec<_> = agents.iter().map(|a| a.id.as_str()).collect();
        assert!(listed.contains(&"claude-code"));
        assert!(listed.contains(&"codex"));
        // Emit for captured output verification (contains "claude", "codex" indicators).
        eprintln!("REGISTERED_AGENT_PLATFORMS: {:?}", listed);
        for a in agents {
            eprintln!("AGENT: id={} label={} command={}", a.id, a.label, a.command);
        }
    }

    #[test]
    fn test_agent_platforms_report_from_builtin_has_claude_and_codex() {
        // Pure unit test on real registry (no eprintln, no mocks of the unit under test).
        let reg = AgentRegistry::builtin();
        let report = AgentPlatformsReport::from_registry(&reg);
        assert!(report.platforms.iter().any(|p| p.id == "claude-code"));
        assert!(report.platforms.iter().any(|p| p.id == "codex"));
        let codex = report
            .platforms
            .iter()
            .find(|platform| platform.id == "codex")
            .expect("builtin codex platform");
        assert_eq!(
            codex.runtime_capabilities,
            reg.get("codex")
                .expect("builtin codex descriptor")
                .runtime_capabilities
        );
        assert!(!report.multi_workspace_note.is_empty());
    }

    #[test]
    fn test_resolve_launch_command_prefers_registry() {
        let reg = AgentRegistry::builtin();
        let claude = AgentPlatformId::try_new("claude-code").unwrap();
        let codex = AgentPlatformId::try_new("codex").unwrap();
        let cmd = resolve_launch_command(&reg, &claude, None).unwrap();
        assert_eq!(cmd, "claude");
        let cmd2 = resolve_launch_command(&reg, &codex, None).unwrap();
        assert_eq!(cmd2, "codex");
        let override_c = resolve_launch_command(&reg, &claude, Some("/custom/claude")).unwrap();
        assert_eq!(override_c, "/custom/claude");
    }

    #[test]
    fn test_resolve_launch_command_accepts_alias_for_registered_platform() {
        let reg = AgentRegistry::builtin();
        let claude_alias = AgentPlatformId::try_new("claude").unwrap();

        assert_eq!(
            resolve_launch_command(&reg, &claude_alias, None).unwrap(),
            "claude"
        );
    }

    #[test]
    fn test_resolve_launch_command_never_falls_back_for_unknown_platform() {
        let reg = AgentRegistry::builtin();
        let missing = AgentPlatformId::try_new("missing-agent").unwrap();
        let resolved = resolve_launch_command(&reg, &missing, None);

        assert!(
            matches!(resolved, Err(RegistryError::UnknownPlatform(id)) if id == "missing-agent")
        );
    }

    #[test]
    fn test_resolve_launch_command_rejects_blank_explicit_override() {
        let reg = AgentRegistry::builtin();
        let codex = AgentPlatformId::try_new("codex").unwrap();

        assert!(
            resolve_launch_command(&reg, &codex, Some("   ")).is_err(),
            "an explicit blank override is invalid input, not an implicit default"
        );
    }

    #[test]
    fn test_resolve_launch_command_rejects_blank_registered_command() {
        let platform = AgentPlatformId::try_new("blank-command").unwrap();
        let registry = AgentRegistry::from_descriptors(vec![AgentDescriptor {
            id: platform.clone(),
            label: "Blank command".to_string(),
            command: "   ".to_string(),
            invocation_args: Vec::new(),
            aliases: Vec::new(),
            capabilities: AgentCapabilities::default(),
            runtime_capabilities: Vec::new(),
        }])
        .unwrap();

        assert!(
            resolve_launch_command(&registry, &platform, None).is_err(),
            "a registered identity without an executable command is not launchable"
        );
    }

    #[test]
    fn test_reconciled_clean_archive_has_contracts_snapshot() {
        // Proof in canonical tree that .clean was reconciled (archived into single active tree).
        // Contents of the duplicate checkout are now under archive/_archived-... or reconciled-...
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let base = std::path::Path::new(&manifest).join("../../archive");
        let has_archived = std::fs::read_dir(&base)
            .ok()
            .map(|rd| {
                rd.filter_map(|e| e.ok()).any(|e| {
                    let p = e.path();
                    let name = p.to_string_lossy();
                    if name.contains("_archived-IMPULSE") || name.contains("reconciled-from-clean")
                    {
                        let c1 = p.join("clean/crates/impulse-contracts/Cargo.toml");
                        if c1.exists() {
                            return true;
                        }
                        let c2 = p.join("crates/impulse-contracts/Cargo.toml");
                        if c2.exists() {
                            return true;
                        }
                        let c3 = p.join("full-snapshot/clean/crates/impulse-contracts/Cargo.toml");
                        if c3.exists() {
                            return true;
                        }
                    }
                    false
                })
            })
            .unwrap_or(false);
        let old_partial = std::path::Path::new(&manifest).join(
            "../../archive/reconciled-from-clean-2026-06-25/crates/impulse-contracts/Cargo.toml",
        );
        assert!(
            has_archived || old_partial.exists(),
            "reconciled archive must contain contracts crate reference from .clean"
        );
    }
}
