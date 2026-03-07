//! CLI handler modules — extracted from run_direct_mode() in main.rs.
//!
//! Each submodule contains handler functions for a related group of CLI commands.
//! Shared helpers live here in mod.rs.

use anyhow::Result;
use std::collections::HashSet;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use crate::{
    agent_discovery, branding, build_hygiene, injection, state, stewardship, storage, tooling,
    verify,
};

pub mod agent;
pub mod build;
pub mod config;
pub mod describe;
pub mod guard;
pub mod injection_handlers;
pub mod memory;
pub mod office;
pub mod retrieval;
pub mod semantic_diff_handlers;
pub mod session;
pub mod stewardship_handlers;
pub mod system;
pub mod tooling_handlers;

// ============================================================================
// Shared helpers (moved from main.rs)
// ============================================================================

pub(crate) fn get_session_id(id: Option<String>) -> Option<String> {
    id.or_else(|| crate::env_with_fallback("IMPULSE_SESSION_ID", "COCKPIT_SESSION_ID"))
}

pub(crate) fn default_session_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "session".to_string())
}

pub(crate) fn is_truthy_env(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

pub(crate) fn read_hook_stdin_payload() -> Option<String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }

    let mut payload = String::new();
    if stdin.read_to_string(&mut payload).is_ok() {
        let trimmed = payload.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

pub(crate) fn preview_block(block: &str, max_chars: usize) -> String {
    let mut chars = block.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

pub(crate) fn hook_env_snapshot() -> serde_json::Value {
    serde_json::json!({
        "CLAUDE_PROJECT_DIR": std::env::var("CLAUDE_PROJECT_DIR").ok(),
        "CLAUDE_PROJECT_NAME": std::env::var("CLAUDE_PROJECT_NAME").ok(),
        "CLAUDE_ENV_FILE": std::env::var("CLAUDE_ENV_FILE").ok(),
        "CLAUDE_SESSION_ID": std::env::var("CLAUDE_SESSION_ID").ok(),
        "CLAUDE_SESSION_SUMMARY": std::env::var("CLAUDE_SESSION_SUMMARY").ok(),
        "CLAUDE_SESSION_REASON": std::env::var("CLAUDE_SESSION_REASON").ok(),
        "CLAUDE_TRANSCRIPT_PATH": std::env::var("CLAUDE_TRANSCRIPT_PATH").ok(),
        "IMPULSE_SESSION_ID": std::env::var("IMPULSE_SESSION_ID").ok(),
    })
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct HookEvidenceRecord {
    pub timestamp: String,
    pub event: String,
    pub impulse_dir: String,
    pub session_id: Option<String>,
    pub session_name: Option<String>,
    pub platform: Option<String>,
    pub summary: Option<String>,
    pub verify_enabled: Option<bool>,
    pub stdin_payload: Option<String>,
    pub output_preview: Option<String>,
    pub output_lines: usize,
    pub env: serde_json::Value,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn capture_hook_evidence(
    impulse_dir: &Path,
    event: &str,
    session_id: Option<String>,
    session_name: Option<String>,
    platform: Option<String>,
    summary: Option<String>,
    verify_enabled: Option<bool>,
    stdin_payload: Option<String>,
    output_preview: Option<String>,
    output_lines: usize,
) -> Result<()> {
    if !is_truthy_env("IMPULSE_HOOK_EVIDENCE") {
        return Ok(());
    }

    let validation_dir = impulse_dir.join("validation").join("runtime");
    std::fs::create_dir_all(&validation_dir)?;
    let log_path = validation_dir.join("hook-events.jsonl");

    let record = HookEvidenceRecord {
        timestamp: chrono::Utc::now().to_rfc3339(),
        event: event.to_string(),
        impulse_dir: impulse_dir.display().to_string(),
        session_id,
        session_name,
        platform,
        summary,
        verify_enabled,
        stdin_payload,
        output_preview,
        output_lines,
        env: hook_env_snapshot(),
    };

    let storage = storage::Storage::new(validation_dir);
    storage.append_jsonl("hook-events.jsonl", &record)?;
    debug_assert!(log_path.exists());
    Ok(())
}

pub(crate) fn persist_claude_env_var(key: &str, value: &str) -> Result<()> {
    let Some(env_file) = std::env::var("CLAUDE_ENV_FILE").ok() else {
        return Ok(());
    };

    let path = PathBuf::from(env_file);
    let mut existing = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };

    let assignment = format!("{key}={value}");
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| !line.starts_with(&format!("{key}=")))
        .map(|line| line.to_string())
        .collect();
    lines.push(assignment);
    existing = format!("{}\n", lines.join("\n"));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    storage::Storage::atomic_write_path(&path, existing.as_bytes())?;
    Ok(())
}

pub(crate) fn hook_session_start_banner() -> Option<String> {
    if !is_truthy_env("IMPULSE_HOOK_SENTINEL") {
        return None;
    }

    Some(format!(
        "IMPULSE_HOOK_SENTINEL: SessionStart validation marker emitted at {}.\n\
If you can explain this marker, Impulse hook startup context reached Claude in a usable form.",
        chrono::Utc::now().to_rfc3339()
    ))
}

pub(crate) fn parse_platform(s: &str) -> Option<state::Platform> {
    state::Platform::from_str_name(s)
}

pub(crate) fn tool_resolution_root(impulse_dir: &Path) -> &Path {
    impulse_dir.parent().unwrap_or(impulse_dir)
}

pub(crate) fn tool_capabilities(allow_all_capabilities: bool) -> HashSet<tooling::Capability> {
    use tooling::Capability;

    if allow_all_capabilities {
        [
            Capability::FileSystemRead,
            Capability::FileSystemWrite,
            Capability::Network,
            Capability::PythonExec,
            Capability::SystemInfo,
        ]
        .into_iter()
        .collect()
    } else {
        [Capability::FileSystemRead, Capability::SystemInfo]
            .into_iter()
            .collect()
    }
}

pub(crate) fn build_tool_registry(
    impulse_dir: &Path,
    config: &state::Config,
) -> Result<tooling::ToolRegistry> {
    let resolution_root = tool_resolution_root(impulse_dir);
    let external_tools_dir = config.resolved_external_tools_dir_from(resolution_root);
    tooling::ToolRegistry::with_runtime(impulse_dir, &external_tools_dir)
        .map_err(|err| anyhow::anyhow!("failed to load runtime tool registry: {}", err))
}

pub(crate) fn build_tool_context(
    impulse_dir: &Path,
    config: &state::Config,
    origin: tooling::ExecutionOrigin,
    allow_all_capabilities: bool,
    session_id: Option<String>,
) -> tooling::ToolContext {
    let resolution_root = tool_resolution_root(impulse_dir);
    tooling::ToolContext {
        impulse_dir: impulse_dir.to_path_buf(),
        session_id,
        allowed_capabilities: tool_capabilities(allow_all_capabilities),
        timeout_ms: config.tool_execution_default_timeout_ms,
        execution_origin: origin,
        max_output_bytes: config.tool_execution_max_output_bytes,
        max_artifacts: config.tool_execution_max_artifacts,
        allowed_read_roots: config.resolved_tool_read_roots_from(resolution_root),
        allowed_write_roots: config.resolved_tool_write_roots_from(resolution_root),
    }
}

pub(crate) fn refresh_capabilities_manifest(
    impulse_dir: &Path,
    registry: &tooling::ToolRegistry,
) -> Result<PathBuf> {
    agent_discovery::write_capabilities_manifest(impulse_dir, registry)
        .map_err(|err| anyhow::anyhow!("failed to write capabilities manifest: {}", err))
}

pub(crate) fn parse_tool_category(category: &str) -> Option<tooling::ToolCategory> {
    match category {
        "utility" => Some(tooling::ToolCategory::Utility),
        "document" => Some(tooling::ToolCategory::Document),
        "analysis" => Some(tooling::ToolCategory::Analysis),
        "system" => Some(tooling::ToolCategory::System),
        _ => None,
    }
}

pub(crate) fn load_build_hygiene_config(state: &state::State) -> build_hygiene::BuildHygieneConfig {
    let config = match state.config_snapshot() {
        Ok(c) => c,
        Err(_) => return build_hygiene::BuildHygieneConfig::default(),
    };

    build_hygiene::BuildHygieneConfig {
        enabled: config.build_hygiene_enabled,
        scan_paths: config.build_hygiene_scan_paths.clone(),
        size_threshold_gb: config.build_hygiene_size_threshold_gb,
        age_threshold_days: config.build_hygiene_age_threshold_days,
        sweep_on_session_end: config.build_hygiene_sweep_on_session_end,
        sweep_on_toolchain_update: config.build_hygiene_sweep_on_toolchain_update,
        dry_run_default: config.build_hygiene_dry_run_default,
    }
}

pub(crate) fn parse_injection_mode(
    value: Option<&str>,
) -> Result<Option<injection::types::InjectionMode>> {
    match value {
        Some(mode) => injection::types::InjectionMode::parse(mode)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("Invalid inject mode. Use off|review|apply")),
        None => Ok(None),
    }
}

pub(crate) fn print_injection_explain(result: &injection::types::InjectionRunResult) {
    println!(
        "Injection: requested={} effective={} applied={} backend={} fallback_code={} timing={}ms candidates={} status={} artifact={}",
        result.requested_mode,
        result.effective_mode,
        result.applied,
        result.explain.backend_used,
        result
            .explain
            .fallback_code
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "none".to_string()),
        result.explain.timing_ms,
        result.explain.candidate_count,
        result.explain.status,
        result
            .artifact_path
            .clone()
            .unwrap_or_else(|| "none".to_string())
    );
    if let Some(reason) = &result.skipped_reason {
        println!("  skipped_reason={}", reason);
    }
    if let Some(error) = &result.explain.error {
        println!("  error={}", error);
    }
}

