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
    #[error("duplicate agent id in registry: {0}")]
    DuplicateId(String),
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
    pub id: String,
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
}

impl AgentDescriptor {
    /// Returns true if `needle` matches this descriptor's id or any alias,
    /// case-insensitively.
    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return false;
        }
        self.id.to_lowercase() == needle || self.aliases.iter().any(|a| a.to_lowercase() == needle)
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
                    id: "claude-code".into(),
                    label: "Claude Code".into(),
                    command: "claude".into(),
                    invocation_args: vec!["--print".into()],
                    aliases: vec!["claude".into(), "claude_code".into(), "claudecode".into()],
                    capabilities: AgentCapabilities {
                        uses_xml_context: true,
                        startup_delay_ms: 3000,
                    },
                },
                AgentDescriptor {
                    id: "codex".into(),
                    label: "Codex".into(),
                    command: "codex".into(),
                    invocation_args: vec!["exec".into()],
                    aliases: vec![],
                    capabilities: AgentCapabilities::default(),
                },
                AgentDescriptor {
                    id: "opencode".into(),
                    label: "OpenCode".into(),
                    command: "opencode".into(),
                    invocation_args: vec!["run".into()],
                    aliases: vec!["open-code".into()],
                    capabilities: AgentCapabilities::default(),
                },
                AgentDescriptor {
                    id: "gemini".into(),
                    label: "Gemini".into(),
                    command: "gemini".into(),
                    invocation_args: vec!["-p".into()],
                    aliases: vec!["antigravity".into()],
                    capabilities: AgentCapabilities::default(),
                },
                AgentDescriptor {
                    id: "cursor".into(),
                    label: "Cursor".into(),
                    // Cursor's headless CLI binary is `cursor-agent`. No stable
                    // public single-prompt flag is wired yet, so args stay empty
                    // rather than guessing.
                    command: "cursor-agent".into(),
                    invocation_args: vec![],
                    aliases: vec!["cursor-agent".into()],
                    capabilities: AgentCapabilities::default(),
                },
                AgentDescriptor {
                    id: "shell".into(),
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
                registry.merge(overrides);
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
        let mut registry = Self::builtin();
        if let Ok(path_str) = std::env::var(REGISTRY_PATH_ENV) {
            let trimmed = path_str.trim();
            if !trimmed.is_empty() {
                let path = Path::new(trimmed);
                if path.exists() {
                    let overrides = Self::load_from_path(path)?;
                    registry.merge(overrides);
                }
                // missing file → builtin, no error
            }
        }
        Ok(registry)
    }

    /// Merge another registry into this one: descriptors whose id already exists
    /// (case-insensitive) replace the existing entry in place; new ids append.
    pub fn merge(&mut self, other: AgentRegistry) {
        for descriptor in other.agents {
            match self
                .agents
                .iter_mut()
                .find(|existing| existing.id.eq_ignore_ascii_case(&descriptor.id))
            {
                Some(existing) => *existing = descriptor,
                None => self.agents.push(descriptor),
            }
        }
    }

    /// Insert a single descriptor, validating non-empty + unique id.
    fn insert(&mut self, descriptor: AgentDescriptor) -> Result<(), RegistryError> {
        if descriptor.id.trim().is_empty() {
            return Err(RegistryError::EmptyId);
        }
        if self
            .agents
            .iter()
            .any(|existing| existing.id.eq_ignore_ascii_case(&descriptor.id))
        {
            return Err(RegistryError::DuplicateId(descriptor.id));
        }
        self.agents.push(descriptor);
        Ok(())
    }

    /// Resolve a string (id or alias, case-insensitive) to a descriptor.
    pub fn resolve(&self, needle: &str) -> Option<&AgentDescriptor> {
        self.agents.iter().find(|d| d.matches(needle))
    }

    /// Look up a descriptor by exact canonical id (case-insensitive).
    pub fn get(&self, id: &str) -> Option<&AgentDescriptor> {
        self.agents
            .iter()
            .find(|d| d.id.eq_ignore_ascii_case(id.trim()))
    }

    /// Detect the agent that produced a command string by substring match
    /// against each descriptor's command and aliases. Returns the first match in
    /// catalog order; the trailing `shell` descriptor therefore acts as a
    /// fallback for plain shells. Returns `None` when nothing matches.
    pub fn detect_from_command(&self, command: &str) -> Option<&AgentDescriptor> {
        let haystack = command.to_lowercase();
        if haystack.trim().is_empty() {
            return None;
        }
        self.agents.iter().find(|d| {
            haystack.contains(&d.command.to_lowercase())
                || d.aliases
                    .iter()
                    .any(|a| haystack.contains(&a.to_lowercase()))
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

/// Structured info for one agent platform (for observability / listing / launch).
#[derive(Debug, Clone, Serialize)]
pub struct AgentPlatformInfo {
    pub id: String,
    pub label: String,
    pub command: String,
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

/// Resolve the command to use when launching a terminal agent for a given platform slug.
/// Prefers explicit override (if non-blank), then registry descriptor, then safe fallback.
/// This collapses the previous duplication of command source between ops and desktop.
pub fn resolve_launch_command(
    reg: &AgentRegistry,
    slug: &str,
    override_cmd: Option<&str>,
) -> String {
    if let Some(cmd) = override_cmd {
        if !cmd.trim().is_empty() {
            return cmd.to_string();
        }
    }
    if let Some(desc) = reg.get(slug) {
        return desc.command.clone();
    }
    if slug.eq_ignore_ascii_case("shell") {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
    } else {
        "sh".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_contains_canonical_agents() {
        let registry = AgentRegistry::builtin();
        assert_eq!(registry.len(), 6);
        for id in [
            "claude-code",
            "codex",
            "opencode",
            "gemini",
            "cursor",
            "shell",
        ] {
            assert!(registry.get(id).is_some(), "missing builtin agent {id}");
        }
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
    fn test_from_toml_str_invalid_returns_toml_error() {
        let err = AgentRegistry::from_toml_str("this is not = valid toml [[[").unwrap_err();
        assert!(matches!(err, RegistryError::Toml(_)));
        assert!(format!("{err}").contains("parse agent registry TOML"));
    }

    #[test]
    fn test_from_descriptors_rejects_empty_id() {
        let err = AgentRegistry::from_descriptors(vec![AgentDescriptor {
            id: "  ".into(),
            label: "x".into(),
            command: "x".into(),
            invocation_args: vec![],
            aliases: vec![],
            capabilities: AgentCapabilities::default(),
        }])
        .unwrap_err();
        assert!(matches!(err, RegistryError::EmptyId));
        assert!(format!("{err}").contains("empty id"));
    }

    #[test]
    fn test_from_descriptors_rejects_duplicate_id() {
        let dup = AgentDescriptor {
            id: "Dup".into(),
            label: "x".into(),
            command: "x".into(),
            invocation_args: vec![],
            aliases: vec![],
            capabilities: AgentCapabilities::default(),
        };
        let mut other = dup.clone();
        other.id = "dup".into(); // case-insensitive collision
        let err = AgentRegistry::from_descriptors(vec![dup, other]).unwrap_err();
        match err {
            RegistryError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
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
        registry.merge(overrides);

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
        std::env::remove_var(REGISTRY_PATH_ENV);
        let reg = AgentRegistry::registry_for_runtime().expect("builtin");
        assert!(reg.get("claude-code").is_some());
        assert!(reg.get("codex").is_some());
    }

    #[test]
    fn test_registry_for_runtime_nonexistent_file_returns_builtin() {
        let non = "/tmp/impulse-no-such-registry-bd088a7e.toml";
        std::env::set_var(REGISTRY_PATH_ENV, non);
        let reg = AgentRegistry::registry_for_runtime().expect("builtin on missing file");
        assert!(reg.get("claude-code").is_some());
        std::env::remove_var(REGISTRY_PATH_ENV);
    }

    #[test]
    fn test_registry_for_runtime_corrupt_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.toml");
        std::fs::write(&p, "[[agent]\nid = \"x\"\nlabel = \"bad\" toml syntax error here").unwrap();
        std::env::set_var(REGISTRY_PATH_ENV, p.to_str().unwrap());
        let err = AgentRegistry::registry_for_runtime().unwrap_err();
        assert!(matches!(err, RegistryError::Toml(_)));
        std::env::remove_var(REGISTRY_PATH_ENV);
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
        assert!(!report.multi_workspace_note.is_empty());
    }

    #[test]
    fn test_resolve_launch_command_prefers_registry() {
        let reg = AgentRegistry::builtin();
        let cmd = resolve_launch_command(&reg, "claude-code", None);
        assert_eq!(cmd, "claude");
        let cmd2 = resolve_launch_command(&reg, "codex", Some("  "));
        assert_eq!(cmd2, "codex");
        let override_c = resolve_launch_command(&reg, "claude-code", Some("/custom/claude"));
        assert_eq!(override_c, "/custom/claude");
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
