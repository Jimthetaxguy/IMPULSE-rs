//! Comparative settlement — the record produced when a WorkItem fans out to N
//! candidate runs and one of them is chosen.
//!
//! Settlement over N candidates is a comparison, not an acceptance. The record
//! carries four required parts, and none of them is optional:
//!
//! 1. a **per-candidate check matrix** — every candidate ran every acceptance
//!    check, so the comparison is between like and like;
//! 2. a **selection rationale** naming the concrete difference that decided it;
//! 3. a **graft record** whenever the winner absorbs a piece of a loser, with
//!    the loser's archive still on disk to review it against;
//! 4. **dissent** — a losing candidate that passed every check is recorded with
//!    its diff summary, because the road not taken is often the question being
//!    asked six months later.
//!
//! [`SettlementRecord::new`] enforces all four. There is no separate `validate`
//! step to forget to call: an invalid record cannot be constructed, and a
//! deserialized record is re-validated through the same constructor.
//!
//! Eligibility composes two predicates and is checked before comparison, not
//! after. A candidate settles only if its [`BasisVerdict`] is still fresh
//! (see [`crate::basis`]) *and* no fatal acceptance check failed. An attractive
//! diff summary is not a third predicate.
//!
//! **Out of scope here:** the spec's fourth acceptance check — fan-out over a
//! WorkItem whose capability set includes an Irreversible effect class must fail
//! at *planning* time, before any candidate runs. That is the planner's gate,
//! not settlement's, so this module carries no effect-class field and no
//! placeholder for one. By the time a settlement record exists, the fan-out
//! already happened; refusing it here would be too late to be a refusal.
//!
//! Like [`crate::basis`], this module is self-contained: it owns its own types
//! and does not depend on the WorkItem/WorkGraph types, whose ADR is not yet
//! ratified. Wiring settlement into any orchestrator is a separate change.

use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use crate::basis::BasisVerdict;

/// Default cap on fan-out width. N candidates cost roughly N times the tokens
/// and the disk of one, and a comparison nobody reads is not a comparison.
pub const DEFAULT_CANDIDATE_CAP: usize = 8;

/// Minimum length of a selection rationale, in bytes after trimming.
///
/// Naming the concrete difference between the top candidates takes a sentence.
/// A score, a candidate id, or "better" fits in less.
pub const MIN_RATIONALE_BYTES: usize = 40;

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum SettlementError {
    /// Modeled on the governed-record id contract: open strings, but never
    /// whitespace, control characters, or unbounded length.
    #[error(
        "invalid {kind} `{value}`: ids must be nonempty, at most 256 bytes, and contain no \
         whitespace or control characters"
    )]
    InvalidId { kind: &'static str, value: String },

    #[error(
        "no candidates: a settlement record with nothing to compare is a decision presented as a \
         comparison"
    )]
    NoCandidates,

    #[error(
        "single candidate `{id}`: N=1 needs no comparative record — settle the run directly rather \
         than recording a comparison that had no alternative"
    )]
    SingleCandidate { id: CandidateId },

    #[error("duplicate candidate `{id}`: the candidate set holds one result per candidate")]
    DuplicateCandidate { id: CandidateId },

    #[error(
        "unknown candidate `{id}` in `{field}`: a settlement may only name candidates it compared"
    )]
    UnknownCandidate {
        field: &'static str,
        id: CandidateId,
    },

    #[error(
        "candidate `{id}` cannot be selected: {check} — a fatal check failure is disqualifying, \
         not one attribute to weigh against an attractive diff"
    )]
    IneligibleSelection { id: CandidateId, check: String },

    #[error(
        "candidate `{candidate}` never ran check `{check}`: the matrix must be complete — a \
         candidate that skipped a check was not compared, it was excused"
    )]
    MissingCheck {
        candidate: CandidateId,
        check: CheckId,
    },

    #[error(
        "selection rationale is {length} bytes, below the minimum of {minimum}: name the concrete \
         difference between the top candidates — a bare score is not a rationale"
    )]
    RationaleTooShort { length: usize, minimum: usize },

    #[error(
        "graft from candidate `{candidate}` references archive `{path}` which is not on disk: \
         archives must outlive their grafts, or the winner carries a piece no one can review"
    )]
    ArchiveMissing {
        candidate: CandidateId,
        path: PathBuf,
    },

    #[error(
        "candidate `{id}` passed every check, lost, and is absent from dissent: a losing candidate \
         that passed is the road not taken, and it is recorded or it is gone"
    )]
    PassingCandidateNotRecorded { id: CandidateId },

    #[error(
        "{count} candidates exceeds the cap of {cap}: fan-out costs N times the tokens and disk of \
         one run — narrow the WorkItem instead of widening the panel"
    )]
    OverCap { count: usize, cap: usize },
}

