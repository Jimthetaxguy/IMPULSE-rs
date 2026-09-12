//! Append-only promoted-memory log, its digest chain, and the GENOME
//! projection (ADR-0020).
//!
//! Three artifacts stay separate on purpose, and nothing merges them:
//!
//! - `.impulse/MEMORY_CANDIDATES.json` — the private, raw review ledger of
//!   candidates derived from accepted governed runs (ADR-0013). Owned by
//!   `state::memory_candidate`.
//! - `.impulse/MEMORY.jsonl` — this module. The append-only, hash-chained log
//!   of promoted [`MemoryRecord`]s. Authoritative for what was promoted.
//! - `.impulse/GENOME_PROJECTION.md` — a derived, regenerated-wholesale
//!   markdown rendering of the currently valid records. Never hand-edited,
//!   never merged into, and never a source of truth.
//!
//! `.impulse/GENOME.md` is a *fourth*, unrelated artifact: the hand-curated
//! `memory::Genome` written by `impulse memory add`. This module never reads or
//! writes it. Regenerating it from promoted records would delete every
//! operator-authored decision it holds.
//!
//! The hash chain lives here rather than in `impulse-ops` because `impulse-ops`
//! carries no cryptographic dependency: it defines *what* bytes are hashed,
//! this module defines *how*.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use impulse_ops::memory_candidate::{
    AcceptedRunMemoryCandidate, MemoryCandidateStatus, MemoryKind, MemoryLogEntry, MemoryRecord,
    MemoryRecordId, MemoryScope, MemorySource, MEMORY_LOG_DIGEST_PREFIX, MEMORY_LOG_GENESIS_DIGEST,
    MEMORY_RECORD_DIGEST_PREFIX, MEMORY_RECORD_ID_PREFIX, MEMORY_RECORD_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::Storage;

pub(super) const MEMORY_LOG_FILE: &str = "MEMORY.jsonl";
pub(super) const GENOME_PROJECTION_FILE: &str = "GENOME_PROJECTION.md";
pub(super) const MEMORY_INDEX_FILE: &str = "MEMORY_INDEX.json";

const MEMORY_INDEX_SCHEMA_VERSION: u32 = 1;

/// A decision commits by appending to the log and then replacing the ledger. A
/// process killed between those two steps leaves exactly one entry the ledger
/// does not yet name. More than one means the file was edited outside Impulse.
const MAX_UNCOMMITTED_TAIL_ENTRIES: usize = 1;

/// Typed load failures, so the daemon's start-up surface can say which of the
/// three distinguishable corruptions it hit instead of one prose blob.
#[derive(Debug, thiserror::Error)]
pub enum MemoryLogError {
    #[error("memory log entry {seq} is malformed: {message}")]
    MalformedEntry { seq: u64, message: String },
    #[error(
        "memory log digest chain broken at entry {seq}: {message}; \
         .impulse/MEMORY.jsonl was modified outside Impulse and is refused"
    )]
    BrokenChain { seq: u64, message: String },
    #[error(
        "memory log is truncated: the candidate ledger commits {committed} entries \
         (head {head_digest}) but .impulse/MEMORY.jsonl holds {found}; the file is refused"
    )]
    Truncated {
        committed: u64,
        found: u64,
        head_digest: String,
    },
    #[error(
        "memory log committed head does not match .impulse/MEMORY.jsonl: the candidate ledger \
         commits entry {committed} with digest {expected} but the log holds {found}"
    )]
    HeadMismatch {
        committed: u64,
        expected: String,
        found: String,
    },
    #[error(
        "memory log holds {found} uncommitted trailing entries (at most \
         {MAX_UNCOMMITTED_TAIL_ENTRIES} is explainable by an interrupted decision); \
         .impulse/MEMORY.jsonl is refused"
    )]
    UncommittedTailTooLong { found: usize },
    #[error("memory log record `{record_id}` is not referenced by any promoted candidate")]
    OrphanRecord { record_id: MemoryRecordId },
    #[error("promoted candidate `{candidate_id}` names record `{record_id}`, which is not in the memory log")]
    MissingPromotedRecord {
        candidate_id: String,
        record_id: MemoryRecordId,
    },
    #[error("memory log holds more than one entry for record `{record_id}`")]
    DuplicateRecord { record_id: MemoryRecordId },
}

