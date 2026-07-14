---
title: Test Traceability Matrix
description: Story-to-test coverage map for the current Rust Impulse workspace and open validation gaps
version: '1.2'
updated: 2026-07-13
type: specification
category: testing
phase: all
status: active
audience: builders
tags: [testing, traceability, validation, rust]
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# Test Traceability Matrix

> Story baseline: [`USER-STORY-MAP.md`](./USER-STORY-MAP.md)
> Contract anchor: [`RUST-CANONICAL-CONTRACT.md`](./RUST-CANONICAL-CONTRACT.md)

## Coverage Labels

| Label | Meaning |
| --- | --- |
| Strong | Dedicated unit and integration evidence exists in the current Rust workspace |
| Moderate | Some direct automated evidence exists, but important edges remain indirect or thin |
| Thin | The capability exists, but automated evidence is incomplete or mostly incidental |
| Manual | The current claim still depends on human-run validation evidence |

## Story Coverage Matrix

| Story | Current evidence | Coverage | Main gaps |
| --- | --- | --- | --- |
| ST-01 Start a tracked coding session | `impulse-rs/tests/integration_enhancements.rs`, daemon lifecycle tests, session-related state modules | Moderate | direct CLI dispatch coverage is still thinner than the stable CLI contract implies |
| ST-02 End a session with a durable summary | `impulse-rs/tests/integration_enhancements.rs`, verification-related paths, state persistence modules | Moderate | verification semantics are stronger than dispatch-level coverage; end-to-end failure cases should grow |
| ST-03 Track files and tools touched during work | daemon protocol + integration flows, handler/common helper coverage | Moderate | direct command routing remains under-tested relative to importance |
| ST-04 Inspect project memory quickly | `impulse-rs/src/handlers/common.rs` local tests, config and validation tests, workspace command coverage | Moderate | command-level regression coverage for the full stable CLI surface remains uneven |
| ST-05 Search prior work with safe fallback behavior | `impulse-rs/tests/integration_enhancements.rs`, retrieval modules, explainability and fallback tests | Strong | performance and scale assertions remain mostly benchmark-driven rather than gating |
| ST-06 Stage context before injecting it | `impulse-rs/src/injection/engine.rs`, `src/injection/staging.rs`, `src/handlers/injection_handlers.rs`, `src/orchestration/mod.rs`, integration enhancement paths | Strong | the biggest remaining gap is full-flow end-to-end validation of retrieval-seeded injection effects across output plus on-disk artifacts |
| ST-07 Produce handoff artifacts for the next agent or session | orchestration tests, injection-handler tests, context artifact contracts, file-path assertions in current Rust modules | Strong | stronger end-to-end artifact assertions would still reduce regression risk |
| ST-08 Use a daemon as the long-lived source of truth | `impulse-rs/src/daemon/tests.rs`, `src/daemon/protocol.rs`, integration daemon guard flows, daemon-adjacent handler coverage | Strong | the open risk is workbench IPC lifecycle coverage, not the absence of daemon tests |
| ST-09 Observe work through the Dioxus desktop host | `impulse-rs/impulse-desktop/src/daemon_ops.rs`, `impulse-rs/impulse-desktop/tests/desktop_contract.rs`, `impulse-rs/impulse-desktop/tests/host_surface.rs`, `impulse-rs/impulse-desktop/tests/runtime.rs`, `impulse-rs/impulse-term/tests/backend_tests.rs`, `impulse-rs/src/daemon/tests.rs::test_publish_then_subscribe_reconciles_terminal_agent_through_real_handler`, and `impulse-rs/impulse-desktop/scripts/host_readiness_smoke.mjs` | Strong | compositional coverage proves Unix JSONL client framing, separate real-handler reconciliation, lifecycle reduction, registry-derived platform catalogs across MCP/host/browser/UI, strict unknown-platform rejection, opt-in real sibling-Ion launch, and browser-host readiness; one packaged desktop-to-real-daemon E2E plus multi-workspace routing remain open |
| ST-10 Review risky context and stewardship actions explicitly | stewardship modules, guardrail and approval surfaces, integration enhancement coverage | Thin | stewardship command dispatch and operator decision paths need clearer regression tests |
| ST-11 Enforce verification-before-completion | `impulse-rs/src/validate.rs`, recent invalid-direct-request fixes, session-end verify flows | Strong | manual operator acceptance still matters for claim wording, but automated coverage is present |
| ST-12 Prove the real hook memory loop before expanding claims | `impulse-rs/tests/hook_validation_session_start.rs`, `hook_validation_precompact.rs`, `hook_validation_extraction_benchmark.rs`, `docs/guides/HOOK-VALIDATION-GUIDE.md` | Manual | the code can generate evidence, but product truth still depends on real external hook runs |
| ST-13 Complete one governed supervisor-and-builder vertical slice | `impulse-rs/impulse-ops/tests/{governed_task_contract,governed_producer_contract}.rs`; `impulse-rs/src/{governed_producers.rs,state/governed_task.rs,agent/mod.rs}` plus daemon protocol/handler tests; `impulse-rs/tests/governed_process_flow.rs`; desktop runtime/contract tests and `impulse-term` process tests | Moderate | the real daemon/CLI/child-process test proves claim, detached verification, persistence, and restart only through `awaiting_supervisor`; in-process handler tests separately prove strict API review, harness fail-closed behavior, and operator-only acceptance. One full launched-runtime process and accepted-run memory promotion remain open |

