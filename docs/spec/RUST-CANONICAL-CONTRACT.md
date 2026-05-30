---
title: Rust Canonical Product Contract
description: Authoritative product contract for Impulse based on impulse-rs
version: '1.7'
updated: 2026-05-22
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
- Cross-session continuity for Claude Code and Codex integrations, with legacy OpenCode compatibility preserved where already implemented
- Operationally safe session lifecycle with verification-before-completion gates
- Human-visible observability through CLI, ratatui TUI, and a Tauri desktop shell (in migration)

## 2) Desktop Shell Contract (Updated 2026-04-15)

> **This section supersedes all prior references to the EGUI workbench as the active desktop product.**

### Chosen Desktop Stack

| Layer | Technology |
|---|---|
| Desktop container | Tauri 2.x |
| UI framework | Dioxus (rsx! components, inside Tauri webview) |
| Terminal rendering | xterm.js (mounted via Dioxus eval(), fed by Tauri events) |
| PTY / session backend | impulse-term (TerminalBackend, WriteQueue) — unchanged |
| Standalone operator TUI | ratatui — first-class, preserved throughout migration |
| **Legacy desktop surface** | **egui / impulse-gui — FROZEN. No new features. Sunset after parity.** |

Architectural detail: `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
Stack tradeoffs: `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
Decision record: `docs/decisions/0007-desktop-shell-stack.md`
Migration sequence: `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`

### Desktop Migration Phases

| Phase | Goal | Status |
|---|---|---|
| 0 | Documentation contract reset | Completed; Plan 6 is correcting residual active-doc drift |
| 1 | Remove eframe from impulse-term, confirm framework-neutral core | Pending |
| 2 | Static Tauri+Dioxus shell skeleton | Partial: `impulse-desktop` Dioxus shell + typed bridge scaffold |
| 3 | Live terminal bridge (PTY → xterm.js) | Pending |
| 4 | Daemon integration and parity | Pending; `impulse-gui` is already frozen for new features |

## 3) Canonical Scope and Roadmap

### Roadmap Contract

| Stage | Focus | Status |
| --- | --- | --- |
| **Now** | Rust memory core + hooks + retrieval/injection + Tauri desktop shell | Active |
| **Next** | terminal bridge hardening + daemon-backed desktop parity | Active |
| **Later** | Daemon parity in desktop shell + agent control + artifact polish | Planned |

### Out of Scope for Current Contract
- Full SWARM semantic injection runtime
- Web UI or non-Rust dashboard surfaces
- Structural blocking before hook validation evidence exists
- New egui features (egui is legacy/frozen)

## 4) Public Interface Contract

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

**Build hygiene:**
- `sweep` — clean build artifacts
- `wipe` — deep clean including caches
- `clean-all` — comprehensive workspace cleanup
- `sccache-setup` — configure shared compilation cache
- `build-health` — report build system health metrics

**Dynamic tooling:**
- `tooling-list` — list registered dynamic tools
- `tooling-describe` — describe a specific tool's schema and capabilities
- `tooling-run` — execute a dynamic tool by name with parameters

**Supervisor:**
- `panes` — list active terminal panes with context summaries

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

### Desktop Shell IPC Contract (Terminal Bridge — Additive)

The desktop shell communicates with the Rust backend via a thin Tauri IPC bridge. These surfaces are **additive** — they do not replace or modify daemon contracts.

**Commands (frontend → backend):**

| Command | Description |
|---|---|
| `terminal_open` | Spawn a PTY session, return session_id |
| `terminal_write` | Write bytes to PTY stdin |
| `terminal_resize` | Resize PTY and vt100 parser |
| `terminal_close` | Kill PTY and clean up session |
| `terminal_focus` | Notify backend of focus change |
| `native_island_request` | Request a narrow macOS-native island and receive a serializable result DTO |

**Events (backend → frontend):**

| Event | Description |
|---|---|
| `terminal_output` | PTY stdout bytes |
| `terminal_exit` | PTY child exited |
| `terminal_status` | Status change |
| `ops_update` | Daemon ProjectOpsSnapshot update |
| native island result | Returned through `native_island_request`; native code does not own session, memory, terminal, or artifact state |

### Daemon Workbench IPC Contract (Preserved — Unchanged)

The daemon is the authoritative source for workbench surfaces:

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
- `AgentAssist { prompt, context?, insights? }` — coordination assistance with context enrichment.
- `AgentReviewCode { file_path, diff, insights? }` — code review.
- `AgentAnalyzeError { error_text, context, insights? }` — error analysis.
- `AgentSummarizePane { pane_id, raw_output?, insights? }` — pane summary.

**Delegation System (Phase 1B):**
- `RegisterDelegation { spec, coordinator_pane_id, context_snapshot? }`
- `CompleteDelegation { delegation_id, summary, tool_trace?, diff_summary? }`
- `ListDelegations`

**Conflict Resolution (Task 20):**
- `GetConflictHistory`, `ClearResolvedConflicts`

**Agent Pool (Phase 2B):**
- `GetAgentPool`

**Steward:**
- `StewardStatus`, `StewardMemory`, `StewardProposals { action, id? }`

**Guard:**
- `GuardEvaluate { target, action }`, `GuardList`

**Supervisor (desktop control plane):**
- `GetSupervisorPermissions`, `SupervisorChat { prompt, context? }`, `RunSupervisorAction { action }`

**Tools:**
- `ListTools { category? }`, `DescribeTool { name }`, `InvokeTool { name, params? }`, `ToolSchema`

**Workbench:**
- `GetOpsSnapshot`, `SubscribeOps { since_seq? }`, `PublishTerminalOps { report: TerminalOpsReport }`
- `ListArtifacts { limit? }`, `GetArtifact { artifact_id }`, `RunArtifactAction { artifact_id, action_id, params? }`

