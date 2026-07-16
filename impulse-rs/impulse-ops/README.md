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
| **Governed tasks** | Profiled registration, revisioned lifecycle records, claim/verification/review trigger DTOs, and the strict Supervisor response envelope |
| **Accepted-run memory candidates** | Versioned deterministic candidate IDs, pending-review status, source assurance, bounded provenance, and candidate shape validation |
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
- **Review is not promotion.** `AcceptedRunMemoryCandidate` is a side-effect-free contract for a
  pending proposal. It has no promote/dismiss action and carries no worker summary or Supervisor
  rationale; the runtime-owned ledger and reconciliation live in the core crate.

## Build & test

```bash
cd impulse-rs/impulse-ops
cargo build && cargo test && cargo clippy -- -D warnings
```

The default suite uses tracked source and fixtures, so fresh clones, linked worktrees, and CI run the
same contract proof. It is also part of the workspace gate (`cd impulse-rs && cargo test`).
