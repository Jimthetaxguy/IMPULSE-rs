---
title: Rust Canonical Product Contract
description: Authoritative product contract for Impulse based on impulse-rs
version: '2.0'
updated: 2026-07-13
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
software-engineering agents. It launches and monitors coding runtimes, provides a supervisor
action surface, and supplies shared memory, tools, telemetry, messaging/handoffs, policy, credentials,
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
- Capability-checked dynamic tools and a supervisor-specific action/permission policy foundation
- Daemon-owned governed tasks with profiled pre-PTY registration, exact acceptance criteria,
  daemon-attested clean Git subjects, derived producer records, and operator-required acceptance
- Operationally safe session lifecycle with recorded endings and optional API-level verification gates
- Human-visible observability through CLI, ratatui TUI, and the Dioxus Desktop cockpit
- Desktop workspace registry plus multi-agent launch/observation surfaces; daemon-side
  multi-workspace routing remains incomplete

## 2) Desktop Shell Contract (Updated 2026-07-13)

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
| 0 | Documentation contract reset | Complete |
| 1 | Keep PTY/process lifecycle usable independently of render surface | Core backend live; optional egui renderer still retained for legacy compatibility |
| 2 | Static Dioxus shell skeleton | Complete foundation |
| 3 | Dioxus Desktop launch scaffold + live terminal bridge (PTY → xterm.js) | Live host/bridge foundation; operational hardening continues |
| 4 | Daemon-backed workbench truth | Live snapshot/telemetry foundation; multi-workspace routing remains incomplete |

## 3) Canonical Scope and Roadmap

### Roadmap Contract

| Stage | Focus | Status |
| --- | --- | --- |
| **Now** | Rust control-plane foundation plus daemon-owned governed runtime producers and the cockpit evidence/decision surface | Active |
| **Next** | Add review-only accepted-run memory promotion, stronger same-user actor authorization, and one full launched-runtime workflow proof | Active |
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
| Role | Obligations, permissions, tools, context, communication, and verification duties | Open `AgentRoleId`/`AgentRoleAssignment` for narrow launch requirements; legacy coordinator/worker `AgentRole` topology; concrete `SupervisorPermissionPolicy`; generalized role composition is not implemented |
| Runtime | External harness or native engine that executes a role | External agent harness calls, desktop PTY runtime, and Ion native provider/tool loop; no common adapter trait yet |
| Agent instance | One running identity with runtime, role, scope, process state, and telemetry | `AgentRuntime`/`AgentRuntimeSnapshot` and desktop runtime records |
| Session | Bounded persisted work history | Daemon session lifecycle and `.impulse/` history/state |
| Task | Assignment plus acceptance/verification criteria | `GovernedTaskRun` is a durable daemon-owned identity distinct from agent/session ids; it carries bounded criteria, typed claims/evidence/verdicts/decisions, and independent execution/review state. Reassignment/resume is not implemented |
| Pane | Cockpit view/input attachment | TUI panes and desktop terminal ids; never an authority boundary |
| Workspace target | Explicit filesystem execution root | Desktop `WorkspaceTarget`/`WorkspaceRegistry` |
| Project | Governance scope for memory, artifacts, policy, and verification | Project-scoped `.impulse/` state and `ProjectOpsSnapshot`; often maps 1:1 to a workspace today |

### Platform Service Contract

Memory/retrieval, tools, telemetry, messaging/handoffs, policy, credentials, artifacts, and
verification are peer control-plane services. A runtime may receive them through native typed calls,
MCP, hooks, sockets, files, generated commands, or mediated PTY operations. Similar conceptual
capabilities do not imply identical enforcement.

Desktop platform identity is registry-backed across MCP, host, runtime, and snapshots, and Ion is
a builtin launchable platform. Repository tests exercise that implementation; release packaging
and distribution remain separate contracts.

