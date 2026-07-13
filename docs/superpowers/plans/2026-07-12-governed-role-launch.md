# Governed Role Launch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make an agent launch carry an explicit product role and task, evaluate the selected runtime's enforcement compatibility before PTY creation, and show the result honestly in the Dioxus cockpit.

**Architecture:** Add a backward-compatible role-assignment and launch-capability contract to `impulse-ops`. Runtime descriptors declare conservative support; the Dioxus launcher previews the shared evaluator, while `DesktopRuntime::spawn_agent` repeats the evaluation as the authoritative pre-spawn gate. Enriched runtime telemetry flows through the existing daemon report without a protocol-version bump. The legacy `AgentRole` remains the pane/delegation topology field and is not repurposed as the product-role abstraction.

**Tech Stack:** Rust, serde, `impulse-ops`, Dioxus Desktop host, portable PTY runtime, daemon terminal-ops telemetry.

## Global Constraints

- Preserve every existing launch payload: all new wire fields default to absent or empty.
- Keep `AgentRole::{Coordinator, Worker}` unchanged and document it as legacy delegation topology.
- Use an open string newtype for product roles and capability identifiers.
- Block only missing mandatory requirements; render optional gaps as degraded.
- Advertise enforcement conservatively. Working-directory control is mediated, not filesystem sandboxing.
- Do not reuse Ion's verification tool-name allowlist as a runtime capability model.
- Do not add a generalized runtime-adapter trait, daemon session lifecycle, artifact approval, or completion workflow in this slice.
- Follow strict RED-GREEN-REFACTOR and record the failing-test command/output before production changes.
- Stage exact files only; never use `git add .` or `git add -A`.

---

### Task 1: Define the product-role compatibility contract

**Files:**
- Add: `impulse-rs/impulse-ops/src/role_assignment.rs`
- Modify: `impulse-rs/impulse-ops/src/lib.rs`
- Test: inline unit tests in `role_assignment.rs`

**Interfaces:**
- Produces: `AgentRoleId`, `RuntimeCapabilityId`, `EnforcementStrength`, `RuntimeCapabilitySupport`, `RoleCapabilityRequirement`, `AgentRoleAssignment`, `CapabilityCompatibility`, `RoleCompatibility`
- Produces: a pure evaluator over a canonical platform ID, declared support, and an assignment
- Preserves: `AgentRole` and existing `AgentRuntime` JSON

- [x] Write failing serde, validation, ordering, mandatory-block, and optional-degradation tests.
- [x] Run `cargo test -p impulse-ops role_assignment -- --nocapture` and record the expected RED failure.
- [x] Implement validated open-string IDs and the minimal compatibility evaluator.
- [x] Add optional assignment/compatibility fields to `AgentRuntime` with serde defaults and omission.
- [x] Prove old `AgentRuntime` JSON still deserializes and new role/capability strings round-trip.
- [x] Run the focused tests to GREEN, then refactor without broadening behavior.

### Task 2: Declare conservative runtime launch capabilities

**Files:**
- Modify: `impulse-rs/impulse-ops/src/agent_registry.rs`
- Test: existing registry test module

**Interfaces:**
- Consumes: `AgentDescriptor`, custom registry TOML, canonical aliases
- Produces: optional `runtime_capabilities` metadata and canonical compatibility results

- [x] Write failing tests proving legacy TOML defaults to no capabilities and aliases return the canonical platform ID.
- [x] Run `cargo test -p impulse-ops agent_registry -- --nocapture` and record RED.
- [x] Add capability metadata with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- [x] Mark built-in desktop workspace targeting and process lifecycle as `mediated`.
- [x] Keep scoped filesystem enforcement `unsupported`; advertise structured tools only where the live runtime proves them.
- [x] Run role-assignment and registry tests to GREEN.

### Task 3: Gate PTY spawn and preserve task/compatibility telemetry

**Files:**
- Modify: `impulse-rs/impulse-desktop/src/runtime.rs`
- Modify: `impulse-rs/impulse-desktop/src/daemon_ops.rs`
- Modify: `impulse-rs/src/ops_workbench.rs`
- Test: inline runtime/daemon/workbench tests

