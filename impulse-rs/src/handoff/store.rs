//! Durable handoff ledger: recorded fetches, pending or accepted handoffs,
//! and shared-artifact generations.
//!
//! Writes go through a temp file and rename. A lock file serializes
//! read-modify-write so two agents cannot both observe the same generation
//! and both commit.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::storage::Storage;

use super::types::{GateFailure, NextStep, WorkflowResult, HANDOFF_GATE_FILE, MAX_ARTIFACT_BYTES};
use super::CheckKind;

const SCHEMA_VERSION: u32 = 1;
const LOCK_WAIT_ATTEMPTS: usize = 50;
const STALE_LOCK: Duration = Duration::from_secs(10);

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
    #[serde(default)]
    fetches: BTreeMap<String, FetchRecord>,
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
        let store: Self = serde_json::from_str(&text).map_err(|error| {
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

struct LedgerLock {
    path: PathBuf,
}

impl LedgerLock {
    fn acquire(dir: &Path) -> Result<Self, super::HandoffError> {
        let path = dir.join("HANDOFF_GATE.lock");
        for _ in 0..LOCK_WAIT_ATTEMPTS {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    thread::sleep(Duration::from_millis(20));
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
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    modified
        .elapsed()
        .unwrap_or(Duration::from_secs(0))
        .gt(&STALE_LOCK)
        || modified > SystemTime::now()
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
    pub(crate) fn fetch(&self, url: &str) -> Option<&FetchRecord> {
        self.fetches.get(url)
    }

    pub(crate) fn insert_fetch(&mut self, record: FetchRecord) -> Result<(), super::HandoffError> {
        if self.fetches.len() >= 256 && !self.fetches.contains_key(&record.url) {
            return Err(super::HandoffError::Invalid(
                "the fetch ledger already holds 256 URLs. Submit or drop a recorded fetch before adding another."
                    .into(),
            ));
        }
        self.fetches.insert(record.url.clone(), record);
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
