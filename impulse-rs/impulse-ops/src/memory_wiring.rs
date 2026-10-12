//! Wire contract for scoped memory promotion and dismissal (ADR-0020).
//!
//! This module is deliberately request-types-only. It exists so the daemon lane
//! can add a `DecideMemoryCandidate` endpoint, and the Dioxus lane an operator
//! Promote/Dismiss control, without editing the state or candidate-contract
//! files this decision's lane owns.
//!
//! The authorization shape follows ADR-0018 exactly: the input carries **no**
//! authentication field. A client cannot assert how it was authenticated; the
//! daemon stamps [`OperatorAuthentication`] onto the persisted decision from
//! the connection class the request arrived on, the same way it does for
//! `OperatorDecisionInput` / `OperatorDecision`.

use serde::{Deserialize, Serialize};

use crate::governed_task::{GovernedActor, GovernedRequestId, OperatorAuthentication};
use crate::memory_candidate::{
    AcceptedRunMemoryCandidate, MemoryCandidateId, MemoryRecord, MemoryRecordId,
};

/// Longest accepted dismissal reason on the wire. Mirrors the ledger-side
/// bound so an over-long reason is refused before it reaches state.
pub const MAX_DISMISSAL_REASON_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryWiringContractError {
    #[error("invalid memory candidate decision field `{field}`: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
}

/// What an operator decided about one pending candidate.
///
/// `Dismiss` carries the reason inline rather than as a sibling `Option<String>`
/// so the type system, not a runtime check, makes "dismissed without a reason"
/// unrepresentable. A dismissal leaves no durable record behind, so its reason
/// is the only surviving account of why an accepted run was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum MemoryCandidateDecisionKind {
    Promote,
    Dismiss { reason: String },
}

impl MemoryCandidateDecisionKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Promote => "promote",
            Self::Dismiss { .. } => "dismiss",
        }
    }

    pub fn is_promote(&self) -> bool {
        matches!(self, Self::Promote)
    }

    pub fn validate(&self) -> Result<(), MemoryWiringContractError> {
        match self {
            Self::Promote => Ok(()),
            Self::Dismiss { reason } => {
                if reason.trim().is_empty()
                    || reason.contains('\0')
                    || reason.len() > MAX_DISMISSAL_REASON_BYTES
                {
                    return Err(MemoryWiringContractError::InvalidField {
                        field: "decision.reason",
                        message: format!(
                            "a dismissal reason must be nonblank, NUL-free, and at most {MAX_DISMISSAL_REASON_BYTES} bytes"
                        ),
                    });
                }
                Ok(())
            }
        }
    }
}

/// Client-composed request to decide one candidate.
///
/// `deny_unknown_fields` plus the deliberate absence of an authentication field
/// means a payload that tries to assert its own provenance is refused at the
/// boundary rather than silently ignored (ADR-0018's rule, restated for this
/// mutation family).
///
/// `expected_ledger_revision` is a compare-and-swap against the candidate
/// ledger's revision, and `request_id` keys the idempotency receipt — the same
/// discipline governed-task mutations use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCandidateDecisionInput {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub candidate_id: MemoryCandidateId,
    pub decision: MemoryCandidateDecisionKind,
    pub actor: GovernedActor,
    pub expected_ledger_revision: u64,
}

impl MemoryCandidateDecisionInput {
    pub fn validate(&self) -> Result<(), MemoryWiringContractError> {
        if self.project_id.trim().is_empty() {
            return Err(MemoryWiringContractError::InvalidField {
                field: "project_id",
                message: "must be nonblank".to_string(),
            });
        }
        if self.actor.id.trim().is_empty() {
            return Err(MemoryWiringContractError::InvalidField {
                field: "actor.id",
                message: "must be nonblank".to_string(),
            });
        }
        self.decision.validate()
    }
}

/// The persisted decision record.
///
/// `authentication` is serde-defaulted to [`OperatorAuthentication::Declared`],
/// so a decision written by a direct-CLI or in-process caller (which has no
/// connection class at all) loads as declared rather than failing, exactly like
/// `OperatorDecision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCandidateDecision {
    pub request_id: GovernedRequestId,
    pub candidate_id: MemoryCandidateId,
    pub decision: MemoryCandidateDecisionKind,
    pub actor: GovernedActor,
    /// Daemon-stamped connection provenance. Never read from client payload.
    #[serde(default)]
    pub authentication: OperatorAuthentication,
    pub decided_at: String,
    pub based_on_ledger_revision: u64,
    pub resulting_ledger_revision: u64,
    /// Present only for a promotion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<MemoryRecordId>,
}

/// Everything a caller needs after a decision lands.
///
/// `replayed` distinguishes a fresh decision from an idempotent replay so an
/// operator surface can say "already decided" instead of implying it just
/// happened. `retrieval_index_dirty` reports whether this decision left the
/// retrieval index needing a reindex — true only for a promotion, because a
/// pending or dismissed candidate is never indexed (ADR-0013 rule 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCandidateDecisionOutcome {
    pub decision: MemoryCandidateDecision,
    pub candidate: AcceptedRunMemoryCandidate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    pub replayed: bool,
    pub projection_digest: String,
    pub retrieval_index_dirty: bool,
}

