---
title: Stack Consolidation Contract + Next-Steps Frameworks
description: Deeper review resolving the three open forks in the ION/IMPULSE/ROSA stack, plus reusable decision frameworks and a dependency-ordered path to a single capability+seam contract
version: '0.1'
updated: 2026-06-30
type: research
category: architecture
phase: phase3
status: draft
audience: builders
tags: [multi-agent, capability-negotiation, trigger-taxonomy, seam-transport, consolidation, ion-harness, mcp]
---

# Stack Consolidation Contract + Next-Steps Frameworks

> **Status: draft / exploratory.** Not canon. Successor to the three-stream research pass
> (codebase inventory + external industry + internal prior-art) on the ION ⊂ IMPULSE ⊂ ROSA
> stack. Cross-anchored with [2026-06-30-multi-agent-provenance-divergence.md](2026-06-30-multi-agent-provenance-divergence.md),
> the ion-harness spec-a/b/c contracts, [META-HARNESS-RUST-MULTI-AGENT.md](META-HARNESS-RUST-MULTI-AGENT.md),
> and the protocol-triangle note.

## 0. The finding that frames everything

The same primitives were invented **three times independently** — in the ion-harness specs, the
provenance/divergence spec, and the Impulse capability registry. Industry validates the direction
on 3 of 4 design questions. **The work is consolidation, not invention.** The visible canary of
divergent re-derivation: `HarnessRequest`/`HarnessResponse` already names *two different contracts*
(ion's verification-gate vs Impulse's policy IPC) and will collide when ION and IMPULSE share a
workspace.

This note resolves the three open forks, then gives four frameworks: the unified contract (A),
the trigger taxonomy made concrete (B), the reusable decision rules (C), and the
dependency-ordered path (D).

---

## 1. Deeper review — resolving the three forks

### Fork 1 — Additive advertised capability vs. deny-by-omission security

**Tension.** Capability *negotiation* implies an agent advertises what it supports (additive at
handshake). But ion spec-c's security property is that a tool **absent from the enum cannot be
constructed, so it cannot be prompt-coerced**. Additive advertisement reintroduces exactly that
coercion surface.

**Resolution — closed universe, open subset-selection, intersect at one gate.**
Capability *classes* are a **closed, code-defined universe** (an enum). An agent advertises a
*subset* it supports; the effective set = `advertised ∩ policy-allowed`. Advertisement can only
**narrow or activate** from the known universe — it can never *add* a class the host doesn't
already know and permit. This is exactly MCP's own model: capability keys come from a *known* set,
and SEP-1724 extensions are "independently governed" (registered), not arbitrary.

- **New capability *class* → a governance event** (code/registry change, deliberate, reviewed).
- **New capability *instance* (a new tool of a known class) → runtime**, via a `list_changed`-style
  notification. This is the 2-tier model: classes negotiated at handshake, instances discovered
  during operation.

**Cost.** You cannot hot-add a brand-new *kind* of ability without touching the universe enum.
That is the correct price: a new ability class is a security-relevant decision, not a runtime event.

**Invariant preserved.** Deny-by-omission holds at the *class* level; negotiation operates strictly
within the closed universe.

### Fork 2 — Open runtime-discovered surface vs. closed-enum fail-closed discipline

**Tension.** A composition-aware surface that is "queryable and mutable at runtime" (open) is in
direct tension with ROSA's closed `SurfaceKind` enum — no default arm, unknown kinds fall through
to `ErrorRecoveryCard` (fail-closed).

**Resolution — seam-tiered openness; and note they are *different surfaces*.**
1. **Discoverability increases outward; fail-closed strictness increases inward.** The inner seam
   (ION↔IMPULSE, control plane) is a **closed enum, fail-closed** — control-plane truth must be
   exhaustively handled. The outer seam (ROSA↔external tools) is **open/discoverable** — ROSA must
   consume heterogeneous, evolving tools.
2. **The apparent contradiction dissolves because ROSA has two surfaces, not one.** The closed
   `SurfaceKind` enum is the *rendering* surface (UI cards — stays closed for safety). The
   *tool-consumption* surface (what ROSA may call via IMPULSE/MCP) is the open one. They are
   governed by opposite disciplines because they carry opposite risk.

