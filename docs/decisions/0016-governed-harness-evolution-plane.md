---
title: "ADR-0016: Governed Harness Evolution Plane"
status: draft
created: 2026-08-26
deciders: [Impulse Maintainers]
---

# ADR-0016: Governed Harness Evolution Plane

## Status

Draft. Proposed for review alongside open VISION decision 6 (candidate promotion/dismissal
authorization). Prior art: AutoSaddler (arXiv:2608.23041, Microsoft Research / POSTECH / KAIST /
SUSTech, 2026-08-24).

## Context

Impulse's harness — role contracts, injection/prompt surfaces, the DynamicTool registry, guardrail
rules, verification policies, and the ADR-0015 step model — is today edited only by humans and
agents acting as humans: free-form file edits, reviewed through ordinary Git flow. Nothing treats
the harness itself as a versioned, learnable systems artifact whose changes are proposed from
execution evidence, evaluated in isolation, and promoted under explicit authority.

AutoSaddler provides the strongest published evidence to date that this loop works and how it
fails. Three of its findings bind directly to Impulse's design:

1. **Deep trace-grounded diagnosis outperforms shallow reflection.** Its diagnosis agent inspects
   full trajectories, tool calls, environment state, and harness source by reference rather than
   flattening everything into one prompt. Impulse already has the substrate for this: governed-task
   records, verification receipts, and telemetry are typed, daemon-owned evidence (ADR-0011,
   ADR-0012).
2. **Structured, targeted patches outperform unconstrained editing.** AutoSaddler's taxonomy
   distinguishes steering patches (textual: prompts, tool descriptions, reminder hooks) from
   capability patches (executable: new tools, implementation fixes, loop changes). Local fix rates
   were similar (58% vs 55%), but steering patches regressed unrelated behavior at 2x the rate
   (17% vs 8%). Structural failures deserve structural fixes; prompts must not become the dumping
   ground for missing system design.
3. **Generalization-aware selection with lineage memory beats trajectory-specific repair.** Its
   EvoDAG stores every explored harness version with scores, lessons, and diffs, enabling revert,
   rebase, and cherry-pick when an accumulation of patches regresses (a documented 12.3-point drop
   was recovered by pruning back to conservative patches).

AutoSaddler itself is offline, stateless-task, and explicitly not production self-modification.
Its authors recommend human review before optimized harnesses reach production. That boundary is
exactly the boundary Impulse already enforces for memory: ADR-0013 made accepted runs project into
deterministic, review-only candidates, with promotion deferred to an explicit operator action.
This ADR extends the same shape to harness changes, rather than inventing a second promotion
system or permitting a self-applying optimizer.

## Decision

Adopt a governed Harness Evolution Plane with these rules:

1. **The harness is an enumerated, versioned artifact.** A `HarnessVersion` is a content-hashed
   manifest over the named harness surfaces: role contracts, injection/prompt surfaces, tool
   contracts, guardrail rules, verification policies, and step-model policy. Two harnesses with
   identical manifests are the same harness. The manifest hash uses the ADR-0013 discipline: fixed
   ordered struct, exact JSON bytes, no maps, no floats, explicit schema version.
2. **Harness changes are proposed as a typed patch IR, never as free-file edits.** A
   `HarnessPatch` carries: `patch_class` (`steering` | `capability`), a closed `patch_kind` enum
   (prompt rule add/modify, tool add, tool contract modify, tool implementation fix, guardrail
   rule change, verification policy change, step-model policy change), target artifact references,
   the diagnosed failure class, a falsifiable hypothesis, expected fixes, predicted regression
   classes, and required validation suites. Free-form edits to harness surfaces by an optimizing
   agent are out of contract.
3. **Evolution proposals are governed candidates.** One diagnosis produces one deterministic
   harness-patch candidate (`harness-candidate-<sha256>`, same derivation discipline as
   ADR-0013's memory candidates) in an owner-only review ledger. v1 lifecycle is `pending_review`
   only: no promote, apply, or dismiss request exists in this slice, and no optimizer applies its
   own patch. Promotion authority is the operator, through the same authorization path VISION
   open decision 6 will settle for memory candidates.
4. **Candidate evaluation happens in disposable worlds.** A candidate harness is materialized and
   evaluated only inside detached-worktree execution (`rust_workspace_v1` machinery from
   ADR-0012), never against the live daemon or the authoritative workspace. Evaluation produces
   an evidence bundle: pass/fail deltas, fixed and regressed task lists, and cost. A candidate
   without an evidence bundle cannot leave `pending_review`.
5. **Regression budgets are class-aware.** Reflecting the 2x regression asymmetry, steering-class
   patches require a generalization check beyond the motivating mini-batch before promotion is
   even recommendable; capability-class patches require contract tests over the touched tool or
   policy surface. Both budgets are explicit fields of the patch's validation requirements, not
   reviewer folklore.
6. **Lineage is remembered.** An append-only evolution ledger (EvoDAG-shaped: nodes are harness
   versions with evaluation summaries and lessons; edges are patch diffs) is persisted with
   private-file atomic replacement beside the governed-task ledger. Revert, rebase, and
   cherry-pick across lineages are operator actions over this ledger in v1, not autonomous moves.
7. **Explicit non-goals for v1.** No automatic promotion. No mutation of a live daemon's harness.
   No model-weight learning. No memory or skill curation through this plane (that remains
   ADR-0013's downstream path). No continuous background optimizer process; scheduling evolution
   runs is a separate decision with its own governance.

## Consequences

- Impulse gains a principled place to put what it already learns: governed-task evidence and
  verification receipts become training signal for the harness, not just audit records.
- The ADR-0013 candidate pattern is reused rather than duplicated, so open decision 6 settles
  promotion authorization once for both memory and harness candidates.
- The typed patch IR constrains optimizer blast radius by construction: an evolution agent's
  entire write surface is one candidate record; everything else it wants must survive review.
- Cost: harness surfaces must be enumerated and manifest-hashed before any of this runs, and the
  detached-worktree evaluator must learn to boot a candidate harness. Both are prerequisites, and
  both are useful independently (reproducible harness identity; hermetic harness tests).
- Risk: patch taxonomies ossify. The `patch_kind` enum is versioned and additive by design; an
  unclassifiable but valuable change is a signal to extend the enum through this same ADR
  process, not to bypass the IR.

## Relation to open decisions and other work

- **Feeds VISION open decision 6** (promotion/dismissal authorization): harness candidates and
  memory candidates should share one authorization design.
- **Depends on decisions 2 and 4** (runtime-adapter interface; role contract composition) for the
  role-contract portion of the harness manifest; the manifest can ship earlier covering tools,
  guardrails, verification, and step-model surfaces.
- **Composes with** `sandbox-agent-analysis.md` (2026-08-22): Builder snapshot-at-mutation /
  `staged_authoritative` world scope is a sibling decision (candidate ADR-0017) and would give
  candidate evaluation stronger isolation than detached worktrees alone.
- **Lane placement:** net-new Lane 9 (Harness Evolution) in `docs/LONG-RANGE-ENHANCEMENTS.md`,
  composing with Lane 1 (its evaluation suites are this plane's promotion gates) and Lane 3
  (lessons feed memory quality). Registration of Lane 9 is deferred to the implementation lane.
- **Prior art:** AutoSaddler (arXiv:2608.23041); GEPA and Meta-Harness (outperformed baselines in
  its evaluation); OpenWiki 0.4.0 claims runtime (evidence-versioned beliefs; the evolution
  ledger's lessons should carry evidence references in the same spirit).
