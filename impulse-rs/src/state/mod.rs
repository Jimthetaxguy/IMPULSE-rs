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
            retrieval_python_cmd: "python3".to_string(),
            retrieval_vector_enabled: false,
            retrieval_semantic_strategy: "auto".to_string(),
            retrieval_query_timeout_secs: 10,
            retrieval_index_timeout_secs: 60,
            retrieval_batch_size: 64,
            retrieval_candidate_pool: 200,
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
        }
    }
}

impl Config {
    /// Get a config value by key
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "log_level" => Some(self.log_level.clone()),
            "default_platform" => self
                .default_platform
                .map(|p| format!("{:?}", p).to_lowercase()),
            "verbose" => Some(self.verbose.to_string()),
            "sync_interval_secs" => Some(self.sync_interval_secs.to_string()),
            "max_history_entries" => Some(self.max_history_entries.to_string()),
            "retrieval_mode" => Some(self.retrieval_mode.clone()),
            "retrieval_backend" => Some(self.retrieval_backend.clone()),
            "retrieval_default_limit" => Some(self.retrieval_default_limit.to_string()),
            "retrieval_similarity_threshold" => {
                Some(self.retrieval_similarity_threshold.to_string())
            }
            "retrieval_embedding_provider" => Some(self.retrieval_embedding_provider.clone()),
            "retrieval_python_cmd" => Some(self.retrieval_python_cmd.clone()),
            "retrieval_vector_enabled" => Some(self.retrieval_vector_enabled.to_string()),
            "retrieval_semantic_strategy" => Some(self.retrieval_semantic_strategy.clone()),
            "retrieval_query_timeout_secs" => Some(self.retrieval_query_timeout_secs.to_string()),
            "retrieval_index_timeout_secs" => Some(self.retrieval_index_timeout_secs.to_string()),
            "retrieval_batch_size" => Some(self.retrieval_batch_size.to_string()),
            "retrieval_candidate_pool" => Some(self.retrieval_candidate_pool.to_string()),
            "retrieval_experimental_pageindex_enabled" => {
                Some(self.retrieval_experimental_pageindex_enabled.to_string())
            }
            "retrieval_pageindex_mode" => Some(self.retrieval_pageindex_mode.clone()),
            "context_injection_mode" => Some(self.context_injection_mode.clone()),
            "context_injection_scope" => Some(self.context_injection_scope.clone()),
            "context_injection_max_items" => Some(self.context_injection_max_items.to_string()),
            "context_injection_max_chars" => Some(self.context_injection_max_chars.to_string()),
            "context_injection_min_score" => Some(self.context_injection_min_score.to_string()),
            "context_injection_use_semantic" => {
                Some(self.context_injection_use_semantic.to_string())
            }
            "context_injection_emit_artifacts" => {
                Some(self.context_injection_emit_artifacts.to_string())
            }
            "stewardship_mode" => Some(self.stewardship_mode.clone()),
            "stewardship_monitor_threshold" => Some(self.stewardship_monitor_threshold.to_string()),
            "stewardship_surgical_threshold" => {
                Some(self.stewardship_surgical_threshold.to_string())
            }
            "stewardship_thoughtful_threshold" => {
                Some(self.stewardship_thoughtful_threshold.to_string())
            }
            "stewardship_emergency_threshold" => {
                Some(self.stewardship_emergency_threshold.to_string())
            }
            "stewardship_poll_interval_secs" => {
                Some(self.stewardship_poll_interval_secs.to_string())
            }
            "stewardship_context_window_tokens" => {
                Some(self.stewardship_context_window_tokens.to_string())
            }
            "stewardship_cross_project_enabled" => {
                Some(self.stewardship_cross_project_enabled.to_string())
            }
            "model.anthropic" => self.model_anthropic.clone(),
            "model.openai" => self.model_openai.clone(),
            "model.google" => self.model_google.clone(),
            "model.mistral" => self.model_mistral.clone(),
            "build_hygiene_enabled" => Some(self.build_hygiene_enabled.to_string()),
            "build_hygiene_scan_paths" => Some(self.build_hygiene_scan_paths.join(",")),
            "build_hygiene_size_threshold_gb" => {
                Some(self.build_hygiene_size_threshold_gb.to_string())
            }
            "build_hygiene_age_threshold_days" => {
                Some(self.build_hygiene_age_threshold_days.to_string())
            }
            "build_hygiene_sweep_on_session_end" => {
                Some(self.build_hygiene_sweep_on_session_end.to_string())
            }
            "build_hygiene_sweep_on_toolchain_update" => {
                Some(self.build_hygiene_sweep_on_toolchain_update.to_string())
            }
            "build_hygiene_dry_run_default" => Some(self.build_hygiene_dry_run_default.to_string()),
            "context_lifecycle_enabled" => Some(self.context_lifecycle_enabled.to_string()),
            "context_lifecycle_poll_secs" => Some(self.context_lifecycle_poll_secs.to_string()),
            "context_lifecycle_startup_delay_ms" => {
                Some(self.context_lifecycle_startup_delay_ms.to_string())
            }
            "context_lifecycle_window_tokens" => {
                Some(self.context_lifecycle_window_tokens.to_string())
            }
            "impulse_agent_provider" => self.impulse_agent_provider.clone(),
            "impulse_agent_api_key" => self
                .impulse_agent_api_key
                .as_ref()
                .map(|_| "***".to_string()),
            "impulse_agent_model" => self.impulse_agent_model.clone(),
            "impulse_agent_harness" => self.impulse_agent_harness.clone(),
            "impulse_agent_auto_review" => Some(self.impulse_agent_auto_review.to_string()),
            "impulse_agent_auto_coordinate" => Some(self.impulse_agent_auto_coordinate.to_string()),
            _ => None,
        }
    }

    /// Set a config value by key
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        match key {
            "log_level" => {
                if ["trace", "debug", "info", "warn", "error"].contains(&value) {
                    self.log_level = value.to_string();
                    true
                } else {
                    false
                }
            }
            "default_platform" => {
                match value {
                    "claude-code" => self.default_platform = Some(Platform::ClaudeCode),
                    "opencode" => self.default_platform = Some(Platform::OpenCode),
                    "none" => self.default_platform = None,
                    _ => return false,
                }
                true
            }
            "verbose" => {
                self.verbose = value.parse().unwrap_or(false);
                true
            }
            "sync_interval_secs" => {
                if let Ok(v) = value.parse() {
                    self.sync_interval_secs = v;
                    true
                } else {
                    false
                }
            }
            "max_history_entries" => {
                if let Ok(v) = value.parse() {
                    self.max_history_entries = v;
                    true
                } else {
                    false
                }
            }
            "retrieval_mode" => {
                if ["keyword", "semantic"].contains(&value) {
                    self.retrieval_mode = value.to_string();
                    true
                } else {
                    false
                }
            }
            "retrieval_backend" => {
                if ["fts", "fts+vec"].contains(&value) {
                    self.retrieval_backend = value.to_string();
                    true
                } else {
                    false
                }
            }
            "retrieval_default_limit" => {
                if let Ok(v) = value.parse::<usize>() {
                    self.retrieval_default_limit = v.max(1);
                    true
                } else {
                    false
                }
            }
            "retrieval_similarity_threshold" => {
                if let Ok(v) = value.parse::<f32>() {
                    if (0.0..=1.0).contains(&v) {
                        self.retrieval_similarity_threshold = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "retrieval_embedding_provider" => {
                if !value.trim().is_empty() {
                    self.retrieval_embedding_provider = value.to_string();
                    true
                } else {
                    false
                }
            }
            "retrieval_python_cmd" => {
                if !value.trim().is_empty() {
                    self.retrieval_python_cmd = value.to_string();
                    true
                } else {
                    false
                }
            }
            "retrieval_vector_enabled" => match value.parse::<bool>() {
                Ok(v) => {
                    self.retrieval_vector_enabled = v;
                    true
                }
                Err(_) => false,
            },
            "retrieval_semantic_strategy" => {
                if ["auto", "sqlite-only", "rust-only"].contains(&value) {
                    self.retrieval_semantic_strategy = value.to_string();
                    true
                } else {
                    false
                }
            }
            "retrieval_query_timeout_secs" => {
                if let Ok(v) = value.parse::<u64>() {
                    if (1..=120).contains(&v) {
                        self.retrieval_query_timeout_secs = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "retrieval_index_timeout_secs" => {
                if let Ok(v) = value.parse::<u64>() {
                    if (10..=600).contains(&v) {
                        self.retrieval_index_timeout_secs = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "retrieval_batch_size" => {
                if let Ok(v) = value.parse::<usize>() {
                    if (1..=512).contains(&v) {
                        self.retrieval_batch_size = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "retrieval_candidate_pool" => {
                if let Ok(v) = value.parse::<usize>() {
                    if (10..=5000).contains(&v) {
                        self.retrieval_candidate_pool = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "retrieval_experimental_pageindex_enabled" => match value.parse::<bool>() {
                Ok(v) => {
                    self.retrieval_experimental_pageindex_enabled = v;
                    true
                }
                Err(_) => false,
            },
            "retrieval_pageindex_mode" => {
                if ["local-structure", "api-augmented"].contains(&value) {
                    self.retrieval_pageindex_mode = value.to_string();
                    true
                } else {
                    false
                }
            }
            "context_injection_mode" => {
                if ["off", "review", "apply"].contains(&value) {
                    self.context_injection_mode = value.to_string();
                    true
                } else {
                    false
                }
            }
            "context_injection_scope" => {
                if ["daemon", "direct", "both"].contains(&value) {
                    self.context_injection_scope = value.to_string();
                    true
                } else {
                    false
                }
            }
            "context_injection_max_items" => {
                if let Ok(v) = value.parse::<usize>() {
                    if (1..=50).contains(&v) {
                        self.context_injection_max_items = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "context_injection_max_chars" => {
                if let Ok(v) = value.parse::<usize>() {
                    if (200..=20000).contains(&v) {
                        self.context_injection_max_chars = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "context_injection_min_score" => {
                if let Ok(v) = value.parse::<f32>() {
                    if (0.0..=1.0).contains(&v) {
                        self.context_injection_min_score = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "context_injection_use_semantic" => match value.parse::<bool>() {
                Ok(v) => {
                    self.context_injection_use_semantic = v;
                    true
                }
                Err(_) => false,
            },
            "context_injection_emit_artifacts" => match value.parse::<bool>() {
                Ok(v) => {
                    self.context_injection_emit_artifacts = v;
                    true
                }
                Err(_) => false,
            },
            "stewardship_mode" => {
                if ["auto", "review", "off"].contains(&value) {
                    self.stewardship_mode = value.to_string();
                    true
                } else {
                    false
                }
            }
            "stewardship_monitor_threshold" => {
                if let Ok(v) = value.parse::<f32>() {
                    if (0.0..=1.0).contains(&v) {
                        self.stewardship_monitor_threshold = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "stewardship_surgical_threshold" => {
                if let Ok(v) = value.parse::<f32>() {
                    if (0.0..=1.0).contains(&v) {
                        self.stewardship_surgical_threshold = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "stewardship_thoughtful_threshold" => {
                if let Ok(v) = value.parse::<f32>() {
                    if (0.0..=1.0).contains(&v) {
                        self.stewardship_thoughtful_threshold = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "stewardship_emergency_threshold" => {
                if let Ok(v) = value.parse::<f32>() {
                    if (0.0..=1.0).contains(&v) {
                        self.stewardship_emergency_threshold = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "stewardship_poll_interval_secs" => {
                if let Ok(v) = value.parse::<u64>() {
                    if (1..=300).contains(&v) {
                        self.stewardship_poll_interval_secs = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "stewardship_context_window_tokens" => {
                if let Ok(v) = value.parse::<usize>() {
                    if (10_000..=2_000_000).contains(&v) {
                        self.stewardship_context_window_tokens = v;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "stewardship_cross_project_enabled" => match value.parse::<bool>() {
                Ok(v) => {
                    self.stewardship_cross_project_enabled = v;
                    true
                }
                Err(_) => false,
            },
            "model.anthropic" => {
                self.model_anthropic = Some(value.to_string());
                true
            }
            "model.openai" => {
                self.model_openai = Some(value.to_string());
                true
            }
            "model.google" => {
                self.model_google = Some(value.to_string());
                true
            }
            "model.mistral" => {
                self.model_mistral = Some(value.to_string());
                true
            }
            "build_hygiene_enabled" => {
                self.build_hygiene_enabled = value.parse().unwrap_or(true);
                true
            }
            "build_hygiene_scan_paths" => {
                self.build_hygiene_scan_paths =
                    value.split(',').map(|s| s.trim().to_string()).collect();
                true
            }
            "build_hygiene_size_threshold_gb" => {
                if let Ok(v) = value.parse() {
                    self.build_hygiene_size_threshold_gb = v;
                    true
                } else {
                    false
                }
            }
            "build_hygiene_age_threshold_days" => {
                if let Ok(v) = value.parse() {
                    self.build_hygiene_age_threshold_days = v;
                    true
                } else {
                    false
                }
            }
            "build_hygiene_sweep_on_session_end" => {
                self.build_hygiene_sweep_on_session_end = value.parse().unwrap_or(false);
                true
            }
            "build_hygiene_sweep_on_toolchain_update" => {
                self.build_hygiene_sweep_on_toolchain_update = value.parse().unwrap_or(true);
                true
            }
            "build_hygiene_dry_run_default" => {
                self.build_hygiene_dry_run_default = value.parse().unwrap_or(true);
                true
            }
            "context_lifecycle_enabled" => {
                self.context_lifecycle_enabled = value.parse().unwrap_or(true);
                true
            }
            "context_lifecycle_poll_secs" => {
                if let Ok(v) = value.parse::<u64>() {
                    if (1..=60).contains(&v) {
                        self.context_lifecycle_poll_secs = v;
                        return true;
                    }
                }
                false
            }
            "context_lifecycle_startup_delay_ms" => {
                if let Ok(v) = value.parse::<u64>() {
                    if (100..=30_000).contains(&v) {
                        self.context_lifecycle_startup_delay_ms = v;
                        return true;
                    }
                }
                false
            }
            "context_lifecycle_window_tokens" => {
                if let Ok(v) = value.parse::<usize>() {
                    if (10_000..=2_000_000).contains(&v) {
                        self.context_lifecycle_window_tokens = v;
                        return true;
                    }
                }
                false
            }
            "impulse_agent_provider" => {
                if value.is_empty() || value == "none" {
                    self.impulse_agent_provider = None;
                } else if crate::impulse_agent::ImpulseProvider::parse(value).is_some() {
                    self.impulse_agent_provider = Some(value.to_string());
                } else {
                    return false;
                }
                true
            }
            "impulse_agent_api_key" => {
                if value.is_empty() {
                    self.impulse_agent_api_key = None;
                } else {
                    self.impulse_agent_api_key = Some(value.to_string());
                }
                true
            }
            "impulse_agent_model" => {
                if value.is_empty() || value == "none" {
                    self.impulse_agent_model = None;
                } else {
                    self.impulse_agent_model = Some(value.to_string());
                }
                true
            }
            "impulse_agent_harness" => {
                if value.is_empty() || value == "none" {
                    self.impulse_agent_harness = None;
                } else if crate::impulse_agent::ImpulseHarness::parse(value).is_some() {
                    self.impulse_agent_harness = Some(value.to_string());
                } else {
                    return false;
                }
                true
            }
            "impulse_agent_auto_review" => {
                self.impulse_agent_auto_review = value.parse().unwrap_or(false);
                true
            }
            "impulse_agent_auto_coordinate" => {
                self.impulse_agent_auto_coordinate = value.parse().unwrap_or(false);
                true
            }
            _ => false,
        }
    }

    /// List all config keys and values
    pub fn list(&self) -> Vec<(String, String)> {
        vec![
            ("log_level".to_string(), self.log_level.clone()),
            (
                "default_platform".to_string(),
                self.default_platform
                    .map(|p| format!("{:?}", p).to_lowercase())
                    .unwrap_or_default(),
            ),
            ("verbose".to_string(), self.verbose.to_string()),
            (
                "sync_interval_secs".to_string(),
                self.sync_interval_secs.to_string(),
            ),
            (
                "max_history_entries".to_string(),
                self.max_history_entries.to_string(),
            ),
            ("retrieval_mode".to_string(), self.retrieval_mode.clone()),
            (
                "retrieval_backend".to_string(),
                self.retrieval_backend.clone(),
            ),
            (
                "retrieval_default_limit".to_string(),
                self.retrieval_default_limit.to_string(),
            ),
            (
                "retrieval_similarity_threshold".to_string(),
                self.retrieval_similarity_threshold.to_string(),
            ),
            (
                "retrieval_embedding_provider".to_string(),
                self.retrieval_embedding_provider.clone(),
            ),
            (
                "retrieval_python_cmd".to_string(),
                self.retrieval_python_cmd.clone(),
            ),
            (
                "retrieval_vector_enabled".to_string(),
                self.retrieval_vector_enabled.to_string(),
            ),
            (
                "retrieval_semantic_strategy".to_string(),
                self.retrieval_semantic_strategy.clone(),
            ),
            (
                "retrieval_query_timeout_secs".to_string(),
                self.retrieval_query_timeout_secs.to_string(),
            ),
            (
                "retrieval_index_timeout_secs".to_string(),
                self.retrieval_index_timeout_secs.to_string(),
            ),
            (
                "retrieval_batch_size".to_string(),
                self.retrieval_batch_size.to_string(),
            ),
            (
                "retrieval_candidate_pool".to_string(),
                self.retrieval_candidate_pool.to_string(),
            ),
            (
                "retrieval_experimental_pageindex_enabled".to_string(),
                self.retrieval_experimental_pageindex_enabled.to_string(),
            ),
            (
                "retrieval_pageindex_mode".to_string(),
                self.retrieval_pageindex_mode.clone(),
            ),
            (
                "context_injection_mode".to_string(),
                self.context_injection_mode.clone(),
            ),
            (
                "context_injection_scope".to_string(),
                self.context_injection_scope.clone(),
            ),
            (
                "context_injection_max_items".to_string(),
                self.context_injection_max_items.to_string(),
            ),
            (
                "context_injection_max_chars".to_string(),
                self.context_injection_max_chars.to_string(),
            ),
            (
                "context_injection_min_score".to_string(),
                self.context_injection_min_score.to_string(),
            ),
            (
                "context_injection_use_semantic".to_string(),
                self.context_injection_use_semantic.to_string(),
            ),
            (
                "context_injection_emit_artifacts".to_string(),
                self.context_injection_emit_artifacts.to_string(),
            ),
            (
                "stewardship_mode".to_string(),
                self.stewardship_mode.clone(),
            ),
            (
                "stewardship_monitor_threshold".to_string(),
                self.stewardship_monitor_threshold.to_string(),
            ),
            (
                "stewardship_surgical_threshold".to_string(),
                self.stewardship_surgical_threshold.to_string(),
            ),
            (
                "stewardship_thoughtful_threshold".to_string(),
                self.stewardship_thoughtful_threshold.to_string(),
            ),
            (
                "stewardship_emergency_threshold".to_string(),
                self.stewardship_emergency_threshold.to_string(),
            ),
            (
                "stewardship_poll_interval_secs".to_string(),
                self.stewardship_poll_interval_secs.to_string(),
            ),
            (
                "stewardship_context_window_tokens".to_string(),
                self.stewardship_context_window_tokens.to_string(),
            ),
            (
                "stewardship_cross_project_enabled".to_string(),
                self.stewardship_cross_project_enabled.to_string(),
            ),
            (
                "model.anthropic".to_string(),
                self.model_anthropic.clone().unwrap_or_default(),
            ),
            (
                "model.openai".to_string(),
                self.model_openai.clone().unwrap_or_default(),
            ),
            (
                "model.google".to_string(),
                self.model_google.clone().unwrap_or_default(),
            ),
            (
                "model.mistral".to_string(),
                self.model_mistral.clone().unwrap_or_default(),
            ),
            (
                "build_hygiene_enabled".to_string(),
                self.build_hygiene_enabled.to_string(),
            ),
            (
                "build_hygiene_scan_paths".to_string(),
                self.build_hygiene_scan_paths.join(","),
            ),
            (
                "build_hygiene_size_threshold_gb".to_string(),
                self.build_hygiene_size_threshold_gb.to_string(),
            ),
            (
                "build_hygiene_age_threshold_days".to_string(),
                self.build_hygiene_age_threshold_days.to_string(),
            ),
            (
                "build_hygiene_sweep_on_session_end".to_string(),
                self.build_hygiene_sweep_on_session_end.to_string(),
            ),
            (
                "build_hygiene_sweep_on_toolchain_update".to_string(),
                self.build_hygiene_sweep_on_toolchain_update.to_string(),
            ),
            (
                "build_hygiene_dry_run_default".to_string(),
                self.build_hygiene_dry_run_default.to_string(),
            ),
            (
                "context_lifecycle_enabled".to_string(),
                self.context_lifecycle_enabled.to_string(),
            ),
            (
                "context_lifecycle_poll_secs".to_string(),
                self.context_lifecycle_poll_secs.to_string(),
            ),
            (
                "context_lifecycle_startup_delay_ms".to_string(),
                self.context_lifecycle_startup_delay_ms.to_string(),
            ),
            (
                "context_lifecycle_window_tokens".to_string(),
                self.context_lifecycle_window_tokens.to_string(),
            ),
            (
                "impulse_agent_provider".to_string(),
                self.impulse_agent_provider
                    .clone()
                    .unwrap_or_else(|| "(not set)".to_string()),
            ),
            (
                "impulse_agent_api_key".to_string(),
                if self.impulse_agent_api_key.is_some() {
                    "***".to_string()
                } else {
                    "(not set)".to_string()
                },
            ),
            (
                "impulse_agent_model".to_string(),
                self.impulse_agent_model
                    .clone()
                    .unwrap_or_else(|| "(default)".to_string()),
            ),
            (
                "impulse_agent_harness".to_string(),
                self.impulse_agent_harness
                    .clone()
                    .unwrap_or_else(|| "(not set)".to_string()),
            ),
            (
                "impulse_agent_auto_review".to_string(),
                self.impulse_agent_auto_review.to_string(),
            ),
            (
                "impulse_agent_auto_coordinate".to_string(),
                self.impulse_agent_auto_coordinate.to_string(),
            ),
        ]
    }
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

    #[allow(dead_code)]
    pub fn active_sessions(&self) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .collect()
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
                    let _ = self.storage.write_json(LIVE_STATE_FILE, &*state);
                }
            }
        }
    }
}

impl State {
    pub fn new(base_path: std::path::PathBuf) -> Result<Self> {
        let storage = Storage::new(base_path);
        let live_state = storage.read_json::<LiveState>(LIVE_STATE_FILE)?;
        let config = storage.read_json::<Config>(CONFIG_FILE).unwrap_or_default();

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

    #[allow(dead_code)]
    pub async fn sync(&self) -> Result<()> {
        let should_sync = {
            if let Ok(dirty) = self.dirty.try_read() {
                *dirty
            } else {
                false
            }
        };

        if should_sync {
            let state = self.live_state.try_read().map(|s| s.clone())?;
            self.storage.write_json(LIVE_STATE_FILE, &state)?;

            if let Ok(mut dirty) = self.dirty.try_write() {
                *dirty = false;
            }
        }

        Ok(())
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
        {
            let mut state = self
                .live_state
                .try_write()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            if let Some(session) = state.get_session_mut(session_id) {
                f(session);
            }
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

    #[allow(dead_code)]
    pub async fn get_history(&self) -> Result<Vec<HistoryEntry>> {
        let entries = self.storage.read_jsonl::<HistoryEntry>(HISTORY_FILE)?;
        Ok(entries)
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
}

pub type SharedState = Arc<State>;

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
}