/// The committed head of the log, recorded in the candidate ledger.
///
/// Keeping the head in the *other* artifact is what makes trailing truncation
/// detectable at all: a hash chain on its own is still a valid chain after its
/// last lines are cut off. The two files are not merged — they are federated,
/// and each is the witness that catches tampering with the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLogHead {
    pub entry_count: u64,
    pub head_digest: String,
}

/// The verified log, split into what the ledger commits and an at-most-one
/// trailing entry left behind by an interrupted decision.
#[derive(Debug, Clone, Default)]
pub(super) struct MemoryLog {
    committed: Vec<MemoryLogEntry>,
    uncommitted_tail: Vec<MemoryLogEntry>,
}

impl MemoryLog {
    #[cfg(test)]
    pub(super) fn committed(&self) -> &[MemoryLogEntry] {
        &self.committed
    }

    pub(super) fn uncommitted_tail(&self) -> &[MemoryLogEntry] {
        &self.uncommitted_tail
    }

    /// Head over committed entries plus the uncommitted tail — the head a
    /// ledger commit is about to record.
    pub(super) fn full_head(&self) -> Option<MemoryLogHead> {
        let last = self
            .uncommitted_tail
            .last()
            .or_else(|| self.committed.last())?;
        Some(MemoryLogHead {
            entry_count: (self.committed.len() + self.uncommitted_tail.len()) as u64,
            head_digest: last.entry_digest.clone(),
        })
    }

    #[cfg(test)]
    pub(super) fn head(&self) -> Option<MemoryLogHead> {
        let last = self.committed.last()?;
        Some(MemoryLogHead {
            entry_count: self.committed.len() as u64,
            head_digest: last.entry_digest.clone(),
        })
    }

    /// Digest the next appended entry must chain to.
    pub(super) fn next_previous_digest(&self) -> String {
        self.uncommitted_tail
            .last()
            .or_else(|| self.committed.last())
            .map(|entry| entry.entry_digest.clone())
            .unwrap_or_else(|| MEMORY_LOG_GENESIS_DIGEST.to_string())
    }

    pub(super) fn next_seq(&self) -> u64 {
        (self.committed.len() + self.uncommitted_tail.len()) as u64
    }

    pub(super) fn committed_record(&self, record_id: &MemoryRecordId) -> Option<&MemoryRecord> {
        self.committed
            .iter()
            .map(|entry| &entry.record)
            .find(|record| &record.id == record_id)
    }

    /// Records the projection renders: every committed, not-yet-superseded
    /// record, in log order. Log order is the only deterministic order that
    /// does not depend on a wall clock.
    pub(super) fn projected_records(&self) -> Vec<&MemoryRecord> {
        let superseded = self
            .committed
            .iter()
            .filter_map(|entry| entry.record.superseded_by.clone())
            .collect::<BTreeSet<_>>();
        self.committed
            .iter()
            .map(|entry| &entry.record)
            .filter(|record| !superseded.contains(&record.id))
            .collect()
    }

    /// Read and fully verify `.impulse/MEMORY.jsonl`.
    ///
    /// Fails closed on a broken chain, a forged record digest, trailing
    /// truncation, a head-digest mismatch, or more than one uncommitted
    /// trailing entry. Never panics: every malformed line becomes a typed
    /// error.
    pub(super) fn load(storage: &Storage, committed_head: Option<&MemoryLogHead>) -> Result<Self> {
        let path = storage.path(MEMORY_LOG_FILE);
        let mut entries: Vec<MemoryLogEntry> = Vec::new();
        if path.exists() {
            let raw = std::fs::read_to_string(&path).context("Failed to read memory log")?;
            for (index, line) in raw.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let entry: MemoryLogEntry =
                    serde_json::from_str(line).map_err(|error| MemoryLogError::MalformedEntry {
                        seq: index as u64,
                        message: error.to_string(),
                    })?;
                entries.push(entry);
            }
        }