pub(crate) fn print_config(config: Vec<(String, String)>) {
    branding::print_header("Configuration");
    for (k, v) in config {
        println!("  {}: {}", k, v);
    }
}

pub(crate) fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| anyhow::anyhow!("JSON serialize: {}", e))?;
    println!("{}", json);
    Ok(())
}

pub(crate) fn print_verification_report(report: &verify::VerificationReport) {
    branding::print_header("Verification Report");
    for result in &report.results {
        let status = if result.success { "PASS" } else { "FAIL" };
        println!("{} - {}", status, result.step.name);
        if !result.success {
            let tail = result
                .output
                .lines()
                .rev()
                .take(20)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            println!("\nLast output:\n{}\n", tail);
        }
    }
    println!(
        "Summary: {}",
        if report.success() {
            "ALL CHECKS PASSED"
        } else {
            "BLOCKED - fix failing checks"
        }
    );
}

// ============================================================================
// Hook Configuration Builders
// ============================================================================

pub(crate) fn build_claude_hook_config() -> serde_json::Value {
    serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs guard --action \"$INPUT\" --target bash"
                        }
                    ]
                },
                {
                    "matcher": "Write",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs guard --action \"$INPUT\" --target file"
                        }
                    ]
                },
                {
                    "matcher": "Edit",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs guard --action \"$INPUT\" --target file"
                        }
                    ]
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs track-tool --tool Bash --session-id $IMPULSE_SESSION_ID"
                        }
                    ]
                },
                {
                    "matcher": "Write",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs track-write --file \"$INPUT\" --session-id $IMPULSE_SESSION_ID"
                        }
                    ]
                },
                {
                    "matcher": "Edit",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs track-write --file \"$INPUT\" --session-id $IMPULSE_SESSION_ID"
                        }
                    ]
                }
            ],
            "SessionStart": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs session-start -n '$CLAUDE_PROJECT_NAME' -p claude-code"
                        }
                    ]
                }
            ],
            "SessionEnd": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "impulse-rs session-end --session-id $IMPULSE_SESSION_ID --summary '$CLAUDE_SESSION_SUMMARY' --verify"
                        }
                    ]
                }
            ]
        }
    })
}

