# Control-Plane Architecture Boundaries

- **Updated:** 2026-07-12
- **Status:** Active boundary map
- **North star:** [`../VISION.md`](../VISION.md)
**Canonical product contract:** [`spec/RUST-CANONICAL-CONTRACT.md`](spec/RUST-CANONICAL-CONTRACT.md)

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
| Role | Obligations, permissions, tools, context, and completion policy | Narrow `AgentRole` plus `SupervisorPermissionPolicy` | Not a model, executable, or pane |
| Runtime | Execution engine/integration | external harness code, desktop runtime, Ion REPL/provider loop | Not an agent instance |
| Agent instance | One running identity and its status | `AgentRuntime`, `AgentRuntimeSnapshot`, desktop runtime record | Not a session |
| Session | Bounded persisted work history | daemon create/end session + `.impulse/LIVE_STATE.json`/history | Not necessarily process lifetime |
| Task | Assignment plus acceptance evidence | delegations, `current_task`, Ion harness `Task` | May span sessions |
| Pane | View/input attachment | TUI pane manager and desktop terminal ids | Not an authorization boundary |
| Workspace target | Filesystem root assigned at launch | desktop `WorkspaceTarget`/`WorkspaceRegistry` | Not the same as global cockpit scope |
| Project | Memory/policy/artifact governance boundary | `ProjectOpsSnapshot`, project-scoped `.impulse/` data | Often maps 1:1 to a workspace today, but conceptually distinct |

The generalized role contract is normative product vocabulary, but its Rust schema is deliberately
not frozen. Today's coordinator/worker enum and supervisor policy are partial carriers, not proof
that arbitrary roles can already be assigned to arbitrary runtimes.

## Current boundary matrix

| Boundary | Authoritative paths | Current truth | Status |
| --- | --- | --- | --- |
| Shared control-plane wire/read models | `impulse-rs/impulse-ops/src/lib.rs` | Versioned daemon requests/responses, workbench snapshots, telemetry, supervisor policy/actions, artifacts | Live |
| Daemon coordination | `impulse-rs/src/daemon/{mod,protocol,handlers}.rs` | Workbench authority, session operations, managed agent turns, telemetry overlays, supervisor enforcement | Live |
| PTY/process lifecycle | `impulse-rs/impulse-term/src/backend.rs`, `impulse-rs/impulse-desktop/src/runtime.rs` | Spawn, input, resize, focus, output, exit, and cleanup | Live |
| Dioxus cockpit | `impulse-rs/impulse-desktop/src/{desktop_host,host_bridge,host_commands,views}.rs` | Renders backend state and dispatches typed host actions; does not own durable truth | Live host/bridge foundation |
| Workspace registration | `impulse-rs/impulse-desktop/src/workspace.rs` | Registered filesystem targets and operator-authored project notes | Live |
| Platform identity/launch metadata | `impulse-rs/impulse-ops/src/agent_registry.rs`, desktop runtime/MCP/host | Registry-backed open ids, fail-closed resolution, Ion builtin launch | Implemented on pending local aggregate; legacy closed types remain |
| External agent harness calls | `impulse-rs/src/agent/{mod,harness,coordinator}.rs` | Bounded CLI request/response integration and shared cached agent state | Live; not a general adapter trait |
| Ion native coding runtime | `impulse-rs/src/bin/ion.rs`, `impulse-rs/src/ion_repl/`, `impulse-rs/src/llm_backends/` | Interactive direct-provider agent with history, typed tools, approval/guardrail gates, and bounded tool loop | Live |
| Ion verification harness contract | `impulse-rs/impulse-ion/src/{lib,pi_adapter}.rs` | Transport-neutral verify/review/summarize contract and external adapter | Live; separate from the interactive Ion runtime |
| Dynamic tools | `impulse-rs/src/tooling/`, `impulse-rs/src/mcp/` | Typed schemas, deny-by-default capabilities, validation, execution, audit | Live |
| Desktop MCP tools | `impulse-rs/impulse-desktop/src/mcp.rs` | Agent spawn/write, memory search, project context, staged injection/review | Live; exposure differs from native Ion tools |
| Memory/context | `impulse-rs/src/{state,memory,retrieval,injection,stewardship}/` | Project-scoped persistence, FTS5/semantic retrieval, review-first injection, context health | Live |
| Credentials | `impulse-rs/src/credentials/` | Provider abstraction for Keychain, socket, CLI proxy, env, and session memory | Live; per-role credential grants are not generalized |
| Artifacts/evidence | `ArtifactEnvelope` in `impulse-ops`, daemon artifact handlers, `impulse-rs/src/verify/` | Provenance-bearing outputs and verification gates | Live foundation |
| Agent messaging/handoffs | `impulse-rs/src/{delegation,orchestration}/`, daemon delegation contracts | Delegations, handoff artifacts, and routing logs | Live partial; no unified typed message bus |
| Legacy desktop | `impulse-rs/impulse-gui/`, optional egui modules in `impulse-term` | Compile-maintenance only | Frozen |

## Enforcement truth

Impulse can structurally control its own tools, daemon actions, PTY lifecycle, workspace selection,
credential exposure, and native Ion loop. For external CLIs it can strongly control launch
arguments, working directory, environment, process tree, injected files/instructions, supported
hooks/MCP, and observable outputs.

It cannot promise control over a vendor's hidden system prompt, proprietary reasoning loop,
internal context compression, or unsupported tool mechanics. Therefore every future runtime/role
assignment needs an explicit enforcement-strength result rather than a boolean "supported" flag.

## Direction that is not implemented yet

The next architecture ADR must settle these together:

1. The exact hierarchy and identifiers for project, workspace, role, runtime, instance, session,
   task, and pane.
2. The minimum runtime-adapter operations, optional operations, and emulation rules.
3. A capability-negotiation result that distinguishes structural, mediated, advisory, and
   unsupported enforcement.
4. Typed message routing and cross-project isolation.
5. Role-specific credential, context, tool, and verification grants.

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
