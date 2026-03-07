use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::sync::Arc;

pub mod token_tracker;

pub mod agent;
pub mod agent_discovery;
pub mod branding;
pub mod build_hygiene;
pub mod client;
pub mod context_lifecycle;
pub mod credentials;
pub mod daemon;
pub mod docs;
pub mod error;
pub mod guardrail;
pub mod injection;
pub mod integration_tests;
pub mod llm_backends;
pub mod mcp;
pub mod memory;
pub mod monty;
pub mod notification;
pub mod office;
pub mod ops_workbench;
pub mod orchestration;
pub mod plugin;
pub mod retrieval;
pub mod session;
pub mod state;
pub mod stewardship;
pub mod storage;
pub mod tooling;
pub mod tools;
pub mod ui;
pub mod verify;

use state::Platform;

#[derive(Parser)]
#[command(name = "impulse-rs")]
#[command(about = "Terminal-native AI coding agent sidecar", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short = 'c', long, default_value = ".impulse")]
    impulse_dir: PathBuf,

    #[arg(short, long)]
    verbose: bool,

    #[arg(long)]
    daemon: bool,

    #[arg(long)]
    socket: Option<PathBuf>,
}

#[derive(Subcommand)]
enum McpCommands {
    /// Serve the registry-backed MCP interface
    Serve {
        /// Transport: stdio (default) or tcp
        #[arg(long, default_value = "stdio")]
        transport: String,
        /// TCP port when using --transport tcp
        #[arg(long)]
        port: Option<u16>,
    },
}

