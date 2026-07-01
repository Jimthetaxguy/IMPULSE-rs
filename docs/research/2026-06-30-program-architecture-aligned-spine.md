---
title: ION / IMPULSE / ROSA — Program Architecture (Aligned Spine)
description: The reusable spine instantiated for the actual build — the three-layer stack and the real in-flight personal-data efforts classified as sensors, actuators, and brains
version: '0.1'
updated: 2026-06-30
type: research
category: architecture
phase: phase3
status: draft
audience: builders
tags: [program-architecture, rosa, impulse, ion, capability-taxonomy, sensors-actuators-brains, multi-tenancy]
---

# ION / IMPULSE / ROSA — Program Architecture (Aligned Spine)

> Draft / exploratory. The generic starter
> (`~/code/_templates/agent-native-project-starter/`) instantiated for **what we are actually
> building**. Builds on [2026-06-30-stack-consolidation-contract-and-next-steps.md](2026-06-30-stack-consolidation-contract-and-next-steps.md)
> and the stack-architecture decision. The point of this doc: stop describing the spine abstractly and
> map the *real* efforts onto it.

## 1. What we are building (concrete target)

A personal-intelligence platform: **ROSA** (flagship personal-OS agent, rich visual frontend) on
**IMPULSE** (orchestration + memory + provenance over a fleet of coding/agent providers) on **ION**
(first-party Rust agent, registry-uniform). The near-term concrete slice is ROSA's
**knowledge-ecosystem** — personal data continuously enriched into a queryable, provenance-tagged
knowledge base that keeps the active agents light. Build order is **ROSA-first on top**, gated by two
infrastructure prerequisites (provenance bridge, ION-as-registered-agent).

## 2. Four layers → actual components

| Layer | In this stack |
|---|---|
| **Data** | Local-first knowledge base (markdown + SQLite), `GENOME.md` (typed JSON), `HISTORY.jsonl`, per-tenant KB stores. Accessed via repositories only. |
| **Logic / Execution** | ION (coding work) · IMPULSE handlers + micro-agents (enrichment "brains") · MCP tool impls. |
| **Trigger** | Platform hooks (agent activity) · explicit IPC/CLI · scheduled enrichment (greenfield) · agent-routed (ROSA) · approval-gated mutations. |
| **Presentation** | ROSA frontend (graphs/charts/cards) · voice (ElevenLabs) · wiki-markdown · CLI/TUI. |

## 3. Capability catalog — the real efforts, classified (the centerpiece)

This is the alignment: each **existing or planned effort** mapped to `kind × execution_tier`, its
layer home, gating, and tenancy. (Grounded in current projects; rows marked *illustrative* need your
confirmation of scope/shape.)

| Effort (real) | kind | execution_tier | Gated? | Provenance + tenancy |
|---|---|---|---|---|
| iMessage read (chat.db / harness) | sensor | code | no | per-tenant; read-only |
| Personal-CFO data pull (Monarch csv, the 7 json) | sensor | code | no | per-tenant; `do-not-unify` across raw sources |
| Calendar / notes / weather lookup *(illustrative)* | sensor | code | no | per-tenant |
| iMessage → derived-store enrichment | **brain** | micro-agent | only if it writes KB | every write carries slug provenance |
| Personal-CFO synthesis (insights/audit layers) | **brain** | micro-agent / agent | writes → gated | federation, never merge; exact values preserved |
| Weather → wiki processor *(the running example)* | **brain** | micro-agent | writes → gated | provenance on each entry |
| Send iMessage / reply | **actuator** | code | **yes** (wire-gate) | compensator: none (irreversible) → always `ask` |
| ElevenLabs outbound call / voice op | **actuator** | code/agent | **yes** (wire-gate) | re-approval per mutation class |
| Create calendar event / write KB entry | **actuator** | code | **yes**, may `defer` low-risk | compensator = delete/undo entry |
| ION coding action (edit/run/test) | mixed | agent | writes → supervisor gate | registry-slug provenance |

**Reading of this table:** your personal-data projects are *already* the sensors and brains of ROSA;
the actuators are the few things that touch the outside world (messages, calls, KB writes) and they
are exactly the set that must pass the wire-gate. The knowledge ecosystem = sensors feeding brains
that write a provenance-tagged KB; ROSA's active agents *query* it, never carry it.

## 4. Seams + the knowledge layer (grounded in current code)

- **ION ↔ IMPULSE** — typed Rust contract over the Unix-socket daemon protocol (`DaemonRequest`,
  `PROTOCOL_VERSION = 2`). Control-plane inner seam: stays typed (R3).
- **ROSA ↔ IMPULSE** — MCP at the edge (Streamable HTTP), exposing the *unified* surface (today only
  a tooling subset is exposed; the four action enums need consolidating first).
- **Knowledge layer** — the wiki/KB is the long-term memory; the consolidated-view read model is the
  "smart targeted query" interface so ROSA stays context-light.

## 5. Decisions applied + multi-tenancy

- **ION is registry-uniform** — advertises a capability subset from the closed universe; no
  privileged side-channel.
- **R1 / R2 / R3** as in the consolidation note (closed capability universe; open-outward/
  fail-closed-inward; typed core + MCP edge).
- **Multi-tenancy from day one:** every personal-data capability is parameterized by `tenant_id` +
  persona handle, even with one tenant (James) today. James-KB and Tiffany-KB are **federated
  tenants, never merged** — `do-not-unify` applied to users. This is nearly free now, brutal as a
  retrofit.

## 6. Sequencing (dependencies, not dates)

**Critical path:** unified contract schema → (extend `AgentDescriptor.capabilities` ⊕
provenance-bridge: `Platform`+`session_id` → registry slug) → unify the gate → MCP edge server →
**ROSA consuming surface + first knowledge-ecosystem slice**. Off-path: `HarnessRequest` naming-fix
(P0), trigger-intake layer (scheduler/watcher), per-tenant KB partitioning.

## 7. Open — needs your steer

1. **Scope confirm:** is the immediate target the full program foundation, or the first concrete ROSA
   slice (knowledge-ecosystem: pick *which* personal-data source enriches first — iMessage? CFO?
   weather?)?
2. **Complete the capability catalog** — confirm/correct the *illustrative* rows and add any missing
   sensors/actuators/brains you have in mind for ROSA.
3. **Confirm the three forks** (R1 closed-universe · R2 inner/outer boundary · R3 typed-core/MCP-edge).
