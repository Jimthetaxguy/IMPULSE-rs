//! In-memory state with dirty-flag sync and Drop persistence.
//!
//! Core types: [`Config`] (runtime settings), [`State`] (session/file tracking),
//! [`LiveState`] (ephemeral session state). All wrapped in `Arc<RwLock<_>>`
//! for concurrent access. Syncs to `.impulse/` files only when dirty.
//!
//! ## Module layout
//!
//! - `config.rs` — struct definition, `Default` impl, path resolution helpers
//! - `config_keys/` — get/set/list key infrastructure, validation rules, tests

use super::*;
use serde::{Deserialize, Serialize};

// ── Section: Core ─────────────────────────────────────────────────────────
//
// General runtime settings: logging, platform default, sync interval, history cap.

/// Impulse configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,
    /// Default platform for new sessions
    pub default_platform: Option<Platform>,
    /// Enable verbose output
    pub verbose: bool,
    /// Auto-sync interval in seconds
    pub sync_interval_secs: u64,
    /// Maximum sessions to keep in history
    pub max_history_entries: usize,

    // ── Section: Retrieval ────────────────────────────────────────────────
    //
    // Search backend, embedding, semantic strategy, fuzzy matching, PageIndex.
    /// Retrieval mode: keyword or semantic
    pub retrieval_mode: String,
    /// Retrieval backend: fts or fts+vec
    pub retrieval_backend: String,
    /// Default retrieval result limit
    pub retrieval_default_limit: usize,
    /// Semantic similarity threshold (0.0 to 1.0)
    pub retrieval_similarity_threshold: f32,
    /// Embedding provider identifier
    pub retrieval_embedding_provider: String,
    /// Embedding model to use for semantic retrieval
    pub embedding_model: String,
    /// Python command used for embedding subprocess
    pub retrieval_python_cmd: String,
    /// Base URL for the Ollama embedding backend (used when
    /// `retrieval_embedding_provider` is `ollama`)
    pub retrieval_ollama_url: String,
    /// Feature flag for vector retrieval
    pub retrieval_vector_enabled: bool,
    /// Semantic backend strategy: auto, sqlite-only, rust-only
    pub retrieval_semantic_strategy: String,
    /// Query embedding timeout in seconds
    pub retrieval_query_timeout_secs: u64,
    /// Batch indexing embedding timeout in seconds
    pub retrieval_index_timeout_secs: u64,
    /// Embedding batch size
    pub retrieval_batch_size: usize,
    /// Candidate pool size for semantic reranking
    pub retrieval_candidate_pool: usize,
    /// Enable deduplication of search results
    pub retrieval_deduplicate_enabled: bool,
    /// Enable fuzzy matching for typo-tolerant search
    pub retrieval_fuzzy_matching_enabled: bool,
    /// Experimental PageIndex capability flag
    pub retrieval_experimental_pageindex_enabled: bool,
    /// PageIndex mode (local-structure or api-augmented)
    pub retrieval_pageindex_mode: String,

    // ── Section: Context Injection ────────────────────────────────────────
    //
    // Controls how context is injected into agent sessions.
    /// Injection mode: off, review, or apply
    pub context_injection_mode: String,
    /// Injection surface scope: daemon, direct, or both
    pub context_injection_scope: String,
    /// Maximum selected snippets per injection
    pub context_injection_max_items: usize,
    /// Maximum total injected characters
    pub context_injection_max_chars: usize,
    /// Minimum score threshold for semantic snippets
    pub context_injection_min_score: f32,
    /// Prefer semantic retrieval during injection
    pub context_injection_use_semantic: bool,
    /// Emit staged injection artifacts and logs
    pub context_injection_emit_artifacts: bool,

    // ── Section: Stewardship ──────────────────────────────────────────────
    //
    // Context window stewardship: thresholds, polling, cross-project learning.
    /// Stewardship mode: auto, review, or off
    pub stewardship_mode: String,
    /// Stewardship monitor threshold (start monitoring)
    pub stewardship_monitor_threshold: f32,
    /// Stewardship surgical threshold (surgical cleanup)
    pub stewardship_surgical_threshold: f32,
    /// Stewardship thoughtful threshold (thoughtful review)
    pub stewardship_thoughtful_threshold: f32,
    /// Stewardship emergency threshold (emergency summarize)
    pub stewardship_emergency_threshold: f32,
    /// Stewardship daemon poll interval in seconds
    pub stewardship_poll_interval_secs: u64,
    /// Stewardship context window token estimate
    pub stewardship_context_window_tokens: usize,
    /// Enable cross-project learning
    pub stewardship_cross_project_enabled: bool,

    // ── Section: Model Overrides ──────────────────────────────────────────
    //
    // Per-provider model selection.
    /// Model configuration: default model per provider
    pub model_anthropic: Option<String>,
    pub model_openai: Option<String>,
    pub model_google: Option<String>,
    pub model_mistral: Option<String>,

    // ── Section: Build Hygiene ────────────────────────────────────────────
    //
    // Build artifact management: scan paths, size/age thresholds, sweep triggers.
    /// Build hygiene: enable build artifact management
    pub build_hygiene_enabled: bool,
    /// Build hygiene: directories to scan (comma-separated, supports ~)
    pub build_hygiene_scan_paths: Vec<String>,
    /// Build hygiene: auto-sweep when total exceeds this (GB)
    pub build_hygiene_size_threshold_gb: f64,
    /// Build hygiene: sweep artifacts older than this (days)
    pub build_hygiene_age_threshold_days: u32,
    /// Build hygiene: sweep on session end
    pub build_hygiene_sweep_on_session_end: bool,
    /// Build hygiene: sweep when toolchain update detected
    pub build_hygiene_sweep_on_toolchain_update: bool,
    /// Build hygiene: default to dry-run for destructive ops
    pub build_hygiene_dry_run_default: bool,

    // ── Section: Context Lifecycle ────────────────────────────────────────
    //
    // Automatic context management for agent panes.
    /// Context lifecycle: enable automatic context management for agent panes
    pub context_lifecycle_enabled: bool,
    /// Context lifecycle: poll interval in seconds for monitoring
    pub context_lifecycle_poll_secs: u64,
    /// Context lifecycle: startup delay before first injection (ms)
    pub context_lifecycle_startup_delay_ms: u64,
    /// Context lifecycle: estimated context window size in tokens
    pub context_lifecycle_window_tokens: usize,

    // ── Section: Impulse Agent ────────────────────────────────────────────
    //
    // LLM provider, harness, and coordination settings for the Impulse agent.
    /// Impulse Agent: LLM provider (anthropic, openai, minimax)
    pub impulse_agent_provider: Option<String>,
    /// Impulse Agent: API key (or set via env var)
    #[serde(default, skip_serializing)]
    pub impulse_agent_api_key: Option<String>,
    /// Impulse Agent: model override
    pub impulse_agent_model: Option<String>,
    /// Impulse Agent: CLI harness (claude-code, opencode)
    pub impulse_agent_harness: Option<String>,
    /// Impulse Agent: enable automatic review of cross-pane activity
    pub impulse_agent_auto_review: bool,
    /// Impulse Agent: enable automatic cross-pane coordination
    pub impulse_agent_auto_coordinate: bool,

    // ── Section: Notifications ────────────────────────────────────────────
    //
    // Real-time conflict notifications and webhooks.
    /// Enable real-time conflict notifications
    pub notifications_enabled: bool,
    /// Conflict webhook URL for external notifications
    pub conflict_webhook_url: Option<String>,
    /// Enable conflict webhook notifications
    pub conflict_webhook_enabled: bool,

    // ── Section: Tool Execution ───────────────────────────────────────────
    //
    // Timeouts, output limits, allowed roots, external tool/MCP configuration.
    /// Default tool execution timeout in milliseconds
    pub tool_execution_default_timeout_ms: u64,
    /// Maximum serialized tool output size
    pub tool_execution_max_output_bytes: usize,
    /// Maximum tool artifacts returned per invocation
    pub tool_execution_max_artifacts: usize,
    /// Allowed roots for read-capable tools
    pub tool_execution_allowed_read_roots: Vec<String>,
    /// Allowed roots for write-capable tools
    pub tool_execution_allowed_write_roots: Vec<String>,
    /// External tool manifest directory
    pub external_tools_dir: String,
    /// Reserved external MCP server definitions
    pub external_mcp_servers: Vec<String>,

    // ── Section: Supervisor & Guardrails ──────────────────────────────────
    //
    // Agent control plane permissions and guardrail rules.
    /// Baseline supervisor permissions for the Impulse agent control plane
    #[serde(default)]
    pub impulse_agent_permissions: impulse_ops::SupervisorPermissionPolicy,
    /// Guardrail configuration
    #[serde(default)]
    pub guardrails: crate::guardrail::GuardConfig,
}