// ============================================================================
// Identifiers
// ============================================================================

fn validate_open_id(kind: &'static str, value: String) -> Result<String, SettlementError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(SettlementError::InvalidId { kind, value });
    }
    Ok(value)
}

macro_rules! settlement_id {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn try_new(value: impl Into<String>) -> Result<Self, SettlementError> {
                validate_open_id($kind, value.into()).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_new(value).map_err(D::Error::custom)
            }
        }
    };
}

settlement_id!(
    CandidateId,
    "candidate_id",
    "One candidate run of a fanned-out WorkItem."
);
settlement_id!(
    CheckId,
    "check_id",
    "One acceptance check, owned by the WorkItem and inherited by every candidate."
);

// ============================================================================
// Check matrix
// ============================================================================

/// The outcome of one acceptance check on one candidate.
///
/// Three-way, mirroring the governed verification vocabulary: a check that
/// could not reach a verdict is [`CheckOutcome::Inconclusive`], never a
/// silent pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    Failed,
    Inconclusive,
}

impl fmt::Display for CheckOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Inconclusive => "inconclusive",
        })
    }
}

/// One cell of the check matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: CheckId,
    pub outcome: CheckOutcome,
    /// A fatal check that failed disqualifies the candidate outright.
    pub fatal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

impl CheckResult {
    pub fn new(check: CheckId, outcome: CheckOutcome, fatal: bool) -> Self {
        Self {
            check,
            outcome,
            fatal,
            evidence: None,
        }
    }

    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }

    /// Whether this cell alone disqualifies its candidate.
    pub fn disqualifying(&self) -> bool {
        self.fatal && self.outcome == CheckOutcome::Failed
    }
}

// ============================================================================
// Candidates
// ============================================================================

/// One candidate run's result: what it produced, what it planned against, and
/// how it scored on every acceptance check.
///
/// The basis verdict is embedded by value. A basis has no id to reference and
/// no store to look it up in; the verdict travels with the result it justifies
/// so a settlement record read later is self-contained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateResult {
    pub id: CandidateId,
    /// Where the candidate's worktree was archived. Losers are archived, never
    /// deleted, until the graft-bearing winner itself settles.
    pub worktree_archive: PathBuf,
    pub diff_summary: String,
    pub basis_verdict: BasisVerdict,
    pub checks: Vec<CheckResult>,
}

impl CandidateResult {
    /// Whether this candidate may be selected at all.
    ///
    /// Eligibility composes both companion predicates: the basis must still be
    /// fresh, and no fatal check may have failed. Neither is weighable against
    /// the diff.
    pub fn eligible(&self) -> bool {
        self.ineligibility_reason().is_none()
    }

    /// Whether every check passed. An empty matrix is not a clean sweep.
    pub fn passed_all(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .iter()
                .all(|result| result.outcome == CheckOutcome::Passed)
    }

    /// Why this candidate cannot be selected, or `None` if it can.
    ///
    /// The basis is reported first: a stale basis means the comparison itself
    /// was run against state that has since moved.
    pub fn ineligibility_reason(&self) -> Option<String> {
        if !self.basis_verdict.settles() {
            let moved: Vec<String> = self
                .basis_verdict
                .mismatches()
                .iter()
                .map(|mismatch| mismatch.source.clone())
                .collect();
            return Some(format!(
                "basis stale, {} source(s) moved since plan: {}",
                moved.len(),
                moved.join(", ")
            ));
        }
        self.checks
            .iter()
            .find(|result| result.disqualifying())
            .map(|result| format!("fatal check `{}` failed", result.check))
    }

    fn check_ids(&self) -> HashSet<CheckId> {
        self.checks
            .iter()
            .map(|result| result.check.clone())
            .collect()
    }
}

// ============================================================================
// Grafts and dissent
// ============================================================================

/// A piece the winner absorbed from a loser — the judge-panel pattern, recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graft {
    pub from: CandidateId,
    pub piece: String,
}

/// A losing candidate preserved with its diff summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dissent {
    pub candidate: CandidateId,
    pub diff_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ============================================================================
// Settlement record
// ============================================================================

/// The comparative settlement of a fanned-out WorkItem.
///
/// Fields are private and there is no `validate` method, because every path to
/// a value of this type — including deserialization — runs
/// [`SettlementRecord::new`]. An incomplete or self-contradicting settlement
/// does not exist as a value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettlementRecord {
    candidates: Vec<CandidateResult>,
    selected: CandidateId,
    rationale: String,
    grafts: Vec<Graft>,
    dissent: Vec<Dissent>,
}

