//! Findings handoff gate.
//!
//! One workflow, [`dispatch`], is what agents call (the `handoff` tool is a
//! thin wrapper). Every call returns a snapshot and the single allowed next
//! step. A failed check is [`WorkflowState::Pending`] with the exact
//! failures. Nothing in this module accepts a handoff and attaches a warning.
//!
//! # Subject
//!
//! The sender declares a subject key on the claim. The gate does not ask a
//! model whether the source is "about" that subject. It compares the key to
//! a subject the source itself records:
//!
//! - a file's `impulse-subject:` line in the first 64 KiB
//! - a commit message's `Impulse-Subject:` trailer
//! - a URL's host
//!
//! A claim about `billing.example.com` whose only source is
//! `https://auth.example.com/...` is refused, even when the sender recorded
//! a real fetch of that URL.
//!
//! # Stale shared writes
//!
//! Two agents writing the same artifact from the same read is not a citation
//! problem, so it is not a handoff-claim failure. `read_shared` /
//! `write_shared` on this same tool record the generation that was read and
//! refuse a write whose `based_on_generation` is not that current generation.
//! The markdown task note from [`crate::orchestration::write_handoff`] is
//! unchanged: it routes a task, it does not accept findings.

mod gate;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use types::{
    CheckKind, Claim, ClaimSource, GateFailure, HandoffError, NextStep, SubjectKey, WorkflowResult,
    WorkflowState, COMMIT_SUBJECT_TRAILER, FILE_SUBJECT_PREFIX, HANDOFF_GATE_FILE,
};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use gate::{evaluate_claims, RecordedFetch};
use store::{
    artifact_snapshot, handoff_snapshot, read_next_step, refuse_missing_artifact,
    stale_write_failure, with_store, FetchRecord, GateStore, HandoffPhase, SharedArtifact,
    StoredHandoff,
};
use types::{
    normalize_url, parse_digest, parse_rfc3339, validate_id, validate_relative_path,
    MAX_ARTIFACT_BYTES, MAX_CLAIMS,
};

/// Run one step of the handoff workflow.
///
/// `action` is `record_fetch`, `submit`, `status`, `read`, `read_shared`, or
/// `write_shared`. `now` is the clock for ledger timestamps; production
/// passes [`Utc::now`](chrono::Utc::now) and tests pass a fixed instant.
pub fn dispatch(
    action: &str,
    payload: &Value,
    impulse_dir: &std::path::Path,
    now: DateTime<Utc>,
) -> Result<WorkflowResult, HandoffError> {
    if !payload.is_object() {
        return Err(HandoffError::Invalid(
            "payload must be a JSON object".into(),
        ));
    }
    match action {
        "record_fetch" => record_fetch(payload, impulse_dir, now),
        "submit" => submit(payload, impulse_dir, now),
        "status" => load_handoff(payload, impulse_dir, HandoffView::Status),
        "read" => load_handoff(payload, impulse_dir, HandoffView::Read),
        "read_shared" => read_shared(payload, impulse_dir, now),
        "write_shared" => write_shared(payload, impulse_dir, now),
        other => unknown_action(other),
    }
}

fn unknown_action(action: &str) -> Result<WorkflowResult, HandoffError> {
    let name = if action.is_empty() {
        "(missing)".to_string()
    } else {
        action.to_string()
    };
    WorkflowResult::pending(
        json!({"kind": "handoff_tool", "action": name}),
        vec![GateFailure {
            target: name.clone(),
            check: CheckKind::Request,
            fix: format!(
                "unknown action `{name}`. Use one of: record_fetch, submit, status, read, read_shared, write_shared."
            ),
        }],
        NextStep {
            action: "submit".into(),
            detail: "Call handoff again with action `submit` for a findings report, or `read_shared` / `write_shared` for a shared artifact.".into(),
        },
    )
}

fn record_fetch(
    payload: &Value,
    impulse_dir: &std::path::Path,
    now: DateTime<Utc>,
) -> Result<WorkflowResult, HandoffError> {
    let sender = validate_id("sender", require_str(payload, "sender")?)?;
    let (url, host) = normalize_url(require_str(payload, "url")?)?;
    let status = require_status(payload)?;
    let body_digest = parse_digest(require_str(payload, "body_digest")?)?;
    let fetched_at = match payload.get("fetched_at").and_then(Value::as_str) {
        Some(raw) => parse_rfc3339(raw)?,
        None => now,
    };
    let record = FetchRecord {
        url: url.clone(),
        host: host.as_str().to_string(),
        status,
        body_digest,
        fetched_at,
        sender: sender.clone(),
    };
    with_store(impulse_dir, |store| {
        store.insert_fetch(record.clone())?;
        Ok(WorkflowResult::recorded(
            json!({
                "kind": "fetch",
                "url": record.url,
                "host": record.host,
                "status": record.status,
                "body_digest": record.body_digest,
                "sender": record.sender,
                "fetched_at": record.fetched_at,
            }),
            NextStep {
                action: "submit".into(),
                detail: format!(
                    "Fetch of `{}` is recorded for `{sender}`. Submit the handoff with action `submit`.",
                    record.url
                ),
            },
        ))
    })
}

