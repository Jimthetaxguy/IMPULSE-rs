# CONTEXT — Impulse Ubiquitous Language

> **Read this first.** Shared L0/L1 vocabulary for Impulse. Update a term here when its stable
> meaning changes; put detailed history in plans/ADRs rather than growing this glossary.
>
> Entries are tagged **`[code]`** (live on the cache/main foundation), **`[aggregate]`**
> (implemented on the local registry/daemon-truth work that is being preserved into the aggregate),
> or **`[vocabulary]`** (the product contract; "Closest in code" names today's partial carrier).
>
> Cross-agent contract: `AGENTS.md`. Product contract:
> `docs/spec/RUST-CANONICAL-CONTRACT.md`. Product north star: `VISION.md`. Current boundary map:
> `docs/ARCHITECTURE-CLARIFICATION.md`.

---

## What Impulse is

Impulse is a **terminal-native local control plane and harness manager for AI software-engineering
agents**. It launches and manages heterogeneous coding runtimes, supervises their operating
conditions, and supplies shared platform services: memory, tools, telemetry, messaging/handoffs,
policy, credentials, artifacts, and verification.

Claude Code, Codex, and similar CLIs retain their internal coding loops. Ion is the Impulse-native
coding runtime. Dioxus is the cockpit that renders and controls the system; backend daemon/runtime
contracts remain authoritative. Memory is essential, but it is one control-plane service rather
than the complete product.

Workspace: `impulse-rs/` (Cargo workspace). Main crates: `impulse-rs`, `impulse-ops`,
`impulse-term`, `impulse-desktop`, and `impulse-ion`; `impulse-gui` is legacy/frozen.

---

## Identity and hierarchy

### role — `[vocabulary]`
The stable behavioral contract assigned to an agent: obligations, permissions, tools, context,
communication rules, and completion evidence. A role is independent of model, runtime, process,
session, and pane.
- **Closest in code:** narrow `AgentRole::{Coordinator, Worker}` plus the concrete
  `SupervisorPermissionPolicy` in `impulse-ops/src/lib.rs`.
- **Boundary:** a generalized typed role contract is direction for a later ADR, not live today.

### runtime — `[vocabulary]`
The engine that executes an agent role. External runtimes are wrapped CLI harnesses (for example,
Claude Code or Codex); Ion is a native direct-provider/tool-loop runtime.
- **Closest in code:** `impulse-desktop/src/runtime.rs`, `src/agent/`, `src/ion_repl/`, and
  `src/llm_backends/`.
- **Boundary:** there is not yet one common runtime-adapter trait or capability-negotiation protocol.

### agent platform id — `[aggregate]`
An open, validated string identity (`AgentPlatformId`) whose metadata and launch command are owned
by `AgentRegistry`. The pending aggregate carries the registry through desktop runtime, host, MCP,
and snapshots, adds Ion as a builtin launchable platform, and fails closed on unknown/blank ids.
Legacy closed enums remain where wire/disk compatibility still requires them.
- **Source of truth:** `impulse-ops/src/agent_registry.rs` on the pending aggregate branch.

### agent instance — `[vocabulary]`
One running identity with a platform/runtime, role, workspace target, process state, and telemetry.
It is not interchangeable with its session or pane.
- **Closest in code:** `AgentRuntime`, `AgentRuntimeSnapshot`, and desktop runtime records.

### session — `[code]`
A bounded, persisted unit of work with a start, tracked activity, and recorded end. The API can
optionally gate that end on verification. A session may
be linked to a running agent instance but does not own that process.
- **Source of truth:** `WorkbenchDaemonRequest::{CreateSession, EndSession}` and CLI
  `session-start` / `session-end --verify`.

### task — `[vocabulary]`
An assignment plus acceptance and verification criteria. A task may span sessions; a session may
perform more than one task.
- **Closest in code:** delegation records, `AgentRuntime.current_task`, and Ion `Task` contracts.

### pane — `[code]`
A UI/terminal viewport attached to an agent channel. It is a presentation and input-routing object,
not an authorization or identity boundary.
- **Source of truth:** `src/ui/pane_manager.rs`, `src/ui/terminal_pane.rs`, and desktop runtime ids.

### project — `[vocabulary]`
A logical codebase/governance boundary for memory, artifacts, policy, and verification. Today it is
usually represented by one registered workspace root and one daemon-owned `ProjectOpsSnapshot`.

### workspace target — `[code]`
The explicit working-directory/project root in which an agent process operates. Several agent
instances may share a workspace; a cockpit can register and switch among several workspaces.
Structural filesystem enforcement depends on the selected runtime or sandbox.
- **Source of truth:** `impulse-desktop/src/workspace.rs` and `WorkspaceTarget` in runtime models.

---

## Platform services

### daemon / workbench truth — `[code]`
The long-running coordination point that owns project workbench snapshots, session operations,
supervisor actions, artifacts, and telemetry overlays over a versioned JSON-line Unix socket.
- **Source of truth:** `impulse-ops/src/lib.rs` and `src/daemon/{mod,protocol,handlers}.rs`.

### managed agent turn — `[code]`
One exclusive, bounded use of the cached `ImpulseAgent`. Concurrent turns fail fast with typed
`Busy { resource: agent_turn, retry_after_ms }`; cancellation releases the guard without removing
the cached agent or losing history.
- **Source of truth:** `try_lock_agent_for_turn` and agent request handlers in
  `src/daemon/handlers.rs`; provider timeouts bound the turn.

### agent registry — `[aggregate]`
The catalog of platform identity and launch metadata. It answers what can be named, detected, and
launched; daemon/runtime telemetry separately answers what is currently running.
- **Invariant:** ids and aliases have one owner; explicit command overrides remain observable.

### terminal runtime — `[code]`
The PTY/process lifecycle boundary for spawn, input, resize, focus, exit, and cleanup. Terminal
mechanics publish state; they do not own durable project truth.
- **Source of truth:** `impulse-term/src/backend.rs` and `impulse-desktop/src/runtime.rs`.

### Dioxus cockpit — `[code]`
The operator-facing desktop composition of terminals, agents, context, artifacts, and controls.
It consumes daemon/runtime state through typed host commands/events and must not become a second
policy or persistence authority.
- **Source of truth:** `impulse-desktop/src/{desktop_host,host_bridge,host_commands,views}.rs`.

### tool capability — `[code]`
A deny-by-default permission required by a typed tool. Tool availability varies by runtime bridge;
conceptual parity does not imply identical enforcement.
- **Source of truth:** `src/tooling/{traits,registry,executor}.rs`, `src/mcp/`, and desktop MCP.

### supervisor policy — `[code]`
The concrete permission and confirmation policy for supervisor actions such as monitoring, memory
search, focus, input, context operations, and permission changes. It is the first role-specific
policy, not a generalized role system.
- **Source of truth:** `SupervisorPermissionPolicy`/`SupervisorPermissionState` in
  `impulse-ops/src/lib.rs` and enforcement in `src/daemon/handlers.rs`.

### memory / genome — `[code]`
Scoped durable continuity: session history plus verified decisions/preferences, retrieval indexes,
and review-first context injection. Memory records must carry project/session provenance.
- **Source of truth:** `src/{state,memory,retrieval,injection,stewardship}/`.

### artifact — `[code]`
A typed, reviewable output with project/agent/session provenance, status, view hints, and permitted
actions. Artifacts keep worker claims separate from evidence and operator decisions.
- **Source of truth:** `ArtifactEnvelope` and artifact IPC in `impulse-ops/src/lib.rs`.

### verification gate — `[code]`
Evidence that claimed work holds before a session/task is accepted: commands run, tests/build/lint,
artifact review, and explicit approval where policy requires it.
- **Source of truth:** `session-end --verify`, `src/verify/`, and Ion harness verdict contracts.

---

## Live-versus-direction boundary (2026-07-12)

- **Live foundation:** PTY lifecycle, daemon workbench truth, managed agent turns, supervisor-specific
  permissions, capability-checked tools, memory/retrieval/injection, artifacts, credentials, and
  Ion's native coding REPL/tool loop.
- **Pending aggregate implementation:** open registry-backed desktop platform identity, real Ion
  desktop launch, and daemon-truth terminal telemetry wiring. These local changes are being
  preserved and integrated; do not describe them as shipped on `origin/main` yet.
- **Direction:** generalized role contracts, one runtime-adapter interface, explicit capability
  negotiation/enforcement strength, and typed cross-agent messaging. These require an ADR and
  vertical slice; names and schemas are intentionally not frozen here.