**Interfaces:**
- Consumes: optional `task` and `role_assignment` on `AgentSpawnRequest`
- Produces: pre-spawn compatibility rejection, task/assignment/result in snapshots and daemon truth
- Preserves: legacy role-only and assignment-free launch behavior

- [x] Write failing tests for legacy payload parsing, mandatory incompatibility before agent-ID reservation, task/assignment snapshot preservation, and enriched telemetry conversion.
- [x] Run the focused package tests and record RED.
- [x] Evaluate role compatibility after platform canonicalization and before ID reservation or PTY spawn.
- [x] Store the explicit task and typed role contract in the runtime record and snapshot.
- [x] Inject non-secret role/task metadata into the child launch environment.
- [x] Preserve enriched fields through `TerminalOpsReport` and workbench overlay; older telemetry must not erase newer typed facts.
- [x] Run focused runtime, daemon-ops, and workbench tests to GREEN.

### Task 4: Make the Dioxus launcher role- and task-explicit

**Files:**
- Modify: `impulse-rs/impulse-desktop/src/ui.rs`
- Modify: `impulse-rs/impulse-desktop/src/views.rs` only if a reusable compatibility view is needed
- Modify: `impulse-rs/impulse-desktop/tests/desktop_contract.rs`
- Modify: `impulse-rs/impulse-desktop/tests/views_ssr.rs` if rendered output changes

**Interfaces:**
- Consumes: selected `AgentDescriptor.runtime_capabilities` and shared evaluator
- Produces: required task input, fixed initial Builder role assignment, compatibility preview, blocked/degraded/allowed copy
- Sends: no governed launch with an empty task or absent product-role assignment

- [x] Write failing source-contract/SSR tests for a required Task field, visible Builder role, enforcement labels, and a non-`None` role assignment payload.
- [x] Run the focused desktop tests and record RED.
- [x] Add the task field and a conservative Builder requirement profile: mediated workspace target and process lifecycle are mandatory; structural filesystem scope is optional and therefore visibly degraded today.
- [x] Disable launch for empty task or a failed mandatory compatibility check.
- [x] Render each compatibility check with its required and available enforcement strength; never call cwd mediation a sandbox.
- [x] Ensure the runtime remains the authoritative repeated gate even if the UI preview is stale.
- [x] Run focused desktop tests to GREEN.

### Task 5: Record the architecture boundary and verify the slice

**Files:**
- Add: `docs/decisions/0010-product-role-launch-contract.md`
- Modify: `docs/decisions/README.md`
- Modify: `docs/INDEX.md`
- Modify: `CONTEXT.md`
- Modify: `VISION.md` only to mark this exact foundation live without claiming the full governed loop

**Interfaces:**
- Records: product role versus legacy delegation topology, launch capability versus model-internal capability, UI preview versus backend gate, and the next daemon-owned lifecycle slice

- [x] Write ADR-0010 and link it from the decision and documentation indexes.
- [x] Update the glossary while keeping `CONTEXT.md` within its L0/L1 budget.
- [x] Mark only explicit task/role preflight and telemetry live; keep supervisor decisions, evidence, and verification in the Next slice.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo check --workspace`.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Run `cargo test --workspace` and record exact totals.
- [ ] Run `python3 docs/validate_docs.py --self-test` and `python3 docs/validate_docs.py --all`.
- [ ] Run `npm run dioxus:host:smoke`.
- [ ] Run `git diff --check` and a focused secret scan.
- [ ] Commit exact implementation/test files first, then exact ADR/documentation files if review indicates a clean split.

## Review and Integration

- [ ] Generate an SDD review package from base `92264fcccfad6391645be174f3ff0fde4f37eecb` to the implementation tip.
- [ ] Obtain a fresh task-spec review; fix every accepted finding and re-review.
- [ ] Obtain a fresh broad maintainability/security review; fix every accepted finding and re-review.
- [ ] Re-run the full verification gate after the final review fix.
- [ ] Cherry-pick reviewed commits onto `agent/control-plane-vision-integration`, push that PR branch, and verify PR #13 remains mergeable and green.
