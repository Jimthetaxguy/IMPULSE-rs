# impulse-ops

Shared, dependency-light **operations contract** crate for the Impulse workspace. It holds the
types and protocol constants that more than one crate needs to agree on — the daemon, the desktop
host, and the core CLI all depend on `impulse-ops` so they speak the same language without depending
on each other.

This crate is deliberately small and side-effect-free: pure types, a few pure helpers, and the
canonical agent registry. It must not pull in heavy runtime dependencies.

## What lives here

| Area | Items |
|------|-------|
| **Daemon protocol** | `DAEMON_PROTOCOL_VERSION`, `WorkbenchDaemonRequest`, `WorkbenchDaemonResponse`, `OpsError` |
| **Agent registry** | `agent_registry` module — the single source of truth for known agent platforms (claude-code, codex, …), launch-command resolution, and `AgentPlatformsReport` |
| **Supervisor model** | `SupervisorAction`, `SupervisorProposal`, `SupervisorChatResult`, `SupervisorActionResult`, `SupervisorPermissionPolicy`/`State`, `SupervisorActionPermission`, `PermissionChangeScope`, `ToolCapabilityId` |
| **Agent runtime/coordination** | `AgentRuntime`, `AgentStatus`, `AgentRole`, `MachineTarget`, `DelegationSummary`, `DiffSummary`, `ToolInvocationRecord`, `InterventionRecommendation`, `TerminalOpsReport` |
| **Ops snapshot / artifacts** | `ProjectOpsSnapshot`, `OpsEvent`, `OpsSubscription`, `ArtifactEnvelope`, `ArtifactAction`, `ArtifactActionResult`, `ArtifactFileRef`, `ArtifactViewHint`, `ArtifactStatus` |
| **Summaries** | `ProjectSummary`, `MemorySummary`, `RetrievalSummary`, `ContextHealthSummary`, `InsightRecord` |
| **Pure helpers** | `sanitize_id`, `atomic_write_path`, `artifact_store_root` |

## Design rules

- **Pure contracts only.** Every public type derives `Serialize`/`Deserialize` and has a serde
  round-trip test. No I/O beyond the two explicit, atomic filesystem helpers.
- **No silent failures.** `atomic_write_path` returns `Result<_, OpsError>` (temp file + rename);
  callers must handle the error.
- **Single source of truth.** `agent_registry` is the only place agent platforms and their launch
  commands are defined; the desktop host and CLI both resolve through it.
- **Stable protocol.** Bump `DAEMON_PROTOCOL_VERSION` when `WorkbenchDaemonRequest`/`Response`
  change shape, and keep the desktop/daemon sides in lockstep.

## Build & test

```bash
cd impulse-rs/impulse-ops
cargo build && cargo test && cargo clippy -- -D warnings
```

Current: 31 tests, 0 warnings. Part of the workspace gate (`cd impulse-rs && cargo test`).