## High-Signal Existing Test Surfaces

### Integration-level anchors

- `impulse-rs/tests/integration_enhancements.rs`
  - exercises guardrail/config, retrieval fallback, search explainability, daemon flows, and coordination-adjacent behavior
- `impulse-rs/tests/hook_validation_session_start.rs`
  - validates sentinel emission and evidence-file creation for SessionStart
- `impulse-rs/tests/hook_validation_precompact.rs`
  - validates PreCompact evidence capture mechanics
- `impulse-rs/tests/hook_validation_extraction_benchmark.rs`
  - validates extraction benchmark harness behavior

### Module and crate-level anchors

- `impulse-rs/src/handlers/common.rs`
  - now contains a substantial local test block and should no longer be described as untested
- `impulse-rs/src/handlers/config.rs`
  - proves repository-root init preserves existing `.gitignore` content, appends only exact
    runtime-owned paths idempotently, never adds a blanket rule, and detects an existing blanket
    rule that can still hide durable `.impulse` artifacts
- `impulse-rs/src/handlers/direct_dispatch.rs`
  - contains a large in-file async test block covering direct command dispatch behavior
- `impulse-rs/src/handlers/daemon_dispatch.rs`
  - has broad parsing, helper, and dispatch-table coverage for daemon request handling
- `impulse-rs/src/handlers/injection_handlers.rs`
  - has direct tests for `orchestrate`, `handoff`, `sync-context`, invalid modes, and computed injection output
- `impulse-rs/src/handlers/agent.rs`
  - includes config, status, and query coverage
- `impulse-rs/src/handlers/guard.rs`
  - includes meaningful guard and analytics coverage
- `impulse-rs/src/validate.rs`
  - explicit validation rule coverage
- `impulse-rs/src/orchestration/mod.rs`
  - orchestration and context artifact logic coverage
- `impulse-rs/src/daemon/tests.rs`
  - daemon state and protocol behavior, including the real governed-task register/mutate/get/list handler route
- `impulse-rs/src/state/governed_task.rs`
  - durable governed-task transition, evidence-boundary, project-scope, replay, restart, and concurrent-decision coverage
- `impulse-rs/impulse-ops/tests/governed_task_contract.rs`
  - wire/serde compatibility and identity/role-compatibility invariants for governed tasks
- `impulse-rs/impulse-ops/tests/governed_producer_contract.rs`
  - profiled registration validation, exact canonical Builder binding, old-payload compatibility,
    omission-based trigger DTOs, and strict versioned Supervisor-envelope contracts
- `impulse-rs/src/governed_producers.rs` and daemon handler tests
  - clean Git binding, detached fixed-command verification, output digests/caps, timeout/reaping,
    bounded post-kill background reaping, partial-worktree cleanup, committed regular-lockfile and
    `--locked` enforcement, source/lockfile symlink rejection, ignored-file byte mutation detection,
    Tokio responsiveness during blocking Git operations, replay without re-execution, shared task
    serialization, daemon-registry compatibility recomputation and forged-result rejection,
    lossless executable provenance, exact-model-digest Supervisor provenance, strict
    criteria-digest review binding, generic-mutation rejection, and operator-only acceptance
