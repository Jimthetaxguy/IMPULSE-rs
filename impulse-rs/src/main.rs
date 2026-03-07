use anyhow::Result;
use clap::{Parser, Subcommand};
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
pub mod envelope;
pub mod error;
pub mod guardrail;
pub mod handlers;
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
pub mod semantic_diff;
pub mod state;
pub mod stewardship;
pub mod storage;
pub mod tooling;
pub mod tools;
pub mod ui;
pub mod validate;
pub mod verify;

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

    /// Output format: json (default for pipes), text (default for TTY), ndjson (streaming)
    #[arg(long, global = true)]
    format: Option<envelope::OutputFormat>,
}

#[derive(Subcommand)]
pub(crate) enum McpCommands {
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
        /// Capture semantic diff (requires sem CLI). Provide base ref (e.g. commit at session start)
        #[arg(long)]
        sem_diff_base: Option<String>,
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
    /// Semantic diff between two Git refs using the `sem` tool
    SemDiff {
        /// Base Git ref (commit, branch, tag). Defaults to HEAD~1
        #[arg(long, default_value = "HEAD~1")]
        base: String,
        /// Head Git ref. Defaults to HEAD
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Session ID to associate the diff with (stores result)
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Semantic blame for a file (entity-level git blame)
    SemBlame {
        /// File to blame
        #[arg(long)]
        file: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Semantic impact analysis for an entity (blast radius)
    SemImpact {
        /// Entity name to analyze (e.g. function or struct name)
        #[arg(long)]
        entity: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check if the sem CLI tool is available
    SemStatus {
        /// Output as JSON
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

    /// Emit a machine-readable registry of all commands with schemas and examples (ATCC v1)
    Describe,

    /// Emit JSON Schema for a specific command path (ATCC v1)
    Schema {
        /// Command path (e.g. "session-start", "guard", "tooling-list")
        command: String,
    },
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
// Helpers (kept in main.rs — used before handler module is available)
// ============================================================================

/// Read an env var with fallback to a deprecated name, emitting a warning on stderr.
pub(crate) fn env_with_fallback(new_name: &str, old_name: &str) -> Option<String> {
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
                handlers::print_json(&status)?;
            }
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode: _,
            inject_explain: _,
        } => {
            let stdin_payload = handlers::read_hook_stdin_payload();
            let name = name.unwrap_or_else(handlers::default_session_name);
            match client.create_session(name, platform).await {
                Ok((id, n)) => {
                    let _ = handlers::persist_claude_env_var("IMPULSE_SESSION_ID", &id);
                    handlers::capture_hook_evidence(
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
            sem_diff_base,
        } => {
            let stdin_payload = handlers::read_hook_stdin_payload();
            if should_verify {
                let steps = verify::default_steps(&std::env::current_dir()?);
                let report = verify::run_verification(steps)?;
                handlers::print_verification_report(&report);
                if !report.success() {
                    anyhow::bail!("Verification failed. Session end blocked.");
                }
            }
            // Capture semantic diff if requested and sem is available
            if let Some(base_ref) = &sem_diff_base {
                if semantic_diff::sem_available() {
                    match semantic_diff::capture_semantic_diff(
                        &cli.impulse_dir,
                        &std::env::current_dir()?,
                        &session_id,
                        base_ref,
                        "HEAD",
                    ) {
                        Ok(report) => {
                            if !report.changes.is_empty() {
                                eprintln!("Semantic diff: {}", report.summary);
                            }
                        }
                        Err(e) => eprintln!("Warning: semantic diff failed: {}", e),
                    }
                }
            }
            match client
                .end_session(session_id.clone(), summary.clone())
                .await
            {
                Ok(_) => {
                    handlers::capture_hook_evidence(
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
            if let Some(sid) = handlers::get_session_id(session_id) {
                match client.track_file(sid, file).await {
                    Ok(_) => println!("Tracked file"),
                    Err(e) => eprintln!("Error: {}", e),
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::TrackTool { tool, session_id } => {
            if let Some(sid) = handlers::get_session_id(session_id) {
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
            Ok(s) => handlers::print_json(&s)?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionConflicts { file, session_id } => {
            let sid = match handlers::get_session_id(session_id) {
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
                            println!("\u{26a0}\u{fe0f}  CONFLICT DETECTED");
                            println!("File is being edited by: {}", sessions.join(", "));
                        } else {
                            println!("\u{2713} No conflicts detected");
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
                                        "  \u{26a0}\u{fe0f}  {} - being edited by: {}",
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
            Ok(s) => handlers::print_json(&s)?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::Chat {
            session_id,
            message,
            inject_mode,
            inject_explain,
        } => {
            let inject_mode = match handlers::parse_injection_mode(inject_mode.as_deref()) {
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
                        handlers::print_json(&result)?;
                    } else if let Some(response) = result.get("response").and_then(|v| v.as_str()) {
                        println!("{}", response);
                    } else {
                        handlers::print_json(&result)?;
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Verify => {
            let steps = verify::default_steps(&std::env::current_dir()?);
            let report = verify::run_verification(steps)?;
            handlers::print_verification_report(&report);
            if !report.success() {
                anyhow::bail!("Verification failed");
            }
        }
        Commands::Describe | Commands::Schema { .. } => {
            // Introspection commands work without daemon state
            let fmt = cli.format.unwrap_or(envelope::OutputFormat::Json);
            match cli.command {
                Commands::Describe => handlers::describe::handle_describe(fmt)?,
                Commands::Schema { command } => handlers::describe::handle_schema(&command, fmt)?,
                _ => unreachable!(),
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
// Direct Mode — thin dispatcher delegating to handler modules
// ============================================================================

async fn run_direct_mode(cli: Cli) -> Result<()> {
    let impulse_dir = cli.impulse_dir.clone();
    let verbose = cli.verbose;
    let state = Arc::new(state::State::new(impulse_dir.clone())?);

    match cli.command {
        Commands::Daemon { .. } => {
            daemon::Daemon::new(state.clone()).start().await?;
        }
        Commands::Run => {
            println!("Use: impulse-rs --daemon for daemon mode");
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode,
            inject_explain,
        } => {
            handlers::session::handle_session_start(
                &state,
                name,
                platform,
                inject_mode,
                inject_explain,
            )
            .await?;
        }
        Commands::SessionEnd {
            session_id,
            summary,
            verify,
            sem_diff_base,
        } => {
            handlers::session::handle_session_end(
                &state,
                session_id,
                summary,
                verify,
                sem_diff_base,
            )
            .await?;
        }
        Commands::TrackWrite { file, session_id } => {
            handlers::session::handle_track_write(&state, file, session_id).await?;
        }
        Commands::TrackTool { tool, session_id } => {
            handlers::session::handle_track_tool(&state, tool, session_id).await?;
        }
        Commands::ListSessions => {
            handlers::session::handle_list_sessions(&state).await?;
        }
        Commands::SessionInfo { id } => {
            handlers::session::handle_session_info(&state, id).await?;
        }
        Commands::SessionConflicts { file, session_id } => {
            handlers::session::handle_session_conflicts(&state, file, session_id).await?;
        }
        Commands::Status => {
            handlers::config::handle_status(&state).await?;
        }
        Commands::Chat { .. } => {
            handlers::system::handle_chat();
        }
        Commands::Genome => {
            handlers::memory::handle_genome(&state)?;
        }
        Commands::History => {
            handlers::memory::handle_history(&state)?;
        }
        Commands::ListProviders => {
            handlers::config::handle_list_providers()?;
        }
        Commands::AddDecision {
            description,
            rationale,
        } => {
            handlers::memory::handle_add_decision(&state, description, rationale)?;
        }
        Commands::Init => {
            handlers::config::handle_init(&state, &impulse_dir)?;
        }
        Commands::Config { key, value, list } => {
            handlers::config::handle_config(&state, key, value, list)?;
        }
        Commands::Extract {
            content,
            session_id,
            json,
        } => {
            handlers::system::handle_extract(content, session_id, json)?;
        }
        Commands::Swarm {
            agent_a,
            agent_b,
            threshold,
            json,
        } => {
            handlers::system::handle_swarm(agent_a, agent_b, threshold, json)?;
        }
        Commands::Activity { limit } => {
            handlers::memory::handle_activity(&state, limit).await?;
        }
        Commands::Hooks { platform } => {
            handlers::system::handle_hooks(&state, platform)?;
        }
        Commands::ValidateHooks { platform } => {
            handlers::system::handle_validate_hooks(platform)?;
        }
        Commands::Orchestrate {
            task,
            inject_mode,
            inject_explain,
            compute_routing,
        } => {
            handlers::injection_handlers::handle_orchestrate(
                &state,
                task,
                inject_mode,
                inject_explain,
                compute_routing,
            )
            .await?;
        }
        Commands::Handoff {
            tool,
            task,
            session_id,
            notes,
            inject_mode,
            inject_explain,
        } => {
            handlers::injection_handlers::handle_handoff(
                &state,
                tool,
                task,
                session_id,
                notes,
                inject_mode,
                inject_explain,
            )
            .await?;
        }
        Commands::SyncContext {
            session_id,
            inject_mode,
            inject_explain,
        } => {
            handlers::injection_handlers::handle_sync_context(
                &state,
                session_id,
                inject_mode,
                inject_explain,
            )
            .await?;
        }
        Commands::ComputeInjection { query, limit, json } => {
            handlers::injection_handlers::handle_compute_injection(query, limit, json)?;
        }
        Commands::Verify => {
            handlers::build::handle_verify()?;
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
            handlers::memory::handle_search_history(
                &state, query, mode, backend, limit, offset, page, total, explain, json,
            )?;
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
            handlers::memory::handle_search_genome(
                &state, query, mode, backend, limit, offset, page, total, explain, json,
            )?;
        }
        Commands::IndexMemory { scope, rebuild } => {
            handlers::retrieval::handle_index_memory(&state, scope, rebuild)?;
        }
        Commands::RetrievalStatus { check, json } => {
            handlers::retrieval::handle_retrieval_status(&state, check, json)?;
        }
        Commands::Tools {
            subcommand,
            tool,
            dry_run,
        } => {
            handlers::system::handle_tools(verbose, subcommand, tool, dry_run)?;
        }
        Commands::Docs {
            subcommand,
            provider,
            verbose: docs_verbose,
            force,
        } => {
            handlers::system::handle_docs(
                &state,
                verbose,
                subcommand,
                provider,
                docs_verbose,
                force,
            )
            .await?;
        }
        Commands::Model {
            subcommand,
            provider,
            model,
        } => {
            handlers::config::handle_model(
                &state,
                &impulse_dir,
                verbose,
                subcommand,
                provider,
                model,
            )?;
        }
        Commands::Office {
            subcommand,
            file,
            goal,
            json,
        } => {
            handlers::office::handle_office(subcommand, file, goal, json)?;
        }
        Commands::Credentials {
            subcommand,
            provider,
            key,
            value,
            socket_path,
            tool,
        } => {
            handlers::config::handle_credentials(
                subcommand,
                provider,
                key,
                value,
                socket_path,
                tool,
            )?;
        }
        Commands::Steward {
            subcommand,
            transcript,
            session_id,
            id,
            json,
        } => {
            handlers::stewardship_handlers::handle_steward(
                &state,
                &impulse_dir,
                subcommand,
                transcript,
                session_id,
                id,
                json,
            )?;
        }
        Commands::Calc { expression } => {
            handlers::system::handle_calc(expression)?;
        }
        Commands::Exec { code } => {
            handlers::system::handle_exec(code)?;
        }
        Commands::System {} => {
            handlers::system::handle_system()?;
        }
        Commands::Analyze { session_id, scope } => {
            handlers::stewardship_handlers::handle_analyze(&state, session_id, scope).await?;
        }
        Commands::Health {} => {
            handlers::stewardship_handlers::handle_health(&impulse_dir)?;
        }
        Commands::Summary {} => {
            handlers::stewardship_handlers::handle_summary(&impulse_dir)?;
        }
        Commands::Sweep {
            dry_run,
            path,
            days,
            verbose: _,
        } => {
            handlers::build::handle_sweep(&state, dry_run, path, days, verbose)?;
        }
        Commands::Wipe { dry_run, path } => {
            handlers::build::handle_wipe(&state, dry_run, path)?;
        }
        Commands::CleanAll { dry_run } => {
            handlers::build::handle_clean_all(&state, dry_run)?;
        }
        Commands::SccacheSetup { check, json } => {
            handlers::build::handle_sccache_setup(check, json)?;
        }
        Commands::BuildHealth { json } => {
            handlers::build::handle_build_health(&state, json)?;
        }
        Commands::ToolingList { category, json } => {
            handlers::tooling_handlers::handle_tooling_list(&state, &impulse_dir, category, json)?;
        }
        Commands::ToolingDescribe { tool_id, json } => {
            handlers::tooling_handlers::handle_tooling_describe(
                &state,
                &impulse_dir,
                tool_id,
                json,
            )?;
        }
        Commands::ToolingRun {
            tool_id,
            params,
            json,
        } => {
            handlers::tooling_handlers::handle_tooling_run(
                &state,
                &impulse_dir,
                tool_id,
                params,
                json,
            )
            .await?;
        }
        Commands::ToolingSchema { format } => {
            handlers::tooling_handlers::handle_tooling_schema(&state, &impulse_dir, format)?;
        }
        Commands::ToolingValidate { json } => {
            handlers::tooling_handlers::handle_tooling_validate(&state, &impulse_dir, json)?;
        }
        Commands::ToolingReload { json } => {
            handlers::tooling_handlers::handle_tooling_reload(&state, &impulse_dir, json)?;
        }
        Commands::Mcp { subcommand } => {
            handlers::system::handle_mcp(&state, &impulse_dir, subcommand).await?;
        }
        Commands::AgentConfigure {
            provider,
            api_key,
            model,
            harness,
            auto_review,
            auto_coordinate,
        } => {
            handlers::agent::handle_agent_configure(
                &state,
                provider,
                api_key,
                model,
                harness,
                auto_review,
                auto_coordinate,
            )?;
        }
        Commands::AgentStatus { json } => {
            handlers::agent::handle_agent_status(&state, json)?;
        }
        Commands::AgentQuery { prompt, json } => {
            handlers::agent::handle_agent_query(&state, prompt, json).await?;
        }
        Commands::SemDiff {
            base,
            head,
            json,
            session_id,
        } => {
            handlers::semantic_diff_handlers::handle_sem_diff(
                &state, base, head, json, session_id,
            )?;
        }
        Commands::SemBlame { file, json } => {
            handlers::semantic_diff_handlers::handle_sem_blame(file, json)?;
        }
        Commands::SemImpact { entity, json } => {
            handlers::semantic_diff_handlers::handle_sem_impact(entity, json)?;
        }
        Commands::SemStatus { json } => {
            handlers::semantic_diff_handlers::handle_sem_status(json)?;
        }
        Commands::Guard {
            action,
            target,
            list,
            enable,
            disable,
            json,
        } => {
            handlers::guard::handle_guard(&state, action, target, list, enable, disable, json)?;
        }
        Commands::Analytics {
            subcommand,
            json,
            period,
        } => {
            handlers::guard::handle_analytics(&state, subcommand, json, period)?;
        }
        Commands::Describe => {
            let fmt = cli.format.unwrap_or(envelope::OutputFormat::Json);
            handlers::describe::handle_describe(fmt)?;
        }
        Commands::Schema { command } => {
            let fmt = cli.format.unwrap_or(envelope::OutputFormat::Json);
            handlers::describe::handle_schema(&command, fmt)?;
        }
    }

    Ok(())
}
