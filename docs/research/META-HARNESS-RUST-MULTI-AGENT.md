---
status: active
phase: all
audience: builder
tags: [research, meta-harness, multi-agent, rust, topology]
last_updated: 2026-03-31
---

# Meta-Harness, Rust, and Multi-Agent Coordination

> **Version:** 1.0 | **Status:** Research Synthesis | **Updated:** 2026-03-31
> **Purpose:** Consolidate the recent meta-harness and multi-agent research into a repo-specific frame for Impulse, with emphasis on Rust implementation leverage.

---

## Executive Summary

Impulse already contains most of the ingredients of a meta-harness system:

- durable traces
- curated project memory
- indexed retrieval
- runtime guardrails
- daemon-owned state
- operator-facing UI surfaces

What is missing is the explicit framing that these pieces form an **optimization surface**, not just a utility layer. The synthesis from the external research is:

1. **Harness logic is code worth optimizing.** Retrieval, injection, routing, memory formatting, and budget policy can change outcomes more than swapping models.
2. **Multi-agent topology is a second optimization surface.** Role boundaries, delegation patterns, permissions, and trust state should be designed and eventually measured independently of prompt text.
3. **Rust is a strong substrate for the control plane.** Typed errors, atomic writes, serde-validated config, and daemon IPC make Impulse well suited to trace-driven optimization without requiring a separate orchestration platform.

Impulse should therefore document multi-agent coordination as more than shared file awareness, and document harness policy as more than prompt injection.

---

## The Three Layers That Matter

| Layer | What Changes | Impulse Surface | Why It Deserves Its Own Doc |
|------|---------------|-----------------|------------------------------|
| **Harness layer** | Retrieval, injection, routing, memory update policy, budget rules | `retrieval/`, `injection/`, `stewardship/`, `orchestration/` | This is the code the system will eventually tune or compare |
| **Topology layer** | Which agents exist, what they may do, and how they hand off | `LIVE_STATE.json`, delegation/conflict tracking, future policy docs | Coordination mistakes are often topology mistakes, not prompt mistakes |
| **Operational memory and trust layer** | Durable lessons, audit records, trust state, stale/active overlays | `GENOME.md`, `HISTORY.jsonl`, daemon telemetry, guardrails | This keeps improvements inspectable and survivable across sessions |

Collapsing all three layers into one system prompt produces ambiguity. Keeping them separate makes the system auditable and eventually optimizable.

---

## Key Findings

1. **Raw traces beat summaries for diagnosis.** The repo should continue treating `HISTORY.jsonl`, typed artifacts, and structured daemon output as primary evidence. Summaries help navigation but should not become the optimization substrate.

2. **Harness policy and multi-agent topology are not the same thing.** Retrieval and context budgeting answer "what enters the model." Team structure and delegation answer "who is allowed to do what."

3. **The default multi-agent shape should stay orchestrated.** A central supervisor or router delegating to bounded specialists is easier to reason about than a free-form peer mesh. This matches the repo's current advisory coordination reality.

4. **Domain locking only becomes real when backed by runtime boundaries.** Markdown prompts can describe a boundary, but `guardrail/`, capability checks, and tool restrictions are the actual enforcement surfaces.

5. **`GENOME.md` should evolve from memory file to optimization memory.** It already stores durable project knowledge. The next step is to capture short, structured notes about which policy changes improved or harmed outcomes.

6. **Trust state should live outside model prose.** If the system ever needs quarantine, degraded mode, or approval thresholds, those belong in runtime state and policy, not only in prompts.

7. **Audit trails should be action-centric.** Tool calls, files touched, policy snapshots, structured results, and typed errors are the trustworthy record. Reasoning text is not.

8. **Rust is a comparative advantage here.** Atomic writes, typed errors, daemon/read-model separation, and serde validation make Impulse a better host for trace-driven harness evolution than a looser scripting stack.

---

## Mapping The Research To Impulse

