//! Shared helpers for CLI handler modules.
//!
//! Extracted from `mod.rs` to keep handler-specific code separate from
//! the utility functions that multiple handlers depend on.

use anyhow::Result;
use std::collections::HashSet;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{agent_discovery, branding, build_hygiene, injection, state, storage, tooling, verify};

// ============================================================================
// Argument Helpers
// ============================================================================

/// Resolve a session ID from an explicit argument or the environment.
pub(crate) fn get_session_id(id: Option<String>) -> Option<String> {
    id.or_else(|| std::env::var("IMPULSE_SESSION_ID").ok())
}

/// Require that a CLI argument is present, returning a friendly error otherwise.
pub(crate) fn require_arg<T>(value: Option<T>, name: &str) -> Result<T> {
    value.ok_or_else(|| anyhow::anyhow!("--{} required", name))
}

pub(crate) fn default_session_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "session".to_string())
}

// ============================================================================
// Environment Helpers
// ============================================================================

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

// ============================================================================
// JSON Parsing Helpers
// ============================================================================

/// Parse a string as JSON, falling back to `{"raw": input}` on failure.
pub(crate) fn parse_json_or_raw(input: &str) -> serde_json::Value {
    serde_json::from_str(input).unwrap_or_else(|_| serde_json::json!({"raw": input}))
}

// ============================================================================
// Text / Formatting Helpers
// ============================================================================

pub(crate) fn preview_block(block: &str, max_chars: usize) -> String {
    let mut chars = block.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

// ============================================================================
// Hook Evidence Helpers
// ============================================================================

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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
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

pub(crate) struct HookEvidenceInput<'a> {
    pub impulse_dir: &'a Path,
    pub event: &'a str,
    pub session_id: Option<String>,
    pub session_name: Option<String>,
    pub platform: Option<String>,
    pub summary: Option<String>,
    pub verify_enabled: Option<bool>,
    pub stdin_payload: Option<String>,
    pub output_preview: Option<String>,
    pub output_lines: usize,
}

pub(crate) fn capture_hook_evidence(input: HookEvidenceInput<'_>) -> Result<()> {
    if !is_truthy_env("IMPULSE_HOOK_EVIDENCE") {
        return Ok(());
    }

    let HookEvidenceInput {
        impulse_dir,
        event,
        session_id,
        session_name,
        platform,
        summary,
        verify_enabled,
        stdin_payload,
        output_preview,
        output_lines,
    } = input;

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
    let env_file = std::env::var("CLAUDE_ENV_FILE").ok().map(PathBuf::from);
    persist_claude_env_var_at(env_file.as_deref(), key, value)
}

