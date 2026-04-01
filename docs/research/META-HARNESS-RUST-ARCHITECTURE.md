---
status: active
phase: all
audience: builder
tags: [research, meta-harness, multi-agent, rust, architecture]
last_updated: 2026-03-31
---

# Meta-Harness and Multi-Agent Rust Architecture

> **Version:** 1.0 | **Status:** Research Synthesis | **Updated:** 2026-03-31
> **Purpose:** Translate recent Meta-Harness and multi-agent research into concrete, Rust-first guidance for Impulse.
> **Contract boundary:** This document is analysis and design guidance. It does not change the active product contract in `docs/spec/RUST-CANONICAL-CONTRACT.md`.

---

## Executive Summary

Impulse already has most of the substrate that a Meta-Harness style system needs: durable traces, decision memory, retrieval, injection artifacts, daemon IPC, and explicit guardrails. What it does not yet have is the outer loop that treats routing policy, injection policy, and evaluation traces as first-class harness artifacts that can be compared, scored, and evolved.

The external source material sharpens four conclusions for this repo:

1. **Harness engineering is program logic, not prompt copy.** The real optimization surface is retrieval policy, state flow, routing, guardrails, and context budgeting.
2. **Raw traces matter more than summaries.** If we want harness iteration to be trustworthy, we need navigable artifacts that preserve code, scores, and execution traces together.
3. **Multi-agent systems only pay off when work is genuinely decomposable.** Impulse should stay skeptical of open-ended autonomy and favor orchestrated specialist topologies with bounded authority.
4. **Rust is a strong fit for the control plane.** Typed IPC, explicit concurrency, atomic persistence, capability checks, and daemon-owned state all map well to the repo's current architecture.

The practical result is not "turn Impulse into a swarm framework." It is: keep Impulse as the sidecar and operator plane, then add a Rust-native harness registry, evaluation runner, routing policy surface, and explicit team-memory/trust artifacts as optional higher-order layers.

---

## 1. What the Source Material Changes

### 1.1 The Harness Is the System Around the Model

The core Meta-Harness lesson is that performance shifts often come from the code around the model rather than the model itself. For Impulse, that means the optimization surface is already present in:

- `src/injection/` for retrieval and context-shaping policy
- `src/retrieval/` for search quality and trace accessibility
- `src/orchestration/` for cross-tool or cross-agent routing
- `src/guardrail/` for domain locking and action control
- daemon and GUI/TUI crates for operator visibility and supervision

This repo should therefore document harnesses as **policy-bearing Rust modules plus data artifacts**, not as prompt snippets alone.

### 1.2 Raw Traces Beat Summaries for Diagnosis

The strongest finding across the Meta-Harness material is that score-only or summary-only feedback hides the causal path that produced a result. For Impulse, the implication is straightforward:

- `HISTORY.jsonl` should stay append-only and inspectable
- injection artifacts should remain trace-linked, not summary-only
- future harness evaluation should store source snapshot, score, and trace together under one run identity

If a proposer or operator cannot answer "which policy produced this trace and why did it score this way?", the archive is too lossy.

### 1.3 Multi-Agent Systems Need Bounded Roles

The multi-agent material converges on one useful correction: multi-agent coordination helps when tasks can be split into parallel or specialized work, and hurts when the work is inherently sequential or underspecified.

For Impulse, that means:

- prefer **orchestrated** topologies over open-ended peer meshes
- keep each worker's domain narrow and inspectable
- treat handoffs as explicit artifacts, not implicit context bleed
- do not market open-ended autonomy that the runtime cannot supervise

This matches the repo's existing bias toward advisory coordination, review-before-apply flows, and capability checks.

### 1.4 Team Memory and Trust Need Explicit Surfaces

The strongest non-Meta-Harness ideas in the source set are useful only when made concrete:

- **team memory** should be a file or typed store, not a vague claim
- **trust state** should be a typed runtime datum with thresholds and audit events
- **topology** should be an inspectable config artifact, not hidden in prompt text

Impulse already trends this way. The next step is to document and persist these concepts as named structures, not prose-only abstractions.