**Cost.** Two disciplines and one explicit boundary marker. The boundary is the
control-plane / render-plane line from META-HARNESS: if it changes control-plane truth, it is
inner+closed; if it renders/measures/packages, it is outer+open.

### Fork 3 — Control-plane-typed vs. MCP-everywhere

**Tension.** "MCP as the universal seam" vs. the META-HARNESS rule that control-plane truth (policy,
trust, gates, daemon state) must stay in typed Rust.

**Resolution — already converged by four independent sources: contract is authority, transport is
an adapter.** Define the contract *once* as a transport-agnostic typed schema (ion spec-a's
`HarnessRequest`/`Response` + JSON Schema). Derive adapters:
- **Typed Rust in-process / Unix-socket** for ION↔IMPULSE (control plane) — MCP's ~3.6× latency and
  JSON-RPC envelope buy nothing co-located, and you keep Rust's typed DX.
- **Real MCP server** at the ROSA↔IMPULSE edge — where a foreign model needs runtime *discovery*,
  token economy, and heterogeneity.

The slogan: **MCP *semantics* everywhere, MCP *transport* at the edge only.** Use static
generation (`mcp-to-ai-sdk` style) for stable production surfaces, dynamic discovery for open/dev
surfaces.

**Cost.** A contract→adapter codegen/lockstep step. The `assert_shared_request_compatible` test
already does this by hand; formalize it.

**Invariant preserved.** Control-plane truth never traverses a tool-shaped seam.

---

## 2. Framework A — The Unified Capability + Seam Contract

One source of truth the three derivations collapse into. Six components:

1. **Capability universe** — a closed enum of capability *classes* (code-defined; new class = governance event).
2. **Agent capability advertisement** — extend `AgentDescriptor.capabilities` from today's two
   behavioral knobs (`uses_xml_context`, `startup_delay_ms`) to *also* carry an advertised subset of
   the universe. Effective = advertised ∩ policy-allowed.
3. **Unified action taxonomy** — collapse the four fragmented surfaces (`DaemonRequest` 40-variant
   superset, `WorkbenchDaemonRequest` 21-subset, `SupervisorAction` 10 mutating, desktop MCP 8 tools)
   into one action catalog. Each action is tagged with: `{capability_class, trigger_class,
   mutation_class, provenance_required}`.
4. **One trigger/enforcement grammar** — a single decision schema for every gated action:
   `{ decision: allow | deny | ask | defer, reason, updatedInput?, compensator? }`.
5. **Transport adapters derived from the contract** — typed-socket (inner) and MCP (edge), both
   generated/lockstep-tested against the one schema.
6. **Provenance binding** — every action result carries a registry-slug `agent_id`; the coarse
   `Platform`+`session_id` provenance is resolved to a canonical slug at the seam (the
   provenance-bridge prerequisite).

---

## 3. Framework B — Trigger taxonomy, made concrete

**Heterogeneous intake, uniform enforcement.** Five intake classes normalize into one internal
`Trigger` enum feeding one dispatch+gate path.

| Intake class | MCP analog | Existing IMPULSE mechanism | Enforcement |
|---|---|---|---|
| Explicit call | `tools/call` | CLI direct-dispatch; IPC `DaemonRequest` | gate grammar |
| Event / hook | Channels / `claude/channel` | platform hooks (auto-generated `hooks.json`); `PublishTerminalOps` push | gate grammar |
| Scheduled / periodic | Tasks + scheduler | **none today** (no `tokio interval`, no watcher) | gate grammar |
| Agent / LLM-routed | Sampling-with-tools | `Orchestrate`, supervisor chat | gate grammar |
| Approval-gated (modifier) | Elicitation | `SupervisorPermissionPolicy.require_confirmation_actions`; desktop review queue | gate grammar |

**Two rules make this hold:**
- **Approval is a modifier, not a class** — it composes onto any of the other four (resolves the
  A-calls-B composition hazard). Implement as the ion spec-b `tool_call` interceptor → one
  authoritative allowlist ("one gate, two harnesses, zero drift"), consolidating today's three
  scattered gates (supervisor policy + guardrails + desktop review queue).