        let mut previous_digest = MEMORY_LOG_GENESIS_DIGEST.to_string();
        for (index, entry) in entries.iter().enumerate() {
            let seq = index as u64;
            entry.verify_links(seq, &previous_digest).map_err(|error| {
                MemoryLogError::BrokenChain {
                    seq,
                    message: error.to_string(),
                }
            })?;
            verify_record_identity(&entry.record).map_err(|error| MemoryLogError::BrokenChain {
                seq,
                message: error.to_string(),
            })?;
            let expected = seal_digest(seq, &previous_digest, &entry.record)?;
            if entry.entry_digest != expected {
                return Err(MemoryLogError::BrokenChain {
                    seq,
                    message: "entry digest does not cover its own content".to_string(),
                }
                .into());
            }
            previous_digest = entry.entry_digest.clone();
        }

        let committed_count = match committed_head {
            Some(head) => {
                let found = entries.len() as u64;
                if found < head.entry_count {
                    return Err(MemoryLogError::Truncated {
                        committed: head.entry_count,
                        found,
                        head_digest: head.head_digest.clone(),
                    }
                    .into());
                }
                let index = head.entry_count.saturating_sub(1) as usize;
                let actual = &entries[index].entry_digest;
                if actual != &head.head_digest {
                    return Err(MemoryLogError::HeadMismatch {
                        committed: head.entry_count,
                        expected: head.head_digest.clone(),
                        found: actual.clone(),
                    }
                    .into());
                }
                head.entry_count as usize
            }
            // No committed head recorded: every entry is uncommitted. An empty
            // log is the normal pre-first-promotion state.
            None => 0,
        };

        let uncommitted_tail = entries.split_off(committed_count);
        if uncommitted_tail.len() > MAX_UNCOMMITTED_TAIL_ENTRIES {
            return Err(MemoryLogError::UncommittedTailTooLong {
                found: uncommitted_tail.len(),
            }
            .into());
        }

        let mut seen = BTreeSet::new();
        for entry in entries.iter().chain(uncommitted_tail.iter()) {
            if !seen.insert(entry.record.id.clone()) {
                return Err(MemoryLogError::DuplicateRecord {
                    record_id: entry.record.id.clone(),
                }
                .into());
            }
        }

        Ok(Self {
            committed: entries,
            uncommitted_tail,
        })
    }

    /// Cross-check the verified log against the candidate ledger's own review
    /// decisions. Every committed record must be claimed by exactly one
    /// promoted candidate, and every promoted candidate must find its record.
    ///
    /// A record's `source.candidate_id` is deliberately *not* required to still
    /// exist: a derivation-version migration re-derives a candidate under a new
    /// id while carrying its status forward, and the log is append-only, so the
    /// record keeps naming the candidate it was actually promoted from.
    pub(super) fn cross_check_statuses<'a>(
        &self,
        candidates: impl Iterator<Item = &'a AcceptedRunMemoryCandidate>,
    ) -> Result<()> {
        let mut promoted = BTreeSet::new();
        for candidate in candidates {
            if let Some(record_id) = candidate.status.promoted_record_id() {
                if self.committed_record(record_id).is_none() {
                    return Err(MemoryLogError::MissingPromotedRecord {
                        candidate_id: candidate.id.to_string(),
                        record_id: record_id.clone(),
                    }
                    .into());
                }
                promoted.insert(record_id.clone());
            }
        }
        for entry in &self.committed {
            if !promoted.contains(&entry.record.id) {
                return Err(MemoryLogError::OrphanRecord {
                    record_id: entry.record.id.clone(),
                }
                .into());
            }
        }
        Ok(())
    }

    /// Append one sealed entry to the log file and to this in-memory view.
    ///
    /// The entry lands in the uncommitted tail until the candidate ledger
    /// records the new head.
    pub(super) fn append(
        &mut self,
        storage: &Storage,
        record: MemoryRecord,
    ) -> Result<MemoryLogEntry> {
        let seq = self.next_seq();
        let previous_digest = self.next_previous_digest();
        let entry_digest = seal_digest(seq, &previous_digest, &record)?;
        let entry = MemoryLogEntry {
            seq,
            previous_digest,
            record,
            entry_digest,
        };
        storage
            .append_jsonl(MEMORY_LOG_FILE, &entry)
            .context("Failed to append to the promoted memory log")?;
        self.uncommitted_tail.push(entry.clone());
        Ok(entry)
    }

    /// Promote the uncommitted tail into the committed prefix after the ledger
    /// has recorded the new head.
    pub(super) fn commit_tail(&mut self) {
        self.committed.append(&mut self.uncommitted_tail);
    }
}