[ADR-0010](../decisions/0010-product-role-launch-contract.md) makes one narrow product-role launch
contract live. `AgentRoleAssignment` carries an open
product-role id plus caller-supplied launch requirements. Trusted Rust-owned registry metadata
declares conservative wrapper support using ordered `unsupported`, `advisory`, `mediated`, and
`structural` strengths. Dioxus previews a fixed initial Builder profile; `DesktopRuntime` reloads
the current registry and re-evaluates the request before agent-id reservation or PTY creation.
Unsatisfied mandatory requirements block, while optional gaps remain allowed but degraded. The
task, assignment, and compatibility result remain observable through runtime and daemon telemetry.
This static preflight does not attest model-internal behavior, and canonical working-directory
mediation is not a filesystem sandbox.

[ADR-0011](../decisions/0011-governed-task-run-lifecycle.md) adds the next narrow operating
contract. A governed launch proposes a distinct task ID and must receive authoritative daemon
registration before PTY creation. The daemon persists the run in `.impulse/GOVERNED_TASKS.json`,
serializes expected-revision/idempotency-key mutations, and keeps execution state separate from
review state. Worker claims, verifier records, supervisor verdicts, and operator decisions remain
distinct typed records; only current passing verification can be recommended, and only an explicit
operator approval creates `accepted`. Dioxus renders daemon snapshots and uses an acknowledged host
command without optimistic task updates. Desktop launch/exit intent uses a bounded, owner-only,
cross-process-locked project outbox written before daemon I/O. The outbox covers queued intent and
ambiguous transport, but not abrupt desktop death before exit intent exists; runtime leases and
orphan reconciliation remain downstream. Missing targets stay queued until a future durable
registration-tombstone/expiry policy can prove they cannot appear.

[ADR-0012](../decisions/0012-daemon-owned-governed-runtime-producers.md) closes the first automatic
producer path. A profiled Dioxus Builder launch supplies exact acceptance criteria and
`rust_workspace_v1`; both desktop and daemon require the canonical Git root to be clean at a
committed `HEAD`, and the daemon independently matches the initial OID before registration. The
shared registration contract requires the exact Builder role plus a matching nonblocked
compatibility result recomputed from the daemon-owned runtime registry; caller-supplied
compatibility cannot strengthen or replace it. The profiled runtime receives
project/task/socket/control-CLI/profile routing;
ordinary and unprofiled panes have inherited producer-routing variables removed. External agents submit only a
summary and artifact IDs through `"$IMPULSE_CONTROL_CLI" --daemon governed-claim`; Ion can instead invoke its typed
`governed_submit_claim` tool. The daemon derives the assigned Worker and clean current subject.

`rust_workspace_v1` verifies the claimed commit in a detached worktree with fixed format,
locked-workspace-check, locked-strict-Clippy, and locked-workspace-test argv. The profile requires a
committed, regular, non-symlink root `Cargo.lock` and rejects source-tree symlinks. The daemon
scrubs the environment, bounds time, streams output into digests/counts/truncation flags, and uses
bounded post-kill reaping before transferring any lingering child to a background reaper.
Production governed records
retain no output preview. The detached subject must remain clean after the fixed commands, and a
bounded before/after byte manifest covers ignored source-tree paths as well as Git-visible paths;
Git administration and the external Cargo target directory are excluded. These measures do not
create an OS sandbox: Rust build scripts, proc macros, and tests are host-trusted code.

Supervisor review is one API-only, tool-free, history-free, temperature-zero turn. Its strict
contract-versioned envelope must bind the exact task revision, claim, verification, subject, and
acceptance-criteria digest. Generic external harness mode fails closed before spawning because it
cannot guarantee a structurally read-only review. Dioxus renders evidence and terminal command
guidance; it does not yet expose producer buttons. Only the operator can record final acceptance.

All governed mutations route through the single project-bound daemon writer. Expected-revision CAS
and atomic replacement do not authorize two daemon processes to write the same ledger.

On ledger load, the daemon validates canonical task/project/workspace identity, replays the
contiguous typed record/event chain, and requires exactly one valid idempotency receipt for every
revision. Forged materialized states, broken history, missing receipts, and malformed persisted
evidence fail closed before becoming workbench truth.

