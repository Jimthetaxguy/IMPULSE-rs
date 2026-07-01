---
title: Key Cleanup + Next Steps — Routine Primitive / Stack Spine
description: Session wrap-up — what landed, what's decided, what's open, and the exact next action
version: '0.1'
updated: 2026-06-30
type: research
category: handoff
status: active
audience: builders
tags: [handoff, next-steps, routine-primitive, impulse, ion, rosa, brainstorming-gate]
---

# Key Cleanup + Next Steps

> Wrap-up for the IMPULSE/ION/ROSA design arc and the Phase-1 Routine vertical.
> We are paused at the **brainstorming user-review gate** — no code until the spec is approved.

## 1. What landed this session (durable artifacts)

| Artifact | Path | State |
|---|---|---|
| Phase-1 Routine spec (THE active spec) | `docs/research/2026-06-30-routine-primitive-phase1-design.md` | hardened, awaiting review |
| Stack consolidation contract (R1/R2/R3 + Frameworks A–E) | `docs/research/2026-06-30-stack-consolidation-contract-and-next-steps.md` | settled |
| Aligned-spine program architecture (real efforts → sensors/actuators/brains) | `docs/research/2026-06-30-program-architecture-aligned-spine.md` | draft, capability catalog needs row-confirm |
| Multi-agent provenance/divergence note | `docs/research/2026-06-30-multi-agent-provenance-divergence.md` | merged (PR #12) |
| Reusable starter kit | `~/code/_templates/agent-native-project-starter/` | built; SECURITY.md + EVALS.md pending |
| Memory: stack model | `~/.claude/projects/-Users-jamespustorino/memory/project_stack-architecture-impulse-ion-rosa.md` | written |
| Memory: starter kit | `…/memory/reference_agent-native-project-starter.md` | written |

## 2. Decided (canon — do not re-litigate)

- **Three-layer stack:** ION (L0 execution) ⊂ IMPULSE (L1 orchestration/memory/provenance) ⊂ ROSA (L2 personal-OS); each consumes the layer below as a tool.
- **R1** — closed capability universe (deny-by-omission; advertise-subset-never-add).
- **R2** — discoverability ∝ distance from control-plane; open-outward, fail-closed-inward.
- **R3** — typed contract is authority; MCP at the edge, typed core inside ("MCP semantics everywhere, MCP transport at edge").
- **ION is registry-uniform** — no privileged side-channel.
- **Multi-tenancy from day one** — Tenant → Account → Provider (closed enum); James-KB / Tiffany-KB federated, never merged (`do-not-unify`).
- **First vertical** — calendar + weather → `daily-brief` brain Routine, new standalone Rust crate, **Approach C** (trait interface first; `RuleBasedBrief` impl₁, LLM brain iter₂).
- **Home location** — Long Island City (default).
- **LLM-agnostic + modular** — `LlmProvider` trait, feature-gated adapters, config-driven.
- **Best-practices doc folded** — risk tiers T0–T4, prompt-injection fencing (#9), observability/correlation-IDs (#10), ledger-derived KB (#11) all in the spec.

## 3. Open — needs your call before code (spec §12)

1. **Calendar access** — `gws` shell-out (read-only) — recommended. Confirm?
2. **Crate name** — `routines` vs `rosa-routines`?
3. **Structure** — single crate + feature-gated adapters (rec) vs workspace?
4. **Secret backend** — Infisical and/or Keychain; start with one?
5. **Config location** — `~/.config/routines/<tenant>.toml` (XDG)?
6. **Starter-kit enrichment** — add `SECURITY.md` (T0–T4 + OWASP) + `EVALS.md` (eval template) now, or defer?

(#3 home location is resolved → LIC.)

## 4. The exact next action

Per the brainstorming HARD GATE, the terminal state is **invoking writing-plans** — but only **after** you:
1. Answer the six items in §3 (or say "go with your recommendations"), and
2. Approve the Phase-1 spec.

Then I write `docs/superpowers/plans/2026-06-30-routine-primitive-phase1.md` and offer subagent-driven vs inline execution.

## 5. Repo hygiene carryover (not blocking, do not auto-act)

- IMPULSE-rs: ~9 local unpushed commits earlier in week — push when ready.
- ROSA_RenewBuild: P0, 5 unpushed + stashes (commit `fc4cbd5` local). Do **not** push casually.
- Config spec commit `682308a` — local, non-PR surface.
- Starter kit not yet wired into global CLAUDE.md/AGENTS.md — opt-in, left to you.

## 6. Roadmap decision (2026-06-30) — actuators after reads

**Decided:** infra/deploy tools (Railway; **not** Vercel — retired 2026-06-11) become a **Phase 2
actuator vertical**, built *after* the Phase-1 daily-brief read vertical ships.

- Deploy = **T3/T4 actuator**, the highest-risk capability class → governed by the **wire-gate**
  (approval-auditor before, warden after, saga compensator at call-time). Phase 2 is where the
  wire-gate gets built for real.
- **R3 applies:** wrap the **existing Railway MCP server** as a wire-gated capability in the same
  closed registry — do **not** hand-roll a deploy API client.
- Phase 1 is reads-only (no actuator path yet); its engine, registry, provenance, config, and
  `SecretStore` are reused verbatim by the actuator vertical. The only net-new piece is the wire-gate
  modifier (`{allow|deny|ask|defer, compensator}`).
- Layer note: deploy tooling primarily serves **ION/IMPULSE** (coding layers), not ROSA's personal-OS
  knowledge ecosystem — different consumer, same spine.

## 7. One-line status

**Phase-1 plan written + self-reviewed (`~/code/routines/docs/superpowers/plans/2026-06-30-routine-primitive-phase1.md`, 13 TDD tasks). Executing subagent-driven. Phase 2 = Railway deploy actuator (wire-gate).**
