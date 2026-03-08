//! `describe` and `schema` command handlers — ATCC v1 runtime introspection.
//!
//! `describe` emits a machine-readable registry of all CLI commands with
//! parameter schemas and examples. `schema` emits JSON Schema for a single
//! command path.

use anyhow::Result;
use serde::Serialize;

use crate::envelope::{write_envelope, EnvelopeBuilder, OutputFormat};

// ─── Command registry ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct CommandInfo {
    path: &'static str,
    description: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    params: Vec<ParamInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    examples: Vec<&'static str>,
    mutating: bool,
    supports_json: bool,
    supports_dry_run: bool,
}

#[derive(Serialize)]
struct ParamInfo {
    name: &'static str,
    #[serde(rename = "type")]
    param_type: &'static str,
    required: bool,
    description: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<&'static str>,
}

#[derive(Serialize)]
struct Registry {
    name: &'static str,
    version: &'static str,
    description: &'static str,
    global_flags: Vec<ParamInfo>,
    commands: Vec<CommandInfo>,
}

fn build_registry() -> Registry {
    Registry {
        name: "impulse-rs",
        version: env!("CARGO_PKG_VERSION"),
        description: "Terminal-native AI coding agent sidecar",
        global_flags: vec![
            ParamInfo {
                name: "--impulse-dir",
                param_type: "path",
                required: false,
                description: "Path to .impulse data directory",
                default: Some(".impulse"),
            },
            ParamInfo {
                name: "--verbose",
                param_type: "bool",
                required: false,
                description: "Enable verbose output",
                default: Some("false"),
            },
            ParamInfo {
                name: "--daemon",
                param_type: "bool",
                required: false,
                description: "Run in daemon mode (Unix socket IPC)",
                default: Some("false"),
            },
            ParamInfo {
                name: "--format",
                param_type: "enum(json,text,ndjson)",
                required: false,
                description: "Output format",
                default: Some("json"),
            },
        ],
        commands: build_command_list(),
    }
}