- **Three patterns to steal that you do not have yet:** `updatedInput` (gate *rewrites* a mutation to
  satisfy the approval-invariant rather than reject), `defer` (gate falls through to default policy —
  never a mandatory chokepoint), and saga **compensator-at-call-time** (formalizes warden-after
  rollback; arXiv 2503.11951 *SagaLLM*).

---

## 4. Framework C — Reusable decision rules

Promote the fork resolutions to standing rules so they are not re-derived:

- **R1 (capability):** Capability *classes* are a closed universe. Agents advertise a *subset*;
  effective = advertised ∩ policy-allowed. New class = governance event; new instance = runtime
  `list_changed`. **Advertisement never adds; it only narrows or activates.**
- **R2 (surface openness):** Discoverability ∝ distance from control-plane truth; fail-closed
  strictness ∝ proximity to it. Render-kinds closed; tool-set open. The boundary is the
  control-plane / render-plane line.
- **R3 (seam):** The typed contract is the authority; transport is a derived adapter. MCP at the
  open edge, typed Rust for the control-plane inner seam. **Never route control-plane truth over MCP.**

---

## 5. Framework D — Dependency-ordered path (no time estimates)

Expressed as dependencies, not durations. Critical path bolded.

- **P0 — hygiene (parallel, blocks nothing but prevents pain):** resolve the
  `HarnessRequest`/`HarnessResponse` naming collision between the ion verification-gate contract and
  the Impulse policy IPC.
- **Prereq A — the unified contract schema (Framework A).** *Blocks everything programmatic.* Turn
  this note into the actual versioned schema + capability-universe enum.
- **Prereq B — extend `AgentDescriptor.capabilities`** to advertised-subset + the universe enum.
  *Blocks capability negotiation.* Depends on A.
- **Prereq C — slug-resolved provenance at the seam** (`Platform`+`session_id` → canonical registry
  slug). *Blocks ROSA consuming IMPULSE without violating the provenance invariant.* This is the
  "Impulse provenance-bridge prerequisite" already named in the stack-architecture decision.
  Depends on A.
- **Step 1 — unify the gate** (ion spec-b interceptor) consolidating supervisor policy + guardrails
  + desktop review queue into one allowlist + the Framework-B grammar. Depends on A.
- **Step 2 — trigger intake layer** (scheduler / watcher / event-bus). *Genuinely greenfield.*
  Depends on Step 1.
- **Step 3 — MCP edge server** exposing the *unified* surface (today only a tooling subset is
  exposed). Depends on A + Step 1.
- **Step 4 — ROSA-first consuming surface** lands on top. Depends on Prereq C + Step 3.

**Critical path:** `A → (B ⊕ C) → Step 1 → Step 3 → Step 4`. P0 and Step 2 run off the critical path.

---

## 6. Hazard register (true regardless of design choices)

1. **Provenance must survive the seam.** An unattributed result is malformed (provenance spec §1.1),
   but MCP results are not natively slug-tagged and Impulse provenance is coarse today. Prereq C is
   mandatory, not optional.
2. **No persisted merged capability view.** If a composition-aware surface caches a "merged
   capability view," it must be a transient read-time federation, never a persisted merge — the
   empty-`consensus()` rule applied to capabilities.
3. **Four-enum drift.** Until the action taxonomy is unified (Framework A.3), the
   `DaemonRequest` / `WorkbenchDaemonRequest` / `SupervisorAction` / desktop-MCP surfaces drift; the
   lockstep test covers only the first two.
4. **Naming collision.** `HarnessRequest`/`HarnessResponse` (P0).

---

## 7. Open decisions for the user

These three are the *only* genuinely undecided points; everything else is convergent:
1. Confirm **R1** (closed universe + advertise-subset) over a fully-additive capability model.
2. Confirm the **inner/outer seam boundary** (R2) sits on the control-plane / render-plane line.
3. Confirm **R3** (typed inner, MCP edge) — and whether to formalize contract→adapter codegen now or
   keep the manual lockstep test.

---

## 8. Integrating the MCP / capability-taxonomy / execution-topology perspectives

A second research input (MCP deployment patterns, micro-agents, agent-OS capability categories,
concurrency patterns, software fundamentals) was reconciled against this note. Most converges; the
additive pieces are folded in below.

