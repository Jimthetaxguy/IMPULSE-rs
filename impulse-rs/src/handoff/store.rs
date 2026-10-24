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
//! Schema 3 keys each recorded fetch by sender and URL and stamps
//! `recorded_via` (execution origin and session) on fetches and handoffs.
//! Schema 2 is that key shape without the stamp. Schema 1, which kept one
//! row per URL, is rewritten to the schema 2 shape and then to schema 3.

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

const SCHEMA_VERSION: u32 = 3;
const LOCK_WAIT_ATTEMPTS: usize = 50;
/// How long a contending caller waits before asking the agent to retry.
/// This is not a staleness threshold: a live owner's lock is never taken,
/// including when the file is older than the previous 10s age check.
const LOCK_RETRY_PAUSE: Duration = Duration::from_millis(20);

/// Session and origin that recorded a row. Stamped by the caller of the
/// gate, never copied from the JSON payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RecordedVia {
    pub execution_origin: String,
    pub session_id: Option<String>,
}

impl RecordedVia {
    pub(crate) fn unrecorded() -> Self {
        Self {
            execution_origin: "unrecorded".to_string(),
            session_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct FetchRecord {
    pub url: String,
    pub host: String,
    pub status: u16,
    pub body_digest: String,
    pub fetched_at: DateTime<Utc>,
    pub sender: String,
    #[serde(default = "RecordedVia::unrecorded")]
    pub recorded_via: RecordedVia,
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
    #[serde(default = "RecordedVia::unrecorded")]
    pub recorded_via: RecordedVia,
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
            Some(3) => {}
            Some(2) => migrate_v2_to_v3(&mut value).map_err(|error| {
                super::HandoffError::Corrupt(format!(
                    "{} is a schema 2 handoff ledger and could not be read as schema 3 ({error}). Delete it, or add recorded_via to each fetch and handoff, then set schema_version to 3.",
                    path.display()
                ))
            })?,
            Some(1) => {
                migrate_v1_fetches(&mut value).map_err(|error| {
                    super::HandoffError::Corrupt(format!(
                        "{} is a schema 1 handoff ledger and could not be read as schema 3 ({error}). Delete it, or rewrite fetches so each record is kept for its sender and URL, then set schema_version to 3.",
                        path.display()
                    ))
                })?;
                migrate_v2_to_v3(&mut value).map_err(|error| {
                    super::HandoffError::Corrupt(format!(
                        "{} migrated from schema 1 but could not be stamped as schema 3 ({error}).",
                        path.display()
                    ))
                })?;
            }
            Some(other) => {
                return Err(super::HandoffError::Corrupt(format!(
                    "handoff ledger schema is {other}; this Impulse reads schema 3. Delete {} or rewrite it as schema 3 with one fetch record per sender and URL and a recorded_via stamp.",
                    path.display()
                )));
            }
            None => {
                return Err(super::HandoffError::Corrupt(format!(
                    "{} has no schema_version. This Impulse reads schema 3. Delete the file, or set schema_version to 3 after keying fetches by sender and URL and adding recorded_via.",
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
    object.insert("schema_version".into(), json!(2));
    object.insert("fetches".into(), Value::Object(nested));
    Ok(())
}

/// Schema 2 rows have no session stamp. Schema 3 adds `recorded_via` and
/// leaves `session_id` null so a later sessioned submit does not inherit
/// an unstamped fetch.
fn migrate_v2_to_v3(value: &mut Value) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Err("schema 2 ledger must be a JSON object".into());
    };
    stamp_recorded_via(object.get_mut("fetches"), true)?;
    stamp_recorded_via(object.get_mut("handoffs"), false)?;
    object.insert("schema_version".into(), json!(SCHEMA_VERSION));
    Ok(())
}

fn stamp_recorded_via(section: Option<&mut Value>, nested_by_url: bool) -> Result<(), String> {
    let Some(section) = section else {
        return Ok(());
    };
    let Some(map) = section.as_object_mut() else {
        return Err("ledger section must be an object".into());
    };
    for (key, entry) in map.iter_mut() {
        if nested_by_url {
            let Some(by_url) = entry.as_object_mut() else {
                return Err(format!("schema 2 fetches for `{key}` must be an object"));
            };
            for (url, record) in by_url.iter_mut() {
                insert_unrecorded(record).map_err(|error| format!("fetch `{url}`: {error}"))?;
            }
        } else {
            insert_unrecorded(entry).map_err(|error| format!("handoff `{key}`: {error}"))?;
        }
    }
    Ok(())
}

fn insert_unrecorded(value: &mut Value) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Err("record must be a JSON object".into());
    };
    if !object.contains_key("recorded_via") {
        object.insert(
            "recorded_via".into(),
            json!({
                "execution_origin": "unrecorded",
                "session_id": null
            }),
        );
    }
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
        "recorded_via": handoff.recorded_via,
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
        field: Some("based_on_generation".to_string()),
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
            field: Some("artifact_id".to_string()),
            fix: format!(
                "payload.artifact_id `{artifact_id}` does not exist. Create it with action `write_shared` and `based_on_generation` 0."
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

    // ---- Serde round-trips for the persisted ledger (HANDOFF_GATE file) ----

    fn fixed_time(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn assert_json_round_trip<T>(original: &T) -> T
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(original).unwrap();
        let recovered: T = serde_json::from_str(&json).unwrap();
        assert_eq!(&recovered, original);
        recovered
    }

    fn sample_recorded_via() -> RecordedVia {
        RecordedVia {
            execution_origin: "ion".into(),
            session_id: Some("session-7".into()),
        }
    }

    fn sample_fetch() -> FetchRecord {
        FetchRecord {
            url: "https://billing.example.com/docs/refunds".into(),
            host: "billing.example.com".into(),
            status: 200,
            body_digest: "0123456789abcdef".repeat(4),
            fetched_at: fixed_time("2026-06-01T12:00:00Z"),
            sender: "researcher".into(),
            recorded_via: sample_recorded_via(),
        }
    }

    fn sample_claim() -> super::super::Claim {
        super::super::Claim::try_new(
            "refund-cap",
            "Refunds over $50 need a second approver.",
            "billing.example.com",
            super::super::ClaimSource::Url {
                url: "https://billing.example.com/docs/refunds".into(),
            },
        )
        .unwrap()
    }

    fn sample_pending_handoff() -> StoredHandoff {
        StoredHandoff {
            id: "h-pending".into(),
            from_agent: "researcher".into(),
            to_agent: "builder".into(),
            workspace: "/work/impulse".into(),
            claims: vec![sample_claim()],
            phase: HandoffPhase::Pending {
                failures: vec![GateFailure {
                    target: "refund-cap".into(),
                    check: CheckKind::SourceResolves,
                    field: Some("source".into()),
                    fix: "record the fetch first".into(),
                }],
            },
            updated_at: fixed_time("2026-06-01T12:05:00Z"),
            recorded_via: sample_recorded_via(),
        }
    }

    fn sample_accepted_handoff() -> StoredHandoff {
        StoredHandoff {
            id: "h-accepted".into(),
            phase: HandoffPhase::Accepted {
                accepted_at: fixed_time("2026-06-01T12:10:00Z"),
            },
            recorded_via: RecordedVia::unrecorded(),
            ..sample_pending_handoff()
        }
    }

    fn sample_artifact() -> SharedArtifact {
        SharedArtifact {
            generation: 3,
            content: "shared plan v3".into(),
            last_writer: "builder".into(),
            last_written_at: fixed_time("2026-06-01T12:20:00Z"),
            reads: BTreeMap::from([(
                "reviewer".to_string(),
                ReadMark {
                    generation: 2,
                    read_at: fixed_time("2026-06-01T12:15:00Z"),
                },
            )]),
        }
    }

    fn sample_store() -> GateStore {
        let fetch = sample_fetch();
        let pending = sample_pending_handoff();
        let accepted = sample_accepted_handoff();
        GateStore {
            schema_version: SCHEMA_VERSION,
            fetches: BTreeMap::from([(
                fetch.sender.clone(),
                BTreeMap::from([(fetch.url.clone(), fetch)]),
            )]),
            handoffs: BTreeMap::from([
                (pending.id.clone(), pending),
                (accepted.id.clone(), accepted),
            ]),
            artifacts: BTreeMap::from([("plan".to_string(), sample_artifact())]),
        }
    }

    #[test]
    fn round_trip_recorded_via() {
        assert_json_round_trip(&sample_recorded_via());
        let recovered = assert_json_round_trip(&RecordedVia::unrecorded());
        assert_eq!(recovered.session_id, None);
    }

    #[test]
    fn round_trip_fetch_record() {
        let recovered = assert_json_round_trip(&sample_fetch());
        assert_eq!(recovered.recorded_via.execution_origin, "ion");
    }

    #[test]
    fn test_fetch_record_without_recorded_via_defaults_to_unrecorded() {
        let json = r#"{"url":"https://a.example.com/x","host":"a.example.com","status":200,"body_digest":"d","fetched_at":"2026-06-01T12:00:00Z","sender":"researcher"}"#;
        let fetch: FetchRecord = serde_json::from_str(json).unwrap();
        assert_eq!(fetch.recorded_via, RecordedVia::unrecorded());
    }

    #[test]
    fn round_trip_stored_handoff_in_each_phase() {
        let pending = assert_json_round_trip(&sample_pending_handoff());
        assert!(
            matches!(pending.phase, HandoffPhase::Pending { ref failures } if failures.len() == 1)
        );
        let accepted = assert_json_round_trip(&sample_accepted_handoff());
        assert!(matches!(accepted.phase, HandoffPhase::Accepted { .. }));
    }

    #[test]
    fn test_stored_handoff_without_recorded_via_defaults_to_unrecorded() {
        let mut value = serde_json::to_value(sample_accepted_handoff()).unwrap();
        value.as_object_mut().unwrap().remove("recorded_via");
        let handoff: StoredHandoff = serde_json::from_value(value).unwrap();
        assert_eq!(handoff.recorded_via, RecordedVia::unrecorded());
    }

    #[test]
    fn test_handoff_phase_wire_tags_are_pinned() {
        let pending = serde_json::to_value(HandoffPhase::Pending { failures: vec![] }).unwrap();
        assert_eq!(pending, json!({"status": "pending", "failures": []}));
        let accepted = serde_json::to_value(HandoffPhase::Accepted {
            accepted_at: fixed_time("2026-06-01T12:10:00Z"),
        })
        .unwrap();
        assert_eq!(
            accepted,
            json!({"status": "accepted", "accepted_at": "2026-06-01T12:10:00Z"})
        );
        assert!(serde_json::from_value::<HandoffPhase>(json!({"status": "Accepted"})).is_err());
    }

    #[test]
    fn round_trip_shared_artifact_and_read_mark() {
        let recovered = assert_json_round_trip(&sample_artifact());
        assert_eq!(recovered.reads["reviewer"].generation, 2);
        assert_json_round_trip(&ReadMark {
            generation: 9,
            read_at: fixed_time("2026-06-02T00:00:00Z"),
        });
    }

    #[test]
    fn test_shared_artifact_without_reads_deserializes_empty() {
        let json = r#"{"generation":1,"content":"c","last_writer":"w","last_written_at":"2026-06-01T12:00:00Z"}"#;
        let artifact: SharedArtifact = serde_json::from_str(json).unwrap();
        assert!(artifact.reads.is_empty());
    }

    #[test]
    fn round_trip_gate_store() {
        let recovered = assert_json_round_trip(&sample_store());
        assert_eq!(recovered.handoffs.len(), 2);
        assert_json_round_trip(&GateStore::default());
    }

    #[test]
    fn test_gate_store_save_then_load_returns_the_same_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let original = sample_store();
        original.save(dir.path()).unwrap();
        let loaded = GateStore::load(dir.path()).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn test_gate_store_schema_3_with_only_version_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(HANDOFF_GATE_FILE),
            format!("{{\"schema_version\":{SCHEMA_VERSION}}}"),
        )
        .unwrap();
        let loaded = GateStore::load(dir.path()).unwrap();
        assert_eq!(loaded, GateStore::default());
    }
}