---

## 2. Rust-First Architecture for Impulse

### 2.1 Existing Rust Surfaces That Already Map to a Harness

| Repo Surface | Current Role | Meta-Harness Interpretation |
|--------------|--------------|-----------------------------|
| `.impulse/HISTORY.jsonl` | durable execution history | raw trace archive |
| `.impulse/GENOME.md` | decision memory | evolving mental model |
| `retrieval.db` | indexed lookup | proposer/operator read surface |
| `src/injection/engine.rs` | context assembly policy | candidate harness logic |
| `src/orchestration/mod.rs` | routing suggestion | router policy surface |
| `src/guardrail/` | capability enforcement | domain-locking layer |
| daemon IPC + workbench | runtime supervision | operator evaluation plane |

The main takeaway is that Impulse does not need a new language or a new stack to become more "meta-harness aware." It needs clearer harness identity and better artifact linkage.

### 2.2 Durable vs Ephemeral State

The repo should keep a clear split between durable artifacts and ephemeral runtime overlays:

| State Type | Form | Ownership |
|------------|------|-----------|
| durable traces | `HISTORY.jsonl` | append-only file |
| durable decisions | `GENOME.md` | curated file |
| durable harness records | future registry file or DB table | evaluation layer |
| durable team memory | future `team-mental-model.md` or typed JSON | coordination layer |
| durable routing policy | future config artifact | orchestration layer |
| ephemeral telemetry | daemon memory | runtime overlay |
| ephemeral trust transitions | daemon memory with audit emission | control plane |

This matters because the outer loop should evaluate durable artifacts, while operators supervise ephemeral runtime state.

### 2.3 Recommended Rust Concurrency Primitives

Rust should stay opinionated here. The source material points toward more automation, but the repo should implement that automation with explicit primitives:

| Need | Preferred Primitive | Why |
|------|---------------------|-----|
| queued evaluation jobs | `tokio::sync::mpsc` | bounded backpressure |
| latest snapshot fan-out | `tokio::sync::watch` | cheap "current truth" distribution |
| event broadcast | `tokio::sync::broadcast` | multi-subscriber runtime signals |
| shared registry state | `RwLock` around typed structs | simple read-heavy access |
| evaluation parallelism cap | `Semaphore` | prevents runaway task explosion |
| artifact persistence | atomic temp-file + rename | preserves repo safety rules |

The system does not need an exotic actor framework before it has a stable evaluation loop.

### 2.4 Typed Boundaries to Prefer

If Impulse expands its multi-agent and harness surfaces, the contract shape should stay Rust-native:

- `serde`-backed message enums for IPC and artifact payloads
- typed run IDs linking policy snapshot, trace, and score
- explicit capability enums for mutating actions
- versioned config structs rather than free-form prompt blobs
- `thiserror` enums at boundaries instead of stringly-typed failures

The implementation burden is lower when each new control-plane concept is introduced first as a Rust type.

---

## 3. What Is Missing Today

### 3.1 Harness Snapshot Registry

Impulse needs a registry that records:

- run ID
- policy snapshot or config delta
- score or evaluation outcome
- trace location
- creation time
- comparison metadata such as dominated/not dominated

Without this, the repo has traces but not harness history in the strong sense used by Meta-Harness.

### 3.2 Evaluation Runner

The repo needs a small, hard evaluation set and a repeatable runner that scores candidate policies without changing the product contract.

That runner should:

1. load a candidate policy
2. execute a bounded evaluation set
3. write score, trace, and artifact outputs
4. record the run in a registry
5. expose results to operators and future proposers

### 3.3 Externalized Routing Policy

`src/orchestration/mod.rs` still behaves like static code, not an evolvable routing surface. A typed routing artifact would make the policy:

- inspectable
- diffable
- testable
- eligible for harness-style iteration

### 3.4 Explicit Team Memory and Trust Artifacts

If the repo documents multi-agent teams seriously, it should eventually grow named artifacts for:

- team-wide mental model
- per-role constraints
- trust-state transitions
- handoff receipts