### 8.1 Capability descriptor — two new tags

Framework A.3's action tag `{capability_class, trigger_class, mutation_class, provenance_required}`
is refined to: **`{capability_class, kind, execution_tier, trigger_class, provenance_required}`**.

- **`kind ∈ {sensor, actuator, brain}`** (replaces the thinner `mutation_class`):
  - *sensor* = read-only → ungated, callable liberally.
  - *actuator* = mutates live state → approval-gated by default (wire-gate makes this mandatory).
  - *brain* = processing/reasoning → cost/latency-tiered; gated only if it writes derived knowledge.
- **`execution_tier ∈ {code, micro-agent, agent}`** — determines cost, latency, determinism, and
  **test strategy**: code → unit tests; micro-agent → *evals* (nondeterministic); agent → expensive
  full reasoning. **A micro-agent that mutates is still an actuator** — small size does not exempt it
  from the gate.

### 8.2 Knowledge ecosystem vs. active workers → IMPULSE memory

The "build high-quality knowledge separately from task workers" pattern maps directly onto the stack:
- The knowledge wiki = **IMPULSE's memory layer**.
- The enrichment step (raw → structured on every ingest) = an **event-triggered `brain` capability**
  that **must stamp registry-slug provenance** on what it writes (`do-not-unify`).
- "Worker asks a targeted question, gets a concise summary, never a raw dump" = the
  **consolidated-view read model** ([2026-06-30-multi-agent-provenance-divergence.md](2026-06-30-multi-agent-provenance-divergence.md)).
  This operationalizes the "context is expensive" rule: workers *query*, they do not *carry*.

### 8.3 Framework E — Execution-topology pattern-picker

Orthogonal to the static contract: *how to fan a task out*. Pick by 7 dimensions (scale,
per-item intelligence, homogeneity, latency, cost/context, reusability, governance).

| Pattern | Best when | Stack mapping |
|---|---|---|
| Backend batch | high volume, low per-item intelligence | a single `sensor` over a batch endpoint |
| Single agent + parallel tool calls | medium volume, homogeneous | ROSA voice + local stdio (latency) |
| Single agent + code execution | custom loop/cache logic, token-sensitive | a `brain` at `execution_tier=code` |
| Orchestrator + dedicated specialist | recurring, valuable domain | IMPULSE `fleet.dispatch` to a reusable sub-agent |
| Flat swarm | per-item analysis genuinely diverges | fleet fan-out, supervisor aggregates |
| Hybrid / adaptive | most production paths | escalate simple→specialist as needs grow |

Default to the simplest that fits; escalate to orchestrator+specialist only when the domain is
recurring and valuable enough to deserve its own manager.

### 8.4 R3 refinement (spec correction)

- Edge transport is **Streamable HTTP** (single `/mcp` POST + optional SSE), *not* the deprecated
  separate HTTP+SSE.
- "MCP everywhere on top of APIs" is correct **at the outer edge only**; the inner control-plane seam
  stays typed Rust (MCP's ~3.6× latency is negligible only across machines). Net: **MCP semantics
  everywhere, MCP transport at the edge.**
- For this Rust-native local-first stack, Python FastMCP is acceptable only for throwaway prototypes;
  the shipping path is `rmcp`/typed-inner (`real-systems-only`).

### 8.5 Reusable cross-project spine (the generalization ask)

A one-page project foundation that slots *beneath* the canonical CLAUDE.md rules and biases every
project toward the scalable path. Drop into each project's `CONTEXT.md` / `ARCHITECTURE.md`:

1. **Four layers, never mixed:** Data · Logic(Execution) · Trigger · Presentation.
2. **Capability kinds:** every capability is a *sensor / actuator / brain* at a *code / micro-agent /
   agent* tier — and is tagged, gated, and tested accordingly.
3. **Six fundamentals:** separation of concerns · boring stable core · small single-purpose units ·
   interfaces before implementation · context-is-expensive · composition over monoliths.
4. **Decision rules:** R1 (closed capability universe) · R2 (open outward / fail-closed inward) ·
   R3 (typed core, MCP edge — on top of, never instead of, a typed contract).
