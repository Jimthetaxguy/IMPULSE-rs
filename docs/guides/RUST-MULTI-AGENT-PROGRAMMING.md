---
status: active
phase: all
audience: builder
tags: [guide, rust, multi-agent, orchestration]
last_updated: 2026-03-31
---

# Rust Multi-Agent Programming Guide

> **Version:** 1.0 | **Status:** Practical Guide | **Updated:** 2026-03-31
> **Purpose:** Turn multi-agent architecture ideas into concrete Rust programming patterns that fit Impulse.
> **Scope:** This guide covers implementation patterns. It does not claim that every pattern here is already implemented in the repo.

---

## 1. Start With the Simplest Topology

Default to a single agent unless the task has a real decomposition boundary.

Use multiple agents only when:

- the work can run in parallel or split cleanly by specialty
- each role has a narrow output contract
- handoffs can be represented as files, typed messages, or explicit artifacts
- the operator can inspect progress and failures without reading prompt internals

The safest default topology for Rust systems is:

| Role | Purpose | Mutates State? |
|------|---------|----------------|
| Orchestrator | decomposes task and chooses next worker | no |
| Implementer | changes code or configuration | yes, scoped |
| Verifier | runs tests, profiling, or audits | no |
| Steward | enforces budget, approvals, or policy | no direct product writes |

This keeps write authority narrow and reviewable.

---

## 2. Use Explicit Message Contracts

Do not coordinate agents with ad hoc strings if the workflow matters.

Prefer typed message envelopes:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnvelope {
    pub correlation_id: String,
    pub sender: AgentRole,
    pub recipient: AgentRole,
    pub body: AgentMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRole {
    Orchestrator,
    Implementer,
    Verifier,
    Steward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentMessage {
    WorkItem { task: String, scope: Vec<String> },
    Handoff { artifact_path: String, summary: String },
    VerificationResult { passed: bool, details: String },
    Escalation { reason: String },
}
```

Why this matters:

- `serde` makes IPC and persistence straightforward
- enums prevent silent protocol drift
- explicit roles make audit logs meaningful

---

## 3. Choose Tokio Primitives by Coordination Shape

| Need | Primitive | Pattern |
|------|-----------|---------|
| bounded job queue | `mpsc` | orchestrator -> worker tasks |
| current snapshot propagation | `watch` | latest daemon or team state |
| fan-out notifications | `broadcast` | status updates, invalidations |
| shared mutable registry | `RwLock<T>` | read-heavy state with occasional writes |
| parallelism cap | `Semaphore` | evaluation runs, subprocess limits |

Rules:

- use `mpsc` for ownership transfer, not shared mutation
- use `watch` when only the latest value matters
- cap concurrency explicitly; do not spawn unbounded worker trees
- prefer one typed state owner over many peer writers

---

## 4. Keep State Versioned and Conflict-Aware

Multi-agent systems fail quietly when two writers think they own the same truth.

Prefer versioned shared state:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versioned<T> {
    pub version: u64,
    pub value: T,
}

impl<T> Versioned<T> {
    pub fn next(self, value: T) -> Self {
        Self { version: self.version + 1, value }
    }
}
```

Good uses in this repo:

- routing policy revisions
- harness run metadata
- trust-state transitions
- operator-approved action artifacts

If a write depends on a prior read, pass the expected version and reject stale updates.

---

## 5. Distinguish Durable and Ephemeral State

Keep files for durable truth and daemon memory for live overlays.

Good durable candidates:

- session history
- harness run records
- routing policy
- team-memory artifact

Good ephemeral candidates:

- live terminal telemetry
- heartbeat freshness
- in-flight evaluation jobs
- temporary trust warnings

This split keeps restart behavior predictable and prevents the daemon from becoming a hidden database.

---

## 6. Persist Artifacts Atomically

If a handoff or harness record matters, write it atomically.

For this repo, keep using:

1. unique temp file
2. full write and fsync where needed
3. atomic rename into place

Multi-agent systems amplify corruption risk because more processes may race on the same artifact family. Atomic writes are not optional.

---

## 7. Treat Guardrails as Runtime Types, Not Prompt Advice

Prompt instructions are not enough for mutating actions.

Prefer typed capability checks:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    ReadRepo,
    WriteScopedFiles,
    RunVerification,
    ApproveRiskyAction,
}
```

Then enforce:

- role -> capability mapping
- path scope checks
- action preconditions
- audit emission on denied or escalated actions

This aligns with the existing guardrail direction in Impulse.

---

## 8. Build the Harness Loop Around Files, Scores, and Traces

If you want Meta-Harness style iteration in Rust, the minimum loop is:

1. serialize the current policy or config snapshot
2. run a bounded evaluation set
3. record score plus traces plus snapshot ID
4. compare against prior runs
5. promote only after review or threshold checks

That implies three durable concepts:

- `HarnessRecord`
- `EvaluationTrace`
- `PolicySnapshot`

Without those, you only have logs, not harness optimization.

---

## 9. Recommended Crate Surface

For the patterns in this guide, prefer crates already aligned with the repo:

| Concern | Crate |
|---------|-------|
| async runtime | `tokio` |
| serialization | `serde`, `serde_json` |
| IDs and timestamps | `uuid`, `chrono` |
| error handling | `thiserror`, `anyhow` |
| logging and audit | `tracing` |
| persistence | `rusqlite` or existing repo storage layer |
| IPC | existing daemon protocol + Unix sockets |

Only add heavier service crates such as `axum` when the system actually needs a service boundary.

---

## 10. Impulse-Specific Next Steps

### Good next implementation moves

- externalize routing policy into a typed config artifact
- add a harness run registry linked to traces
- define a team-memory artifact format
- make trust-state transitions explicit in daemon-owned state
- expose run comparison and stale-run visibility in operator surfaces

### Bad next implementation moves

- open-ended worker meshes
- unbounded recursive delegation
- summary-only handoffs
- new dependencies for coordination before the message/state model is stable

---

## Key Findings

1. **Rust multi-agent systems are easiest to trust when every boundary is typed.**
2. **Tokio primitives are enough for the first serious coordination layer.**
3. **Versioned state beats implicit last-writer-wins behavior.**
4. **Atomic persistence matters more as agent count rises.**
5. **Meta-Harness style optimization requires a run registry, not just better prompts.**

---

_Created: 2026-03-31 | Status: Active practical guide_