Until those exist, docs should present them as planned patterns, not implemented features.

---

## 4. Recommended Multi-Agent Topology for This Repo

### 4.1 Default Topology

Start with a narrow orchestrated topology:

| Role | Responsibility | Write Scope |
|------|----------------|------------|
| Orchestrator | task decomposition, routing, final synthesis | none |
| Planner or Researcher | read-heavy analysis, artifact proposals | none |
| Rust Implementer | crate/module changes | scoped repo paths |
| Verifier | tests, benchmarks, regression checks | none |
| Steward or Guardrail layer | approvals, budget enforcement, risky-action checks | policy/state artifacts only |

This is much easier to debug than peer-to-peer delegation storms.

### 4.2 Decision Rule: When to Use More Than One Agent

Use multi-agent coordination only when all of the following are true:

1. work can be decomposed into parallel or specialized steps
2. each role has a bounded input/output contract
3. the handoff artifact is inspectable
4. the coordination overhead is smaller than the likely review savings

If those conditions do not hold, stay single-agent.

### 4.3 What Not to Build

Do not add:

- open-ended autonomous workers with repo-wide write scope
- hidden topology encoded only in prompts
- summary-only memory handoffs
- trust systems that are prose-only and not emitted as state or audit events

Impulse's value is supervised leverage, not theatrical autonomy.

---

## 5. Practical Rust Recommendations

### 5.1 Document Harnesses as Code + Data

Every serious harness change should be explainable in terms of:

- Rust code path
- config surface
- runtime trace
- evaluation result

That is the minimum bar for reproducibility.

### 5.2 Keep the Sidecar Identity

Impulse should remain the sidecar/control plane even if it grows harness-evaluation features. The repo should not blur into "yet another agent framework" without a deliberate product decision.

### 5.3 Prefer Configurable Policies Over Prompt Mutation

When a behavior can be expressed as validated Rust config plus typed runtime logic, do that instead of embedding policy in long prompt prose. Prompts still matter, but they should not be the only place a system rule exists.

### 5.4 Treat Trace Access as a First-Class Capability

Searchability is part of the product. If a future proposer or operator cannot grep, query, or diff a run cleanly, the harness infrastructure is incomplete.

---

## 6. Documentation Implications

The repo should carry this topic in three layers:

1. **Research layer** - documents like this one explain the architecture and tradeoffs.
2. **Guide layer** - Rust programming guidance turns the ideas into implementation patterns.
3. **Contract layer** - only after code lands should `RUST-CANONICAL-CONTRACT.md` absorb new runtime guarantees.

That separation keeps the docs honest.

---

## 7. Recommended Near-Term Roadmap

### Phase A: Documentation Baseline

- add a Rust-first multi-agent programming guide
- add this research synthesis to the research index
- expose both docs in `docs/INDEX.md`

### Phase B: Harness Identity

- externalize routing policy
- define a typed harness record
- link injection artifacts to run IDs

### Phase C: Evaluation Loop

- add a bounded evaluation runner
- record score plus trace plus policy snapshot
- expose results through daemon or CLI inspection paths

### Phase D: Team Memory and Trust

- add explicit team-memory artifact(s)
- add typed trust-state tracking
- emit audit events when trust state changes

### Phase E: Operator Experience

- show harness runs, comparisons, and stale evaluations in workbench surfaces
- keep operator review in the loop for risky mutations

---

## Key Findings

1. **Impulse is already structurally close to a Meta-Harness substrate.** The missing piece is the evaluation loop and harness registry, not a new platform.
2. **Rust is the right place for the control plane.** The repo's strongest future surfaces are typed IPC, daemon-owned overlays, guardrails, and trace-linked artifacts.
3. **Multi-agent work should stay orchestrated and bounded.** The repo should resist open-ended agent meshes.
4. **Team memory, trust, and topology should be explicit artifacts.** If they matter, they should be inspectable and versioned.
5. **Research docs should stay ahead of the contract, not overwrite it.** This topic belongs in research and guides until the code actually ships.

---

_Created: 2026-03-31 | Status: Active research synthesis_
