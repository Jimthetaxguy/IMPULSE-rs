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

The daemon includes `protocol_version` in its `Status` response. Clients must check this matches their expected version.

| Constant | Value | Location |
|----------|-------|----------|
| `EXPECTED_PROTOCOL_VERSION` | **3** | GUI client |
| `PROTOCOL_VERSION` | **3** | Daemon (`src/daemon/protocol.rs`) |

Version mismatch triggers a warning in the GUI status bar.

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
- Updated `EXPECTED_PROTOCOL_VERSION` to **2** (GUI constant)
