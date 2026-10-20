//! Durable handoff ledger: recorded fetches, pending or accepted handoffs,
//! and shared-artifact generations.
//!
//! Writes go through a temp file and rename. A lock file serializes
//! read-modify-write so two agents cannot both observe the same generation
//! and both commit. The lock records the owner's pid and a token. It is
//! reclaimed only when that pid is no longer alive, never because the file
//! is old. `git` calls that run while the lock is held are killed after a
//! fixed timeout so a slow subprocess cannot sit on the lock forever.
//!
//! Schema 2 keys each recorded fetch by sender and URL. A schema 1 ledger,
//! which kept one row per URL, is rewritten to that shape on load.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::storage::Storage;

use super::types::{GateFailure, NextStep, WorkflowResult, HANDOFF_GATE_FILE, MAX_ARTIFACT_BYTES};
use super::CheckKind;

const SCHEMA_VERSION: u32 = 2;
const LOCK_WAIT_ATTEMPTS: usize = 50;
/// How long a contending caller waits before asking the agent to retry.
/// This is not a staleness threshold: a live owner's lock is never taken,
/// including when the file is older than the previous 10s age check.
const LOCK_RETRY_PAUSE: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct FetchRecord {
    pub url: String,
    pub host: String,
    pub status: u16,
    pub body_digest: String,
    pub fetched_at: DateTime<Utc>,
    pub sender: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct StoredHandoff {
    pub id: String,
    pub from_agent: String,
    pub to_agent: String,
    pub workspace: String,
    pub claims: Vec<super::Claim>,
    pub phase: HandoffPhase,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum HandoffPhase {
    Pending { failures: Vec<GateFailure> },
    Accepted { accepted_at: DateTime<Utc> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SharedArtifact {
    pub generation: u64,
    pub content: String,
    pub last_writer: String,
    pub last_written_at: DateTime<Utc>,
    #[serde(default)]
    pub reads: BTreeMap<String, ReadMark>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ReadMark {
    pub generation: u64,
    pub read_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct GateStore {
    schema_version: u32,
    /// Sender, then normalized URL. One record per sender, so a second
    /// agent recording the same URL does not erase the first agent's fetch.
    #[serde(default)]
    fetches: BTreeMap<String, BTreeMap<String, FetchRecord>>,
    #[serde(default)]
    handoffs: BTreeMap<String, StoredHandoff>,
    #[serde(default)]
    artifacts: BTreeMap<String, SharedArtifact>,
}

impl Default for GateStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            fetches: BTreeMap::new(),
            handoffs: BTreeMap::new(),
            artifacts: BTreeMap::new(),
        }
    }
}

pub(crate) fn with_store<T>(
    impulse_dir: &Path,
    body: impl FnOnce(&mut GateStore) -> Result<T, super::HandoffError>,
) -> Result<T, super::HandoffError> {
    fs::create_dir_all(impulse_dir).map_err(|source| super::HandoffError::Store {
        action: "create the impulse directory for",
        source,
    })?;
    let _lock = LedgerLock::acquire(impulse_dir)?;
    let mut store = GateStore::load(impulse_dir)?;
    let result = body(&mut store)?;
    store.save(impulse_dir)?;
    Ok(result)
}

impl GateStore {
    fn load(impulse_dir: &Path) -> Result<Self, super::HandoffError> {
        let path = impulse_dir.join(HANDOFF_GATE_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).map_err(|source| super::HandoffError::Store {
            action: "read",
            source,
        })?;
        let mut value: Value = serde_json::from_str(&text).map_err(|error| {
            super::HandoffError::Corrupt(format!(
                "{} is not a handoff ledger ({error})",
                path.display()
            ))
        })?;
        match value.get("schema_version").and_then(Value::as_u64) {
            Some(2) => {}
            Some(1) => migrate_v1_fetches(&mut value).map_err(|error| {
                super::HandoffError::Corrupt(format!(
                    "{} is a schema 1 handoff ledger and could not be read as schema 2 ({error}). Delete it, or rewrite fetches so each record is kept for its sender and URL, then set schema_version to 2.",
                    path.display()
                ))
            })?,
            Some(other) => {
                return Err(super::HandoffError::Corrupt(format!(
                    "handoff ledger schema is {other}; this Impulse reads schema 2. Delete {} or rewrite it as schema 2 with one fetch record per sender and URL.",
                    path.display()
                )));
            }
            None => {
                return Err(super::HandoffError::Corrupt(format!(
                    "{} has no schema_version. This Impulse reads schema 2. Delete the file, or set schema_version to 2 after keying fetches by sender and URL.",
                    path.display()
                )));
            }
        }
        let store: Self = serde_json::from_value(value).map_err(|error| {
            super::HandoffError::Corrupt(format!(
                "{} is not a handoff ledger ({error})",
                path.display()
            ))
        })?;
        store.validate()?;
        Ok(store)
    }

    fn validate(&self) -> Result<(), super::HandoffError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(super::HandoffError::Corrupt(format!(
                "handoff ledger schema is {}, this Impulse reads {SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        for (id, handoff) in &self.handoffs {
            if let HandoffPhase::Pending { failures } = &handoff.phase {
                if failures.is_empty() {
                    return Err(super::HandoffError::Corrupt(format!(
                        "handoff `{id}` is pending with no failures"
                    )));
                }
            }
        }
        Ok(())
    }

    fn save(&self, impulse_dir: &Path) -> Result<(), super::HandoffError> {
        let path = impulse_dir.join(HANDOFF_GATE_FILE);
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| {
            super::HandoffError::Corrupt(format!("failed to serialize the handoff ledger: {error}"))
        })?;
        Storage::atomic_write_path(&path, &bytes).map_err(|error| {
            super::HandoffError::Corrupt(format!("failed to write {}: {error}", path.display()))
        })
    }
}

struct LockOwner {
    pid: u32,
    token: String,
}

impl LockOwner {
    fn encode(&self) -> String {
        format!("pid={}\ntoken={}\n", self.pid, self.token)
    }
}

fn parse_lock(text: &str) -> Option<LockOwner> {
    let mut pid = None;
    let mut token = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("pid=") {
            pid = value.parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("token=") {
            if !value.is_empty() {
                token = Some(value.to_string());
            }
        }
    }
    Some(LockOwner {
        pid: pid?,
        token: token?,
    })
}

/// `true` when `pid` names a process this caller can see.
///
/// `kill` with signal 0 does not deliver a signal. A live owner, including
/// one whose lock file is older than the old 10s age threshold, stays the
/// owner. `EPERM` still means the process exists.
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 performs an existence check and does not queue a
    // signal. `pid` is a positive process id, not a process-group id.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

struct LedgerLock {
    path: PathBuf,
    token: String,
}

impl LedgerLock {
    fn acquire(dir: &Path) -> Result<Self, super::HandoffError> {
        let path = dir.join("HANDOFF_GATE.lock");
        for _ in 0..LOCK_WAIT_ATTEMPTS {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let owner = LockOwner {
                        pid: std::process::id(),
                        token: uuid::Uuid::new_v4().simple().to_string(),
                    };
                    let body = owner.encode();
                    if let Err(source) = file.write_all(body.as_bytes()) {
                        let _ = fs::remove_file(&path);
                        return Err(super::HandoffError::Store {
                            action: "lock",
                            source,
                        });
                    }
                    return Ok(Self {
                        path,
                        token: owner.token,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if reclaim_dead_owner(&path) {
                        continue;
                    }
                    thread::sleep(LOCK_RETRY_PAUSE);
                }
                Err(source) => {
                    return Err(super::HandoffError::Store {
                        action: "lock",
                        source,
                    });
                }
            }
        }
        Err(super::HandoffError::Invalid(
            "handoff ledger is locked by another writer. Retry this call.".into(),
        ))
    }
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return;
        };
        if parse_lock(&text).is_some_and(|owner| owner.token == self.token) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Remove `path` only when its recorded owner is not a live process and the
/// bytes are still the ones that were checked. A live pid is left in place
/// even when the file's age exceeds the old 10 second threshold.
fn reclaim_dead_owner(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    if let Some(owner) = parse_lock(&text) {
        if process_is_alive(owner.pid) {
            return false;
        }
    }
    remove_if_unchanged(path, &text)
}

fn remove_if_unchanged(path: &Path, expected: &str) -> bool {
    match fs::read_to_string(path) {
        Ok(current) if current == expected => fs::remove_file(path).is_ok(),
        _ => false,
    }
}

/// Schema 1 stored fetches as `{ url: record }`. Schema 2 stores
/// `{ sender: { url: record } }` so two senders can record one URL.
fn migrate_v1_fetches(value: &mut Value) -> Result<(), String> {
    let fetches = value
        .get("fetches")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let Some(map) = fetches.as_object() else {
        return Err("schema 1 fetches must be an object".into());
    };
    let mut nested = serde_json::Map::new();
    for (key, record) in map {
        let Some(sender) = record
            .get("sender")
            .and_then(Value::as_str)
            .filter(|sender| !sender.is_empty())
        else {
            return Err(format!("schema 1 fetch `{key}` has no sender"));
        };
        // Prefer the record's own URL. A schema 1 file may key the object by
        // the URL, or by a sender/URL composite; both still carry `url`.
        let url = record
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .unwrap_or(key.as_str());
        let by_sender = nested
            .entry(sender.to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(by_url) = by_sender.as_object_mut() else {
            return Err(format!(
                "schema 1 fetch for `{sender}` could not be grouped by URL"
            ));
        };
        by_url.insert(url.to_string(), record.clone());
    }
    let Some(object) = value.as_object_mut() else {
        return Err("schema 1 ledger must be a JSON object".into());
    };
    object.insert("schema_version".into(), json!(SCHEMA_VERSION));
    object.insert("fetches".into(), Value::Object(nested));
    Ok(())
}

pub(crate) fn handoff_snapshot(handoff: &StoredHandoff) -> Value {
    json!({
        "kind": "handoff",
        "id": handoff.id,
        "from_agent": handoff.from_agent,
        "to_agent": handoff.to_agent,
        "workspace": handoff.workspace,
        "phase": handoff.phase,
        "claims": handoff.claims,
        "updated_at": handoff.updated_at,
    })
}

impl GateStore {
    pub(crate) fn fetch(&self, sender: &str, url: &str) -> Option<&FetchRecord> {
        self.fetches.get(sender)?.get(url)
    }

    pub(crate) fn insert_fetch(&mut self, record: FetchRecord) -> Result<(), super::HandoffError> {
        let total = self
            .fetches
            .values()
            .map(|by_url| by_url.len())
            .sum::<usize>();
        let exists = self
            .fetches
            .get(&record.sender)
            .and_then(|by_url| by_url.get(&record.url))
            .is_some();
        if total >= 256 && !exists {
            return Err(super::HandoffError::Invalid(
                "the fetch ledger already holds 256 recorded fetches. Submit or drop a recorded fetch before adding another."
                    .into(),
            ));
        }
        self.fetches
            .entry(record.sender.clone())
            .or_default()
            .insert(record.url.clone(), record);
        Ok(())
    }

    pub(crate) fn handoff(&self, id: &str) -> Option<&StoredHandoff> {
        self.handoffs.get(id)
    }

    pub(crate) fn insert_handoff(
        &mut self,
        handoff: StoredHandoff,
    ) -> Result<(), super::HandoffError> {
        if self.handoffs.len() >= 256 && !self.handoffs.contains_key(&handoff.id) {
            return Err(super::HandoffError::Invalid(
                "the handoff ledger already holds 256 handoffs".into(),
            ));
        }
        self.handoffs.insert(handoff.id.clone(), handoff);
        Ok(())
    }

    pub(crate) fn artifact(&self, id: &str) -> Option<&SharedArtifact> {
        self.artifacts.get(id)
    }

    pub(crate) fn artifact_mut(&mut self, id: &str) -> Option<&mut SharedArtifact> {
        self.artifacts.get_mut(id)
    }

    pub(crate) fn insert_artifact(
        &mut self,
        id: String,
        artifact: SharedArtifact,
    ) -> Result<(), super::HandoffError> {
        if self.artifacts.len() >= 64 && !self.artifacts.contains_key(&id) {
            return Err(super::HandoffError::Invalid(
                "the shared-artifact ledger already holds 64 artifacts".into(),
            ));
        }
        if artifact.content.len() > MAX_ARTIFACT_BYTES {
            return Err(super::HandoffError::Invalid(format!(
                "shared artifact `{id}` is {} bytes; the maximum is {MAX_ARTIFACT_BYTES}",
                artifact.content.len()
            )));
        }
        self.artifacts.insert(id, artifact);
        Ok(())
    }
}

pub(crate) fn artifact_snapshot(id: &str, artifact: &SharedArtifact, phase: &str) -> Value {
    json!({
        "kind": "shared_artifact",
        "artifact_id": id,
        "generation": artifact.generation,
        "content": artifact.content,
        "last_writer": artifact.last_writer,
        "last_written_at": artifact.last_written_at,
        "phase": phase,
    })
}

pub(crate) fn stale_write_failure(
    artifact_id: &str,
    writer: &str,
    based_on: u64,
    artifact: &SharedArtifact,
    now: DateTime<Utc>,
) -> GateFailure {
    let read = artifact.reads.get(writer);
    let minutes = now
        .signed_duration_since(artifact.last_written_at)
        .num_minutes()
        .max(0);
    let minute_label = if minutes == 1 {
        "1 minute".to_string()
    } else {
        format!("{minutes} minutes")
    };
    let read_label = match read {
        Some(mark) => format!(
            "`{writer}` last read generation {} at {}",
            mark.generation,
            mark.read_at.to_rfc3339()
        ),
        None => format!("`{writer}` has no recorded read of this artifact"),
    };
    GateFailure {
        target: artifact_id.to_string(),
        check: CheckKind::StaleRead,
        fix: format!(
            "refusing write of `{artifact_id}` by `{writer}`: this write is based on generation {based_on}, and {read_label}. `{}` wrote generation {} at {} ({minute_label} before this write). Re-read `{artifact_id}` and write against generation {}.",
            artifact.last_writer,
            artifact.generation,
            artifact.last_written_at.to_rfc3339(),
            artifact.generation
        ),
    }
}

pub(crate) fn read_next_step(artifact_id: &str, generation: u64) -> NextStep {
    NextStep {
        action: "read_shared".into(),
        detail: format!(
            "Re-read `{artifact_id}` with action `read_shared`. The current generation is {generation}. Then call `write_shared` with `based_on_generation` {generation}."
        ),
    }
}

pub(crate) fn refuse_missing_artifact(
    artifact_id: &str,
) -> Result<WorkflowResult, super::HandoffError> {
    WorkflowResult::pending(
        json!({
            "kind": "shared_artifact",
            "artifact_id": artifact_id,
            "present": false,
            "phase": "pending",
        }),
        vec![GateFailure {
            target: artifact_id.to_string(),
            check: CheckKind::ArtifactExists,
            fix: format!(
                "shared artifact `{artifact_id}` does not exist. Create it with action `write_shared` and `based_on_generation` 0."
            ),
        }],
        NextStep {
            action: "write_shared".into(),
            detail: format!(
                "Create `{artifact_id}` with action `write_shared` and `based_on_generation` 0."
            ),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn test_live_owner_lock_older_than_ten_seconds_is_not_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("HANDOFF_GATE.lock");
        let token = "live-owner-token";
        fs::write(
            &path,
            format!("pid={}\ntoken={token}\n", std::process::id()),
        )
        .unwrap();
        let old = SystemTime::now() - Duration::from_secs(11);
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(old)).unwrap();
        let age = fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap();
        assert!(
            age > Duration::from_secs(10),
            "the fixture must be older than the previous 10s stale threshold, age {age:?}"
        );

        let acquired = LedgerLock::acquire(dir.path());
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains(token),
            "a live owner's lock was replaced: {body}"
        );
        assert!(
            body.contains(&format!("pid={}", std::process::id())),
            "the live pid was not kept: {body}"
        );
        let Err(error) = acquired else {
            panic!("a live owner keeps the lock past 10s");
        };
        assert!(
            format!("{error}").contains("locked"),
            "the contender should be told to retry, got {error}"
        );
    }

    #[test]
    fn test_dead_owner_lock_is_reclaimed_and_records_the_new_pid_and_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("HANDOFF_GATE.lock");
        fs::write(&path, "pid=4294967295\ntoken=dead-owner\n").unwrap();
        assert!(
            !process_is_alive(u32::MAX),
            "the fixture pid must not be a live process"
        );

        let lock = LedgerLock::acquire(dir.path()).expect("a dead owner is reclaimed");
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains(&format!("pid={}", std::process::id())));
        assert!(body.contains("token="));
        assert!(!body.contains("dead-owner"));
        drop(lock);
        assert!(
            !path.exists(),
            "dropping the holder removes only the lock whose token it wrote"
        );
    }
}