That receipt contract deduplicates persisted requests, and one per-task daemon lock serializes live
producer and lifecycle mutations. It is not crash-safe exactly-once execution: if the daemon dies
after a producer side effect but before durable receipt storage, retry can repeat the side effect.
A durable producer reservation journal remains required.

This is not a generalized role/runtime contract. Typed actor kinds are auditable provenance and
transition checks, not cryptographic authorization between processes running as the same OS user.
In particular, `RecordOperatorDecision` is gated only on the client-declared actor kind and travels
the same unauthenticated socket that the Desktop operator surface and profiled Builders both use, so
a same-user Builder can in principle forge the operator-required acceptance record. The
daemon-computed claim, verification, and Supervisor producers are the only transitions that are
structurally unforgeable; enforcing operator-decision provenance requires socket peer-credential
authorization and is tracked as follow-up (see
`docs/superpowers/plans/2026-07-13-governed-runtime-producers.md`).
Unprofiled caller-composed evidence retains its existing validation boundary; profiled automatic
records are derived by the daemon and cannot enter through generic mutations. One daemon adapter
remains bound to one project, project identity is currently
directory-name-derived, tasks cannot be reassigned/resumed, review errors are not yet structured CAS
codes, and global task/receipt collections are not paginated or archived.

A future generalized/dynamic adapter contract must still define runtime discovery, optional and
emulated operations, attestation freshness, and post-launch re-evaluation. General role
composition, stronger same-user actor authorization, accepted-run memory promotion, producer
profiles beyond Rust, and a complete launched Builder/Supervisor proof remain outside this slice.

## 4) Public Interface Contract

### CLI Contract (Stable)

The executable command registry is defined by Clap in `impulse-rs/src/cli.rs` and exposed by
`impulse-rs --help`. The current public commands are:

- **Lifecycle and state:** `daemon`, `run`, `init`, `session-start`, `session-end`, `track-write`,
  `track-tool`, `list-sessions`, `session-info`, `session-conflicts`, `status`, `debug`,
  `conflict-history`, `history`, `genome`, `add-decision`, `activity`, `summary`, `health`,
  `system`, `analyze`, `config`
- **Context, memory, and coordination:** `chat`, `orchestrate`, `handoff`, `sync-context`,
  `compute-injection`, `verify`, `search-history`, `search-genome`, `index-memory`,
  `retrieval-status`, `steward`, `swarm`
- **Agent and platform integration:** `hooks`, `validate-hooks`, `list-providers`,
  `agent-configure`, `agent-status`, `agent-query`, `guard`, `ion-verify`, `governed-claim`,
  `governed-verify`, `governed-review`, `mcp serve`

The packaged executable is `impulse-rs`. The three governed producer subcommands are daemon-only;
installed invocations use `impulse-rs --daemon governed-*`, while governed panes preserve their
exact injected executable path through `"$IMPULSE_CONTROL_CLI" --daemon governed-*` only when they
carry a governed verification profile.
- **Tools and content:** `tools`, `tooling-list`, `tooling-describe`, `tooling-run`,
  `tooling-schema`, `tooling-validate`, `tooling-reload`, `docs`, `model`, `office`,
  `credentials`, `extract`, `calc`, `exec`
- **Build and source operations:** `sweep`, `wipe`, `clean-all`, `sccache-setup`, `build-health`,
  `sem-diff`, `sem-blame`, `sem-impact`, `sem-status`, `analytics`
- **Machine-readable extension surfaces:** `describe`, `schema`, `plugin-list`, `plugin-invoke`

The exact direct/daemon support matrix and flags live in [`docs/CLI-COMMANDS.md`](../CLI-COMMANDS.md).

### State and Artifact Contract

