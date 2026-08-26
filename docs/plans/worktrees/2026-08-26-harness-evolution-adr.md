# Lane: Harness Evolution Plane ADR (2026-08-26)

- **Owner:** claude-code (Fable 5), dispatched by James 2026-08-26 ~02:14 ET ("Work on this for impulse", AutoSaddler arXiv:2608.23041)
- **Role:** design lane — ADR authoring only, no implementation
- **Branch:** `agent/claude-harness-evolution-20260826` (from origin/main @ b2b38cb)
- **Worktree:** `.worktrees/harness-evolution-adr`
- **Owned paths:** `docs/decisions/0016-governed-harness-evolution-plane.md`, this card
- **Blocked/shared paths:** everything else. Root checkout dirty state (step-model re-do vs PR #33) is a separate pending decision — this lane must not touch it. `docs/LONG-RANGE-ENHANCEMENTS.md` Lane 9 registration deferred to implementation lane to avoid conflicting with in-flight doc edits.
- **Verification commands:** docs-only lane — `git diff --stat origin/main` shows only owned paths; docs validator if run must not regress existing baseline (known pre-existing failures: ADR-0014 metadata, 3 freshness items).

## Intent

Turn AutoSaddler's evidence (harness = versioned learnable artifact; deep diagnosis > shallow
reflection; typed patches > free edits, steering 2x regression vs capability; lineage memory +
generalization-aware selection) into an Impulse-native decision that reuses the ADR-0011/0012/0013
governance chain instead of adding a second promotion system.

## Gate checklist

- [x] ADR-0016 drafted in house style (frontmatter, numbered decision rules, consequences,
      relation to open decisions)
- [x] Grounded against ADR-0013 candidate discipline, `rust_workspace_v1` detached-worktree
      producer, VISION open decisions 2/4/6, `sandbox-agent-analysis.md`
- [x] ADR number 0016 claimed (0001-0015 taken on main; the snapshot-at-mutation idea from
      `sandbox-agent-analysis.md` shifts to candidate ADR-0017 — noted in ADR text)
- [ ] James review: accept/amend ADR-0016 (draft status until then)
- [ ] On acceptance: register Lane 9 in LONG-RANGE-ENHANCEMENTS.md; open implementation lane

## Implementation sketch (dependency-ordered, for the future lane — NOT this one)

Phase A blocks everything: harness-surface enumeration + `HarnessVersion` manifest hashing
(likely `impulse-ops`, reusing ADR-0013's exact-bytes digest discipline).
Phase B after A: `HarnessPatch` IR types + deterministic `harness-candidate-<sha256>` projection
into an owner-only ledger (mirror of `MEMORY_CANDIDATES.json` machinery).
Phase C after B: diagnosis producer profile — a governed producer that reads accepted-run evidence
and emits at most one patch candidate; evaluation via detached-worktree runs producing evidence
bundles.
Phase D after B (parallel with C): Dioxus read-only candidate review surface beside the memory
candidate view.
Cross-cutting: promotion authorization lands with VISION decision 6, shared with memory
candidates; Builder `staged_authoritative` world scope (candidate ADR-0017) strengthens Phase C
isolation.

## Log

- 2026-08-26 02:2x ET — worktree created from b2b38cb; ADR-0016 drafted; card written; committed
  and pushed for backup. No other paths touched.