- `impulse-rs/tests/governed_process_flow.rs`
  - fresh real init plus durable-file commit, real daemon plus CLI claim, real detached Rust command
    execution, durable evidence, daemon restart, and exact `awaiting_supervisor` record reload; it
    does not run Supervisor/operator steps
- `impulse-rs/impulse-desktop/src/daemon_ops.rs` and Desktop runtime tests
  - require an absolute executable control CLI (including configured/PATH/sibling paths), reject
    missing or non-executable candidates before profiled registration, use the longer bounded
    profiled-registration read without duplicate sends, quote routed guidance, and prove ordinary
    PTYs cannot inherit governed routing variables from the Desktop parent
- `impulse-rs/impulse-term/tests/backend_tests.rs`
  - terminal backend behavior
- `impulse-rs/impulse-desktop/src/bridge.rs`
  - daemon-backed GUI state and snapshot logic

## Known Coverage Gaps To Prioritize

These gaps matter because they sit on stable or nearly stable public interfaces:

1. Add one complete launched-runtime governed supervisor-and-builder E2E
   Reason: the daemon-owned claim/verification/review chain is proven compositionally, and the
   claim/verification/restart path is proven with real processes. One proof must still launch the
   Builder through DesktopRuntime, run the configured API Supervisor path in the same workflow,
   carry evidence into operator acceptance, and create only a review-only memory candidate.
2. Add a packaged desktop-to-real-daemon E2E
   Reason: Unix client framing, real handler reconciliation, reducer behavior, and browser-host readiness are proven separately; one process-level test must connect those boundaries before calling the full desktop path closed.
3. Add multi-workspace daemon-routing tests
   Reason: the delivered desktop adapter intentionally derives one project from one daemon socket; a first-class manager must route distinct workspace agents without cross-project bleed.
4. Complete daemon socket workbench coverage
   Reason: `PublishTerminalOps` → `SubscribeOps` has compositional client/handler regressions; `ListArtifacts`, `GetArtifact`, and `RunArtifactAction` still need equivalent desktop-client proof.
5. Stable CLI mutation flow integration tests
   Reason: `session-start`, `session-end --verify`, `track-write`, and `track-tool` should be asserted against real `.impulse/*` artifacts.
6. Stewardship integration tests
   Reason: `steward analyze`, `compact`, `approve`, and `reject` are still underrepresented relative to their safety importance.
7. End-to-end injection mode tests
   Reason: `off|review|apply` should be proven against both returned output and emitted artifact/log behavior.

## Documentation Corrections Captured By This Matrix

### Corrected assumptions

- The current product is Rust-first and workspace-based, not `impulse/` plus Vitest.
- handler coverage is materially better than older docs implied; the problem is not “no tests,” it is “not enough full-flow coverage where the product contract is strongest.”
- `src/handlers/common.rs` is no longer accurately described as untested.
- hook validation is best described as evidence-generating automation plus required real-world proof, not as a fully closed claim.
- agent control is no longer wholly future work: registered runtimes can launch, receive input, focus, close, and publish lifecycle truth under a concrete supervisor policy.

### Still true

- the stable CLI contract needs broader regression coverage than it currently has
- single-project daemon-truth GUI behavior has direct automated evidence; multi-workspace routing remains an active delivery lane
- daemon-owned profiled claims, detached Rust verification, and strict API Supervisor review are live
  and directly tested; the broader supervisor/builder claim remains incomplete until one process
  composes launched runtimes and promotes only an accepted result to a review-only memory candidate
- narrow static product-role launch preflight is live; generalized role composition and dynamic runtime capability negotiation remain target-only and must not be inferred from it
- validation evidence should continue to gate stronger marketing or architectural claims

## Recommended Test Expansion Order

1. Add the complete launched-runtime governed supervisor/builder E2E across DesktopRuntime launch,
   daemon-derived claim/verification, API Supervisor review, operator approval, and review-only
   accepted-run memory promotion.
2. Add a packaged desktop-to-real-daemon E2E across publish, subscribe, host event, and reducer boundaries.
3. Add multi-workspace daemon routing and cross-project isolation tests.
4. Extend daemon client/handler coverage to artifact list/get/action paths.
5. Add stable CLI mutation tests that assert real `.impulse/*` state transitions.
6. Add stewardship integration tests with proposal and approval artifact assertions.
7. Keep hook validation evidence generation automated, but treat real external hook runs as release-gating proof.
