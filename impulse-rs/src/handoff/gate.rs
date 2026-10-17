//! Deterministic checks for a findings handoff.
//!
//! No model is involved. A file resolves when it exists inside the workspace
//! and carries an `impulse-subject:` line. A commit resolves when `git`
//! confirms the object and the message carries an `Impulse-Subject:` trailer.
//! A URL resolves when the sender previously recorded a fetch for that URL.
//! The claim's declared subject must equal the subject those sources record
//! (the file line, the trailer, or the URL host).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::types::{
    validate_commit_hash, CheckKind, Claim, ClaimSource, GateFailure, SubjectKey,
    COMMIT_SUBJECT_TRAILER, FILE_SUBJECT_PREFIX, SUBJECT_SCAN_BYTES,
};

/// Fetch the gate is willing to treat as evidence that the sender retrieved
/// this URL. The host is recomputed from the URL at check time; this copy is
/// only used to detect a ledger row that no longer matches its URL.
#[derive(Debug, Clone)]
pub(crate) struct RecordedFetch {
    pub sender: String,
    pub host: String,
}

pub(crate) fn evaluate_claims(
    claims: &[Claim],
    workspace: &Path,
    sender: &str,
    fetch_for: &dyn Fn(&str) -> Option<RecordedFetch>,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();
    for claim in claims {
        if let Some(failure) = evaluate_claim(claim, workspace, sender, fetch_for) {
            failures.push(failure);
        }
    }
    failures
}

fn evaluate_claim(
    claim: &Claim,
    workspace: &Path,
    sender: &str,
    fetch_for: &dyn Fn(&str) -> Option<RecordedFetch>,
) -> Option<GateFailure> {
    match claim.source() {
        ClaimSource::File { path } => evaluate_file(claim, workspace, path),
        ClaimSource::Commit { hash } => evaluate_commit(claim, workspace, hash),
        ClaimSource::Url { url } => evaluate_url(claim, url, sender, fetch_for),
    }
}

fn evaluate_file(claim: &Claim, workspace: &Path, relative: &str) -> Option<GateFailure> {
    let Some(root) = canonicalize_dir(workspace) else {
        return Some(resolve_failure(
            claim,
            format!(
                "workspace `{}` does not exist, so file `{relative}` cannot be resolved. Cite a file inside an existing workspace.",
                workspace.display()
            ),
        ));
    };
    let candidate = root.join(relative);
    if !candidate.exists() {
        return Some(resolve_failure(
            claim,
            format!(
                "file `{relative}` does not exist under the workspace. Cite a file that exists, and put `impulse-subject: {}` in it only if the file is about that subject.",
                claim.subject()
            ),
        ));
    }
    let Ok(canonical) = candidate.canonicalize() else {
        return Some(resolve_failure(
            claim,
            format!("file `{relative}` could not be resolved inside the workspace"),
        ));
    };
    if !canonical.starts_with(&root) {
        return Some(resolve_failure(
            claim,
            format!(
                "file `{relative}` resolves outside the workspace. Cite a file inside the workspace."
            ),
        ));
    }
    if !canonical.is_file() {
        return Some(resolve_failure(
            claim,
            format!("file `{relative}` is not a regular file"),
        ));
    }
    match read_recorded_subject(&canonical, FILE_SUBJECT_PREFIX) {
        Ok(Some(found)) => subject_mismatch(claim, &format!("file `{relative}`"), &found),
        Ok(None) => Some(resolve_failure(
            claim,
            format!(
                "file `{relative}` has no recorded subject in its first {} bytes. Add a line `{FILE_SUBJECT_PREFIX} {}` only if this file is about that subject, or cite a different source.",
                SUBJECT_SCAN_BYTES,
                claim.subject()
            ),
        )),
        Err(message) => Some(resolve_failure(claim, message)),
    }
}

