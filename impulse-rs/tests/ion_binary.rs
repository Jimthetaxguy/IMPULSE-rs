//! Integration tests for the `ion` binary skeleton (TUI_SPEC.md T5).
//!
//! Scope is deliberately tiny: CLI parsing + banner. Gate behavior is covered
//! by the T4 fake-gate tests against `run_ion_verify`; these tests must never
//! spawn the real Pi gate (no network, no launch-gate.sh).

use std::process::Command;

fn ion() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ion"))
}

#[test]
fn test_ion_bare_run_prints_banner_and_exits_zero() {
    let output = ion().output().expect("Failed to run ion binary");
    assert!(
        output.status.success(),
        "bare `ion` must exit 0, got {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ion") && stdout.contains("Ion interactive harness"),
        "banner missing from stdout: {stdout}"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "banner should include the crate version: {stdout}"
    );
}

#[test]
fn test_ion_verify_help_parses_and_lists_flags() {
    let output = ion()
        .args(["verify", "--help"])
        .output()
        .expect("Failed to run ion verify --help");
    assert!(
        output.status.success(),
        "`ion verify --help` must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--repo", "--diff-ref", "--description", "--json"] {
        assert!(stdout.contains(flag), "help must document {flag}: {stdout}");
    }
}

#[test]
fn test_ion_unknown_subcommand_fails_with_clap_error() {
    let output = ion()
        .arg("frobnicate")
        .output()
        .expect("Failed to run ion with bad subcommand");
    assert!(
        !output.status.success(),
        "unknown subcommand must be a parse error"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("frobnicate") || stderr.to_lowercase().contains("unrecognized"),
        "clap error should name the bad subcommand: {stderr}"
    );
}

#[test]
fn test_ion_version_flag_prints_version_and_exits_zero() {
    let output = ion()
        .arg("--version")
        .output()
        .expect("Failed to run ion --version");
    assert!(
        output.status.success(),
        "`ion --version` must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "--version output should include the crate version: {stdout}"
    );
}