| Path | Purpose | Notes |
| --- | --- | --- |
| `.impulse/LIVE_STATE.json` | Active sessions/files/tools | Ephemeral runtime state |
| `.impulse/HISTORY.jsonl` | Session history (append-only) | Durable project memory |
| `.impulse/GENOME.md` | Durable decisions/preferences | Durable project memory |
| `.impulse/config.json` | Runtime configuration | Durable config |
| `.impulse/GOVERNED_TASKS.json` | Daemon-owned governed task records plus idempotency receipts | Durable project control-plane state; atomically replaced at mode `0600` |
| `.impulse/DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json` | Bounded write-ahead desktop launch/exit intent awaiting daemon reconciliation | Durable local recovery state; cross-process sibling lock, mode `0600`, removed/emptied after acknowledgment |
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

At a repository-root `.impulse`, `impulse-rs init` idempotently adds only runtime-owned state,
socket, retrieval-cache, governed-ledger/temp, and Desktop-outbox/temp paths to `.gitignore`. It
never adds `.impulse/` as a blanket rule: `GENOME.md`, `HISTORY.jsonl`, `config.json`, and
`impulse-capabilities.json` remain deliberate durable project artifacts unless an existing
operator-owned blanket rule already hides them. Init preserves and warns about that broader rule.

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
| `governed_task_mutate` | Submit an acknowledged revisioned governed-task mutation and return daemon-owned state |
| `native_island_request` | Request a narrow macOS-native island and receive a serializable result DTO |

**Events (backend → frontend):**

| Event | Description |
|---|---|
| `terminal_output` | PTY stdout bytes |
| `terminal_exit` | PTY child exited |
| `terminal_status` | Status change |
| `ops_update` | Daemon ProjectOpsSnapshot update |
| native island result | Returned through `native_island_request`; native code does not own session, memory, terminal, or artifact state |

### Daemon Workbench IPC Contract (Authoritative — Extended in v5)

The daemon is the authoritative source for workbench surfaces:

- `Overview`
- `Agents`
- `Context`
- `Artifacts`
- governed task evidence and decision cards in the Supervisor view
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
- `AgentRuntime.governed_task_id` and `governed_task_revision` retain task provenance in
  runtime telemetry without making the runtime record authoritative for review state.
- `ProjectOpsSnapshot.governed_tasks` is the authoritative governed-task collection; old payloads
  deserialize it as empty.

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

### Daemon IPC Contract (PROTOCOL_VERSION = 5)

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
- `RegisterGovernedTask { registration }`, `GetGovernedTask { project_id, task_id }`,
  `ListGovernedTasks { project_id }`, `MutateGovernedTask { request }`
- `SubmitGovernedClaim { request }`, `RunGovernedVerification { request }`,
  `RunGovernedSupervisorReview { request }`

**Chat:**
- `Chat { session_id, message, inject_mode?, inject_explain? }`

## 5) Capability Matrix