| Research Concept | Current Repo Analog | Current Gap | Recommended Direction |
|------------------|--------------------|-------------|------------------------|
| Filesystem-mediated harness search | `.impulse/`, `HISTORY.jsonl`, `retrieval.db`, staged artifacts | No explicit harness registry or scored outer loop | Document the harness surface as a coherent subsystem |
| Team topology as a first-class artifact | advisory coordination, conflict/delegation tracking | No active topology doc in the repo's current surface | Keep a synthesis doc at the research layer until implementation evidence exists |
| Persistent mental models | `GENOME.md` | Not yet described as policy-learning memory | Add structured write patterns and examples |
| Runtime domain locking | `guardrail/`, capability checks, AGENTS rules | Under-documented as part of coordination safety | Treat guardrails as the real boundary in docs |
| Trust evolution | external dynamic-trust work in shared memory | Not part of this repo's active contract | Document as future operational layer, not current product fact |
| Trace-driven evaluation | tests, retrieval, injection artifacts | No joined score-policy-trace surface | Keep Rust guidance concrete and small-scope |

---

## Multi-Agent Documentation Rules For This Repo

### Keep The Product Claim Honest

- File-awareness and conflict/delegation tracking are implemented.
- Structural coordination enforcement is not yet the general product story.
- Future topology work should be labeled as proposed until backed by runtime behavior and tests.

### Separate Coordination From Memory

- `LIVE_STATE.json` and related runtime views describe who is active now.
- `GENOME.md` and `HISTORY.jsonl` describe what the project has learned over time.
- Future harness registries would describe which policy variants performed well or poorly.

### Prefer Bounded Specialists

The repo should document future agent roles in terms of:

- supervisor/router
- implementation worker
- verification/review worker

That pattern aligns with the existing control-plane direction better than open-ended swarms.

---

## Rust-Specific Implications

The most important Rust insight from this research is that **implementation discipline is part of the optimization surface**:

- atomic writes protect trace integrity
- typed errors improve diagnostics
- serde-validated config narrows unsafe mutation space
- daemon/state separation keeps UI truth inspectable
- testable structs and explicit IPC reduce ambiguity

That means the next useful document is not another generic architecture memo. It is a Rust patterns guide for how to evolve Impulse safely.

See: [`../guides/RUST-MULTI-AGENT-PATTERNS.md`](../guides/RUST-MULTI-AGENT-PATTERNS.md)

---

## When Sidecars Are The Right Boundary

The external stack material is useful here, but only if the boundary stays explicit.

**Rust should stay authoritative for:**

- daemon-owned state
- policy evaluation
- trust and guardrail decisions
- artifact and trace persistence
- typed IPC contracts

**A sidecar is the right choice for:**

- rich rendering catalogs
- browser-native measurement or layout work
- spreadsheet/document interaction surfaces
- UI-specific export or worker pipelines

The rule is simple: if the feature changes control-plane truth, Rust should own it. If the feature exists to render, measure, or package that truth for a UI surface, a sidecar can be the better boundary.

---

## What This Repo Should Not Claim Yet

- It should not claim open-ended swarm autonomy.
- It should not present prompt-only domain locking as real enforcement.
- It should not treat summaries as the authoritative audit surface.
- It should not promote proposed topology ideas into the canonical contract before runtime evidence exists.

---

## Recommended Next Steps

1. Keep this file as the high-level synthesis for harness plus topology thinking.
2. Use the Rust guide for concrete implementation constraints and module-level patterns.
3. When a run-scoped harness registry exists, promote it from research note into contract documentation.
4. Update roadmap/spec docs only after the runtime behavior exists and passes verification.

---

## Related Docs

- [`AGENT-HARNESS-ANALYSIS.md`](./AGENT-HARNESS-ANALYSIS.md)
- [`RESEARCH-DIGEST.md`](./RESEARCH-DIGEST.md)
- [`TERMINAL-LAYER-ANALYSIS.md`](./TERMINAL-LAYER-ANALYSIS.md)
- [`../guides/RUST-MULTI-AGENT-PATTERNS.md`](../guides/RUST-MULTI-AGENT-PATTERNS.md)

---

_Created: 2026-03-31 | Status: Active research synthesis_
