//! Config key registry: get/set/list infrastructure for [`Config`].
//!
//! Extracted from `config.rs` to keep the struct definition focused.
//! All 69 config keys, their validation rules, and the serde round-trip
//! machinery live here.
//!
//! ## Module layout
//!
//! - `mod.rs` — SetRule enum, CONFIG_KEYS constant, get/set/list, helpers
//! - `rules.rs` — `build_set_rules()` validation rule registry
//! - `tests.rs` — unit tests for all config key operations

mod rules;
#[cfg(test)]
mod tests;

use super::config::Config;

// ── SetRule enum ──────────────────────────────────────────────────────────

/// Validation rules for `Config::set()`. Each variant describes how to parse
/// and validate a string value before applying it to the config.
enum SetRule {
    /// Parse as bool; reject non-boolean strings.
    Bool,
    /// Non-empty string required.
    String,
    /// Empty or "none" -> null/None, otherwise set as string.
    OptionalString,
    /// Always wrap in Some(string) -- no clearing.
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

// ── Config key infrastructure ─────────────────────────────────────────────
//
// All 69 config keys in display order. Used by get(), list(), and set().
// Adding a new config field requires:
//   1. Add the field to the Config struct (in config.rs)
//   2. Add the key to CONFIG_KEYS
//   3. Add a SetRule entry in build_set_rules()
//   4. If the key name differs from the field name, add to resolve_field_name()

impl Config {
    /// Ordered list of all config keys (used by `list()`).
    pub(crate) const CONFIG_KEYS: &'static [&'static str] = &[
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
        "retrieval_ollama_url",
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

    // ── Key resolution ────────────────────────────────────────────────────

    /// Map user-facing key -> serde field name (only for keys that differ).
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

    // ── get / set / list ──────────────────────────────────────────────────

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

    // ── Serde round-trip setter ───────────────────────────────────────────

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
}