pub(crate) fn build_opencode_hook_config() -> serde_json::Value {
    serde_json::json!({
        "impulse": {
            "enabled": true,
            "session_tracking": true,
            "hooks": {
                "pre_tool_use": "impulse-rs guard --action \"$INPUT\" --target any",
                "session_start": "impulse-rs session-start -n '$OPENCODE_PROJECT_NAME' -p opencode",
                "session_end": "impulse-rs session-end --session-id $IMPULSE_SESSION_ID --summary '$OPENCODE_SESSION_SUMMARY' --verify",
                "file_write": "impulse-rs track-write --file \"$OPENCODE_FILE\" --session-id \"$IMPULSE_SESSION_ID\"",
                "tool_use": "impulse-rs track-tool --tool \"$OPENCODE_TOOL_NAME\" --session-id \"$IMPULSE_SESSION_ID\""
            }
        }
    })
}

pub(crate) fn build_hook_validation_files(platform: &str) -> Vec<(PathBuf, String)> {
    match platform {
        "claude-code" | "claude" => {
            let readme = r#"# Claude Hook Validation Kit

This kit validates the real memory loop with Claude Code hooks before claiming the feature works end-to-end.

## What this proves

1. `SessionStart` runs the real `impulse-rs session-start` command and reaches Claude in usable startup context
2. `SessionStart` persists `IMPULSE_SESSION_ID` through `CLAUDE_ENV_FILE` so later hooks can reuse the same session
3. `SessionEnd` runs the real `impulse-rs session-end` command and records hook evidence
4. The persisted history/GENOME files are the source for the next session's recall

## How to run

1. Ensure the daemon is running for memory features:
   - `cargo run -- daemon`
2. Register the hooks from `settings.local.json` into your Claude project settings.
3. Start a fresh Claude session in this project.
4. Ask Claude: `What does IMPULSE_HOOK_SENTINEL mean in this project?`
5. Do a small piece of real work.
6. End the session cleanly.
7. Start a second session and ask:
   - `What happened in the previous Impulse session?`
   - `What does Impulse remember from the last run?`
8. Inspect `.impulse/validation/runtime/hook-events.jsonl`.
9. Record the result in `evidence.md`.

## Pass criteria

- Claude can explain the sentinel from `SessionStart`, not just echo terminal output.
- `.impulse/validation/runtime/hook-events.jsonl` contains both `session_start` and `session_end` events.
- The `session_start` record shows a non-empty output preview and the `session_end` record captures stdin/env metadata.
- `.impulse/HISTORY.jsonl` gains a new entry after the first session ends.
- The next session references the prior summary/files from `.impulse/HISTORY.jsonl` or `GENOME.md`.

## Failure criteria

- Claude cannot see the sentinel on startup.
- `SessionEnd` never records runtime evidence.
- The next session does not recall the prior persisted summary.
"#;

            let settings = r#"{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.impulse/validation/claude-code/session-start-sentinel.sh"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.impulse/validation/claude-code/session-end-capture.sh"
          }
        ]
      }
    ]
  }
}
"#;

            let session_start = r#"#!/bin/bash
