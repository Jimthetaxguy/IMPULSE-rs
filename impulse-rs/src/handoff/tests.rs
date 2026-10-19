use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::json;
use tempfile::TempDir;

use super::{dispatch, CheckKind, Claim, ClaimSource, WorkflowState, HANDOFF_GATE_FILE};

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn ledger_dir() -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().expect("tempdir");
    let impulse = temp.path().join(".impulse");
    (temp, impulse)
}

fn at(minutes: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap() + Duration::minutes(minutes)
}

fn record_fetch(impulse: &Path, sender: &str, url: &str, now: DateTime<Utc>) {
    let result = dispatch(
        "record_fetch",
        &json!({
            "sender": sender,
            "url": url,
            "status": 200,
            "body_digest": DIGEST,
        }),
        impulse,
        now,
    )
    .expect("record fetch");
    assert!(
        !result.accepted,
        "recording a fetch is not a handoff acceptance"
    );
    assert_eq!(result.state, WorkflowState::Recorded);
    assert_eq!(result.next_step.action, "submit");
    assert!(result.failures.is_empty());
}

#[test]
fn test_submit_refuses_claim_sourced_from_a_different_domain() {
    let (temp, impulse) = ledger_dir();
    let now = at(0);
    record_fetch(
        &impulse,
        "agent-a",
        "https://auth.example.com/docs/refunds",
        now,
    );

    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {
                    "kind": "url",
                    "url": "https://auth.example.com/docs/refunds"
                }
            }]
        }),
        &impulse,
        now,
    )
    .expect("submit returns a workflow result");

    assert!(
        !result.accepted,
        "a domain mismatch must not be accepted, with or without a warning"
    );
    assert_eq!(result.state, WorkflowState::Pending);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].check, CheckKind::SubjectMatches);
    assert_eq!(result.failures[0].target, "refund-cap");
    assert!(result.failures[0].fix.contains("billing.example.com"));
    assert!(result.failures[0].fix.contains("auth.example.com"));
    assert_eq!(result.next_step.action, "submit");
    assert_eq!(result.snapshot["phase"]["status"], "pending");

    let stored = fs::read_to_string(impulse.join(HANDOFF_GATE_FILE)).expect("ledger");
    assert!(stored.contains("\"status\": \"pending\""));
    assert!(
        !stored.contains("\"status\": \"accepted\""),
        "the refused handoff must not be stored as accepted"
    );

    let status = dispatch("status", &json!({"id": "refund-report"}), &impulse, now).unwrap();
    assert!(!status.accepted);
    assert_eq!(status.state, WorkflowState::Pending);
    assert_eq!(status.failures[0].check, CheckKind::SubjectMatches);
}

#[test]
fn test_submit_accepts_clean_handoff_whose_source_matches_the_claim() {
    let (temp, impulse) = ledger_dir();
    let now = at(0);
    record_fetch(
        &impulse,
        "agent-a",
        "https://billing.example.com/docs/refunds",
        now,
    );

    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {
                    "kind": "url",
                    "url": "https://billing.example.com/docs/refunds"
                }
            }]
        }),
        &impulse,
        now,
    )
    .expect("clean submit");

    assert!(result.accepted);
    assert_eq!(result.state, WorkflowState::Accepted);
    assert!(result.failures.is_empty());
    assert_eq!(result.next_step.action, "read");
    assert_eq!(result.snapshot["phase"]["status"], "accepted");
    assert_eq!(
        result.snapshot["claims"][0]["statement"],
        "Refunds over $50 need a second approver."
    );

    let read = dispatch("read", &json!({"id": "refund-report"}), &impulse, now).unwrap();
    assert!(read.accepted);
    assert_eq!(read.state, WorkflowState::Accepted);
    assert_eq!(read.next_step.action, "none");
}