fn submit(
    payload: &Value,
    impulse_dir: &std::path::Path,
    now: DateTime<Utc>,
) -> Result<WorkflowResult, HandoffError> {
    let id = validate_id("handoff id", require_str(payload, "id")?)?;
    let from_agent = validate_id("from_agent", require_str(payload, "from_agent")?)?;
    let to_agent = validate_id("to_agent", require_str(payload, "to_agent")?)?;
    let workspace = require_str(payload, "workspace")?.trim().to_string();
    if workspace.is_empty() {
        return Err(HandoffError::Invalid("workspace must not be empty".into()));
    }
    let (claims, mut failures) = parse_claims(payload.get("claims"), &id)?;
    let workspace_path = std::path::PathBuf::from(&workspace);

    with_store(impulse_dir, |store| {
        if let Some(existing) = store.handoff(&id) {
            if matches!(existing.phase, HandoffPhase::Accepted { .. }) {
                return already_accepted(existing);
            }
        }
        let sender = from_agent.clone();
        let resolved = evaluate_claims(&claims, &workspace_path, &sender, &|url| {
            store.fetch(&sender, url).map(|record| RecordedFetch {
                sender: record.sender.clone(),
                host: record.host.clone(),
            })
        });
        failures.extend(resolved);
        let handoff = StoredHandoff {
            id: id.clone(),
            from_agent: from_agent.clone(),
            to_agent: to_agent.clone(),
            workspace: workspace.clone(),
            claims: claims.clone(),
            phase: if failures.is_empty() {
                HandoffPhase::Accepted { accepted_at: now }
            } else {
                HandoffPhase::Pending {
                    failures: failures.clone(),
                }
            },
            updated_at: now,
        };
        let snapshot = handoff_snapshot(&handoff);
        if failures.is_empty() {
            store.insert_handoff(handoff)?;
            return Ok(WorkflowResult::accepted(
                snapshot,
                NextStep {
                    action: "read".into(),
                    detail: format!(
                        "Handoff `{id}` is accepted. `{to_agent}` may read it with action `read`."
                    ),
                },
            ));
        }
        let detail = failure_detail(&id, &failures);
        store.insert_handoff(handoff)?;
        WorkflowResult::pending(
            snapshot,
            failures,
            NextStep {
                action: "submit".into(),
                detail,
            },
        )
    })
}

fn already_accepted(existing: &StoredHandoff) -> Result<WorkflowResult, HandoffError> {
    WorkflowResult::pending(
        handoff_snapshot(existing),
        vec![GateFailure {
            target: existing.id.clone(),
            check: CheckKind::AlreadyAccepted,
            fix: format!(
                "handoff `{}` is already accepted and was not replaced. Read it with action `read`.",
                existing.id
            ),
        }],
        NextStep {
            action: "read".into(),
            detail: format!(
                "Handoff `{}` is already accepted. The new submit was refused.",
                existing.id
            ),
        },
    )
}

fn failure_detail(id: &str, failures: &[GateFailure]) -> String {
    let fixes: Vec<&str> = failures
        .iter()
        .map(|failure| failure.fix.as_str())
        .collect();
    format!(
        "Handoff `{id}` is pending and was not accepted. Fix {} failure(s) and submit again: {}",
        failures.len(),
        fixes.join(" ")
    )
}

#[derive(Clone, Copy)]
enum HandoffView {
    Status,
    Read,
}

fn load_handoff(
    payload: &Value,
    impulse_dir: &std::path::Path,
    view: HandoffView,
) -> Result<WorkflowResult, HandoffError> {
    let id = validate_id("handoff id", require_str(payload, "id")?)?;
    with_store(impulse_dir, |store| match store.handoff(&id) {
        None => WorkflowResult::pending(
            json!({"kind": "handoff", "id": id, "present": false, "phase": "pending"}),
            vec![GateFailure {
                target: id.clone(),
                check: CheckKind::HandoffExists,
                fix: format!("no handoff `{id}` is stored. Submit it with action `submit`."),
            }],
            NextStep {
                action: "submit".into(),
                detail: format!("Submit handoff `{id}` with action `submit`."),
            },
        ),
        Some(handoff) => view_handoff(handoff, view),
    })
}