fn evaluate_commit(claim: &Claim, workspace: &Path, hash: &str) -> Option<GateFailure> {
    if validate_commit_hash(hash).is_err() {
        return Some(resolve_failure(
            claim,
            format!("commit hash `{hash}` must be 7..=40 lowercase hex characters"),
        ));
    }
    if canonicalize_dir(workspace).is_none() {
        return Some(resolve_failure(
            claim,
            format!(
                "workspace `{}` does not exist, so commit `{hash}` cannot be resolved",
                workspace.display()
            ),
        ));
    }
    let spec = format!("{hash}^{{commit}}");
    match git(
        workspace,
        &["rev-parse", "--verify", "--end-of-options", &spec],
    ) {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            return Some(resolve_failure(
                claim,
                format!(
                    "commit `{hash}` does not exist in `{}`. {}. Cite a commit that exists in this workspace.",
                    workspace.display(),
                    stderr_tail(&output.stderr)
                ),
            ));
        }
        Err(error) => {
            return Some(resolve_failure(
                claim,
                format!(
                    "commit `{hash}` could not be checked because git failed to start: {error}"
                ),
            ));
        }
    }
    let message = match git(workspace, &["log", "-1", "--format=%B", hash]) {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => {
            return Some(resolve_failure(
                claim,
                format!(
                    "commit `{hash}` exists but its message could not be read. {}",
                    stderr_tail(&output.stderr)
                ),
            ));
        }
        Err(error) => {
            return Some(resolve_failure(
                claim,
                format!("commit `{hash}` message could not be read: {error}"),
            ));
        }
    };
    match recorded_subject_in_text(&message, COMMIT_SUBJECT_TRAILER) {
        Some(found) => subject_mismatch(claim, &format!("commit `{hash}`"), &found),
        None => Some(resolve_failure(
            claim,
            format!(
                "commit `{hash}` has no `{COMMIT_SUBJECT_TRAILER}` trailer. Record the subject in the commit message as `{COMMIT_SUBJECT_TRAILER} {}` only if the commit is about that subject.",
                claim.subject()
            ),
        )),
    }
}

fn evaluate_url(
    claim: &Claim,
    url: &str,
    sender: &str,
    fetch_for: &dyn Fn(&str) -> Option<RecordedFetch>,
) -> Option<GateFailure> {
    let host = match super::types::normalize_url(url) {
        Ok((_, host)) => host,
        Err(error) => {
            return Some(resolve_failure(
                claim,
                format!("URL `{url}` cannot be resolved: {error}"),
            ));
        }
    };
    let Some(fetch) = fetch_for(url) else {
        return Some(resolve_failure(
            claim,
            format!(
                "URL `{url}` has no recorded fetch by `{sender}`. Call handoff action `record_fetch` with the HTTP status and body digest before submitting this claim."
            ),
        ));
    };
    if fetch.sender != sender {
        return Some(resolve_failure(
            claim,
            format!(
                "URL `{url}` was fetched by `{}`, not by the handoff sender `{sender}`. The sender must record the fetch with action `record_fetch` before submitting.",
                fetch.sender
            ),
        ));
    }
    if fetch.host != host.as_str() {
        return Some(resolve_failure(
            claim,
            format!(
                "recorded fetch for `{url}` has host `{}`, which does not match the URL host `{}`. Record the fetch again.",
                fetch.host,
                host.as_str()
            ),
        ));
    }
    subject_mismatch(
        claim,
        &format!("URL `{url}` (host `{}`)", host.as_str()),
        &host,
    )
}

fn subject_mismatch(claim: &Claim, source_label: &str, found: &SubjectKey) -> Option<GateFailure> {
    if claim.subject() == found {
        return None;
    }
    Some(GateFailure {
        target: claim.id().to_string(),
        check: CheckKind::SubjectMatches,
        fix: format!(
            "claim `{}` is about `{}` but its source {source_label} records subject `{}`. Cite a source about `{}`, or change the claim subject to `{}` if that is what the claim is about.",
            claim.id(),
            claim.subject(),
            found.as_str(),
            claim.subject(),
            found.as_str()
        ),
    })
}

fn resolve_failure(claim: &Claim, fix: String) -> GateFailure {
    GateFailure {
        target: claim.id().to_string(),
        check: CheckKind::SourceResolves,
        fix,
    }
}

fn canonicalize_dir(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    canonical.is_dir().then_some(canonical)
}

fn read_recorded_subject(path: &Path, prefix: &str) -> Result<Option<SubjectKey>, String> {
    let mut file = File::open(path).map_err(|error| format!("file could not be read: {error}"))?;
    let mut buffer = vec![0_u8; SUBJECT_SCAN_BYTES];
    let read = file
        .read(&mut buffer)
        .map_err(|error| format!("file could not be read: {error}"))?;
    buffer.truncate(read);
    let text = String::from_utf8(buffer).map_err(|_| {
        "file is not UTF-8 in its first 64 KiB, so its recorded subject cannot be read".to_string()
    })?;
    Ok(recorded_subject_in_text(&text, prefix))
}

fn recorded_subject_in_text(text: &str, prefix: &str) -> Option<SubjectKey> {
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(prefix) else {
            continue;
        };
        return SubjectKey::parse(rest.trim()).ok();
    }
    None
}

fn git(workspace: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
}

fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    if text.is_empty() {
        return "git reported no detail".to_string();
    }
    let clipped: String = text.chars().take(180).collect();
    format!("git reported: {clipped}")
}
