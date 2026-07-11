//! CLI-level integration tests for `impulse-rs ion-verify` (TUI_SPEC.md T4).
//!
//! These drive the real `impulse-rs` binary as a subprocess (mirroring the
//! `cargo run -- ...` pattern in `tests/integration_enhancements.rs`) against
//! the fake-gate fixtures under `tests/fakes/`, via the `ION_GATE_LAUNCHER`
//! env override (T2). This is the layer above `src/handlers/ion.rs`'s
//! `run_ion_verify` unit tests: it exercises `handle_ion_verify`'s
//! exit-code mapping and `--json` `contract_violation` field end-to-end,
//! which an in-process test cannot observe because `handle_ion_verify` calls
//! `std::process::exit` on a failing/violating verdict (G1).
//!
//! T4 scenario 1 (pass) and scenario 4 (non-zero exit) are covered at the
//! `run_ion_verify` unit level (`src/handlers/ion.rs`) and are not repeated
//! here as subprocess tests, since both a failing verdict and a gate-side
//! error currently produce the same CLI-observable outcome (non-zero exit,
//! `main`'s default `Result` handling) — the interesting CLI-only surface is
//! the exit-code-vs-verdict mapping and the `--json` envelope, covered below.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn impulse_rs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Creates a throwaway git repo with one empty commit, so `--diff-ref HEAD`
/// resolves via `git rev-parse` inside `run_ion_verify`.
fn init_git_repo() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .status()
            .expect("failed to run git");
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "--quiet"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["commit", "--allow-empty", "--quiet", "-m", "init"]);
    dir
}

fn run_ion_verify_cli(repo: &Path, gate_script: &Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--"])
        .args(["ion-verify", "--repo"])
        .arg(repo)
        .args(["--diff-ref", "HEAD"])
        .args(extra_args)
        .current_dir(impulse_rs_dir())
        .env("ION_GATE_LAUNCHER", gate_script);
    cmd.output()
        .expect("failed to run `cargo run -- ion-verify`")
}

fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// T4 scenario 2 ("changes requested") at the CLI layer: `handle_ion_verify`
/// must map a non-passing verdict to a non-zero process exit code, and the
/// text-mode output must surface the verdict and finding — matching the
/// existing gate-command exit-code convention (`handlers::guard`).
#[test]
fn ion_verify_cli_exits_non_zero_on_changes_requested_verdict() {
    let repo = init_git_repo();
    let gate_script =
        impulse_rs_dir().join("tests/fakes/ion-verify-stub-gate-changes-requested.sh");

    let output = run_ion_verify_cli(repo.path(), &gate_script, &[]);

    assert!(
        !output.status.success(),
        "CLI must exit non-zero for a CHANGES REQUESTED verdict; stdout: {}",
        stdout_str(&output)
    );
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("ChangesRequested"),
        "expected verdict in stdout, got: {stdout}"
    );
    assert!(
        stdout.contains("off-by-one in loop bound"),
        "expected the stub gate's finding message in stdout, got: {stdout}"
    );
}

/// T4 scenario 3 ("contract-violating response") at the CLI layer:
/// `--json` output must populate the machine-readable `contract_violation`
/// field (G6) when the gate's response violates spec-a, and the process
/// must still exit non-zero.
#[test]
fn ion_verify_cli_json_reports_contract_violation_and_exits_non_zero() {
    let repo = init_git_repo();
    let gate_script =
        impulse_rs_dir().join("tests/fakes/ion-verify-stub-gate-contract-violation.sh");

    let output = run_ion_verify_cli(repo.path(), &gate_script, &["--json"]);

    assert!(
        !output.status.success(),
        "CLI must exit non-zero when the gate response violates the spec-a contract; stdout: {}",
        stdout_str(&output)
    );

    let stdout = stdout_str(&output);
    let json: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("--json output should be valid JSON ({e}): {stdout}"));

    assert_eq!(json["verdict"], "APPROVE");
    let violation = json["contract_violation"]
        .as_str()
        .expect("contract_violation should be a populated string, not null");
    assert!(
        violation.contains("CriticalBlocksApprove")
            || violation.to_lowercase().contains("critical"),
        "contract_violation should describe the critical/approve conflict, got: {violation}"
    );
}
