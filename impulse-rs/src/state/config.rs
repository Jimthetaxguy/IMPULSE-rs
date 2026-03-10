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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default values ──────────────────────────────────────────────────

    #[test]
    fn default_config_has_expected_values() {
        let c = Config::default();
        assert_eq!(c.log_level, "info");
        assert!(c.default_platform.is_none());
        assert!(!c.verbose);
        assert_eq!(c.sync_interval_secs, 30);
        assert_eq!(c.max_history_entries, 1000);
        assert_eq!(c.retrieval_mode, "keyword");
        assert_eq!(c.retrieval_backend, "fts");
        assert_eq!(c.retrieval_default_limit, 10);
        assert!((c.retrieval_similarity_threshold - 0.75).abs() < f32::EPSILON);
        assert_eq!(c.context_injection_mode, "review");
        assert_eq!(c.context_injection_scope, "both");
        assert_eq!(c.stewardship_mode, "review");
        assert!(c.notifications_enabled);
        assert!(!c.conflict_webhook_enabled);
        assert!(c.impulse_agent_api_key.is_none());
        assert!(c.impulse_agent_provider.is_none());
        assert!(c.context_lifecycle_enabled);
        assert_eq!(c.context_lifecycle_poll_secs, 5);
        assert_eq!(c.tool_execution_default_timeout_ms, 30_000);
        assert_eq!(c.tool_execution_max_output_bytes, 256 * 1024);
    }

    // ── Serde roundtrip ─────────────────────────────────────────────────

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let c = Config::default();
        let json = serde_json::to_string(&c).unwrap();
        let c2: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(c2.log_level, c.log_level);
        assert_eq!(c2.sync_interval_secs, c.sync_interval_secs);
        assert_eq!(c2.max_history_entries, c.max_history_entries);
        assert_eq!(c2.retrieval_mode, c.retrieval_mode);
        assert_eq!(c2.context_injection_mode, c.context_injection_mode);
        assert_eq!(c2.stewardship_mode, c.stewardship_mode);
        assert_eq!(
            c2.tool_execution_default_timeout_ms,
            c.tool_execution_default_timeout_ms
        );
    }

    #[test]
    fn serde_roundtrip_api_key_is_skipped() {
        let mut c = Config::default();
        c.impulse_agent_api_key = Some("secret-key-123".to_string());
        let json = serde_json::to_string(&c).unwrap();
        // skip_serializing means the key should NOT appear in JSON output
        assert!(!json.contains("secret-key-123"));
        assert!(!json.contains("impulse_agent_api_key"));
        // Deserializing back gives None for the api key
        let c2: Config = serde_json::from_str(&json).unwrap();
        assert!(c2.impulse_agent_api_key.is_none());
    }

    #[test]
    fn serde_default_fills_missing_fields() {
        // Minimal JSON — all fields should fill from Default
        let json = r#"{}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.log_level, "info");
        assert_eq!(c.sync_interval_secs, 30);
        assert_eq!(c.max_history_entries, 1000);
    }

    #[test]
    fn serde_partial_json_preserves_overrides() {
        let json = r#"{"log_level":"debug","verbose":true}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.log_level, "debug");
        assert!(c.verbose);
        // Everything else still defaults
        assert_eq!(c.sync_interval_secs, 30);
    }

    // ── get() ───────────────────────────────────────────────────────────

    #[test]
    fn get_string_fields() {
        let c = Config::default();
        assert_eq!(c.get("log_level"), Some("info".to_string()));
        assert_eq!(c.get("retrieval_mode"), Some("keyword".to_string()));
        assert_eq!(c.get("context_injection_mode"), Some("review".to_string()));
    }

    #[test]
    fn get_bool_fields() {
        let c = Config::default();
        assert_eq!(c.get("verbose"), Some("false".to_string()));
        assert_eq!(c.get("notifications_enabled"), Some("true".to_string()));
    }

    #[test]
    fn get_numeric_fields() {
        let c = Config::default();
        assert_eq!(c.get("sync_interval_secs"), Some("30".to_string()));
        assert_eq!(c.get("max_history_entries"), Some("1000".to_string()));
        assert_eq!(
            c.get("tool_execution.default_timeout_ms"),
            Some("30000".to_string())
        );
    }

    #[test]
    fn get_optional_none_returns_none() {
        let c = Config::default();
        assert!(c.get("default_platform").is_none());
    }

    #[test]
    fn get_api_key_is_masked() {
        let mut c = Config::default();
        c.impulse_agent_api_key = Some("sk-ant-12345".to_string());
        assert_eq!(c.get("impulse_agent_api_key"), Some("***".to_string()));
    }

    #[test]
    fn get_api_key_none_returns_none() {
        let c = Config::default();
        assert!(c.get("impulse_agent_api_key").is_none());
    }

    #[test]
    fn get_csv_list_fields() {
        let c = Config::default();
        let val = c.get("build_hygiene_scan_paths").unwrap();
        assert!(val.contains("~/projects"));
        assert!(val.contains("~/Desktop"));
    }

    #[test]
    fn get_guardrails_enabled() {
        let c = Config::default();
        // GuardConfig::default().enabled is true
        let val = c.get("guardrails_enabled").unwrap();
        assert_eq!(val, "true");
    }

    #[test]
    fn get_unknown_key_returns_none() {
        let c = Config::default();
        assert!(c.get("nonexistent_key").is_none());
    }

    // ── set() — Bool ────────────────────────────────────────────────────

    #[test]
    fn set_bool_valid() {
        let mut c = Config::default();
        assert!(c.set("verbose", "true"));
        assert!(c.verbose);
        assert!(c.set("verbose", "false"));
        assert!(!c.verbose);
    }

    #[test]
    fn set_bool_invalid_rejected() {
        let mut c = Config::default();
        assert!(!c.set("verbose", "yes"));
        assert!(!c.set("verbose", "1"));
        assert!(!c.set("verbose", ""));
    }

    // ── set() — Enum ────────────────────────────────────────────────────

    #[test]
    fn set_enum_valid() {
        let mut c = Config::default();
        assert!(c.set("log_level", "debug"));
        assert_eq!(c.log_level, "debug");
        assert!(c.set("log_level", "error"));
        assert_eq!(c.log_level, "error");
    }

    #[test]
    fn set_enum_invalid_rejected() {
        let mut c = Config::default();
        assert!(!c.set("log_level", "verbose"));
        assert!(!c.set("log_level", ""));
        // Value unchanged
        assert_eq!(c.log_level, "info");
    }

    #[test]
    fn set_retrieval_mode_valid() {
        let mut c = Config::default();
        assert!(c.set("retrieval_mode", "semantic"));
        assert_eq!(c.retrieval_mode, "semantic");
    }

    #[test]
    fn set_context_injection_mode_valid() {
        let mut c = Config::default();
        for mode in &["off", "review", "apply"] {
            assert!(c.set("context_injection_mode", mode));
            assert_eq!(c.context_injection_mode, *mode);
        }
    }

    #[test]
    fn set_stewardship_mode_valid() {
        let mut c = Config::default();
        for mode in &["auto", "review", "off"] {
            assert!(c.set("stewardship_mode", mode));
            assert_eq!(c.stewardship_mode, *mode);
        }
    }

    // ── set() — U64 with range ──────────────────────────────────────────

    #[test]
    fn set_u64_valid() {
        let mut c = Config::default();
        assert!(c.set("sync_interval_secs", "60"));
        assert_eq!(c.sync_interval_secs, 60);
    }

    #[test]
    fn set_u64_range_rejected() {
        let mut c = Config::default();
        // retrieval_query_timeout_secs has min=1, max=120
        assert!(!c.set("retrieval_query_timeout_secs", "0"));
        assert!(!c.set("retrieval_query_timeout_secs", "121"));
        assert!(c.set("retrieval_query_timeout_secs", "1"));
        assert_eq!(c.retrieval_query_timeout_secs, 1);
        assert!(c.set("retrieval_query_timeout_secs", "120"));
        assert_eq!(c.retrieval_query_timeout_secs, 120);
    }

    #[test]
    fn set_u64_non_numeric_rejected() {
        let mut c = Config::default();
        assert!(!c.set("sync_interval_secs", "abc"));
        assert!(!c.set("sync_interval_secs", ""));
    }

    // ── set() — Usize with range ────────────────────────────────────────

    #[test]
    fn set_usize_valid() {
        let mut c = Config::default();
        assert!(c.set("max_history_entries", "500"));
        assert_eq!(c.max_history_entries, 500);
    }

    #[test]
    fn set_usize_range_rejected() {
        let mut c = Config::default();
        // retrieval_batch_size: min=1, max=512
        assert!(!c.set("retrieval_batch_size", "0"));
        assert!(!c.set("retrieval_batch_size", "513"));
        assert!(c.set("retrieval_batch_size", "1"));
        assert_eq!(c.retrieval_batch_size, 1);
        assert!(c.set("retrieval_batch_size", "512"));
        assert_eq!(c.retrieval_batch_size, 512);
    }

    // ── set() — F32 with range ──────────────────────────────────────────

    #[test]
    fn set_f32_valid() {
        let mut c = Config::default();
        assert!(c.set("retrieval_similarity_threshold", "0.85"));
        assert!((c.retrieval_similarity_threshold - 0.85).abs() < 0.01);
    }

    #[test]
    fn set_f32_range_rejected() {
        let mut c = Config::default();
        assert!(!c.set("retrieval_similarity_threshold", "-0.1"));
        assert!(!c.set("retrieval_similarity_threshold", "1.1"));
        // Boundary values
        assert!(c.set("retrieval_similarity_threshold", "0.0"));
        assert!(c.set("retrieval_similarity_threshold", "1.0"));
    }

    // ── set() — F64 / U32 ──────────────────────────────────────────────

    #[test]
    fn set_f64_valid() {
        let mut c = Config::default();
        assert!(c.set("build_hygiene_size_threshold_gb", "25.5"));
        assert!((c.build_hygiene_size_threshold_gb - 25.5).abs() < f64::EPSILON);
    }

    #[test]
    fn set_u32_valid() {
        let mut c = Config::default();
        assert!(c.set("build_hygiene_age_threshold_days", "90"));
        assert_eq!(c.build_hygiene_age_threshold_days, 90);
    }

    // ── set() — String / OptionalString / SomeString ────────────────────

    #[test]
    fn set_string_valid() {
        let mut c = Config::default();
        assert!(c.set("embedding_model", "text-embedding-ada-002"));
        assert_eq!(c.embedding_model, "text-embedding-ada-002");
    }

    #[test]
    fn set_string_empty_rejected() {
        let mut c = Config::default();
        assert!(!c.set("embedding_model", ""));
        assert!(!c.set("embedding_model", "   "));
    }

    #[test]
    fn set_optional_string_to_value() {
        let mut c = Config::default();
        assert!(c.set("impulse_agent_model", "claude-3-opus"));
        assert_eq!(c.impulse_agent_model, Some("claude-3-opus".to_string()));
    }

    #[test]
    fn set_optional_string_clear_with_none() {
        let mut c = Config::default();
        c.impulse_agent_model = Some("gpt-4".to_string());
        assert!(c.set("impulse_agent_model", "none"));
        assert!(c.impulse_agent_model.is_none());
    }

    #[test]
    fn set_optional_string_clear_with_empty() {
        let mut c = Config::default();
        c.impulse_agent_model = Some("gpt-4".to_string());
        assert!(c.set("impulse_agent_model", ""));
        assert!(c.impulse_agent_model.is_none());
    }

    #[test]
    fn set_some_string_wraps() {
        let mut c = Config::default();
        assert!(c.set("model.anthropic", "claude-3-5-sonnet"));
        assert_eq!(c.model_anthropic, Some("claude-3-5-sonnet".to_string()));
    }

    // ── set() — CsvList ─────────────────────────────────────────────────

    #[test]
    fn set_csv_list_valid() {
        let mut c = Config::default();
        assert!(c.set("build_hygiene_scan_paths", "~/a, ~/b, ~/c"));
        assert_eq!(
            c.build_hygiene_scan_paths,
            vec!["~/a".to_string(), "~/b".to_string(), "~/c".to_string()]
        );
    }

    #[test]
    fn set_csv_list_filters_empty() {
        let mut c = Config::default();
        assert!(c.set("build_hygiene_scan_paths", "~/a,,, ~/b"));
        assert_eq!(
            c.build_hygiene_scan_paths,
            vec!["~/a".to_string(), "~/b".to_string()]
        );
    }

    // ── set() — Custom: default_platform ────────────────────────────────

    #[test]
    fn set_platform_valid() {
        let mut c = Config::default();
        assert!(c.set("default_platform", "claude-code"));
        assert_eq!(c.default_platform, Some(Platform::ClaudeCode));
        assert!(c.set("default_platform", "opencode"));
        assert_eq!(c.default_platform, Some(Platform::OpenCode));
        assert!(c.set("default_platform", "none"));
        assert!(c.default_platform.is_none());
    }

    #[test]
    fn set_platform_invalid_rejected() {
        let mut c = Config::default();
        assert!(!c.set("default_platform", "cursor"));
        assert!(!c.set("default_platform", ""));
    }

    // ── set() — Custom: api_key ─────────────────────────────────────────

    #[test]
    fn set_api_key_stores_and_clears() {
        let mut c = Config::default();
        assert!(c.set("impulse_agent_api_key", "sk-test-key"));
        assert_eq!(c.impulse_agent_api_key, Some("sk-test-key".to_string()));
        assert!(c.set("impulse_agent_api_key", ""));
        assert!(c.impulse_agent_api_key.is_none());
    }

    // ── set() — Custom: guardrails_enabled ──────────────────────────────

    #[test]
    fn set_guardrails_enabled() {
        let mut c = Config::default();
        assert!(c.set("guardrails_enabled", "false"));
        assert!(!c.guardrails.enabled);
        assert!(c.set("guardrails_enabled", "true"));
        assert!(c.guardrails.enabled);
    }

    #[test]
    fn set_guardrails_enabled_invalid_rejected() {
        let mut c = Config::default();
        assert!(!c.set("guardrails_enabled", "yes"));
    }

    // ── set() — unknown key ─────────────────────────────────────────────

    #[test]
    fn set_unknown_key_rejected() {
        let mut c = Config::default();
        assert!(!c.set("nonexistent", "value"));
    }

    // ── set_field_json preserves api_key ─────────────────────────────────

    #[test]
    fn set_field_json_preserves_api_key() {
        let mut c = Config::default();
        c.impulse_agent_api_key = Some("my-secret".to_string());
        // Setting an unrelated field should preserve the api key
        assert!(c.set("verbose", "true"));
        assert_eq!(c.impulse_agent_api_key, Some("my-secret".to_string()));
        assert!(c.verbose);
    }

    #[test]
    fn multiple_sets_preserve_api_key() {
        let mut c = Config::default();
        c.impulse_agent_api_key = Some("persistent-key".to_string());
        assert!(c.set("log_level", "debug"));
        assert!(c.set("verbose", "true"));
        assert!(c.set("max_history_entries", "2000"));
        assert_eq!(c.impulse_agent_api_key, Some("persistent-key".to_string()));
    }

    // ── list() ──────────────────────────────────────────────────────────

    #[test]
    fn list_returns_all_config_keys() {
        let c = Config::default();
        let pairs = c.list();
        assert_eq!(pairs.len(), Config::CONFIG_KEYS.len());
        // Verify ordering matches CONFIG_KEYS
        for (i, (key, _)) in pairs.iter().enumerate() {
            assert_eq!(key, Config::CONFIG_KEYS[i]);
        }
    }

    #[test]
    fn list_shows_not_set_for_optional_agent_fields() {
        let c = Config::default();
        let pairs = c.list();
        let map: HashMap<String, String> = pairs.into_iter().collect();
        assert_eq!(map.get("impulse_agent_provider").unwrap(), "(not set)");
        assert_eq!(map.get("impulse_agent_harness").unwrap(), "(not set)");
        assert_eq!(map.get("impulse_agent_api_key").unwrap(), "(not set)");
        assert_eq!(map.get("impulse_agent_model").unwrap(), "(default)");
    }

    // ── resolve_path_setting ────────────────────────────────────────────

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

    // ── resolved_tool_roots ─────────────────────────────────────────────

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

    // ── resolve_field_name ──────────────────────────────────────────────

    #[test]
    fn resolve_field_name_dot_keys() {
        assert_eq!(
            Config::resolve_field_name("model.anthropic"),
            "model_anthropic"
        );
        assert_eq!(Config::resolve_field_name("model.openai"), "model_openai");
        assert_eq!(
            Config::resolve_field_name("tool_execution.default_timeout_ms"),
            "tool_execution_default_timeout_ms"
        );
    }

    #[test]
    fn resolve_field_name_passthrough() {
        assert_eq!(Config::resolve_field_name("log_level"), "log_level");
        assert_eq!(Config::resolve_field_name("verbose"), "verbose");
    }

    // ── json_value_to_string ────────────────────────────────────────────

    #[test]
    fn json_value_to_string_conversions() {
        assert!(Config::json_value_to_string(&serde_json::Value::Null).is_none());
        assert_eq!(
            Config::json_value_to_string(&serde_json::Value::String("hello".into())),
            Some("hello".to_string())
        );
        assert_eq!(
            Config::json_value_to_string(&serde_json::Value::Bool(true)),
            Some("true".to_string())
        );
        assert_eq!(
            Config::json_value_to_string(&serde_json::json!(42)),
            Some("42".to_string())
        );
        assert_eq!(
            Config::json_value_to_string(&serde_json::json!(["a", "b"])),
            Some("a,b".to_string())
        );
    }

    // ── Dot-notation keys via get/set ───────────────────────────────────

    #[test]
    fn get_set_dot_notation_keys() {
        let mut c = Config::default();
        assert!(c.set("model.anthropic", "claude-3-opus"));
        assert_eq!(c.get("model.anthropic"), Some("claude-3-opus".to_string()));
        assert!(c.set("tool_execution.max_artifacts", "16"));
        assert_eq!(
            c.get("tool_execution.max_artifacts"),
            Some("16".to_string())
        );
    }

    // ── Edge case: set + get roundtrip ──────────────────────────────────

    #[test]
    fn set_get_roundtrip_for_all_types() {
        let mut c = Config::default();
        // Bool
        c.set("verbose", "true");
        assert_eq!(c.get("verbose"), Some("true".to_string()));
        // Enum
        c.set("log_level", "debug");
        assert_eq!(c.get("log_level"), Some("debug".to_string()));
        // U64
        c.set("sync_interval_secs", "120");
        assert_eq!(c.get("sync_interval_secs"), Some("120".to_string()));
        // Usize
        c.set("max_history_entries", "2000");
        assert_eq!(c.get("max_history_entries"), Some("2000".to_string()));
        // F32
        c.set("retrieval_similarity_threshold", "0.9");
        let val = c.get("retrieval_similarity_threshold").unwrap();
        let parsed: f32 = val.parse().unwrap();
        assert!((parsed - 0.9).abs() < 0.01);
        // Platform custom
        c.set("default_platform", "claude-code");
        assert_eq!(c.get("default_platform"), Some("claude-code".to_string()));
    }
}
