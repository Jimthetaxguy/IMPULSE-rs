# Impulse IPC Protocol

> Unix domain socket protocol between Impulse daemon and clients (GUI, CLI `--daemon` mode).

## Transport

- **Socket:** Unix domain socket at `.impulse/sockets/impulse.sock`
- **Framing:** Newline-delimited JSON (each message is one JSON object followed by `\n`)
- **Direction:** Request-response. Client sends one `DaemonRequest`, daemon replies with one `DaemonResponse`.
- **Encoding:** UTF-8

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
{"type": "ConflictCheck", "data": {"has_conflict": true, "conflicting_sessions": ["id1"]}}
```

## Protocol Version

The daemon includes `protocol_version` in its `Status` response. Clients should check this matches their expected version.

```
GUI constant: EXPECTED_PROTOCOL_VERSION = 1
Daemon constant: PROTOCOL_VERSION = 1
```

Version mismatch triggers a warning in the GUI status bar.

## Request Variants

### Session Management

| Request | Data | Description |
|---------|------|-------------|
| `Ping` | — | Health check, returns `Ok` |
| `Status` | — | Returns session count, active count, protocol version |
| `ListSessions` | — | Returns array of session objects |
| `CreateSession` | `{name, platform?}` | Create a new session |
| `EndSession` | `{session_id, summary}` | End session with summary |

#### Examples

```json
{"type": "CreateSession", "data": {"name": "feature-work", "platform": "claude-code"}}
{"type": "EndSession", "data": {"session_id": "abc123", "summary": "Added auth module"}}
```

### File & Tool Tracking

| Request | Data | Description |
|---------|------|-------------|
| `TrackFile` | `{session_id, file_path}` | Record a file write |

```json
{"type": "TrackFile", "data": {"session_id": "abc123", "file_path": "src/main.rs"}}
```

### Tool System

| Request | Data | Description |
|---------|------|-------------|
| `InvokeTool` | `{name, params}` | Execute a registered tool |
| `ToolSchema` | — | Export all tool schemas |

```json
{"type": "InvokeTool", "data": {"name": "calc", "params": {"expression": "2+2"}}}
```

### Operations Snapshot (GUI)

| Request | Data | Description |
|---------|------|-------------|
| `GetOpsSnapshot` | — | Full state snapshot for GUI rendering |
| `SubscribeOps` | `{since_seq?}` | Get ops updates since sequence number |
| `PublishTerminalOps` | `{report}` | Push terminal telemetry from GUI |

The `TerminalOpsReport` contains per-pane context lifecycle data (tier, tokens, insights).

```json
{"type": "SubscribeOps", "data": {"since_seq": 42}}
{"type": "GetOpsSnapshot"}
```

### Supervisor System

| Request | Data | Description |
|---------|------|-------------|
| `GetSupervisorPermissions` | — | Get current permission policy |
| `SupervisorChat` | `{prompt, context?}` | Send a supervisor chat message |
| `RunSupervisorAction` | `{action}` | Execute a supervisor action |

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

| Request | Data | Description |
|---------|------|-------------|
| `RunArtifactAction` | `{artifact_id, action_id, params}` | Execute an artifact action |

```json
{"type": "RunArtifactAction", "data": {
  "artifact_id": "doc-1",
  "action_id": "render",
  "params": {"format": "html"}
}}
```

### Guardrails

| Request | Data | Description |
|---------|------|-------------|
| `GuardList` | — | List all active guardrail rules |
| `GuardEvaluate` | `{action, target}` | Evaluate an action against rules |

### Plugins

| Request | Data | Description |
|---------|------|-------------|
| `ListPlugins` | — | List registered context providers and action handlers |
| `InvokePlugin` | `{name, path?, query?, options?}` | Invoke a named action handler |

### Debug

| Request | Data | Description |
|---------|------|-------------|
| `DebugSnapshot` | — | Internal state dump (pid, sessions, tools, plugins, config) |

### Search & Retrieval

Search and retrieval requests are dispatched via the daemon when using `--daemon` mode:

| Request | Data | Description |
|---------|------|-------------|
| `SearchHistory` | `{query, mode?, limit?}` | Search session history |
| `SearchGenome` | `{query, mode?, limit?}` | Search genome decisions |
| `IndexMemory` | `{scope, rebuild}` | Trigger re-indexing |

## Response Format

All responses use the `DaemonResponse` enum:

### Ok

Contains the result as a JSON value. The structure depends on the request.

```json
{"type": "Ok", "data": {"result": {"sessions": 3, "active": 1, "protocol_version": 1}}}
```

### Error

```json
{"type": "Error", "data": {"message": "session not found: abc123"}}
```

### ConflictCheck

Returned by conflict-related operations:

```json
{"type": "ConflictCheck", "data": {"has_conflict": true, "conflicting_sessions": ["session-a", "session-b"]}}
```

## Connection Lifecycle

1. Client connects to Unix socket
2. Client sends request JSON + newline
3. Daemon processes and sends response JSON + newline
4. Connection can be reused for multiple request-response pairs
5. Client disconnects when done

The GUI maintains a persistent connection via a poller thread that sends periodic `Ping` requests and measures RTT for the status bar health indicator.

## Error Handling

- Invalid JSON → `Error` response with parse details
- Unknown request type → `Error` response
- Handler failure → `Error` response with error message
- Socket not found → client-side connection error (daemon not running)
- Stale socket → daemon startup detects and cleans up (since v0.1, Loop 18)
