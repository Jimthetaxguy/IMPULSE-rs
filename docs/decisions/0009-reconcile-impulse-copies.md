---
title: "ADR-0009: Reconcile duplicate Impulse codebases (active vs .clean)"
status: accepted
created: 2026-06-25
deciders: [Impulse Maintainers]
---

# ADR-0009: Reconcile duplicate Impulse codebases (active vs .clean)

## Status

Accepted for this goal. Active checkout (`impulse-rs/`) is the single canonical tree. The `.clean` sibling checkout was relocated (mv) into the maintainer's local canonical-tree archive (`_archived-IMPULSE-rs.clean-...` + `-source`); the original path is no longer a live checkout. Those gitignored snapshots are optional historical provenance, not source or test fixtures.

## Context

The objective required working on Impulse (Dioxus + built-in tools to augment coding agents; Impulse Agent as always-on tech lead managing/monitoring/augmenting terminal CLI-TUI agents such as claude-code, codex, cursor etc.; UI for cycling multiple project workspaces/folders + agents per space; type-safe Rust plugins/tools as built-in augmentation; subagents/workflows to reduce machine load) and explicitly called out the existence of two trees:

- `/Users/jamespustorino/code/IMPULSE-rs` (active development, current branch agent/codex-dioxus-host-goal-cleanup)
- `/Users/jamespustorino/code/IMPULSE-rs.clean` (separate HEAD f78cc0c on clean/dioxus-pty-orchestrator)

`git worktree list` at start surfaced both. `git status` showed clean-ish state (untracked CONTEXT.md from prior session on lane).

Inspection showed:
- Active tree contains mature, tested multi-agent support in `impulse-ops/src/agent_registry.rs` (AgentRegistry, AgentDescriptor, builtin with claude-code + codex + cursor + gemini + opencode + shell, TOML override, resolve/detect/list/merge, many unit tests exercising happy+error paths).
- Active tree contains multi-workspace (project folder) support inside `impulse-desktop/src/workspace.rs` + `runtime.rs` (WorkspaceRegistry, WorkspaceTarget, WorkspaceEntry, register/list/touch/unregister, defaults, MCP exposure for list_workspaces, tests).
- Desktop host (Dioxus) already wires workspaces + agent spawns via PTY for terminal agents "logged in".
- `.clean` contains an experimental re-architecture under `clean/crates/`:
  - `impulse-contracts`: pure data (no I/O) — typed IDs (WorkspaceId "ws_...", SessionId etc.), WorkspacePath/Handle/Summary, harness BackendDescriptor/BackendRegistry, session phases, tool specs.
  - `impulse-workspace`: the registry impl using the contracts types (register, list, touch, find_by_path, concurrency-safe).
  - `impulse-runtime`, `impulse-mcp`.
- Drift exists because clean/ captured a "clean contracts-first layering" attempt during pty-orchestrator exploration; active tree pragmatically embedded workspace concepts directly in the desktop crate while advancing the Dioxus host pivot (see ADR-0008) and kept shared agent facts in impulse-ops.
- Different git histories; forcing structural merge would risk conflict with in-flight dioxus work and violate "dont overbuild".

Per plan non-goals: no GUI changes, no new subagent impls, no physical removal of .clean, no full PTY runs.

## Decision

1. **Canonical tree**: The active `/.../IMPULSE-rs` checkout (and its `impulse-rs/` Cargo workspace) is the single source for continued development. No parallel maintenance of the .clean copy is required.

2. **Reconciliation mechanism**: Documentation + decision record only (minimal diff). 
   - Record cause, mapping, and decision here.
   - No wholesale crate extraction or moves from clean/ into active (would be overbuild on current dioxus host lane).
   - Pure type ideas from clean (WorkspaceHandle etc.) are noted as future layering candidates but remain aspirational; current pragmatic implementations (desktop Workspace* + ops Agent*) already deliver the required observables.

3. **Mapping of concepts**:
   - clean's BackendDescriptor / harness agents → active's AgentDescriptor + AgentRegistry (more complete, tested, extensible via TOML).
   - clean's WorkspaceHandle/Id/Path + WorkspaceRegistry → active's WorkspaceTarget + WorkspaceEntry + WorkspaceRegistry (desktop) + usage in MCP / host for cycling project spaces.
   - Orchestration / supervisor concepts exist in both; active's src/orchestration + src/agent + daemon provide the coordination surface.

4. **Observability for goal AC2**: Made real by wiring:
   - `impulse-ops::AgentRegistry` (with claude-code, codex, ...) now called from desktop `runtime::spawn_agent` (for command/label resolution) and `mcp.rs` (ListAgentPlatformsTool + explicit get in AgentSpawnTool). Live list + available platforms both observable.
   - Multi-workspace via desktop `WorkspaceRegistry`.
   - Direct unit tests + captured --nocapture output + MCP tools exercise registration + launch descriptors on real paths.

5. **Docs update responsibility**: Living docs (CONTEXT, README, HANDBOOK ImpulseAgent section, RUST-CANONICAL-CONTRACT) will be updated in the same change set to embed the objective's vision language (always-on tech lead managing terminal agents that "login", UI picker for multiple project spaces + agents per space, Rust type-safe built-in augmentation/plugins, subagents/workflows for load reduction).

6. **.clean disposition and proof boundary**: The sibling checkout was relocated (mv) into the maintainer's local canonical-tree archive as `_archived-IMPULSE-rs.clean-YYYY...` (full source tree including `.git` and `clean/crates/`). The original sibling path is no longer active, and no further writes target it. The ignored archive and diagnostic captures remain optional historical provenance; builds and tests never depend on them. Tracked executable proof is `agent_registry::tests::test_reconciled_backend_contract_lives_in_canonical_registry`, which verifies the reconciled typed identity, alias resolution, launch command, and control-plane reporting in the active implementation. This keeps the single-source decision verifiable in fresh clones, linked worktrees, and CI.

## Consequences

- Single canonical tree simplifies maintenance.
- Default workspace tests are portable because they validate tracked canonical behavior rather than maintainer-local archive contents.
- Existing registries + tests already satisfy "observable registration of multiple distinct workspaces and ... agent CLI types".
- Vision codified without new runtime surface (per non-goals).
- Future work that wants the clean/ contracts split will start from this ADR and a new lane (will need to address shared-file rules).
- Verification gate + entrypoint runs + evidence capture remain mandatory before goal close.

## References

- Goal plan: the session goal/plan.md (acceptance, verification, checklist).
- Work card: docs/plans/worktrees/2026-06-25-reconcile-dupe-vision.md
- Related: ADR-0008 (Dioxus Desktop Host), docs/spec/DESKTOP-SHELL-ARCHITECTURE.md, impulse-ops agent_registry, impulse-desktop workspace/runtime, COLLABORATIVE-AGENTIC-CODING.md.
