//! Shared helpers for CLI handler modules.
//!
//! Extracted from `mod.rs` to keep handler-specific code separate from
//! the utility functions that multiple handlers depend on.

use anyhow::Result;
use std::collections::HashSet;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

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
