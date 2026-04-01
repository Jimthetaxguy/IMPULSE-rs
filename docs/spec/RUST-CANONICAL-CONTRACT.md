---
title: Rust Canonical Product Contract
description: Authoritative product contract for Impulse based on impulse-rs
version: '1.5'
updated: 2026-04-01
type: specification
category: core
phase: all
status: active
audience: builders
tags: [contract, rust, canonical, roadmap]
authors:
  - name: James Pustorino
    role: Creator
    email: James.s.Pustorino@gmail.com
    github: jamespustorino
---

# Rust Canonical Product Contract

> **Canonical implementation:** `impulse-rs`
> **Contract policy:** If a document conflicts with this file, this file wins.

## 1) Product Purpose

Impulse is a terminal-native sidecar for AI coding agents that preserves session continuity across direct hook invocations and daemon/TUI workflows.

Core outcomes:
- Persistent project memory (`GENOME`, session history, active state)
- Cross-session continuity for Claude Code and OpenCode integrations
- Operationally safe session lifecycle with verification-before-completion gates
- Human-visible observability through CLI, TUI, and the EGUI operator workbench

## 2) Canonical Scope and Roadmap

### Roadmap Contract

| Stage | Focus | Status |
| --- | --- | --- |
| **Now** | Rust memory core + hooks + retrieval/injection + EGUI operator workbench | Active |
| **Next** | Daemon-truth EGUI integration + hook/compaction validation | Active |
| **Later** | Agent control + artifact polish + deeper coordination UX | Planned |

### Out of Scope for Current Contract
- Full SWARM semantic injection runtime
- Web UI or non-Rust dashboard surfaces
- Structural blocking before hook validation evidence exists

## 3) Public Interface Contract

### CLI Contract (Stable)

Primary commands that must remain documented and regression-tested:

**Session lifecycle:**
- `session-start`
- `session-end --verify`
- `track-write`
- `track-tool`

**Info and status:**
- `status`
- `history`
- `genome`
- `activity`
- `summary`
- `health`
- `system`
- `analyze`

**Retrieval and search:**
- `index-memory`
- `search-history`
- `search-genome`
- `retrieval-status`

**Orchestration and context:**
- `orchestrate`
- `handoff`
- `sync-context`
- `hooks`
- `verify`

**Stewardship:**
- `steward` (subcommands: status, analyze, compact, approve, reject)

**Tool and model management:**
- `tools` (subcommands: list, init, update)
- `docs` (subcommands: list, fetch)
- `model`
- `credentials` (subcommands: set, get, list, proxy)

**Utilities:**
- `calc`
- `exec`
- `run` (TUI mode)
- `config`

### State and Artifact Contract

| Path | Purpose | Notes |
| --- | --- | --- |
| `.impulse/LIVE_STATE.json` | Active sessions/files/tools | Ephemeral runtime state |
| `.impulse/HISTORY.jsonl` | Session history (append-only) | Durable project memory |
| `.impulse/GENOME.md` | Durable decisions/preferences | Durable project memory |
| `.impulse/config.json` | Runtime configuration | Durable config |
| `.impulse/context/current-task.md` | Shared current context | Generated via `sync-context` |
| `.impulse/context/handoff-*.md` | Tool handoff artifacts | Generated via `handoff` |
| `.impulse/context/routing-log.jsonl` | Orchestration log | Append-only audit |
| `.impulse/context/injections/injection-log.jsonl` | Injection audit log | Append-only staged/apply metadata |
| `.impulse/context/injections/inject-*.md` | Staged injection bundles | Review/apply artifacts |
| `.impulse/retrieval.db` | Retrieval cache/index database | Rebuildable cache (gitignored) |
| `.impulse/retrieval_index_state.json` | Retrieval index metadata/state | Rebuildable metadata |
| `.impulse/embeddings/*` | Optional embedding temp artifacts | Runtime cache (gitignored) |
| `.impulse/retrieval.lock` | Indexing lock guard | Runtime safety artifact |
| `.impulse/projects/<project_id>/agents/<agent_id>/artifacts/*` | Project-organized operator artifacts | Durable workbench artifacts |

### EGUI Workbench IPC Contract

The daemon is the authoritative source for the EGUI workbench surfaces:

- `Overview`
- `Agents`
- `Context`
- `Artifacts`
- sidebar operator alerts
- status bar workbench summary

Canonical snapshot request/response surfaces:

- `GetOpsSnapshot`
- `SubscribeOps`
- `ListArtifacts`
- `GetArtifact`
- `RunArtifactAction`

Telemetry publication surface:

- `PublishTerminalOps { report: TerminalOpsReport }`

Shared workbench model contract:

- `ProjectOpsSnapshot` is the canonical read model for the daemon-backed workbench.
- `TerminalOpsReport` is the ephemeral publication model for live terminal telemetry.
- `AgentRuntime.ephemeral = true` identifies telemetry-only agents that do not currently map to a durable session.
- `Memory` may continue to use dedicated history/genome/search IPC outside the snapshot model in the current phase.

`TerminalOpsReport` fields:

- `source_id`
- `published_at`
- `agents`
- `context`
- `interventions`

Daemon overlay rules:

- Build durable snapshot data first.
- Overlay fresh terminal telemetry by `session_id` first, then agent `id`.
- Expose unmatched telemetry as ephemeral agents.
- Mark telemetry stale after 10 seconds without heartbeat.
- Stop overlaying stale telemetry after 10 seconds.
- Purge telemetry-only state after 60 seconds.

### Daemon IPC Contract (PROTOCOL_VERSION = 2)

The daemon exposes a JSON-line Unix socket protocol (`impulse.sock`). Full spec: [`docs/IPC-PROTOCOL.md`](../IPC-PROTOCOL.md).

**Session & Tool Management:**
- `Ping`, `Status`
- `CreateSession { name, platform }`, `EndSession { session_id, summary }`
- `TrackFile { session_id, file_path }`, `TrackTool { session_id, tool_name }`
- `GetSession { session_id }`, `ListSessions`

**Agent System (Phase 3):**
- `AgentAssist { prompt, context?, insights? }` — coordination assistance with context enrichment. Response: `AgentAssistResult { success, response, recommendations, pane_summaries }`
- `AgentReviewCode { file_path, diff, insights? }` — code review. Response: `AgentSpecializedResult { success, response }`
- `AgentAnalyzeError { error_text, context, insights? }` — error analysis. Response: `AgentSpecializedResult { success, response }`
- `AgentSummarizePane { pane_id, raw_output?, insights? }` — pane summary. Response: `AgentSpecializedResult { success, response }`

**Delegation System (Phase 1B):**
- `RegisterDelegation { spec, coordinator_pane_id, context_snapshot? }` — track cross-agent delegation
- `CompleteDelegation { delegation_id, summary, tool_trace?, diff_summary? }` — mark delegation done
- `ListDelegations` — list all tracked delegations

**Conflict Resolution (Task 20):**
- `GetConflictHistory` — get conflict resolution history
- `ClearResolvedConflicts` — clear resolved conflicts

**Agent Pool (Phase 2B):**
- `GetAgentPool` — all sessions grouped by role

**Steward:**
- `StewardStatus`, `StewardMemory`, `StewardProposals { action, id? }`

**Guard:**
- `GuardEvaluate { target, action }`, `GuardList`

**Supervisor (EGUI control plane):**
- `GetSupervisorPermissions`, `SupervisorChat { prompt, context? }`, `RunSupervisorAction { action }`

**Tools:**
- `ListTools { category? }`, `DescribeTool { name }`, `InvokeTool { name, params? }`, `ToolSchema`

**EGUI Workbench:**
- `GetOpsSnapshot`, `SubscribeOps { since_seq? }`, `PublishTerminalOps { report: TerminalOpsReport }`
- `ListArtifacts { limit? }`, `GetArtifact { artifact_id }`, `RunArtifactAction { artifact_id, action_id, params? }`

**Chat:**
- `Chat { session_id, message, inject_mode?, inject_explain? }` — daemon-aware chat with context injection

### Retrieval Command Extensions (Additive)

- `search-history` / `search-genome`:
  - `--backend auto|sqlite-vec|rust-cosine|keyword`
  - `--explain`
- `retrieval-status`:
  - `--check`
  - `--json`
- Context injection overrides (additive):
  - `--daemon chat --inject-mode off|review|apply --inject-explain`
  - `orchestrate --inject-mode off|review|apply --inject-explain`
  - `handoff --inject-mode off|review|apply --inject-explain`
  - `sync-context --inject-mode off|review|apply --inject-explain`

### Context Injection Config Contract (Additive)

- `context_injection_mode`: `off|review|apply` (default `review`)
- `context_injection_scope`: `daemon|direct|both` (default `both`)
- `context_injection_max_items`: integer (default `5`)
- `context_injection_max_chars`: integer (default `2000`)
- `context_injection_min_score`: float `0.0..1.0` (default `0.60`)
- `context_injection_use_semantic`: bool (default `true`)
- `context_injection_emit_artifacts`: bool (default `true`)

## 4) Capability Matrix

Test column shows current coverage level: **Full** (≥3.0/KLOC), **Partial** (1.0-3.0/KLOC), **Minimal** (<1.0/KLOC).

