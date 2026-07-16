# Control-Plane Architecture Boundaries

- **Updated:** 2026-07-15
- **Status:** Active boundary map
- **North star:** [`../VISION.md`](../VISION.md)
- **Canonical product contract:** [`spec/RUST-CANONICAL-CONTRACT.md`](spec/RUST-CANONICAL-CONTRACT.md)

This replaces the February 2026 "dead code versus scaffolding" inventory. Several files named by
that inventory no longer exist, and its memory-sidecar framing is no longer the product contract.
The useful question now is which boundary owns truth today and which future contracts still need
an ADR before implementation.

## Architectural center

Impulse is a local coding-agent control plane and harness manager.

```text
 operator surfaces (Dioxus / ratatui / CLI)
                     |
                     v
 daemon + shared control-plane contracts
                     |
       +-------------+--------------+
       |                            |
 PTY/external runtime boundary   Ion native runtime
       |                            |
 Claude, Codex, other CLIs       direct provider/tool loop
                     |
 shared platform services: memory, tools, telemetry, messaging,
 policy, credentials, artifacts, and verification
```

The lower-level runtime controls its own coding loop. Impulse controls or observes the operating
conditions around that loop. Dioxus is the cockpit; it is never the policy, persistence, or
workbench authority.

## Product identities are not interchangeable

| Identity | Meaning | Live carrier | Important non-equivalence |
| --- | --- | --- | --- |
| Role | Obligations, permissions, tools, context, and completion policy | Product-role `AgentRoleAssignment`/`AgentRoleId`; legacy coordinator/worker `AgentRole`; `SupervisorPermissionPolicy` for supervisor actions | Not a model, executable, pane, or legacy topology label |
| Runtime | Execution engine/integration | external harness code, desktop runtime, Ion REPL/provider loop | Not an agent instance |
| Agent instance | One running identity and its status | `AgentRuntime`, `AgentRuntimeSnapshot`, desktop runtime record | Not a session |
| Session | Bounded persisted work history | daemon create/end session + `.impulse/LIVE_STATE.json`/history | Not necessarily process lifetime |
| Task | Assignment plus acceptance evidence | daemon-owned `GovernedTaskRun`; delegations, `current_task`, and Ion harness `Task` remain separate carriers | Governed task id is distinct from agent/session ids; current assignment is immutable and resume/reassignment is future work |
| Pane | View/input attachment | TUI pane manager and desktop terminal ids | Not an authorization boundary |
| Workspace target | Filesystem root assigned at launch | desktop `WorkspaceTarget`/`WorkspaceRegistry` | Not the same as global cockpit scope |
| Project | Memory/policy/artifact governance boundary | `ProjectOpsSnapshot`, project-scoped `.impulse/` data | Often maps 1:1 to a workspace today, but conceptually distinct |

A narrow `AgentRoleId` assignment and compatibility schema now carries explicit role/task launch
preflight. It remains distinct from legacy coordinator/worker topology and does not freeze
generalized role composition, a common runtime-adapter contract, or dynamic capability negotiation.

## Current boundary matrix

