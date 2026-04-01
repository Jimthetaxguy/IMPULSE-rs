---
status: active
phase: all
audience: builder
tags: [guide, rust, multi-agent, harness, implementation]
last_updated: 2026-03-31
---

# Rust Multi-Agent and Meta-Harness Patterns

> **Version:** 1.0 | **Status:** Implementation Guide | **Updated:** 2026-03-31
> **Purpose:** Capture the Rust programming patterns that best fit Impulse if it grows toward trace-driven harness optimization and richer multi-agent coordination.

---

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

5. **Use serde as part of the safety boundary.** Policy mutations that do not deserialize cleanly should never progress deeper into evaluation.

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
- safe to mutate via `serde_json`
- consistent with the repo's existing config direction

---

## Pattern 2: Run-Scoped Correlation IDs

If a policy can be evaluated, it needs a stable `run_id` that joins:

- policy snapshot
- evaluation score
- trace file
- resulting artifact
- durable note in `GENOME.md`

Without that, the system can search traces but cannot learn from them precisely.

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

The goal is deterministic, inspectable evaluation, not maximum task fan-out.

```rust
use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinSet};

async fn run_candidates(candidates: Vec<String>) {
    let gate = Arc::new(Semaphore::new(4));
    let mut tasks = JoinSet::new();

    for candidate in candidates {
        let gate = gate.clone();
        tasks.spawn(async move {
            let _permit = gate.acquire_owned().await.expect("semaphore closed");
            evaluate_candidate(candidate).await
        });
    }

    while let Some(result) = tasks.join_next().await {
        // Record each result immediately; don't wait for the full batch.
        let _ = result;
    }
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

1. **Externalize routing policy** into serializable config.
2. **Attach `run_id` to injection artifacts** and downstream trace outputs.
3. **Introduce a small harness registry** using the current SQLite patterns.
4. **Add a focused evaluation harness** with deterministic fixtures and `#[tokio::test]`.
5. **Write structured GENOME entries** for meaningful wins and regressions.
6. **Only then** consider proposer-driven policy mutation.

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
cargo fmt --check
cargo check --all-features
cargo test
cargo clippy --all-features --all-targets -- -D warnings
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