| Capability | Status | Interface | Tests |
| --- | --- | --- | --- |
| Session lifecycle tracking | Implemented | `session-start`, `session-end` | Rust unit + integration |
| File/tool activity tracking | Implemented | `track-write`, `track-tool` | Rust unit + integration |
| TUI tabs (dashboard/session/history/etc.) | Implemented | `run` TUI mode | Rust UI tests |
| Daemon socket operations | Implemented | `daemon`, `--daemon ...` | Daemon tests |
| Context-aware chat (daemon) | Implemented | `--daemon chat` | Daemon + provider tests |
| Hook config generation | Implemented | `hooks --platform ...` | Integration tests |
| Orchestration handoff/context files | Implemented | `orchestrate`, `handoff`, `sync-context` | Rust tests |
| Verification gate | Implemented | `verify`, `session-end --verify` | Rust tests |
| Agent harness (10 features) | Implemented (2026-03-31) | `build_context_prompt`, `query_with_context`, intent classification, `CoordinationResult`, conflict history IPC, JSON harness protocol, session awareness, specialized IPC endpoints | Rust tests |
| Retrieval indexing + keyword search | Implemented | `index-memory`, `search-history --mode keyword`, `search-genome --mode keyword` | Rust unit + integration |
| Semantic search (feature-flagged) | Implemented (fallback-safe) | `search-* --mode semantic` with keyword fallback | Rust unit + integration |
| Retrieval health diagnostics | Implemented | `retrieval-status --check --json` | Rust integration |
| Retrieval explainability metadata | Implemented | `search-* --explain` + JSON metadata (`backend_used`, `fallback_code`, `timing_ms`) | Rust integration |
| Review-first context injection | Implemented (additive) | daemon chat + `orchestrate`/`handoff`/`sync-context` with `--inject-mode` | Rust unit + integration |
| Injection staging artifacts | Implemented | `.impulse/context/injections/*` | Rust unit + integration |
| EGUI operator workbench | Implemented (daemon snapshot + telemetry overlay) | `impulse-gui` | Rust unit + workspace checks |
| Context stewardship | Implemented | `steward` (status/analyze/compact/approve/reject) | Rust unit + integration |
| Token tracking algorithm | Implemented | Internal metrics for compaction measurement | Rust unit + integration |
| Tool management | Implemented | `tools` (list/init/update) | Rust unit |
| Credential management | Implemented | `credentials` (set/get/list/proxy) | Rust unit |
| Documentation fetcher | Implemented | `docs` (list/fetch) | Rust unit |
| System utilities | Implemented | `calc`, `exec`, `system`, `health` | Rust unit |
| SWARM semantic coordination runtime | Planned | Future orchestration engine | Not started |

## 5) Claude/OpenCode Parity Contract

| Area | Claude Code | OpenCode | Contract Expectation |
| --- | --- | --- | --- |
| Session lifecycle hooks | Generated config | Generated config | Equivalent coverage |
| File write tracking | Supported | Supported | Equivalent behavior |
| Tool tracking | Supported | Supported | Equivalent behavior |
| Session end verification (`--verify`) | Included in generated hook command | Included in generated hook command | Required |
| Context handoff artifacts | Shared `.impulse/context/*` | Shared `.impulse/context/*` | Required |
| Known deltas | Hook event payload shape differs by platform | Hook event payload shape differs by platform | Handle by adapter mapping, not by feature removal |

## 6) Governance and Ownership

### Contract Ownership

The following files define product truth and must be updated together for contract changes:
- `docs/spec/RUST-CANONICAL-CONTRACT.md` (authoritative contract)
- `AGENTS.md` (operator-facing guidance)
- `CLAUDE.md` (project technical context)
- `docs/INDEX.md` (navigation + source-of-truth routing)
- `docs/SUMMARY.yaml` (navigation source)
- `docs/SUMMARY.md` (high-level map)

### Required Update Checklist for Any Interface Change

When adding/changing CLI commands, hooks, state files, or roadmap stage definitions:
1. Update this contract doc.
2. Update command/state references in `AGENTS.md` and `CLAUDE.md`.
3. Update `docs/INDEX.md`, `docs/SUMMARY.yaml`, and `docs/SUMMARY.md`.
4. Run `python3 docs/validate_docs.py --contract`.
5. Include release note fields from `docs/guides/RELEASE-NOTES-TEMPLATE.md`.

## 7) Test Quality Contract

### Test Density Targets

| Module Category | Target (tests/KLOC) | Current (2026-04-01) | Gap |
|----------------|---------------------|----------------------|-----|
| Core (state, daemon, agent) | ≥3.0 | ~1.5 (state ~80, agent +24, daemon +2) | HIGH — need ~2x more |
| Handlers | ≥2.0 | ~0.8 (38 tests in 6/19 files; 13 files at zero) | CRITICAL — 68% untested |
| UI/TUI | ≥1.0 | ~0.4 | MEDIUM — layout/rendering |
| Tooling | ≥2.0 | ~17.1 (84 tests, 4,920 LOC) | MET — well above target |
| Integration | Every stable CLI command | 26 tests | PARTIAL — expanding |