#[derive(Subcommand)]
enum Commands {
    Daemon {
        #[arg(long)]
        stop: bool,
    },
    Run,
    SessionStart {
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        platform: Option<String>,
        #[arg(long)]
        inject_mode: Option<String>,
        #[arg(long)]
        inject_explain: bool,
    },
    SessionEnd {
        #[arg(short, long)]
        session_id: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        verify: bool,
    },
    TrackWrite {
        #[arg(short, long)]
        file: String,
        #[arg(long)]
        session_id: Option<String>,
    },
    TrackTool {
        #[arg(short, long)]
        tool: String,
        #[arg(long)]
        session_id: Option<String>,
    },
    ListSessions,
    SessionInfo {
        id: String,
    },
    /// Check for cross-session file conflicts
    SessionConflicts {
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
    },
    Status,
    Chat {
        #[arg(short, long)]
        session_id: String,
        #[arg(short, long)]
        message: String,
        #[arg(long)]
        inject_mode: Option<String>,
        #[arg(long)]
        inject_explain: bool,
    },
    Genome,
    History,
    ListProviders,
    AddDecision {
        #[arg(short, long)]
        description: String,
        #[arg(short, long)]
        rationale: Option<String>,
    },
    Init,
    Config {
        key: Option<String>,
        #[arg(short, long)]
        value: Option<String>,
        #[arg(long)]
        list: bool,
    },
    /// Extract structured data from content using Monty
    Extract {
        #[arg(short, long)]
        content: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Analyze SWARM coordination patterns between agents
    Swarm {
        #[arg(long)]
        agent_a: String,
        #[arg(long)]
        agent_b: String,
        #[arg(long, default_value = "0.88")]
        threshold: f64,
        #[arg(long)]
        json: bool,
    },
    Activity {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    Hooks {
        #[arg(short, long, default_value = "all")]
        platform: String,
    },
    /// Generate a reproducible validation kit for real Claude Code hook testing
    ValidateHooks {
        #[arg(short, long, default_value = "claude-code")]
        platform: String,
    },
    Orchestrate {
        #[arg(short, long)]
        task: String,
        #[arg(long)]
        inject_mode: Option<String>,
        #[arg(long)]
        inject_explain: bool,
        #[arg(long)]
        compute_routing: bool,
    },
    Handoff {
        #[arg(short = 'o', long)]
        tool: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        inject_mode: Option<String>,
        #[arg(long)]
        inject_explain: bool,
    },
    SyncContext {
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        inject_mode: Option<String>,
        #[arg(long)]
        inject_explain: bool,
    },
    /// Compute dynamic injection selection using Monty
    ComputeInjection {
        #[arg(short, long)]
        query: String,
        #[arg(long, default_value = "5")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Verify,
    SearchHistory {
        #[arg(short, long)]
        query: String,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        backend: Option<String>,
        #[arg(short, long)]
        limit: Option<usize>,
        #[arg(long)]
        offset: Option<usize>,
        #[arg(short, long)]
        page: Option<usize>,
        #[arg(long)]
        total: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        json: bool,
    },
    SearchGenome {
        #[arg(short, long)]
        query: String,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        backend: Option<String>,
        #[arg(short, long)]
        limit: Option<usize>,
        #[arg(long)]
        offset: Option<usize>,
        #[arg(short, long)]
        page: Option<usize>,
        #[arg(long)]
        total: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        json: bool,
    },
    IndexMemory {
        #[arg(long, default_value = "all")]
        scope: String,
        #[arg(long)]
        rebuild: bool,
    },
    RetrievalStatus {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },
    // Tools management commands
    Tools {
        #[arg(default_value = "list")]
        subcommand: String,
        #[arg(long)]
        tool: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    // Docs and model management commands
    Docs {
        #[arg(default_value = "list")]
        subcommand: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        force: bool,
    },
    // Model management commands
    Model {
        #[arg(default_value = "list")]
        subcommand: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    // Office document handling commands
    Office {
        #[arg(default_value = "info")]
        subcommand: String,
        /// Path to document file
        #[arg(long)]
        file: Option<String>,
        /// Extraction goal (for extract subcommand)
        #[arg(long)]
        goal: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    // Credential management commands
    Credentials {
        #[arg(default_value = "list")]
        subcommand: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        socket_path: Option<String>,
        #[arg(long)]
        tool: Option<String>,
    },
    // Stewardship commands
    Steward {
        #[arg(default_value = "status")]
        subcommand: String,
        /// Transcript path for analyze/compact
        #[arg(long)]
        transcript: Option<PathBuf>,
        /// Session ID for compact
        #[arg(long)]
        session_id: Option<String>,
        /// Proposal ID for approve/reject
        #[arg(long)]
        id: Option<String>,
        /// JSON output
        #[arg(long)]
        json: bool,
    },
    // Python calculation command
    Calc {
        #[arg(short, long)]
        expression: String,
    },
    // Python execution command
    Exec {
        #[arg(short, long)]
        code: String,
    },
    // System info command
    System {},
    // Analyze session/performance
    Analyze {
        #[arg(short, long)]
        session_id: Option<String>,
        #[arg(long, default_value = "all")]
        scope: String,
    },
    // Health check
    Health {},
    // Quick summary
    Summary {},
    // Build hygiene: sweep stale Rust build artifacts
    Sweep {
        /// Only show what would be cleaned (default: true)
        #[arg(long)]
        dry_run: Option<bool>,
        /// Path to scan (default: configured scan_paths)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Remove artifacts older than N days (default: 30)
        #[arg(long)]
        days: Option<u32>,
        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    // Build hygiene: aggressive wipe of target/ dirs
    Wipe {
        /// Only show what would be wiped (default: true)
        #[arg(long)]
        dry_run: Option<bool>,
        /// Path to scan
        #[arg(long)]
        path: Option<PathBuf>,
    },
    // Build hygiene: workspace-wide cargo clean
    CleanAll {
        /// Only show what would be cleaned (default: true)
        #[arg(long)]
        dry_run: Option<bool>,
    },
    // Build hygiene: setup sccache compilation cache
    SccacheSetup {
        /// Only check status, don't modify config
        #[arg(long)]
        check: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    // Build hygiene: disk usage report for Rust build artifacts
    BuildHealth {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List available dynamic tools
    ToolingList {
        /// Filter by category (utility, document, analysis, system)
        #[arg(long)]
        category: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Describe a dynamic tool's parameters and capabilities
    ToolingDescribe {
        /// Tool ID to describe
        tool_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Execute a dynamic tool
    ToolingRun {
        /// Tool ID to execute
        tool_id: String,
        /// JSON parameters
        #[arg(long)]
        params: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Export tool schemas for agent/harness discovery (Claude Code, OpenCode)
    ToolingSchema {
        /// Format: "json" (default) or "markdown"
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Validate manifest-defined external process tools
    ToolingValidate {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reload runtime tooling and refresh the capabilities manifest
    ToolingReload {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Serve registry-backed MCP tools
    Mcp {
        #[command(subcommand)]
        subcommand: McpCommands,
    },
    /// Configure the Impulse Agent (LLM-powered coordination)
    AgentConfigure {
        /// LLM provider: anthropic, openai, minimax
        #[arg(long)]
        provider: Option<String>,
        /// API key for the provider
        #[arg(long)]
        api_key: Option<String>,
        /// Model to use (provider-specific)
        #[arg(long)]
        model: Option<String>,
        /// CLI harness: claude-code, opencode
        #[arg(long)]
        harness: Option<String>,
        /// Enable automatic cross-pane review
        #[arg(long)]
        auto_review: bool,
        /// Enable automatic cross-pane coordination
        #[arg(long)]
        auto_coordinate: bool,
    },
    /// Show Impulse Agent status
    AgentStatus {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Ask the Impulse Agent to review a pane or general query
    AgentQuery {
        /// The prompt to send to the agent
        #[arg(short, long)]
        prompt: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Evaluate an action against guardrail rules
    Guard {
        /// The action/command to evaluate
        #[arg(long)]
        action: Option<String>,
        /// Target type: bash, tool-call, file-write, any
        #[arg(long, default_value = "bash")]
        target: String,
        /// List all active rules
        #[arg(long)]
        list: bool,
        /// Enable a rule by ID
        #[arg(long)]
        enable: Option<String>,
        /// Disable a rule by ID
        #[arg(long)]
        disable: Option<String>,
        /// Output results as JSON (matches daemon IPC format)
        #[arg(long)]
        json: bool,
    },
    /// Show conflict analytics and statistics
    Analytics {
        /// Analytics type: conflicts
        #[arg(default_value = "conflicts")]
        subcommand: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Time period: day, week, month, all
        #[arg(long, default_value = "all")]
        period: String,
    },
}

#[derive(Debug, serde::Serialize)]
struct HookEvidenceRecord {
    timestamp: String,
    event: String,
    impulse_dir: String,
    session_id: Option<String>,
    session_name: Option<String>,
    platform: Option<String>,
    summary: Option<String>,
    verify_enabled: Option<bool>,
    stdin_payload: Option<String>,
    output_preview: Option<String>,
    output_lines: usize,
    env: serde_json::Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();
    cli.impulse_dir = resolve_impulse_dir(cli.impulse_dir);
    if cli.daemon {
        run_daemon_mode(cli).await
    } else {
        run_direct_mode(cli).await
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Read an env var with fallback to a deprecated name, emitting a warning on stderr.
fn env_with_fallback(new_name: &str, old_name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(new_name) {
        return Some(v);
    }
    if let Ok(v) = std::env::var(old_name) {
        eprintln!(
            "Warning: {} is deprecated, use {} instead",
            old_name, new_name
        );
        return Some(v);
    }
    None
}

/// Resolve the data directory, falling back to `.cockpit/` if `.impulse/` doesn't exist.
fn resolve_impulse_dir(requested: PathBuf) -> PathBuf {
    if requested.exists() {
        return requested;
    }
    // Fall back to old .cockpit/ directory
    let old_dir = if requested.ends_with(".impulse") {
        requested.parent().map(|p| p.join(".cockpit"))
    } else {
        None
    };
    if let Some(old) = old_dir {
        if old.exists() {
            eprintln!(
                "Warning: using legacy .cockpit/ directory. Rename to .impulse/ to silence this warning."
            );
            return old;
        }
    }
    requested
}

fn get_session_id(id: Option<String>) -> Option<String> {
    id.or_else(|| env_with_fallback("IMPULSE_SESSION_ID", "COCKPIT_SESSION_ID"))
}

fn default_session_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "session".to_string())
}

fn is_truthy_env(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn read_hook_stdin_payload() -> Option<String> {
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

fn preview_block(block: &str, max_chars: usize) -> String {
    let mut chars = block.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn hook_env_snapshot() -> serde_json::Value {
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

#[allow(clippy::too_many_arguments)]
fn capture_hook_evidence(
    impulse_dir: &std::path::Path,
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

fn persist_claude_env_var(key: &str, value: &str) -> Result<()> {
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

fn hook_session_start_banner() -> Option<String> {
    if !is_truthy_env("IMPULSE_HOOK_SENTINEL") {
        return None;
    }

    Some(format!(
        "IMPULSE_HOOK_SENTINEL: SessionStart validation marker emitted at {}.\n\
If you can explain this marker, Impulse hook startup context reached Claude in a usable form.",
        chrono::Utc::now().to_rfc3339()
    ))
}

fn parse_platform(s: &str) -> Option<Platform> {
    Platform::from_str_name(s)
}

fn tool_resolution_root(impulse_dir: &std::path::Path) -> &std::path::Path {
    impulse_dir.parent().unwrap_or(impulse_dir)
}

fn tool_capabilities(
    allow_all_capabilities: bool,
) -> std::collections::HashSet<tooling::Capability> {
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

fn build_tool_registry(
    impulse_dir: &std::path::Path,
    config: &state::Config,
) -> Result<tooling::ToolRegistry> {
    let resolution_root = tool_resolution_root(impulse_dir);
    let external_tools_dir = config.resolved_external_tools_dir_from(resolution_root);
    tooling::ToolRegistry::with_runtime(impulse_dir, &external_tools_dir)
        .map_err(|err| anyhow::anyhow!("failed to load runtime tool registry: {}", err))
}

pub(crate) fn build_tool_context(
    impulse_dir: &std::path::Path,
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

fn refresh_capabilities_manifest(
    impulse_dir: &std::path::Path,
    registry: &tooling::ToolRegistry,
) -> Result<PathBuf> {
    agent_discovery::write_capabilities_manifest(impulse_dir, registry)
        .map_err(|err| anyhow::anyhow!("failed to write capabilities manifest: {}", err))
}

fn parse_tool_category(category: &str) -> Option<tooling::ToolCategory> {
    match category {
        "utility" => Some(tooling::ToolCategory::Utility),
        "document" => Some(tooling::ToolCategory::Document),
        "analysis" => Some(tooling::ToolCategory::Analysis),
        "system" => Some(tooling::ToolCategory::System),
        _ => None,
    }
}

/// Load build hygiene config from state, falling back to defaults
fn load_build_hygiene_config(state: &state::State) -> build_hygiene::BuildHygieneConfig {
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

fn parse_injection_mode(value: Option<&str>) -> Result<Option<injection::types::InjectionMode>> {
    match value {
        Some(mode) => injection::types::InjectionMode::parse(mode)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("Invalid inject mode. Use off|review|apply")),
        None => Ok(None),
    }
}

fn print_injection_explain(result: &injection::types::InjectionRunResult) {
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

fn print_config(config: Vec<(String, String)>) {
    branding::print_header("Configuration");
    for (k, v) in config {
        println!("  {}: {}", k, v);
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| anyhow::anyhow!("JSON serialize: {}", e))?;
    println!("{}", json);
    Ok(())
}

fn print_verification_report(report: &verify::VerificationReport) {
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

/// Build the Claude Code hook configuration JSON value.
///
/// Includes both PreToolUse guard hooks (for pre-execution guardrail evaluation)
/// and PostToolUse tracking hooks (for post-observation recording).
fn build_claude_hook_config() -> serde_json::Value {
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

/// Build the OpenCode hook configuration JSON value.
///
/// Includes pre_tool_use guard hook alongside existing tracking hooks.
fn build_opencode_hook_config() -> serde_json::Value {
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

fn build_hook_validation_files(platform: &str) -> Vec<(std::path::PathBuf, String)> {
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
                    std::path::PathBuf::from(".impulse/validation/claude-code/README.md"),
                    readme.to_string(),
                ),
                (
                    std::path::PathBuf::from(".impulse/validation/claude-code/settings.local.json"),
                    settings.to_string(),
                ),
                (
                    std::path::PathBuf::from(
                        ".impulse/validation/claude-code/session-start-sentinel.sh",
                    ),
                    session_start.to_string(),
                ),
                (
                    std::path::PathBuf::from(
                        ".impulse/validation/claude-code/session-end-capture.sh",
                    ),
                    session_end.to_string(),
                ),
                (
                    std::path::PathBuf::from(".impulse/validation/claude-code/evidence.md"),
                    evidence.to_string(),
                ),
            ]
        }
        _ => Vec::new(),
    }
}

fn write_hook_validation_kit(platform: &str) -> Result<Vec<std::path::PathBuf>> {
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod hook_config_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_claude_hook_config_includes_guard() {
        let config = build_claude_hook_config();

        // Verify top-level "hooks" key exists
        assert!(
            config.get("hooks").is_some(),
            "config must have 'hooks' key"
        );

        let hooks = &config["hooks"];

        // Verify PreToolUse section exists with guard commands
        let pre_tool_use = hooks
            .get("PreToolUse")
            .expect("hooks must have 'PreToolUse' key");
        assert!(pre_tool_use.is_array(), "PreToolUse must be an array");

        let pre_arr = pre_tool_use.as_array().unwrap();
        assert!(
            !pre_arr.is_empty(),
            "PreToolUse must have at least one entry"
        );

        // Check that the Bash matcher guard is present
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

        // Verify PostToolUse section still exists for tracking
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

        // Verify SessionStart and SessionEnd exist
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

        // Verify pre_tool_use guard is present
        let pre_tool = hooks
            .get("pre_tool_use")
            .expect("hooks must have 'pre_tool_use' key");
        let pre_tool_str = pre_tool.as_str().unwrap();
        assert!(
            pre_tool_str.contains("impulse-rs guard"),
            "pre_tool_use must invoke 'impulse-rs guard', got: {}",
            pre_tool_str
        );

        // Verify existing tracking hooks are still present
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

// ============================================================================
// Daemon Mode
// ============================================================================

async fn run_daemon_mode(cli: Cli) -> Result<()> {
    let socket_path = cli.socket.unwrap_or_else(|| {
        let new_sock = cli.impulse_dir.join("sockets").join("impulse.sock");
        if new_sock.exists() {
            return new_sock;
        }
        let old_sock = cli.impulse_dir.join("sockets").join("cockpit.sock");
        if old_sock.exists() {
            eprintln!("Warning: using legacy cockpit.sock. Rename to impulse.sock to silence this warning.");
            return old_sock;
        }
        new_sock
    });
    let client = client::DaemonClient::new(socket_path);

    match cli.command {
        Commands::Daemon { stop } => {
            if stop {
                println!("Stopping daemon...");
                let _ = client.ping().await;
                println!("Daemon stopped");
            } else {
                println!("Daemon running");
                let status = client.status().await?;
                print_json(&status)?;
            }
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode: _,
            inject_explain: _,
        } => {
            let stdin_payload = read_hook_stdin_payload();
            let name = name.unwrap_or_else(default_session_name);
            match client.create_session(name, platform).await {
                Ok((id, n)) => {
                    let _ = persist_claude_env_var("IMPULSE_SESSION_ID", &id);
                    capture_hook_evidence(
                        &cli.impulse_dir,
                        "session_start",
                        Some(id.clone()),
                        Some(n.clone()),
                        Some("daemon".to_string()),
                        None,
                        None,
                        stdin_payload,
                        Some("daemon create_session".to_string()),
                        1,
                    )?;
                    println!("Created session: {} ({})", n, id)
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::SessionEnd {
            session_id,
            summary,
            verify: should_verify,
        } => {
            let stdin_payload = read_hook_stdin_payload();
            if should_verify {
                let steps = verify::default_steps(&std::env::current_dir()?);
                let report = verify::run_verification(steps)?;
                print_verification_report(&report);
                if !report.success() {
                    anyhow::bail!("Verification failed. Session end blocked.");
                }
            }
            match client
                .end_session(session_id.clone(), summary.clone())
                .await
            {
                Ok(_) => {
                    capture_hook_evidence(
                        &cli.impulse_dir,
                        "session_end",
                        Some(session_id.clone()),
                        None,
                        Some("daemon".to_string()),
                        Some(summary),
                        Some(should_verify),
                        stdin_payload,
                        Some(format!("Session {} ended", session_id)),
                        1,
                    )?;
                    println!("Session {} ended", session_id)
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::TrackWrite { file, session_id } => {
            if let Some(sid) = get_session_id(session_id) {
                match client.track_file(sid, file).await {
                    Ok(_) => println!("Tracked file"),
                    Err(e) => eprintln!("Error: {}", e),
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::TrackTool { tool, session_id } => {
            if let Some(sid) = get_session_id(session_id) {
                match client.track_tool(sid, tool).await {
                    Ok(_) => println!("Tracked tool"),
                    Err(e) => eprintln!("Error: {}", e),
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::ListSessions => match client.list_sessions().await {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("No active sessions");
                } else {
                    for s in sessions {
                        println!(
                            "{} - {} ({})",
                            s["id"].as_str().unwrap_or("?"),
                            s["name"].as_str().unwrap_or("?"),
                            s["status"].as_str().unwrap_or("?")
                        );
                    }
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionInfo { id } => match client.get_session(id).await {
            Ok(s) => print_json(&s)?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionConflicts { file, session_id } => {
            let sid = match get_session_id(session_id) {
                Some(s) => s,
                None => {
                    eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
                    return Ok(());
                }
            };
            match file {
                Some(f) => match client.check_conflict(sid, f).await {
                    Ok((has_conflict, sessions)) => {
                        if has_conflict {
                            println!("⚠️  CONFLICT DETECTED");
                            println!("File is being edited by: {}", sessions.join(", "));
                        } else {
                            println!("✓ No conflicts detected");
                        }
                    }
                    Err(e) => eprintln!("Error: {}", e),
                },
                None => match client.list_sessions().await {
                    Ok(sessions) => {
                        let mut all_files: std::collections::HashSet<String> =
                            std::collections::HashSet::new();
                        let mut file_to_session: std::collections::HashMap<String, Vec<String>> =
                            std::collections::HashMap::new();

                        for s in &sessions {
                            if let Some(files) = s.get("active_files").and_then(|v| v.as_array()) {
                                for f in files {
                                    if let Some(path) = f.as_str() {
                                        all_files.insert(path.to_string());
                                        file_to_session.entry(path.to_string()).or_default().push(
                                            s.get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?")
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                        }

                        if all_files.is_empty() {
                            println!("No active file modifications across sessions");
                        } else {
                            println!("Active file modifications across sessions:");
                            for (file, sessions) in &file_to_session {
                                if sessions.len() > 1 {
                                    println!(
                                        "  ⚠️  {} - being edited by: {}",
                                        file,
                                        sessions.join(", ")
                                    );
                                } else {
                                    println!("  {} - edited by: {}", file, sessions.join(", "));
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Error: {}", e),
                },
            }
        }
        Commands::Status => match client.status().await {
            Ok(s) => print_json(&s)?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::Chat {
            session_id,
            message,
            inject_mode,
            inject_explain,
        } => {
            let inject_mode = match parse_injection_mode(inject_mode.as_deref()) {
                Ok(mode) => mode.map(|m| m.as_str().to_string()),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return Ok(());
                }
            };
            match client
                .chat(session_id.clone(), message, inject_mode, inject_explain)
                .await
            {
                Ok(result) => {
                    if inject_explain {
                        print_json(&result)?;
                    } else if let Some(response) = result.get("response").and_then(|v| v.as_str()) {
                        println!("{}", response);
                    } else {
                        print_json(&result)?;
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Verify => {
            let steps = verify::default_steps(&std::env::current_dir()?);
            let report = verify::run_verification(steps)?;
            print_verification_report(&report);
            if !report.success() {
                anyhow::bail!("Verification failed");
            }
        }
        Commands::SearchHistory { .. }
        | Commands::SearchGenome { .. }
        | Commands::IndexMemory { .. }
        | Commands::RetrievalStatus { .. } => {
            println!("Use direct mode (without --daemon) for retrieval commands");
        }
        _ => println!("Use direct mode (without --daemon) for this command"),
    }
    Ok(())
}

// ============================================================================
// Direct Mode
// ============================================================================

async fn run_direct_mode(cli: Cli) -> Result<()> {
    let impulse_dir = cli.impulse_dir.clone();
    let state = Arc::new(state::State::new(impulse_dir.clone())?);

    match cli.command {
        Commands::Daemon { .. } => {
            daemon::Daemon::new(state.clone()).start().await?;
        }
        Commands::Run => {
            ui::run_ui(state.clone())?;
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode,
            inject_explain,
        } => {
            let stdin_payload = read_hook_stdin_payload();
            let name = name.unwrap_or_else(default_session_name);
            let platform = platform.and_then(|p| parse_platform(&p));
            let session = state.create_session(name.clone(), platform).await?;
            let _ = persist_claude_env_var("IMPULSE_SESSION_ID", &session.id);

            let query_parts = vec![name];
            let config = state.config_snapshot()?;
            let mode_override = inject_mode
                .as_deref()
                .and_then(injection::InjectionMode::parse)
                .or(Some(injection::InjectionMode::Apply)); // Default to outputting context to stdout at startup

            let injection_result = injection::run_injection(
                state.storage().base_path(),
                &config,
                injection::InjectionSurface::Orchestrate,
                mode_override,
                &query_parts,
            );

            if inject_explain {
                let _ = serde_json::to_writer_pretty(std::io::stdout(), &injection_result.explain);
                println!();
            }

            let mut output_lines = 0usize;
            if let Some(sentinel) = hook_session_start_banner() {
                println!("{}", sentinel);
                println!();
                output_lines += sentinel.lines().count() + 1;
            }

            if let Some(block) = injection_result.injected_block.clone() {
                let hook_mode = std::env::var("CLAUDE_ENV_FILE").is_ok();
                if hook_mode {
                    println!("{}", block);
                    output_lines += block.lines().count();
                } else {
                    println!("{}\n\n{}", session.id, block);
                    output_lines += block.lines().count() + 2;
                }
                capture_hook_evidence(
                    state.storage().base_path(),
                    "session_start",
                    Some(session.id.clone()),
                    Some(session.name.clone()),
                    session.platform.map(|p| p.as_str().to_string()),
                    None,
                    None,
                    stdin_payload,
                    Some(preview_block(&block, 400)),
                    output_lines,
                )?;
            } else {
                let hook_mode = std::env::var("CLAUDE_ENV_FILE").is_ok();
                if hook_mode {
                    println!(
                        "Impulse started session {}. No prior context was injected on this run.",
                        session.id
                    );
                    output_lines += 1;
                } else {
                    println!("{}", session.id);
                    output_lines += 1;
                }
                capture_hook_evidence(
                    state.storage().base_path(),
                    "session_start",
                    Some(session.id.clone()),
                    Some(session.name.clone()),
                    session.platform.map(|p| p.as_str().to_string()),
                    None,
                    None,
                    stdin_payload,
                    Some("no injection block".to_string()),
                    output_lines,
                )?;
            }
        }
        Commands::SessionEnd {
            session_id,
            summary,
            verify: should_verify,
        } => {
            let stdin_payload = read_hook_stdin_payload();
            if should_verify {
                let steps = verify::default_steps(&std::env::current_dir()?);
                let report = verify::run_verification(steps)?;
                print_verification_report(&report);
                if !report.success() {
                    anyhow::bail!("Verification failed. Session end blocked.");
                }
            }
            match state.end_session(&session_id, summary.clone()).await {
                Ok(Some(_)) => {
                    capture_hook_evidence(
                        state.storage().base_path(),
                        "session_end",
                        Some(session_id.clone()),
                        None,
                        None,
                        Some(summary),
                        Some(should_verify),
                        stdin_payload,
                        Some(format!("Session {} ended", session_id)),
                        1,
                    )?;
                    println!("Session {} ended", session_id)
                }
                Ok(None) => {
                    capture_hook_evidence(
                        state.storage().base_path(),
                        "session_end_missing",
                        Some(session_id.clone()),
                        None,
                        None,
                        Some(summary),
                        Some(should_verify),
                        stdin_payload,
                        Some(format!("Session not found: {}", session_id)),
                        1,
                    )?;
                    println!("Session not found: {}", session_id)
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::TrackWrite { file, session_id } => {
            if let Some(sid) = get_session_id(session_id) {
                match state.track_file(&sid, &file).await {
                    Ok(_) => println!("Tracked: {}", file),
                    Err(e) => eprintln!("Error: {}", e),
                }
                // Post-observation: evaluate Warn/Log guardrails on the tracked file.
                // Uses "any" target so all rules (bash, file, tool) are checked.
                if let Ok(config) = state.config_snapshot() {
                    if config.guardrails.enabled {
                        if let Ok(results) =
                            guardrail::evaluate_action(&file, "any", &config.guardrails)
                        {
                            for result in &results {
                                match result.action {
                                    guardrail::GuardAction::Warn => {
                                        eprintln!("{}", result.format_human());
                                    }
                                    guardrail::GuardAction::Log => {
                                        // Tag the session for audit trail
                                        let _ = state
                                            .add_tag(&sid, &format!("guard:{}", result.rule_id))
                                            .await;
                                    }
                                    guardrail::GuardAction::Block => {} // handled pre-execution
                                }
                            }
                        }
                    }
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::TrackTool { tool, session_id } => {
            if let Some(sid) = get_session_id(session_id) {
                match state.track_tool(&sid, &tool).await {
                    Ok(_) => println!("Tracked: {}", tool),
                    Err(e) => eprintln!("Error: {}", e),
                }
                // Post-observation: evaluate Warn/Log guardrails on the tracked tool.
                // Uses "any" target so all rules (bash, file, tool) are checked.
                if let Ok(config) = state.config_snapshot() {
                    if config.guardrails.enabled {
                        if let Ok(results) =
                            guardrail::evaluate_action(&tool, "any", &config.guardrails)
                        {
                            for result in &results {
                                match result.action {
                                    guardrail::GuardAction::Warn => {
                                        eprintln!("{}", result.format_human());
                                    }
                                    guardrail::GuardAction::Log => {
                                        // Tag the session for audit trail
                                        let _ = state
                                            .add_tag(&sid, &format!("guard:{}", result.rule_id))
                                            .await;
                                    }
                                    guardrail::GuardAction::Block => {} // handled pre-execution
                                }
                            }
                        }
                    }
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::ListSessions => {
            let sessions = state.list_sessions().await?;
            if sessions.is_empty() {
                println!("No active sessions");
            } else {
                for s in sessions {
                    println!("{} - {} ({:?})", s.id, s.name, s.status);
                }
            }
        }
        Commands::SessionInfo { id } => match state.get_session(&id).await {
            Ok(Some(s)) => {
                println!("Session: {}", s.name);
                println!("ID: {}", s.id);
                println!("Status: {:?}", s.status);
                println!("Platform: {:?}", s.platform);
                println!("Working Dir: {}", s.working_directory);
                println!("Created: {}", s.created_at);
                println!("Files: {:?}", s.active_files);
                println!("Tools: {:?}", s.recent_tools);
            }
            Ok(None) => println!("Session not found: {}", id),
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionConflicts { file, session_id } => {
            let sid = match get_session_id(session_id) {
                Some(s) => s,
                None => {
                    eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
                    return Ok(());
                }
            };
            match file {
                Some(f) => {
                    let conflicting = state.check_file_conflict(&sid, &f).await?;
                    if !conflicting.is_empty() {
                        println!("⚠️  CONFLICT DETECTED");
                        println!("File is being edited by: {}", conflicting.join(", "));
                    } else {
                        println!("✓ No conflicts detected");
                    }
                }
                None => {
                    let sessions = state.list_sessions().await?;
                    let mut all_files: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    let mut file_to_session: std::collections::HashMap<String, Vec<String>> =
                        std::collections::HashMap::new();

                    for s in &sessions {
                        for f in &s.active_files {
                            all_files.insert(f.clone());
                            file_to_session
                                .entry(f.clone())
                                .or_default()
                                .push(s.name.clone());
                        }
                    }

                    if all_files.is_empty() {
                        println!("No active file modifications across sessions");
                    } else {
                        println!("Active file modifications across sessions:");
                        for (file, sessions) in &file_to_session {
                            if sessions.len() > 1 {
                                println!(
                                    "  ⚠️  {} - being edited by: {}",
                                    file,
                                    sessions.join(", ")
                                );
                            } else {
                                println!("  {} - edited by: {}", file, sessions.join(", "));
                            }
                        }
                    }
                }
            }
        }
        Commands::Chat { .. } => {
            println!("Chat requires daemon mode. Use: impulse-rs --daemon chat --session-id <id> --message <msg>");
        }
        Commands::Status => {
            branding::print_banner();
            let sessions = state.list_sessions().await?;
            println!("Active sessions: {}", sessions.len());
            for s in &sessions {
                println!("  - {} ({}) [{:?}]", s.name, s.id, s.status);
            }
        }
        Commands::Genome => {
            let genome = state.storage().read_json::<memory::Genome>("GENOME.md")?;
            println!("{}", genome.to_markdown());
        }
        Commands::History => {
            let history = state.get_history_sync()?;
            if history.is_empty() {
                println!("No session history");
            } else {
                for entry in history.iter().rev().take(20) {
                    println!(
                        "[{}] {} - {}",
                        entry.ended_at.format("%Y-%m-%d %H:%M"),
                        entry.session_name,
                        entry.summary
                    );
                }
            }
        }
        Commands::ListProviders => {
            use agent::{AnthropicProvider, LlmProvider, MinimaxProvider, OpenAiProvider};

            println!("Available LLM Providers:\n");

            let anthropic = AnthropicProvider::new(String::new());
            let openai = OpenAiProvider::new(String::new());
            let minimax = MinimaxProvider::new(String::new());

            println!(
                "  {} (default: {})\n    Models: {}",
                anthropic.name(),
                anthropic.default_model(),
                anthropic.supported_models().join(", ")
            );
            println!(
                "\n  {} (default: {})\n    Models: {}",
                openai.name(),
                openai.default_model(),
                openai.supported_models().join(", ")
            );
            println!(
                "\n  {} (default: {})\n    Models: {}",
                minimax.name(),
                minimax.default_model(),
                minimax.supported_models().join(", ")
            );
        }
        Commands::AddDecision {
            description,
            rationale,
        } => {
            let mut genome: memory::Genome = state.storage().read_json("GENOME.md")?;
            genome.add_decision(description, rationale, Vec::new());
            state.storage().write_json("GENOME.md", &genome)?;
            println!("Added decision to GENOME");
        }
        Commands::Init => {
            state
                .storage()
                .write_json("LIVE_STATE.json", &state::LiveState::new())?;
            state
                .storage()
                .write_json("GENOME.md", &memory::Genome::new())?;
            state
                .storage()
                .write_json("config.json", &state::Config::default())?;
            let _ = orchestration::ensure_context_dirs(state.storage().base_path())?;
            let config = state.config_snapshot()?;
            let external_tools_dir =
                config.resolved_external_tools_dir_from(tool_resolution_root(&impulse_dir));
            std::fs::create_dir_all(&external_tools_dir)?;
            let registry = build_tool_registry(&impulse_dir, &config)?;
            let manifest_path =
                refresh_capabilities_manifest(state.storage().base_path(), &registry)?;
            branding::print_banner();
            println!("Initialized at {:?}", state.storage().base_path());
            println!("External tools dir: {}", external_tools_dir.display());
            println!("Capabilities manifest: {}", manifest_path.display());
        }
        Commands::Config { key, value, list } => {
            if list {
                let config = state.list_config()?;
                print_config(config);
            } else if let Some(key) = key {
                if let Some(value) = value {
                    match state.set_config(&key, &value) {
                        Ok(true) => println!("Set {} = {}", key, value),
                        Ok(false) => {
                            eprintln!("Error: Invalid value '{}' for key '{}'", value, key)
                        }
                        Err(e) => eprintln!("Error: {}", e),
                    }
                } else {
                    match state.get_config(&key)? {
                        Some(v) => println!("{} = {}", key, v),
                        None => println!("Unknown config key: {}", key),
                    }
                }
            } else {
                let config = state.list_config()?;
                print_config(config);
                println!("\nUse 'config <key>' to get a value");
                println!("Use 'config <key> --value <value>' to set a value");
                println!("Use 'config --list' to list all values");
            }
        }
        Commands::Extract {
            content,
            session_id,
            json,
        } => {
            let sid = session_id.unwrap_or_else(|| "unknown".to_string());
            let mut contrib = monty::kdb_extraction::KdbContribution::new(sid.clone());

            // Keyword-based extraction
            let lower = content.to_lowercase();
            if lower.contains("finding") || lower.contains("found") {
                contrib.add_finding(
                    content.clone(),
                    if lower.contains("critical") || lower.contains("urgent") {
                        "high".to_string()
                    } else {
                        "medium".to_string()
                    },
                );
            }
            if lower.contains("risk") || lower.contains("concern") {
                contrib.add_risk(content.clone(), "medium".to_string(), None);
            }

            if json {
                print_json(&contrib)?;
            } else {
                println!("Extracted from session: {}", sid);
                if !contrib.findings.is_empty() {
                    println!("\nFindings ({}):", contrib.findings.len());
                    for f in &contrib.findings {
                        println!("  - [{}] {}", f.severity, f.content);
                    }
                }
                if !contrib.risks.is_empty() {
                    println!("\nRisks ({}):", contrib.risks.len());
                    for r in &contrib.risks {
                        println!("  - [{}] {}", r.severity, r.description);
                    }
                }
                if contrib.findings.is_empty() && contrib.risks.is_empty() {
                    println!("No structured findings extracted.");
                }
            }
        }
        Commands::Swarm {
            agent_a,
            agent_b,
            threshold,
            json,
        } => {
            let patterns =
                monty::swarm_coordination::detect_patterns(&agent_a, &agent_b, threshold);

            if json {
                print_json(&patterns)?;
            } else {
                println!("SWARM Pattern Detection:");
                println!("  Agent A: {}", agent_a);
                println!("  Agent B: {}", agent_b);
                println!("  Threshold: {}", threshold);
                if patterns.is_empty() {
                    println!("\nNo patterns detected at threshold {}", threshold);
                } else {
                    println!("\nDetected {} pattern(s):", patterns.len());
                    for p in &patterns {
                        println!("  - {:?} (confidence: {:.2})", p.pattern_type, p.confidence);
                    }
                }
            }
        }
        Commands::Activity { limit } => {
            let sessions = state.list_sessions().await?;
            if sessions.is_empty() {
                println!("No sessions found");
            } else {
                println!("Recent Activity (showing {} most recent):\n=========================================", limit);

                let mut all_files: Vec<_> = sessions
                    .iter()
                    .flat_map(|s| {
                        s.active_files
                            .iter()
                            .map(|f| (s.name.clone(), f.clone(), s.last_activity))
                    })
                    .collect();
                let mut all_tools: Vec<_> = sessions
                    .iter()
                    .flat_map(|s| {
                        s.recent_tools
                            .iter()
                            .map(|t| (s.name.clone(), t.clone(), s.last_activity))
                    })
                    .collect();

                all_files.sort_by(|a, b| b.2.cmp(&a.2));
                all_tools.sort_by(|a, b| b.2.cmp(&a.2));

                println!("\n📝 Files Modified:");
                for (name, file, time) in all_files.iter().take(limit) {
                    println!("  [{}] {} - {}", time.format("%H:%M"), name, file);
                }
                println!("\n🔧 Tools Used:");
                for (name, tool, time) in all_tools.iter().take(limit) {
                    println!("  [{}] {} - {}", time.format("%H:%M"), name, tool);
                }
            }
        }
        Commands::Hooks { platform } => {
            let impulse_path = state.storage().base_path().display().to_string();

            if platform == "claude-code" || platform == "all" {
                println!("Setting up Claude Code hooks...");
                let hooks_dir = std::path::Path::new(".claude/hooks");
                if let Err(e) = std::fs::create_dir_all(hooks_dir) {
                    eprintln!("Error creating .claude/hooks: {}", e);
                } else {
                    let hook_config = build_claude_hook_config();
                    let hook_json =
                        serde_json::to_string_pretty(&hook_config).unwrap_or_else(|e| {
                            eprintln!("Error serializing hook config: {}", e);
                            String::from("{}")
                        });
                    let hook_path = std::path::Path::new(".claude/hooks/hooks.json");
                    if let Err(e) = stewardship::atomic_write_file(hook_path, hook_json.as_bytes())
                    {
                        eprintln!("Error writing hooks: {}", e);
                    } else {
                        println!("  \u{2713} Created .claude/hooks/hooks.json");
                    }
                }
            }

            if platform == "opencode" || platform == "all" {
                println!("\nSetting up OpenCode integration...");
                let opencode_dir = std::path::Path::new(".opencode");
                if let Err(e) = std::fs::create_dir_all(opencode_dir) {
                    eprintln!("Error creating .opencode: {}", e);
                } else {
                    let opencode_config = build_opencode_hook_config();
                    let opencode_json = serde_json::to_string_pretty(&opencode_config)
                        .unwrap_or_else(|e| {
                            eprintln!("Error serializing OpenCode config: {}", e);
                            String::from("{}")
                        });
                    let opencode_path = std::path::Path::new(".opencode/impulse.json");
                    if let Err(e) =
                        stewardship::atomic_write_file(opencode_path, opencode_json.as_bytes())
                    {
                        eprintln!("Error writing OpenCode config: {}", e);
                    } else {
                        println!("  \u{2713} Created .opencode/impulse.json");
                    }
                }
            }

            println!("\nHooks setup complete!\nImpulse path: {}\nEdit .claude/hooks/hooks.json to customize.", impulse_path);
        }
        Commands::ValidateHooks { platform } => {
            let written = write_hook_validation_kit(&platform)?;
            println!("Generated hook validation kit for {}:", platform);
            for path in written {
                println!("  - {}", path.display());
            }
            println!(
                "\nNext steps:\n  1. Copy .impulse/validation/{}/settings.local.json into your Claude local settings.\n  2. Run a real Claude session and inspect .impulse/validation/runtime/hook-events.jsonl for captured SessionStart/SessionEnd evidence.\n  3. Record the outcome in .impulse/validation/{}/evidence.md and compare it against .impulse/HISTORY.jsonl / .impulse/GENOME.md",
                platform, platform
            );
        }
        Commands::Orchestrate {
            task,
            inject_mode,
            inject_explain,
            compute_routing,
        } => {
            let mode_override = parse_injection_mode(inject_mode.as_deref())?;
            let mut query_parts = vec![task.clone()];
            if let Some(active_id) = get_session_id(None) {
                if let Some(active_session) = state.get_session(&active_id).await? {
                    query_parts.push(active_session.name);
                    if !active_session.active_files.is_empty() {
                        query_parts.push(active_session.active_files.join(" "));
                    }
                    if !active_session.recent_tools.is_empty() {
                        query_parts.push(active_session.recent_tools.join(" "));
                    }
                }
            }

            let config = state.config_snapshot()?;
            let injection_result = injection::run_injection(
                state.storage().base_path(),
                &config,
                injection::InjectionSurface::Orchestrate,
                mode_override,
                &query_parts,
            );

            let mut reasoning_input = task.clone();
            if injection_result.applied {
                if let Some(block) = &injection_result.injected_block {
                    reasoning_input = format!("{}\n\n{}", reasoning_input, block);
                }
            }

            // Use computed routing if requested
            if compute_routing {
                let monty_config = monty::MontyConfig::default();
                let context = format!("Task: {}\nContext: {}", task, reasoning_input);

                match monty::execute_computed_routing(&context, &monty_config) {
                    Ok(route) => {
                        println!("Computed routing result:");
                        println!("  Target: {}", route.target.as_str());
                        println!("  Confidence: {:.2}", route.confidence);
                        println!("  Reasoning: {}", route.reasoning);
                    }
                    Err(e) => {
                        eprintln!("Computed routing failed: {}", e);
                        let tool = orchestration::suggest_tool(&reasoning_input);
                        println!("Recommended tool: {}", tool.as_str());
                    }
                }
            } else {
                let tool = orchestration::suggest_tool(&reasoning_input);
                println!("Recommended tool: {}", tool.as_str());
            }

            println!("Task: {}", task);
            if inject_explain {
                print_injection_explain(&injection_result);
            }
        }
        Commands::Handoff {
            tool,
            task,
            session_id,
            notes,
            inject_mode,
            inject_explain,
        } => {
            let mode_override = parse_injection_mode(inject_mode.as_deref())?;
            let sid = get_session_id(session_id);
            let session = if let Some(id) = sid {
                state.get_session(&id).await?
            } else {
                None
            };

            let mut query_parts = vec![task.clone()];
            if let Some(n) = &notes {
                query_parts.push(n.clone());
            }
            if let Some(s) = &session {
                query_parts.push(s.name.clone());
                if !s.active_files.is_empty() {
                    query_parts.push(s.active_files.join(" "));
                }
                if !s.recent_tools.is_empty() {
                    query_parts.push(s.recent_tools.join(" "));
                }
            }
            let config = state.config_snapshot()?;
            let injection_result = injection::run_injection(
                state.storage().base_path(),
                &config,
                injection::InjectionSurface::Handoff,
                mode_override,
                &query_parts,
            );

            let handoff_path = orchestration::write_handoff(
                state.storage().base_path(),
                &tool,
                &task,
                notes.as_deref(),
                session.as_ref(),
            )?;
            if injection_result.applied {
                if let Some(block) = &injection_result.injected_block {
                    if let Err(err) = orchestration::append_injected_context(&handoff_path, block) {
                        eprintln!("Warning: failed to append injected context: {}", err);
                    }
                }
            }
            println!("Wrote handoff file: {}", handoff_path.display());
            if inject_explain {
                print_injection_explain(&injection_result);
            }
        }
        Commands::SyncContext {
            session_id,
            inject_mode,
            inject_explain,
        } => {
            let mode_override = parse_injection_mode(inject_mode.as_deref())?;
            let sid = get_session_id(session_id);
            let session = if let Some(id) = sid {
                state.get_session(&id).await?
            } else {
                None
            };

            let mut query_parts = vec!["sync context".to_string()];
            if let Some(s) = &session {
                query_parts.push(s.name.clone());
                if !s.active_files.is_empty() {
                    query_parts.push(s.active_files.join(" "));
                }
                if !s.recent_tools.is_empty() {
                    query_parts.push(s.recent_tools.join(" "));
                }
            }
            let config = state.config_snapshot()?;
            let injection_result = injection::run_injection(
                state.storage().base_path(),
                &config,
                injection::InjectionSurface::SyncContext,
                mode_override,
                &query_parts,
            );

            let context_path =
                orchestration::sync_context(state.storage().base_path(), session.as_ref())?;
            if injection_result.applied {
                if let Some(block) = &injection_result.injected_block {
                    if let Err(err) = orchestration::append_injected_context(&context_path, block) {
                        eprintln!("Warning: failed to append injected context: {}", err);
                    }
                }
            }
            println!("Synced context file: {}", context_path.display());
            if inject_explain {
                print_injection_explain(&injection_result);
            }
        }
        Commands::ComputeInjection {
            query,
            limit: _,
            json,
        } => {
            let monty_config = monty::MontyConfig::default();
            let context = format!("Query: {}", query);

            match monty::execute_injection_selection(&context, &monty_config) {
                Ok(decisions) => {
                    if json {
                        print_json(&decisions)?;
                    } else {
                        println!("Computed injection decisions:");
                        for (i, decision) in decisions.iter().enumerate() {
                            println!(
                                "  {}. [{}] {} - {}",
                                i + 1,
                                decision.priority,
                                decision.context_type,
                                decision.reasoning
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Injection selection failed: {}", e);
                    anyhow::bail!("Failed to compute injection: {}", e);
                }
            }
        }
        Commands::Verify => {
            let steps = verify::default_steps(&std::env::current_dir()?);
            let report = verify::run_verification(steps)?;
            print_verification_report(&report);
            if !report.success() {
                anyhow::bail!("Verification failed");
            }
        }
        Commands::IndexMemory { scope, rebuild } => {
            let scope = retrieval::types::IndexScope::parse(&scope).ok_or_else(|| {
                anyhow::anyhow!("Invalid scope '{}'. Use history|genome|all", scope)
            })?;
            let config = state.config_snapshot()?;
            let index_state =
                retrieval::index_from_storage(state.storage(), &config, scope, rebuild)?;
            println!(
                "Indexed memory: history={} genome={} vector_enabled={} vector_available={} duration={}ms",
                index_state.history_count,
                index_state.genome_count,
                index_state.vector_enabled,
                index_state.vector_available,
                index_state.last_index_duration_ms
            );
            for note in index_state.notes {
                println!("Note: {}", note);
            }
        }
        Commands::SearchHistory {
            query,
            mode,
            backend,
            limit,
            offset,
            page,
            total,
            explain,
            json,
        } => {
            let mode = if let Some(m) = mode.as_deref() {
                Some(
                    retrieval::types::RetrievalMode::parse(m)
                        .ok_or_else(|| anyhow::anyhow!("Invalid mode. Use keyword|semantic"))?,
                )
            } else {
                None
            };
            let backend = if let Some(b) = backend.as_deref() {
                Some(retrieval::types::SearchBackend::parse(b).ok_or_else(|| {
                    anyhow::anyhow!("Invalid backend. Use auto|sqlite-vec|rust-cosine|keyword")
                })?)
            } else {
                None
            };
            let page_limit = limit.unwrap_or(10);
            let page_offset = offset.unwrap_or(0)
                + page
                    .map(|p| (p.saturating_sub(1)) * page_limit)
                    .unwrap_or(0);
            let config = state.config_snapshot()?;
            let resp = retrieval::search_history(
                state.storage().base_path(),
                &config,
                &query,
                mode,
                backend,
                limit,
                Some(page_offset),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                if total {
                    if let Some(tc) = resp.total_count {
                        println!("Total matches: {}", tc);
                    }
                }
                if resp.used_fallback {
                    println!(
                        "Mode: {} (fallback) [{}] - {}",
                        resp.mode,
                        resp.backend_used,
                        resp.fallback_reason
                            .unwrap_or_else(|| "unknown reason".to_string())
                    );
                } else {
                    println!("Mode: {} [{}]", resp.mode, resp.backend_used);
                }
                if resp.results.is_empty() {
                    println!("No results");
                } else {
                    for (idx, item) in resp.results.iter().enumerate() {
                        println!(
                            "{}. [{}] {} ({})\n   {}",
                            idx + 1,
                            item.source,
                            item.title,
                            item.id,
                            item.snippet
                        );
                    }
                }
                if explain {
                    println!(
                        "\nExplain: timing={}ms candidates={} fallback_code={}",
                        resp.timing_ms,
                        resp.candidate_count,
                        resp.fallback_code
                            .map(|c| c.as_str().to_string())
                            .unwrap_or_else(|| "none".to_string())
                    );
                    for note in resp.engine_notes {
                        println!("  - {}", note);
                    }
                }
            }
        }
        Commands::SearchGenome {
            query,
            mode,
            backend,
            limit,
            offset,
            page,
            total,
            explain,
            json,
        } => {
            let mode = if let Some(m) = mode.as_deref() {
                Some(
                    retrieval::types::RetrievalMode::parse(m)
                        .ok_or_else(|| anyhow::anyhow!("Invalid mode. Use keyword|semantic"))?,
                )
            } else {
                None
            };
            let backend = if let Some(b) = backend.as_deref() {
                Some(retrieval::types::SearchBackend::parse(b).ok_or_else(|| {
                    anyhow::anyhow!("Invalid backend. Use auto|sqlite-vec|rust-cosine|keyword")
                })?)
            } else {
                None
            };
            let page_limit = limit.unwrap_or(10);
            let page_offset = offset.unwrap_or(0)
                + page
                    .map(|p| (p.saturating_sub(1)) * page_limit)
                    .unwrap_or(0);
            let config = state.config_snapshot()?;
            let resp = retrieval::search_genome(
                state.storage().base_path(),
                &config,
                &query,
                mode,
                backend,
                limit,
                Some(page_offset),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                if total {
                    if let Some(tc) = resp.total_count {
                        println!("Total matches: {}", tc);
                    }
                }
                if resp.used_fallback {
                    println!(
                        "Mode: {} (fallback) [{}] - {}",
                        resp.mode,
                        resp.backend_used,
                        resp.fallback_reason
                            .unwrap_or_else(|| "unknown reason".to_string())
                    );
                } else {
                    println!("Mode: {} [{}]", resp.mode, resp.backend_used);
                }
                if resp.results.is_empty() {
                    println!("No results");
                } else {
                    for (idx, item) in resp.results.iter().enumerate() {
                        println!(
                            "{}. [{}] {} ({})\n   {}",
                            idx + 1,
                            item.source,
                            item.title,
                            item.id,
                            item.snippet
                        );
                    }
                }
                if explain {
                    println!(
                        "\nExplain: timing={}ms candidates={} fallback_code={}",
                        resp.timing_ms,
                        resp.candidate_count,
                        resp.fallback_code
                            .map(|c| c.as_str().to_string())
                            .unwrap_or_else(|| "none".to_string())
                    );
                    for note in resp.engine_notes {
                        println!("  - {}", note);
                    }
                }
            }
        }
        Commands::RetrievalStatus { check, json } => {
            let config = state.config_snapshot()?;
            let status = retrieval::status(state.storage().base_path(), &config, check)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                println!("Retrieval DB: {}", status.db_path);
                println!(
                    "Exists: {} ({} bytes)",
                    status.db_exists, status.db_size_bytes
                );
                println!(
                    "Indexed at: {} (duration={}ms)",
                    status.index_state.indexed_at.to_rfc3339(),
                    status.index_state.last_index_duration_ms
                );
                println!(
                    "Counts: history={} genome={}",
                    status.index_state.history_count, status.index_state.genome_count
                );
                println!(
                    "Vector: enabled={} extension_available={}",
                    status.index_state.vector_enabled, status.vector_extension_available
                );
                println!("Python cmd available: {}", status.python_available);
                println!(
                    "Injection: mode={} scope={} emit_artifacts={} staged={} last_status={} last_artifact={}",
                    status.injection.config_mode,
                    status.injection.config_scope,
                    status.injection.emit_artifacts,
                    status.injection.staged_artifact_count,
                    status
                        .injection
                        .last_staged_status
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                    status
                        .injection
                        .last_staged_artifact
                        .clone()
                        .unwrap_or_else(|| "none".to_string())
                );
                if let Some(ok) = status.integrity_ok {
                    println!(
                        "Integrity check: {} ({})",
                        if ok { "ok" } else { "failed" },
                        status
                            .integrity_message
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                }
                if !status.index_state.notes.is_empty() {
                    for n in status.index_state.notes {
                        println!("Note: {}", n);
                    }
                }
            }
        }
        // Tools management
        Commands::Tools {
            subcommand,
            tool,
            dry_run,
        } => {
            use tools::{init, list, update};

            let tool_ids = if tool.is_empty() { None } else { Some(tool) };

            match subcommand.as_str() {
                "list" | "ls" => {
                    let _ = list::list_tools(cli.verbose)?;
                }
                "init" | "install" => {
                    let results = init::init_tools(tool_ids, dry_run)?;
                    for (id, status) in &results {
                        println!("{}: {}", id, status);
                    }
                }
                "update" | "upgrade" => {
                    let results = update::update_tools(tool_ids, dry_run)?;
                    for (id, success, msg) in &results {
                        let status = if *success { "✓" } else { "✗" };
                        println!("{} {}: {}", status, id, msg);
                    }
                }
                "check" => {
                    let tools = update::check_updates()?;
                    if tools.is_empty() {
                        println!("No tools installed");
                    } else {
                        println!("{:<20} {:<15} Status", "Tool", "Version");
                        println!("{:-<20} {:-<-15} ", "", "");
                        for (id, version, up_to_date) in tools {
                            let status = if up_to_date {
                                "up to date"
                            } else {
                                "update available"
                            };
                            println!("{:<20} {:<15} {}", id, version, status);
                        }
                    }
                }
                _ => {
                    eprintln!(
                        "Unknown tools subcommand: {}. Use: list, init, update, check",
                        subcommand
                    );
                }
            }
        }
        // Docs management
        Commands::Docs {
            subcommand,
            provider,
            verbose,
            force,
        } => {
            use docs::{cache, fetch, models as model_mgr};

            let cache = cache::create_cache(state.storage().base_path())?;

            match subcommand.as_str() {
                "fetch" | "update" => {
                    println!("Fetching latest model information...");

                    // Get OpenAI API key from environment if available
                    let openai_key = std::env::var("OPENAI_API_KEY").ok();

                    let models = fetch::fetch_all_models(openai_key.as_deref()).await?;
                    let providers = docs::known_providers();

                    cache.save_models(&models)?;
                    cache.save_providers(&providers)?;

                    let metadata = cache::CacheMetadata {
                        last_updated: std::time::SystemTime::now(),
                        model_count: models.len(),
                        provider_count: providers.len(),
                        source: "api".to_string(),
                    };
                    cache.save_metadata(&metadata)?;

                    println!(
                        "✓ Fetched {} models from {} providers",
                        models.len(),
                        providers.len()
                    );
                }
                "list" | "ls" => {
                    let models = if force {
                        println!("Fetching latest models...");
                        let openai_key = std::env::var("OPENAI_API_KEY").ok();
                        fetch::fetch_all_models(openai_key.as_deref()).await?
                    } else {
                        cache.load_models().unwrap_or_else(|_| {
                            println!("No cached models. Use --force to fetch latest.");
                            Vec::new()
                        })
                    };

                    let filter = model_mgr::ModelFilter {
                        provider: provider.clone(),
                        ..Default::default()
                    };

                    let filtered = filter.apply(&models);
                    println!(
                        "{}",
                        model_mgr::format_models(&filtered, verbose || cli.verbose)
                    );
                }
                "providers" => {
                    let providers = docs::known_providers();
                    println!("Available Providers:\n");
                    for p in &providers {
                        println!(
                            "{} ({})\n  API: {}\n  Docs: {}\n",
                            p.name, p.id, p.api_url, p.docs_url
                        );
                    }
                }
                "status" => {
                    let metadata = cache.load_metadata()?;
                    let age = cache.age_seconds().unwrap_or(0);
                    println!("Cache Status:");
                    println!("  Last updated: {} seconds ago", age);
                    println!("  Models cached: {}", metadata.model_count);
                    println!("  Providers cached: {}", metadata.provider_count);
                    println!("  Source: {}", metadata.source);

                    if cache.is_stale(std::time::Duration::from_secs(86400)) {
                        println!("  Status: STALE (older than 24 hours)");
                    } else {
                        println!("  Status: Fresh");
                    }
                }
                _ => {
                    eprintln!(
                        "Unknown docs subcommand: {}. Use: fetch, list, providers, status",
                        subcommand
                    );
                }
            }
        }
        // Model management
        Commands::Model {
            subcommand,
            provider,
            model,
        } => {
            use docs::cache;
            use docs::models as model_mgr;

            match subcommand.as_str() {
                "list" | "ls" => {
                    // Load cached models or use defaults
                    let cache = cache::create_cache(state.storage().base_path())?;
                    let models = cache.load_models().unwrap_or_else(|_| {
                        // Fall back to fetching default models
                        Vec::new()
                    });

                    let filter = model_mgr::ModelFilter {
                        provider: provider.clone(),
                        ..Default::default()
                    };

                    let filtered = filter.apply(&models);
                    println!("{}", model_mgr::format_models(&filtered, cli.verbose));
                }
                "set" => {
                    let provider =
                        provider.ok_or_else(|| anyhow::anyhow!("--provider required"))?;
                    let model = model.ok_or_else(|| anyhow::anyhow!("--model required"))?;

                    // Store in state/config
                    state.set_config(&format!("model.{}", provider), &model)?;
                    println!("Set default model for {} to {}", provider, model);
                }
                "get" => {
                    let provider =
                        provider.ok_or_else(|| anyhow::anyhow!("--provider required"))?;
                    let model = state.get_config(&format!("model.{}", provider))?;
                    if let Some(m) = model {
                        println!("{}: {}", provider, m);
                    } else {
                        // Show default
                        let defaults = model_mgr::ModelConfig::default();
                        if let Some(def) = defaults.get_default_model(&provider) {
                            println!("{} (default): {}", provider, def);
                        } else {
                            println!("No model configured for {}", provider);
                        }
                    }
                }
                _ => {
                    eprintln!(
                        "Unknown model subcommand: {}. Use: list, set, get",
                        subcommand
                    );
                }
            }
        }
        // Office document handling
        Commands::Office {
            subcommand,
            file,
            goal,
            json,
        } => {
            use office;

            match subcommand.as_str() {
                "info" | "status" => {
                    println!("Office Document Support:");
                    println!("  Formats: xlsx, xls, csv, docx");
                    println!("  Status: Available (enable office-support feature for full functionality)");

                    let formats = office::supported_formats();
                    println!("\nSupported Formats:");
                    println!(
                        "  {:<10} {:<20} {:<10} {:<10}",
                        "Extension", "Name", "Read", "Write"
                    );
                    println!("  {}", "-".repeat(50));
                    for (ext, name, read, write) in formats {
                        println!("  {:<10} {:<20} {:<10} {:<10}", ext, name, read, write);
                    }
                }
                "parse" | "extract" => {
                    let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
                    let path = std::path::Path::new(&file);

                    if !path.exists() {
                        return Err(anyhow::anyhow!("File not found: {}", file));
                    }

                    let result = office::parse_document(path)
                        .map_err(|e| anyhow::anyhow!("Failed to parse document: {}", e))?;

                    if json {
                        print_json(&result)?;
                    } else {
                        println!("Document: {}", result.metadata.source_path);
                        println!("Type: {}", result.document_type);
                        println!("Format: {}", result.metadata.format);
                        println!("Size: {} bytes", result.metadata.size_bytes);
                        println!("Chunks: {}", result.chunks.len());
                        println!("\n--- Content Preview ---");
                        let preview = result.content.chars().take(1000).collect::<String>();
                        println!("{}", preview);
                        if result.content.len() > 1000 {
                            println!("\n... (truncated, use --json for full content)");
                        }
                    }
                }
                "sheets" => {
                    let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
                    let path = std::path::Path::new(&file);

                    if !path.exists() {
                        return Err(anyhow::anyhow!("File not found: {}", file));
                    }

                    match office::excel::get_sheet_info(path) {
                        Ok(sheets) => {
                            println!("Sheets in {}:", file);
                            for sheet in sheets {
                                println!(
                                    "  - {} ({} rows x {} cols)",
                                    sheet.name, sheet.row_count, sheet.column_count
                                );
                            }
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Failed to get sheet info: {}", e));
                        }
                    }
                }
                "chunk" => {
                    let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
                    let path = std::path::Path::new(&file);

                    if !path.exists() {
                        return Err(anyhow::anyhow!("File not found: {}", file));
                    }

                    let result = office::parse_document(path)
                        .map_err(|e| anyhow::anyhow!("Failed to parse document: {}", e))?;

                    let chunks = office::extraction::chunk_content(&result.content, 1000, 100);

                    println!("Content split into {} chunks:", chunks.len());
                    for (i, chunk) in chunks.iter().enumerate() {
                        println!("\n--- Chunk {} ---", i);
                        let preview = chunk.content.chars().take(200).collect::<String>();
                        println!("{}", preview);
                        if chunk.content.len() > 200 {
                            println!("...");
                        }
                    }
                }
                "extract-smart" | "smart" => {
                    let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
                    let goal = goal.unwrap_or_else(|| "extract all key information".to_string());
                    let path = std::path::Path::new(&file);

                    if !path.exists() {
                        return Err(anyhow::anyhow!("File not found: {}", file));
                    }

                    let result = office::parse_document(path)
                        .map_err(|e| anyhow::anyhow!("Failed to parse document: {}", e))?;

                    let chunks = office::extraction::chunk_content(&result.content, 1000, 100);

                    if json {
                        let target = office::extraction::create_extraction_target(path, &goal)
                            .map_err(|e| {
                                anyhow::anyhow!("Failed to create extraction target: {}", e)
                            })?;
                        print_json(&serde_json::json!({
                            "goal": goal,
                            "document_type": result.document_type,
                            "chunks": chunks.len(),
                            "content_length": result.content.len(),
                            "target": target,
                        }))?;
                    } else {
                        println!("Smart extraction for goal: {}", goal);
                        println!("Type: {}", result.document_type);
                        println!("Chunks: {}", chunks.len());
                        println!("Content length: {} characters", result.content.len());
                    }
                }
                _ => {
                    eprintln!("Unknown office subcommand: {}. Use: info, parse, sheets, chunk, extract-smart", subcommand);
                }
            }
        }
        // Credential management
        Commands::Credentials {
            subcommand,
            provider,
            key,
            value,
            socket_path,
            tool,
        } => {
            use credentials::{create_provider, CredentialConfig, CredentialProviderType};

            // Determine provider type
            let provider_type = provider
                .as_ref()
                .and_then(|p| CredentialProviderType::parse(p))
                .unwrap_or(CredentialProviderType::Env);

            let config = CredentialConfig {
                provider: provider_type,
                cli_tool: tool.clone(),
                socket_path: socket_path.clone(),
                provider_url: None,
            };

            let cred_provider = create_provider(&config);

            match subcommand.as_str() {
                "list" | "ls" => {
                    let status = cred_provider.status();
                    println!(
                        "Provider: {} (available: {})",
                        status.provider, status.available
                    );

                    match cred_provider.list() {
                        Ok(secrets) => {
                            if secrets.is_empty() {
                                println!("No secrets stored.");
                            } else {
                                println!("\nStored secrets:");
                                for secret in secrets {
                                    println!("  - {}", secret.key);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error listing secrets: {}", e);
                        }
                    }
                }
                "get" => {
                    let key = key
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--key required"))?;
                    match cred_provider.get(key) {
                        Ok(val) => {
                            println!("{}", val);
                        }
                        Err(e) => {
                            eprintln!("Error getting secret: {}", e);
                        }
                    }
                }
                "set" => {
                    let key = key
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--key required"))?;
                    let value = value
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--value required"))?;
                    match cred_provider.set(key, value) {
                        Ok(_) => {
                            println!("Secret '{}' stored successfully.", key);
                        }
                        Err(e) => {
                            eprintln!("Error setting secret: {}", e);
                        }
                    }
                }
                "delete" | "rm" => {
                    let key = key
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--key required"))?;
                    match cred_provider.delete(key) {
                        Ok(_) => {
                            println!("Secret '{}' deleted.", key);
                        }
                        Err(e) => {
                            eprintln!("Error deleting secret: {}", e);
                        }
                    }
                }
                "status" => {
                    let status = cred_provider.status();
                    println!("Provider: {}", status.provider);
                    println!("Available: {}", status.available);
                    println!("Secrets count: {}", status.secrets_count);
                    if let Some(err) = status.last_error {
                        println!("Last error: {}", err);
                    }
                }
                _ => {
                    eprintln!(
                        "Unknown credentials subcommand: {}. Use: list, get, set, delete, status",
                        subcommand
                    );
                }
            }
        }
        Commands::Calc { expression } => {
            use crate::tools::python;

            // Check if Python is available
            if !python::is_python_available() {
                eprintln!("Error: Python is not available. Please install Python 3.");
                return Err(anyhow::anyhow!("Python not available"));
            }

            // Calculate the expression
            match python::calculate(&expression) {
                Ok(result) => {
                    println!("{}", result);
                }
                Err(e) => {
                    eprintln!("Calculation error: {}", e);
                    return Err(anyhow::anyhow!("Calculation failed"));
                }
            }
        }
        Commands::Exec { code } => {
            use crate::tools::python;

            // Check if Python is available
            if !python::is_python_available() {
                eprintln!("Error: Python is not available. Please install Python 3.");
                return Err(anyhow::anyhow!("Python not available"));
            }

            // Execute the code
            match python::execute_python(&code) {
                Ok(result) => {
                    if let Some(error) = result.error {
                        eprintln!("Error:\n{}", error);
                    }
                    if !result.output.is_empty() {
                        print!("{}", result.output);
                    }
                }
                Err(e) => {
                    eprintln!("Execution error: {}", e);
                    return Err(anyhow::anyhow!("Execution failed"));
                }
            }
        }
        Commands::System {} => {
            use crate::tools::system::{get_impulse_env_vars, SystemInfo};

            let info = SystemInfo::collect();

            println!("=== System Information ===");
            println!("OS: {}", info.os);
            println!("Architecture: {}", info.arch);
            println!("Home Directory: {:?}", info.home_dir);
            println!("Current Directory: {}", info.current_dir);
            println!("Python Available: {}", info.python_available);
            if let Some(version) = info.python_version {
                println!("Python Version: {}", version.trim());
            }

            let impulse_vars = get_impulse_env_vars();
            if !impulse_vars.is_empty() {
                println!("\n=== Impulse Environment Variables ===");
                for var in impulse_vars {
                    println!("{}: {}", var.key, var.value);
                }
            }
        }
        Commands::Steward {
            subcommand,
            transcript,
            session_id,
            id,
            json,
        } => {
            use crate::stewardship;

            let base = &impulse_dir;
            let config = state.config_snapshot()?;
            let stew_config = stewardship::StewardshipConfig::from_config(&config);

            match subcommand.as_str() {
                "status" => {
                    let proposals = stewardship::approval::list_pending(base)?;
                    let cross = stewardship::cross_project::load_cross_project(base)?;

                    if json {
                        let status = serde_json::json!({
                            "mode": stew_config.mode.as_str(),
                            "thresholds": {
                                "monitor": stew_config.monitor_threshold,
                                "surgical": stew_config.surgical_threshold,
                                "thoughtful": stew_config.thoughtful_threshold,
                                "emergency": stew_config.emergency_threshold,
                            },
                            "context_window_tokens": stew_config.context_window_tokens,
                            "pending_proposals": proposals.len(),
                            "cross_project_patterns": cross.patterns.len(),
                            "cross_project_learnings": cross.learnings.len(),
                        });
                        println!("{}", serde_json::to_string_pretty(&status)?);
                    } else {
                        branding::print_header("Stewardship Status");
                        println!("  Mode: {:?}", stew_config.mode);
                        println!(
                            "  Thresholds: {:.0}% / {:.0}% / {:.0}% / {:.0}%",
                            stew_config.monitor_threshold * 100.0,
                            stew_config.surgical_threshold * 100.0,
                            stew_config.thoughtful_threshold * 100.0,
                            stew_config.emergency_threshold * 100.0,
                        );
                        println!(
                            "  Context window: {} tokens",
                            stew_config.context_window_tokens
                        );
                        println!("  Pending proposals: {}", proposals.len());
                        for p in &proposals {
                            println!(
                                "    - {} [{}] ~{} tokens freed",
                                p.id,
                                p.strategy.as_str(),
                                p.estimated_tokens_freed
                            );
                        }
                        println!("  Cross-project patterns: {}", cross.patterns.len());
                        println!("  Cross-project learnings: {}", cross.learnings.len());
                    }
                }
                "analyze" => {
                    let transcript_path = transcript
                        .ok_or_else(|| anyhow::anyhow!("--transcript required for analyze"))?;
                    let sid = session_id.as_deref().unwrap_or("unknown");
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let phash = stewardship::cross_project::project_hash(&cwd.to_string_lossy());
                    let analysis = stewardship::analyzer::analyze_session(
                        &transcript_path,
                        sid,
                        &phash,
                        &config,
                    )?;

                    if json {
                        let out = serde_json::json!({
                            "session_id": analysis.session_id,
                            "message_count": analysis.message_count,
                            "estimated_tokens": analysis.estimated_tokens,
                            "estimated_context_pct": analysis.estimated_context_pct,
                            "decisions": analysis.decisions.len(),
                            "files_touched": analysis.files_touched,
                            "duplicate_regions": analysis.duplicate_regions.len(),
                            "rot_candidates": analysis.rot_candidates.len(),
                            "key_insights": analysis.key_insights,
                        });
                        println!("{}", serde_json::to_string_pretty(&out)?);
                    } else {
                        branding::print_header("Session Analysis");
                        println!("  Session: {}", analysis.session_id);
                        println!("  Messages: {}", analysis.message_count);
                        println!(
                            "  Tokens: ~{} ({:.1}% of window)",
                            analysis.estimated_tokens,
                            analysis.estimated_context_pct * 100.0
                        );
                        println!("  Decisions: {}", analysis.decisions.len());
                        println!("  Files touched: {}", analysis.files_touched.len());
                        println!("  Duplicate regions: {}", analysis.duplicate_regions.len());
                        println!("  Rot candidates: {}", analysis.rot_candidates.len());
                        if !analysis.key_insights.is_empty() {
                            println!("  Insights:");
                            for insight in &analysis.key_insights {
                                println!("    - {}", insight);
                            }
                        }
                    }
                }
                "list" => {
                    let proposals = stewardship::approval::list_pending(base)?;
                    if json {
                        let out: Vec<_> = proposals
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "id": p.id,
                                    "strategy": p.strategy.as_str(),
                                    "threshold": p.threshold.as_str(),
                                    "estimated_tokens_freed": p.estimated_tokens_freed,
                                    "regions": p.regions.len(),
                                    "status": p.status.as_str(),
                                })
                            })
                            .collect();
                        println!("{}", serde_json::to_string_pretty(&out)?);
                    } else {
                        println!("Pending Proposals ({}):", proposals.len());
                        for p in &proposals {
                            println!(
                                "  {} [{:?}] {} — ~{} tokens freed",
                                p.id,
                                p.threshold,
                                p.strategy.as_str(),
                                p.estimated_tokens_freed
                            );
                            for region in &p.regions {
                                println!(
                                    "    Region: {} ({} messages, ~{} tokens)",
                                    region.description,
                                    region.message_indices.len(),
                                    region.estimated_tokens
                                );
                            }
                        }
                    }
                }
                "approve" => {
                    let pid = id.ok_or_else(|| anyhow::anyhow!("--id required for approve"))?;
                    match stewardship::approval::approve_proposal(base, &pid)? {
                        true => println!("Proposal {} approved and moved to applied.", pid),
                        false => println!("Proposal {} not found in pending.", pid),
                    }
                }
                "reject" => {
                    let pid = id.ok_or_else(|| anyhow::anyhow!("--id required for reject"))?;
                    match stewardship::approval::reject_proposal(base, &pid)? {
                        true => println!("Proposal {} rejected and removed.", pid),
                        false => println!("Proposal {} not found in pending.", pid),
                    }
                }
                "memory" => {
                    let cross = stewardship::cross_project::load_cross_project(base)?;
                    if json {
                        let out = serde_json::json!({
                            "version": cross.version,
                            "updated": cross.updated.to_rfc3339(),
                            "patterns": cross.patterns.iter().map(|p| serde_json::json!({
                                "id": p.id,
                                "type": p.pattern_type,
                                "description": p.description,
                                "occurrences": p.occurrences,
                                "projects": p.projects,
                                "insight": p.insight,
                            })).collect::<Vec<_>>(),
                            "learnings": cross.learnings,
                            "stats": {
                                "total_patterns": cross.stats.total_patterns,
                                "total_sessions": cross.stats.total_sessions_analyzed,
                                "total_learnings": cross.stats.total_learnings,
                            },
                        });
                        println!("{}", serde_json::to_string_pretty(&out)?);
                    } else {
                        branding::print_header("Cross-Project Memory");
                        println!("  Version: {}", cross.version);
                        println!("  Updated: {}", cross.updated.format("%Y-%m-%d %H:%M"));
                        println!("  Patterns ({}):", cross.patterns.len());
                        for p in &cross.patterns {
                            println!(
                                "    [{}] {} (seen {} times across {} projects)",
                                p.pattern_type,
                                p.description,
                                p.occurrences,
                                p.projects.len()
                            );
                            println!("      Insight: {}", p.insight);
                        }
                        println!("  Learnings ({}):", cross.learnings.len());
                        for l in &cross.learnings {
                            println!("    - {}", l);
                        }
                    }
                }
                "compact" => {
                    let sid = session_id
                        .ok_or_else(|| anyhow::anyhow!("--session-id required for compact"))?;
                    let cross = stewardship::cross_project::load_cross_project(base)?;

                    if let Some(transcript_path) = transcript {
                        let sid_ref = sid.as_str();
                        let cwd = std::env::current_dir().unwrap_or_default();
                        let phash =
                            stewardship::cross_project::project_hash(&cwd.to_string_lossy());
                        let analysis = stewardship::analyzer::analyze_session(
                            &transcript_path,
                            sid_ref,
                            &phash,
                            &config,
                        )?;
                        let context =
                            stewardship::cleanup::build_refined_context(&analysis, &cross);
                        print!("{}", context);
                    } else {
                        let mut context = format!("# Session {} — Stewardship Context\n\n", sid);
                        if !cross.learnings.is_empty() {
                            context.push_str("## Cross-Project Learnings\n");
                            for l in &cross.learnings {
                                context.push_str(&format!("- {}\n", l));
                            }
                        }
                        if !cross.patterns.is_empty() {
                            context.push_str("\n## Relevant Patterns\n");
                            for p in &cross.patterns {
                                context.push_str(&format!("- {} ({})\n", p.description, p.insight));
                            }
                        }
                        print!("{}", context);
                    }
                }
                _ => {
                    eprintln!("Unknown steward subcommand: '{}'. Available: status, analyze, list, approve, reject, memory, compact", subcommand);
                }
            }
        }
        Commands::Analyze { session_id, scope } => {
            println!("=== Impulse Analysis ===");

            match scope.as_str() {
                "session" | "sessions" => {
                    if let Some(sid) = session_id {
                        // Analyze specific session
                        println!("\nAnalyzing session: {}", sid);
                        // For now, show basic session info
                        match state.get_session(&sid).await {
                            Ok(Some(s)) => {
                                println!("Session: {} ({})", s.name, s.id);
                                println!("Files: {}", s.active_files.len());
                                println!("Tools: {}", s.recent_tools.len());
                            }
                            Ok(None) => {
                                println!("Session not found: {}", sid);
                            }
                            Err(e) => {
                                eprintln!("Error fetching session: {}", e);
                            }
                        }
                    } else {
                        println!("\nUsage: --session-id required for session analysis");
                    }
                }
                "token" | "tokens" => {
                    println!("\nToken analysis:");
                    println!("Use `impulse-rs activity` for token tracking details");
                }
                "all" | "*" => {
                    println!("\nAvailable analysis scopes:");
                    println!("  session  - Analyze specific session (requires --session-id)");
                    println!("  tokens   - Token usage analysis");
                    println!("  all      - This help message");
                }
                _ => {
                    eprintln!("Unknown scope: {}. Use: session, tokens, all", scope);
                }
            }
        }
        Commands::Health {} => {
            use crate::tools::health::{check_impulse_health, check_python_health, HealthStatus};

            println!("=== Impulse Health Check ===\n");

            // Check Python
            let python_check = check_python_health();
            let status_icon = match python_check.status {
                HealthStatus::Healthy => "✓",
                HealthStatus::Warning => "⚠",
                HealthStatus::Error => "✗",
            };
            print!("Python: {} ", status_icon);
            match python_check.status {
                HealthStatus::Healthy => println!("OK"),
                HealthStatus::Warning => println!("Warning: {:?}", python_check.message),
                HealthStatus::Error => println!("Error: {:?}", python_check.message),
            }

            // Check impulse directory
            let report = check_impulse_health(&impulse_dir);

            let overall_icon = match report.overall_status {
                HealthStatus::Healthy => "✓",
                HealthStatus::Warning => "⚠",
                HealthStatus::Error => "✗",
            };

            println!("\nOverall Status: {} ", overall_icon);
            match report.overall_status {
                HealthStatus::Healthy => println!("All systems operational"),
                HealthStatus::Warning => println!("Some issues detected"),
                HealthStatus::Error => println!("Critical issues found"),
            }

            println!("\nDetailed Checks:");
            for check in &report.checks {
                let icon = match check.status {
                    HealthStatus::Healthy => "✓",
                    HealthStatus::Warning => "⚠",
                    HealthStatus::Error => "✗",
                };
                print!("  {} {}", icon, check.name);
                if let Some(msg) = &check.message {
                    println!(" - {}", msg);
                } else {
                    println!();
                }
            }
        }
        Commands::Summary {} => {
            println!("=== Impulse Summary ===\n");

            // Show quick overview
            println!("Impulse Directory: {}", impulse_dir.display());
            println!("\nQuick Commands:");
            println!("  impulse-rs status     - Detailed status");
            println!("  impulse-rs health    - Health check");
            println!("  impulse-rs activity  - Recent activity");
            println!("  impulse-rs history   - Session history");
            println!("  impulse-rs list      - List sessions");
            println!("  impulse-rs system    - System info");

            // Show available CLI tools
            println!("\nCLI Tools Tracked:");
            for tool in crate::tools::known_tools() {
                println!("  - {} ({})", tool.name, tool.id);
            }

            // Show build hygiene commands
            println!("\nBuild Hygiene:");
            println!("  impulse-rs sweep         - Clean stale build artifacts");
            println!("  impulse-rs wipe          - Aggressive target/ cleanup");
            println!("  impulse-rs clean-all     - Workspace-wide cargo clean");
            println!("  impulse-rs sccache-setup - Configure compilation cache");
            println!("  impulse-rs build-health  - Disk usage report");
        }

        // ====================================================================
        // Build Hygiene Commands
        // ====================================================================
        Commands::Sweep {
            dry_run,
            path,
            days,
            verbose,
        } => {
            let config = load_build_hygiene_config(&state);
            let dry_run = dry_run.unwrap_or(config.dry_run_default);
            let days = days.unwrap_or(config.age_threshold_days);

            let paths = if let Some(p) = path {
                vec![p]
            } else {
                config.expanded_scan_paths()
            };

            if paths.is_empty() {
                println!("No scan paths configured. Use --path or set build_hygiene_scan_paths in config.");
                return Ok(());
            }

            println!("=== Cargo Sweep ===\n");
            println!(
                "Scanning: {:?}",
                paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
            );
            println!("Artifacts older than: {} days", days);
            println!("Mode: {}\n", if dry_run { "DRY RUN" } else { "LIVE" });

            let opts = build_hygiene::sweep::SweepOptions {
                days,
                dry_run,
                paths,
                recursive: true,
                verbose,
            };

            match build_hygiene::sweep::run_sweep(&opts) {
                Ok(result) => {
                    println!("{}", result.summary);
                    if !result.errors.is_empty() {
                        println!("\nWarnings:");
                        for err in &result.errors {
                            println!("  - {}", err);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Sweep failed: {}", e);
                    eprintln!("\nHint: Check filesystem permissions and scan path configuration.");
                }
            }
        }

        Commands::Wipe { dry_run, path } => {
            let config = load_build_hygiene_config(&state);
            let dry_run = dry_run.unwrap_or(config.dry_run_default);

            let paths = if let Some(p) = path {
                vec![p]
            } else {
                config.expanded_scan_paths()
            };

            if paths.is_empty() {
                println!("No scan paths configured. Use --path or set build_hygiene_scan_paths in config.");
                return Ok(());
            }

            println!("=== Cargo Wipe ===\n");
            println!(
                "Mode: {}\n",
                if dry_run {
                    "DRY RUN (safe)"
                } else {
                    "LIVE — will delete target/ dirs!"
                }
            );

            let opts = build_hygiene::wipe::WipeOptions {
                dry_run,
                paths,
                include_node_modules: false,
            };

            match build_hygiene::wipe::run_wipe(&opts) {
                Ok(result) => {
                    println!("{}", result.summary);
                    if !result.errors.is_empty() {
                        println!("\nWarnings:");
                        for err in &result.errors {
                            println!("  - {}", err);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Wipe failed: {}", e);
                    eprintln!("\nHint: Check filesystem permissions and scan path configuration.");
                }
            }
        }

        Commands::CleanAll { dry_run } => {
            let config = load_build_hygiene_config(&state);
            let dry_run = dry_run.unwrap_or(config.dry_run_default);
            let paths = config.expanded_scan_paths();

            if paths.is_empty() {
                println!("No scan paths configured.");
                return Ok(());
            }

            println!("=== Cargo Clean All ===\n");
            println!(
                "Mode: {}\n",
                if dry_run {
                    "DRY RUN"
                } else {
                    "LIVE — will cargo clean all projects!"
                }
            );

            match build_hygiene::clean_all::clean_all_projects(&paths, dry_run) {
                Ok(result) => {
                    println!("{}", result.summary);
                    if !result.errors.is_empty() {
                        println!("\nWarnings:");
                        for err in &result.errors {
                            println!("  - {}", err);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Clean-all failed: {}", e);
                }
            }
        }

        Commands::SccacheSetup { check, json } => {
            if check || json {
                let status = build_hygiene::sccache::sccache_status();
                if json {
                    print_json(&status)?;
                } else {
                    println!("=== sccache Status ===\n");
                    println!("Installed: {}", if status.installed { "yes" } else { "no" });
                    if let Some(ref v) = status.version {
                        println!("Version: {}", v);
                    }
                    println!(
                        "Configured: {}",
                        if status.configured { "yes" } else { "no" }
                    );
                    println!("Config path: {}", status.config_path);
                    if let Some(ref stats) = status.stats {
                        println!("\nCache Stats:");
                        if let Some(hits) = stats.cache_hits {
                            println!("  Hits: {}", hits);
                        }
                        if let Some(misses) = stats.cache_misses {
                            println!("  Misses: {}", misses);
                        }
                        if let Some(ref size) = stats.cache_size {
                            println!("  Size: {}", size);
                        }
                    }
                }
            } else {
                match build_hygiene::sccache::sccache_setup(false) {
                    Ok(result) => {
                        println!("=== sccache Setup ===\n");
                        println!("{}", result.action_taken);
                        println!("Config: {}", result.config_path);
                    }
                    Err(e) => {
                        eprintln!("sccache setup failed: {}", e);
                    }
                }
            }
        }

        Commands::BuildHealth { json } => {
            let config = load_build_hygiene_config(&state);
            let paths = config.expanded_scan_paths();

            let projects = build_hygiene::discovery::discover_rust_projects(&paths);
            let report =
                build_hygiene::measurement::generate_report(&projects, config.size_threshold_gb);

            if json {
                print_json(&report)?;
            } else {
                println!("=== Rust Build Health ===\n");
                println!(
                    "Total: {} across {} projects\n",
                    report.total_human, report.project_count
                );

                if !report.projects.is_empty() {
                    println!("Projects (largest first):");
                    for (i, p) in report.projects.iter().enumerate().take(20) {
                        println!("  {}. {} — {}", i + 1, p.path, p.target_size_human);
                    }
                    if report.projects.len() > 20 {
                        println!("  ... and {} more", report.projects.len() - 20);
                    }
                }

                println!("\nRecommendations:");
                for rec in &report.recommendations {
                    println!("  - {}", rec);
                }

                // Also show sccache status
                let sccache_st = build_hygiene::sccache::sccache_status();
                println!(
                    "\nsccache: {}",
                    if sccache_st.installed && sccache_st.configured {
                        "configured"
                    } else if sccache_st.installed {
                        "installed but not configured — run `impulse-rs sccache-setup`"
                    } else {
                        "not installed — run `cargo install sccache`"
                    }
                );
            }
        }

        Commands::ToolingList { category, json } => {
            let config = state.config_snapshot()?;
            let registry = build_tool_registry(&impulse_dir, &config)?;
            let mut tools = registry.manifest_tools();

            if let Some(ref cat) = category {
                let Some(category_kind) = parse_tool_category(cat) else {
                    eprintln!(
                        "Unknown category: {} (use: utility, document, analysis, system)",
                        cat
                    );
                    return Ok(());
                };
                let category_name = category_kind.to_string();
                tools.retain(|tool| tool.category == category_name);
            }

            if json {
                print_json(&tools)?;
            } else {
                println!("=== Dynamic Tools ({}) ===\n", tools.len());
                for tool in &tools {
                    println!(
                        "  {} — {} [{} | {}]",
                        tool.id, tool.description, tool.category, tool.source
                    );
                }
                if tools.is_empty() {
                    println!("  (no tools registered)");
                }
                println!("\nUse `tooling-describe <id>` for details.");
            }
        }

        Commands::ToolingDescribe { tool_id, json } => {
            let config = state.config_snapshot()?;
            let registry = build_tool_registry(&impulse_dir, &config)?;
            match registry.get(&tool_id) {
                Some(tool) => {
                    let desc = tool.descriptor();
                    let capabilities: Vec<_> = tool
                        .required_capabilities()
                        .iter()
                        .map(|cap| cap.as_str())
                        .collect();
                    let source = registry
                        .source(&tool_id)
                        .map(|value| value.as_str())
                        .unwrap_or("builtin");
                    if json {
                        print_json(&serde_json::json!({
                            "descriptor": desc,
                            "capabilities": capabilities,
                            "source": source,
                        }))?;
                    } else {
                        println!("=== {} (v{}) ===\n", desc.name, desc.version);
                        println!("ID:       {}", desc.id);
                        println!("Category: {}", desc.category);
                        println!("Source:   {}", source);
                        println!("Description: {}\n", desc.description);

                        if !desc.params.is_empty() {
                            println!("Parameters:");
                            for p in &desc.params {
                                let req = if p.required { "required" } else { "optional" };
                                println!(
                                    "  --{} ({:?}, {}) — {}",
                                    p.name, p.param_type, req, p.description
                                );
                            }
                        } else {
                            println!("Parameters: none");
                        }

                        if !capabilities.is_empty() {
                            println!("\nRequired capabilities: {}", capabilities.join(", "));
                        }
                    }
                }
                None => {
                    eprintln!("Tool not found: {}", tool_id);
                    eprintln!("Use `tooling-list` to see available tools.");
                }
            }
        }

        Commands::ToolingRun {
            tool_id,
            params,
            json,
        } => {
            let config = state.config_snapshot()?;
            let registry = build_tool_registry(&impulse_dir, &config)?;
            let params_value: serde_json::Value = if let Some(ref p) = params {
                match serde_json::from_str(p) {
                    Ok(value) => value,
                    Err(e) => {
                        eprintln!("Invalid JSON params: {}", e);
                        return Ok(());
                    }
                }
            } else {
                serde_json::json!({})
            };

            let ctx = build_tool_context(
                &impulse_dir,
                &config,
                tooling::ExecutionOrigin::Cli,
                true,
                get_session_id(None),
            );

            match registry.execute(&tool_id, params_value, &ctx).await {
                Ok(result) => {
                    if json {
                        print_json(&serde_json::json!({
                            "tool": tool_id,
                            "output": result.output,
                            "artifacts": result.artifacts,
                            "metadata": result.metadata,
                        }))?;
                    } else {
                        print_json(&result.output)?;
                        if !result.artifacts.is_empty() {
                            println!("\n--- Artifacts ---");
                            for artifact in &result.artifacts {
                                println!("  {}", artifact.display());
                            }
                        }
                        if !result.metadata.is_empty() {
                            println!("\n--- Metadata ---");
                            for (k, v) in &result.metadata {
                                println!("  {}: {}", k, v);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Tool execution failed: {}", e);
                }
            }
        }

        Commands::ToolingSchema { format } => {
            let config = state.config_snapshot()?;
            let registry = build_tool_registry(&impulse_dir, &config)?;

            match format.as_str() {
                "json" => {
                    print_json(&registry.schema_json())?;
                }
                "markdown" => {
                    println!("# Impulse Dynamic Tools\n");
                    println!("{}", registry.schema_markdown());
                }
                _ => {
                    eprintln!("Unknown format: {} (use: json, markdown)", format);
                }
            }
        }

        Commands::ToolingValidate { json } => {
            let config = state.config_snapshot()?;
            let external_tools_dir =
                config.resolved_external_tools_dir_from(tool_resolution_root(&impulse_dir));
            let report = tooling::validate_manifests_in_dir(&external_tools_dir);

            if json {
                print_json(&report)?;
            } else {
                println!("=== External Tool Validation ===\n");
                println!("Directory: {}", external_tools_dir.display());
                println!("Valid tools: {}", report.valid_tools);
                println!("Invalid tools: {}", report.invalid_tools);
                if report.issues.is_empty() {
                    println!("\nNo validation issues found.");
                } else {
                    println!("\nIssues:");
                    for issue in &report.issues {
                        println!("  {} — {}", issue.file, issue.error);
                    }
                }
            }

            if report.invalid_tools > 0 {
                anyhow::bail!(
                    "found {} invalid external tool manifest(s)",
                    report.invalid_tools
                );
            }
        }

        Commands::ToolingReload { json } => {
            let config = state.config_snapshot()?;
            let external_tools_dir =
                config.resolved_external_tools_dir_from(tool_resolution_root(&impulse_dir));
            let report = tooling::validate_manifests_in_dir(&external_tools_dir);
            if report.invalid_tools > 0 {
                if json {
                    print_json(&report)?;
                }
                anyhow::bail!(
                    "cannot reload tooling: found {} invalid external tool manifest(s)",
                    report.invalid_tools
                );
            }

            let registry = build_tool_registry(&impulse_dir, &config)?;
            let manifest_path =
                refresh_capabilities_manifest(state.storage().base_path(), &registry)?;

            if json {
                print_json(&serde_json::json!({
                    "manifest_path": manifest_path,
                    "tool_count": registry.len(),
                    "external_tools_dir": external_tools_dir,
                }))?;
            } else {
                println!("Reloaded runtime tooling.");
                println!("External tools dir: {}", external_tools_dir.display());
                println!("Registered tools: {}", registry.len());
                println!("Capabilities manifest: {}", manifest_path.display());
            }
        }

        Commands::Mcp { subcommand } => match subcommand {
            McpCommands::Serve { transport, port } => {
                let config = state.config_snapshot()?;
                let registry = Arc::new(build_tool_registry(&impulse_dir, &config)?);
                let tool_context = build_tool_context(
                    &impulse_dir,
                    &config,
                    tooling::ExecutionOrigin::Mcp,
                    false,
                    get_session_id(None),
                );
                let _ =
                    refresh_capabilities_manifest(state.storage().base_path(), registry.as_ref())?;
                let transport = match transport.as_str() {
                    "stdio" => mcp::server::McpTransport::Stdio,
                    "tcp" => mcp::server::McpTransport::Tcp(port.unwrap_or(8765)),
                    _ => anyhow::bail!("Unknown MCP transport: {} (use: stdio or tcp)", transport),
                };
                mcp::McpServer::new(registry, tool_context)
                    .serve(transport)
                    .await?;
            }
        },
        Commands::AgentConfigure {
            provider,
            api_key,
            model,
            harness,
            auto_review,
            auto_coordinate,
        } => {
            if let Some(ref p) = provider {
                if state.set_config("impulse_agent_provider", p)? {
                    println!("Set impulse_agent_provider = {}", p);
                } else {
                    eprintln!("Invalid provider: {} (use: anthropic, openai, minimax)", p);
                }
            }
            if let Some(ref key) = api_key {
                let _ = state.set_config("impulse_agent_api_key", key)?;
                println!("Set impulse_agent_api_key = ***");
            }
            if let Some(ref m) = model {
                let _ = state.set_config("impulse_agent_model", m)?;
                println!("Set impulse_agent_model = {}", m);
            }
            if let Some(ref h) = harness {
                if state.set_config("impulse_agent_harness", h)? {
                    println!("Set impulse_agent_harness = {}", h);
                } else {
                    eprintln!("Invalid harness: {} (use: claude-code, opencode)", h);
                }
            }
            if auto_review {
                let _ = state.set_config("impulse_agent_auto_review", "true")?;
                println!("Enabled auto-review");
            }
            if auto_coordinate {
                let _ = state.set_config("impulse_agent_auto_coordinate", "true")?;
                println!("Enabled auto-coordinate");
            }

            // Show resulting agent status
            let config = state.config_snapshot()?;
            let agent = agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            );
            match agent {
                Some(a) => println!("\nImpulse Agent: {}", a.status_summary()),
                None => println!("\nImpulse Agent: not configured"),
            }
        }
        Commands::AgentStatus { json } => {
            let config = state.config_snapshot()?;

            let agent = agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            );

            if json {
                let status = serde_json::json!({
                    "configured": agent.is_some(),
                    "ready": agent.as_ref().map(|a| a.is_ready()).unwrap_or(false),
                    "status": agent.as_ref().map(|a| a.status_summary()).unwrap_or_else(|| "not configured".to_string()),
                    "provider": config.impulse_agent_provider,
                    "model": config.impulse_agent_model,
                    "harness": config.impulse_agent_harness,
                    "auto_review": config.impulse_agent_auto_review,
                    "auto_coordinate": config.impulse_agent_auto_coordinate,
                });
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                branding::print_header("Impulse Agent Status");
                match agent {
                    Some(a) => {
                        println!("  Status: {}", a.status_summary());
                        println!("  Ready:  {}", if a.is_ready() { "yes" } else { "no" });
                    }
                    None => {
                        println!("  Status: not configured");
                        println!("\n  Configure with:");
                        println!("    impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY");
                        println!("    impulse-rs agent-configure --harness claude-code");
                    }
                }
                println!("  Auto-review:     {}", config.impulse_agent_auto_review);
                println!(
                    "  Auto-coordinate: {}",
                    config.impulse_agent_auto_coordinate
                );
            }
        }
        Commands::AgentQuery { prompt, json } => {
            let config = state.config_snapshot()?;

            let mut agent = agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            )
            .ok_or_else(|| anyhow::anyhow!("Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY"))?;

            if !agent.is_ready() {
                anyhow::bail!("Impulse Agent is configured but not ready (check API key or harness installation)");
            }

            let response = agent
                .query(agent::prompts::CODE_REVIEW_SYSTEM, &prompt)
                .await
                .map_err(|e| anyhow::anyhow!("Agent query failed: {}", e))?;

            if json {
                let result = serde_json::json!({
                    "response": response,
                    "agent_status": agent.status_summary(),
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", response);
            }
        }
        Commands::Guard {
            action,
            target,
            list,
            enable,
            disable,
            json,
        } => {
            let config = state.config_snapshot()?;

            if list {
                let rules = guardrail::list_active_rules(&config.guardrails);
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "rules": rules }))
                            .unwrap_or_else(|_| "{}".to_string())
                    );
                } else if rules.is_empty() {
                    println!("No active guardrail rules.");
                } else {
                    println!("Active guardrail rules ({}):\n", rules.len());
                    for rule in &rules {
                        println!("{}\n", rule.format_human());
                    }
                }
            } else if let Some(ref rule_id) = enable {
                // Validate the rule ID exists in built-in or user rules
                let all_rules = guardrail::defaults::builtin_rules();
                let mut config = state.config_snapshot()?;
                let known = all_rules.iter().any(|r| r.id == *rule_id)
                    || config.guardrails.rules.iter().any(|r| r.id == *rule_id);
                if !known {
                    eprintln!(
                        "Error: rule '{}' not found. Use --list to see available rules.",
                        rule_id
                    );
                    std::process::exit(1);
                }
                // Remove any disabled override for this rule
                config
                    .guardrails
                    .rules
                    .retain(|r| r.id != *rule_id || r.enabled);
                state.update_guardrail_rules(config.guardrails.rules.clone())?;
                println!("Enabled rule: {}", rule_id);
            } else if let Some(ref rule_id) = disable {
                // Validate the rule ID exists in built-in or user rules
                let all_rules = guardrail::defaults::builtin_rules();
                let mut config = state.config_snapshot()?;
                let known = all_rules.iter().any(|r| r.id == *rule_id)
                    || config.guardrails.rules.iter().any(|r| r.id == *rule_id);
                if !known {
                    eprintln!(
                        "Error: rule '{}' not found. Use --list to see available rules.",
                        rule_id
                    );
                    std::process::exit(1);
                }
                // Remove any existing override for this rule first
                config.guardrails.rules.retain(|r| r.id != *rule_id);
                // Add a disabled override
                config.guardrails.rules.push(guardrail::GuardRule {
                    id: rule_id.clone(),
                    pattern: String::new(),
                    action: guardrail::GuardAction::Log,
                    target: guardrail::GuardTarget::Any,
                    reason: "Disabled by user".to_string(),
                    suggestion: None,
                    enabled: false,
                    builtin: false,
                });
                state.update_guardrail_rules(config.guardrails.rules.clone())?;
                println!("Disabled rule: {}", rule_id);
            } else if let Some(ref action_str) = action {
                match guardrail::evaluate_action(action_str, &target, &config.guardrails) {
                    Ok(results) => {
                        if json {
                            let has_block = guardrail::GuardEngine::has_blocking(&results);
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "blocked": has_block,
                                    "results": results,
                                }))
                                .unwrap_or_else(|_| "{}".to_string())
                            );
                            if has_block {
                                std::process::exit(1);
                            }
                        } else if results.is_empty() {
                            eprintln!("PASS: No guardrail rules matched.");
                        } else {
                            let has_block = guardrail::GuardEngine::has_blocking(&results);
                            for result in &results {
                                eprintln!("{}", result.format_human());
                            }
                            if has_block {
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Guardrail evaluation error: {}", e);
                        std::process::exit(2);
                    }
                }
            } else {
                println!("Usage:");
                println!("  impulse-rs guard --list                         List all active rules");
                println!("  impulse-rs guard --action \"<cmd>\" --target bash  Evaluate a command");
                println!("  impulse-rs guard --enable <rule-id>              Enable a rule");
                println!("  impulse-rs guard --disable <rule-id>             Disable a rule");
                println!("  impulse-rs guard --list --json                   List rules as JSON");
                println!("  impulse-rs guard --action \"<cmd>\" --json         Evaluate as JSON");
            }
        }
        Commands::Analytics {
            subcommand,
            json,
            period,
        } => {
            if subcommand == "conflicts" {
                let history = state.get_conflict_analytics()?;
                let analytics = history.get_analytics();

                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&analytics)
                            .unwrap_or_else(|_| "{}".to_string())
                    );
                } else {
                    println!("\n=== Conflict Analytics ===\n");
                    println!("Total Conflicts: {}", analytics.total_conflicts);
                    println!(
                        "Resolved: {} ({:.1}%)",
                        analytics.resolved_count, analytics.resolution_rate
                    );
                    println!("Unresolved: {}", analytics.unresolved_count);
                    println!(
                        "Avg Time to Resolution: {}",
                        analytics.format_time_to_resolution()
                    );

                    if !analytics.most_common_files.is_empty() {
                        println!("\n--- Most Common Conflict Files ---");
                        for (file, count) in analytics.most_common_files.iter().take(5) {
                            println!("  {} ({} times)", file, count);
                        }
                    }

                    if !analytics.resolution_methods.is_empty() {
                        println!("\n--- Resolution Methods ---");
                        for (method, count) in &analytics.resolution_methods {
                            println!("  {}: {}", method, count);
                        }
                    }

                    match period.as_str() {
                        "day" => {
                            if !analytics.conflicts_by_day.is_empty() {
                                println!("\n--- Conflicts by Day ---");
                                let mut days: Vec<_> = analytics.conflicts_by_day.iter().collect();
                                days.sort_by(|a, b| a.0.cmp(b.0));
                                for (day, count) in days.iter().rev().take(7) {
                                    println!("  {}: {}", day, count);
                                }
                            }
                        }
                        "week" => {
                            if !analytics.conflicts_by_week.is_empty() {
                                println!("\n--- Conflicts by Week ---");
                                let mut weeks: Vec<_> =
                                    analytics.conflicts_by_week.iter().collect();
                                weeks.sort_by(|a, b| a.0.cmp(b.0));
                                for (week, count) in weeks.iter().rev().take(8) {
                                    println!("  {}: {}", week, count);
                                }
                            }
                        }
                        "month" => {
                            if !analytics.conflicts_by_month.is_empty() {
                                println!("\n--- Conflicts by Month ---");
                                let mut months: Vec<_> =
                                    analytics.conflicts_by_month.iter().collect();
                                months.sort_by(|a, b| a.0.cmp(b.0));
                                for (month, count) in months.iter().rev().take(6) {
                                    println!("  {}: {}", month, count);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                println!(
                    "Unknown analytics type: {}. Available: conflicts",
                    subcommand
                );
            }
        }
    }

    Ok(())
}
