//! CLI handler for `ion-verify` — drives the Ion verification gate (harness #2,
//! Pi on MiniMax) against a repo diff via `impulse_ion::pi_adapter`.
//!
//! This is Impulse acting as the *caller* side of the Ion contract
//! (`~/.ai-memory/docs/ion-harness/spec-a-harness-contract-v0.md`): it builds a
//! `HarnessRequest`, hands it to the already-promoted Pi gate, and reports the
//! `HarnessResponse` verdict — the operator-cockpit half of the boundary that
//! keeps a write-denied verifier decoupled from the primary coder.

use anyhow::{Context as AnyhowContext, Result};

use impulse_ion::pi_adapter::PiAdapter;
use impulse_ion::{HarnessRequest, HarnessResponse};

use super::print_json;

/// Pure core of `ion-verify` (TUI_SPEC.md T3 / gap G1): resolves the repo
/// path, sanity-checks `diff_ref` via `git rev-parse`, builds a spec-a
/// `HarnessRequest` (`HarnessRequest::verify`, T1/G4), validates it, and
/// drives it through the Ion Pi gate on a blocking thread (G2). Always
/// returns `Ok(response)` when the round trip succeeds — including when
/// `response.passed()` is `false` or the response itself violates the
/// spec-a contract. Pass/fail interpretation, printing, and exit-code
/// mapping are the caller's job (see [`handle_ion_verify`] below), which is
/// what lets this function be called in-process by a future chat tool
/// (T7's `ion_verify` ReplTool) or a fake-gate integration test (T4)
/// without going through `std::process::exit`.
pub async fn run_ion_verify(
    repo: Option<String>,
    diff_ref: String,
    description: String,
) -> Result<HarnessResponse> {
    let repo_path = repo.unwrap_or_else(|| ".".to_string());
    let repo_path = std::fs::canonicalize(&repo_path)
        .with_context(|| format!("Failed to resolve repo path: {repo_path}"))?;

    if !repo_path.join(".git").exists() {
        anyhow::bail!(
            "{} is not a git repository (no .git directory found)",
            repo_path.display()
        );
    }

    let rev_parse_status = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo_path)
        .arg("rev-parse")
        .arg("--quiet")
        .arg(&diff_ref)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to run `git rev-parse` for diff_ref: {diff_ref}"))?;

    if !rev_parse_status.success() {
        anyhow::bail!(
            "diff_ref {diff_ref} does not resolve in repo {}",
            repo_path.display()
        );
    }

    let request = HarnessRequest::verify(repo_path.display().to_string(), diff_ref, description);

    request
        .validate()
        .context("HarnessRequest failed spec-a contract validation")?;

    let response = tokio::task::spawn_blocking(move || {
        let adapter = PiAdapter::new();
        adapter.verify(&request)
    })
    .await
    .context("Ion Pi gate verify() blocking task panicked")?
    .context("Ion Pi gate verify() call failed")?;

    Ok(response)
}

/// `--json` output envelope (TUI_SPEC.md gap G6). Flattens the
/// `HarnessResponse` fields at the top level (via `#[serde(flatten)]`) and
/// appends `contract_violation` so a scripted caller can branch on a spec-a
/// contract violation without scraping stderr. Flattening — rather than
/// nesting under a `response` key — is the less invasive choice: existing
/// consumers reading `verdict`/`findings`/`request_id`/etc. at the top level
/// of the emitted JSON keep working unchanged; they simply gain one new
/// optional field.
#[derive(serde::Serialize)]
struct VerifyResponseJson<'a> {
    #[serde(flatten)]
    response: &'a HarnessResponse,
    contract_violation: Option<String>,
}

/// Handle `ion-verify` — thin CLI wrapper around [`run_ion_verify`]. Prints the
/// verdict (text or `--json`) and exits the process with status 1 if the gate
/// does not return a PASS verdict (spec-a: APPROVE with no CRITICAL finding)
/// or if the response itself violates the spec-a contract, matching this
/// repo's existing gate-command exit-code convention (see `handlers::guard`).
pub async fn handle_ion_verify(
    repo: Option<String>,
    diff_ref: String,
    description: String,
    json: bool,
) -> Result<()> {
    let response = run_ion_verify(repo, diff_ref, description).await?;

    let contract_violation = response
        .validate()
        .err()
        .map(|violation| violation.to_string());
    if let Some(violation) = &contract_violation {
        eprintln!("Warning: gate response violated the spec-a contract: {violation}");
    }

    if json {
        print_json(&VerifyResponseJson {
            response: &response,
            contract_violation: contract_violation.clone(),
        })?;
    } else {
        print_response_text(&response);
    }

    if !response.passed() || contract_violation.is_some() {
        std::process::exit(1);
    }

    Ok(())
}