| Capability | Status | Interface | Tests |
| --- | --- | --- | --- |
| Session lifecycle tracking | Implemented | `session-start`, `session-end` | Rust unit + integration |
| File/tool activity tracking | Implemented | `track-write`, `track-tool` | Rust unit + integration |
| ratatui TUI tabs | Implemented | `run` TUI mode | Rust UI tests |
| Daemon socket operations | Implemented for selected commands | `daemon`, CLI matrix IPC entries | Daemon tests |
| Context-aware chat (daemon) | Implemented | `--daemon chat` | Daemon + provider tests |
| Hook config generation | Implemented | `hooks --platform ...` | Integration tests |
| Orchestration handoff/context files | Implemented | `orchestrate`, `handoff`, `sync-context` | Rust tests |
| Verification gate | Implemented | `verify`, `session-end --verify` | Rust tests |
| External agent-assistance harness | Implemented foundation | Agent assistance/review IPC endpoints | Rust tests |
| Retrieval indexing + keyword search | Implemented | `index-memory`, `search-history`, `search-genome` | Rust unit + integration |
| Semantic search (feature-flagged) | Implemented (fallback-safe) | `search-* --mode semantic` | Rust unit + integration |
| Review-first context injection | Implemented (additive) | daemon chat + orchestrate/handoff/sync-context | Rust unit + integration |
| Context stewardship | Implemented | `steward` | Rust unit + integration |
| Tool management | Implemented | `tools` | Rust unit |
| Credential management | Implemented | `credentials` | Rust unit |
| PTY/process lifecycle | Implemented | `impulse-term::TerminalBackend`, desktop runtime | Rust unit + integration |
| Daemon workbench truth | Implemented foundation | `ProjectOpsSnapshot`, terminal telemetry overlay, workbench IPC | Ops/daemon/desktop tests |
| Supervisor-specific action policy | Implemented foundation | `SupervisorPermissionPolicy`, `RunSupervisorAction` | Ops + daemon tests |
| Ion native coding runtime | Implemented foundation | `ion` REPL, provider/tool loop, approvals/guardrails | Rust unit + CLI tests |
| Registry-backed open desktop platform identity | Implemented foundation | `AgentRegistry`, `AgentPlatformId`, desktop/MCP/host | Registry + desktop tests |
| Product-role launch preflight | Implemented narrow foundation | `AgentRoleId`, `AgentRoleAssignment`, static registry support, Dioxus preview, `DesktopRuntime` pre-PTY gate | Ops + desktop runtime/contract tests |
| Daemon-owned governed-task lifecycle | Implemented narrow foundation | `GovernedTaskRun`, governed workbench IPC, persistent ledger, desktop acknowledged gateway/outbox, Supervisor evidence cards | Ops/state/daemon/desktop contract and recovery tests |
| Daemon-owned governed runtime producers | Implemented first profile | `rust_workspace_v1`, governed CLI/Ion claim, detached verification, strict API Supervisor review | Handler proof covers review/operator; real daemon/CLI/restart proof stops at `awaiting_supervisor` |
| General role contract | Direction, not implemented | Future ADR | Not applicable |
| Common dynamic runtime adapter + generalized capability negotiation | Direction, not implemented | Future ADR | Not applicable |
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
supported integration seams, and the daemon-owned task transition boundary. Structural filesystem
isolation depends on runtime/sandbox support. The daemon socket directory/socket/PID file are
restricted to the local user (`0700`/`0600`/`0600`), which protects the OS-user boundary but does
not authenticate distinct roles among processes owned by that user.
The product must not infer that a role is satisfied merely because a prompt was injected or a pane
was labeled. Ion can support deeper structural enforcement because Impulse owns its tool/model loop,
but it remains subject to the explicit policies and gaps documented in code and `VISION.md`.

## 7) Governance and Ownership

### Contract Ownership

The following files define product truth and must be updated together for contract changes:
- `VISION.md` (living product north star and target-state boundary)
- `docs/spec/RUST-CANONICAL-CONTRACT.md` (authoritative contract)
- `CONTEXT.md` (stable project vocabulary and live/direction boundary)
- `AGENTS.md` (operator-facing guidance)
- `CLAUDE.md` (project technical context)
- `docs/INDEX.md` (navigation + source-of-truth routing)
- `docs/SUMMARY.yaml` (navigation source)
- `docs/SUMMARY.md` (high-level map)
- `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` (desktop contract)
- `docs/spec/USER-STORY-MAP.md` and `docs/spec/TEST-TRACEABILITY.md` (acceptance and evidence map)
- `docs/decisions/0008-dioxus-desktop-host.md` (desktop ADR)
- `docs/decisions/0010-product-role-launch-contract.md` and
  `docs/decisions/0011-governed-task-run-lifecycle.md` (governed launch/lifecycle authority)
- `docs/decisions/0012-daemon-owned-governed-runtime-producers.md` (profiled producer authority)

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

**Workspace totals:** The final `cargo test --workspace` output for the current checkout is the
authority; record its package-level passed, ignored, and failed totals in commit/PR evidence rather
than freezing a moving aggregate in this contract. Default tests must depend only on tracked source
and fixtures so fresh clones, linked worktrees, and CI execute the same gate.

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
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
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
