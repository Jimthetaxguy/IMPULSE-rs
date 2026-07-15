# Impulse CLI Command Reference

> Packaged executable: `impulse-rs`
> All commands: `impulse-rs [global flags] <command> [command flags]`
> Daemon mode: `impulse-rs --daemon <command>` (requires a foreground daemon in another terminal)
> Direct mode: `impulse-rs <command>` (default, stateless)

The **With `--daemon`** column distinguishes actual socket IPC from commands that still execute
locally. `—` means the dispatcher prints a direct-mode instruction instead of running the command.

## Command Matrix

| Command | Direct | With `--daemon` | Description |
|---------|--------|--------|-------------|
| `daemon` | Yes | IPC status only | Run the foreground daemon listener; `--daemon daemon --stop` does not terminate it |
| `run` | Yes | — | Launch the 10-tab ratatui workbench |
| `session-start` | Yes | IPC | Begin a new session (`--name`, `--platform`, `--inject-mode`) |
| `session-end` | Yes | IPC | End session (`--session-id`, `--summary`, `--verify`, `--sem-diff-base`) |
| `track-write` | Yes | IPC | Record a file write (`--file`, `--session-id`) |
| `track-tool` | Yes | IPC | Record a tool use (`--tool`, `--session-id`) |
| `list-sessions` | Yes | IPC | List all sessions |
| `session-info` | Yes | IPC | Show details for a session (`<id>`) |
| `session-conflicts` | Yes | IPC | Check cross-session file conflicts (`--file`, `--session-id`) |
| `status` | Yes | IPC | Show daemon/control-plane status |
| `debug` | Yes | IPC | Show internal state snapshot (sessions, tools, plugins, config) |
| `conflict-history` | Yes | — | Show historical file conflict audit trail |
| `chat` | Yes | IPC | Send a message to the LLM (`--session-id`, `--message`, `--inject-mode`) |
| `genome` | Yes | — | Display genome (permanent decisions) |
| `history` | Yes | — | Display session history |
| `list-providers` | Yes | — | List available LLM providers |
| `add-decision` | Yes | — | Add a genome decision (`--description`, `--rationale`) |
| `init` | Yes | — | Initialize `.impulse/`; in a Git root append exact runtime-only ignore rules and warn on an existing blanket rule |
| `config` | Yes | — | Get/set config (`<key>`, `--value`, `--list`) |
| `extract` | Yes | — | Extract structured data using Monty (`--content`, `--json`) |
| `swarm` | Yes | — | Analyze SWARM patterns (`--agent-a`, `--agent-b`, `--threshold`) |
| `activity` | Yes | — | Show recent activity (`--limit`) |
| `hooks` | Yes | — | Generate hook configurations (`--platform`) |
| `validate-hooks` | Yes | — | Generate hook validation kit (`--platform`) |
| `orchestrate` | Yes | — | Orchestrate a multi-agent task (`--task`, `--inject-mode`) |
| `handoff` | Yes | — | Hand off context between tools (`--tool`, `--task`, `--session-id`) |
| `sync-context` | Yes | — | Synchronize context across sessions (`--session-id`, `--inject-mode`) |
| `compute-injection` | Yes | — | Compute dynamic injection selection (`--query`, `--limit`) |
| `verify` | Yes | Local | Run project verification checks |
| `search-history` | Yes | — | Search session history (`--query`, `--mode`, `--backend`, `--limit`) |
| `search-genome` | Yes | — | Search genome decisions (`--query`, `--mode`, `--backend`, `--limit`) |
| `index-memory` | Yes | — | Index memory for search (`--scope`, `--rebuild`) |
| `retrieval-status` | Yes | — | Show retrieval index status (`--check`, `--json`) |
| `tools` | Yes | — | Manage installed tools (`list`/`init`/`update`/`check`, `--tool`, `--dry-run`) |
| `docs` | Yes | — | Manage model/provider docs cache (`list`/`fetch`/`update`/`providers`/`status`, `--provider`, `--force`) |
| `model` | Yes | — | Manage models (`list`/`set`, `--provider`, `--model`) |
| `office` | Yes | — | Office document handling (`info`/`extract`, `--file`, `--goal`) |
| `credentials` | Yes | — | Manage credentials (`list`/`get`/`set`/`delete`/`status`, `--provider`, `--key`, `--socket-path`, `--tool`) |
| `steward` | Yes | — | Stewardship (`status`/`analyze`/`list`/`approve`/`reject`/`memory`/`compact`) |
| `calc` | Yes | — | Python calculation (`--expression`) |
| `exec` | Yes | — | Python execution (`--code`) |
| `system` | Yes | — | Show system info |
| `analyze` | Yes | — | Analyze session/performance (`--session-id`, `--scope`) |
| `health` | Yes | — | Health check |
| `summary` | Yes | — | Quick summary |
| `sweep` | Yes | — | Sweep stale build artifacts (`--dry-run`, `--path`, `--days`) |
| `wipe` | Yes | — | Aggressive wipe of target/ dirs (`--dry-run`, `--path`) |
| `clean-all` | Yes | — | Workspace-wide cargo clean (`--dry-run`) |
| `sccache-setup` | Yes | — | Setup sccache compilation cache (`--check`, `--json`) |
| `build-health` | Yes | — | Disk usage report for build artifacts (`--json`) |
| `tooling-list` | Yes | — | List dynamic tools (`--category`, `--json`) |
| `tooling-describe` | Yes | — | Describe a dynamic tool (`<tool_id>`, `--json`) |
| `tooling-run` | Yes | — | Execute a dynamic tool (`<tool_id>`, `--params`, `--json`) |
| `tooling-schema` | Yes | — | Export tool schemas for agent discovery (`--format`) |
| `tooling-validate` | Yes | — | Validate manifest-defined tools (`--json`) |
| `tooling-reload` | Yes | — | Reload runtime tooling (`--json`) |
| `mcp serve` | Yes | — | Serve registry-backed MCP interface (`--transport`, `--port`) |
| `agent-configure` | Yes | — | Configure Impulse Agent (`--provider`, `--api-key`, `--harness`) |
| `agent-status` | Yes | — | Show agent status (`--json`) |
| `agent-query` | Yes | — | Query the agent (`--prompt`, `--json`) |
| `guard` | Yes | — | Evaluate guardrail rules (`--action`, `--target`, `--list`, `--enable`) |
| `sem-diff` | Yes | — | Semantic diff between Git refs (`--base`, `--head`, `--json`) |
| `sem-blame` | Yes | — | Semantic blame for a file (`--file`, `--json`) |
| `sem-impact` | Yes | — | Semantic impact analysis (`--entity`, `--json`) |
| `sem-status` | Yes | — | Check if sem CLI is available (`--json`) |
| `analytics` | Yes | — | Show conflict analytics (`--json`, `--period`) |
| `describe` | Yes | Local | Emit machine-readable command registry (ATCC v1) |
| `schema` | Yes | Local | Emit JSON Schema for a command (`<command>`) |
| `plugin-list` | Yes | IPC | List registered plugins (`--json`) |
| `plugin-invoke` | Yes | IPC | Invoke a plugin action handler (`<name>`, `--path`, `--query`) |
| `ion-verify` | Yes | — | Run the Ion/Pi verification gate (`--repo`, `--diff-ref`, `--description`, `--json`) |
| `governed-claim` | — | IPC | Submit only Builder intent (`--summary`, repeated `--artifact-id`); daemon derives actor and clean Git subject |
| `governed-verify` | — | IPC | Trigger the current task's daemon-owned closed verification profile |
| `governed-review` | — | IPC | Trigger strict API-only, tool-free, stateless Supervisor review |