fn print_response_text(response: &impulse_ion::HarnessResponse) {
    println!("Ion gate verdict: {:?}", response.verdict);
    if response.findings.is_empty() {
        println!("No findings.");
    } else {
        for finding in &response.findings {
            println!(
                "  [{:?}] {}:{} ({}) — {}",
                finding.severity, finding.file, finding.line, finding.category, finding.message
            );
        }
    }
    for run in &response.commands_run {
        println!("  ran: {} (exit {})", run.command, run.exit_code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV;
    use impulse_ion::{CommandRun, Finding, HarnessResponse, Metrics, Severity, Verdict};

    /// Serializes tests that mutate the process-global `ION_GATE_LAUNCHER` env
    /// var, since `cargo test` runs unit tests in the same process on
    /// multiple threads by default. Mirrors the identical helper in
    /// `impulse-ion/src/pi_adapter.rs`'s own env-override tests (T2).
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Creates a throwaway git repo with one empty commit, so `diff_ref`
    /// values like `HEAD` resolve via `git rev-parse`.
    fn init_git_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("failed to create tempdir");
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
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

    /// T3 item 4: proves `run_ion_verify` returns a `HarnessResponse` without
    /// going through the CLI print/exit path, by driving `PiAdapter` against
    /// the stub gate script under `tests/fakes/` (via the `ION_GATE_LAUNCHER`
    /// env override that `PiAdapter::new()` already respects, T2) instead of
    /// the real MiniMax-backed gate. This is a lighter-weight substitute for
    /// T4's dedicated fake-gate integration test suite (pass / changes
    /// requested / contract-violation / non-zero-exit / timeout), which is
    /// out of scope here — it only needs to prove the pure function's return
    /// path works end-to-end through the real adapter/spawn_blocking wiring.
    #[tokio::test]
    async fn run_ion_verify_returns_response_via_stub_gate() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate.sh");
        std::env::set_var(ION_GATE_LAUNCHER_ENV, &stub_gate);

        let result = run_ion_verify(
            Some(repo.path().display().to_string()),
            "HEAD".to_string(),
            "Test description".to_string(),
        )
        .await;

        std::env::remove_var(ION_GATE_LAUNCHER_ENV);

        let response = result.expect("run_ion_verify should succeed against the stub gate");
        assert_eq!(response.verdict, Verdict::Approve);
        assert!(response.passed());
        assert_eq!(response.commands_run.len(), 1);
        assert!(response.validate().is_ok());
    }

    /// T4 scenario 2 ("changes requested"): the gate can run cleanly and
    /// still flag a real issue. `run_ion_verify` must return `Ok(response)`
    /// (not an `Err`) — pass/fail interpretation is the caller's job per G1
    /// — and the response itself must report `passed() == false` with a
    /// non-empty findings list, while still satisfying the spec-a contract
    /// (`validate()` succeeds — this is not a contract-violation case).
    #[tokio::test]
    async fn run_ion_verify_returns_changes_requested_response_via_stub_gate() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate-changes-requested.sh");
        std::env::set_var(ION_GATE_LAUNCHER_ENV, &stub_gate);

        let result = run_ion_verify(
            Some(repo.path().display().to_string()),
            "HEAD".to_string(),
            "Test description".to_string(),
        )
        .await;

        std::env::remove_var(ION_GATE_LAUNCHER_ENV);

        let response = result.expect("run_ion_verify should return Ok even for a failing verdict");
        assert_eq!(response.verdict, Verdict::ChangesRequested);
        assert!(!response.passed());
        assert!(!response.findings.is_empty());
        assert!(
            response.validate().is_ok(),
            "a CHANGES REQUESTED verdict with a WARNING finding does not violate spec-a"
        );
    }

    /// T4 scenario 3 ("contract-violating response"): the gate can emit a
    /// syntactically valid `HarnessResponse` that nonetheless violates
    /// spec-a's invariants (here: `verdict: APPROVE` with a CRITICAL finding
    /// present). `run_ion_verify` must still return `Ok(response)` — parsing
    /// succeeded — while a caller invoking `response.validate()` (as
    /// `handle_ion_verify` does to populate `contract_violation`) must
    /// detect the violation.
    #[tokio::test]
    async fn run_ion_verify_returns_contract_violating_response_ok_but_validate_fails() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate-contract-violation.sh");
        std::env::set_var(ION_GATE_LAUNCHER_ENV, &stub_gate);

        let result = run_ion_verify(
            Some(repo.path().display().to_string()),
            "HEAD".to_string(),
            "Test description".to_string(),
        )
        .await;

        std::env::remove_var(ION_GATE_LAUNCHER_ENV);

        let response = result
            .expect("run_ion_verify should return Ok; contract validation is the caller's job");
        assert_eq!(response.verdict, Verdict::Approve);
        assert!(
            !response.passed(),
            "passed() must also be false: a CRITICAL finding is present"
        );
        assert_eq!(
            response.validate(),
            Err(impulse_ion::ContractViolation::CriticalBlocksApprove(
                Verdict::Approve
            ))
        );
    }

    /// T4 scenario 4 ("non-zero exit"): the gate process can crash before
    /// emitting any parseable response. `run_ion_verify` must propagate this
    /// as an `Err` whose chain contains `AdapterError::NonZeroExit`, not
    /// silently return a default/empty `HarnessResponse`.
    #[tokio::test]
    async fn run_ion_verify_returns_err_on_nonzero_exit() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate-nonzero-exit.sh");
        std::env::set_var(ION_GATE_LAUNCHER_ENV, &stub_gate);

        let result = run_ion_verify(
            Some(repo.path().display().to_string()),
            "HEAD".to_string(),
            "Test description".to_string(),
        )
        .await;

        std::env::remove_var(ION_GATE_LAUNCHER_ENV);

        let err = result.expect_err("a crashing gate process must surface as an error");
        let found_non_zero_exit = err.chain().any(|cause| {
            matches!(
                cause.downcast_ref::<impulse_ion::pi_adapter::AdapterError>(),
                Some(impulse_ion::pi_adapter::AdapterError::NonZeroExit { code: 7, .. })
            )
        });
        assert!(
            found_non_zero_exit,
            "expected AdapterError::NonZeroExit{{code: 7, ..}} in the error chain, got: {err:?}"
        );
    }

    /// T3 item 2/G6: proves the `--json` envelope flattens `HarnessResponse`
    /// fields and includes a machine-readable `contract_violation` alongside
    /// them, rather than only warning on stderr.
    #[test]
    fn verify_response_json_flattens_fields_and_includes_contract_violation() {
        let response = passing_response();
        let envelope = VerifyResponseJson {
            response: &response,
            contract_violation: Some("verdict Approve forbidden".to_string()),
        };

        let json = serde_json::to_value(&envelope).expect("envelope should serialize");
        assert_eq!(json["verdict"], "APPROVE");
        assert_eq!(json["request_id"], "req-test");
        assert_eq!(json["contract_violation"], "verdict Approve forbidden");
    }

    /// Same envelope with no violation — the field must still be present
    /// (as JSON `null`), not silently dropped, so scripted callers can rely
    /// on the key always existing.
    #[test]
    fn verify_response_json_includes_null_contract_violation_when_absent() {
        let response = passing_response();
        let envelope = VerifyResponseJson {
            response: &response,
            contract_violation: None,
        };

        let json = serde_json::to_value(&envelope).expect("envelope should serialize");
        assert!(json.get("contract_violation").is_some());
        assert!(json["contract_violation"].is_null());
    }

    fn passing_response() -> HarnessResponse {
        HarnessResponse {
            contract_version: impulse_ion::CONTRACT_VERSION.to_string(),
            request_id: "req-test".to_string(),
            verdict: Verdict::Approve,
            findings: vec![Finding {
                severity: Severity::Note,
                category: "style".to_string(),
                file: "src/lib.rs".to_string(),
                line: 1,
                message: "looks fine".to_string(),
            }],
            commands_run: vec![CommandRun {
                command: "cargo test".to_string(),
                exit_code: 0,
                output_ref: "log-1".to_string(),
            }],
            output_logs: Default::default(),
            metrics: Metrics::default(),
        }
    }

    #[test]
    fn print_response_text_does_not_panic_on_passing_response() {
        // Smoke-checks the formatting path itself; the exit-code branch in
        // handle_ion_verify (std::process::exit) is exercised via
        // response.passed() in the dedicated contract tests in impulse-ion,
        // not here, since exiting the test process is not observable.
        print_response_text(&passing_response());
        assert!(passing_response().passed());
    }

    #[test]
    fn print_response_text_does_not_panic_on_empty_findings() {
        let mut response = passing_response();
        response.findings.clear();
        print_response_text(&response);
        assert!(response.findings.is_empty());
    }
}