set -eu

ROOT="${CLAUDE_PROJECT_DIR:-$(pwd)}"
VALIDATION_DIR="$ROOT/.impulse/validation/claude-code"
ARTIFACT_DIR="$VALIDATION_DIR/artifacts"
mkdir -p "$ARTIFACT_DIR"

PAYLOAD_PATH="$ARTIFACT_DIR/session-start.stdin.json"
if [ ! -t 0 ]; then
  cat > "$PAYLOAD_PATH"
else
  : > "$PAYLOAD_PATH"
fi

IMPULSE_HOOK_EVIDENCE=1 \
IMPULSE_HOOK_SENTINEL=1 \
impulse-rs -c "$ROOT/.impulse" session-start -n "${CLAUDE_PROJECT_NAME:-hook-validation}" -p claude-code < "$PAYLOAD_PATH"
"#;

            let session_end = r#"#!/bin/bash
set -eu

ROOT="${CLAUDE_PROJECT_DIR:-$(pwd)}"
VALIDATION_DIR="$ROOT/.impulse/validation/claude-code"
ARTIFACT_DIR="$VALIDATION_DIR/artifacts"
mkdir -p "$ARTIFACT_DIR"

PAYLOAD_PATH="$ARTIFACT_DIR/session-end.stdin.json"
if [ ! -t 0 ]; then
  cat > "$PAYLOAD_PATH"