/// Hash the canonical digest-source bytes of a log entry.
fn seal_digest(seq: u64, previous_digest: &str, record: &MemoryRecord) -> Result<String> {
    let bytes = MemoryLogEntry::digest_source_bytes(seq, previous_digest, record)
        .context("Failed to build memory log entry digest source")?;
    Ok(format!(
        "{MEMORY_LOG_DIGEST_PREFIX}{:x}",
        Sha256::digest(bytes)
    ))
}

/// Check that a record's id and digest really are the SHA-256 of its own
/// content, not merely two spellings of the same arbitrary hex.
pub(super) fn verify_record_identity(record: &MemoryRecord) -> Result<()> {
    record
        .validate_shape()
        .context("Invalid promoted memory record")?;
    let bytes = record
        .recompute_identity_source_bytes()
        .context("Failed to build memory record identity source")?;
    let hex = format!("{:x}", Sha256::digest(bytes));
    if record.id.as_str() != format!("{MEMORY_RECORD_ID_PREFIX}{hex}")
        || record.digest != format!("{MEMORY_RECORD_DIGEST_PREFIX}{hex}")
    {
        anyhow::bail!(
            "memory record `{}` id/digest do not match its own content digest",
            record.id
        );
    }
    Ok(())
}

/// Derive the durable record a promotion of `candidate` produces.
///
/// Deterministic in everything except `valid_from`, which is excluded from the
/// identity digest — so replaying the same decision request after a crash
/// re-derives the same record id even though the clock moved.
///
/// The body carries only what ADR-0013 rule 3 already allows into a candidate:
/// registration-time task text plus daemon-observed evidence metadata. No
/// worker, Supervisor, or operator prose is copied forward.
pub(super) fn derive_promoted_record(
    candidate: &AcceptedRunMemoryCandidate,
    request_id: &impulse_ops::governed_task::GovernedRequestId,
    based_on_ledger_revision: u64,
    valid_from: &str,
) -> Result<MemoryRecord> {
    let source = MemorySource::CandidateRef {
        candidate_id: candidate.id.clone(),
        candidate_source_digest: candidate.source_digest.clone(),
        governed_task_id: candidate.governed_task_id.clone(),
    };
    let title = candidate.task.clone();
    let body = render_record_body(candidate);
    let bytes = MemoryRecord::identity_source_bytes(
        MemoryKind::AcceptedRunOutcome,
        MemoryScope::Project,
        &candidate.project_id,
        &source,
        request_id,
        based_on_ledger_revision,
        &title,
        &body,
    )
    .context("Failed to build memory record identity source")?;
    let hex = format!("{:x}", Sha256::digest(bytes));
    let record = MemoryRecord {
        id: MemoryRecordId::try_new(format!("{MEMORY_RECORD_ID_PREFIX}{hex}"))
            .context("Failed to build memory record id")?,
        schema_version: MEMORY_RECORD_SCHEMA_VERSION,
        kind: MemoryKind::AcceptedRunOutcome,
        scope: MemoryScope::Project,
        project_id: candidate.project_id.clone(),
        source,
        request_id: request_id.clone(),
        based_on_ledger_revision,
        title,
        body,
        valid_from: valid_from.to_string(),
        superseded_by: None,
        digest: format!("{MEMORY_RECORD_DIGEST_PREFIX}{hex}"),
    };
    verify_record_identity(&record)?;
    Ok(record)
}

fn render_record_body(candidate: &AcceptedRunMemoryCandidate) -> String {
    let mut body = String::new();
    body.push_str(&candidate.proposed_summary);
    body.push_str("\n\nAcceptance criteria:\n");
    for criterion in &candidate.acceptance_criteria {
        body.push_str("- ");
        body.push_str(criterion);
        body.push('\n');
    }
    body.push_str(&format!(
        "\nVerified subject: {}\nVerification policy: {}\nGoverned task: {}\n",
        candidate.subject_revision, candidate.verification_policy, candidate.governed_task_id
    ));
    body
}

