//! Clap CLI argument definitions.
//!
//! Extracted from main.rs to keep the entry point focused on dispatch logic.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::envelope;

pub const PRODUCT_DESCRIPTION: &str =
    "Feed the impulse to build — a terminal-native local control plane and harness manager for AI coding agents";

#[derive(Parser)]
#[command(name = "impulse-rs")]
#[command(about = PRODUCT_DESCRIPTION, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short = 'c', long, default_value = ".impulse")]
    pub impulse_dir: PathBuf,

    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long)]
    pub daemon: bool,

    #[arg(long)]
    pub socket: Option<PathBuf>,

    /// Internal lifecycle binding for a desktop-spawned daemon companion.
    ///
    /// The daemon accepts this only when the supplied PID is its exact direct
    /// parent, so operator-started daemons remain independent by default.
    #[arg(long, hide = true)]
    pub owner_pid: Option<u32>,

    /// Output format: json (default for pipes), text (default for TTY), ndjson (streaming)
    #[arg(long, global = true)]
    pub format: Option<envelope::OutputFormat>,
}

#[derive(Subcommand)]
pub enum McpCommands {
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
pub enum Commands {
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
    /// Show daemon/session status. Includes registered agent platforms (claude-code, codex, ...) from AgentRegistry and workspace cycling note.
    Status,
    /// Show detailed daemon internal state snapshot (sessions, tools, plugins, config)
    Debug,
    /// Show historical file conflict audit trail
    ConflictHistory,
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

    /// List registered plugins (context providers + action handlers)
    PluginList {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Invoke a named action handler plugin
    PluginInvoke {
        /// Plugin name
        name: String,
        /// Path to operate on
        #[arg(long)]
        path: Option<String>,
        /// Query string
        #[arg(long)]
        query: Option<String>,
        /// Extra options as JSON
        #[arg(long)]
        options: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Run the Ion verification gate (harness #2 — Pi on MiniMax) against a diff
    IonVerify {
        /// Repository path to verify (defaults to the current directory)
        #[arg(long)]
        repo: Option<String>,
        /// Git ref range to verify, e.g. HEAD~1..HEAD
        #[arg(long, default_value = "HEAD~1..HEAD")]
        diff_ref: String,
        /// Task description passed to the gate
        #[arg(long, default_value = "Verify the pending diff.")]
        description: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Submit a closed-loop Builder claim through the project daemon.
    GovernedClaim {
        /// Project id injected by Impulse (`IMPULSE_PROJECT_ID`) when omitted.
        #[arg(long)]
        project_id: Option<String>,
        /// Governed task id injected by Impulse (`IMPULSE_GOVERNED_TASK_ID`) when omitted.
        #[arg(long)]
        task_id: Option<String>,
        /// Builder-authored completion summary. Actor and Git subject are daemon-derived.
        #[arg(long)]
        summary: String,
        /// Project artifact IDs supporting the claim.
        #[arg(long = "artifact-id")]
        artifact_ids: Vec<String>,
        /// Output the acknowledged daemon-owned task as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Ask the daemon to execute the governed task's closed verification profile.
    GovernedVerify {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ask the configured Impulse Agent to perform a strict governed Supervisor review.
    GovernedReview {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}