**Workspace totals (2026-04-01):** 79,194 LOC, 1,025 tests (999+26 impulse-rs, 4 ops, 90 term, 220 gui), 227 .rs files across 4 crates.

### Required Test Patterns

New code must include:

| Pattern | Requirement |
|---------|-------------|
| Happy path | Every public function has at least one correctness test |
| Error path | Every `Result<T>`-returning function has at least one `Err` test |
| Serde round-trip | Every `Serialize + Deserialize` type: `deserialize(serialize(val)) == val` |
| Error Display | Every `thiserror` enum: `assert!(format!("{e}").contains("expected"))` |
| Boundary conditions | Empty inputs, zero/max values where applicable |

### Quality Floor

- Tests must contain assertions (`assert!`, `assert_eq!`, `assert_ne!`). `println!`-only tests are not acceptable.
- Test names: `test_<function>_<scenario>_<expected_result>`, not `test_parse_2`.
- `unwrap()` in tests must be on operations expected to succeed; use `assert!(result.is_err())` for expected failures.

### How to Meet Density Targets

- 1 happy-path test per public function (establishes baseline)
- 1 Err-path test per `Result`-returning function
- Boundary condition tests where type allows (empty, zero, max)
- Serde round-trip test for every `Serialize + Deserialize` type
- Display test for every `thiserror` enum
- Property-based tests (`proptest`) for combinatorial input spaces (path validation, config parsing, serialization)

### Test Helper Centralization

| Helper Type | Location |
|---|---|
| State factories | `#[cfg(test)]` in owning module |
| Mock tools | `src/tooling/` test module |
| Daemon guards | `src/integration_tests.rs` |
| Assertion helpers | Near first usage; extract if 3+ modules use |

Rule: helpers used by 3+ modules must be extracted to a shared `#[cfg(test)]` module.

### Unsafe Code

Any `unsafe` block requires all three: (1) `// SAFETY:` comment documenting every invariant, (2) precondition validation **before** the unsafe block (never inside), (3) a dedicated test exercising the unsafe code path. Never use `unsafe` for convenience or to avoid `Result`.

### Lint Suppression

- `#[allow(dead_code)]` requires `// dead_code: <reason>` comment and `grep` proof of no callers. If truly dead, delete it.
- `#![allow(...)]` (file-level) is not acceptable in new code — must be broken into per-item allows.
- All `#[allow(clippy::*)]` require `// clippy: <reason>` comment.
- `#[allow(clippy::too_many_arguments)]` is temporary only — must include `// TODO: refactor to struct params`.

### Verification Gate

All changes must pass before commit:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```
Exit on first failure. Do not skip or bypass any step.

**Expected outputs (update when counts change):**
- `cargo test`: 1,025 passed, 3 ignored, 0 failed (5 test result lines)
- `cargo clippy`: 0 warnings
- `cargo fmt --check`: no output

### Policy Compliance Audit Commands

Run periodically to detect policy drift:

```bash
# 1. Find bare ? on I/O (should have .context())
git grep -n "fs::read\|fs::write\|fs::remove" -- "*.rs" | grep -v "context\|test\|// "

# 2. Find unwrap() outside tests/main (violation of Principle 1)
git grep -n "\.unwrap()" -- "*.rs" | grep -v "#\[test\]\|mod tests\|fn main\|impl Default\|// unwrap:"

# 3. Find #[allow] missing required comments
git grep -n "#\[allow" -- "*.rs" | grep -v "// dead_code:\|// TODO:\|// clippy:\|// serde:\|cfg_attr"

# 4. Find Serialize+Deserialize types missing round-trip tests
# Compare: types declaring derive vs files with round-trip tests
git grep -c "Serialize.*Deserialize\|Deserialize.*Serialize" -- "*.rs"
git grep -c "round_trip\|roundtrip" -- "*.rs"

# 5. Find handler files without mod tests
for f in src/handlers/*.rs; do grep -qL "mod tests" "$f" && echo "UNTESTED: $f"; done

# 6. Verify test count hasn't regressed
cargo test 2>&1 | grep "test result:" | awk '{sum += $4} END {print "Total: " sum " (expected: 1025)"}'
```

### egui Import Convention

`impulse-gui` uses `eframe::egui::*` — never bare `egui::*`. The crate re-exports egui through eframe.

## 8) Validation and Drift Prevention

Documentation contract validation command:

```bash
python3 docs/validate_docs.py --contract
```

This command must fail on:
- Missing canonical references in source-of-truth docs
- Contradictory active-doc claims (for example, active docs claiming TypeScript/Bun-only core)
- Missing roadmap contract markers in key top-level docs