impl SettlementRecord {
    /// Record a settlement, enforcing every part of the contract.
    ///
    /// This touches the filesystem: each graft's source archive must still
    /// exist, exactly as [`crate::basis::BasisSet::capture`] reads the sources
    /// it records.
    pub fn new(
        candidates: Vec<CandidateResult>,
        selected: CandidateId,
        rationale: impl Into<String>,
        grafts: Vec<Graft>,
        dissent: Vec<Dissent>,
    ) -> Result<Self, SettlementError> {
        Self::new_with_cap(
            candidates,
            selected,
            rationale,
            grafts,
            dissent,
            DEFAULT_CANDIDATE_CAP,
        )
    }

    pub fn new_with_cap(
        candidates: Vec<CandidateResult>,
        selected: CandidateId,
        rationale: impl Into<String>,
        grafts: Vec<Graft>,
        dissent: Vec<Dissent>,
        cap: usize,
    ) -> Result<Self, SettlementError> {
        let rationale = rationale.into();

        let first = match candidates.first() {
            Some(first) => first,
            None => return Err(SettlementError::NoCandidates),
        };
        if candidates.len() == 1 {
            return Err(SettlementError::SingleCandidate {
                id: first.id.clone(),
            });
        }
        if candidates.len() > cap {
            return Err(SettlementError::OverCap {
                count: candidates.len(),
                cap,
            });
        }

        let mut seen = HashSet::with_capacity(candidates.len());
        for candidate in &candidates {
            if !seen.insert(candidate.id.clone()) {
                return Err(SettlementError::DuplicateCandidate {
                    id: candidate.id.clone(),
                });
            }
        }

        // Part 1: the check matrix must be complete before anything is compared.
        let every_check: HashSet<CheckId> = candidates
            .iter()
            .flat_map(|candidate| candidate.check_ids())
            .collect();
        for candidate in &candidates {
            let ran = candidate.check_ids();
            if let Some(missing) = every_check.difference(&ran).min() {
                return Err(SettlementError::MissingCheck {
                    candidate: candidate.id.clone(),
                    check: missing.clone(),
                });
            }
        }

        let winner = candidates
            .iter()
            .find(|candidate| candidate.id == selected)
            .ok_or_else(|| SettlementError::UnknownCandidate {
                field: "selected",
                id: selected.clone(),
            })?;
        if let Some(reason) = winner.ineligibility_reason() {
            return Err(SettlementError::IneligibleSelection {
                id: selected.clone(),
                check: reason,
            });
        }

        // Part 2: a rationale names the difference that decided it.
        let trimmed = rationale.trim();
        if trimmed.len() < MIN_RATIONALE_BYTES {
            return Err(SettlementError::RationaleTooShort {
                length: trimmed.len(),
                minimum: MIN_RATIONALE_BYTES,
            });
        }

        // Part 3: a graft's source archive must outlive the graft.
        for graft in &grafts {
            let source = candidates
                .iter()
                .find(|candidate| candidate.id == graft.from)
                .ok_or_else(|| SettlementError::UnknownCandidate {
                    field: "graft.from",
                    id: graft.from.clone(),
                })?;
            if !source.worktree_archive.exists() {
                return Err(SettlementError::ArchiveMissing {
                    candidate: graft.from.clone(),
                    path: source.worktree_archive.clone(),
                });
            }
        }

        // Part 4: a loser that passed everything is preserved or it is gone.
        let mut recorded: HashSet<CandidateId> = HashSet::with_capacity(dissent.len());
        for entry in &dissent {
            if !seen.contains(&entry.candidate) {
                return Err(SettlementError::UnknownCandidate {
                    field: "dissent.candidate",
                    id: entry.candidate.clone(),
                });
            }
            recorded.insert(entry.candidate.clone());
        }
        for candidate in &candidates {
            if candidate.id != selected
                && candidate.passed_all()
                && !recorded.contains(&candidate.id)
            {
                return Err(SettlementError::PassingCandidateNotRecorded {
                    id: candidate.id.clone(),
                });
            }
        }

        Ok(Self {
            candidates,
            selected,
            rationale,
            grafts,
            dissent,
        })
    }

    pub fn candidates(&self) -> &[CandidateResult] {
        &self.candidates
    }

    pub fn selected(&self) -> &CandidateId {
        &self.selected
    }

    /// The winning candidate's full result.
    pub fn winner(&self) -> &CandidateResult {
        self.candidates
            .iter()
            .find(|candidate| candidate.id == self.selected)
            .unwrap_or_else(|| unreachable!("the selected candidate is validated in new()"))
    }

    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    pub fn grafts(&self) -> &[Graft] {
        &self.grafts
    }

    pub fn dissent(&self) -> &[Dissent] {
        &self.dissent
    }

