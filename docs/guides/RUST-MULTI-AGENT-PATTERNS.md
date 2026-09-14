---
status: active
phase: all
audience: builder
tags: [guide, rust, multi-agent, harness, implementation]
last_updated: 2026-09-14
---

# Rust Multi-Agent and Meta-Harness Patterns

> **Version:** 1.1 | **Status:** Implementation Guide | **Updated:** 2026-09-14
> **Purpose:** Capture the Rust programming patterns that best fit Impulse if it grows toward trace-driven harness optimization and richer multi-agent coordination.

---

## Current Impulse contract

Reviewed against the current Rust sources on 2026-09-14. The snippets below illustrate patterns;
they do not define new daemon messages, role authority, or a second harness registry.

- Extend the governed-task records and revision/request receipts in
  [`governed_task`](../../impulse-rs/impulse-ops/src/governed_task.rs). Registration, claim,
  verification, Supervisor review, and operator decision are distinct transitions; model output
  or process exit does not accept a task.
- Socket operator authority comes from peer credentials plus the per-daemon-run capability in
  [`actor_provenance.rs`](../../impulse-rs/src/daemon/actor_provenance.rs), not a role field supplied
  by a client. A same-UID process that deliberately discovers the token remains inside the stated
  threat-model limit.
- [`governed_wiring.rs`](../../impulse-rs/src/daemon/governed_wiring.rs) wraps verification,
  Supervisor review, and promotion side effects and their recorded mutations in durable producer
  reservations. A panic/crash can leave an open reservation requiring reconciliation; this is not
  a promise of transactional rollback of arbitrary external effects.
- Reuse the existing [`daemon protocol`](../../impulse-rs/src/daemon/protocol.rs) and
  [`impulse-ops`](../../impulse-rs/impulse-ops/src/lib.rs) contracts. Dioxus consumes daemon truth;
  the CLI and runtime adapters must not create a parallel policy authority.
- Keep memory candidates, promoted records, their derived projection, and hand-curated GENOME
  separate. [ADR-0020](../decisions/0020-scoped-memory-promotion-and-dismissal.md) provides state
  and wire contracts but explicitly defers daemon endpoint, Dioxus controls, and Ion integration.
  [ADR-0019](../decisions/0019-builder-staged-worktree-world-scope.md) staged-worktree Promote/Discard
  is a separate, wired operation.

## Why This Guide Exists

The external research points in one direction: the code around the model is a real optimization target. For Impulse, that means any future harness or multi-agent work should be built in a way that is:

- serializable
- diffable
- testable
- traceable
- rejectable at the boundary

This guide keeps that requirement concrete for the existing Rust codebase.

---

## Non-Negotiable Design Constraints

1. **Reuse the current persistence stack.** Prefer `.impulse/`, `retrieval.db`, and existing artifact flows over introducing a second database or service just for harness experiments.

2. **Keep writes atomic.** Snapshot files, trace companions, and policy artifacts should follow temp-file-plus-rename discipline.

3. **Keep errors typed.** The system should emit structured failures the same way it emits structured state.

4. **Preserve the direct/daemon split.** Short hook paths stay cheap; long-lived authoritative state belongs in the daemon.

5. **Deserialize, then validate and authorize.** Serde checks representation; it does not grant mutation authority. Reject invalid invariants, stale revisions, and missing permissions before evaluation or effects.

---

## Pattern 1: Policy As Data