fn view_handoff(
    handoff: &StoredHandoff,
    view: HandoffView,
) -> Result<WorkflowResult, HandoffError> {
    let snapshot = handoff_snapshot(handoff);
    match &handoff.phase {
        HandoffPhase::Accepted { .. } => Ok(WorkflowResult::accepted(
            snapshot,
            NextStep {
                action: "none".into(),
                detail: format!(
                    "Handoff `{}` is accepted. No further gate step is required.",
                    handoff.id
                ),
            },
        )),
        HandoffPhase::Pending { failures } => {
            if failures.is_empty() {
                return Err(HandoffError::Corrupt(format!(
                    "handoff `{}` is pending with no failures",
                    handoff.id
                )));
            }
            let detail = match view {
                HandoffView::Read => format!(
                    "Handoff `{}` is pending and must not be treated as delivered. {}",
                    handoff.id,
                    failure_detail(&handoff.id, failures)
                ),
                HandoffView::Status => failure_detail(&handoff.id, failures),
            };
            WorkflowResult::pending(
                snapshot,
                failures.clone(),
                NextStep {
                    action: "submit".into(),
                    detail,
                },
            )
        }
    }
}

fn read_shared(
    payload: &Value,
    impulse_dir: &std::path::Path,
    now: DateTime<Utc>,
) -> Result<WorkflowResult, HandoffError> {
    let artifact_id = validate_relative_path("artifact_id", require_str(payload, "artifact_id")?)?;
    let reader = validate_id("reader", require_str(payload, "reader")?)?;
    with_store(impulse_dir, |store| {
        let Some(artifact) = store.artifact_mut(&artifact_id) else {
            return refuse_missing_artifact(&artifact_id);
        };
        artifact.reads.insert(
            reader.clone(),
            store::ReadMark {
                generation: artifact.generation,
                read_at: now,
            },
        );
        let generation = artifact.generation;
        Ok(WorkflowResult::recorded(
            artifact_snapshot(&artifact_id, artifact, "recorded"),
            NextStep {
                action: "write_shared".into(),
                detail: format!(
                    "`{reader}` read `{artifact_id}` at generation {generation}. A later write must set `based_on_generation` to {generation}."
                ),
            },
        ))
    })
}

fn write_shared(
    payload: &Value,
    impulse_dir: &std::path::Path,
    now: DateTime<Utc>,
) -> Result<WorkflowResult, HandoffError> {
    let artifact_id = validate_relative_path("artifact_id", require_str(payload, "artifact_id")?)?;
    let writer = validate_id("writer", require_str(payload, "writer")?)?;
    let based_on = require_u64(payload, "based_on_generation")?;
    let content = match payload.get("content").and_then(Value::as_str) {
        Some(content) => content.to_string(),
        None => {
            return Err(HandoffError::Invalid(
                "write_shared requires string `content`".into(),
            ));
        }
    };
    if content.len() > MAX_ARTIFACT_BYTES {
        return WorkflowResult::pending(
            json!({
                "kind": "shared_artifact",
                "artifact_id": artifact_id,
                "phase": "pending",
            }),
            vec![GateFailure {
                target: artifact_id,
                check: CheckKind::Request,
                fix: format!(
                    "content is {} bytes; the maximum is {MAX_ARTIFACT_BYTES}. Shorten the artifact and write again.",
                    content.len()
                ),
            }],
            NextStep {
                action: "write_shared".into(),
                detail: "Shorten `content` and call `write_shared` again.".into(),
            },
        );
    }

    with_store(impulse_dir, |store| {
        let Some(current) = store.artifact(&artifact_id).cloned() else {
            return create_artifact(store, &artifact_id, &writer, based_on, &content, now);
        };
        let read_generation = current.reads.get(&writer).map(|mark| mark.generation);
        let fresh = based_on == current.generation && read_generation == Some(current.generation);
        if !fresh {
            return WorkflowResult::pending(
                artifact_snapshot(&artifact_id, &current, "pending"),
                vec![stale_write_failure(
                    &artifact_id,
                    &writer,
                    based_on,
                    &current,
                    now,
                )],
                read_next_step(&artifact_id, current.generation),
            );
        }
        let next = SharedArtifact {
            generation: current.generation.saturating_add(1),
            content: content.clone(),
            last_writer: writer.clone(),
            last_written_at: now,
            reads: current.reads,
        };
        let generation = next.generation;
        let snapshot = artifact_snapshot(&artifact_id, &next, "accepted");
        store.insert_artifact(artifact_id.clone(), next)?;
        Ok(WorkflowResult::accepted(
            snapshot,
            NextStep {
                action: "read_shared".into(),
                detail: format!(
                    "`{writer}` wrote `{artifact_id}` at generation {generation}. Other writers must re-read before they write."
                ),
            },
        ))
    })
}

