---
title: Rust Canonical Product Contract
description: Authoritative product contract for Impulse based on impulse-rs
version: '1.8'
updated: 2026-07-12
type: specification
category: core
phase: all
status: active
audience: builders
tags: [contract, rust, canonical, roadmap]
authors:
  - name: Impulse Maintainers
    role: Maintainer
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---

# Rust Canonical Product Contract

> **Canonical implementation:** `impulse-rs`
> **Contract policy:** If a document conflicts with this file, this file wins.
> **Product north star:** [`../../VISION.md`](../../VISION.md) defines intent and target state;
> this contract distinguishes what the current implementation actually guarantees.

## 1) Product Purpose

Impulse is a terminal-native **local control plane and harness manager** for AI
software-engineering agents. It launches and manages coding runtimes, supervises their operating
conditions, and provides shared memory, tools, telemetry, messaging/handoffs, policy, credentials,
artifacts, and verification. Claude Code, Codex, and similar CLIs retain their proprietary internal
loops; Ion is the Impulse-native direct-provider/tool-loop runtime.

The Dioxus application, ratatui TUI, and CLI are operator surfaces. They project and command the
system; the Rust daemon, shared `impulse-ops` models, runtime/PTY state, and scoped persistence are
authoritative. Impulse does not claim full structural control over unsupported internals of an
external runtime.

Core outcomes:
- Governed launch, working-directory/project scoping, and PTY/process lifecycle for coding-agent runtimes; structural filesystem isolation depends on runtime/sandbox support
- Daemon-owned, inspectable workbench truth for agents, context, interventions, and artifacts
- Persistent project memory (`GENOME`, session history, active state) with review-first injection
- Cross-session continuity for Claude Code and Codex integrations, with legacy OpenCode compatibility preserved where already implemented
- Capability-checked platform tools and supervisor-specific permission/confirmation policy
- Operationally safe session lifecycle with recorded endings and optional API-level verification gates
- Human-visible observability through CLI, ratatui TUI, and the Dioxus Desktop cockpit
- Multi-workspace + multi-agent orchestration surface (observable registration and launch/monitoring of agents across project spaces)

## 2) Desktop Shell Contract (Updated 2026-07-12)

> **This section supersedes all prior references to the EGUI workbench as the active desktop product.**

### Chosen Desktop Stack

| Layer | Technology |
|---|---|
| Desktop host | Dioxus Desktop |
| UI framework | Dioxus (rsx! components, signals, host adapter) |
| Terminal rendering | xterm.js (mounted via Dioxus eval(), fed by host events) |
| PTY / session backend | `impulse-term` (`TerminalBackend`, parser, `WriteQueue`) plus desktop runtime lifecycle |
| Standalone operator TUI | ratatui — first-class, preserved throughout migration |
| **Legacy desktop surface** | **egui / impulse-gui — FROZEN. No new features. Sunset after parity.** |
| **Legacy host adapter** | **Tauri-shaped command/event bridge — compatibility only, not the next product scaffold.** |

Architectural detail: `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
Stack tradeoffs: `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
Decision record: `docs/decisions/0008-dioxus-desktop-host.md`
Historical migration sequence: `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`

### Desktop Migration Phases

| Phase | Goal | Status |
|---|---|---|
| 0 | Documentation contract reset | Completed; Plan 6 is correcting residual active-doc drift |
| 1 | Keep PTY/process lifecycle usable independently of render surface | Core backend live; optional egui renderer still retained for legacy compatibility |
| 2 | Static Dioxus shell skeleton | Complete foundation |
| 3 | Dioxus Desktop launch scaffold + live terminal bridge (PTY → xterm.js) | Live host/bridge foundation; operational hardening continues |
| 4 | Daemon-backed workbench truth | Live snapshot/telemetry foundation; multi-workspace routing remains incomplete |

## 3) Canonical Scope and Roadmap

### Roadmap Contract

| Stage | Focus | Status |
| --- | --- | --- |
| **Now** | Rust control-plane foundation: daemon truth, PTY lifecycle, memory/tools/policy/artifacts, Dioxus cockpit, Ion native runtime | Active |
| **Next** | Preserve registry-backed runtime identity, prove one governed supervisor + builder vertical slice, and define hierarchy/enforcement ADR | Active |
| **Later** | General role contracts, runtime capability negotiation, typed agent messaging, and multi-project supervisor attention | Planned |
| **Legacy** | egui / `impulse-gui` compile-maintenance only | Frozen |

### Out of Scope for Current Contract
- Full SWARM semantic injection runtime
- Web UI or non-Rust dashboard surfaces
- Claims that every external runtime is structurally governed or capability-equivalent
- A generalized role/runtime schema before the hierarchy and enforcement ADR lands
- New egui features (egui is legacy/frozen)

### Control-Plane Object Model

| Concept | Contract meaning | Current implementation boundary |
| --- | --- | --- |
| Role | Obligations, permissions, tools, context, communication, and verification duties | Narrow coordinator/worker `AgentRole` plus concrete `SupervisorPermissionPolicy`; general role contract is not implemented |
| Runtime | External harness or native engine that executes a role | External agent harness calls, desktop PTY runtime, and Ion native provider/tool loop; no common adapter trait yet |
| Agent instance | One running identity with runtime, role, scope, process state, and telemetry | `AgentRuntime`/`AgentRuntimeSnapshot` and desktop runtime records |
| Session | Bounded persisted work history | Daemon session lifecycle and `.impulse/` history/state |
| Task | Assignment plus acceptance/verification criteria | Delegation/current-task/Ion harness carriers; not yet one canonical task model |
| Pane | Cockpit view/input attachment | TUI panes and desktop terminal ids; never an authority boundary |
| Workspace target | Explicit filesystem execution root | Desktop `WorkspaceTarget`/`WorkspaceRegistry` |
| Project | Governance scope for memory, artifacts, policy, and verification | Project-scoped `.impulse/` state and `ProjectOpsSnapshot`; often maps 1:1 to a workspace today |