/// Render the deterministic GENOME projection.
///
/// Byte-identical for a given record set: no timestamp of its own, no map
/// iteration, records in log order. Regenerated wholesale, never merged into.
pub(super) fn render_projection(records: &[&MemoryRecord]) -> String {
    let mut out = String::new();
    out.push_str("# Impulse Memory Projection\n\n");
    out.push_str("<!-- generated: do not edit; regenerated from .impulse/MEMORY.jsonl -->\n\n");
    out.push_str(
        "*Promoted project memory. The raw candidate ledger and the promoted-record log are \
         separate artifacts; this file is a projection of the log and is never a source of \
         truth.*\n",
    );
    if records.is_empty() {
        out.push_str("\nNo promoted memory records.\n");
        return out;
    }
    for record in records {
        out.push_str(&format!("\n## {}\n\n", record.id));
        out.push_str(&format!("- scope: {}\n", record.scope.label()));
        out.push_str(&format!("- kind: {}\n", record.kind.label()));
        out.push_str(&format!("- valid_from: {}\n", record.valid_from));
        match &record.source {
            MemorySource::CandidateRef {
                candidate_id,
                candidate_source_digest,
                governed_task_id,
            } => {
                out.push_str(&format!(
                    "- source: candidate {candidate_id} ({candidate_source_digest}) from governed task {governed_task_id}\n"
                ));
            }
            MemorySource::OperatorManual { note } => {
                out.push_str(&format!("- source: operator manual — {note}\n"));
            }
        }
        out.push_str(&format!("- digest: {}\n", record.digest));
        out.push_str(&format!("\n### {}\n\n", record.title));
        out.push_str(record.body.trim_end());
        out.push('\n');
    }
    out
}

pub(super) fn projection_digest(rendered: &str) -> String {
    format!("sha256-proj-v1:{:x}", Sha256::digest(rendered.as_bytes()))
}

/// Retrieval-index dirty marker.
///
/// The state layer never opens the SQLite index during a decision — that would
/// put a database open inside the ledger lock. It records *that* the projection
/// moved; the indexer reads this marker, reindexes, and stamps
/// `indexed_digest`. `dirty` is therefore `projection_digest != indexed_digest`
/// made explicit rather than inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryIndexMarker {
    pub schema_version: u32,
    pub projection_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_digest: Option<String>,
    pub marked_at: String,
}

impl MemoryIndexMarker {
    pub fn is_dirty(&self) -> bool {
        self.indexed_digest.as_deref() != Some(self.projection_digest.as_str())
    }
}

/// Read the marker, if one exists.
pub fn read_index_marker(storage: &Storage) -> Result<Option<MemoryIndexMarker>> {
    let path = storage.path(MEMORY_INDEX_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).context("Failed to read memory index marker")?;
    let marker = serde_json::from_str(&raw).context("Failed to parse memory index marker")?;
    Ok(Some(marker))
}

/// Regenerate the projection and, when the projection actually changed, mark
/// the retrieval index dirty.
///
/// Returns the projection digest and whether the index was left dirty. A
/// dismissal regenerates nothing new — the digest is unchanged — so it never
/// marks the index dirty. That is ADR-0013 rule 9 held in place: only a
/// promoted record ever reaches the index.
pub(super) fn write_projection(
    storage: &Storage,
    records: &[&MemoryRecord],
    now: &str,
    mark_index_dirty: bool,
) -> Result<(String, bool)> {
    let rendered = render_projection(records);
    let digest = projection_digest(&rendered);
    let path = storage.path(GENOME_PROJECTION_FILE);
    let existing = std::fs::read_to_string(&path).ok();
    if existing.as_deref() != Some(rendered.as_str()) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create the projection's parent directory")?;
        }
        storage
            .atomic_write(&path, rendered.as_bytes())
            .context("Failed to write the GENOME projection")?;
    }

    let previous = read_index_marker(storage)?;
    let indexed_digest = if mark_index_dirty {
        None
    } else if records.is_empty() {
        // An empty projection is trivially indexed: there is nothing for the
        // indexer to do, so it must not be reported as work outstanding.
        Some(digest.clone())
    } else {
        previous.as_ref().and_then(|marker| {
            // Keep an existing clean marker clean when nothing changed.
            (marker.projection_digest == digest)
                .then(|| marker.indexed_digest.clone())
                .flatten()
        })
    };
    let marker = MemoryIndexMarker {
        schema_version: MEMORY_INDEX_SCHEMA_VERSION,
        projection_digest: digest.clone(),
        indexed_digest,
        marked_at: now.to_string(),
    };
    let dirty = marker.is_dirty();
    if previous.as_ref() != Some(&marker) {
        storage
            .write_json(MEMORY_INDEX_FILE, &marker)
            .context("Failed to write the memory retrieval index marker")?;
    }
    Ok((digest, dirty))
}

