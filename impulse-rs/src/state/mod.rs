//! In-memory state with dirty-flag sync and Drop persistence.
//!
//! Core types: [`Config`] (runtime settings), [`State`] (session/file tracking),
//! [`LiveState`] (ephemeral session state). All wrapped in `Arc<RwLock<_>>`
//! for concurrent access. Syncs to `.impulse/` files only when dirty.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::storage::{get_working_dir_name, sanitize_filename, Storage};

const LIVE_STATE_FILE: &str = "LIVE_STATE.json";
const HISTORY_FILE: &str = "HISTORY.jsonl";
const CONFIG_FILE: &str = "config.json";

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
    /// Model configuration: default model per provider
    pub model_anthropic: Option<String>,
    pub model_openai: Option<String>,
    pub model_google: Option<String>,
    pub model_mistral: Option<String>,
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
    /// Context lifecycle: enable automatic context management for agent panes
    pub context_lifecycle_enabled: bool,
    /// Context lifecycle: poll interval in seconds for monitoring
    pub context_lifecycle_poll_secs: u64,
    /// Context lifecycle: startup delay before first injection (ms)
    pub context_lifecycle_startup_delay_ms: u64,
    /// Context lifecycle: estimated context window size in tokens
    pub context_lifecycle_window_tokens: usize,
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
    /// Enable real-time conflict notifications
    pub notifications_enabled: bool,
    /// Conflict webhook URL for external notifications
    pub conflict_webhook_url: Option<String>,
    /// Enable conflict webhook notifications
    pub conflict_webhook_enabled: bool,
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
    /// Baseline supervisor permissions for the Impulse agent control plane
    #[serde(default)]
    pub impulse_agent_permissions: impulse_ops::SupervisorPermissionPolicy,
    /// Guardrail configuration
    #[serde(default)]
    pub guardrails: crate::guardrail::GuardConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            default_platform: None,
            verbose: false,
            sync_interval_secs: 30,
            max_history_entries: 1000,
            retrieval_mode: "keyword".to_string(),
            retrieval_backend: "fts".to_string(),
            retrieval_default_limit: 10,
            retrieval_similarity_threshold: 0.75,
            retrieval_embedding_provider: "python-st".to_string(),
            embedding_model: "all-MiniLM-L6-v2".to_string(),
            retrieval_python_cmd: "python3".to_string(),
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
            context_injection_mode: "review".to_string(),
            context_injection_scope: "both".to_string(),
            context_injection_max_items: 5,
            context_injection_max_chars: 2000,
            context_injection_min_score: 0.60,
            context_injection_use_semantic: true,
            context_injection_emit_artifacts: true,
            stewardship_mode: "review".to_string(),
            stewardship_monitor_threshold: 0.30,
            stewardship_surgical_threshold: 0.45,
            stewardship_thoughtful_threshold: 0.60,
            stewardship_emergency_threshold: 0.80,
            stewardship_poll_interval_secs: 10,
            stewardship_context_window_tokens: 200_000,
            stewardship_cross_project_enabled: true,
            model_anthropic: None,
            model_openai: None,
            model_google: None,
            model_mistral: None,
            build_hygiene_enabled: true,
            build_hygiene_scan_paths: vec!["~/projects".to_string(), "~/Desktop".to_string()],
            build_hygiene_size_threshold_gb: 10.0,
            build_hygiene_age_threshold_days: 30,
            build_hygiene_sweep_on_session_end: false,
            build_hygiene_sweep_on_toolchain_update: true,
            build_hygiene_dry_run_default: true,
            context_lifecycle_enabled: true,
            context_lifecycle_poll_secs: 5,
            context_lifecycle_startup_delay_ms: 3000,
            context_lifecycle_window_tokens: 200_000,
            impulse_agent_provider: None,
            impulse_agent_api_key: None,
            impulse_agent_model: None,
            impulse_agent_harness: None,
            impulse_agent_auto_review: false,
            impulse_agent_auto_coordinate: false,
            notifications_enabled: true,
            conflict_webhook_url: None,
            conflict_webhook_enabled: false,
            tool_execution_default_timeout_ms: 30_000,
            tool_execution_max_output_bytes: 256 * 1024,
            tool_execution_max_artifacts: 8,
            tool_execution_allowed_read_roots: vec![".".to_string(), ".impulse".to_string()],
            tool_execution_allowed_write_roots: vec![".impulse".to_string()],
            external_tools_dir: ".impulse/tools.d".to_string(),
            external_mcp_servers: Vec::new(),
            impulse_agent_permissions: impulse_ops::SupervisorPermissionPolicy::default(),
            guardrails: crate::guardrail::GuardConfig::default(),
        }
    }
}

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

    // ── Config key infrastructure ──────────────────────────────────────────────
    //
    // All 69 config keys in display order. Used by get(), list(), and set().
    // Adding a new config field requires:
    //   1. Add the field to the Config struct
    //   2. Add the key to CONFIG_KEYS
    //   3. Add a SetRule entry in build_set_rules()
    //   4. If the key name differs from the field name, add to resolve_field_name()

    /// Ordered list of all config keys (used by `list()`).
    const CONFIG_KEYS: &'static [&'static str] = &[
        "log_level", "default_platform", "verbose", "sync_interval_secs",
        "max_history_entries", "retrieval_mode", "retrieval_backend",
        "retrieval_default_limit", "retrieval_similarity_threshold",
        "retrieval_embedding_provider", "embedding_model", "retrieval_python_cmd",
        "retrieval_vector_enabled", "retrieval_semantic_strategy",
        "retrieval_query_timeout_secs", "retrieval_index_timeout_secs",
        "retrieval_batch_size", "retrieval_candidate_pool",
        "retrieval_deduplicate_enabled", "retrieval_fuzzy_matching_enabled",
        "retrieval_experimental_pageindex_enabled", "retrieval_pageindex_mode",
        "context_injection_mode", "context_injection_scope",
        "context_injection_max_items", "context_injection_max_chars",
        "context_injection_min_score", "context_injection_use_semantic",
        "context_injection_emit_artifacts", "stewardship_mode",
        "stewardship_monitor_threshold", "stewardship_surgical_threshold",
        "stewardship_thoughtful_threshold", "stewardship_emergency_threshold",
        "stewardship_poll_interval_secs", "stewardship_context_window_tokens",
        "stewardship_cross_project_enabled",
        "model.anthropic", "model.openai", "model.google", "model.mistral",
        "build_hygiene_enabled", "build_hygiene_scan_paths",
        "build_hygiene_size_threshold_gb", "build_hygiene_age_threshold_days",
        "build_hygiene_sweep_on_session_end", "build_hygiene_sweep_on_toolchain_update",
        "build_hygiene_dry_run_default",
        "context_lifecycle_enabled", "context_lifecycle_poll_secs",
        "context_lifecycle_startup_delay_ms", "context_lifecycle_window_tokens",
        "impulse_agent_provider", "impulse_agent_api_key",
        "impulse_agent_model", "impulse_agent_harness",
        "impulse_agent_auto_review", "impulse_agent_auto_coordinate",
        "notifications_enabled", "conflict_webhook_url", "conflict_webhook_enabled",
        "tool_execution.default_timeout_ms", "tool_execution.max_output_bytes",
        "tool_execution.max_artifacts", "tool_execution.allowed_read_roots",
        "tool_execution.allowed_write_roots",
        "external_tools_dir", "external_mcp_servers",
        "impulse_agent_permissions", "guardrails_enabled",
    ];

    /// Map user-facing key → serde field name (only for keys that differ).
    fn resolve_field_name(key: &str) -> &str {
        match key {
            "model.anthropic" => "model_anthropic",
            "model.openai" => "model_openai",
            "model.google" => "model_google",
            "model.mistral" => "model_mistral",
            "tool_execution.default_timeout_ms" => "tool_execution_default_timeout_ms",
            "tool_execution.max_output_bytes" => "tool_execution_max_output_bytes",
            "tool_execution.max_artifacts" => "tool_execution_max_artifacts",
            "tool_execution.allowed_read_roots" => "tool_execution_allowed_read_roots",
            "tool_execution.allowed_write_roots" => "tool_execution_allowed_write_roots",
            _ => key,
        }
    }

    /// Convert a JSON value from serde reflection to a display string.
    fn json_value_to_string(v: &serde_json::Value) -> Option<String> {
        match v {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            other => Some(other.to_string()),
        }
    }

    /// Get a config value by key (serde reflection with special-case overrides).
    pub fn get(&self, key: &str) -> Option<String> {
        // Fields that can't use serde reflection
        match key {
            "default_platform" => {
                return self.default_platform.map(|p| p.as_str().to_string());
            }
            "impulse_agent_api_key" => {
                return self.impulse_agent_api_key.as_ref().map(|_| "***".to_string());
            }
            "impulse_agent_permissions" => {
                return serde_json::to_string(&self.impulse_agent_permissions).ok();
            }
            "guardrails_enabled" => return Some(self.guardrails.enabled.to_string()),
            _ => {}
        }

        let field = Self::resolve_field_name(key);
        let value = serde_json::to_value(self).ok()?;
        let obj = value.as_object()?;
        let v = obj.get(field)?;
        Self::json_value_to_string(v)
    }

    /// Set a config value by key (validation registry + serde round-trip).
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let rules = Self::build_set_rules();
        let Some(rule) = rules.get(key) else {
            return false;
        };

        match rule {
            SetRule::Bool => match value.parse::<bool>() {
                Ok(b) => self.set_field_json(Self::resolve_field_name(key), serde_json::Value::Bool(b)),
                Err(_) => false,
            },
            SetRule::String => {
                if value.trim().is_empty() {
                    return false;
                }
                self.set_field_json(
                    Self::resolve_field_name(key),
                    serde_json::Value::String(value.to_string()),
                )
            }
            SetRule::OptionalString => {
                let json = if value.is_empty() || value == "none" {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(value.to_string())
                };
                self.set_field_json(Self::resolve_field_name(key), json)
            }
            SetRule::SomeString => self.set_field_json(
                Self::resolve_field_name(key),
                serde_json::Value::String(value.to_string()),
            ),
            SetRule::CsvList => {
                let items: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect();
                self.set_field_json(
                    Self::resolve_field_name(key),
                    serde_json::to_value(items).unwrap_or_default(),
                )
            }
            SetRule::U64 { min, max } => {
                let v: u64 = match value.parse() {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                if v < *min || v > *max {
                    return false;
                }
                self.set_field_json(
                    Self::resolve_field_name(key),
                    serde_json::Value::Number(v.into()),
                )
            }
            SetRule::Usize { min, max } => {
                let v: usize = match value.parse() {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                if v < *min || v > *max {
                    return false;
                }
                self.set_field_json(
                    Self::resolve_field_name(key),
                    serde_json::Value::Number(v.into()),
                )
            }
            SetRule::U32 => {
                let v: u32 = match value.parse() {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                self.set_field_json(
                    Self::resolve_field_name(key),
                    serde_json::Value::Number(v.into()),
                )
            }
            SetRule::F32 { min, max } => {
                let v: f32 = match value.parse() {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                if v < *min || v > *max {
                    return false;
                }
                match serde_json::Number::from_f64(v as f64) {
                    Some(n) => self.set_field_json(
                        Self::resolve_field_name(key),
                        serde_json::Value::Number(n),
                    ),
                    None => false,
                }
            }
            SetRule::F64 => {
                let v: f64 = match value.parse() {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                match serde_json::Number::from_f64(v) {
                    Some(n) => self.set_field_json(
                        Self::resolve_field_name(key),
                        serde_json::Value::Number(n),
                    ),
                    None => false,
                }
            }
            SetRule::Enum(allowed) => {
                if !allowed.contains(&value) {
                    return false;
                }
                self.set_field_json(
                    Self::resolve_field_name(key),
                    serde_json::Value::String(value.to_string()),
                )
            }
            SetRule::Custom(f) => f(self, value),
        }
    }

    /// List all config keys and values.
    pub fn list(&self) -> Vec<(String, String)> {
        Self::CONFIG_KEYS
            .iter()
            .map(|&key| {
                let display = self.get(key).unwrap_or_else(|| {
                    match key {
                        "impulse_agent_provider" | "impulse_agent_harness"
                        | "impulse_agent_api_key" | "conflict_webhook_url" => {
                            "(not set)".to_string()
                        }
                        "impulse_agent_model" => "(default)".to_string(),
                        _ => String::new(),
                    }
                });
                (key.to_string(), display)
            })
            .collect()
    }

    /// Apply a single field change via serde round-trip.
    /// Preserves `impulse_agent_api_key` (which has skip_serializing) by
    /// saving and restoring it across the round-trip.
    fn set_field_json(&mut self, field: &str, val: serde_json::Value) -> bool {
        let saved_api_key = self.impulse_agent_api_key.take();
        let mut obj = match serde_json::to_value(&*self) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => {
                self.impulse_agent_api_key = saved_api_key;
                return false;
            }
        };
        obj.insert(field.to_string(), val);
        match serde_json::from_value::<Config>(serde_json::Value::Object(obj)) {
            Ok(mut new_config) => {
                new_config.impulse_agent_api_key = saved_api_key;
                *self = new_config;
                true
            }
            Err(_) => {
                self.impulse_agent_api_key = saved_api_key;
                false
            }
        }
    }

    fn build_set_rules() -> HashMap<&'static str, SetRule> {
        let mut m = HashMap::new();

        // Bool fields (18)
        for &key in &[
            "verbose", "retrieval_vector_enabled", "retrieval_deduplicate_enabled",
            "retrieval_fuzzy_matching_enabled", "retrieval_experimental_pageindex_enabled",
            "context_injection_use_semantic", "context_injection_emit_artifacts",
            "stewardship_cross_project_enabled", "build_hygiene_enabled",
            "build_hygiene_sweep_on_session_end", "build_hygiene_sweep_on_toolchain_update",
            "build_hygiene_dry_run_default", "context_lifecycle_enabled",
            "impulse_agent_auto_review", "impulse_agent_auto_coordinate",
            "notifications_enabled", "conflict_webhook_enabled",
        ] {
            m.insert(key, SetRule::Bool);
        }

        // Enum fields (8)
        m.insert("log_level", SetRule::Enum(&["trace", "debug", "info", "warn", "error"]));
        m.insert("retrieval_mode", SetRule::Enum(&["keyword", "semantic"]));
        m.insert("retrieval_backend", SetRule::Enum(&["fts", "fts+vec"]));
        m.insert("retrieval_semantic_strategy", SetRule::Enum(&["auto", "sqlite-only", "rust-only"]));
        m.insert("retrieval_pageindex_mode", SetRule::Enum(&["local-structure", "api-augmented"]));
        m.insert("context_injection_mode", SetRule::Enum(&["off", "review", "apply"]));
        m.insert("context_injection_scope", SetRule::Enum(&["daemon", "direct", "both"]));
        m.insert("stewardship_mode", SetRule::Enum(&["auto", "review", "off"]));

        // U64 fields with ranges (7)
        m.insert("sync_interval_secs", SetRule::U64 { min: 0, max: u64::MAX });
        m.insert("retrieval_query_timeout_secs", SetRule::U64 { min: 1, max: 120 });
        m.insert("retrieval_index_timeout_secs", SetRule::U64 { min: 10, max: 600 });
        m.insert("stewardship_poll_interval_secs", SetRule::U64 { min: 1, max: 300 });
        m.insert("context_lifecycle_poll_secs", SetRule::U64 { min: 1, max: 60 });
        m.insert("context_lifecycle_startup_delay_ms", SetRule::U64 { min: 100, max: 30_000 });
        m.insert("tool_execution.default_timeout_ms", SetRule::U64 { min: 100, max: 300_000 });

        // Usize fields with ranges (10)
        m.insert("max_history_entries", SetRule::Usize { min: 0, max: usize::MAX });
        m.insert("retrieval_default_limit", SetRule::Usize { min: 1, max: usize::MAX });
        m.insert("retrieval_batch_size", SetRule::Usize { min: 1, max: 512 });
        m.insert("retrieval_candidate_pool", SetRule::Usize { min: 10, max: 5000 });
        m.insert("context_injection_max_items", SetRule::Usize { min: 1, max: 50 });
        m.insert("context_injection_max_chars", SetRule::Usize { min: 200, max: 20000 });
        m.insert("stewardship_context_window_tokens", SetRule::Usize { min: 10_000, max: 2_000_000 });
        m.insert("context_lifecycle_window_tokens", SetRule::Usize { min: 10_000, max: 2_000_000 });
        m.insert("tool_execution.max_output_bytes", SetRule::Usize { min: 256, max: 5_000_000 });
        m.insert("tool_execution.max_artifacts", SetRule::Usize { min: 1, max: 128 });

        // F32 fields with 0..1 range (6)
        for &key in &[
            "retrieval_similarity_threshold", "context_injection_min_score",
            "stewardship_monitor_threshold", "stewardship_surgical_threshold",
            "stewardship_thoughtful_threshold", "stewardship_emergency_threshold",
        ] {
            m.insert(key, SetRule::F32 { min: 0.0, max: 1.0 });
        }

        // F64 / U32 fields (2)
        m.insert("build_hygiene_size_threshold_gb", SetRule::F64);
        m.insert("build_hygiene_age_threshold_days", SetRule::U32);

        // String fields (non-empty required) (4)
        for &key in &[
            "retrieval_embedding_provider", "embedding_model",
            "retrieval_python_cmd", "external_tools_dir",
        ] {
            m.insert(key, SetRule::String);
        }

        // Optional string fields (empty/"none" clears) (3)
        m.insert("impulse_agent_model", SetRule::OptionalString);
        m.insert("conflict_webhook_url", SetRule::OptionalString);

        // Some-string fields (always wraps in Some) (4)
        for &key in &["model.anthropic", "model.openai", "model.google", "model.mistral"] {
            m.insert(key, SetRule::SomeString);
        }

        // CSV list fields (5)
        for &key in &[
            "build_hygiene_scan_paths", "tool_execution.allowed_read_roots",
            "tool_execution.allowed_write_roots", "external_mcp_servers",
        ] {
            m.insert(key, SetRule::CsvList);
        }

        // Custom fields (6)
        m.insert("default_platform", SetRule::Custom(|c, v| {
            match v {
                "claude-code" => c.default_platform = Some(Platform::ClaudeCode),
                "opencode" => c.default_platform = Some(Platform::OpenCode),
                "none" => c.default_platform = None,
                _ => return false,
            }
            true
        }));
        m.insert("impulse_agent_api_key", SetRule::Custom(|c, v| {
            c.impulse_agent_api_key = if v.is_empty() { None } else { Some(v.to_string()) };
            true
        }));
        m.insert("impulse_agent_provider", SetRule::Custom(|c, v| {
            if v.is_empty() || v == "none" {
                c.impulse_agent_provider = None;
            } else if crate::agent::ImpulseProvider::parse(v).is_some() {
                c.impulse_agent_provider = Some(v.to_string());
            } else {
                return false;
            }
            true
        }));
        m.insert("impulse_agent_harness", SetRule::Custom(|c, v| {
            if v.is_empty() || v == "none" {
                c.impulse_agent_harness = None;
            } else if crate::agent::ImpulseHarness::parse(v).is_some() {
                c.impulse_agent_harness = Some(v.to_string());
            } else {
                return false;
            }
            true
        }));
        m.insert("impulse_agent_permissions", SetRule::Custom(|c, v| {
            match serde_json::from_str::<impulse_ops::SupervisorPermissionPolicy>(v) {
                Ok(mut policy) => {
                    policy.normalize();
                    c.impulse_agent_permissions = policy;
                    true
                }
                Err(_) => false,
            }
        }));
        m.insert("guardrails_enabled", SetRule::Custom(|c, v| {
            match v.parse::<bool>() {
                Ok(b) => { c.guardrails.enabled = b; true }
                Err(_) => false,
            }
        }));

        m
    }
}

/// Validation rules for `Config::set()`. Each variant describes how to parse
/// and validate a string value before applying it to the config.
enum SetRule {
    /// Parse as bool; reject non-boolean strings.
    Bool,
    /// Non-empty string required.
    String,
    /// Empty or "none" → null/None, otherwise set as string.
    OptionalString,
    /// Always wrap in Some(string) — no clearing.
    SomeString,
    /// Split on commas, trim, filter empty.
    CsvList,
    /// Parse u64 with inclusive range.
    U64 { min: u64, max: u64 },
    /// Parse usize with inclusive range.
    Usize { min: usize, max: usize },
    /// Parse u32 (any valid value).
    U32,
    /// Parse f32 with inclusive range.
    F32 { min: f32, max: f32 },
    /// Parse f64 (any valid finite value).
    F64,
    /// Value must be one of the allowed strings.
    Enum(&'static [&'static str]),
    /// Custom validation + setter.
    Custom(fn(&mut Config, &str) -> bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub platform: Option<Platform>,
    pub working_directory: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub active_files: Vec<String>,
    pub recent_tools: Vec<String>,
    pub metadata: HashMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>, // New: session tags for organization
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    ClaudeCode,
    OpenCode,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::ClaudeCode => "claude-code",
            Platform::OpenCode => "opencode",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "claude-code" => Some(Self::ClaudeCode),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SessionStatus {
    #[default]
    Active,
    Idle,
    Waiting,
    Completed,
    Error,
}

impl Session {
    pub fn new(name: String, platform: Option<Platform>) -> Self {
        let now = Utc::now();
        let working_dir = get_working_dir_name();
        Self {
            id: format!(
                "{}-{}-{}",
                sanitize_filename(&working_dir),
                now.format("%Y%m%d-%H%M%S"),
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            name,
            platform,
            working_directory: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            status: SessionStatus::Active,
            created_at: now,
            last_activity: now,
            active_files: Vec::new(),
            recent_tools: Vec::new(),
            metadata: HashMap::new(),
            tags: Vec::new(),
        }
    }

    pub fn add_tag(&mut self, tag: &str) {
        if !self.tags.contains(&tag.to_string()) {
            self.tags.push(tag.to_string());
        }
        self.last_activity = Utc::now();
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
        self.last_activity = Utc::now();
    }

    pub fn add_file(&mut self, path: &str) {
        if !self.active_files.contains(&path.to_string()) {
            self.active_files.push(path.to_string());
        }
        self.last_activity = Utc::now();
    }

    pub fn add_tool(&mut self, tool: &str) {
        self.recent_tools.retain(|t| t != tool);
        self.recent_tools.insert(0, tool.to_string());
        if self.recent_tools.len() > 20 {
            self.recent_tools.truncate(20);
        }
        self.last_activity = Utc::now();
    }

    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
        self.last_activity = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveState {
    pub sessions: HashMap<String, Session>,
    pub last_updated: DateTime<Utc>,
}

impl Default for LiveState {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveState {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            last_updated: Utc::now(),
        }
    }

    pub fn add_session(&mut self, session: Session) {
        self.sessions.insert(session.id.clone(), session);
        self.last_updated = Utc::now();
    }

    pub fn get_session(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn remove_session(&mut self, id: &str) -> Option<Session> {
        let removed = self.sessions.remove(id);
        if removed.is_some() {
            self.last_updated = Utc::now();
        }
        removed
    }

    pub fn list_sessions(&self) -> Vec<&Session> {
        self.sessions.values().collect()
    }

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub session_id: String,
    pub session_name: String,
    pub platform: Option<Platform>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub summary: String,
    pub files_touched: Vec<String>,
    pub tools_used: Vec<String>,
}

pub struct State {
    storage: Storage,
    live_state: RwLock<LiveState>,
    dirty: RwLock<bool>,
    config: RwLock<Config>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("storage", &self.storage.base_path())
            .finish()
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Ok(dirty) = self.dirty.try_read() {
            if *dirty {
                if let Ok(state) = self.live_state.try_read() {
                    if let Err(err) = self.storage.write_json(LIVE_STATE_FILE, &*state) {
                        tracing::error!("failed to persist live state on drop: {}", err);
                    }
                }
            }
        }
    }
}

impl State {
    pub fn new(base_path: std::path::PathBuf) -> Result<Self> {
        let storage = Storage::new(base_path);
        let live_state = storage.read_json::<LiveState>(LIVE_STATE_FILE)?;
        let config = storage.read_json::<Config>(CONFIG_FILE)?;

        Ok(Self {
            storage,
            live_state: RwLock::new(live_state),
            dirty: RwLock::new(false),
            config: RwLock::new(config),
        })
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    fn mark_dirty(&self) {
        if let Ok(mut dirty) = self.dirty.try_write() {
            *dirty = true;
        }
    }

    pub async fn sync_immediate(&self) -> Result<()> {
        let state = self.live_state.try_read().map(|s| s.clone())?;
        self.storage.write_json(LIVE_STATE_FILE, &state)?;

        if let Ok(mut dirty) = self.dirty.try_write() {
            *dirty = false;
        }

        Ok(())
    }

    pub async fn create_session(
        &self,
        name: String,
        platform: Option<Platform>,
    ) -> Result<Session> {
        let session = Session::new(name, platform);

        {
            let mut state = self
                .live_state
                .try_write()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            state.add_session(session.clone());
        }

        self.mark_dirty();
        self.sync_immediate().await?;

        Ok(session)
    }

    pub async fn end_session(
        &self,
        session_id: &str,
        summary: String,
    ) -> Result<Option<HistoryEntry>> {
        let history_entry = {
            let mut state = self
                .live_state
                .try_write()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

            if let Some(session) = state.get_session_mut(session_id) {
                session.set_status(SessionStatus::Completed);

                let entry = HistoryEntry {
                    session_id: session.id.clone(),
                    session_name: session.name.clone(),
                    platform: session.platform,
                    started_at: session.created_at,
                    ended_at: Utc::now(),
                    summary,
                    files_touched: session.active_files.clone(),
                    tools_used: session.recent_tools.clone(),
                };

                state.remove_session(session_id);
                Some(entry)
            } else {
                None
            }
        };

        if let Some(ref entry) = history_entry {
            self.storage.append_jsonl(HISTORY_FILE, entry)?;
        }

        self.mark_dirty();
        self.sync_immediate().await?;

        Ok(history_entry)
    }

    pub async fn track_file(&self, session_id: &str, file_path: &str) -> Result<()> {
        self.with_session(session_id, |s| s.add_file(file_path))
            .await
    }

    pub async fn track_tool(&self, session_id: &str, tool_name: &str) -> Result<()> {
        self.with_session(session_id, |s| s.add_tool(tool_name))
            .await
    }

    pub async fn check_file_conflict(
        &self,
        session_id: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let state = self
            .live_state
            .try_read()
            .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on live state"))?;

        let mut conflicting = Vec::new();
        let normalized_path = std::path::Path::new(file_path)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(file_path))
            .to_string_lossy()
            .to_string();

        for session in state.sessions.values() {
            if session.id == session_id {
                continue;
            }
            for active_file in &session.active_files {
                let active_normalized = std::path::Path::new(active_file)
                    .canonicalize()
                    .unwrap_or_else(|_| std::path::PathBuf::from(active_file))
                    .to_string_lossy()
                    .to_string();
                if active_normalized == normalized_path {
                    conflicting.push(session.name.clone());
                }
            }
        }
        Ok(conflicting)
    }

    pub async fn add_tag(&self, session_id: &str, tag: &str) -> Result<()> {
        self.with_session(session_id, |s| s.add_tag(tag)).await
    }

    pub async fn remove_tag(&self, session_id: &str, tag: &str) -> Result<()> {
        self.with_session(session_id, |s| s.remove_tag(tag)).await
    }

    /// Helper for common session mutation pattern
    async fn with_session<F>(&self, session_id: &str, mut f: F) -> Result<()>
    where
        F: FnMut(&mut Session),
    {
        let mut updated = false;
        {
            let mut state = self
                .live_state
                .try_write()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            if let Some(session) = state.get_session_mut(session_id) {
                f(session);
                updated = true;
            }
        }
        if !updated {
            anyhow::bail!("Session not found: {}", session_id);
        }
        self.mark_dirty();
        self.sync_immediate().await?;
        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let state = self
            .live_state
            .try_read()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(state.get_session(session_id).cloned())
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let state = self
            .live_state
            .try_read()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(state.list_sessions().into_iter().cloned().collect())
    }


    pub fn get_history_sync(&self) -> Result<Vec<HistoryEntry>> {
        let entries = self.storage.read_jsonl::<HistoryEntry>(HISTORY_FILE)?;
        Ok(entries)
    }

    /// Get a config value by key
    pub fn get_config(&self, key: &str) -> Result<Option<String>> {
        let config = self
            .config
            .try_read()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(config.get(key))
    }

    /// Set a config value by key
    pub fn set_config(&self, key: &str, value: &str) -> Result<bool> {
        let mut config = self
            .config
            .try_write()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let result = config.set(key, value);
        if result {
            self.storage.write_json(CONFIG_FILE, &*config)?;
        }
        Ok(result)
    }

    /// List all config values
    pub fn list_config(&self) -> Result<Vec<(String, String)>> {
        let config = self
            .config
            .try_read()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(config.list())
    }

    pub fn config_snapshot(&self) -> Result<Config> {
        let config = self
            .config
            .try_read()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(config.clone())
    }

    /// Update guardrail rules in config and persist to disk
    pub fn update_guardrail_rules(&self, rules: Vec<crate::guardrail::GuardRule>) -> Result<()> {
        let mut config = self
            .config
            .try_write()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        config.guardrails.rules = rules;
        self.storage.write_json(CONFIG_FILE, &*config)?;
        Ok(())
    }

    pub fn update_impulse_agent_permissions(
        &self,
        policy: impulse_ops::SupervisorPermissionPolicy,
    ) -> Result<()> {
        let mut config = self
            .config
            .try_write()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut normalized = policy;
        normalized.normalize();
        config.impulse_agent_permissions = normalized;
        self.storage.write_json(CONFIG_FILE, &*config)?;
        Ok(())
    }

    pub fn get_conflict_analytics(&self) -> Result<ConflictHistory> {
        let history: ConflictHistory = self.storage.read_json("CONFLICTS.json").unwrap_or_default();
        Ok(history)
    }

    pub fn record_conflict(&self, file_path: &str, sessions: Vec<String>) -> Result<()> {
        let mut history: ConflictHistory =
            self.storage.read_json("CONFLICTS.json").unwrap_or_default();
        history.record_conflict(file_path, sessions);
        self.storage.write_json("CONFLICTS.json", &history)?;
        Ok(())
    }

    pub fn record_conflict_resolution(&self, file_path: &str, resolution: &str) -> Result<()> {
        let mut history: ConflictHistory =
            self.storage.read_json("CONFLICTS.json").unwrap_or_default();
        history.record_resolution(file_path, resolution);
        self.storage.write_json("CONFLICTS.json", &history)?;
        Ok(())
    }
}

pub type SharedState = Arc<State>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConflictHistory {
    #[serde(default)]
    pub conflict_history: Vec<ConflictEntry>,
}

impl ConflictHistory {
    pub fn new() -> Self {
        Self {
            conflict_history: Vec::new(),
        }
    }

    pub fn record_conflict(&mut self, file_path: &str, detected_sessions: Vec<String>) {
        if let Some(entry) = self
            .conflict_history
            .iter_mut()
            .find(|e| e.file_path == file_path)
        {
            entry.detection_count += 1;
            entry.last_detected = Utc::now();
            entry.involved_sessions = detected_sessions;
        } else {
            self.conflict_history.push(ConflictEntry {
                file_path: file_path.to_string(),
                detection_count: 1,
                first_detected: Utc::now(),
                last_detected: Utc::now(),
                involved_sessions: detected_sessions,
                resolution: None,
                resolved_at: None,
            });
        }
    }

    pub fn record_resolution(&mut self, file_path: &str, resolution: &str) {
        if let Some(entry) = self
            .conflict_history
            .iter_mut()
            .find(|e| e.file_path == file_path)
        {
            entry.resolution = Some(resolution.to_string());
            entry.resolved_at = Some(Utc::now());
        }
    }

    pub fn get_conflict_history(&self) -> &[ConflictEntry] {
        &self.conflict_history
    }

    pub fn get_analytics(&self) -> ConflictAnalytics {
        ConflictAnalytics::from_history(&self.conflict_history)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEntry {
    pub file_path: String,
    pub detection_count: usize,
    pub first_detected: DateTime<Utc>,
    pub last_detected: DateTime<Utc>,
    pub involved_sessions: Vec<String>,
    pub resolution: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConflictAnalytics {
    pub total_conflicts: usize,
    pub resolved_count: usize,
    pub unresolved_count: usize,
    pub resolution_rate: f64,
    pub conflicts_by_day: HashMap<String, usize>,
    pub conflicts_by_week: HashMap<String, usize>,
    pub conflicts_by_month: HashMap<String, usize>,
    pub most_common_files: Vec<(String, usize)>,
    pub resolution_methods: HashMap<String, usize>,
    pub avg_time_to_resolution_secs: Option<i64>,
}

impl ConflictAnalytics {
    pub fn from_history(entries: &[ConflictEntry]) -> Self {
        let total_conflicts = entries.len();
        let resolved_count = entries.iter().filter(|e| e.resolution.is_some()).count();
        let unresolved_count = total_conflicts - resolved_count;
        let resolution_rate = if total_conflicts > 0 {
            (resolved_count as f64 / total_conflicts as f64) * 100.0
        } else {
            0.0
        };

        let mut conflicts_by_day: HashMap<String, usize> = HashMap::new();
        let mut conflicts_by_week: HashMap<String, usize> = HashMap::new();
        let mut conflicts_by_month: HashMap<String, usize> = HashMap::new();
        let mut file_counts: HashMap<String, usize> = HashMap::new();
        let mut resolution_methods: HashMap<String, usize> = HashMap::new();
        let mut total_resolution_time = 0i64;
        let mut resolved_with_time = 0usize;

        for entry in entries {
            let day = entry.first_detected.format("%Y-%m-%d").to_string();
            let week = entry.first_detected.format("%Y-W%U").to_string();
            let month = entry.first_detected.format("%Y-%m").to_string();

            *conflicts_by_day.entry(day).or_insert(0) += 1;
            *conflicts_by_week.entry(week).or_insert(0) += 1;
            *conflicts_by_month.entry(month).or_insert(0) += 1;
            *file_counts.entry(entry.file_path.clone()).or_insert(0) += 1;

            if let Some(ref resolution) = entry.resolution {
                *resolution_methods.entry(resolution.clone()).or_insert(0) += 1;

                if let Some(resolved_at) = entry.resolved_at {
                    let duration = (resolved_at - entry.first_detected).num_seconds();
                    total_resolution_time += duration;
                    resolved_with_time += 1;
                }
            }
        }

        let mut most_common_files: Vec<_> = file_counts.into_iter().collect();
        most_common_files.sort_by(|a, b| b.1.cmp(&a.1));

        let avg_time_to_resolution_secs = if resolved_with_time > 0 {
            Some(total_resolution_time / resolved_with_time as i64)
        } else {
            None
        };

        Self {
            total_conflicts,
            resolved_count,
            unresolved_count,
            resolution_rate,
            conflicts_by_day,
            conflicts_by_week,
            conflicts_by_month,
            most_common_files,
            resolution_methods,
            avg_time_to_resolution_secs,
        }
    }

    pub fn format_time_to_resolution(&self) -> String {
        if let Some(secs) = self.avg_time_to_resolution_secs {
            if secs < 60 {
                format!("{}s", secs)
            } else if secs < 3600 {
                format!("{}m", secs / 60)
            } else {
                let hours = secs / 3600;
                let mins = (secs % 3600) / 60;
                format!("{}h {}m", hours, mins)
            }
        } else {
            "N/A".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.log_level, "info");
        assert_eq!(config.default_platform, None);
        assert!(!config.verbose);
        assert_eq!(config.sync_interval_secs, 30);
        assert_eq!(config.max_history_entries, 1000);
        assert_eq!(config.retrieval_mode, "keyword");
        assert_eq!(config.retrieval_backend, "fts");
        assert_eq!(config.retrieval_default_limit, 10);
        assert_eq!(config.retrieval_similarity_threshold, 0.75);
        assert_eq!(config.retrieval_embedding_provider, "python-st");
        assert_eq!(config.retrieval_python_cmd, "python3");
        assert!(!config.retrieval_vector_enabled);
        assert_eq!(config.retrieval_semantic_strategy, "auto");
        assert_eq!(config.retrieval_query_timeout_secs, 10);
        assert_eq!(config.retrieval_index_timeout_secs, 60);
        assert_eq!(config.retrieval_batch_size, 64);
        assert_eq!(config.retrieval_candidate_pool, 200);
        assert!(!config.retrieval_experimental_pageindex_enabled);
        assert_eq!(config.retrieval_pageindex_mode, "local-structure");
        assert_eq!(config.context_injection_mode, "review");
        assert_eq!(config.context_injection_scope, "both");
        assert_eq!(config.context_injection_max_items, 5);
        assert_eq!(config.context_injection_max_chars, 2000);
        assert_eq!(config.context_injection_min_score, 0.60);
        assert!(config.context_injection_use_semantic);
        assert!(config.context_injection_emit_artifacts);
    }

    #[test]
    fn test_config_get() {
        let config = Config::default();
        assert_eq!(config.get("log_level"), Some("info".to_string()));
        assert_eq!(config.get("verbose"), Some("false".to_string()));
        assert_eq!(config.get("nonexistent"), None);
    }

    #[test]
    fn test_config_set() {
        let mut config = Config::default();

        assert!(config.set("log_level", "debug"));
        assert_eq!(config.log_level, "debug");

        assert!(!config.set("log_level", "invalid"));

        assert!(config.set("verbose", "true"));
        assert!(config.verbose);

        assert!(config.set("default_platform", "claude-code"));
        assert_eq!(config.default_platform, Some(Platform::ClaudeCode));

        assert!(config.set("default_platform", "none"));
        assert_eq!(config.default_platform, None);

        assert!(config.set("sync_interval_secs", "60"));
        assert_eq!(config.sync_interval_secs, 60);

        assert!(config.set("retrieval_mode", "semantic"));
        assert_eq!(config.retrieval_mode, "semantic");
        assert!(!config.set("retrieval_mode", "bad"));

        assert!(config.set("retrieval_backend", "fts+vec"));
        assert_eq!(config.retrieval_backend, "fts+vec");
        assert!(!config.set("retrieval_backend", "db"));

        assert!(config.set("retrieval_default_limit", "25"));
        assert_eq!(config.retrieval_default_limit, 25);

        assert!(config.set("retrieval_similarity_threshold", "0.9"));
        assert_eq!(config.retrieval_similarity_threshold, 0.9);
        assert!(!config.set("retrieval_similarity_threshold", "1.1"));

        assert!(config.set("retrieval_embedding_provider", "python-st"));
        assert!(config.set("retrieval_python_cmd", "python3"));
        assert!(config.set("retrieval_vector_enabled", "true"));
        assert!(config.retrieval_vector_enabled);
        assert!(!config.set("retrieval_vector_enabled", "enabled"));

        assert!(config.set("retrieval_semantic_strategy", "rust-only"));
        assert_eq!(config.retrieval_semantic_strategy, "rust-only");
        assert!(!config.set("retrieval_semantic_strategy", "none"));

        assert!(config.set("retrieval_query_timeout_secs", "15"));
        assert_eq!(config.retrieval_query_timeout_secs, 15);
        assert!(!config.set("retrieval_query_timeout_secs", "0"));

        assert!(config.set("retrieval_index_timeout_secs", "90"));
        assert_eq!(config.retrieval_index_timeout_secs, 90);
        assert!(!config.set("retrieval_index_timeout_secs", "5"));

        assert!(config.set("retrieval_batch_size", "32"));
        assert_eq!(config.retrieval_batch_size, 32);
        assert!(!config.set("retrieval_batch_size", "9999"));

        assert!(config.set("retrieval_candidate_pool", "350"));
        assert_eq!(config.retrieval_candidate_pool, 350);
        assert!(!config.set("retrieval_candidate_pool", "2"));

        assert!(config.set("retrieval_experimental_pageindex_enabled", "true"));
        assert!(config.retrieval_experimental_pageindex_enabled);
        assert!(!config.set("retrieval_experimental_pageindex_enabled", "enabled"));

        assert!(config.set("retrieval_pageindex_mode", "api-augmented"));
        assert_eq!(config.retrieval_pageindex_mode, "api-augmented");
        assert!(!config.set("retrieval_pageindex_mode", "hybrid"));

        assert!(config.set("context_injection_mode", "apply"));
        assert_eq!(config.context_injection_mode, "apply");
        assert!(!config.set("context_injection_mode", "auto"));

        assert!(config.set("context_injection_scope", "direct"));
        assert_eq!(config.context_injection_scope, "direct");
        assert!(!config.set("context_injection_scope", "none"));

        assert!(config.set("context_injection_max_items", "8"));
        assert_eq!(config.context_injection_max_items, 8);
        assert!(!config.set("context_injection_max_items", "0"));

        assert!(config.set("context_injection_max_chars", "4096"));
        assert_eq!(config.context_injection_max_chars, 4096);
        assert!(!config.set("context_injection_max_chars", "128"));

        assert!(config.set("context_injection_min_score", "0.7"));
        assert_eq!(config.context_injection_min_score, 0.7);
        assert!(!config.set("context_injection_min_score", "1.5"));

        assert!(config.set("context_injection_use_semantic", "false"));
        assert!(!config.context_injection_use_semantic);
        assert!(!config.set("context_injection_use_semantic", "maybe"));

        assert!(config.set("context_injection_emit_artifacts", "false"));
        assert!(!config.context_injection_emit_artifacts);
        assert!(!config.set("context_injection_emit_artifacts", "enabled"));
    }

    #[test]
    fn test_config_list() {
        let config = Config::default();
        let items = config.list();

        assert!(items.len() >= 45); // May increase as modules are added
        assert!(items.iter().any(|(k, _)| k == "retrieval_mode"));
        assert!(items.iter().any(|(k, _)| k == "stewardship_mode"));
        assert!(items.iter().any(|(k, _)| k == "build_hygiene_enabled"));
        assert!(items.iter().any(|(k, _)| k == "build_hygiene_scan_paths"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_backend"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_default_limit"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_similarity_threshold"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_embedding_provider"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_python_cmd"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_vector_enabled"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_semantic_strategy"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_query_timeout_secs"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_index_timeout_secs"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_batch_size"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_candidate_pool"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_experimental_pageindex_enabled"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_pageindex_mode"));
        assert!(items.iter().any(|(k, _)| k == "context_injection_mode"));
        assert!(items.iter().any(|(k, _)| k == "context_injection_scope"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_max_items"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_max_chars"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_min_score"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_use_semantic"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_emit_artifacts"));
    }

    #[test]
    fn test_session_new() {
        let session = Session::new("test-session".to_string(), Some(Platform::ClaudeCode));

        assert!(!session.id.is_empty());
        assert_eq!(session.name, "test-session");
        assert_eq!(session.platform, Some(Platform::ClaudeCode));
        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.active_files.is_empty());
        assert!(session.recent_tools.is_empty());
    }

    #[test]
    fn test_session_add_file() {
        let mut session = Session::new("test".to_string(), None);

        session.add_file("src/main.rs");
        assert!(session.active_files.contains(&"src/main.rs".to_string()));

        session.add_file("src/main.rs");
        assert_eq!(session.active_files.len(), 1);
    }

    #[test]
    fn test_session_add_tool() {
        let mut session = Session::new("test".to_string(), None);

        session.add_tool("Write");
        session.add_tool("Read");
        session.add_tool("Edit");

        assert_eq!(session.recent_tools.len(), 3);
        assert_eq!(session.recent_tools[0], "Edit");

        session.add_tool("Write");
        assert_eq!(session.recent_tools.len(), 3);
        assert_eq!(session.recent_tools[0], "Write");
    }

    #[test]
    fn test_session_set_status() {
        let mut session = Session::new("test".to_string(), None);

        session.set_status(SessionStatus::Idle);
        assert_eq!(session.status, SessionStatus::Idle);

        session.set_status(SessionStatus::Completed);
        assert_eq!(session.status, SessionStatus::Completed);
    }

    #[test]
    fn test_live_state_new() {
        let state = LiveState::new();
        assert!(state.sessions.is_empty());
    }

    #[test]
    fn test_live_state_add_session() {
        let mut state = LiveState::new();
        let session = Session::new("test".to_string(), None);

        state.add_session(session.clone());

        assert_eq!(state.sessions.len(), 1);
        assert!(state.get_session(&session.id).is_some());
    }

    #[test]
    fn test_live_state_remove_session() {
        let mut state = LiveState::new();
        let session = Session::new("test".to_string(), None);
        let id = session.id.clone();

        state.add_session(session);
        let removed = state.remove_session(&id);

        assert!(removed.is_some());
        assert!(state.get_session(&id).is_none());
    }

    #[test]
    fn test_live_state_list_sessions() {
        let mut state = LiveState::new();

        state.add_session(Session::new("session1".to_string(), None));
        state.add_session(Session::new("session2".to_string(), None));

        let sessions = state.list_sessions();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let storage = crate::storage::Storage::new(temp_dir.path().to_path_buf());

        let config = Config::default();
        storage.write_json("config.json", &config).unwrap();

        let loaded: Config = storage.read_json("config.json").unwrap();
        assert_eq!(loaded.log_level, "info");
    }

    #[tokio::test]
    async fn test_check_file_conflict_same_file() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();
        let session2 = state
            .create_session("session2".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session1.id, "src/main.rs").await.unwrap();
        state.track_file(&session2.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session1.id, "src/main.rs")
            .await
            .unwrap();
        assert!(!conflicting.is_empty());
        assert!(conflicting.contains(&"session2".to_string()));
    }

    #[tokio::test]
    async fn test_check_file_conflict_different_files() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();
        let _session2 = state
            .create_session("session2".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session1.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session1.id, "src/lib.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());
    }

    #[tokio::test]
    async fn test_check_file_conflict_no_other_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session1.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session1.id, "src/main.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());
    }

    #[tokio::test]
    async fn test_check_file_conflict_self_excluded() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session.id, "src/main.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());
    }

    #[tokio::test]
    async fn test_track_file_missing_session_errors() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let err = state
            .track_file("missing-session", "src/main.rs")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Session not found"));
    }

    #[test]
    fn test_state_new_surfaces_config_corruption() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path()).unwrap();
        std::fs::write(temp_dir.path().join("config.json"), "{not-json").unwrap();

        let err = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap_err();
        assert!(err.to_string().contains("Failed to parse JSON"));
    }

    #[test]
    fn test_conflict_history_new() {
        let history = ConflictHistory::new();
        assert!(history.conflict_history.is_empty());
    }

    #[test]
    fn test_conflict_history_record_conflict() {
        let mut history = ConflictHistory::new();
        history.record_conflict(
            "src/main.rs",
            vec!["session1".to_string(), "session2".to_string()],
        );

        assert_eq!(history.conflict_history.len(), 1);
        assert_eq!(history.conflict_history[0].file_path, "src/main.rs");
        assert_eq!(history.conflict_history[0].detection_count, 1);
        assert_eq!(history.conflict_history[0].involved_sessions.len(), 2);
    }

    #[test]
    fn test_conflict_history_record_conflict_increments_count() {
        let mut history = ConflictHistory::new();
        history.record_conflict("src/main.rs", vec!["session1".to_string()]);
        history.record_conflict("src/main.rs", vec!["session2".to_string()]);

        assert_eq!(history.conflict_history.len(), 1);
        assert_eq!(history.conflict_history[0].detection_count, 2);
    }

    #[test]
    fn test_conflict_history_record_resolution() {
        let mut history = ConflictHistory::new();
        history.record_conflict("src/main.rs", vec!["session1".to_string()]);
        history.record_resolution("src/main.rs", "merge");

        assert_eq!(
            history.conflict_history[0].resolution,
            Some("merge".to_string())
        );
        assert!(history.conflict_history[0].resolved_at.is_some());
    }

    #[test]
    fn test_conflict_analytics_from_history_empty() {
        let analytics = ConflictAnalytics::from_history(&[]);
        assert_eq!(analytics.total_conflicts, 0);
        assert_eq!(analytics.resolved_count, 0);
        assert_eq!(analytics.unresolved_count, 0);
        assert_eq!(analytics.resolution_rate, 0.0);
    }

    #[test]
    fn test_conflict_analytics_from_history_with_data() {
        use chrono::Duration;

        let mut entry1 = ConflictEntry {
            file_path: "src/main.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec!["session1".to_string()],
            resolution: Some("merge".to_string()),
            resolved_at: Some(chrono::Utc::now()),
        };
        entry1.resolved_at = Some(entry1.first_detected + Duration::seconds(60));

        let entry2 = ConflictEntry {
            file_path: "src/lib.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec!["session2".to_string()],
            resolution: None,
            resolved_at: None,
        };

        let analytics = ConflictAnalytics::from_history(&[entry1, entry2]);

        assert_eq!(analytics.total_conflicts, 2);
        assert_eq!(analytics.resolved_count, 1);
        assert_eq!(analytics.unresolved_count, 1);
        assert_eq!(analytics.resolution_rate, 50.0);
    }

    #[test]
    fn test_conflict_analytics_most_common_files() {
        let entry1 = ConflictEntry {
            file_path: "src/main.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec![],
            resolution: None,
            resolved_at: None,
        };
        let entry2 = ConflictEntry {
            file_path: "src/main.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec![],
            resolution: None,
            resolved_at: None,
        };
        let entry3 = ConflictEntry {
            file_path: "src/lib.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec![],
            resolution: None,
            resolved_at: None,
        };

        let analytics = ConflictAnalytics::from_history(&[entry1, entry2, entry3]);

        assert_eq!(analytics.most_common_files.len(), 2);
        assert_eq!(analytics.most_common_files[0].0, "src/main.rs");
        assert_eq!(analytics.most_common_files[0].1, 2);
    }

    #[test]
    fn test_conflict_analytics_format_time_to_resolution() {
        let analytics_empty = ConflictAnalytics::default();
        assert_eq!(analytics_empty.format_time_to_resolution(), "N/A");

        let analytics_with_time = ConflictAnalytics {
            avg_time_to_resolution_secs: Some(30),
            ..ConflictAnalytics::default()
        };
        assert_eq!(analytics_with_time.format_time_to_resolution(), "30s");

        let analytics_minutes = ConflictAnalytics {
            avg_time_to_resolution_secs: Some(120),
            ..ConflictAnalytics::default()
        };
        assert_eq!(analytics_minutes.format_time_to_resolution(), "2m");

        let analytics_hours = ConflictAnalytics {
            avg_time_to_resolution_secs: Some(3665),
            ..ConflictAnalytics::default()
        };
        assert_eq!(analytics_hours.format_time_to_resolution(), "1h 1m");
    }
}