fn create_artifact(
    store: &mut GateStore,
    artifact_id: &str,
    writer: &str,
    based_on: u64,
    content: &str,
    now: DateTime<Utc>,
) -> Result<WorkflowResult, HandoffError> {
    if based_on != 0 {
        return WorkflowResult::pending(
            json!({
                "kind": "shared_artifact",
                "artifact_id": artifact_id,
                "present": false,
                "phase": "pending",
            }),
            vec![GateFailure {
                target: artifact_id.to_string(),
                check: CheckKind::StaleRead,
                fix: format!(
                    "shared artifact `{artifact_id}` does not exist, so basing a write on generation {based_on} is stale. Create it with `based_on_generation` 0."
                ),
            }],
            NextStep {
                action: "write_shared".into(),
                detail: format!(
                    "Create `{artifact_id}` with `based_on_generation` 0."
                ),
            },
        );
    }
    let artifact = SharedArtifact {
        generation: 1,
        content: content.to_string(),
        last_writer: writer.to_string(),
        last_written_at: now,
        reads: Default::default(),
    };
    let snapshot = artifact_snapshot(artifact_id, &artifact, "accepted");
    store.insert_artifact(artifact_id.to_string(), artifact)?;
    Ok(WorkflowResult::accepted(
        snapshot,
        NextStep {
            action: "read_shared".into(),
            detail: format!(
                "`{writer}` created `{artifact_id}` at generation 1. Another writer must read that generation before writing."
            ),
        },
    ))
}

fn parse_claims(
    value: Option<&Value>,
    handoff_id: &str,
) -> Result<(Vec<Claim>, Vec<GateFailure>), HandoffError> {
    let Some(value) = value else {
        return Err(HandoffError::Invalid(
            "submit requires a `claims` array. Each claim needs a subject and a file, commit, or URL source."
                .into(),
        ));
    };
    let Some(items) = value.as_array() else {
        return Err(HandoffError::Invalid("`claims` must be an array".into()));
    };
    if items.is_empty() {
        return Ok((
            Vec::new(),
            vec![GateFailure {
                target: handoff_id.to_string(),
                check: CheckKind::ClaimsPresent,
                fix: "handoff has no claims. Add at least one claim; each claim must name a subject and a file, commit, or URL source.".into(),
            }],
        ));
    }
    if items.len() > MAX_CLAIMS {
        return Err(HandoffError::Invalid(format!(
            "a handoff can carry at most {MAX_CLAIMS} claims"
        )));
    }
    let mut claims = Vec::new();
    let mut failures = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match parse_claim(item, index) {
            Ok(claim) => claims.push(claim),
            Err(failure) => failures.push(failure),
        }
    }
    Ok((claims, failures))
}

fn parse_claim(value: &Value, index: usize) -> Result<Claim, GateFailure> {
    let target = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("claim[{index}]"));
    if value.get("source").is_none() {
        return Err(GateFailure {
            target: target.clone(),
            check: CheckKind::SourceResolves,
            fix: format!(
                "claim `{target}` is missing `source`. Add a source of kind `file` (path), `commit` (hash), or `url` (url). An unsourced claim cannot be handed off."
            ),
        });
    }
    serde_json::from_value(value.clone()).map_err(|error| GateFailure {
        target,
        check: CheckKind::Request,
        fix: format!(
            "claim could not be read ({error}). Each claim needs id, statement, subject, and a file, commit, or URL source."
        ),
    })
}

fn require_str<'a>(payload: &'a Value, field: &str) -> Result<&'a str, HandoffError> {
    match payload.get(field) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(HandoffError::Invalid(format!(
            "`{field}` must be a non-empty string"
        ))),
        None => Err(HandoffError::Invalid(format!("missing `{field}`"))),
    }
}

fn require_status(payload: &Value) -> Result<u16, HandoffError> {
    let status = payload
        .get("status")
        .and_then(Value::as_u64)
        .ok_or_else(|| HandoffError::Invalid("fetch `status` must be an integer".into()))?;
    u16::try_from(status)
        .ok()
        .filter(|code| (100..=599).contains(code))
        .ok_or_else(|| {
            HandoffError::Invalid(format!(
                "fetch status {status} must be an HTTP status from 100 to 599"
            ))
        })
}

fn require_u64(payload: &Value, field: &str) -> Result<u64, HandoffError> {
    payload
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| HandoffError::Invalid(format!("`{field}` must be a non-negative integer")))
}