// ── Default impl ──────────────────────────────────────────────────────────

impl Default for Config {
    fn default() -> Self {
        Self {
            // Core
            log_level: "info".to_string(),
            default_platform: None,
            verbose: false,
            sync_interval_secs: 30,
            max_history_entries: 1000,

            // Retrieval
            retrieval_mode: "keyword".to_string(),
            retrieval_backend: "fts".to_string(),
            retrieval_default_limit: 10,
            retrieval_similarity_threshold: 0.75,
            retrieval_embedding_provider: "python-st".to_string(),
            embedding_model: "all-MiniLM-L6-v2".to_string(),
            retrieval_python_cmd: "python3".to_string(),
            retrieval_ollama_url: "http://localhost:11434".to_string(),
            retrieval_vector_enabled: false,
            retrieval_semantic_strategy: "auto".to_string(),
            retrieval_query_timeout_secs: 10,
            retrieval_index_timeout_secs: 60,
            retrieval_batch_size: 64,
            retrieval_candidate_pool: 200,
            retrieval_deduplicate_enabled: true,
            retrieval_fuzzy_matching_enabled: false,
            retrieval_experimental_pageindex_enabled: false,
            retrieval_pageindex_mode: "local-structure".to_string(),

            // Context Injection
            context_injection_mode: "review".to_string(),
            context_injection_scope: "both".to_string(),
            context_injection_max_items: 5,
            context_injection_max_chars: 2000,
            context_injection_min_score: 0.60,
            context_injection_use_semantic: true,
            context_injection_emit_artifacts: true,

            // Stewardship
            stewardship_mode: "review".to_string(),
            stewardship_monitor_threshold: 0.30,
            stewardship_surgical_threshold: 0.45,
            stewardship_thoughtful_threshold: 0.60,
            stewardship_emergency_threshold: 0.80,
            stewardship_poll_interval_secs: 10,
            stewardship_context_window_tokens: 200_000,
            stewardship_cross_project_enabled: true,

            // Model Overrides
            model_anthropic: None,
            model_openai: None,
            model_google: None,
            model_mistral: None,

            // Build Hygiene
            build_hygiene_enabled: true,
            build_hygiene_scan_paths: vec!["~/projects".to_string(), "~/Desktop".to_string()],
            build_hygiene_size_threshold_gb: 10.0,
            build_hygiene_age_threshold_days: 30,
            build_hygiene_sweep_on_session_end: false,
            build_hygiene_sweep_on_toolchain_update: true,
            build_hygiene_dry_run_default: true,

            // Context Lifecycle
            context_lifecycle_enabled: true,
            context_lifecycle_poll_secs: 5,
            context_lifecycle_startup_delay_ms: 3000,
            context_lifecycle_window_tokens: 200_000,

            // Impulse Agent
            impulse_agent_provider: None,
            impulse_agent_api_key: None,
            impulse_agent_model: None,
            impulse_agent_harness: None,
            impulse_agent_auto_review: false,
            impulse_agent_auto_coordinate: false,

            // Notifications
            notifications_enabled: true,
            conflict_webhook_url: None,
            conflict_webhook_enabled: false,

            // Tool Execution
            tool_execution_default_timeout_ms: 30_000,
            tool_execution_max_output_bytes: 256 * 1024,
            tool_execution_max_artifacts: 8,
            tool_execution_allowed_read_roots: vec![".".to_string(), ".impulse".to_string()],
            tool_execution_allowed_write_roots: vec![".impulse".to_string()],
            external_tools_dir: ".impulse/tools.d".to_string(),
            external_mcp_servers: Vec::new(),

            // Supervisor & Guardrails
            impulse_agent_permissions: impulse_ops::SupervisorPermissionPolicy::default(),
            guardrails: crate::guardrail::GuardConfig::default(),
        }
    }
}