fn build_command_list() -> Vec<CommandInfo> {
    vec![
        // ── Session management ──────────────────────────────────────────
        CommandInfo {
            path: "session-start",
            description: "Create a new session and emit the session ID",
            params: vec![
                ParamInfo { name: "--name", param_type: "string", required: false, description: "Session name (defaults to current dir)", default: None },
                ParamInfo { name: "--platform", param_type: "string", required: false, description: "Platform: claude-code, opencode", default: None },
                ParamInfo { name: "--inject-mode", param_type: "enum(off,review,apply)", required: false, description: "Context injection mode", default: None },
                ParamInfo { name: "--inject-explain", param_type: "bool", required: false, description: "Show injection metadata", default: Some("false") },
            ],
            examples: vec![
                "impulse-rs session-start --name my-feature --platform claude-code --format json",
            ],
            mutating: true,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "session-end",
            description: "Close a session with a summary",
            params: vec![
                ParamInfo { name: "--session-id", param_type: "string", required: true, description: "Session ID to close", default: None },
                ParamInfo { name: "--summary", param_type: "string", required: true, description: "Session summary", default: None },
                ParamInfo { name: "--verify", param_type: "bool", required: false, description: "Run verification before ending", default: Some("false") },
                ParamInfo { name: "--sem-diff-base", param_type: "string", required: false, description: "Base ref for semantic diff capture", default: None },
            ],
            examples: vec![
                "impulse-rs session-end --session-id abc-123 --summary 'Added auth module' --format json",
            ],
            mutating: true,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "track-write",
            description: "Record a file modification in the active session",
            params: vec![
                ParamInfo { name: "--file", param_type: "path", required: true, description: "File path that was modified", default: None },
                ParamInfo { name: "--session-id", param_type: "string", required: false, description: "Session ID (or IMPULSE_SESSION_ID env)", default: None },
            ],
            examples: vec!["impulse-rs track-write --file src/main.rs --format json"],
            mutating: true,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "track-tool",
            description: "Record a tool usage in the active session",
            params: vec![
                ParamInfo { name: "--tool", param_type: "string", required: true, description: "Tool name", default: None },
                ParamInfo { name: "--session-id", param_type: "string", required: false, description: "Session ID (or IMPULSE_SESSION_ID env)", default: None },
            ],
            examples: vec!["impulse-rs track-tool --tool Bash --format json"],
            mutating: true,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "list-sessions",
            description: "List all active sessions",
            params: vec![],
            examples: vec!["impulse-rs list-sessions --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "session-info",
            description: "Get details of a specific session",
            params: vec![
                ParamInfo { name: "id", param_type: "string", required: true, description: "Session ID", default: None },
            ],
            examples: vec!["impulse-rs session-info abc-123 --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "session-conflicts",
            description: "Check for cross-session file conflicts",
            params: vec![
                ParamInfo { name: "--file", param_type: "path", required: false, description: "Specific file to check", default: None },
                ParamInfo { name: "--session-id", param_type: "string", required: false, description: "Session ID", default: None },
            ],
            examples: vec!["impulse-rs session-conflicts --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Memory & history ────────────────────────────────────────────
        CommandInfo {
            path: "genome",
            description: "Display permanent decisions/preferences (GENOME.md)",
            params: vec![],
            examples: vec!["impulse-rs genome --format text"],
            mutating: false,
            supports_json: false,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "history",
            description: "Show session history (last 20, most recent first)",
            params: vec![],
            examples: vec!["impulse-rs history --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "add-decision",
            description: "Add a decision to GENOME.md",
            params: vec![
                ParamInfo { name: "--description", param_type: "string", required: true, description: "Decision description", default: None },
                ParamInfo { name: "--rationale", param_type: "string", required: false, description: "Reasoning for the decision", default: None },
            ],
            examples: vec!["impulse-rs add-decision --description 'Use tokio for async' --rationale 'Community standard'"],
            mutating: true,
            supports_json: false,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "activity",
            description: "Show recent activity across sessions",
            params: vec![
                ParamInfo { name: "--limit", param_type: "integer", required: false, description: "Max entries", default: Some("20") },
            ],
            examples: vec!["impulse-rs activity --limit 10 --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Search & retrieval ──────────────────────────────────────────
        CommandInfo {
            path: "search-history",
            description: "Keyword/semantic search across session history",
            params: vec![
                ParamInfo { name: "--query", param_type: "string", required: true, description: "Search query", default: None },
                ParamInfo { name: "--mode", param_type: "enum(keyword,semantic)", required: false, description: "Search mode", default: Some("keyword") },
                ParamInfo { name: "--limit", param_type: "integer", required: false, description: "Max results", default: Some("10") },
                ParamInfo { name: "--offset", param_type: "integer", required: false, description: "Result offset", default: None },
                ParamInfo { name: "--page", param_type: "integer", required: false, description: "Page number", default: None },
                ParamInfo { name: "--total", param_type: "bool", required: false, description: "Include total count", default: Some("false") },
                ParamInfo { name: "--explain", param_type: "bool", required: false, description: "Show scoring explanation", default: Some("false") },
            ],
            examples: vec!["impulse-rs search-history --query 'auth module' --mode keyword --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "search-genome",
            description: "Search decision history in GENOME",
            params: vec![
                ParamInfo { name: "--query", param_type: "string", required: true, description: "Search query", default: None },
                ParamInfo { name: "--mode", param_type: "enum(keyword,semantic)", required: false, description: "Search mode", default: Some("keyword") },
                ParamInfo { name: "--limit", param_type: "integer", required: false, description: "Max results", default: Some("10") },
            ],
            examples: vec!["impulse-rs search-genome --query 'async runtime' --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Tooling ─────────────────────────────────────────────────────
        CommandInfo {
            path: "tooling-list",
            description: "List available dynamic tools",
            params: vec![
                ParamInfo { name: "--category", param_type: "enum(utility,document,analysis,system)", required: false, description: "Filter by category", default: None },
            ],
            examples: vec!["impulse-rs tooling-list --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "tooling-describe",
            description: "Describe a dynamic tool's parameters and capabilities",
            params: vec![
                ParamInfo { name: "tool_id", param_type: "string", required: true, description: "Tool ID to describe", default: None },
            ],
            examples: vec!["impulse-rs tooling-describe file-reader --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "tooling-run",
            description: "Execute a dynamic tool",
            params: vec![
                ParamInfo { name: "tool_id", param_type: "string", required: true, description: "Tool ID to execute", default: None },
                ParamInfo { name: "--params", param_type: "json", required: false, description: "JSON parameters", default: None },
            ],
            examples: vec!["impulse-rs tooling-run file-reader --params '{\"path\":\"src/main.rs\"}' --format json"],
            mutating: true,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "tooling-schema",
            description: "Export tool schemas for agent/harness discovery",
            params: vec![
                ParamInfo { name: "--format", param_type: "enum(json,markdown)", required: false, description: "Output format", default: Some("json") },
            ],
            examples: vec!["impulse-rs tooling-schema --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Guard & safety ──────────────────────────────────────────────
        CommandInfo {
            path: "guard",
            description: "Evaluate an action against guardrail rules",
            params: vec![
                ParamInfo { name: "--action", param_type: "string", required: false, description: "The action/command to evaluate", default: None },
                ParamInfo { name: "--target", param_type: "enum(bash,tool-call,file-write,any)", required: false, description: "Target type", default: Some("bash") },
                ParamInfo { name: "--list", param_type: "bool", required: false, description: "List all active rules", default: Some("false") },
            ],
            examples: vec!["impulse-rs guard --action 'rm -rf /' --target bash --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Semantic diff ───────────────────────────────────────────────
        CommandInfo {
            path: "sem-diff",
            description: "Semantic diff between two Git refs using the sem tool",
            params: vec![
                ParamInfo { name: "--base", param_type: "string", required: false, description: "Base Git ref", default: Some("HEAD~1") },
                ParamInfo { name: "--head", param_type: "string", required: false, description: "Head Git ref", default: Some("HEAD") },
                ParamInfo { name: "--session-id", param_type: "string", required: false, description: "Session to associate with", default: None },
            ],
            examples: vec!["impulse-rs sem-diff --base main --head feature --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "sem-status",
            description: "Check if the sem CLI tool is available",
            params: vec![],
            examples: vec!["impulse-rs sem-status --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Build hygiene ───────────────────────────────────────────────
        CommandInfo {
            path: "sweep",
            description: "Clean stale Rust build artifacts",
            params: vec![
                ParamInfo { name: "--dry-run", param_type: "bool", required: false, description: "Only show what would be cleaned", default: Some("true") },
                ParamInfo { name: "--path", param_type: "path", required: false, description: "Path to scan", default: None },
                ParamInfo { name: "--days", param_type: "integer", required: false, description: "Remove artifacts older than N days", default: Some("30") },
            ],
            examples: vec!["impulse-rs sweep --dry-run true --format json"],
            mutating: true,
            supports_json: false,
            supports_dry_run: true,
        },
        CommandInfo {
            path: "wipe",
            description: "Aggressive wipe of target/ directories",
            params: vec![
                ParamInfo { name: "--dry-run", param_type: "bool", required: false, description: "Only show what would be wiped", default: Some("true") },
                ParamInfo { name: "--path", param_type: "path", required: false, description: "Path to scan", default: None },
            ],
            examples: vec!["impulse-rs wipe --dry-run true"],
            mutating: true,
            supports_json: false,
            supports_dry_run: true,
        },

        // ── Introspection ───────────────────────────────────────────────
        CommandInfo {
            path: "describe",
            description: "Emit machine-readable registry of all commands and schemas",
            params: vec![],
            examples: vec!["impulse-rs describe --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "schema",
            description: "Emit JSON Schema for a specific command path",
            params: vec![
                ParamInfo { name: "command", param_type: "string", required: true, description: "Command path (e.g. 'session-start')", default: None },
            ],
            examples: vec!["impulse-rs schema session-start --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Configuration ───────────────────────────────────────────────
        CommandInfo {
            path: "init",
            description: "Initialize .impulse/ directory structure",
            params: vec![],
            examples: vec!["impulse-rs init"],
            mutating: true,
            supports_json: false,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "config",
            description: "Manage configuration (get/set/list)",
            params: vec![
                ParamInfo { name: "key", param_type: "string", required: false, description: "Config key", default: None },
                ParamInfo { name: "--value", param_type: "string", required: false, description: "Value to set", default: None },
                ParamInfo { name: "--list", param_type: "bool", required: false, description: "List all config", default: Some("false") },
            ],
            examples: vec!["impulse-rs config --list --format json"],
            mutating: true,
            supports_json: false,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "status",
            description: "Show daemon/session status",
            params: vec![],
            examples: vec!["impulse-rs status --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "system",
            description: "Display system info and Impulse environment variables",
            params: vec![],
            examples: vec!["impulse-rs system --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "health",
            description: "System health check",
            params: vec![],
            examples: vec!["impulse-rs health --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── Context injection ───────────────────────────────────────────
        CommandInfo {
            path: "orchestrate",
            description: "Task routing with optional computed routing and context injection",
            params: vec![
                ParamInfo { name: "--task", param_type: "string", required: true, description: "Task description", default: None },
                ParamInfo { name: "--inject-mode", param_type: "enum(off,review,apply)", required: false, description: "Context injection mode", default: None },
                ParamInfo { name: "--inject-explain", param_type: "bool", required: false, description: "Show injection metadata", default: Some("false") },
                ParamInfo { name: "--compute-routing", param_type: "bool", required: false, description: "Use computed routing", default: Some("false") },
            ],
            examples: vec!["impulse-rs orchestrate --task 'refactor auth module' --inject-mode review --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
        CommandInfo {
            path: "handoff",
            description: "Hand off a task to an external tool with context",
            params: vec![
                ParamInfo { name: "--tool", param_type: "string", required: true, description: "Target tool", default: None },
                ParamInfo { name: "--task", param_type: "string", required: true, description: "Task description", default: None },
                ParamInfo { name: "--session-id", param_type: "string", required: false, description: "Session ID", default: None },
                ParamInfo { name: "--notes", param_type: "string", required: false, description: "Additional notes", default: None },
            ],
            examples: vec!["impulse-rs handoff --tool claude-code --task 'review PR' --format json"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },

        // ── MCP ─────────────────────────────────────────────────────────
        CommandInfo {
            path: "mcp serve",
            description: "Serve registry-backed MCP tools over stdio or TCP",
            params: vec![
                ParamInfo { name: "--transport", param_type: "enum(stdio,tcp)", required: false, description: "Transport type", default: Some("stdio") },
                ParamInfo { name: "--port", param_type: "integer", required: false, description: "TCP port (when transport=tcp)", default: None },
            ],
            examples: vec!["impulse-rs mcp serve --transport stdio"],
            mutating: false,
            supports_json: true,
            supports_dry_run: false,
        },
    ]
}

// ─── Handlers ───────────────────────────────────────────────────────────────

pub fn handle_describe(format: OutputFormat) -> Result<()> {
    let registry = build_registry();
    let env = EnvelopeBuilder::new("describe").ok(&registry);
    write_envelope(format, &env)?;
    Ok(())
}

pub fn handle_schema(command: &str, format: OutputFormat) -> Result<()> {
    let registry = build_registry();
    let cmd = registry.commands.iter().find(|c| c.path == command);

    match cmd {
        Some(cmd_info) => {
            // Build a JSON Schema-like object from the command's param list
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for p in &cmd_info.params {
                let mut prop = serde_json::Map::new();

                // Map param_type to valid JSON Schema types
                if let Some(variants) = p
                    .param_type
                    .strip_prefix("enum(")
                    .and_then(|s| s.strip_suffix(')'))
                {
                    prop.insert("type".to_string(), serde_json::json!("string"));
                    let values: Vec<&str> = variants.split(',').collect();
                    prop.insert("enum".to_string(), serde_json::json!(values));
                } else {
                    let schema_type = match p.param_type {
                        "string" | "path" | "json" => "string",
                        "bool" => "boolean",
                        "integer" => "integer",
                        "float" | "number" => "number",
                        other => other,
                    };
                    prop.insert("type".to_string(), serde_json::json!(schema_type));
                    if p.param_type == "path" {
                        prop.insert("format".to_string(), serde_json::json!("path"));
                    }
                }

                prop.insert("description".to_string(), serde_json::json!(p.description));
                if let Some(d) = p.default {
                    prop.insert("default".to_string(), serde_json::json!(d));
                }
                properties.insert(p.name.trim_start_matches('-').to_string(), prop.into());

                if p.required {
                    required.push(serde_json::json!(p.name.trim_start_matches('-')));
                }
            }

            let schema = serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "title": format!("impulse-rs {}", command),
                "description": cmd_info.description,
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            });

            let env = EnvelopeBuilder::new("schema").ok(&schema);
            write_envelope(format, &env)?;
        }
        None => {
            let env: crate::envelope::Envelope<()> = EnvelopeBuilder::new("schema").err_details(
                "unknown_command",
                &format!("no command with path '{}'", command),
                false,
                serde_json::json!({
                    "available": registry.commands.iter().map(|c| c.path).collect::<Vec<_>>()
                }),
            );
            write_envelope(format, &env)?;
            anyhow::bail!("unknown command: {}", command);
        }
    }

    Ok(())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_expected_commands() {
        let reg = build_registry();
        assert!(!reg.commands.is_empty());
        let paths: Vec<&str> = reg.commands.iter().map(|c| c.path).collect();
        assert!(paths.contains(&"session-start"));
        assert!(paths.contains(&"describe"));
        assert!(paths.contains(&"schema"));
        assert!(paths.contains(&"guard"));
        assert!(paths.contains(&"tooling-list"));
    }

    #[test]
    fn registry_serializes_to_json() {
        let reg = build_registry();
        let json = serde_json::to_string(&reg).unwrap();
        assert!(json.contains("impulse-rs"));
        assert!(json.contains("session-start"));
    }

    #[test]
    fn schema_command_produces_json_schema() {
        let reg = build_registry();
        let cmd = reg
            .commands
            .iter()
            .find(|c| c.path == "session-start")
            .unwrap();
        assert!(!cmd.params.is_empty());
    }

    #[test]
    fn all_mutating_commands_documented() {
        let reg = build_registry();
        let mutating: Vec<&str> = reg
            .commands
            .iter()
            .filter(|c| c.mutating)
            .map(|c| c.path)
            .collect();
        // Mutating commands should include at least session-start, session-end, track-write
        assert!(mutating.contains(&"session-start"));
        assert!(mutating.contains(&"session-end"));
        assert!(mutating.contains(&"track-write"));
    }
}
