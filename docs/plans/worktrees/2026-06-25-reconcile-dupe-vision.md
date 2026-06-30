---
title: Reconcile duplicate Impulse codebases + codify multi-workspace/agent orchestration vision
description: Work card for the active goal on agent/codex-dioxus-host-goal-cleanup
updated: 2026-06-25
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, reconcile, dioxus, workspace, agent, vision]
---

# Reconcile duplicate Impulse codebases + codify multi-workspace/agent orchestration vision

## Lane Facts
- Owner: Grok (implementer)
- Role: Execute the goal plan fully: reconcile IMPULSE-rs vs .clean to single canonical, ensure observable multi-workspace + multi-agent CLI support, update living docs with the objective vision (tech lead, terminal logins for claude-code/codex/etc, cycle project spaces + agents, Rust type-safe augmentation, subagents for load reduction), run full verification.
- Branch: agent/codex-dioxus-host-goal-cleanup
- Worktree: (repo root; no additional worktree spawned for doc+test scope)
- Owned paths:
  - docs/plans/worktrees/2026-06-25-reconcile-dupe-vision.md (this card)
  - docs/decisions/0009-reconcile-impulse-copies.md (new)
  - CONTEXT.md
  - README.md
  - HANDBOOK.md
  - docs/spec/RUST-CANONICAL-CONTRACT.md
  - impulse-rs/impulse-ops/src/agent_registry.rs (if test extension needed)
  - impulse-rs/impulse-desktop/src/workspace.rs (if test extension needed)
  - The plan.md for this goal (checklist flips + deviations only)
- Blocked/shared paths:
  - All Cargo.toml / Cargo.lock / AGENTS.md / CLAUDE.md / protocol specs / shared core (read-only for this lane; no structural refactor)
  - impulse-gui (frozen legacy)
  - No changes to dioxus host UI/views per non-goals
- Plan/spec: /Users/jamespustorino/.grok/sessions/%2FUsers%2Fjamespustorino%2Fcode%2FIMPULSE-rs/019efcf9-3b22-74d0-a523-b81b4110138b/goal/plan.md
- Verification: per plan Verification plan (gate, launch twice to SCRATCH, doc grep, unit test exercise, evidence)
- Latest status: in progress — inspection complete; work card + side-by-side + decision record next

## Decisions
- 2026-06-25: Active tree (this checkout) is the canonical; .clean is a historical reference snapshot from "clean/dioxus-pty-orchestrator" experiment. No git merge of divergent histories. Reconciliation = decision record + vision docs + confirmation that existing registries already provide the observables.
- 2026-06-25: Agent support for multiple CLIs (claude-code, codex, cursor, gemini...) is already first-class and exercised in impulse-ops::agent_registry (builtin + TOML extensibility + tests). Do not add new CLIs.
- 2026-06-25: Multi-workspace (distinct project folder roots) support for cycling spaces + agents-per-space is already present in impulse-desktop's WorkspaceRegistry (register/list/touch + MCP exposure) and used by the Dioxus host. The clean/ version is a cleaner contracts-based design for future layering but is not ported now (non-goal: overbuild).
- 2026-06-25: "Impulse Agent as always-on tech lead" language goes into living docs (CONTEXT, README, HANDBOOK ImpulseAgent section, RUST-CANONICAL-CONTRACT) verbatim per objective. No new runtime behavior.
- 2026-06-25: Subagent + workflow load reduction is documented intent (existing orchestration/supervisor/daemon + plan to use subagents); no new spawning impl.

## Changes
- Pending: create work card, create decision ADR 0009, update 4 living docs with vision, ensure/ add 1-2 direct tests if coverage gap for "multi registration + listing", run gate + entrypoints x2 to SCRATCH, append note to CONTEXT, final evidence.
- Use archive-don't-delete for any reference material moved.

## Tests
- Existing high-density tests in agent_registry.rs (builtin lists 6 agents incl. claude-code+codex; resolve/detect/list/merge/serde) and desktop workspace.rs (register N roots, list, touch, error paths, concurrent safety in clean equiv).
- Will run targeted `cargo test -p impulse-ops --test ...` or workspace agent/workspace tests + full gate.
- Any added test must be direct (construct real types, exercise register/list on real impl, assert observable multi).

## Handoff Notes
- This work is self-contained on the goal lane. After completion, the .clean sibling checkout remains as-is (reference only); future unification work (if any) would be a separate lane with explicit ownership of crate boundaries.
- Desktop host continues on Dioxus per prior lane; this goal only touches docs + verification + possibly one test addition in registry modules.
=== FINAL DEVIATIONS NOTE ===
- 2026-06-25: .clean sibling relocated (mv) to active/archive/_archived-* to achieve single canonical tree per AC1 (git prune + ls + proof test confirm); full contents archived; no parallel maintenance.