Hard-coded routing and policy logic is difficult to diff and impossible to evolve safely. Prefer serializable structs:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoutingRule {
    pub tool: String,
    pub keywords: Vec<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoutingPolicy {
    pub version: u32,
    pub rules: Vec<RoutingRule>,
    pub default_tool: String,
}
```

Benefits:

- human-readable
- testable with round-trip serde
- serializable via `serde_json`; applying a change still requires validation and authority
- consistent with the repo's existing config direction

---

## Pattern 2: Run-Scoped Correlation IDs

If a policy can be evaluated, it needs a stable `run_id` that joins:

- policy snapshot
- evaluation score
- trace file
- resulting artifact
- governed-task evidence and any derived review candidate, with source identity preserved

Correlation supports inspection and comparison; it does not authorize automatic GENOME writes. Hand-curated `GENOME.md` retains its existing operator-controlled writer, separate from candidate and promoted-record artifacts.

---

## Pattern 3: Versioned Shared State

If multiple agents, evaluators, or daemon publishers can update the same logical value, use explicit versions instead of assuming "last write wins" is acceptable.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Versioned<T> {
    pub value: T,
    pub version: u64,
}

impl<T> Versioned<T> {
    pub fn next(value: T, version: u64) -> Self {
        Self {
            value,
            version: version + 1,
        }
    }
}
```

Recommended rule:

- read value and version together
- validate the expected version on write
- reject or retry on mismatch

This keeps coordination bugs visible instead of silently overwriting state.

---

## Pattern 4: Typed Errors As Diagnostics

Avoid generic error text when recording harness failures. Prefer typed enums that can survive serialization:

```rust
#[derive(thiserror::Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum PolicyEvalError {
    #[error("candidate exceeded budget: {candidate_tokens} > {budget_tokens}")]
    BudgetExceeded {
        candidate_tokens: usize,
        budget_tokens: usize,
    },
    #[error("policy field validation failed: {field}")]
    InvalidField {
        field: String,
    },
}
```

This is not cosmetic. Typed failures are easier to aggregate, index, and reason about than string fragments.

---

## Pattern 5: Bounded Concurrency

If candidate policies are ever evaluated in parallel, use bounded concurrency rather than free spawning:

- `tokio::task::JoinSet`
- `tokio::sync::Semaphore`
- explicit evaluation budgets

The window bounds spawned-but-uncollected tasks as well as active evaluators. Completion order
is nondeterministic. On an evaluator or join error, this example returns the error and dropping
the JoinSet requests cancellation of remaining tasks; evaluators must define their own cleanup
and must not assume cancellation rolls back external effects.

```rust
use anyhow::Result;
use tokio::task::JoinSet;

// evaluate_candidate returns Result<()>; task and evaluator failures must reach the caller.
async fn run_candidates(candidates: Vec<String>) -> Result<()> {
    const MAX_IN_FLIGHT: usize = 4;
    let mut tasks = JoinSet::new();

    for candidate in candidates {
        if tasks.len() >= MAX_IN_FLIGHT {
            if let Some(result) = tasks.join_next().await {
                result??;
            }
        }
        tasks.spawn(async move { evaluate_candidate(candidate).await });
    }

    while let Some(result) = tasks.join_next().await {
        result??;
    }
    Ok(())
}
```

Prefer:

- `mpsc` for work queues
- `watch` for latest-state propagation
- `broadcast` only when every subscriber genuinely needs every event

---

## Pattern 6: Snapshot Plus Overlay State

The repo already distinguishes durable state from live telemetry. Preserve that:

- **durable:** sessions, genome, retrieval index, artifact registry, scored runs
- **ephemeral:** evaluator progress, terminal telemetry, in-flight recommendations

This prevents UI layers from quietly becoming the only source of truth.

---

## Pattern 7: Typed IPC Contracts

The system proposing a policy change should not also be the final authority on whether that change "worked."

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HarnessRequest {
    PublishCandidate { run_id: String, policy_path: String },
    EvaluateCandidate { run_id: String },
    GetEvaluation { run_id: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HarnessResponse {
    Accepted { run_id: String },
    EvaluationRecorded { run_id: String, score: f32 },
    Rejected { run_id: String, reason: String },
}
```

Typed request/response enums make daemon logs, tests, and future migrations much safer than ad hoc JSON blobs.

---

## Pattern 8: Keep The Evaluator Outside The Proposer

In Rust terms:

- proposer suggests a policy snapshot
- validator checks structure and safety
- evaluator runs the candidate
- registry records score and trace reference

This split aligns with existing guardrail and daemon design instincts in the repo.

---

## Module Mapping For Future Work

| Module | Future Role | Guidance |
|-------|-------------|----------|
| `src/injection/` | policy execution surface | keep result structs rich and serializable |
| `src/retrieval/` | proposer read surface | add run correlation before expanding search layers |
| `src/stewardship/` | budget controller | make policy accept/reject outcomes explicit |
| `src/orchestration/` | routing layer | externalize routing before trying to optimize it |
| `src/guardrail/` | runtime boundary | keep unsafe action checks outside the proposer path |
| `src/daemon/` | authoritative state publisher | aggregate evaluator state here, not in the UI |
| `src/semantic_diff/` | run-to-run diff engine | reuse it for policy comparisons before inventing new diffing |

---

## Recommended Build Sequence

1. **Map the change onto existing governed-task and artifact records** before proposing new storage.
2. **Carry task, request, revision, and basis identity** through the existing producer path.
3. **Keep verification and Supervisor review separate from operator acceptance**, including failure and replay paths.
4. **Exercise bounded scheduling, stale revisions, producer failures, and recovery** in focused tests.
5. **Preserve raw evidence and review candidates**; accepting a run must not silently rewrite GENOME.
6. **Add policy changes or memory promotion wiring only under their own scoped contracts and gates.**

This sequence keeps the codebase honest: first make behavior observable, then make it evolvable.

---

## What Not To Build

- Do not add a second ORM or second persistence stack just for harness search.
- Do not model open-ended agent swarms before the orchestrated path is explicit and testable.
- Do not use visible reasoning text as the primary audit format.
- Do not bypass serde or typed errors for "faster iteration."
- Do not add new crates where the current repo patterns already solve the problem.

---

## Testing Expectations

Every new policy type or harness record should have:

- serde round-trip coverage
- boundary validation tests
- error display tests
- integration coverage for successful recording and safe rejection

Verification should still use the repo's existing Rust gate:

```bash
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

---

## Practical Rule

If a future harness or coordination feature cannot be:

- serialized,
- diffed,
- traced,
- scored,
- and rejected safely,

it is not ready to join the optimization surface.

---

## Related Docs

- [`../research/META-HARNESS-RUST-MULTI-AGENT.md`](../research/META-HARNESS-RUST-MULTI-AGENT.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