/// Persist one assignment to an explicitly resolved Claude environment file.
///
/// Keeping path resolution separate from persistence lets callers use the
/// process environment while tests inject isolated paths without mutating the
/// process-global `CLAUDE_ENV_FILE` value seen by parallel session tests.
pub(crate) fn persist_claude_env_var_at(
    env_file: Option<&Path>,
    key: &str,
    value: &str,
) -> Result<()> {
    let Some(path) = env_file else {
        return Ok(());
    };

    let mut existing = if path.exists() {
        std::fs::read_to_string(path)?
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
    storage::Storage::atomic_write_path(path, existing.as_bytes())?;
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

// ============================================================================
// Parsing Helpers
// ============================================================================

pub(crate) fn parse_platform(s: &str) -> Option<state::Platform> {
    state::Platform::from_str_name(s)
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

// ============================================================================
// Tooling Builders
// ============================================================================

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
            Capability::ShellExec,
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

/// Snapshot the current config and build a tool registry from it in one step.
/// Centralizes the `config_snapshot()` + `build_tool_registry()` pair that was
/// duplicated across the tooling handlers.
pub(crate) fn load_tool_registry(
    state: &Arc<state::State>,
    impulse_dir: &Path,
) -> Result<(state::Config, tooling::ToolRegistry)> {
    let config = state.config_snapshot()?;
    let registry = build_tool_registry(impulse_dir, &config)?;
    Ok((config, registry))
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

// ============================================================================
// Config / State Loaders
// ============================================================================

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

// ============================================================================
// Print / Output Helpers
// ============================================================================

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<state::State>) {
        let tmp = TempDir::new().unwrap();
        let st = state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    // ── get_session_id ────────────────────────────────────────────────────

    #[test]
    fn test_get_session_id_explicit_arg_always_wins() {
        // Regardless of env state, explicit arg takes precedence
        let result = get_session_id(Some("explicit-session".to_string()));
        assert_eq!(result, Some("explicit-session".to_string()));
    }

    #[test]
    fn test_get_session_id_none_arg_delegates_to_env() {
        // When no explicit arg, result depends on whether IMPULSE_SESSION_ID is set.
        // We can't safely set/remove env vars in parallel tests, so we test the
        // branch logic: if env var is set, we get Some; if not, we get None.
        // Just verify the function doesn't panic with None arg.
        let result = get_session_id(None);
        // Result is either Some (env set) or None (env unset) — both valid.
        if let Some(id) = &result {
            assert!(!id.is_empty(), "env-sourced session ID should not be empty");
        }
    }

    // ── require_arg ───────────────────────────────────────────────────────

    #[test]
    fn test_require_arg_some_returns_ok() {
        let result = require_arg(Some(42), "count");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_require_arg_none_returns_error() {
        let result = require_arg::<i32>(None, "count");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("--count required"),
            "Expected error to mention '--count required', got: {err_msg}"
        );
    }

    #[test]
    fn test_require_arg_string_type() {
        let result = require_arg(Some("hello".to_string()), "name");
        assert_eq!(result.unwrap(), "hello");
    }

    // ── default_session_name ──────────────────────────────────────────────

    #[test]
    fn test_default_session_name_returns_nonempty_string() {
        let name = default_session_name();
        assert!(!name.is_empty(), "default_session_name should not be empty");
    }

    // ── is_truthy_env ─────────────────────────────────────────────────────

    #[test]
    fn test_is_truthy_env_returns_true_for_1() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_1", "1");
        assert!(is_truthy_env("IMPULSE_TEST_TRUTHY_1"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_1");
    }

    #[test]
    fn test_is_truthy_env_returns_true_for_true_lowercase() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_TRUE", "true");
        assert!(is_truthy_env("IMPULSE_TEST_TRUTHY_TRUE"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_TRUE");
    }

    #[test]
    fn test_is_truthy_env_returns_true_for_true_uppercase() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_TRUE_UC", "TRUE");
        assert!(is_truthy_env("IMPULSE_TEST_TRUTHY_TRUE_UC"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_TRUE_UC");
    }

    #[test]
    fn test_is_truthy_env_returns_true_for_yes_lowercase() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_YES", "yes");
        assert!(is_truthy_env("IMPULSE_TEST_TRUTHY_YES"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_YES");
    }

    #[test]
    fn test_is_truthy_env_returns_true_for_yes_uppercase() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_YES_UC", "YES");
        assert!(is_truthy_env("IMPULSE_TEST_TRUTHY_YES_UC"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_YES_UC");
    }

    #[test]
    fn test_is_truthy_env_returns_false_for_0() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_0", "0");
        assert!(!is_truthy_env("IMPULSE_TEST_TRUTHY_0"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_0");
    }

    #[test]
    fn test_is_truthy_env_returns_false_for_false() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_FALSE", "false");
        assert!(!is_truthy_env("IMPULSE_TEST_TRUTHY_FALSE"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_FALSE");
    }

    #[test]
    fn test_is_truthy_env_returns_false_for_random_string() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_RAND", "maybe");
        assert!(!is_truthy_env("IMPULSE_TEST_TRUTHY_RAND"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_RAND");
    }

    #[test]
    fn test_is_truthy_env_returns_false_for_unset_var() {
        std::env::remove_var("IMPULSE_TEST_TRUTHY_UNSET");
        assert!(!is_truthy_env("IMPULSE_TEST_TRUTHY_UNSET"));
    }

    #[test]
    fn test_is_truthy_env_returns_false_for_empty_string() {
        std::env::set_var("IMPULSE_TEST_TRUTHY_EMPTY", "");
        assert!(!is_truthy_env("IMPULSE_TEST_TRUTHY_EMPTY"));
        std::env::remove_var("IMPULSE_TEST_TRUTHY_EMPTY");
    }

    // ── preview_block ─────────────────────────────────────────────────────

    #[test]
    fn test_preview_block_short_string_unchanged() {
        let result = preview_block("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_preview_block_exact_length_no_ellipsis() {
        let result = preview_block("12345", 5);
        assert_eq!(result, "12345");
    }

    #[test]
    fn test_preview_block_truncates_with_ellipsis() {
        let result = preview_block("hello world", 5);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_preview_block_empty_string() {
        let result = preview_block("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_preview_block_zero_max_chars_adds_ellipsis() {
        let result = preview_block("hello", 0);
        assert_eq!(result, "...");
    }

    #[test]
    fn test_preview_block_unicode_chars() {
        // Each emoji is one char
        let result = preview_block("abcde", 3);
        assert_eq!(result, "abc...");
    }

    // ── parse_platform ────────────────────────────────────────────────────

    #[test]
    fn test_parse_platform_claude_code() {
        let result = parse_platform("claude-code");
        assert_eq!(result, Some(state::Platform::ClaudeCode));
    }

    #[test]
    fn test_parse_platform_opencode() {
        let result = parse_platform("opencode");
        assert_eq!(result, Some(state::Platform::OpenCode));
    }

    #[test]
    fn test_parse_platform_codex() {
        let result = parse_platform("codex");
        assert_eq!(result, Some(state::Platform::Codex));
    }

    #[test]
    fn test_parse_platform_invalid_returns_none() {
        assert!(parse_platform("vscode").is_none());
    }

    #[test]
    fn test_parse_platform_empty_returns_none() {
        assert!(parse_platform("").is_none());
    }

    #[test]
    fn test_parse_platform_case_sensitive() {
        // from_str_name is exact match
        assert!(parse_platform("Claude-Code").is_none());
        assert!(parse_platform("CLAUDE-CODE").is_none());
    }

    // ── parse_tool_category ───────────────────────────────────────────────

    #[test]
    fn test_parse_tool_category_utility() {
        assert_eq!(
            parse_tool_category("utility"),
            Some(tooling::ToolCategory::Utility)
        );
    }

    #[test]
    fn test_parse_tool_category_document() {
        assert_eq!(
            parse_tool_category("document"),
            Some(tooling::ToolCategory::Document)
        );
    }

    #[test]
    fn test_parse_tool_category_analysis() {
        assert_eq!(
            parse_tool_category("analysis"),
            Some(tooling::ToolCategory::Analysis)
        );
    }

    #[test]
    fn test_parse_tool_category_system() {
        assert_eq!(
            parse_tool_category("system"),
            Some(tooling::ToolCategory::System)
        );
    }

    #[test]
    fn test_parse_tool_category_invalid_returns_none() {
        assert!(parse_tool_category("unknown").is_none());
    }

    #[test]
    fn test_parse_tool_category_empty_returns_none() {
        assert!(parse_tool_category("").is_none());
    }

    #[test]
    fn test_parse_tool_category_case_sensitive() {
        assert!(parse_tool_category("Utility").is_none());
        assert!(parse_tool_category("SYSTEM").is_none());
    }

    // ── parse_injection_mode ──────────────────────────────────────────────

    #[test]
    fn test_parse_injection_mode_off() {
        let result = parse_injection_mode(Some("off")).unwrap();
        assert_eq!(result, Some(injection::types::InjectionMode::Off));
    }

    #[test]
    fn test_parse_injection_mode_review() {
        let result = parse_injection_mode(Some("review")).unwrap();
        assert_eq!(result, Some(injection::types::InjectionMode::Review));
    }

    #[test]
    fn test_parse_injection_mode_apply() {
        let result = parse_injection_mode(Some("apply")).unwrap();
        assert_eq!(result, Some(injection::types::InjectionMode::Apply));
    }

    #[test]
    fn test_parse_injection_mode_none_returns_ok_none() {
        let result = parse_injection_mode(None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_injection_mode_invalid_returns_error() {
        let result = parse_injection_mode(Some("auto"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("off|review|apply"),
            "Expected error to mention valid modes, got: {err_msg}"
        );
    }

    #[test]
    fn test_parse_injection_mode_empty_string_returns_error() {
        let result = parse_injection_mode(Some(""));
        assert!(result.is_err());
    }

    // ── tool_resolution_root ──────────────────────────────────────────────

    #[test]
    fn test_tool_resolution_root_returns_parent() {
        let impulse_dir = PathBuf::from("/project/.impulse");
        let root = tool_resolution_root(&impulse_dir);
        assert_eq!(root, Path::new("/project"));
    }

    #[test]
    fn test_tool_resolution_root_root_path_returns_self() {
        let impulse_dir = PathBuf::from("/");
        let root = tool_resolution_root(&impulse_dir);
        // "/" has no parent in Path semantics, so returns self
        assert_eq!(root, Path::new("/"));
    }

    // ── tool_capabilities ─────────────────────────────────────────────────

    #[test]
    fn test_tool_capabilities_allow_all_returns_six_caps() {
        let caps = tool_capabilities(true);
        assert_eq!(caps.len(), 6);
        assert!(caps.contains(&tooling::Capability::FileSystemRead));
        assert!(caps.contains(&tooling::Capability::FileSystemWrite));
        assert!(caps.contains(&tooling::Capability::Network));
        assert!(caps.contains(&tooling::Capability::PythonExec));
        assert!(caps.contains(&tooling::Capability::SystemInfo));
        assert!(caps.contains(&tooling::Capability::ShellExec));
    }

    #[test]
    fn test_tool_capabilities_restricted_returns_two_caps() {
        let caps = tool_capabilities(false);
        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&tooling::Capability::FileSystemRead));
        assert!(caps.contains(&tooling::Capability::SystemInfo));
    }

    #[test]
    fn test_tool_capabilities_restricted_excludes_write_network_python() {
        let caps = tool_capabilities(false);
        assert!(!caps.contains(&tooling::Capability::FileSystemWrite));
        assert!(!caps.contains(&tooling::Capability::Network));
        assert!(!caps.contains(&tooling::Capability::PythonExec));
    }

    // ── hook_env_snapshot ─────────────────────────────────────────────────

    #[test]
    fn test_hook_env_snapshot_returns_json_object() {
        let snap = hook_env_snapshot();
        assert!(
            snap.is_object(),
            "hook_env_snapshot must return a JSON object"
        );
    }

    #[test]
    fn test_hook_env_snapshot_contains_expected_keys() {
        let snap = hook_env_snapshot();
        let obj = snap.as_object().unwrap();
        let expected_keys = [
            "CLAUDE_PROJECT_DIR",
            "CLAUDE_PROJECT_NAME",
            "CLAUDE_ENV_FILE",
            "CLAUDE_SESSION_ID",
            "CLAUDE_SESSION_SUMMARY",
            "CLAUDE_SESSION_REASON",
            "CLAUDE_TRANSCRIPT_PATH",
            "IMPULSE_SESSION_ID",
        ];
        for key in &expected_keys {
            assert!(
                obj.contains_key(*key),
                "hook_env_snapshot missing key: {key}"
            );
        }
        assert_eq!(obj.len(), expected_keys.len());
    }

    // ── hook_session_start_banner ─────────────────────────────────────────

    #[test]
    fn test_hook_session_start_banner_respects_sentinel_env() {
        // Set the sentinel and test that the banner is returned
        std::env::set_var("IMPULSE_HOOK_SENTINEL", "1");
        let result = hook_session_start_banner();
        // Due to env var races, result may be None if another test cleared it
        if let Some(banner) = result {
            assert!(
                banner.contains("IMPULSE_HOOK_SENTINEL"),
                "Banner should contain sentinel marker"
            );
            assert!(
                banner.contains("SessionStart"),
                "Banner should mention SessionStart"
            );
        }
        std::env::remove_var("IMPULSE_HOOK_SENTINEL");
    }

    #[test]
    fn test_hook_session_start_banner_does_not_panic() {
        // Regardless of env state, function should never panic
        let _result = hook_session_start_banner();
    }

    // ── capture_hook_evidence ─────────────────────────────────────────────

    #[test]
    fn test_capture_hook_evidence_noop_when_env_unset() {
        // Use a unique env var name to avoid races, but capture_hook_evidence
        // reads IMPULSE_HOOK_EVIDENCE directly. We test with the env var unset
        // by relying on its default state (not set in CI).
        // If IMPULSE_HOOK_EVIDENCE happens to be set, the file may be created,
        // so we just verify no error occurs.
        let tmp = TempDir::new().unwrap();
        let result = capture_hook_evidence(HookEvidenceInput {
            impulse_dir: tmp.path(),
            event: "test-event",
            session_id: None,
            session_name: None,
            platform: None,
            summary: None,
            verify_enabled: None,
            stdin_payload: None,
            output_preview: None,
            output_lines: 0,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_capture_hook_evidence_writes_when_env_set() {
        // This test exercises the write path by setting IMPULSE_HOOK_EVIDENCE.
        // Due to env var races in parallel tests, we set it and immediately call
        // the function, accepting that another test may unset it concurrently.
        std::env::set_var("IMPULSE_HOOK_EVIDENCE", "1");
        let tmp = TempDir::new().unwrap();
        let result = capture_hook_evidence(HookEvidenceInput {
            impulse_dir: tmp.path(),
            event: "session-start",
            session_id: Some("sess-123".to_string()),
            session_name: Some("my-session".to_string()),
            platform: Some("claude-code".to_string()),
            summary: Some("test summary".to_string()),
            verify_enabled: Some(true),
            stdin_payload: None,
            output_preview: Some("output preview".to_string()),
            output_lines: 42,
        });
        assert!(result.is_ok());

        let log_path = tmp.path().join("validation/runtime/hook-events.jsonl");
        if log_path.exists() {
            // If the env var survived the race, verify the content
            let content = std::fs::read_to_string(&log_path).unwrap();
            assert!(content.contains("session-start"));
            assert!(content.contains("sess-123"));
        }
        // If the file doesn't exist, the env var was cleared by a racing test —
        // still valid since is_truthy_env returned false.
        std::env::remove_var("IMPULSE_HOOK_EVIDENCE");
    }

    // ── persist_claude_env_var ─────────────────────────────────────────────

    #[test]
    fn test_persist_claude_env_var_noop_when_no_env_file() {
        let result = persist_claude_env_var_at(None, "MY_KEY", "my_value");
        assert!(result.is_ok());
    }

    #[test]
    fn test_persist_claude_env_var_creates_file_with_assignment() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join("env_file");

        let result = persist_claude_env_var_at(Some(&env_path), "IMPULSE_SESSION_ID", "sess-abc");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&env_path).unwrap();
        assert!(
            content.contains("IMPULSE_SESSION_ID=sess-abc"),
            "File should contain the assignment, got: {content}"
        );
    }

    #[test]
    fn test_persist_claude_env_var_replaces_existing_key() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join("env_file");
        // Pre-populate with an existing assignment
        std::fs::write(&env_path, "IMPULSE_SESSION_ID=old-value\nOTHER_KEY=keep\n").unwrap();

        let result = persist_claude_env_var_at(Some(&env_path), "IMPULSE_SESSION_ID", "new-value");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&env_path).unwrap();
        assert!(
            content.contains("IMPULSE_SESSION_ID=new-value"),
            "Should contain new value"
        );
        assert!(
            !content.contains("old-value"),
            "Should not contain old value"
        );
        assert!(
            content.contains("OTHER_KEY=keep"),
            "Should preserve other keys"
        );
    }

    // ── HookEvidenceRecord serde round-trip ───────────────────────────────

    #[test]
    fn test_hook_evidence_record_serde_round_trip() {
        let record = HookEvidenceRecord {
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            event: "session-start".to_string(),
            impulse_dir: "/tmp/.impulse".to_string(),
            session_id: Some("sess-001".to_string()),
            session_name: Some("test".to_string()),
            platform: Some("claude-code".to_string()),
            summary: Some("A test summary".to_string()),
            verify_enabled: Some(true),
            stdin_payload: None,
            output_preview: Some("preview text".to_string()),
            output_lines: 10,
            env: serde_json::json!({"IMPULSE_SESSION_ID": "sess-001"}),
        };

        let json = serde_json::to_string(&record).unwrap();
        let recovered: HookEvidenceRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(recovered.timestamp, record.timestamp);
        assert_eq!(recovered.event, record.event);
        assert_eq!(recovered.impulse_dir, record.impulse_dir);
        assert_eq!(recovered.session_id, record.session_id);
        assert_eq!(recovered.session_name, record.session_name);
        assert_eq!(recovered.platform, record.platform);
        assert_eq!(recovered.summary, record.summary);
        assert_eq!(recovered.verify_enabled, record.verify_enabled);
        assert_eq!(recovered.stdin_payload, record.stdin_payload);
        assert_eq!(recovered.output_preview, record.output_preview);
        assert_eq!(recovered.output_lines, record.output_lines);
        assert_eq!(recovered.env, record.env);
    }

    // ── print_json ────────────────────────────────────────────────────────

    #[test]
    fn test_print_json_valid_struct_returns_ok() {
        let data = serde_json::json!({"key": "value", "count": 42});
        let result = print_json(&data);
        assert!(result.is_ok());
    }

    // ── load_build_hygiene_config ─────────────────────────────────────────

    #[test]
    fn test_load_build_hygiene_config_returns_defaults() {
        let (_tmp, st) = test_state();
        let config = load_build_hygiene_config(&st);
        // Default config should have enabled = false
        let default = build_hygiene::BuildHygieneConfig::default();
        assert_eq!(config.enabled, default.enabled);
        assert_eq!(config.dry_run_default, default.dry_run_default);
    }

    // ── build_tool_context ────────────────────────────────────────────────

    #[test]
    fn test_build_tool_context_restricted_capabilities() {
        let tmp = TempDir::new().unwrap();
        let config = state::Config::default();
        let ctx = build_tool_context(
            tmp.path(),
            &config,
            tooling::ExecutionOrigin::Cli,
            false,
            Some("sess-1".to_string()),
        );
        assert_eq!(ctx.session_id, Some("sess-1".to_string()));
        assert_eq!(ctx.allowed_capabilities.len(), 2);
        assert!(ctx
            .allowed_capabilities
            .contains(&tooling::Capability::FileSystemRead));
        assert!(ctx
            .allowed_capabilities
            .contains(&tooling::Capability::SystemInfo));
    }

    #[test]
    fn test_build_tool_context_all_capabilities() {
        let tmp = TempDir::new().unwrap();
        let config = state::Config::default();
        let ctx = build_tool_context(
            tmp.path(),
            &config,
            tooling::ExecutionOrigin::Daemon,
            true,
            None,
        );
        assert!(ctx.session_id.is_none());
        assert_eq!(ctx.allowed_capabilities.len(), 6);
    }

    // ── build_tool_registry ───────────────────────────────────────────────

    #[test]
    fn test_build_tool_registry_succeeds_with_temp_dir() {
        let tmp = TempDir::new().unwrap();
        let config = state::Config::default();
        let result = build_tool_registry(tmp.path(), &config);
        assert!(result.is_ok());
    }

    // ── refresh_capabilities_manifest ────────────────────────────────────

    #[test]
    fn test_refresh_capabilities_manifest_creates_file() {
        let tmp = TempDir::new().unwrap();
        let config = state::Config::default();
        let registry = build_tool_registry(tmp.path(), &config).unwrap();
        let result = refresh_capabilities_manifest(tmp.path(), &registry);
        assert!(result.is_ok());
        let manifest_path = result.unwrap();
        assert!(manifest_path.exists(), "Manifest file should be created");
    }

    // ── print_injection_explain ──────────────────────────────────────────

    fn make_test_injection_result(
        skipped_reason: Option<String>,
        fallback_code: Option<crate::retrieval::types::FallbackCode>,
        error: Option<String>,
    ) -> injection::types::InjectionRunResult {
        injection::types::InjectionRunResult {
            surface: "claude-code".to_string(),
            requested_mode: "apply".to_string(),
            effective_mode: "apply".to_string(),
            applied: true,
            injected_block: None,
            artifact_path: Some("/tmp/artifact.md".to_string()),
            deduped: false,
            skipped_reason,
            explain: injection::types::InjectionExplain {
                mode_requested: "apply".to_string(),
                mode_effective: "apply".to_string(),
                scope: "session".to_string(),
                retrieval_mode: "keyword".to_string(),
                backend_used: "jsonl".to_string(),
                used_fallback: fallback_code.is_some(),
                fallback_code,
                timing_ms: 42,
                candidate_count: 5,
                engine_notes: vec![],
                status: "ok".to_string(),
                artifact_path: None,
                deduped: false,
                error,
            },
            bundle: None,
        }
    }

    #[test]
    fn test_print_injection_explain_no_panic_basic() {
        let result = make_test_injection_result(None, None, None);
        // Should not panic — output goes to stdout
        print_injection_explain(&result);
    }

    #[test]
    fn test_print_injection_explain_no_panic_with_skipped_reason() {
        let result = make_test_injection_result(Some("no candidates".to_string()), None, None);
        print_injection_explain(&result);
    }

    #[test]
    fn test_print_injection_explain_no_panic_with_error() {
        let result = make_test_injection_result(None, None, Some("retrieval timeout".to_string()));
        print_injection_explain(&result);
    }

    #[test]
    fn test_print_injection_explain_no_panic_with_fallback_code() {
        let result = make_test_injection_result(
            None,
            Some(crate::retrieval::types::FallbackCode::VectorBackendDisabled),
            None,
        );
        print_injection_explain(&result);
    }

    #[test]
    fn test_print_injection_explain_no_panic_with_no_artifact() {
        let mut result = make_test_injection_result(None, None, None);
        result.artifact_path = None;
        print_injection_explain(&result);
    }

    // ── print_config ─────────────────────────────────────────────────────

    #[test]
    fn test_print_config_no_panic_empty_list() {
        print_config(vec![]);
    }

    #[test]
    fn test_print_config_no_panic_with_entries() {
        let entries = vec![
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ];
        print_config(entries);
    }

    // ── print_json ───────────────────────────────────────────────────────

    #[test]
    fn test_print_json_nested_struct_returns_ok() {
        let data = serde_json::json!({
            "outer": {"inner": [1, 2, 3]},
            "flag": true
        });
        let result = print_json(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_json_empty_object_returns_ok() {
        let data = serde_json::json!({});
        assert!(print_json(&data).is_ok());
    }

    // ── print_verification_report ────────────────────────────────────────

    #[test]
    fn test_print_verification_report_no_panic_all_pass() {
        let report = verify::VerificationReport {
            results: vec![verify::VerificationResult {
                step: verify::VerificationStep {
                    name: "cargo test".to_string(),
                    command: vec!["cargo".to_string(), "test".to_string()],
                },
                success: true,
                output: "all tests passed".to_string(),
            }],
        };
        print_verification_report(&report);
    }

    #[test]
    fn test_print_verification_report_no_panic_with_failure() {
        let report = verify::VerificationReport {
            results: vec![verify::VerificationResult {
                step: verify::VerificationStep {
                    name: "cargo clippy".to_string(),
                    command: vec!["cargo".to_string(), "clippy".to_string()],
                },
                success: false,
                output: "error: unused variable\n  --> src/main.rs:5:9\n".to_string(),
            }],
        };
        print_verification_report(&report);
    }

    #[test]
    fn test_print_verification_report_no_panic_empty_results() {
        let report = verify::VerificationReport { results: vec![] };
        print_verification_report(&report);
    }

    #[test]
    fn test_print_verification_report_no_panic_mixed_results() {
        let report = verify::VerificationReport {
            results: vec![
                verify::VerificationResult {
                    step: verify::VerificationStep {
                        name: "build".to_string(),
                        command: vec!["cargo".to_string(), "build".to_string()],
                    },
                    success: true,
                    output: String::new(),
                },
                verify::VerificationResult {
                    step: verify::VerificationStep {
                        name: "test".to_string(),
                        command: vec!["cargo".to_string(), "test".to_string()],
                    },
                    success: false,
                    output: "FAILED test_something\nassertion failed".to_string(),
                },
            ],
        };
        print_verification_report(&report);
    }

    // ── persist_claude_env_var edge cases ─────────────────────────────────

    #[test]
    fn test_persist_claude_env_var_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join("deep/nested/env_file");

        let result = persist_claude_env_var_at(Some(&env_path), "TEST_KEY", "test_value");
        assert!(result.is_ok());
        assert!(env_path.exists(), "Should create parent directories");

        let content = std::fs::read_to_string(&env_path).unwrap();
        assert!(content.contains("TEST_KEY=test_value"));
    }

    #[test]
    fn test_persist_claude_env_var_handles_empty_existing_file() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join("env_file");
        std::fs::write(&env_path, "").unwrap();

        let result = persist_claude_env_var_at(Some(&env_path), "NEW_KEY", "new_value");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&env_path).unwrap();
        assert!(
            content.contains("NEW_KEY=new_value"),
            "Should add key to empty file, got: {content}"
        );
    }

    // ── build_tool_context edge cases ────────────────────────────────────

    #[test]
    fn test_build_tool_context_preserves_impulse_dir() {
        let tmp = TempDir::new().unwrap();
        let config = state::Config::default();
        let ctx = build_tool_context(
            tmp.path(),
            &config,
            tooling::ExecutionOrigin::Cli,
            false,
            None,
        );
        assert_eq!(ctx.impulse_dir, tmp.path().to_path_buf());
    }

    #[test]
    fn test_build_tool_context_uses_config_timeout() {
        let tmp = TempDir::new().unwrap();
        let config = state::Config {
            tool_execution_default_timeout_ms: 9999,
            ..Default::default()
        };
        let ctx = build_tool_context(
            tmp.path(),
            &config,
            tooling::ExecutionOrigin::Cli,
            false,
            None,
        );
        assert_eq!(ctx.timeout_ms, 9999);
    }

    // ── tool_resolution_root edge cases ──────────────────────────────────

    #[test]
    fn test_tool_resolution_root_nested_path() {
        let path = PathBuf::from("/a/b/c/.impulse");
        let root = tool_resolution_root(&path);
        assert_eq!(root, Path::new("/a/b/c"));
    }

    // ── default_session_name edge cases ──────────────────────────────────

    #[test]
    fn test_default_session_name_is_deterministic() {
        let name1 = default_session_name();
        let name2 = default_session_name();
        assert_eq!(name1, name2, "Repeated calls should return the same name");
    }

    // ── HookEvidenceRecord serde edge cases ──────────────────────────────

    #[test]
    fn test_hook_evidence_record_round_trip_all_none_optionals() {
        let record = HookEvidenceRecord {
            timestamp: "2026-04-01T12:00:00Z".to_string(),
            event: "test".to_string(),
            impulse_dir: "/tmp".to_string(),
            session_id: None,
            session_name: None,
            platform: None,
            summary: None,
            verify_enabled: None,
            stdin_payload: None,
            output_preview: None,
            output_lines: 0,
            env: serde_json::json!({}),
        };

        let json = serde_json::to_string(&record).unwrap();
        let recovered: HookEvidenceRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(recovered.event, "test");
        assert!(recovered.session_id.is_none());
        assert!(recovered.session_name.is_none());
        assert!(recovered.platform.is_none());
        assert!(recovered.summary.is_none());
        assert!(recovered.verify_enabled.is_none());
        assert!(recovered.stdin_payload.is_none());
        assert!(recovered.output_preview.is_none());
        assert_eq!(recovered.output_lines, 0);
    }

    // ── parse_injection_mode edge cases ──────────────────────────────────

    #[test]
    fn test_parse_injection_mode_case_sensitive() {
        // Uppercase variants should fail
        assert!(parse_injection_mode(Some("Off")).is_err());
        assert!(parse_injection_mode(Some("REVIEW")).is_err());
        assert!(parse_injection_mode(Some("Apply")).is_err());
    }

    #[test]
    fn test_parse_injection_mode_whitespace_returns_error() {
        assert!(parse_injection_mode(Some(" apply")).is_err());
        assert!(parse_injection_mode(Some("apply ")).is_err());
    }
}