## Global Flags

| Flag | Description |
|------|-------------|
| `-c, --impulse-dir` | Custom `.impulse/` directory (default: `.impulse`) |
| `-v, --verbose` | Verbose output |
| `--daemon` | Route command through daemon IPC instead of direct mode |
| `--socket` | Custom Unix socket path; otherwise `IMPULSE_SOCKET_PATH`, then `.impulse/sockets/impulse.sock` |
| `--format` | Output format: `json`, `text`, `ndjson` |

## Mode Differences

**Direct mode** (default): Stateless. Each command reads from disk, processes, writes back, and exits. Best for hooks and one-off queries.

**Daemon mode** (`--daemon`): Routes commands through the running daemon via Unix socket IPC. In-memory state, periodic disk sync. Best for interactive use and the GUI.

Only entries marked **IPC** are routed through the Unix socket. Entries marked **Local** are accepted
with the flag but execute in the client process; they do not acquire daemon state or semantics.

### Governed launch routing

The three governed producer commands require `--daemon`; direct mode fails with an explicit retry
instruction. `--project-id` and `--task-id` are optional only inside a governed launch, where the
desktop runtime injects `IMPULSE_PROJECT_ID` and `IMPULSE_GOVERNED_TASK_ID`. Only a profiled
governed pane also receives `IMPULSE_SOCKET_PATH`, `IMPULSE_CONTROL_CLI`, and
`IMPULSE_GOVERNED_VERIFICATION_PROFILE`; ordinary and unprofiled panes have inherited producer
routing removed.

```bash
"$IMPULSE_CONTROL_CLI" --daemon governed-claim --summary "Implemented the exact acceptance criteria"
"$IMPULSE_CONTROL_CLI" --daemon governed-verify
"$IMPULSE_CONTROL_CLI" --daemon governed-review
```

Outside an injected pane, the equivalent prefix is `impulse-rs --daemon`. The executable name is
not `impulse`; `$IMPULSE_CONTROL_CLI` remains the launch-time path supplied by Impulse.

Ion exposes the same claim path as the typed `governed_submit_claim` tool. Neither transport can
submit actor identity, subject revision, command evidence, or a Supervisor verdict. Verification
executes the fixed profile against a detached committed subject and may run project-authored Rust
build scripts/tests; it is host-trusted execution, not an OS sandbox. Review requires a configured
API provider. A generic Claude Code/Codex/Gemini harness configuration fails closed before spawn.
Only the existing operator decision path can accept the task.

`rust_workspace_v1` requires a committed, regular root `Cargo.lock`; dependency-resolving commands
use `--locked`. It rejects source-tree symlinks, and requires the detached subject to remain
Git-clean and its bounded source-tree byte manifest, including ignored paths, to remain unchanged
after all fixed commands. Generated source-tree output makes the result inconclusive rather than
passing.

## Feature Flags

| Flag | Default | Commands Affected |
|------|---------|-------------------|
| `office-support` | Yes | `office` |
| `monty-support` | No | `extract`, `compute-injection`, `calc`, `exec` |
| `datafusion-support` | No | Advanced analytics |