| Boundary | Authoritative paths | Current truth | Status |
| --- | --- | --- | --- |
| Shared control-plane wire/read models | `impulse-rs/impulse-ops/src/{lib,governed_task,memory_candidate}.rs` | Versioned daemon requests/responses, workbench snapshots, governed task/evidence/decision types, accepted-run candidate types, telemetry, supervisor policy/actions, artifacts | Live |
| Daemon coordination | `impulse-rs/src/daemon/{mod,protocol,handlers}.rs`, `src/state/{governed_task,memory_candidate}.rs`, `src/governed_producers.rs` | Workbench authority, persistent governed-task lifecycle, deterministic accepted-run review projection, profiled claim/verification/Supervisor producers, managed agent turns, telemetry overlays | Live |
| PTY/process lifecycle | `impulse-rs/impulse-term/src/backend.rs`, `impulse-rs/impulse-desktop/src/{runtime,daemon_ops}.rs` | Spawn, input, resize, focus, output, exit, cleanup, pre-PTY governed registration, and durable launch/exit reconciliation | Live |
| Dioxus cockpit | `impulse-rs/impulse-desktop/src/{desktop_host,host_bridge,host_commands,ui}.rs` | Renders backend state/evidence and dispatches acknowledged revisioned decisions; does not own or optimistically mutate durable task truth | Live host/bridge foundation |
| Workspace registration | `impulse-rs/impulse-desktop/src/workspace.rs` | Registered filesystem targets and operator-authored project notes | Live |
| Platform identity/launch metadata | `impulse-rs/impulse-ops/src/agent_registry.rs`, desktop runtime/MCP/host | Registry-backed open ids, fail-closed resolution, Ion builtin launch | Live; legacy closed types remain where compatibility requires them |
| External agent harness calls | `impulse-rs/src/agent/{mod,harness,coordinator}.rs` | Bounded CLI request/response integration and shared cached agent state | Live; not a general adapter trait |
| Ion native coding runtime | `impulse-rs/src/bin/ion.rs`, `impulse-rs/src/ion_repl/`, `impulse-rs/src/llm_backends/` | Interactive direct-provider agent with history, typed tools, approval/guardrail gates, and bounded tool loop | Live |
| Ion verification harness contract | `impulse-rs/impulse-ion/src/{lib,pi_adapter}.rs` | Transport-neutral verify/review/summarize contract and external adapter | Live; separate from the interactive Ion runtime |
| Dynamic tools | `impulse-rs/src/tooling/`, `impulse-rs/src/mcp/` | Typed schemas, deny-by-default capabilities, validation, execution, audit | Live |
| Desktop MCP tools | `impulse-rs/impulse-desktop/src/mcp.rs` | Agent spawn/write, memory search, project context, staged injection/review | Live; exposure differs from native Ion tools |
| Memory/context | `impulse-rs/src/{state,memory,retrieval,injection,stewardship}/` | Project-scoped persistence, deterministic pending accepted-run candidates, FTS5/semantic retrieval, review-first injection, context health | Live; candidate promotion/dismissal is not implemented |
| Credentials | `impulse-rs/src/credentials/` | Provider abstraction for Keychain, socket, CLI proxy, env, and session memory | Live; per-role credential grants are not generalized |
| Artifacts/evidence | governed/candidate types and `ArtifactEnvelope` in `impulse-ops`, daemon governed/artifact handlers, `impulse-rs/src/governed_producers.rs` | Separate claims, daemon-observed profiled evidence, strict Supervisor verdicts, operator decisions, deterministic pending memory candidates, and provenance-bearing outputs | Live first producer profile + review-only candidate foundation |
| Agent messaging/handoffs | `impulse-rs/src/{delegation,orchestration}/`, daemon delegation contracts | Delegations, handoff artifacts, and routing logs | Live partial; no unified typed message bus |
| Legacy desktop | `impulse-rs/impulse-gui/`, optional egui modules in `impulse-term` | Compile-maintenance only | Frozen |

## Enforcement truth

Impulse can structurally control its own tools, daemon actions, PTY lifecycle, workspace selection,
credential exposure, and native Ion loop. For external CLIs it can strongly control launch
arguments, working directory, environment, process tree, injected files/instructions, supported
hooks/MCP, and observable outputs.

The current profiled governed desktop launch boundary requires a nonblank task, exact non-empty
acceptance criteria, `rust_workspace_v1`, and both `workspace.root` and `cwd` naming the same
absolute canonical Git worktree root. The desktop requires a clean committed `HEAD`; the daemon
independently re-observes that OID before registering the task and creating the PTY. The canonical
root drives process state and telemetry. Working-directory binding is not filesystem sandboxing.