/// Daemon request. The handler belongs to the daemon lane; this type exists so
/// that lane never has to edit a file this one owns.
///
/// Authorization contract for whoever wires it: this mutation is operator-class
/// only. It must be gated the same way `RecordOperatorDecision` is — refused on
/// a connection that has not presented this daemon run's operator capability —
/// and the resulting [`OperatorAuthentication`] stamped from the connection,
/// never from the payload. A refusal must leave the candidate ledger, the
/// memory log, and the projection byte-identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideMemoryCandidateRequest {
    pub decision: MemoryCandidateDecisionInput,
}

/// Daemon acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideMemoryCandidateAck {
    pub outcome: MemoryCandidateDecisionOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governed_task::GovernedActorKind;

    fn actor() -> GovernedActor {
        GovernedActor {
            kind: GovernedActorKind::Operator,
            id: "operator-a".to_string(),
        }
    }

    fn input(decision: MemoryCandidateDecisionKind) -> MemoryCandidateDecisionInput {
        MemoryCandidateDecisionInput {
            request_id: GovernedRequestId::try_new("request-a").unwrap(),
            project_id: "project-a".to_string(),
            candidate_id: MemoryCandidateId::try_new(format!(
                "memory-candidate-{}",
                "a".repeat(64)
            ))
            .unwrap(),
            decision,
            actor: actor(),
            expected_ledger_revision: 3,
        }
    }

    #[test]
    fn test_decision_input_round_trips_through_serde() {
        for decision in [
            MemoryCandidateDecisionKind::Promote,
            MemoryCandidateDecisionKind::Dismiss {
                reason: "duplicates an existing record".to_string(),
            },
        ] {
            let original = input(decision);
            let json = serde_json::to_string(&original).unwrap();
            let decoded: MemoryCandidateDecisionInput = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, original);
        }
    }

    #[test]
    fn test_decision_input_rejects_a_client_asserted_authentication_field() {
        let json = serde_json::to_value(input(MemoryCandidateDecisionKind::Promote)).unwrap();
        let mut object = json.as_object().unwrap().clone();
        assert!(
            !object.contains_key("authentication"),
            "the input contract must not carry an authentication field"
        );
        object.insert(
            "authentication".to_string(),
            serde_json::json!("capability_authenticated"),
        );
        let error = serde_json::from_value::<MemoryCandidateDecisionInput>(
            serde_json::Value::Object(object),
        )
        .unwrap_err();
        assert!(format!("{error}").contains("authentication"));
    }

    #[test]
    fn test_decision_validate_blank_dismissal_reason_returns_error() {
        for reason in ["", "   ", "\n\t "] {
            let error = input(MemoryCandidateDecisionKind::Dismiss {
                reason: reason.to_string(),
            })
            .validate()
            .unwrap_err();
            assert!(format!("{error}").contains("decision.reason"));
        }
    }

    #[test]
    fn test_decision_validate_oversized_dismissal_reason_returns_error() {
        let error = input(MemoryCandidateDecisionKind::Dismiss {
            reason: "x".repeat(MAX_DISMISSAL_REASON_BYTES + 1),
        })
        .validate()
        .unwrap_err();
        assert!(format!("{error}").contains("decision.reason"));
    }

    #[test]
    fn test_decision_validate_blank_project_or_actor_returns_error() {
        let mut blank_project = input(MemoryCandidateDecisionKind::Promote);
        blank_project.project_id = "  ".to_string();
        assert!(format!("{}", blank_project.validate().unwrap_err()).contains("project_id"));

        let mut blank_actor = input(MemoryCandidateDecisionKind::Promote);
        blank_actor.actor.id = "  ".to_string();
        assert!(format!("{}", blank_actor.validate().unwrap_err()).contains("actor.id"));
    }

    #[test]
    fn test_decision_kind_labels_are_stable() {
        assert_eq!(MemoryCandidateDecisionKind::Promote.label(), "promote");
        assert!(MemoryCandidateDecisionKind::Promote.is_promote());
        let dismiss = MemoryCandidateDecisionKind::Dismiss {
            reason: "stale".to_string(),
        };
        assert_eq!(dismiss.label(), "dismiss");
        assert!(!dismiss.is_promote());
    }

    #[test]
    fn test_persisted_decision_without_authentication_loads_as_declared() {
        let json = serde_json::json!({
            "request_id": "request-a",
            "candidate_id": format!("memory-candidate-{}", "a".repeat(64)),
            "decision": { "kind": "promote" },
            "actor": { "kind": "operator", "id": "operator-a" },
            "decided_at": "2026-09-12T10:00:00Z",
            "based_on_ledger_revision": 3,
            "resulting_ledger_revision": 4
        });
        let decoded: MemoryCandidateDecision = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.authentication, OperatorAuthentication::Declared);
        assert!(decoded.record_id.is_none());

        let round_trip: MemoryCandidateDecision =
            serde_json::from_str(&serde_json::to_string(&decoded).unwrap()).unwrap();
        assert_eq!(round_trip, decoded);
    }

    #[test]
    fn test_request_and_ack_round_trip() {
        let request = DecideMemoryCandidateRequest {
            decision: input(MemoryCandidateDecisionKind::Promote),
        };
        let decoded: DecideMemoryCandidateRequest =
            serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
        assert_eq!(decoded, request);
    }
}