    /// Human-readable comparison: the matrix, the rationale, the grafts, and
    /// the dissent, in that order.
    pub fn matrix_report(&self) -> String {
        let mut out = format!(
            "settlement: selected `{}` from {} candidates\n",
            self.selected,
            self.candidates.len()
        );
        out.push_str(&format!("  rationale: {}\n", self.rationale.trim()));
        for candidate in &self.candidates {
            let standing = if candidate.id == self.selected {
                String::from("selected")
            } else {
                match candidate.ineligibility_reason() {
                    Some(reason) => format!("ineligible: {reason}"),
                    None => String::from("eligible"),
                }
            };
            out.push_str(&format!("  candidate `{}` [{standing}]\n", candidate.id));
            for result in &candidate.checks {
                let fatal = if result.fatal { " (fatal)" } else { "" };
                out.push_str(&format!(
                    "    check `{}`: {}{fatal}\n",
                    result.check, result.outcome
                ));
            }
        }
        for graft in &self.grafts {
            out.push_str(&format!(
                "  graft: `{}` from candidate `{}`\n",
                graft.piece, graft.from
            ));
        }
        for entry in &self.dissent {
            out.push_str(&format!(
                "  dissent: candidate `{}` — {}\n",
                entry.candidate, entry.diff_summary
            ));
        }
        out
    }
}

/// Wire shape for [`SettlementRecord`], used only to re-run the constructor.
#[derive(Deserialize)]
struct SettlementRecordWire {
    candidates: Vec<CandidateResult>,
    selected: CandidateId,
    rationale: String,
    #[serde(default)]
    grafts: Vec<Graft>,
    #[serde(default)]
    dissent: Vec<Dissent>,
}