else
  : > "$PAYLOAD_PATH"
fi

IMPULSE_HOOK_EVIDENCE=1 \
impulse-rs -c "$ROOT/.impulse" session-end --session-id "${IMPULSE_SESSION_ID:-missing}" --summary "${CLAUDE_SESSION_SUMMARY:-missing}" < "$PAYLOAD_PATH"
"#;

            let evidence = r#"# Claude Hook Validation Evidence

Date:
Operator:
Project:

## Run 1
- Did Claude explain `IMPULSE_HOOK_SENTINEL` correctly?
- What exact wording confirmed it saw the injected context?
- Did `.impulse/HISTORY.jsonl` gain a new entry after the session ended?
- Did `GENOME.md` change? If yes, what persisted?

## Run 2
- What prior-session facts did Claude recall?
- Did the recalled facts match `.impulse/HISTORY.jsonl` / `GENOME.md`?
- Any mismatch between hook truth and GUI display?

## Verdict
- Status: PASS / FAIL / PARTIAL
- Blocking issue:
- Next fix:
"#;

            vec![
                (
                    PathBuf::from(".impulse/validation/claude-code/README.md"),
                    readme.to_string(),
                ),
                (
                    PathBuf::from(".impulse/validation/claude-code/settings.local.json"),
                    settings.to_string(),
                ),
                (
                    PathBuf::from(".impulse/validation/claude-code/session-start-sentinel.sh"),
                    session_start.to_string(),
                ),
                (
                    PathBuf::from(".impulse/validation/claude-code/session-end-capture.sh"),
                    session_end.to_string(),
                ),
                (
                    PathBuf::from(".impulse/validation/claude-code/evidence.md"),
                    evidence.to_string(),
                ),
            ]
        }
        _ => Vec::new(),
    }
}

