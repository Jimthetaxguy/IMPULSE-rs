---
status: active
phase: all
audience: builder
tags: [guide, rust, multi-agent, orchestration]
last_updated: 2026-09-14
---

# Rust Multi-Agent Programming Guide

> **Version:** 1.1 | **Status:** Practical Guide | **Updated:** 2026-09-14
> **Purpose:** Turn multi-agent architecture ideas into concrete Rust programming patterns that fit Impulse.
> **Scope:** This guide covers implementation patterns. It does not claim that every pattern here is already implemented in the repo.

---

## Current implementation boundary

Reviewed on 2026-09-14 against the [current contract map](RUST-MULTI-AGENT-PATTERNS.md#current-impulse-contract)
and the source files it links. The role, envelope, and capability types below are illustrative;
production changes must extend `impulse_ops::governed_task`, the daemon protocol, and existing
producer wiring. A role name never substitutes for a connection's operator authentication.
General team-memory and harness-registry sketches are design options, not shipped interfaces.

The active path separates registration, claim, verification, Supervisor review, and operator
acceptance. Durable producer reservations cover verification/review/promotion side effects and
their receipts; cancellation and crash recovery still need explicit evidence. Dioxus is a consumer
of that authority, not a second owner of task state.

## 1. Start With the Simplest Topology

Default to a single agent unless the task has a real decomposition boundary.

Use multiple agents only when:

- the work can run in parallel or split cleanly by specialty
- each role has a narrow output contract
- handoffs can be represented as files, typed messages, or explicit artifacts
- the operator can inspect progress and failures without reading prompt internals

A useful decomposition for Rust systems is:

| Role | Purpose | Expected write boundary |
|------|---------|-------------------------|
| Orchestrator | decomposes task and chooses next worker | scoped planning and handoff records |
| Implementer | changes code or configuration | approved checkout paths |
| Verifier | runs tests, profiling, or audits | owned test worktrees, caches, and evidence; producer receipts through the daemon |
| Steward | reviews budget, approvals, or policy | audit records and explicitly authorized policy operations |

Verification can execute code and write artifacts. Give it an isolated, declared scope; the role
name itself grants no product-write or operator authority.

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

These are storage categories, not a claim that every proposed artifact exists. Current governed
truth lives in `.impulse/GOVERNED_TASKS.json`; recovery also uses durable producer reservations.
The memory boundary keeps `.impulse/MEMORY_CANDIDATES.json`, `MEMORY.jsonl`,
`GENOME_PROJECTION.md`, and hand-curated `GENOME.md` distinct.
[ADR-0020](../decisions/0020-scoped-memory-promotion-and-dismissal.md) implements state and wire
contracts while deferring daemon endpoint, Dioxus Promote/Dismiss controls, and Ion integration.
Do not confuse this with the wired staged-worktree Promote/Discard operation in
[ADR-0019](../decisions/0019-builder-staged-worktree-world-scope.md).

This split keeps restart and recovery behavior explicit; in-flight work is not safely ephemeral
merely because its process can be restarted.

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

The sample enum is not an authorization implementation. The shipped socket boundary uses peer
credentials plus a per-daemon-run capability, and the daemon checks operator class before protected
mutations. Same-UID deliberate token discovery is an explicit limit; the role field in an incoming
message is insufficient proof. Reuse the existing tool capability registry for tool execution.

---

## 8. Build the Harness Loop Around Files, Scores, and Traces

If you want Meta-Harness style iteration in Rust, the minimum loop is:

1. serialize the current policy or config snapshot
2. run a bounded evaluation set
3. record score plus traces plus snapshot ID
4. compare against prior runs
5. retain scores as evidence; promote only through an explicitly authorized, gated path

That implies three durable concepts:

- `HarnessRecord`
- `EvaluationTrace`
- `PolicySnapshot`

These are conceptual records, not required new Rust types. Map them onto existing governed-task and artifact identities first. A score or model review is evidence for an operator decision, not permission to apply a change.

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

- extend existing governed records with scoped, versioned evidence rather than a second run registry
- test stale-revision refusals, request replay, and durable producer recovery at real dispatch boundaries
- keep operator authorization separate from worker-supplied actor and role fields
- preserve memory candidates and source evidence; implement remaining ADR-0020 surfaces only with explicit acceptance gates
- expose verified state and recovery needs in operator surfaces without treating a model verdict as acceptance

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