impl<'de> Deserialize<'de> for SettlementRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SettlementRecordWire::deserialize(deserializer)?;
        Self::new(
            wire.candidates,
            wire.selected,
            wire.rationale,
            wire.grafts,
            wire.dissent,
        )
        .map_err(D::Error::custom)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basis::Mismatch;
    use std::fs;
    use tempfile::TempDir;

    const RATIONALE: &str =
        "candidate b keeps the daemon lock out of the subprocess await; candidate a holds it \
         across the harness call";

    fn candidate_id(value: &str) -> CandidateId {
        CandidateId::try_new(value).expect("valid candidate id")
    }

    fn check_id(value: &str) -> CheckId {
        CheckId::try_new(value).expect("valid check id")
    }

    fn passed(name: &str) -> CheckResult {
        CheckResult::new(check_id(name), CheckOutcome::Passed, true)
    }

    fn failed_fatal(name: &str) -> CheckResult {
        CheckResult::new(check_id(name), CheckOutcome::Failed, true)
    }

    fn fresh() -> BasisVerdict {
        BasisVerdict::Fresh {
            unverifiable: Vec::new(),
        }
    }

    fn stale() -> BasisVerdict {
        BasisVerdict::Stale {
            mismatches: vec![Mismatch {
                source: "file:/tmp/plan-input.md".into(),
                planned: "aaa".into(),
                found: "bbb".into(),
            }],
            unverifiable: Vec::new(),
        }
    }

    /// A candidate whose worktree archive is a real directory on disk.
    fn candidate(dir: &TempDir, id: &str, checks: Vec<CheckResult>) -> CandidateResult {
        let archive = dir.path().join(format!("{id}-archive"));
        fs::create_dir_all(&archive).expect("create archive dir");
        CandidateResult {
            id: candidate_id(id),
            worktree_archive: archive,
            diff_summary: format!("candidate {id}: 3 files changed"),
            basis_verdict: fresh(),
            checks,
        }
    }

    // --- Acceptance check 1: a record missing any of the four parts fails ----

    #[test]
    fn test_new_complete_record_with_all_four_parts_constructs() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build"), passed("clippy")]),
            candidate(&dir, "b", vec![passed("build"), passed("clippy")]),
        ];

        let record = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            vec![Graft {
                from: candidate_id("a"),
                piece: "the bounded-read helper".into(),
            }],
            vec![Dissent {
                candidate: candidate_id("a"),
                diff_summary: "candidate a: 3 files changed".into(),
                note: Some("kept the lock, simpler control flow".into()),
            }],
        )
        .expect("a record with all four parts must construct");

        assert_eq!(record.selected(), &candidate_id("b"));
        assert_eq!(record.winner().id, candidate_id("b"));
        assert_eq!(record.candidates().len(), 2);
        assert_eq!(record.grafts().len(), 1);
        assert_eq!(record.dissent().len(), 1);
        assert_eq!(record.rationale(), RATIONALE);
    }

    #[test]
    fn test_new_incomplete_check_matrix_returns_missing_check() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build"), passed("clippy")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            Vec::new(),
            vec![Dissent {
                candidate: candidate_id("a"),
                diff_summary: "candidate a: 3 files changed".into(),
                note: None,
            }],
        )
        .expect_err("an incomplete matrix is not a comparison");

        match err {
            SettlementError::MissingCheck { candidate, check } => {
                assert_eq!(candidate, candidate_id("b"), "b is the one that skipped it");
                assert_eq!(check, check_id("clippy"));
            }
            other => panic!("expected MissingCheck, got {other:?}"),
        }
    }

    #[test]
    fn test_new_rationale_of_a_bare_score_returns_rationale_too_short() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            "9/10",
            Vec::new(),
            vec![Dissent {
                candidate: candidate_id("a"),
                diff_summary: "candidate a: 3 files changed".into(),
                note: None,
            }],
        )
        .expect_err("a bare score is not a rationale");

        assert!(
            matches!(
                err,
                SettlementError::RationaleTooShort {
                    length: 4,
                    minimum: MIN_RATIONALE_BYTES
                }
            ),
            "expected RationaleTooShort, got {err:?}"
        );
    }

    #[test]
    fn test_new_passing_loser_absent_from_dissent_returns_passing_candidate_not_recorded() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a losing candidate that passed everything must be preserved");

        match err {
            SettlementError::PassingCandidateNotRecorded { id } => {
                assert_eq!(id, candidate_id("a"));
            }
            other => panic!("expected PassingCandidateNotRecorded, got {other:?}"),
        }
    }

    #[test]
    fn test_new_losing_candidate_that_failed_a_check_needs_no_dissent_entry() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![failed_fatal("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let record = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect("only a loser that passed everything must be recorded");

        assert!(record.dissent().is_empty());
    }

    #[test]
    fn test_new_no_candidates_returns_no_candidates() {
        let err = SettlementRecord::new(
            Vec::new(),
            candidate_id("a"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("nothing to compare is not a settlement");
        assert!(matches!(err, SettlementError::NoCandidates));
    }

    #[test]
    fn test_new_single_candidate_returns_single_candidate() {
        let dir = TempDir::new().expect("tempdir");
        let err = SettlementRecord::new(
            vec![candidate(&dir, "a", vec![passed("build")])],
            candidate_id("a"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("N=1 needs no comparative record");

        match err {
            SettlementError::SingleCandidate { id } => assert_eq!(id, candidate_id("a")),
            other => panic!("expected SingleCandidate, got {other:?}"),
        }
    }

    #[test]
    fn test_new_duplicate_candidate_returns_duplicate_candidate() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build")]),
            candidate(&dir, "a", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("a"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("the candidate set holds one result per candidate");
        assert!(matches!(err, SettlementError::DuplicateCandidate { .. }));
    }

    #[test]
    fn test_new_selected_not_among_candidates_returns_unknown_candidate() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("c"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a settlement may only select a candidate it compared");

        match err {
            SettlementError::UnknownCandidate { field, id } => {
                assert_eq!(field, "selected");
                assert_eq!(id, candidate_id("c"));
            }
            other => panic!("expected UnknownCandidate, got {other:?}"),
        }
    }

    #[test]
    fn test_new_dissent_for_uncompared_candidate_returns_unknown_candidate() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![failed_fatal("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            Vec::new(),
            vec![Dissent {
                candidate: candidate_id("ghost"),
                diff_summary: "never ran".into(),
                note: None,
            }],
        )
        .expect_err("dissent must name a compared candidate");

        match err {
            SettlementError::UnknownCandidate { field, id } => {
                assert_eq!(field, "dissent.candidate");
                assert_eq!(id, candidate_id("ghost"));
            }
            other => panic!("expected UnknownCandidate, got {other:?}"),
        }
    }

    #[test]
    fn test_new_over_cap_returns_over_cap_naming_the_fan_out_cost() {
        let dir = TempDir::new().expect("tempdir");
        let candidates: Vec<_> = (0..3)
            .map(|i| candidate(&dir, &format!("c{i}"), vec![failed_fatal("build")]))
            .collect();

        let err = SettlementRecord::new_with_cap(
            candidates,
            candidate_id("c0"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
            2,
        )
        .expect_err("fan-out wider than the cap must not construct");

        assert!(matches!(err, SettlementError::OverCap { count: 3, cap: 2 }));
        assert_eq!(DEFAULT_CANDIDATE_CAP, 8);
    }

    // --- Acceptance check 2: a fatal failure cannot be selected -------------

    /// The temptation the spec asks for: the most attractive diff in the panel,
    /// disqualified by one fatal check, passed as the selection anyway.
    #[test]
    fn test_new_beautiful_diff_with_failed_fatal_check_returns_ineligible_selection() {
        let dir = TempDir::new().expect("tempdir");
        let mut beautiful = candidate(&dir, "beautiful", vec![failed_fatal("build")]);
        beautiful.diff_summary =
            "removes 400 lines, collapses three modules into one, reads beautifully".into();
        let candidates = vec![beautiful, candidate(&dir, "plain", vec![passed("build")])];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("beautiful"),
            RATIONALE,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("an attractive diff does not outweigh a fatal check failure");

        match err {
            SettlementError::IneligibleSelection { id, check } => {
                assert_eq!(id, candidate_id("beautiful"));
                assert!(
                    check.contains("build"),
                    "the error must name the check that disqualified it: {check}"
                );
            }
            other => panic!("expected IneligibleSelection, got {other:?}"),
        }
    }

    /// Eligibility composes both predicates: a fresh check matrix does not
    /// rescue a candidate whose basis moved under it.
    #[test]
    fn test_new_stale_basis_candidate_selected_returns_ineligible_selection() {
        let dir = TempDir::new().expect("tempdir");
        let mut moved = candidate(&dir, "moved", vec![passed("build")]);
        moved.basis_verdict = stale();
        let candidates = vec![moved, candidate(&dir, "current", vec![passed("build")])];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("moved"),
            RATIONALE,
            Vec::new(),
            vec![Dissent {
                candidate: candidate_id("current"),
                diff_summary: "candidate current: 3 files changed".into(),
                note: None,
            }],
        )
        .expect_err("a candidate whose basis moved is ineligible before comparison");

        match err {
            SettlementError::IneligibleSelection { id, check } => {
                assert_eq!(id, candidate_id("moved"));
                assert!(
                    check.contains("basis stale") && check.contains("plan-input.md"),
                    "the error must name what moved: {check}"
                );
            }
            other => panic!("expected IneligibleSelection, got {other:?}"),
        }
    }

    #[test]
    fn test_eligible_composes_basis_freshness_and_fatal_check_outcome() {
        let dir = TempDir::new().expect("tempdir");

        let clean = candidate(&dir, "clean", vec![passed("build")]);
        assert!(clean.eligible(), "fresh basis, no fatal failure");
        assert!(clean.passed_all());
        assert_eq!(clean.ineligibility_reason(), None);

        let mut moved = candidate(&dir, "moved", vec![passed("build")]);
        moved.basis_verdict = stale();
        assert!(!moved.eligible(), "a stale basis alone is disqualifying");

        let broken = candidate(&dir, "broken", vec![failed_fatal("build")]);
        assert!(
            !broken.eligible(),
            "a failed fatal check alone disqualifies"
        );
        assert!(!broken.passed_all());
    }

    #[test]
    fn test_eligible_nonfatal_failure_does_not_disqualify_but_is_not_a_clean_sweep() {
        let dir = TempDir::new().expect("tempdir");
        let mut candidate = candidate(&dir, "a", Vec::new());
        candidate.checks = vec![
            CheckResult::new(check_id("build"), CheckOutcome::Passed, true),
            CheckResult::new(check_id("docs"), CheckOutcome::Failed, false),
        ];

        assert!(candidate.eligible(), "a non-fatal failure is weighable");
        assert!(
            !candidate.passed_all(),
            "but it is not a candidate that passed everything"
        );
    }

    #[test]
    fn test_passed_all_empty_matrix_is_not_a_clean_sweep() {
        let dir = TempDir::new().expect("tempdir");
        let candidate = candidate(&dir, "a", Vec::new());
        assert!(
            !candidate.passed_all(),
            "a candidate that ran no checks passed nothing"
        );
    }

    #[test]
    fn test_check_result_disqualifying_only_for_failed_fatal_checks() {
        assert!(failed_fatal("build").disqualifying());
        assert!(!passed("build").disqualifying());
        assert!(
            !CheckResult::new(check_id("build"), CheckOutcome::Failed, false).disqualifying(),
            "a non-fatal failure is weighable, not disqualifying"
        );
        assert!(
            !CheckResult::new(check_id("build"), CheckOutcome::Inconclusive, true).disqualifying(),
            "inconclusive is not failed"
        );
    }

    // --- Acceptance check 3: a graft outlives its archive -------------------

    #[test]
    fn test_new_graft_referencing_deleted_archive_returns_archive_missing() {
        let dir = TempDir::new().expect("tempdir");
        let loser = candidate(&dir, "a", vec![failed_fatal("build")]);
        let archive = loser.worktree_archive.clone();
        let candidates = vec![loser, candidate(&dir, "b", vec![passed("build")])];

        fs::remove_dir_all(&archive).expect("delete the loser's archive");

        let err = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            vec![Graft {
                from: candidate_id("a"),
                piece: "the bounded-read helper".into(),
            }],
            Vec::new(),
        )
        .expect_err("archives must outlive their grafts");

        match err {
            SettlementError::ArchiveMissing { candidate, path } => {
                assert_eq!(candidate, candidate_id("a"));
                assert_eq!(path, archive);
            }
            other => panic!("expected ArchiveMissing, got {other:?}"),
        }
    }

    #[test]
    fn test_new_graft_from_uncompared_candidate_returns_unknown_candidate() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![failed_fatal("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];

        let err = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            vec![Graft {
                from: candidate_id("ghost"),
                piece: "a piece from a run that never happened".into(),
            }],
            Vec::new(),
        )
        .expect_err("a graft must name a compared candidate");

        match err {
            SettlementError::UnknownCandidate { field, id } => {
                assert_eq!(field, "graft.from");
                assert_eq!(id, candidate_id("ghost"));
            }
            other => panic!("expected UnknownCandidate, got {other:?}"),
        }
    }

    // --- Acceptance check 4: out of scope, and deliberately so --------------
    //
    // "Fan-out over a WorkItem whose capability set includes an Irreversible
    // class fails at planning time, not at settlement." There is no test here
    // because there is nothing here to test: by the time a SettlementRecord
    // exists, the N candidates have already run. The check belongs to the
    // planner that authorizes the fan-out, and this module deliberately carries
    // no effect-class field for it — see the module doc comment.

    // --- Report ------------------------------------------------------------

    #[test]
    fn test_matrix_report_names_every_candidate_check_graft_and_dissent() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build"), passed("clippy")]),
            candidate(&dir, "b", vec![passed("build"), passed("clippy")]),
            candidate(&dir, "c", vec![failed_fatal("build"), passed("clippy")]),
        ];

        let record = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            vec![Graft {
                from: candidate_id("a"),
                piece: "the bounded-read helper".into(),
            }],
            vec![Dissent {
                candidate: candidate_id("a"),
                diff_summary: "candidate a: 3 files changed".into(),
                note: None,
            }],
        )
        .expect("construct");

        let report = record.matrix_report();
        for expected in [
            "selected `b`",
            "candidate `a` [eligible]",
            "candidate `b` [selected]",
            "check `clippy`: passed",
            "check `build`: failed (fatal)",
            "ineligible: fatal check `build` failed",
            "graft: `the bounded-read helper` from candidate `a`",
            "dissent: candidate `a`",
        ] {
            assert!(
                report.contains(expected),
                "report must contain {expected:?}:\n{report}"
            );
        }
    }

    // --- Identifiers -------------------------------------------------------

    #[test]
    fn test_candidate_id_try_new_rejects_whitespace_control_and_oversize_values() {
        assert!(CandidateId::try_new("candidate-a").is_ok());
        assert_eq!(candidate_id("candidate-a").as_str(), "candidate-a");
        assert_eq!(candidate_id("candidate-a").to_string(), "candidate-a");

        for bad in ["", "has space", "tab\there", "new\nline"] {
            assert!(
                CandidateId::try_new(bad).is_err(),
                "must reject {bad:?} as a candidate id"
            );
        }
        assert!(
            CandidateId::try_new("x".repeat(257)).is_err(),
            "over 256 bytes"
        );
        assert!(
            CheckId::try_new("x".repeat(256)).is_ok(),
            "256 bytes is the bound"
        );
    }

    #[test]
    fn test_candidate_id_deserialize_revalidates_whitespace_bearing_values() {
        let recovered: CandidateId = serde_json::from_str("\"candidate-a\"").expect("deserialize");
        assert_eq!(recovered, candidate_id("candidate-a"));

        let err = serde_json::from_str::<CandidateId>("\"has space\"")
            .expect_err("deserialization must re-validate, not trust the wire");
        assert!(err.to_string().contains("candidate_id"));

        let err = serde_json::from_str::<CheckId>("\"\"")
            .expect_err("an empty check id must not deserialize");
        assert!(err.to_string().contains("check_id"));
    }

    // --- Serde -------------------------------------------------------------

    #[test]
    fn test_settlement_record_round_trips_through_json() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];
        let record = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            vec![Graft {
                from: candidate_id("a"),
                piece: "the bounded-read helper".into(),
            }],
            vec![Dissent {
                candidate: candidate_id("a"),
                diff_summary: "candidate a: 3 files changed".into(),
                note: Some("kept the lock".into()),
            }],
        )
        .expect("construct");

        let json = serde_json::to_string(&record).expect("serialize");
        let recovered: SettlementRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(record, recovered);
    }

    #[test]
    fn test_settlement_record_deserialize_rejects_a_record_new_would_reject() {
        let dir = TempDir::new().expect("tempdir");
        let candidates = vec![
            candidate(&dir, "a", vec![passed("build")]),
            candidate(&dir, "b", vec![passed("build")]),
        ];
        let record = SettlementRecord::new(
            candidates,
            candidate_id("b"),
            RATIONALE,
            Vec::new(),
            vec![Dissent {
                candidate: candidate_id("a"),
                diff_summary: "candidate a: 3 files changed".into(),
                note: None,
            }],
        )
        .expect("construct");

        let mut wire: serde_json::Value =
            serde_json::to_value(&record).expect("serialize to value");
        wire["dissent"] = serde_json::Value::Array(Vec::new());

        let err = serde_json::from_value::<SettlementRecord>(wire)
            .expect_err("an invalid record must not slip in through the wire");
        assert!(
            err.to_string().contains("absent from dissent"),
            "deserialization must fail with the constructor's own reason: {err}"
        );
    }

    #[test]
    fn test_check_outcome_round_trips_through_json_as_snake_case() {
        for (outcome, expected) in [
            (CheckOutcome::Passed, "\"passed\""),
            (CheckOutcome::Failed, "\"failed\""),
            (CheckOutcome::Inconclusive, "\"inconclusive\""),
        ] {
            let json = serde_json::to_string(&outcome).expect("serialize");
            assert_eq!(json, expected);
            let recovered: CheckOutcome = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(recovered, outcome);
            assert_eq!(outcome.to_string(), expected.trim_matches('"'));
        }
    }

    #[test]
    fn test_check_result_round_trips_through_json_with_and_without_evidence() {
        let bare = passed("build");
        let recovered: CheckResult =
            serde_json::from_str(&serde_json::to_string(&bare).expect("serialize"))
                .expect("deserialize");
        assert_eq!(bare, recovered);

        let detailed = failed_fatal("build").with_evidence("cargo build: 2 errors");
        let recovered: CheckResult =
            serde_json::from_str(&serde_json::to_string(&detailed).expect("serialize"))
                .expect("deserialize");
        assert_eq!(detailed, recovered);
        assert_eq!(recovered.evidence.as_deref(), Some("cargo build: 2 errors"));
    }

    // --- Error Display -----------------------------------------------------

    #[test]
    fn test_settlement_error_display_explains_the_consequence_for_every_variant() {
        let invalid_id = SettlementError::InvalidId {
            kind: "candidate_id",
            value: "has space".into(),
        }
        .to_string();
        assert!(invalid_id.contains("candidate_id") && invalid_id.contains("has space"));

        assert!(SettlementError::NoCandidates
            .to_string()
            .contains("nothing to compare"));

        let single = SettlementError::SingleCandidate {
            id: candidate_id("a"),
        }
        .to_string();
        assert!(single.contains("N=1") && single.contains("`a`"));

        let duplicate = SettlementError::DuplicateCandidate {
            id: candidate_id("a"),
        }
        .to_string();
        assert!(duplicate.contains("one result per candidate"));

        let unknown = SettlementError::UnknownCandidate {
            field: "graft.from",
            id: candidate_id("ghost"),
        }
        .to_string();
        assert!(unknown.contains("graft.from") && unknown.contains("ghost"));

        let ineligible = SettlementError::IneligibleSelection {
            id: candidate_id("a"),
            check: "fatal check `build` failed".into(),
        }
        .to_string();
        assert!(ineligible.contains("build") && ineligible.contains("disqualifying"));

        let missing = SettlementError::MissingCheck {
            candidate: candidate_id("a"),
            check: check_id("clippy"),
        }
        .to_string();
        assert!(missing.contains("clippy") && missing.contains("excused"));

        let short = SettlementError::RationaleTooShort {
            length: 4,
            minimum: MIN_RATIONALE_BYTES,
        }
        .to_string();
        assert!(short.contains("bare score") && short.contains("40"));

        let archive = SettlementError::ArchiveMissing {
            candidate: candidate_id("a"),
            path: PathBuf::from("/tmp/gone"),
        }
        .to_string();
        assert!(archive.contains("/tmp/gone") && archive.contains("outlive"));

        let dissent = SettlementError::PassingCandidateNotRecorded {
            id: candidate_id("a"),
        }
        .to_string();
        assert!(dissent.contains("road not taken"));

        let over_cap = SettlementError::OverCap { count: 12, cap: 8 }.to_string();
        assert!(over_cap.contains("12") && over_cap.contains("8"));
        assert!(over_cap.contains("N times the tokens"));
    }
}