After that preflight, a governed Builder launch must register a distinct task with the bound daemon
before the PTY is created. Env-routed `"$IMPULSE_CONTROL_CLI" --daemon governed-claim` and Ion claim surfaces submit only summary/artifact intent;
the daemon derives the Worker and clean Git subject. It verifies a detached checkout with fixed locked Rust
argv and derives evidence. Supervisor review is a strict criteria-digest-bound, tool-free,
history-free API turn; generic external harness mode fails closed before spawn. A model may
recommend acceptance, but only the operator can accept. Dioxus renders daemon-owned records and
terminal command guidance rather than producer buttons. Launch/exit intent is written ahead of
daemon I/O to a bounded, owner-only,
cross-process-locked project outbox and reconciled after daemon recovery. This does not detect an
abrupt desktop death that occurs before exit intent exists; runtime leases/orphan reconciliation
remain a separate contract.

Profiled command evidence stores fixed argv, SHA-256 digests, byte counts, and truncation flags,
never raw output. Verification executes host-trusted Rust build scripts/proc macros/tests in a
detached worktree with a scrubbed environment, bounds, and process cleanup; it is not an OS sandbox.
Typed actors are provenance and transition claims, not cryptographic
identity among processes running as the same user. The socket directory, socket, and PID file use
`0700`, `0600`, and `0600`, respectively; that is an OS-user boundary, not a same-user role boundary.

After operator approval, `GOVERNED_TASKS.json` remains the acceptance authority and the daemon
derives one pending projection in owner-only `MEMORY_CANDIDATES.json`. The two writes are not one
filesystem transaction: acceptance replay and daemon-start reconciliation repair a missing
projection, while orphaned or source-mismatched candidates fail closed. Protocol v6 exposes the
serde-defaulted candidates through `ProjectOpsSnapshot`; Dioxus renders them read-only. This path
never mutates `GENOME.md` or `HISTORY.jsonl` and grants no semantic promotion capability.
Accepted/rejected decisions are terminal. Each ledger uses its own synced-temp-file rename (without
parent-directory fsync), not a cross-file transaction. Candidate digests cover exact JSON bytes from
a fixed ordered/versioned source struct, not Unicode semantic normalization.

It cannot promise control over a vendor's hidden system prompt, proprietary reasoning loop,
internal context compression, or unsupported tool mechanics. The live static preflight therefore
uses explicit enforcement strengths rather than a boolean "supported" flag; future generalized
runtime adapters must preserve that honesty while adding discovery and lifecycle semantics.

## Direction that is not implemented yet

The next architecture ADR must settle these together:

1. The remaining hierarchy and identifiers for project, workspace, role, runtime, instance,
   session, and pane, plus stable project identity and governed-task reassignment/resume.
2. The minimum runtime-adapter operations, optional operations, and emulation rules.
3. Generalized and dynamic capability negotiation beyond the static desktop preflight, including
   discovery, attestation freshness, emulation, and post-launch re-evaluation.
4. Typed message routing and cross-project isolation.
5. Role-specific credential, context, tool, and verification grants.
6. Durable producer reservations for crash-safe replay, structured CAS error codes, task/receipt
   pagination and archival, and stronger same-user actor authorization where deployment profiles
   require it.
7. A complete launched Builder/Supervisor proof that reaches acceptance and observes exactly one
   staged candidate.
8. Explicit candidate promotion/dismissal with semantic validation, authorization, audit, and the
   curated-memory write boundary.

Do **not** create separate `ROLES`, `RUNTIMES`, `SUPERVISOR`, or replacement architecture schema
documents before those decisions land. `VISION.md` and the canonical contract remain the source of
product intent; the later ADR will authorize schema-level splits.

## Verification commands

```bash
rg -n "WorkbenchDaemonRequest|ProjectOpsSnapshot|SupervisorPermissionPolicy" impulse-rs/impulse-ops/src impulse-rs/src/daemon
rg -n "TerminalBackend|spawn_agent|AgentRuntimeSnapshot" impulse-rs/impulse-term/src impulse-rs/impulse-desktop/src
rg -n "ToolRegistry|Capability|McpToolRegistry" impulse-rs/src/tooling impulse-rs/src/mcp impulse-rs/impulse-desktop/src/mcp.rs
rg -n "ChatState|chat_with_tools|ReplToolRegistry" impulse-rs/src/ion_repl impulse-rs/src/llm_backends
git diff --check
```
