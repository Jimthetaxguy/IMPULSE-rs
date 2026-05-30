//! Tests for config key get/set/list operations.

use super::super::config::Config;
use super::super::*;
use std::collections::HashMap;

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

/// Full round-trip via serde_json::Value -- proves zero data loss
/// on serialize -> deserialize for the default config.
#[test]
fn config_round_trip_via_value() {
    let original = Config::default();
    let json = serde_json::to_string_pretty(&original).unwrap();
    let recovered: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(&original).unwrap(),
        serde_json::to_value(&recovered).unwrap(),
    );
}

/// Load the real .impulse/config.json from the project (if present),
/// deserialize, re-serialize, and verify no data loss.
#[test]
fn config_round_trip_real_file() {
    // Walk up from CARGO_MANIFEST_DIR to find .impulse/config.json
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = manifest.join(".impulse/config.json");
    if !config_path.exists() {
        // Not an error -- CI or fresh checkouts may not have this file
        eprintln!(
            "skipping real config.json round-trip: {:?} not found",
            config_path
        );
        return;
    }
    let raw = std::fs::read_to_string(&config_path).unwrap();
    let parsed: Config = serde_json::from_str(&raw).unwrap();
    let reserialized = serde_json::to_string_pretty(&parsed).unwrap();
    let reparsed: Config = serde_json::from_str(&reserialized).unwrap();

    // Compare via Value to catch any drift
    let v1 = serde_json::to_value(&parsed).unwrap();
    let v2 = serde_json::to_value(&reparsed).unwrap();
    assert_eq!(v1, v2, "real config.json round-trip lost data");
}

#[test]
fn serde_roundtrip_api_key_is_skipped() {
    let c = Config {
        impulse_agent_api_key: Some("secret-key-123".to_string()),
        ..Default::default()
    };
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
    // Minimal JSON -- all fields should fill from Default
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
    let c = Config {
        impulse_agent_api_key: Some("sk-ant-12345".to_string()),
        ..Default::default()
    };
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

// ── set() -- Bool ────────────────────────────────────────────────────

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

// ── set() -- Enum ────────────────────────────────────────────────────

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

// ── set() -- U64 with range ──────────────────────────────────────────

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

// ── set() -- Usize with range ────────────────────────────────────────

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

// ── set() -- F32 with range ──────────────────────────────────────────

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

// ── set() -- F64 / U32 ──────────────────────────────────────────────

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

// ── set() -- String / OptionalString / SomeString ────────────────────

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
    let mut c = Config {
        impulse_agent_model: Some("gpt-4".to_string()),
        ..Default::default()
    };
    assert!(c.set("impulse_agent_model", "none"));
    assert!(c.impulse_agent_model.is_none());
}

#[test]
fn set_optional_string_clear_with_empty() {
    let mut c = Config {
        impulse_agent_model: Some("gpt-4".to_string()),
        ..Default::default()
    };
    assert!(c.set("impulse_agent_model", ""));
    assert!(c.impulse_agent_model.is_none());
}

#[test]
fn set_some_string_wraps() {
    let mut c = Config::default();
    assert!(c.set("model.anthropic", "claude-3-5-sonnet"));
    assert_eq!(c.model_anthropic, Some("claude-3-5-sonnet".to_string()));
}

// ── set() -- CsvList ─────────────────────────────────────────────────

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

// ── set() -- Custom: default_platform ────────────────────────────────

#[test]
fn set_platform_valid() {
    let mut c = Config::default();
    assert!(c.set("default_platform", "claude-code"));
    assert_eq!(c.default_platform, Some(Platform::ClaudeCode));
    assert!(c.set("default_platform", "codex"));
    assert_eq!(c.default_platform, Some(Platform::Codex));
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

// ── set() -- Custom: api_key ─────────────────────────────────────────

#[test]
fn set_api_key_stores_and_clears() {
    let mut c = Config::default();
    assert!(c.set("impulse_agent_api_key", "sk-test-key"));
    assert_eq!(c.impulse_agent_api_key, Some("sk-test-key".to_string()));
    assert!(c.set("impulse_agent_api_key", ""));
    assert!(c.impulse_agent_api_key.is_none());
}

// ── set() -- Custom: guardrails_enabled ──────────────────────────────

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

// ── set() -- unknown key ─────────────────────────────────────────────

#[test]
fn set_unknown_key_rejected() {
    let mut c = Config::default();
    assert!(!c.set("nonexistent", "value"));
}

// ── set_field_json preserves api_key ─────────────────────────────────

#[test]
fn set_field_json_preserves_api_key() {
    let mut c = Config {
        impulse_agent_api_key: Some("my-secret".to_string()),
        ..Default::default()
    };
    // Setting an unrelated field should preserve the api key
    assert!(c.set("verbose", "true"));
    assert_eq!(c.impulse_agent_api_key, Some("my-secret".to_string()));
    assert!(c.verbose);
}

#[test]
fn multiple_sets_preserve_api_key() {
    let mut c = Config {
        impulse_agent_api_key: Some("persistent-key".to_string()),
        ..Default::default()
    };
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
