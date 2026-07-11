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
use impulse_ion::{Context as IonContext, HarnessRequest, Intent, RepoRef, Task};

use super::print_json;

/// Handle `ion-verify` — build a `HarnessRequest` for `diff_ref` in `repo` and
/// run it through the Ion Pi gate. Exits the process with status 1 if the gate
/// does not return a PASS verdict (spec-a: APPROVE with no CRITICAL finding),
/// matching this repo's existing gate-command exit-code convention (see
/// `handlers::guard`).
pub async fn handle_ion_verify(
    repo: Option<String>,
    diff_ref: String,
    description: String,
    json: bool,
) -> Result<()> {
    let repo_path = repo.unwrap_or_else(|| ".".to_string());
    let repo_path = std::fs::canonicalize(&repo_path)
        .with_context(|| format!("Failed to resolve repo path: {repo_path}"))?;

    let request = HarnessRequest {
        contract_version: impulse_ion::CONTRACT_VERSION.to_string(),
        request_id: format!("req-ion-verify-{}", uuid::Uuid::new_v4()),
        intent: Intent::Verify,
        repo: RepoRef {
            path: repo_path.display().to_string(),
            diff_ref: Some(diff_ref),
            diff_inline: None,
        },
        task: Task {
            description,
            verdict_priority: vec![
                "correctness".into(),
                "security".into(),
                "style".into(),
                "performance".into(),
            ],
        },
        capability_allowlist: vec!["read", "grep", "find", "ls", "build", "test"]
            .into_iter()
            .map(String::from)
            .collect(),
        model_role: "verifier-cheap".to_string(),
        context: IonContext {
            read_only: true,
            payload: vec![],
        },
    };

    request
        .validate()
        .context("HarnessRequest failed spec-a contract validation")?;

    let adapter = PiAdapter::new();
    let response = adapter
        .verify(&request)
        .context("Ion Pi gate verify() call failed")?;

    if let Err(violation) = response.validate() {
        eprintln!("Warning: gate response violated the spec-a contract: {violation}");
    }

    if json {
        print_json(&response)?;
    } else {
        print_response_text(&response);
    }

    if !response.passed() {
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
    use impulse_ion::{CommandRun, Finding, HarnessResponse, Metrics, Severity, Verdict};

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