/// Stamp the retrieval-index marker clean after a successful reindex.
pub(super) fn stamp_indexed(storage: &Storage, digest: &str, now: &str) -> Result<()> {
    let marker = MemoryIndexMarker {
        schema_version: MEMORY_INDEX_SCHEMA_VERSION,
        projection_digest: digest.to_string(),
        indexed_digest: Some(digest.to_string()),
        marked_at: now.to_string(),
    };
    storage
        .write_json(MEMORY_INDEX_FILE, &marker)
        .context("Failed to stamp the memory retrieval index marker")
}

/// Carry a review decision forward across a candidate re-derivation.
///
/// ADR-0018 follow-up 2: dropping and re-deriving a candidate is lossless only
/// while `MemoryCandidateStatus` has one variant. Once a candidate can be
/// promoted or dismissed, a prune would silently revert an operator's decision
/// to `PendingReview`.
pub(super) fn carry_status_forward(
    stored: &MemoryCandidateStatus,
    rederived: &mut AcceptedRunMemoryCandidate,
) {
    if !stored.is_pending() {
        rederived.status = stored.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_ops::governed_task::GovernedRequestId;

    fn storage() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().to_path_buf());
        (dir, storage)
    }

    pub(super) fn candidate() -> AcceptedRunMemoryCandidate {
        crate::state::memory_candidate::tests::sample_candidate()
    }

    fn request(id: &str) -> GovernedRequestId {
        GovernedRequestId::try_new(id).unwrap()
    }

    #[test]
    fn test_derive_promoted_record_is_deterministic_and_clock_independent() {
        let candidate = candidate();
        let first =
            derive_promoted_record(&candidate, &request("r-1"), 3, "2026-09-12T10:00:00Z").unwrap();
        let later =
            derive_promoted_record(&candidate, &request("r-1"), 3, "2030-01-01T00:00:00Z").unwrap();
        assert_eq!(first.id, later.id);
        assert_eq!(first.digest, later.digest);
        assert_ne!(first.valid_from, later.valid_from);

        let other_request =
            derive_promoted_record(&candidate, &request("r-2"), 3, "2026-09-12T10:00:00Z").unwrap();
        assert_ne!(first.id, other_request.id);

        let other_revision =
            derive_promoted_record(&candidate, &request("r-1"), 4, "2026-09-12T10:00:00Z").unwrap();
        assert_ne!(first.id, other_revision.id);
    }

    #[test]
    fn test_derive_promoted_record_carries_no_producer_prose() {
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        for forbidden in ["worker_claim_summary", "supervisor_rationale", "rationale"] {
            assert!(
                !serde_json::to_string(&record).unwrap().contains(forbidden),
                "record must not carry `{forbidden}`"
            );
        }
    }

    #[test]
    fn test_empty_log_loads_clean_and_has_no_head() {
        let (_dir, storage) = storage();
        let log = MemoryLog::load(&storage, None).unwrap();
        assert!(log.committed().is_empty());
        assert!(log.uncommitted_tail().is_empty());
        assert!(log.head().is_none());
        assert_eq!(log.next_seq(), 0);
        assert_eq!(log.next_previous_digest(), MEMORY_LOG_GENESIS_DIGEST);
    }

    #[test]
    fn test_append_then_commit_produces_a_verifiable_chain() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        log.append(&storage, record.clone()).unwrap();
        assert_eq!(log.uncommitted_tail().len(), 1);
        log.commit_tail();
        let head = log.head().unwrap();
        assert_eq!(head.entry_count, 1);

        let reloaded = MemoryLog::load(&storage, Some(&head)).unwrap();
        assert_eq!(reloaded.committed().len(), 1);
        assert!(reloaded.uncommitted_tail().is_empty());
        assert_eq!(reloaded.committed()[0].record, record);
    }

    #[test]
    fn test_tampered_log_fails_closed_without_panicking() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        log.append(&storage, record).unwrap();
        log.commit_tail();
        let head = log.head().unwrap();

        let path = storage.path(MEMORY_LOG_FILE);
        let raw = std::fs::read_to_string(&path).unwrap();
        let tampered = raw.replace("Verified subject", "Unverified subject");
        assert_ne!(tampered, raw);
        std::fs::write(&path, tampered).unwrap();

        let error = MemoryLog::load(&storage, Some(&head)).unwrap_err();
        assert!(
            format!("{error:#}").contains("modified outside Impulse"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_truncated_log_fails_closed_without_panicking() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        for (index, request_id) in ["r-1", "r-2"].iter().enumerate() {
            let record = derive_promoted_record(
                &candidate(),
                &request(request_id),
                index as u64,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
            log.append(&storage, record).unwrap();
            log.commit_tail();
        }
        let head = log.head().unwrap();
        assert_eq!(head.entry_count, 2);

        let path = storage.path(MEMORY_LOG_FILE);
        let raw = std::fs::read_to_string(&path).unwrap();
        let first_line = raw.lines().next().unwrap().to_string();
        std::fs::write(&path, format!("{first_line}\n")).unwrap();

        let error = MemoryLog::load(&storage, Some(&head)).unwrap_err();
        assert!(
            format!("{error:#}").contains("truncated"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_partially_truncated_last_line_fails_closed_without_panicking() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        log.append(&storage, record).unwrap();
        log.commit_tail();
        let head = log.head().unwrap();

        let path = storage.path(MEMORY_LOG_FILE);
        let raw = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, &raw[..raw.len() / 2]).unwrap();

        let error = MemoryLog::load(&storage, Some(&head)).unwrap_err();
        assert!(
            format!("{error:#}").contains("malformed"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_head_digest_mismatch_fails_closed() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        log.append(&storage, record).unwrap();
        log.commit_tail();
        let mut head = log.head().unwrap();
        head.head_digest = format!("{MEMORY_LOG_DIGEST_PREFIX}{}", "0".repeat(64));

        let error = MemoryLog::load(&storage, Some(&head)).unwrap_err();
        assert!(
            format!("{error:#}").contains("committed head does not match"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_two_uncommitted_trailing_entries_fail_closed() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        for request_id in ["r-1", "r-2"] {
            let record = derive_promoted_record(
                &candidate(),
                &request(request_id),
                0,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
            log.append(&storage, record).unwrap();
        }
        let error = MemoryLog::load(&storage, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("uncommitted trailing entries"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_projection_regeneration_is_byte_identical_across_runs() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        log.append(&storage, record).unwrap();
        log.commit_tail();

        let records = log.projected_records();
        let first = render_projection(&records);
        let second = render_projection(&records);
        assert_eq!(first, second);
        assert_eq!(projection_digest(&first), projection_digest(&second));
        assert!(first.contains("do not edit"));

        let (digest_a, _) =
            write_projection(&storage, &records, "2026-09-12T10:00:00Z", true).unwrap();
        let bytes_a = std::fs::read(storage.path(GENOME_PROJECTION_FILE)).unwrap();
        let (digest_b, _) =
            write_projection(&storage, &records, "2026-09-12T11:00:00Z", true).unwrap();
        let bytes_b = std::fs::read(storage.path(GENOME_PROJECTION_FILE)).unwrap();
        assert_eq!(digest_a, digest_b);
        assert_eq!(bytes_a, bytes_b);
    }

    #[test]
    fn test_empty_projection_renders_and_does_not_mark_the_index_dirty() {
        let (_dir, storage) = storage();
        let (digest, dirty) =
            write_projection(&storage, &[], "2026-09-12T10:00:00Z", false).unwrap();
        assert!(!dirty, "an empty projection has nothing to index");
        let marker = read_index_marker(&storage).unwrap().unwrap();
        assert_eq!(marker.indexed_digest.as_deref(), Some(digest.as_str()));
        assert!(read_index_marker(&Storage::new(
            tempfile::tempdir().unwrap().path().to_path_buf()
        ))
        .unwrap()
        .is_none());
    }

    #[test]
    fn test_index_marker_round_trips_and_reports_clean_when_digests_match() {
        let marker = MemoryIndexMarker {
            schema_version: MEMORY_INDEX_SCHEMA_VERSION,
            projection_digest: "sha256-proj-v1:abc".to_string(),
            indexed_digest: Some("sha256-proj-v1:abc".to_string()),
            marked_at: "2026-09-12T10:00:00Z".to_string(),
        };
        assert!(!marker.is_dirty());
        let decoded: MemoryIndexMarker =
            serde_json::from_str(&serde_json::to_string(&marker).unwrap()).unwrap();
        assert_eq!(decoded, marker);

        let mut moved = marker;
        moved.projection_digest = "sha256-proj-v1:def".to_string();
        assert!(moved.is_dirty());
    }

    #[test]
    fn test_carry_status_forward_preserves_a_decision_and_leaves_pending_alone() {
        let stored = MemoryCandidateStatus::Dismissed {
            reason: "duplicate".to_string(),
            decided_at: "2026-09-12T10:00:00Z".to_string(),
            decided_by: impulse_ops::governed_task::GovernedActor {
                kind: impulse_ops::governed_task::GovernedActorKind::Operator,
                id: "operator-a".to_string(),
            },
        };
        let mut rederived = candidate();
        carry_status_forward(&stored, &mut rederived);
        assert_eq!(rederived.status, stored);

        let mut untouched = candidate();
        carry_status_forward(&MemoryCandidateStatus::PendingReview, &mut untouched);
        assert!(untouched.status.is_pending());
    }

    #[test]
    fn test_cross_check_rejects_an_orphan_record_and_a_missing_promoted_record() {
        let (_dir, storage) = storage();
        let mut log = MemoryLog::load(&storage, None).unwrap();
        let record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        let record_id = record.id.clone();
        log.append(&storage, record).unwrap();
        log.commit_tail();

        let pending = candidate();
        let error = log
            .cross_check_statuses([&pending].into_iter())
            .unwrap_err();
        assert!(format!("{error:#}").contains("not referenced by any promoted candidate"));

        let mut promoted = candidate();
        promoted.status = MemoryCandidateStatus::Promoted {
            record_id: record_id.clone(),
            decided_at: "2026-09-12T10:00:00Z".to_string(),
            decided_by: impulse_ops::governed_task::GovernedActor {
                kind: impulse_ops::governed_task::GovernedActorKind::Operator,
                id: "operator-a".to_string(),
            },
        };
        log.cross_check_statuses([&promoted].into_iter()).unwrap();

        let mut dangling = candidate();
        dangling.status = MemoryCandidateStatus::Promoted {
            record_id: MemoryRecordId::try_new(format!(
                "{MEMORY_RECORD_ID_PREFIX}{}",
                "9".repeat(64)
            ))
            .unwrap(),
            decided_at: "2026-09-12T10:00:00Z".to_string(),
            decided_by: impulse_ops::governed_task::GovernedActor {
                kind: impulse_ops::governed_task::GovernedActorKind::Operator,
                id: "operator-a".to_string(),
            },
        };
        let error = MemoryLog::load(&storage, log.head().as_ref())
            .unwrap()
            .cross_check_statuses([&dangling].into_iter())
            .unwrap_err();
        assert!(format!("{error:#}").contains("not in the memory log"));
    }

    #[test]
    fn test_verify_record_identity_rejects_a_forged_digest() {
        let mut record =
            derive_promoted_record(&candidate(), &request("r-1"), 3, "2026-09-12T10:00:00Z")
                .unwrap();
        verify_record_identity(&record).unwrap();
        record.body.push_str(" tampered");
        let error = verify_record_identity(&record).unwrap_err();
        assert!(format!("{error:#}").contains("do not match"));
    }
}
