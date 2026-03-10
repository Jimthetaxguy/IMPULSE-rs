//! In-memory state with dirty-flag sync and Drop persistence.
//!
//! Core types: [`Config`] (runtime settings), [`State`] (session/file tracking),
//! [`LiveState`] (ephemeral session state). All wrapped in `Arc<RwLock<_>>`
//! for concurrent access. Syncs to `.impulse/` files only when dirty.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        "log_level",
        "default_platform",
        "verbose",
        "sync_interval_secs",
        "max_history_entries",
        "retrieval_mode",
        "retrieval_backend",
        "retrieval_default_limit",
        "retrieval_similarity_threshold",
        "retrieval_embedding_provider",
        "embedding_model",
        "retrieval_python_cmd",
        "retrieval_vector_enabled",
        "retrieval_semantic_strategy",
        "retrieval_query_timeout_secs",
        "retrieval_index_timeout_secs",
        "retrieval_batch_size",
        "retrieval_candidate_pool",
        "retrieval_deduplicate_enabled",
        "retrieval_fuzzy_matching_enabled",
        "retrieval_experimental_pageindex_enabled",
        "retrieval_pageindex_mode",
        "context_injection_mode",
        "context_injection_scope",
        "context_injection_max_items",
        "context_injection_max_chars",
        "context_injection_min_score",
        "context_injection_use_semantic",
        "context_injection_emit_artifacts",
        "stewardship_mode",
        "stewardship_monitor_threshold",
        "stewardship_surgical_threshold",
        "stewardship_thoughtful_threshold",
        "stewardship_emergency_threshold",
        "stewardship_poll_interval_secs",
        "stewardship_context_window_tokens",
        "stewardship_cross_project_enabled",
        "model.anthropic",
        "model.openai",
        "model.google",
        "model.mistral",
        "build_hygiene_enabled",
        "build_hygiene_scan_paths",
        "build_hygiene_size_threshold_gb",
        "build_hygiene_age_threshold_days",
        "build_hygiene_sweep_on_session_end",
        "build_hygiene_sweep_on_toolchain_update",
        "build_hygiene_dry_run_default",
        "context_lifecycle_enabled",
        "context_lifecycle_poll_secs",
        "context_lifecycle_startup_delay_ms",
        "context_lifecycle_window_tokens",
        "impulse_agent_provider",
        "impulse_agent_api_key",
        "impulse_agent_model",
        "impulse_agent_harness",
        "impulse_agent_auto_review",
        "impulse_agent_auto_coordinate",
        "notifications_enabled",
        "conflict_webhook_url",
        "conflict_webhook_enabled",
        "tool_execution.default_timeout_ms",
        "tool_execution.max_output_bytes",
        "tool_execution.max_artifacts",
        "tool_execution.allowed_read_roots",
        "tool_execution.allowed_write_roots",
        "external_tools_dir",
        "external_mcp_servers",
        "impulse_agent_permissions",
        "guardrails_enabled",
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
                return self
                    .impulse_agent_api_key
                    .as_ref()
                    .map(|_| "***".to_string());
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
                Ok(b) => {
                    self.set_field_json(Self::resolve_field_name(key), serde_json::Value::Bool(b))
                }
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
                let display = self.get(key).unwrap_or_else(|| match key {
                    "impulse_agent_provider"
                    | "impulse_agent_harness"
                    | "impulse_agent_api_key"
                    | "conflict_webhook_url" => "(not set)".to_string(),
                    "impulse_agent_model" => "(default)".to_string(),
                    _ => String::new(),
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
            "verbose",
            "retrieval_vector_enabled",
            "retrieval_deduplicate_enabled",
            "retrieval_fuzzy_matching_enabled",
            "retrieval_experimental_pageindex_enabled",
            "context_injection_use_semantic",
            "context_injection_emit_artifacts",
            "stewardship_cross_project_enabled",
            "build_hygiene_enabled",
            "build_hygiene_sweep_on_session_end",
            "build_hygiene_sweep_on_toolchain_update",
            "build_hygiene_dry_run_default",
            "context_lifecycle_enabled",
            "impulse_agent_auto_review",
            "impulse_agent_auto_coordinate",
            "notifications_enabled",
            "conflict_webhook_enabled",
        ] {
            m.insert(key, SetRule::Bool);
        }

        // Enum fields (8)
        m.insert(
            "log_level",
            SetRule::Enum(&["trace", "debug", "info", "warn", "error"]),
        );
        m.insert("retrieval_mode", SetRule::Enum(&["keyword", "semantic"]));
        m.insert("retrieval_backend", SetRule::Enum(&["fts", "fts+vec"]));
        m.insert(
            "retrieval_semantic_strategy",
            SetRule::Enum(&["auto", "sqlite-only", "rust-only"]),
        );
        m.insert(
            "retrieval_pageindex_mode",
            SetRule::Enum(&["local-structure", "api-augmented"]),
        );
        m.insert(
            "context_injection_mode",
            SetRule::Enum(&["off", "review", "apply"]),
        );
        m.insert(
            "context_injection_scope",
            SetRule::Enum(&["daemon", "direct", "both"]),
        );
        m.insert(
            "stewardship_mode",
            SetRule::Enum(&["auto", "review", "off"]),
        );

        // U64 fields with ranges (7)
        m.insert(
            "sync_interval_secs",
            SetRule::U64 {
                min: 0,
                max: u64::MAX,
            },
        );
        m.insert(
            "retrieval_query_timeout_secs",
            SetRule::U64 { min: 1, max: 120 },
        );
        m.insert(
            "retrieval_index_timeout_secs",
            SetRule::U64 { min: 10, max: 600 },
        );
        m.insert(
            "stewardship_poll_interval_secs",
            SetRule::U64 { min: 1, max: 300 },
        );
        m.insert(
            "context_lifecycle_poll_secs",
            SetRule::U64 { min: 1, max: 60 },
        );
        m.insert(
            "context_lifecycle_startup_delay_ms",
            SetRule::U64 {
                min: 100,
                max: 30_000,
            },
        );
        m.insert(
            "tool_execution.default_timeout_ms",
            SetRule::U64 {
                min: 100,
                max: 300_000,
            },
        );

        // Usize fields with ranges (10)
        m.insert(
            "max_history_entries",
            SetRule::Usize {
                min: 0,
                max: usize::MAX,
            },
        );
        m.insert(
            "retrieval_default_limit",
            SetRule::Usize {
                min: 1,
                max: usize::MAX,
            },
        );
        m.insert("retrieval_batch_size", SetRule::Usize { min: 1, max: 512 });
        m.insert(
            "retrieval_candidate_pool",
            SetRule::Usize { min: 10, max: 5000 },
        );
        m.insert(
            "context_injection_max_items",
            SetRule::Usize { min: 1, max: 50 },
        );
        m.insert(
            "context_injection_max_chars",
            SetRule::Usize {
                min: 200,
                max: 20000,
            },
        );
        m.insert(
            "stewardship_context_window_tokens",
            SetRule::Usize {
                min: 10_000,
                max: 2_000_000,
            },
        );
        m.insert(
            "context_lifecycle_window_tokens",
            SetRule::Usize {
                min: 10_000,
                max: 2_000_000,
            },
        );
        m.insert(
            "tool_execution.max_output_bytes",
            SetRule::Usize {
                min: 256,
                max: 5_000_000,
            },
        );
        m.insert(
            "tool_execution.max_artifacts",
            SetRule::Usize { min: 1, max: 128 },
        );

        // F32 fields with 0..1 range (6)
        for &key in &[
            "retrieval_similarity_threshold",
            "context_injection_min_score",
            "stewardship_monitor_threshold",
            "stewardship_surgical_threshold",
            "stewardship_thoughtful_threshold",
            "stewardship_emergency_threshold",
        ] {
            m.insert(key, SetRule::F32 { min: 0.0, max: 1.0 });
        }

        // F64 / U32 fields (2)
        m.insert("build_hygiene_size_threshold_gb", SetRule::F64);
        m.insert("build_hygiene_age_threshold_days", SetRule::U32);

        // String fields (non-empty required) (4)
        for &key in &[
            "retrieval_embedding_provider",
            "embedding_model",
            "retrieval_python_cmd",
            "external_tools_dir",
        ] {
            m.insert(key, SetRule::String);
        }

        // Optional string fields (empty/"none" clears) (3)
        m.insert("impulse_agent_model", SetRule::OptionalString);
        m.insert("conflict_webhook_url", SetRule::OptionalString);

        // Some-string fields (always wraps in Some) (4)
        for &key in &[
            "model.anthropic",
            "model.openai",
            "model.google",
            "model.mistral",
        ] {
            m.insert(key, SetRule::SomeString);
        }

        // CSV list fields (5)
        for &key in &[
            "build_hygiene_scan_paths",
            "tool_execution.allowed_read_roots",
            "tool_execution.allowed_write_roots",
            "external_mcp_servers",
        ] {
            m.insert(key, SetRule::CsvList);
        }

        // Custom fields (6)
        m.insert(
            "default_platform",
            SetRule::Custom(|c, v| {
                match v {
                    "claude-code" => c.default_platform = Some(Platform::ClaudeCode),
                    "opencode" => c.default_platform = Some(Platform::OpenCode),
                    "none" => c.default_platform = None,
                    _ => return false,
                }
                true
            }),
        );
        m.insert(
            "impulse_agent_api_key",
            SetRule::Custom(|c, v| {
                c.impulse_agent_api_key = if v.is_empty() {
                    None
                } else {
                    Some(v.to_string())
                };
                true
            }),
        );
        m.insert(
            "impulse_agent_provider",
            SetRule::Custom(|c, v| {
                if v.is_empty() || v == "none" {
                    c.impulse_agent_provider = None;
                } else if crate::agent::ImpulseProvider::parse(v).is_some() {
                    c.impulse_agent_provider = Some(v.to_string());
                } else {
                    return false;
                }
                true
            }),
        );
        m.insert(
            "impulse_agent_harness",
            SetRule::Custom(|c, v| {
                if v.is_empty() || v == "none" {
                    c.impulse_agent_harness = None;
                } else if crate::agent::ImpulseHarness::parse(v).is_some() {
                    c.impulse_agent_harness = Some(v.to_string());
                } else {
                    return false;
                }
                true
            }),
        );
        m.insert(
            "impulse_agent_permissions",
            SetRule::Custom(|c, v| {
                match serde_json::from_str::<impulse_ops::SupervisorPermissionPolicy>(v) {
                    Ok(mut policy) => {
                        policy.normalize();
                        c.impulse_agent_permissions = policy;
                        true
                    }
                    Err(_) => false,
                }
            }),
        );
        m.insert(
            "guardrails_enabled",
            SetRule::Custom(|c, v| match v.parse::<bool>() {
                Ok(b) => {
                    c.guardrails.enabled = b;
                    true
                }
                Err(_) => false,
            }),
        );

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