### Platform Service Contract

Memory/retrieval, tools, telemetry, messaging/handoffs, policy, credentials, artifacts, and
verification are peer control-plane services. A runtime may receive them through native typed calls,
MCP, hooks, sockets, files, generated commands, or mediated PTY operations. Similar conceptual
capabilities do not imply identical enforcement.

The pending local aggregate makes desktop platform identity registry-backed, carries that catalog
through MCP/host/runtime/snapshots, and launches Ion as a builtin platform. Until that aggregate is
integrated, treat it as preserved local implementation rather than remote-release truth.

A future adapter contract must report required, optional, emulated, and unsupported operations plus
enforcement strength. Mandatory role requirements must eventually block an incompatible launch;
advisory degradation must be visible. The schema is reserved for the hierarchy/enforcement ADR.

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

### Desktop Shell Host Contract (Terminal Bridge — Additive)

The desktop shell communicates with the Rust backend through a Dioxus host adapter command/event boundary. The remaining Tauri-shaped bridge is compatibility-only while Dioxus Desktop launch plumbing reaches parity. These surfaces are **additive** — they do not replace or modify daemon contracts.

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

### Daemon IPC Contract (PROTOCOL_VERSION = 3)

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
| PTY/process lifecycle | Implemented | `impulse-term::TerminalBackend`, desktop runtime | Rust unit + integration |
| Daemon workbench truth | Implemented foundation | `ProjectOpsSnapshot`, terminal telemetry overlay, workbench IPC | Ops/daemon/desktop tests |
| Supervisor-specific policy | Implemented | `SupervisorPermissionPolicy`, `RunSupervisorAction` | Ops + daemon tests |
| Ion native coding runtime | Implemented foundation | `ion` REPL, provider/tool loop, approvals/guardrails | Rust unit + CLI tests |
| Registry-backed open desktop platform identity | Pending aggregate implementation | `AgentRegistry`, `AgentPlatformId`, desktop/MCP/host | Registry + desktop tests on aggregate branch |
| General role contract | Direction, not implemented | Future ADR | Not applicable |
| Common runtime adapter + capability negotiation | Direction, not implemented | Future ADR | Not applicable |
| Typed cross-agent message bus | Partial delegations/handoffs only | `delegation`, `orchestration`, daemon contracts | Rust tests |
| **Dioxus desktop shell** | **Live host/bridge foundation; hardening continues** | Dioxus Desktop + xterm.js | Desktop contract/host tests + smoke |
| **Tauri-shaped host adapter** | **LEGACY — compatibility only** | Optional gated bridge | Remove after Dioxus host command/event parity |
| **egui operator workbench** | **LEGACY — frozen** | `impulse-gui` (compile-only) | Legacy tests only |
| SWARM semantic coordination runtime | Planned | Future orchestration engine | Not started |

## 6) Runtime Platform And Legacy Compatibility Contract

Claude Code and Codex are the current primary external coding-agent platforms for active Impulse
work; Ion is the native runtime. OpenCode support is legacy compatibility: preserve existing
behavior unless a removal plan owns migration risk, but do not treat legacy presence as proof of
active parity.

| Area | Claude Code | Codex | OpenCode legacy compatibility | Contract Expectation |
| --- | --- | --- | --- | --- |
| Session lifecycle hooks | Primary | Primary where implemented | Preserve existing generated config | Primary coverage for active platforms; no new OpenCode parity requirement |
| File write tracking | Supported | Supported where implemented | Preserve existing behavior | Equivalent active-platform behavior |
| Tool tracking | Supported | Supported where implemented | Preserve existing behavior | Equivalent active-platform behavior |
| Session end verification (`--verify`) | Available and optional | Available where hook integration exists | Preserve existing generated command | Session end is recorded without the flag; the flag makes verification a hard API gate |
| Context handoff artifacts | Shared `.impulse/context/*` | Shared `.impulse/context/*` | Shared `.impulse/context/*` | Handoff artifacts stay platform-neutral |

Contributor verification before completion remains mandatory even though the session-end API makes
`--verify` optional.

External runtime enforcement is limited to launch conditions, working-directory/project scoping,
and supported integration seams. Structural filesystem isolation depends on runtime/sandbox support.
The product must not infer that a role is satisfied merely because a prompt was injected or a pane
was labeled. Ion can support deeper structural enforcement because Impulse owns its tool/model loop,
but it remains subject to the explicit policies and gaps documented in code and `VISION.md`.

## 7) Governance and Ownership

### Contract Ownership

The following files define product truth and must be updated together for contract changes:
- `VISION.md` (living product north star and target-state boundary)
- `docs/spec/RUST-CANONICAL-CONTRACT.md` (authoritative contract)
- `AGENTS.md` (operator-facing guidance)
- `CLAUDE.md` (project technical context)
- `docs/INDEX.md` (navigation + source-of-truth routing)
- `docs/SUMMARY.yaml` (navigation source)
- `docs/SUMMARY.md` (high-level map)
- `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` (desktop contract)
- `docs/decisions/0008-dioxus-desktop-host.md` (desktop ADR)

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

**Workspace totals (2026-07-12 canonical projection):** 1,895 tests across the 5 workspace crates/packages (impulse-rs 1,567, ops 32 including the canonical-checkout archive proof, term 114, desktop 159, ion 23); 8 ignored, 0 failed. The verified isolated worktree without the gitignored reconciliation archive reports 1,894 passed with that one proof explicitly filtered.

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