**Chat:**
- `Chat { session_id, message, inject_mode?, inject_explain? }`

## 5) Capability Matrix

| Capability | Status | Interface | Tests |
| --- | --- | --- | --- |
| Session lifecycle tracking | Implemented | `session-start`, `session-end` | Rust unit + integration |
| File/tool activity tracking | Implemented | `track-write`, `track-tool` | Rust unit + integration |
| ratatui TUI tabs | Implemented | `run` TUI mode | Rust UI tests |
| Daemon socket operations | Implemented | `daemon`, `--daemon ...` | Daemon tests |
| Context-aware chat (daemon) | Implemented | `--daemon chat` | Daemon + provider tests |
| Hook config generation | Implemented | `hooks --platform ...` | Integration tests |
| Orchestration handoff/context files | Implemented | `orchestrate`, `handoff`, `sync-context` | Rust tests |
| Verification gate | Implemented | `verify`, `session-end --verify` | Rust tests |
| Agent harness | Implemented (2026-03-31) | Multiple IPC endpoints | Rust tests |
| Retrieval indexing + keyword search | Implemented | `index-memory`, `search-history`, `search-genome` | Rust unit + integration |
| Semantic search (feature-flagged) | Implemented (fallback-safe) | `search-* --mode semantic` | Rust unit + integration |
| Review-first context injection | Implemented (additive) | daemon chat + orchestrate/handoff/sync-context | Rust unit + integration |
| Context stewardship | Implemented | `steward` | Rust unit + integration |
| Tool management | Implemented | `tools` | Rust unit |
| Credential management | Implemented | `credentials` | Rust unit |
| **Tauri desktop shell** | **In migration; scaffold partial** | Tauri + Dioxus + xterm.js | Pending live bridge + daemon parity |
| **egui operator workbench** | **LEGACY — frozen** | `impulse-gui` (compile-only) | Legacy tests only |
| SWARM semantic coordination runtime | Planned | Future orchestration engine | Not started |

## 6) Primary Platform And Legacy Compatibility Contract

Claude Code and Codex are the current primary coding-agent platforms for active Impulse work. OpenCode support is legacy compatibility: preserve existing behavior unless a removal plan explicitly owns migration risk, but do not treat OpenCode as a peer platform for new roadmap work.

| Area | Claude Code | Codex | OpenCode legacy compatibility | Contract Expectation |
| --- | --- | --- | --- | --- |
| Session lifecycle hooks | Primary | Primary where implemented | Preserve existing generated config | Primary coverage for active platforms; no new OpenCode parity requirement |
| File write tracking | Supported | Supported where implemented | Preserve existing behavior | Equivalent active-platform behavior |
| Tool tracking | Supported | Supported where implemented | Preserve existing behavior | Equivalent active-platform behavior |
| Session end verification (`--verify`) | Required | Required where hook integration exists | Preserve existing generated command | Verification remains required for active platforms |
| Context handoff artifacts | Shared `.impulse/context/*` | Shared `.impulse/context/*` | Shared `.impulse/context/*` | Handoff artifacts stay platform-neutral |

## 7) Governance and Ownership

### Contract Ownership

The following files define product truth and must be updated together for contract changes:
- `docs/spec/RUST-CANONICAL-CONTRACT.md` (authoritative contract)
- `AGENTS.md` (operator-facing guidance)
- `CLAUDE.md` (project technical context)
- `docs/INDEX.md` (navigation + source-of-truth routing)
- `docs/SUMMARY.yaml` (navigation source)
- `docs/SUMMARY.md` (high-level map)
- `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` (desktop contract)
- `docs/decisions/0007-desktop-shell-stack.md` (desktop ADR)

### Required Update Checklist for Any Interface Change

When adding/changing CLI commands, hooks, state files, or roadmap stage definitions:
1. Update this contract doc.
2. Update command/state references in `AGENTS.md` and `CLAUDE.md`.
3. Update `docs/INDEX.md`, `docs/SUMMARY.yaml`, and `docs/SUMMARY.md`.
4. Run `python3 docs/validate_docs.py --contract`.
5. Include release note fields from `docs/guides/RELEASE-NOTES-TEMPLATE.md`.

## 8) Test Quality Contract

### Test Density Targets

| Module Category | Target (tests/KLOC) | Current (2026-04-04) | Gap |
|----------------|---------------------|----------------------|-----|
| Core (state, daemon, agent) | ≥3.0 | ~1.5 | HIGH |
| Handlers | ≥2.0 | ~2.5 | IMPROVING |
| UI/TUI | ≥1.0 | ~0.4 | MEDIUM |
| Tooling | ≥2.0 | ~17.1 | MET |
| Integration | Every stable CLI command | 26 tests | PARTIAL |

**Workspace totals (2026-04-04):** ~111K LOC, 1,344 tests, 237 .rs files across 4 crates.

### Required Test Patterns

| Pattern | Requirement |
|---------|-------------|
| Happy path | Every public function has at least one correctness test |
| Error path | Every `Result<T>`-returning function has at least one `Err` test |
| Serde round-trip | Every `Serialize + Deserialize` type: `deserialize(serialize(val)) == val` |
| Error Display | Every `thiserror` enum: `assert!(format!("{e}").contains("expected"))` |
| Boundary conditions | Empty inputs, zero/max values where applicable |

### Verification Gate

All changes must pass before commit:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

## 9) Validation and Drift Prevention

Documentation contract validation command:

```bash
python3 docs/validate_docs.py --contract
```

This command must fail on:
- Missing canonical references in source-of-truth docs
- Contradictory active-doc claims
- Missing roadmap contract markers in key top-level docs
- Any doc describing egui as the active or target desktop surface