#[test]
fn test_write_shared_refuses_second_writer_based_on_a_stale_read() {
    let (_temp, impulse) = ledger_dir();
    let created = dispatch(
        "write_shared",
        &json!({
            "artifact_id": "findings.md",
            "writer": "seed",
            "based_on_generation": 0,
            "content": "shared findings v1"
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(created.accepted);
    assert_eq!(created.snapshot["generation"], 1);

    for reader in ["agent-a", "agent-b"] {
        let read = dispatch(
            "read_shared",
            &json!({"artifact_id": "findings.md", "reader": reader}),
            &impulse,
            at(1),
        )
        .unwrap();
        assert_eq!(read.state, WorkflowState::Recorded);
        assert!(!read.accepted);
        assert_eq!(read.snapshot["generation"], 1);
        assert_eq!(read.snapshot["content"], "shared findings v1");
    }

    let first = dispatch(
        "write_shared",
        &json!({
            "artifact_id": "findings.md",
            "writer": "agent-a",
            "based_on_generation": 1,
            "content": "billing notes from agent-a"
        }),
        &impulse,
        at(2),
    )
    .unwrap();
    assert!(
        first.accepted,
        "the writer who just read generation 1 may write"
    );
    assert_eq!(first.snapshot["generation"], 2);

    let second = dispatch(
        "write_shared",
        &json!({
            "artifact_id": "findings.md",
            "writer": "agent-b",
            "based_on_generation": 1,
            "content": "stale rewrite from agent-b"
        }),
        &impulse,
        at(5),
    )
    .unwrap();

    assert!(!second.accepted);
    assert_eq!(second.state, WorkflowState::Pending);
    assert_eq!(second.failures.len(), 1);
    assert_eq!(second.failures[0].check, CheckKind::StaleRead);
    assert!(second.failures[0].fix.contains("generation 1"));
    assert!(second.failures[0].fix.contains("generation 2"));
    assert!(second.failures[0].fix.contains("3 minutes"));
    assert!(second.failures[0].fix.contains("agent-a"));
    assert!(second.failures[0].fix.contains("Re-read"));
    assert_eq!(second.next_step.action, "read_shared");
    assert_eq!(second.snapshot["content"], "billing notes from agent-a");
    assert_ne!(second.snapshot["content"], "stale rewrite from agent-b");

    let witness = dispatch(
        "read_shared",
        &json!({"artifact_id": "findings.md", "reader": "witness"}),
        &impulse,
        at(6),
    )
    .unwrap();
    assert_eq!(witness.snapshot["generation"], 2);
    assert_eq!(witness.snapshot["content"], "billing notes from agent-a");
    assert_eq!(witness.snapshot["last_writer"], "agent-a");
}

#[test]
fn test_submit_refuses_url_claim_without_a_recorded_fetch() {
    let (temp, impulse) = ledger_dir();
    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "url", "url": "https://billing.example.com/docs/refunds"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.state, WorkflowState::Pending);
    assert_eq!(result.failures[0].check, CheckKind::SourceResolves);
    assert!(result.failures[0].fix.contains("record_fetch"));
    assert!(result.failures[0].fix.contains("billing.example.com"));
}

#[test]
fn test_submit_refuses_claim_missing_source_field() {
    let (temp, impulse) = ledger_dir();
    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com"
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.failures[0].check, CheckKind::SourceResolves);
    assert!(result.failures[0].fix.contains("missing `source`"));
    assert_eq!(result.snapshot["phase"]["status"], "pending");
}

#[test]
fn test_submit_refuses_file_claim_whose_recorded_subject_differs() {
    let (temp, impulse) = ledger_dir();
    let workspace = temp.path();
    fs::create_dir(workspace.join("notes")).unwrap();
    fs::write(
        workspace.join("notes/auth.md"),
        "impulse-subject: auth.example.com\nAuth owns sessions.\n",
    )
    .unwrap();

    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": workspace,
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "file", "path": "notes/auth.md"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.failures[0].check, CheckKind::SubjectMatches);
    assert!(result.failures[0].fix.contains("billing.example.com"));
    assert!(result.failures[0].fix.contains("auth.example.com"));
    assert!(result.failures[0].fix.contains("notes/auth.md"));
}

