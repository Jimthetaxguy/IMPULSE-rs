# CONTEXT — Impulse Ubiquitous Language

> **Read this first.** Shared L0/L1 vocabulary for Impulse. Update a term here when its stable
> meaning changes; put detailed history in plans/ADRs rather than growing this glossary.
>
> Entries are tagged **`[code]`** (live in the current repository implementation) or
> **`[vocabulary]`** (the product contract; "Closest in code" names today's partial carrier).
>
> Cross-agent contract: `AGENTS.md`. Product contract:
> `docs/spec/RUST-CANONICAL-CONTRACT.md`. Product north star: `VISION.md`. Current boundary map:
> `docs/ARCHITECTURE-CLARIFICATION.md`.

---

## What Impulse is

Impulse is a **terminal-native local control plane and harness manager for AI software-engineering
agents**. It manages heterogeneous runtimes and supplies memory, tools, telemetry, handoffs,
policy, credentials, artifacts, and verification. Claude Code, Codex, and similar CLIs retain their
internal loops; Ion is the native runtime. Dioxus is the cockpit, while daemon/runtime contracts
remain authoritative. Memory is one platform service, not the whole product.

Workspace: `impulse-rs/` (Cargo workspace). Main crates: `impulse-rs`, `impulse-ops`,
`impulse-term`, `impulse-desktop`, and `impulse-ion`; `impulse-gui` is legacy/frozen.

---

## Identity and hierarchy

### role — `[vocabulary]`
The stable behavioral contract assigned to an agent: obligations, permissions, tools, context,
communication, and evidence; independent of model, runtime, process, session, and pane.
- **Closest in code:** the narrow launch-time `AgentRoleId`/`AgentRoleAssignment` contract plus the
  concrete `SupervisorPermissionPolicy`. `AgentRole::{Coordinator, Worker}` remains legacy pane and
  delegation topology, not product-role identity.
- **Boundary:** generalized role composition is not live. ADR-0011's narrow governed Builder task
  lifecycle is live and remains distinct from a general role system.

### runtime — `[vocabulary]`
The engine that executes an agent role. External runtimes are wrapped CLI harnesses (for example,
Claude Code or Codex); Ion is a native direct-provider/tool-loop runtime.
- **Closest in code:** `impulse-desktop/src/runtime.rs`, `src/agent/`, `src/ion_repl/`, and
  `src/llm_backends/`.
- **Boundary:** there is not yet one common runtime-adapter trait or capability-negotiation protocol.

### agent platform id — `[code]`
An open, validated string identity (`AgentPlatformId`) owned by `AgentRegistry` across desktop,
host, MCP, browser, reducer, launcher, and snapshot paths. The registry adds Ion, derives capability
manifests, resolves its sibling binary, and fails closed on unknown/blank IDs. Declared platform and
observed command remain distinct so wrappers stay visible; legacy closed enums remain only for
wire/disk compatibility.
- **Source of truth:** `impulse-ops/src/agent_registry.rs`.

### role launch compatibility — `[code]`
A static preflight comparing caller-supplied product-role requirements with trusted Rust-owned
runtime declarations. Strength is ordered `unsupported` < `advisory` < `mediated` < `structural`;
mandatory gaps block and optional gaps degrade. Dioxus previews the result, while the desktop
runtime re-evaluates before agent-id reservation or PTY creation. Working-directory mediation is
not filesystem sandboxing.
- **Source of truth:** `impulse-ops/src/role_assignment.rs`, `impulse-desktop/src/runtime.rs`, and
  ADR-0010.

### agent instance — `[vocabulary]`
One running identity with a platform/runtime, role, workspace target, process state, and telemetry.
It is not interchangeable with its session or pane.
- **Closest in code:** `AgentRuntime`, `AgentRuntimeSnapshot`, and desktop runtime records.

### session — `[code]`
A bounded, persisted unit of work with a start, tracked activity, and recorded end. Verification
may gate that end. A session may link to an agent instance but does not own its process.
- **Source of truth:** `WorkbenchDaemonRequest::{CreateSession, EndSession}` and CLI
  `session-start` / `session-end --verify`.

### governed task — `[code]`
A daemon-owned assignment plus exact acceptance criteria and four distinct truth layers: worker
claim, verification evidence, Supervisor judgment, and operator decision. A profiled Builder binds
the canonical clean Git subject and exact shared Builder assignment; the daemon independently
recomputes runtime compatibility, derives producer actors, and creates automatic records. Execution
remains independent of review, and only operator approval accepts. Resume/reassignment is future
work.
- **Source of truth:** `impulse-ops/src/governed_task.rs`, `src/state/governed_task.rs`, ADR-0011,
  and ADR-0012.

### accepted-run memory candidate — `[code]`
A deterministic pending-review projection of one accepted governed task, persisted in owner-only
`MEMORY_CANDIDATES.json` with versioned source assurance/evidence and repairable from task truth. It
is not curated memory, never mutates `GENOME.md`/`HISTORY.jsonl`, and has no v1 mutation action.
- **Source:** `impulse-ops/src/memory_candidate.rs`, `src/state/memory_candidate.rs`, ADR-0013.

### task — `[vocabulary]`
The broader product assignment concept. A governed task is today's durable carrier; delegations,
`AgentRuntime.current_task`, and Ion `Task` contracts remain separate legacy/specialized carriers
until a future hierarchy ADR reconciles their cardinalities.

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
supervisor actions, artifacts, governed tasks, and telemetry overlays over a versioned JSON-line
Unix socket.
- **Source of truth:** `impulse-ops/src/lib.rs` and `src/daemon/{mod,protocol,handlers}.rs`.
- **Desktop daemon-truth wire:** PTY lifecycle facts publish as `TerminalOpsReport` on change and
  heartbeat, then return through daemon `SubscribeOps` snapshots. Local
  `agent_runtime_update`/`agent_snapshot` messages own terminal mechanics only and cannot overwrite
  `ProjectOpsSnapshot`. Subscription freshness is distinct from publish degradation; lifecycle
  delivery uses a reentrant FIFO, natural exits reap records, and runtime agent ids remain one-use
  routing addresses until the protocol carries explicit incarnations. The adapter currently binds
  one daemon project; cross-workspace daemon routing remains a protocol follow-up.
- **Governed task wire:** protocol v5 adds specialized claim/verify/review requests whose callers
  omit derived truth. The daemon attests clean Git subjects, verifies detached committed code, and
  binds strict API-only Supervisor output. Profiled registration requires the shared canonical
  Builder assignment and the exact compatibility result recomputed from the daemon registry;
  generic producer mutations fail for profiled tasks.
- **Candidate wire:** protocol v6 adds serde-defaulted `ProjectOpsSnapshot.memory_candidates` only;
  it defines no candidate mutation request.

### managed agent turn — `[code]`
One exclusive, bounded use of the cached `ImpulseAgent`. Concurrent turns fail fast with typed
`Busy { resource: agent_turn, retry_after_ms }`; cancellation releases the guard without removing
the cached agent or losing history.
- **Source of truth:** `try_lock_agent_for_turn` and agent request handlers in
  `src/daemon/handlers.rs`; provider timeouts bound the turn.

### agent registry — `[code]`
The catalog of platform identity and launch metadata. It answers what can be named, detected, and
launched; daemon/runtime telemetry separately answers what is currently running.
- **Invariant:** ids and aliases have one owner, identity-collision registration fails
  transactionally, and explicit command overrides remain observable.

### terminal runtime — `[code]`
The PTY/process lifecycle boundary for spawn, input, resize, focus, exit, and cleanup. Terminal
mechanics publish state; they do not own durable project truth.
- **Source of truth:** `impulse-term/src/backend.rs` and `impulse-desktop/src/runtime.rs`.

### launch target — `[code]`
The one desktop selection used by both the workspace rail and launch dock for the next Builder
root. Changing it does not re-scope running worker terminals or switch the connected daemon's
project truth; multi-project daemon routing remains later work. If no standard project-local daemon
socket is explicitly supplied at process launch, oversight remains disconnected rather than using
cwd, an ancestor, or the home-level memory root as project authority. The first confirmed governed
launch must target a registered workspace and atomically binds it as the process-lifetime daemon,
memory, telemetry, and task boundary before task registration; malformed or unconfirmed MCP
launches are audited before activation, and switching that boundary then requires restart.
- **Source of truth:** the lifted `focused_workspace_root` signal in `impulse-desktop/src/ui.rs`.

### Dioxus cockpit — `[code]`
The operator-facing desktop composition of one shared launch-target selection, an explicit
daemon-project-scoped oversight lane, desktop-wide specialized workers, focused terminals,
evidence inspectors, and launch/review controls. It consumes
daemon/runtime state through typed host commands/events and must not become a second policy or
persistence authority. The packaged host owns only a daemon companion it starts; an already-running
operator daemon remains external. A desktop-owned companion validates and watches the exact desktop
parent PID so owner loss reuses graceful daemon drain/sync/runtime-file cleanup. The oversight dock
currently represents the connected daemon project's review service and must not imply that a
model-backed Supervisor runtime is running. The home-level `~/.impulse` fallback can still back
CLI user memory, but the disconnected desktop does not expose it as project memory or review state.
- **Source of truth:**
  `impulse-desktop/src/{desktop_host,host_bridge,host_commands,daemon_ops,daemon_sidecar,desktop_shutdown,project_boundary,ui,views}.rs`.

### governed lifecycle outbox — `[code]`
An owner-only, project-local write-ahead queue for desktop launch/exit mutations that may outlive an
ambiguous daemon transport failure. Its data and sibling lock reject symlinks and non-regular leaves;
the cross-process lock preserves serialization with a bounded, shutdown-aware wait so application
close cannot hang forever behind another holder.
- **Source of truth:** `impulse-desktop/src/{daemon_ops,project_boundary}.rs`.

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

### governed producer profile — `[code]`
`rust_workspace_v1` combines env-routed/typed claim intent, fixed detached Rust verification, and
criteria-bound, history/tool-free API Supervisor review. It runs host-trusted code, not a sandbox;
receipts/task locks do not close crash-before-receipt. External harness review fails closed.

### memory / genome — `[code]`
Scoped durable continuity: session history plus verified decisions/preferences, retrieval indexes,
and review-first context injection. Memory records must carry project/session provenance.
- **Boundary:** pending accepted-run candidates are review state, not curated memory.
- **Source of truth:** `src/{state,memory,retrieval,injection,stewardship}/`.

### artifact — `[code]`
A typed, reviewable output with project/agent/session provenance, status, view hints, and permitted
actions. Artifacts keep worker claims separate from evidence and operator decisions.
- **Source of truth:** `ArtifactEnvelope` and artifact IPC in `impulse-ops/src/lib.rs`.

### verification gate — `[code]`
Evidence that claimed work holds before a session/task is accepted. Governed profiled verification
is daemon-observed against the claimed commit in a symlink-free detached source tree with a
committed regular root `Cargo.lock`. It persists fixed argv plus digests, never raw output;
session-end and Ion verification remain separate contracts.

### governed actor — `[code]`
A typed provenance claim (`system`, `worker`, `verifier`, `supervisor`, or `operator`) checked by the
task transition machine. It is not cryptographic same-user authentication; local processes that can
reach the user-restricted daemon socket remain inside the current trust boundary.

---

## Live-versus-direction boundary (2026-07-15)

- **Live foundation:** PTY/workbench truth, managed turns, registry-backed platforms/Ion, shared
  services, profiled Builder launch, routed claims, detached verification, strict API review,
  operator acceptance, repairable pending candidates with no `GENOME`/`HISTORY` mutation, and a
  signed macOS Dioxus package/verifier that requires a fresh real eval-bridge, PTY, local-assets,
  ops-event, and ordered-host-close receipt for release acceptance. Static/signature and
  disconnected-scope package proof pass on this branch, but the latest local signed GUI-host smoke
  abort remains open and historical receipts are not current proof. Real daemon tests separately
  prove desktop-owner loss cleanup; AppKit Quit plus live-PTY package proof remains release
  hardening.
- **Next:** stronger same-user actor authorization and one full launched Builder/Supervisor proof.
- **Later:** explicit candidate promotion/dismissal, reassignment/resume, generalized
  roles/adapters/capabilities, multi-project routing, and typed cross-agent messaging.
