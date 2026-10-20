//! Deterministic checks for a findings handoff.
//!
//! No model is involved. A file resolves when it exists inside the workspace
//! and carries an `impulse-subject:` line. A commit resolves when `git`
//! confirms the object and the message carries an `Impulse-Subject:` trailer.
//! A URL resolves when the sender previously recorded a fetch for that URL.
//! The claim's declared subject must equal the subject those sources record
//! (the file line, the trailer, or the URL host).

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    // The resolved path is what gets opened. A symlink whose target leaves
    // the workspace already failed the check above; opening that resolved
    // path with `O_NOFOLLOW` refuses a last-component swap after the check.
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
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            return Some(resolve_failure(
                claim,
                format!(
                    "commit `{hash}` could not be checked because git exceeded {}s. Retry the submit once git responds.",
                    GIT_TIMEOUT.as_secs()
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

fn open_resolved_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|error| format!("file could not be read: {error}"))
}

fn read_recorded_subject(path: &Path, prefix: &str) -> Result<Option<SubjectKey>, String> {
    let mut file = open_resolved_file(path)?;
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

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

fn git(workspace: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = thread::spawn(move || read_pipe(stdout));
    let stderr_thread = thread::spawn(move || read_pipe(stderr));
    let waited = wait_child(&mut child, GIT_TIMEOUT);
    let stdout = join_bytes(stdout_thread);
    let stderr = join_bytes(stderr_thread);
    let status = waited?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut buffer = Vec::new();
    let _ = pipe.read_to_end(&mut buffer);
    buffer
}

fn join_bytes(handle: thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    handle.join().unwrap_or_default()
}

/// Wait until `child` exits or `timeout` elapses. On timeout the child is
/// killed and reaped so a slow `git` cannot hold the handoff lock forever.
fn wait_child(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("git timed out after {}s", timeout.as_secs()),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
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

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::wait_child;

    #[test]
    fn test_wait_child_kills_a_process_that_exceeds_the_timeout() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleep");
        let started = Instant::now();
        let error = wait_child(&mut child, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the timeout must kill the child instead of waiting out the command"
        );
        assert!(
            format!("{error}").contains("timed out"),
            "timeout error must name the limit, got {error}"
        );
    }
}