// ── Path resolution helpers ───────────────────────────────────────────────

impl Config {
    pub fn resolve_path_setting(&self, value: &str) -> std::path::PathBuf {
        self.resolve_path_setting_from(
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            value,
        )
    }

    pub fn resolve_path_setting_from(
        &self,
        base_dir: &std::path::Path,
        value: &str,
    ) -> std::path::PathBuf {
        let path = std::path::PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            base_dir.join(path)
        }
    }

    pub fn resolved_external_tools_dir(&self) -> std::path::PathBuf {
        self.resolve_path_setting(&self.external_tools_dir)
    }

    pub fn resolved_external_tools_dir_from(
        &self,
        base_dir: &std::path::Path,
    ) -> std::path::PathBuf {
        self.resolve_path_setting_from(base_dir, &self.external_tools_dir)
    }

    pub fn resolved_tool_read_roots(&self) -> Vec<std::path::PathBuf> {
        let base_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.resolved_tool_read_roots_from(&base_dir)
    }

    pub fn resolved_tool_read_roots_from(
        &self,
        base_dir: &std::path::Path,
    ) -> Vec<std::path::PathBuf> {
        self.tool_execution_allowed_read_roots
            .iter()
            .map(|value| self.resolve_path_setting_from(base_dir, value))
            .collect()
    }

    pub fn resolved_tool_write_roots(&self) -> Vec<std::path::PathBuf> {
        let base_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.resolved_tool_write_roots_from(&base_dir)
    }

    pub fn resolved_tool_write_roots_from(
        &self,
        base_dir: &std::path::Path,
    ) -> Vec<std::path::PathBuf> {
        self.tool_execution_allowed_write_roots
            .iter()
            .map(|value| self.resolve_path_setting_from(base_dir, value))
            .collect()
    }
}

// ── Path resolution tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_path_setting_relative() {
        let c = Config::default();
        let base = std::path::Path::new("/home/user/project");
        let result = c.resolve_path_setting_from(base, ".impulse/tools.d");
        assert_eq!(
            result,
            std::path::PathBuf::from("/home/user/project/.impulse/tools.d")
        );
    }

    #[test]
    fn resolve_path_setting_absolute() {
        let c = Config::default();
        let base = std::path::Path::new("/home/user/project");
        let result = c.resolve_path_setting_from(base, "/opt/impulse/tools");
        assert_eq!(result, std::path::PathBuf::from("/opt/impulse/tools"));
    }

    #[test]
    fn resolved_tool_roots_from_base() {
        let c = Config::default();
        let base = std::path::Path::new("/project");
        let read = c.resolved_tool_read_roots_from(base);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], std::path::PathBuf::from("/project/."));
        assert_eq!(read[1], std::path::PathBuf::from("/project/.impulse"));
        let write = c.resolved_tool_write_roots_from(base);
        assert_eq!(write.len(), 1);
        assert_eq!(write[0], std::path::PathBuf::from("/project/.impulse"));
    }
}
