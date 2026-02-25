// Integration tests for build_hygiene module

use super::*;
use std::fs;

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1 KB");
    assert_eq!(format_bytes(1_048_576), "1.0 MB");
    assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
    assert_eq!(format_bytes(5_368_709_120), "5.0 GB");
}

#[test]
fn test_build_hygiene_config_default() {
    let config = BuildHygieneConfig::default();
    assert!(config.enabled);
    assert_eq!(config.size_threshold_gb, 10.0);
    assert_eq!(config.age_threshold_days, 30);
    assert!(config.dry_run_default);
    assert!(!config.sweep_on_session_end);
    assert!(config.sweep_on_toolchain_update);
}

#[test]
fn test_build_hygiene_config_serialization() {
    let config = BuildHygieneConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: BuildHygieneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.enabled, config.enabled);
    assert_eq!(deserialized.size_threshold_gb, config.size_threshold_gb);
    assert_eq!(deserialized.age_threshold_days, config.age_threshold_days);
}

#[test]
fn test_expanded_scan_paths_filters_nonexistent() {
    let config = BuildHygieneConfig {
        scan_paths: vec!["/tmp".to_string(), "/nonexistent_path_12345".to_string()],
        ..Default::default()
    };
    let paths = config.expanded_scan_paths();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].to_string_lossy(), "/tmp");
}

#[test]
fn test_evaluate_rules_disabled() {
    let config = BuildHygieneConfig {
        enabled: false,
        ..Default::default()
    };
    let (should, _) = evaluate_auto_sweep_rules(&config, 100_000_000_000, AutoSweepTrigger::Manual);
    assert!(!should);
}

#[test]
fn test_evaluate_rules_session_end_enabled() {
    let config = BuildHygieneConfig {
        sweep_on_session_end: true,
        ..Default::default()
    };
    let (should, reason) = evaluate_auto_sweep_rules(&config, 0, AutoSweepTrigger::SessionEnd);
    assert!(should);
    assert!(reason.unwrap().contains("Session ended"));
}

#[test]
fn test_evaluate_rules_session_end_disabled() {
    let config = BuildHygieneConfig {
        sweep_on_session_end: false,
        ..Default::default()
    };
    let (should, _) = evaluate_auto_sweep_rules(&config, 0, AutoSweepTrigger::SessionEnd);
    assert!(!should);
}

#[test]
fn test_evaluate_rules_toolchain_update() {
    let config = BuildHygieneConfig {
        sweep_on_toolchain_update: true,
        ..Default::default()
    };
    let (should, reason) = evaluate_auto_sweep_rules(&config, 0, AutoSweepTrigger::ToolchainUpdate);
    assert!(should);
    assert!(reason.unwrap().contains("Toolchain update"));
}

#[test]
fn test_evaluate_rules_size_threshold_exceeded() {
    let config = BuildHygieneConfig {
        size_threshold_gb: 5.0,
        ..Default::default()
    };
    // 10 GB
    let (should, reason) =
        evaluate_auto_sweep_rules(&config, 10_737_418_240, AutoSweepTrigger::SizeThreshold);
    assert!(should);
    assert!(reason.unwrap().contains("10.0 GB"));
}

#[test]
fn test_evaluate_rules_size_threshold_not_exceeded() {
    let config = BuildHygieneConfig {
        size_threshold_gb: 20.0,
        ..Default::default()
    };
    // 5 GB
    let (should, _) =
        evaluate_auto_sweep_rules(&config, 5_368_709_120, AutoSweepTrigger::SizeThreshold);
    assert!(!should);
}

#[test]
fn test_evaluate_rules_manual_always_triggers() {
    let config = BuildHygieneConfig::default();
    let (should, reason) = evaluate_auto_sweep_rules(&config, 0, AutoSweepTrigger::Manual);
    assert!(should);
    assert!(reason.unwrap().contains("Manual"));
}

#[test]
fn test_clean_result_dry_run_summary() {
    let result = CleanResult::dry_run_summary(5, 5_368_709_120);
    assert!(result.was_dry_run);
    assert_eq!(result.projects_cleaned, 5);
    assert!(result.summary.contains("DRY RUN"));
    assert!(result.summary.contains("5.0 GB"));
}

#[test]
fn test_is_cargo_tool_installed_nonexistent() {
    // A tool that definitely doesn't exist
    assert!(!is_cargo_tool_installed("nonexistent-tool-xyz-12345"));
}

#[test]
fn test_cargo_tool_install_cmd() {
    assert_eq!(
        cargo_tool_install_cmd("cargo-sweep"),
        "cargo install cargo-sweep"
    );
}

#[test]
fn test_end_to_end_discovery_and_measurement() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a fake Rust project
    let proj = tmp.path().join("fake-project");
    fs::create_dir_all(proj.join("target/debug")).unwrap();
    fs::write(proj.join("Cargo.toml"), "[package]\nname = \"fake\"\n").unwrap();
    fs::write(proj.join("Cargo.lock"), "# lock file").unwrap();
    fs::write(proj.join("target/debug/fake"), "binary-content-here").unwrap();

    // Discover
    let projects = discover_rust_projects(&[tmp.path().to_path_buf()]);
    assert_eq!(projects.len(), 1);
    assert!(projects[0].has_cargo_lock);
    assert!(projects[0].target_size_bytes > 0);

    // Measure
    let report = measurement::generate_report(&projects, 10.0);
    assert_eq!(report.project_count, 1);
    assert!(report.total_bytes > 0);

    // Health check
    let health = measurement::check_build_health(&projects, 10.0);
    assert_eq!(health.status, crate::tools::health::HealthStatus::Healthy);
}
