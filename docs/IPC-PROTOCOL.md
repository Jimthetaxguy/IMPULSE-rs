# Impulse IPC Protocol

> Unix domain socket protocol between Impulse daemon and clients (GUI, CLI `--daemon` mode).
> **Protocol version: 3** — see [Version section](#protocol-version) for upgrade notes.

---

## Transport

- **Socket:** Unix domain socket at `.impulse/sockets/impulse.sock`
- **Framing:** Newline-delimited JSON (each message is one JSON object followed by `\n`)
- **Direction:** Request-response. Client sends one `DaemonRequest`, daemon replies with one `DaemonResponse`.
- **Encoding:** UTF-8

---

## Envelope Format

All messages use Serde internally-tagged enums:

```rust
#[serde(tag = "type", content = "data")]
```

### Request

```json
{"type": "RequestVariant", "data": { ... }}
```

Requests with no data omit the `data` field:

```json
{"type": "Ping"}
```

### Response

```json
{"type": "Ok", "data": {"result": ...}}
{"type": "Error", "data": {"message": "..."}}
{"type": "Busy", "data": {"resource": "agent_turn", "retry_after_ms": 250}}
{"type": "AgentAssistResult", "data": { ... }}
{"type": "ConflictCheck", "data": {"has_conflict": true, "conflicting_sessions": ["id1"]}}
```

---

## Protocol Version

The daemon reports `protocol_version` in `Ping`/`Status` results.

| Constant | Value | Location |
|----------|-------|----------|
| `EXPECTED_PROTOCOL_VERSION` | **3** | GUI client |
| `PROTOCOL_VERSION` | **3** | Daemon (`src/daemon/protocol.rs`) |

Current clients do **not** perform a version handshake or preflight-reject a mismatched daemon, and
the protocol does not negotiate a downgrade. Known variants continue through normal JSON-line
request/response decoding; an unknown request or response variant fails through the ordinary Serde
or client error path. The reported number is observability, not negotiated compatibility.

**Upgrading from v2:** v3 adds the typed `Busy` response. Agent-backed requests return `resource: "agent_turn"` with a retry hint when another logical turn owns the cached agent; busy requests never reach a provider or mutate conversation state.

**Upgrading from v1:** v2 added the full Agent System section, Delegation System, `CheckConflict`, `GetSession`, `TrackTool`, `StewardStatus/Proposals/Memory`, `ListTools`, `DescribeTool`, `Chat`, `GetAgentPool`, and the `AgentAssistResult` / `AgentSpecializedResult` response variants.

---

## Request Variants

### Session Management

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `Ping` | — | v1 | Health check, returns `Ok` |
| `Status` | — | v1 | Returns session count, active count, protocol version |
| `CreateSession` | `{name, platform?}` | v1 | Create a new session |
| `EndSession` | `{session_id, summary}` | v1 | End session with summary |
| `GetSession` | `{session_id}` | v1 | Get details for a specific session |
| `ListSessions` | — | v1 | Returns array of session objects |

#### Examples

```json
{"type": "CreateSession", "data": {"name": "feature-work", "platform": "claude-code"}}
{"type": "EndSession", "data": {"session_id": "abc123", "summary": "Added auth module"}}
{"type": "GetSession", "data": {"session_id": "abc123"}}
```

### File & Tool Tracking

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `TrackFile` | `{session_id, file_path}` | v1 | Record a file write |
| `TrackTool` | `{session_id, tool_name}` | v1 | Record a tool use |

```json
{"type": "TrackFile", "data": {"session_id": "abc123", "file_path": "src/main.rs"}}
{"type": "TrackTool", "data": {"session_id": "abc123", "tool_name": "Bash"}}
```

### Tool System

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `ListTools` | `{category?}` | v1 | List all available tools (optionally filtered by category) |
| `DescribeTool` | `{name}` | v1 | Get a tool's descriptor (params, capabilities) |
| `InvokeTool` | `{name, params}` | v1 | Execute a registered tool |
| `ToolSchema` | — | v1 | Export all tool schemas in Claude tool-calling format |

```json
{"type": "ListTools"}
{"type": "ListTools", "data": {"category": "builtin"}}
{"type": "InvokeTool", "data": {"name": "calc", "params": {"expression": "2+2"}}}
{"type": "ToolSchema"}
```

### Stewardship

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `StewardStatus` | — | v1 | Get current stewardship status and mode |
| `StewardProposals` | `{action, id?}` | v1 | Propose or review a stewardship action |
| `StewardMemory` | — | v1 | Analyze and report on memory health |

### Operations Snapshot (Desktop Shell)

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `GetOpsSnapshot` | — | v1 | Full state snapshot for desktop shell rendering |
| `SubscribeOps` | `{since_seq?}` | v1 | Get ops updates since sequence number |
| `PublishTerminalOps` | `{report}` | v1 | Push live terminal telemetry from the desktop shell |

### Connection Provenance

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `PresentOperatorCapability` | `{token}` | v7 | Raise this connection to operator class by presenting the daemon run's capability |

Classification is per connection and is never derived from request payload. A connection begins as
`non_operator` — which is what a launched governed runtime holding `IMPULSE_SOCKET_PATH` is — and a
successful presentation returns `{"connection_class": "operator"}`. The presentation succeeds only
when the peer uid, read from `SO_PEERCRED`/`LOCAL_PEERCRED`, equals the daemon's own uid **and** the
token matches; a rejection leaves the class unchanged and never echoes the token. The daemon writes
its capability at mode 0600 beside the socket (`impulse.sock` -> `impulse.operator-cap`) while it is
listening, and removes it on shutdown. `IMPULSE_OPERATOR_CAPABILITY` is an accepted client override;
governed panes never receive it, because every inherited `IMPULSE_*` key is scrubbed before a pane
spawns. This is a structural boundary, not protection against a same-uid process that deliberately
reads the file — see ADR-0018.

### Governed Tasks

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `RegisterGovernedTask` | `{registration}` | v4 | Persist the client-proposed distinct task identity before PTY launch |
| `GetGovernedTask` | `{project_id, task_id}` | v4 | Return one authoritative governed task, if present |
| `ListGovernedTasks` | `{project_id}` | v4 | Return the bound project's governed task records |
| `MutateGovernedTask` | `{request}` | v4 | Apply an expected-revision/idempotency-key lifecycle mutation and return authoritative state |
| `SubmitGovernedClaim` | `{request: {request_id, project_id, task_id, expected_revision, summary, artifact_ids[]}}` | v5 | Derive assigned Worker and clean Git subject, then record the claim |
| `RunGovernedVerification` | `{request: {request_id, project_id, task_id, expected_revision}}` | v5 | Run the task's closed verification profile and derive evidence |
| `RunGovernedSupervisorReview` | `{request: {request_id, project_id, task_id, expected_revision}}` | v5 | Run one strict API-only Supervisor review and derive its verdict |
| `PromoteGovernedOutcome` | `{request: {request_id, project_id, task_id, expected_revision}}` | v9 | Fast-forward the canonical branch onto an accepted staged outcome (operator-class) |
| `DiscardGovernedStagedWorktree` | `{request: {request_id, project_id, task_id, expected_revision, reason}}` | v9 | Reclaim a finished staged worktree (operator-class) |

**Staged world scope (v9).** A registration carrying `world_scope: "staged_authoritative"` is an
operator-initiated launch: the daemon materializes `<workspace>/.impulse/worktrees/<task id>` from
the attested initial OID as part of registration, so the checkout exists before the Builder's PTY
starts, and `launch_working_directory` on the returned record is the staged root. A non-operator
connection is refused with the same typed error the other operator-class requests use, and a
materialization that fails leaves no task record behind.

**Blocked promotion is a success, not an error.** `PromoteGovernedOutcome` answers `Ok` whenever it
reached a decision, including when it could not move the branch. The recorded outcome is on the
returned task (`promotions[].outcome`): either `{"kind": "promoted", "promoted_revision": "<oid>"}`
or `{"kind": "promotion_blocked", "canonical_head": "<oid>", "reason": "..."}` where the reason is
`canonical_head_moved`, `detached_head`, `concurrent_branch_update`, `repository_config_changed`
(with the `component` that changed), or `repository_config_unpinned`. Review state stays `accepted`
and the staged worktree stays active, so an operator who reconciles the canonical branch can retry.
Submodule configuration found at promotion time is neither a blocked outcome nor an error: it is
the typed staged-configuration refusal described below. `Err` is reserved for a genuine failure: a
non-operator connection, a task that is not staged or not accepted, a revision conflict, or a Git
error.

**Discard states what it costs.** `DiscardGovernedStagedWorktree` is refused, before anything is
deleted, unless the run is rejected, escalated, launch-failed, unpinned, or accepted with a recorded
promotion outcome. When the discard drops the only ref to an accepted commit a blocked promotion
never made canonical, the acknowledgement carries `unreferenced_accepted_commit` with that OID: the
commit survives only in the reflog afterwards, and the operator surface must say so.

**A drifted staged configuration is a typed refusal, not an error.** ADR-0019 rule 13 pins the
worktree-shared repository configuration across a staged run, because a `filter.<name>.smudge` or
`diff.<name>.textconv` defined there executes whenever Git materializes a file — including inside a
daemon-owned producer. When that pin no longer holds, `SubmitGovernedClaim` and
`RunGovernedVerification` refuse **before spawning any Git process** and answer `Ok` with a refusal
acknowledgement rather than an error: the governed task flattened as usual, plus
`"refused": true`, a typed `reason`, and the `remedy` to apply. The task is unchanged — a refusal
records nothing — and any producer reservation taken for the attempt is released, so the retry the
remedy ends in is not blocked. `PromoteGovernedOutcome` answers the same acknowledgement for the one
gate that refuses at promotion — submodule configuration, `unsupported_submodules` — while a drifted
pin at promotion is the recorded `promotion_blocked` outcome above, because promotion has an accepted
run to record it against and the other two producers do not.

```json
{"type": "Ok", "data": {"result": {
  "id": "task-1", "revision": 7, "...": "...",
  "refused": true,
  "reason": {"kind": "repository_config_changed", "component": "repository_config"},
  "remedy": "discard the staged worktree and re-materialize it, then re-run the producer"
}}}
```

`reason.kind` is `repository_config_unpinned` (the worktree predates the pin, so there is nothing
to compare), `repository_config_changed` (with the `component` that differs: `repository_config`,
`worktree_config`, or `info_attributes`), or `unsupported_submodules` (with the `path`; the staged
scope cannot pin a submodule's own configuration and refuses to run in such a repository). The
`kind` token is the same string the daemon logs and the CLI renders — one name per reason. Clients discriminate on the
`refused` flag. An `Error` response from these endpoints still means what it always did: the
producer genuinely failed.

The CLI prints this refusal in text or as the unchanged JSON acknowledgement and exits nonzero
for `governed-claim`, `governed-verify`, and `governed-promote`: a completed protocol exchange does
not mean the requested producer ran. The desktop promotion gateway discriminates `refused` before
mutation adoption, validates the echoed project/task and unchanged revision, and forwards the typed
reason and daemon remedy to a durable per-task notice. Recorded mutation acknowledgements still
require a strictly newer revision; a refusal never updates cached task state.

**Producer acknowledgements and the reservation journal.** `RunGovernedVerification`,
`RunGovernedSupervisorReview`, and `PromoteGovernedOutcome` run their side effect *and* persist the
governed-task mutation that records it inside one durable producer reservation (ADR-0012's
2026-09-02 amendment). Two observable consequences: a request arriving while a same-revision
reservation is still open is refused with the journal's typed
`an open producer reservation already exists for task ... producer ...` error; and a request whose
earlier attempt was interrupted before its receipt — reconciled to `needs_rerun` when the daemon
reloaded — reruns, with `pending_rerun_reason` on the acknowledgement explaining why. A request the
daemon recognizes as a replay of an already-recorded receipt answers with `replayed: true` and runs
no side effect at all.

Governed task actor kinds are typed provenance and transition claims, not cryptographic same-user
authentication. Since v7 the *connection* behind an operator decision is authorized: the mutation
requires operator class, and the daemon stamps `OperatorDecision.authentication`
(`declared` or `capability_authenticated`) from the connection rather than from the payload, which
carries no such field. The daemon socket directory/socket/PID permissions protect the local OS-user
boundary. For unprofiled tasks, the generic mutation endpoint still accepts caller-composed records
and validates their shape/lifecycle consistency. For `rust_workspace_v1`, automatic producer
records can only enter through the three v5 requests: the daemon attests the clean Git subject,
executes fixed Rust commands in a detached worktree, derives evidence, and strictly binds the
Supervisor response. Evidence retains argv/outcome and digests rather than raw output.

The verifier executes host-trusted project code, including Rust build scripts, proc macros, and
tests. Detached checkout, environment scrubbing, bounded timeouts, and process-group cleanup are not
an OS sandbox. Governed Supervisor review uses an API runtime with exactly system+user messages,
temperature zero, no tools, and no shared chat history. Generic external harness mode fails closed
before spawning. CAS and transition failures currently use
`Error { message }`; stable structured error codes/current-revision payloads remain future work.
`ProjectOpsSnapshot.governed_tasks` is serde-defaulted for older snapshots, while
`AgentRuntime.governed_task_id` and `governed_task_revision` carry runtime provenance.
`ProjectOpsSnapshot.memory_candidates` is also serde-defaulted. It is a read-only projection from
accepted governed-task truth, not a candidate mutation or semantic-memory promotion surface.

#### PublishTerminalOps — TerminalOpsReport fields

The `report` object carries live telemetry from terminal panes to the daemon for overlay on the durable snapshot:

```json
{
  "source_id": "terminal-1",
  "published_at": "2026-03-31T12:00:00Z",
  "agents": [{ ... }],
  "context": { ... },
  "interventions": [{ ... }]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `source_id` | string | Unique pane/terminal identifier |
| `published_at` | ISO 8601 | Timestamp of the report |
| `agents` | `AgentRuntime[]` | Active agent state per pane — includes `id`, `label`, `status`, `role`, `group`, `context`, `recent_files`, `recent_tools`, `warnings`, `ephemeral`, `tool_invocations`, `diff_summary` |
| `context` | `ContextHealthSummary` | Context health for this pane — includes tier (`hot`/`warm`/`cold`), token counts, and summary |
| `interventions` | `InterventionRecommendation[]` | Pending operator interventions — includes `id`, `title`, `description`, `severity`, `action_kind` |

> **Daemon overlay rules:** Build durable snapshot first. Overlay fresh telemetry by `session_id`, then by `agent.id`. Expose unmatched telemetry as ephemeral agents (`AgentRuntime.ephemeral = true`). Mark stale after 10s without heartbeat. Stop overlaying stale telemetry after 10s. Purge telemetry-only entries after 60s.

```json
{"type": "SubscribeOps", "data": {"since_seq": 42}}
{"type": "GetOpsSnapshot"}
{"type": "PublishTerminalOps", "data": {"report": { ... }}}
```

### Agent System

The agent system provides AI-powered coordination, code review, error analysis, and pane summarization. All agent requests enrich prompts with `ExtractedInsight` context from the context lifecycle.

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `AgentAssist` | `{prompt, context?, insights[]}` | v2 | AI coordination assistance with cross-pane context enrichment |
| `AgentReviewCode` | `{file_path, diff, insights[]}` | v2 | Request code review via the Impulse Agent |
| `AgentAnalyzeError` | `{error_text, context, insights[]}` | v2 | Request error analysis via the Impulse Agent |
| `AgentSummarizePane` | `{pane_id, raw_output?, insights[]}` | v2 | Request pane activity summary via the Impulse Agent |

#### AgentAssist

Formats extracted insights into a structured cross-pane context block and prepends it to the user prompt before delegation. Returns `AgentAssistResult` with coordination recommendations and per-pane summaries.

```json
{"type": "AgentAssist", "data": {
  "prompt": "Should I refactor the auth module?",
  "context": "User is working on feature-branch",
  "insights": [
    {"pane_id": 1, "agent_kind": "ClaudeCode", "insight_type": "FileModified",
     "content": "src/auth/login.rs modified 3 times this session"}
  ]
}}
```

#### AgentReviewCode

Sends a file diff to the Impulse Agent for structured review. Returns `AgentSpecializedResult`.

```json
{"type": "AgentReviewCode", "data": {
  "file_path": "src/main.rs",
  "diff": "-fn old() {}\\n+fn new() {}",
  "insights": []
}}
```

#### AgentAnalyzeError

Provides structured error analysis with context. Returns `AgentSpecializedResult`.

```json
{"type": "AgentAnalyzeError", "data": {
  "error_text": "thread 'main' panicked at 'index out of bounds'",
  "context": "When calling parse_config() with empty input",
  "insights": []
}}
```

#### AgentSummarizePane

Generates a natural-language summary of pane activity. Returns `AgentSpecializedResult`.

```json
{"type": "AgentSummarizePane", "data": {
  "pane_id": 1,
  "raw_output": "...",
  "insights": []
}}
```

### Conflict System

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `CheckConflict` | `{session_id, file_path}` | v1 | Check if a file is being modified by another session |
| `GetConflictHistory` | — | v2 | Get the full conflict resolution audit trail |
| `ClearResolvedConflicts` | — | v2 | Purge resolved conflicts from the audit trail |

```json
{"type": "CheckConflict", "data": {"session_id": "abc123", "file_path": "src/main.rs"}}
{"type": "GetConflictHistory"}
{"type": "ClearResolvedConflicts"}
```

### Delegation System

Tracks sub-agent delegations detected in coordinator output. Each delegation records a frozen context snapshot and depth-limited child agent chains.

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `RegisterDelegation` | `{spec, coordinator_pane_id, context_snapshot?}` | v2 | Register a delegation detected in agent output |
| `CompleteDelegation` | `{delegation_id, summary, tool_trace?, diff_summary?}` | v2 | Mark a delegation as completed |
| `ListDelegations` | — | v2 | List all tracked delegations |

```json
{"type": "RegisterDelegation", "data": {
  "spec": { "delegation_id": "del-1", "delegated_to": "claude-code", "depth": 1 },
  "coordinator_pane_id": 0,
  "context_snapshot": "Frozen at delegation time..."
}}
{"type": "CompleteDelegation", "data": {
  "delegation_id": "del-1",
  "summary": "Refactored database layer",
  "tool_trace": [],
  "diff_summary": { "files_changed": 4, "insertions": 120 }
}}
{"type": "ListDelegations"}
```

### Agent Pool

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `GetAgentPool` | — | v2 | All sessions grouped by role (coordinator, worker, supervisor) |

Returns `AgentAssistResult` with sessions organized by agent role.

### Supervisor System

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `GetSupervisorPermissions` | — | v1 | Get current permission policy |
| `SupervisorChat` | `{prompt, context?}` | v1 | Send a supervisor chat message |
| `RunSupervisorAction` | `{action}` | v1 | Execute a supervisor action with daemon-side policy enforcement |

#### SupervisorAction Variants

```json
{"type": "RunSupervisorAction", "data": {"action": {"FocusAgent": {"pane_id": 1}}}}
{"type": "RunSupervisorAction", "data": {"action": {"ModifyPermissions": {
  "scope": "SessionOverride",
  "grant_actions": ["InjectContext"],
  "grant_tool_capabilities": [],
  "confirmed": true
}}}}
```

### Artifacts

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `ListArtifacts` | `{limit?}` | v1 | List project-scoped artifacts for the desktop shell |
| `GetArtifact` | `{artifact_id}` | v1 | Get a single artifact by ID |
| `RunArtifactAction` | `{artifact_id, action_id, params}` | v1 | Execute an artifact action |

```json
{"type": "ListArtifacts", "data": {"limit": 50}}
{"type": "GetArtifact", "data": {"artifact_id": "doc-1"}}
{"type": "RunArtifactAction", "data": {
  "artifact_id": "doc-1",
  "action_id": "render",
  "params": {"format": "html"}
}}
```

### Guardrails

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `GuardList` | — | v1 | List all active guardrail rules |
| `GuardEvaluate` | `{action, target}` | v1 | Evaluate an action against rules |

### Plugins

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `ListPlugins` | — | v1 | List registered context providers and action handlers |
| `InvokePlugin` | `{name, input?}` | v1 | Invoke a named action handler plugin |

### Search & Retrieval

Protocol v3 does not define daemon request variants for retrieval. `search-history`,
`search-genome`, `index-memory`, and `retrieval-status` are direct-mode CLI operations; the
`--daemon` dispatcher tells callers to retry without the flag.

### Debug

| Request | Data | Since | Description |
|---------|------|-------|-------------|
| `DebugSnapshot` | — | v1 | Internal state dump (pid, sessions, tools, plugins, config) |

---

## Response Format

All responses use the `DaemonResponse` enum.

### Ok

Contains the result as a JSON value. The structure depends on the request.

```json
{"type": "Ok", "data": {"result": {"sessions": 3, "active": 1, "protocol_version": 3}}}
```

### Error

```json
{"type": "Error", "data": {"message": "session not found: abc123"}}
```

### Busy

Returned when a singleton daemon resource is already owned by another request. The caller may
retry after the supplied backoff; the rejected request has not reached the provider or changed
daemon conversation state.

```json
{"type": "Busy", "data": {"resource": "agent_turn", "retry_after_ms": 250}}
```

### AgentAssistResult

Returned by `AgentAssist`. Contains the agent's response plus coordination recommendations and per-pane summaries.

```json
{"type": "AgentAssistResult", "data": {
  "success": true,
  "response": "Based on the cross-pane context...",
  "recommendations": [
    {"kind": "Conflict", "message": "src/main.rs is being edited in session-2"},
    {"kind": "Error", "message": "Previous pane encountered a timeout"}
  ],
  "pane_summaries": [
    ["pane-0", ["Modified 3 files", "Running tests"]],
    ["pane-1", ["Reviewing PR #42"]]
  ]
}}
```

| Field | Type | Description |
|-------|------|-------------|
| `success` | bool | Whether the agent request succeeded |
| `response` | string | The agent's main response text |
| `recommendations` | array | `CoordinationResult` recommendations (conflicts, errors, delegations). Empty when no insights were provided. |
| `pane_summaries` | array | Per-pane summaries as `[pane_label, summary_lines]` tuples from `aggregate_pane_summaries`. |

### AgentSpecializedResult

Returned by `AgentReviewCode`, `AgentAnalyzeError`, and `AgentSummarizePane`.

```json
{"type": "AgentSpecializedResult", "data": {
  "success": true,
  "response": "The diff introduces a potential nil dereference..."
}}
```

### ConflictCheck

Returned by `CheckConflict`:

```json
{"type": "ConflictCheck", "data": {"has_conflict": true, "conflicting_sessions": ["session-a", "session-b"]}}
```

---

## Connection Lifecycle

1. Client connects to Unix socket
2. Client sends request JSON + newline
3. Daemon processes and sends response JSON + newline
4. Connection can be reused for multiple request-response pairs
5. Client disconnects when done

The GUI maintains a persistent connection via a poller thread that sends periodic `Ping` requests and measures RTT for the status bar health indicator.

Ordinary Desktop IPC reads and writes use a two-second timeout. A profiled
`RegisterGovernedTask` read uses a dedicated 90-second bound because the daemon performs bounded Git
attestation before acknowledging it. Ambiguous transport retries reuse the exact serialized request
and idempotency key; each acknowledged attempt retains its own bound. Registration also recomputes
the supplied role compatibility from the daemon-owned runtime registry and rejects any mismatch.

---

## Error Handling

- Invalid JSON → `Error` response with parse details
- Unknown request type → `Error` response
- Handler failure → `Error` response with error message
- Socket not found → client-side connection error (daemon not running)
- Stale socket → daemon startup detects and cleans up (since v0.1, Loop 18)

---

## Changelog

### v3 — Managed-agent backpressure

Added 2026-07-11:

- `Busy { resource, retry_after_ms }` response variant shared by daemon, workbench, CLI, and GUI contracts.
- `agent_turn` busy resource for fail-fast rejection of concurrent singleton-agent turns.
- Busy requests do not invoke a provider, queue past the client timeout, or mutate cached agent state.

### v2 — Agent System additions

Added in Ralph Plan 3 (2026-03-31):

- `AgentAssist` — AI coordination with context enrichment
- `AgentReviewCode` — code review via Impulse Agent
- `AgentAnalyzeError` — error analysis via Impulse Agent
- `AgentSummarizePane` — pane summary via Impulse Agent
- `GetConflictHistory` / `ClearResolvedConflicts` — conflict audit trail management
- `RegisterDelegation` / `CompleteDelegation` / `ListDelegations` — delegation lifecycle
- `GetAgentPool` — sessions grouped by agent role
- `AgentAssistResult` response variant with `recommendations` + `pane_summaries`
- `AgentSpecializedResult` response variant
- `TrackTool`, `GetSession`, `StewardStatus/Proposals/Memory`, `ListTools`, `DescribeTool`, `Chat`
