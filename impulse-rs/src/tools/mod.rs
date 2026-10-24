//! CLI tool discovery, installation, and update management.
//!
//! Manages external CLI tools (Claude Code, OpenCode, Codex, Gemini, cargo helpers).
//! Tool definitions come from the compile-time [`known_tools`] registry only.
//!
//! # Trust boundary
//!
//! `CliTool` derives `Serialize + Deserialize` for status reporting, but all
//! `check_cmd`, `install_cmd`, and `update_cmd` values are sourced exclusively
//! from [`known_tools`]. Shell execution via `sh -c` is safe under this
//! invariant. If tool definitions are ever loaded from external config or user
//! input, the execution sites in `init.rs` and `update.rs` MUST be hardened
//! against command injection.

pub mod benchmark;
pub mod health;
pub mod init;
pub mod list;
pub mod python;
pub mod system;
pub mod update;

use serde::{Deserialize, Serialize};

/// Represents a CLI tool that can be managed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliTool {
    /// Unique identifier (e.g., "claude-code", "opencode")
    pub id: String,
    /// Display name
    pub name: String,
    /// Installation command
    pub install_cmd: String,
    /// Update command
    pub update_cmd: String,
    /// Check if installed command
    pub check_cmd: String,
    /// URL to official docs
    pub docs_url: String,
    /// Whether currently installed (runtime field, not persisted)
    #[serde(default)]
    pub installed: bool,
    /// Version if installed
    pub version: Option<String>,
    /// Why the installation check failed (it timed out, say), when it did.
    /// The tool's status is then unknown, not "not installed".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_error: Option<String>,
}

impl CliTool {
    pub fn new(
        id: &str,
        name: &str,
        install_cmd: &str,
        update_cmd: &str,
        check_cmd: &str,
        docs_url: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            install_cmd: install_cmd.to_string(),
            update_cmd: update_cmd.to_string(),
            check_cmd: check_cmd.to_string(),
            docs_url: docs_url.to_string(),
            installed: false,
            version: None,
            check_error: None,
        }
    }

    /// "installed", "not installed", or "unknown" when the check failed.
    pub fn status_label(&self) -> &'static str {
        if self.check_error.is_some() {
            "unknown"
        } else if self.installed {
            "installed"
        } else {
            "not installed"
        }
    }
}

/// Known CLI tools registry
pub fn known_tools() -> Vec<CliTool> {
    vec![
        CliTool::new(
            "claude-code",
            "Claude Code",
            "npm install -g @anthropic-ai/claude-code",
            "npm update -g @anthropic-ai/claude-code",
            "claude --version",
            "https://docs.anthropic.com/en/docs/claude-code/overview",
        ),
        // OpenCode ships on npm as `opencode-ai` (opencode.ai/docs). The
        // PyPI name `opencode` this used is not OpenCode's: installing or
        // updating would run whatever package someone published there.
        CliTool::new(
            "opencode",
            "OpenCode",
            "npm install -g opencode-ai",
            "npm update -g opencode-ai",
            "opencode --version",
            "https://opencode.ai/docs",
        ),
        CliTool::new(
            "codex",
            "Codex CLI",
            "npm install -g @openai/codex",
            "npm update -g @openai/codex",
            "codex --version",
            "https://openai.com/codex",
        ),
        CliTool::new(
            "gemini",
            "Google Gemini CLI",
            "npm install -g @google/gemini-cli",
            "npm update -g @google/gemini-cli",
            "gemini --version",
            "https://gemini.google.com/app",
        ),
        // Build hygiene tools
        CliTool::new(
            "cargo-sweep",
            "Cargo Sweep",
            "cargo install cargo-sweep",
            "cargo install cargo-sweep",
            "cargo sweep --version",
            "https://github.com/holmgr/cargo-sweep",
        ),
        CliTool::new(
            "cargo-wipe",
            "Cargo Wipe",
            "cargo install cargo-wipe",
            "cargo install cargo-wipe",
            "cargo wipe --version",
            "https://github.com/nicholasgasior/cargo-wipe",
        ),
        CliTool::new(
            "cargo-clean-all",
            "Cargo Clean All",
            "cargo install cargo-clean-all",
            "cargo install cargo-clean-all",
            "cargo clean-all --version",
            "https://github.com/nicholasgasior/cargo-clean-all",
        ),
        CliTool::new(
            "sccache",
            "sccache",
            "cargo install sccache",
            "cargo install sccache",
            "sccache --version",
            "https://github.com/mozilla/sccache",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trust boundary in the module docs: `check_cmd`, `install_cmd` and
    /// `update_cmd` run through `sh -c`, which is safe only while every one
    /// is a plain compiled-in command. Pin that a registry edit cannot add
    /// shell control syntax without this test being changed on purpose.
    #[test]
    fn test_known_tool_commands_hold_no_shell_control_syntax() {
        const CONTROL: [char; 10] = [';', '|', '&', '$', '`', '>', '<', '(', ')', '\n'];
        for tool in known_tools() {
            for (field, command) in [
                ("check_cmd", &tool.check_cmd),
                ("install_cmd", &tool.install_cmd),
                ("update_cmd", &tool.update_cmd),
            ] {
                assert!(!command.trim().is_empty(), "{} {field} is empty", tool.id);
                assert!(
                    !command.contains(CONTROL),
                    "{} {field} has shell control syntax: {command}",
                    tool.id
                );
            }
        }
    }

    #[test]
    fn test_known_tools_have_unique_ids_and_https_docs() {
        let tools = known_tools();
        assert!(!tools.is_empty());
        let mut ids: Vec<_> = tools.iter().map(|tool| tool.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate tool id");
        for tool in &tools {
            assert!(tool.docs_url.starts_with("https://"), "{}", tool.docs_url);
            assert!(!tool.installed, "a fresh registry entry is not installed");
        }
    }

    #[test]
    fn test_status_label_distinguishes_unknown_from_not_installed() {
        let mut tool = CliTool::new("t", "T", "i", "u", "c", "https://example.test");
        assert_eq!(tool.status_label(), "not installed");
        tool.installed = true;
        assert_eq!(tool.status_label(), "installed");
        tool.check_error = Some("check timed out".into());
        assert_eq!(tool.status_label(), "unknown", "a failed check wins");
    }

    #[test]
    fn round_trip_cli_tool_and_legacy_record() {
        let mut original = CliTool::new("t", "T", "i", "u", "c", "https://example.test");
        original.installed = true;
        original.version = Some("1.2.3".into());
        let json = serde_json::to_value(&original).unwrap();
        assert!(json.get("check_error").is_none(), "absent error is omitted");
        let recovered: CliTool = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(recovered.id, original.id);
        assert_eq!(recovered.version, original.version);
        assert!(recovered.installed);

        let mut legacy = json;
        legacy.as_object_mut().unwrap().remove("installed");
        let recovered: CliTool = serde_json::from_value(legacy).unwrap();
        assert!(!recovered.installed);
        assert_eq!(recovered.check_error, None);
    }
}