pub(crate) fn write_hook_validation_kit(platform: &str) -> Result<Vec<PathBuf>> {
    let files = build_hook_validation_files(platform);
    if files.is_empty() {
        anyhow::bail!("Unsupported platform for validation kit: {}", platform);
    }

    let mut written = Vec::new();
    for (path, content) in files {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        stewardship::atomic_write_file(&path, content.as_bytes())?;
        #[cfg(unix)]
        if path.extension().and_then(|ext| ext.to_str()) == Some("sh") {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms)?;
        }
        written.push(path);
    }

    Ok(written)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod hook_config_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_claude_hook_config_includes_guard() {
        let config = build_claude_hook_config();

        assert!(
            config.get("hooks").is_some(),
            "config must have 'hooks' key"
        );

        let hooks = &config["hooks"];

        let pre_tool_use = hooks
            .get("PreToolUse")
            .expect("hooks must have 'PreToolUse' key");
        assert!(pre_tool_use.is_array(), "PreToolUse must be an array");

        let pre_arr = pre_tool_use.as_array().unwrap();
        assert!(
            !pre_arr.is_empty(),
            "PreToolUse must have at least one entry"
        );

        let bash_guard = pre_arr
            .iter()
            .find(|entry| entry.get("matcher").and_then(|m| m.as_str()) == Some("Bash"))
            .expect("PreToolUse must have a Bash matcher");
        let bash_hooks = bash_guard["hooks"].as_array().unwrap();
        let bash_cmd = bash_hooks[0]["command"].as_str().unwrap();
        assert!(
            bash_cmd.contains("impulse-rs guard"),
            "Bash PreToolUse hook must invoke 'impulse-rs guard', got: {}",
            bash_cmd
        );

        let post_tool_use = hooks
            .get("PostToolUse")
            .expect("hooks must have 'PostToolUse' key");
        assert!(post_tool_use.is_array(), "PostToolUse must be an array");

        let post_arr = post_tool_use.as_array().unwrap();
        let bash_track = post_arr
            .iter()
            .find(|entry| entry.get("matcher").and_then(|m| m.as_str()) == Some("Bash"))
            .expect("PostToolUse must have a Bash matcher");
        let track_cmd = bash_track["hooks"].as_array().unwrap()[0]["command"]
            .as_str()
            .unwrap();
        assert!(
            track_cmd.contains("impulse-rs track-tool"),
            "Bash PostToolUse hook must invoke 'impulse-rs track-tool', got: {}",
            track_cmd
        );

        assert!(
            hooks.get("SessionStart").is_some(),
            "hooks must have 'SessionStart' key"
        );
        assert!(
            hooks.get("SessionEnd").is_some(),
            "hooks must have 'SessionEnd' key"
        );
    }

    #[test]
    fn test_opencode_hook_config_includes_guard() {
        let config = build_opencode_hook_config();

        let impulse = config
            .get("impulse")
            .expect("config must have 'impulse' key");
        assert_eq!(impulse["enabled"], true);
        assert_eq!(impulse["session_tracking"], true);

        let hooks = impulse.get("hooks").expect("impulse must have 'hooks' key");

        let pre_tool = hooks
            .get("pre_tool_use")
            .expect("hooks must have 'pre_tool_use' key");
        let pre_tool_str = pre_tool.as_str().unwrap();
        assert!(
            pre_tool_str.contains("impulse-rs guard"),
            "pre_tool_use must invoke 'impulse-rs guard', got: {}",
            pre_tool_str
        );

        assert!(
            hooks.get("session_start").is_some(),
            "hooks must have 'session_start'"
        );
        assert!(
            hooks.get("session_end").is_some(),
            "hooks must have 'session_end'"
        );
        assert!(
            hooks.get("file_write").is_some(),
            "hooks must have 'file_write'"
        );
        assert!(
            hooks.get("tool_use").is_some(),
            "hooks must have 'tool_use'"
        );
    }

    #[test]
    fn test_validation_kit_uses_runtime_impulse_commands() {
        let files = build_hook_validation_files("claude-code");
        let settings = files
            .iter()
            .find(|(path, _)| path.ends_with("settings.local.json"))
            .map(|(_, content)| content.clone())
            .expect("settings.local.json missing");
        let session_start = files
            .iter()
            .find(|(path, _)| path.ends_with("session-start-sentinel.sh"))
            .map(|(_, content)| content.clone())
            .expect("session-start script missing");
        let session_end = files
            .iter()
            .find(|(path, _)| path.ends_with("session-end-capture.sh"))
            .map(|(_, content)| content.clone())
            .expect("session-end script missing");

        assert!(settings.contains("session-start-sentinel.sh"));
        assert!(session_start.contains("impulse-rs -c \"$ROOT/.impulse\" session-start"));
        assert!(session_start.contains("IMPULSE_HOOK_EVIDENCE=1"));
        assert!(session_start.contains("IMPULSE_HOOK_SENTINEL=1"));
        assert!(session_end.contains("impulse-rs -c \"$ROOT/.impulse\" session-end"));
        assert!(session_end.contains("IMPULSE_HOOK_EVIDENCE=1"));
    }

    #[test]
    fn test_persist_claude_env_var_replaces_existing_assignment() {
        let dir = TempDir::new().unwrap();
        let env_file = dir.path().join("claude.env");
        std::fs::write(&env_file, "IMPULSE_SESSION_ID=old\nOTHER_KEY=keep\n").unwrap();
        std::env::set_var("CLAUDE_ENV_FILE", &env_file);

        persist_claude_env_var("IMPULSE_SESSION_ID", "new-session").unwrap();

        let updated = std::fs::read_to_string(&env_file).unwrap();
        assert!(updated.contains("IMPULSE_SESSION_ID=new-session"));
        assert!(updated.contains("OTHER_KEY=keep"));
        assert!(!updated.contains("IMPULSE_SESSION_ID=old"));

        std::env::remove_var("CLAUDE_ENV_FILE");
    }
}
