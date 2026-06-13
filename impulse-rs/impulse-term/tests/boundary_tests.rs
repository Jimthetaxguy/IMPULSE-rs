//! Boundary tests for the framework-neutral terminal API.
//!
//! This file intentionally imports only APIs that must remain available when
//! `impulse-term` is built with `--no-default-features`.

use impulse_term::{bracketed_paste, AgentKind, ContextHealth, ContextTier, TerminalBackend};

/// The exact signature `TerminalBackend::spawn` must keep exporting when built
/// without the egui feature. Aliasing both documents the API contract and
/// satisfies `clippy::type_complexity` (clippy's own "factor into a type def").
type SpawnFn = fn(
    &str,
    &[String],
    Option<&std::path::Path>,
    &[(&str, String)],
    u16,
    u16,
    Option<usize>,
) -> Result<TerminalBackend, Box<dyn std::error::Error>>;

#[test]
fn test_core_exports_available_without_egui_feature() {
    let _spawn: SpawnFn = TerminalBackend::spawn;

    assert_eq!(AgentKind::detect("codex", "agent"), AgentKind::Codex);
    assert_eq!(ContextTier::Critical.as_str(), "critical");
}

#[test]
fn test_paste_api_available_without_egui_feature() {
    let bytes = bracketed_paste("framework neutral");

    assert!(bytes.starts_with(b"\x1b[200~"));
    assert!(bytes.ends_with(b"\x1b[201~"));
    assert!(bytes
        .windows("framework neutral".len())
        .any(|window| window == b"framework neutral"));
}

#[test]
fn test_context_health_type_available_without_egui_feature() {
    let health = ContextHealth {
        tier: ContextTier::Essential,
        estimated_tokens: 90_000,
        window_tokens: 200_000,
        usage_fraction: 0.45,
        compaction_count: 1,
        injection_count: 2,
    };

    assert_eq!(health.tier, ContextTier::Essential);
    assert_eq!(health.estimated_tokens, 90_000);
    assert_eq!(health.window_tokens, 200_000);
    assert_eq!(health.compaction_count, 1);
    assert_eq!(health.injection_count, 2);
}
