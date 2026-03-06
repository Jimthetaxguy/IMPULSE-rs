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
        CliTool::new(
            "opencode",
            "OpenCode",
            "pip install opencode",
            "pip install --upgrade opencode",
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
