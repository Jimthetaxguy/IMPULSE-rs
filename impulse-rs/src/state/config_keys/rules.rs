//! Validation rule registry for config key `set()`.
//!
//! Maps each config key to a [`SetRule`] that governs parsing and validation.
//! Separated from the main module to keep the registry declarative and scannable.

use super::super::config::Config;
use super::super::*;
use super::SetRule;
use std::collections::HashMap;

// ── Validation rule registry ──────────────────────────────────────────────

impl Config {
    pub(super) fn build_set_rules() -> HashMap<&'static str, SetRule> {
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

        // String fields (non-empty required) (5)
        for &key in &[
            "retrieval_embedding_provider",
            "embedding_model",
            "retrieval_python_cmd",
            "retrieval_ollama_url",
            "external_tools_dir",
        ] {
            m.insert(key, SetRule::String);
        }

        // Optional string fields (empty/"none" clears) (4)
        m.insert("impulse_agent_model", SetRule::OptionalString);
        m.insert("impulse_agent_escalate_model", SetRule::OptionalString);
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
                    "codex" => c.default_platform = Some(Platform::Codex),
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