#[test]
fn test_submit_refuses_file_claim_when_the_file_does_not_exist() {
    let (temp, impulse) = ledger_dir();
    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "file", "path": "notes/missing.md"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.failures[0].check, CheckKind::SourceResolves);
    assert!(result.failures[0].fix.contains("does not exist"));
    assert!(result.failures[0].fix.contains("notes/missing.md"));
}

#[test]
fn test_submit_accepts_file_claim_whose_recorded_subject_matches() {
    let (temp, impulse) = ledger_dir();
    let workspace = temp.path();
    fs::write(
        workspace.join("billing.md"),
        "impulse-subject: billing.example.com\nRefunds over $50 need a second approver.\n",
    )
    .unwrap();
    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": workspace,
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "file", "path": "billing.md"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(
        result.accepted,
        "matching file subject should pass: {result:?}"
    );
    assert!(result.failures.is_empty());
}

#[test]
fn test_submit_refuses_a_path_that_escapes_the_workspace() {
    let (temp, impulse) = ledger_dir();
    let result = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "file", "path": "../secret.md"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.state, WorkflowState::Pending);
    assert!(
        result.failures[0].fix.contains(".."),
        "the refusal must name the escaping path: {}",
        result.failures[0].fix
    );
}

#[test]
fn test_submit_refuses_commit_that_does_not_exist_and_accepts_one_that_matches() {
    let (temp, impulse) = ledger_dir();
    let repo = temp.path().canonicalize().unwrap();
    git(&repo, &["init", "--initial-branch=main"]);
    git(&repo, &["config", "user.email", "handoff@example.invalid"]);
    git(&repo, &["config", "user.name", "Handoff Gate"]);
    fs::write(
        repo.join("policy.md"),
        "impulse-subject: billing.example.com\n",
    )
    .unwrap();
    git(&repo, &["add", "policy.md"]);
    git(
        &repo,
        &[
            "commit",
            "-m",
            "record billing policy",
            "-m",
            "Impulse-Subject: billing.example.com",
        ],
    );
    let hash = git(&repo, &["rev-parse", "HEAD"]);

    let missing = dispatch(
        "submit",
        &json!({
            "id": "missing-commit",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": repo,
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "commit", "hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!missing.accepted);
    assert_eq!(missing.failures[0].check, CheckKind::SourceResolves);
    assert!(missing.failures[0].fix.contains("does not exist"));

    let mismatch = dispatch(
        "submit",
        &json!({
            "id": "wrong-subject",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": repo,
            "claims": [{
                "id": "refund-cap",
                "statement": "Sessions expire after 30 minutes.",
                "subject": "auth.example.com",
                "source": {"kind": "commit", "hash": hash}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(!mismatch.accepted);
    assert_eq!(mismatch.failures[0].check, CheckKind::SubjectMatches);
    assert!(mismatch.failures[0].fix.contains("auth.example.com"));
    assert!(mismatch.failures[0].fix.contains("billing.example.com"));

    let accepted = dispatch(
        "submit",
        &json!({
            "id": "billing-commit",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": repo,
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "commit", "hash": hash}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(
        accepted.accepted,
        "commit whose trailer matches should pass: {accepted:?}"
    );
    assert!(accepted.failures.is_empty());
}

#[test]
fn test_submit_of_an_accepted_handoff_does_not_replace_it() {
    let (temp, impulse) = ledger_dir();
    record_fetch(
        &impulse,
        "agent-a",
        "https://billing.example.com/docs/refunds",
        at(0),
    );
    let first = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "Refunds over $50 need a second approver.",
                "subject": "billing.example.com",
                "source": {"kind": "url", "url": "https://billing.example.com/docs/refunds"}
            }]
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    assert!(first.accepted);

    let again = dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": [{
                "id": "refund-cap",
                "statement": "A different unsourced story.",
                "subject": "auth.example.com",
                "source": {"kind": "url", "url": "https://auth.example.com/docs/refunds"}
            }]
        }),
        &impulse,
        at(1),
    )
    .unwrap();
    assert!(!again.accepted);
    assert_eq!(again.failures[0].check, CheckKind::AlreadyAccepted);
    assert_eq!(again.snapshot["phase"]["status"], "accepted");
    assert_eq!(
        again.snapshot["claims"][0]["statement"],
        "Refunds over $50 need a second approver."
    );
}

#[test]
fn test_read_of_a_pending_handoff_is_not_acceptance() {
    let (temp, impulse) = ledger_dir();
    dispatch(
        "submit",
        &json!({
            "id": "refund-report",
            "from_agent": "agent-a",
            "to_agent": "agent-b",
            "workspace": temp.path(),
            "claims": []
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    let read = dispatch("read", &json!({"id": "refund-report"}), &impulse, at(1)).unwrap();
    assert!(!read.accepted);
    assert_eq!(read.state, WorkflowState::Pending);
    assert_eq!(read.failures[0].check, CheckKind::ClaimsPresent);
    assert_eq!(read.next_step.action, "submit");
}

#[test]
fn test_write_shared_without_a_read_is_refused() {
    let (_temp, impulse) = ledger_dir();
    dispatch(
        "write_shared",
        &json!({
            "artifact_id": "findings.md",
            "writer": "seed",
            "based_on_generation": 0,
            "content": "shared findings v1"
        }),
        &impulse,
        at(0),
    )
    .unwrap();
    let write = dispatch(
        "write_shared",
        &json!({
            "artifact_id": "findings.md",
            "writer": "agent-b",
            "based_on_generation": 1,
            "content": "unseen rewrite"
        }),
        &impulse,
        at(1),
    )
    .unwrap();
    assert!(!write.accepted);
    assert_eq!(write.failures[0].check, CheckKind::StaleRead);
    assert!(write.failures[0].fix.contains("no recorded read"));
    assert_eq!(write.snapshot["content"], "shared findings v1");
}

#[test]
fn test_dispatch_store_error_when_impulse_dir_is_a_file() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("not-a-directory");
    fs::write(&file, "x").unwrap();
    let error = dispatch("status", &json!({"id": "refund-report"}), &file, at(0)).unwrap_err();
    assert!(
        format!("{error}").contains("handoff store"),
        "store failure must stay an error, got {error}"
    );
}

#[test]
fn test_unknown_action_is_pending_and_names_the_actions() {
    let (_temp, impulse) = ledger_dir();
    let result = dispatch("approve", &json!({}), &impulse, at(0)).unwrap();
    assert!(!result.accepted);
    assert_eq!(result.state, WorkflowState::Pending);
    assert_eq!(result.failures[0].check, CheckKind::Request);
    assert!(result.failures[0].fix.contains("record_fetch"));
    assert!(result.failures[0].fix.contains("write_shared"));
}

#[test]
fn test_claim_source_round_trip_for_each_kind() {
    for source in [
        ClaimSource::File {
            path: "notes/billing.md".into(),
        },
        ClaimSource::Commit {
            hash: "0123456789abcdef".into(),
        },
        ClaimSource::Url {
            url: "https://billing.example.com/docs".into(),
        },
    ] {
        let claim = Claim::try_new(
            "refund-cap",
            "Refunds over $50 need a second approver.",
            "billing.example.com",
            source,
        )
        .unwrap();
        let json = serde_json::to_string(&claim).unwrap();
        let recovered: Claim = serde_json::from_str(&json).unwrap();
        assert_eq!(claim, recovered);
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_string()
}

#[test]
fn test_two_senders_can_submit_the_same_url_after_reopening_the_ledger() {
    let (temp, impulse) = ledger_dir();
    let url = "https://billing.example.com/refunds";
    record_fetch(&impulse, "agent-a", url, at(0));
    record_fetch(&impulse, "agent-b", url, at(1));
    for sender in ["agent-a", "agent-b"] {
        let result = dispatch(
            "submit",
            &json!({
                "id": format!("report-{sender}"), "from_agent": sender, "to_agent": "reader",
                "workspace": temp.path(), "claims": [{
                    "id": "refunds", "statement": "Refunds need approval.",
                    "subject": "billing.example.com", "source": {"kind": "url", "url": url}
                }]
            }),
            &impulse,
            at(2),
        )
        .unwrap();
        assert!(result.accepted, "{sender}: {:?}", result.failures);
    }
}
