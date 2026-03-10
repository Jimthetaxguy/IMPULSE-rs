# Impulse CLI Command Reference

> All commands: `impulse-rs <command> [flags]`
> Daemon mode: `impulse-rs --daemon <command>` (requires running daemon)
> Direct mode: `impulse-rs <command>` (default, stateless)

## Command Matrix

| Command | Direct | Daemon | Description |
|---------|--------|--------|-------------|
| `daemon` | — | Yes | Start/stop/status of the daemon process |
| `run` | Yes | — | Start the daemon (direct entry point) |
| `session-start` | Yes | Yes | Begin a new session (`--name`, `--platform`, `--inject-mode`) |
| `session-end` | Yes | Yes | End session (`--session-id`, `--summary`, `--verify`, `--sem-diff-base`) |
| `track-write` | Yes | Yes | Record a file write (`--file`, `--session-id`) |
| `track-tool` | Yes | Yes | Record a tool use (`--tool`, `--session-id`) |
| `list-sessions` | Yes | Yes | List all sessions |
| `session-info` | Yes | Yes | Show details for a session (`<id>`) |
| `session-conflicts` | Yes | Yes | Check cross-session file conflicts (`--file`, `--session-id`) |
| `status` | Yes | Yes | Show daemon/sidecar status |
| `debug` | Yes | Yes | Show internal state snapshot (sessions, tools, plugins, config) |
| `conflict-history` | Yes | Yes | Show historical file conflict audit trail |
| `chat` | Yes | Yes | Send a message to the LLM (`--session-id`, `--message`, `--inject-mode`) |
| `genome` | Yes | — | Display genome (permanent decisions) |
| `history` | Yes | — | Display session history |
| `list-providers` | Yes | — | List available LLM providers |
| `add-decision` | Yes | — | Add a genome decision (`--description`, `--rationale`) |
| `init` | Yes | — | Initialize `.impulse/` directory |
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
| `verify` | Yes | Yes | Run project verification checks |
| `search-history` | Yes | Yes | Search session history (`--query`, `--mode`, `--backend`, `--limit`) |
| `search-genome` | Yes | Yes | Search genome decisions (`--query`, `--mode`, `--backend`, `--limit`) |
| `index-memory` | Yes | Yes | Index memory for search (`--scope`, `--rebuild`) |
| `retrieval-status` | Yes | Yes | Show retrieval index status (`--check`, `--json`) |
| `tools` | Yes | — | Manage tools (`list`/`run`/`schema`, `--tool`, `--dry-run`) |
| `docs` | Yes | — | Manage docs (`list`/`refresh`, `--provider`, `--force`) |
| `model` | Yes | — | Manage models (`list`/`set`, `--provider`, `--model`) |
| `office` | Yes | — | Office document handling (`info`/`extract`, `--file`, `--goal`) |
| `credentials` | Yes | — | Manage credentials (`list`/`set`/`get`, `--provider`, `--key`) |
| `steward` | Yes | — | Stewardship (`status`/`analyze`/`compact`/`approve`/`reject`) |
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
| `describe` | Yes | Yes | Emit machine-readable command registry (ATCC v1) |
| `schema` | Yes | Yes | Emit JSON Schema for a command (`<command>`) |
| `plugin-list` | Yes | Yes | List registered plugins (`--json`) |
| `plugin-invoke` | Yes | Yes | Invoke a plugin action handler (`<name>`, `--path`, `--query`) |

## Global Flags

| Flag | Description |
|------|-------------|
| `-c, --impulse-dir` | Custom `.impulse/` directory (default: `.impulse`) |
| `-v, --verbose` | Verbose output |
| `--daemon` | Route command through daemon IPC instead of direct mode |
| `--socket` | Custom Unix socket path |
| `--format` | Output format: `json`, `text`, `ndjson` |

## Mode Differences

**Direct mode** (default): Stateless. Each command reads from disk, processes, writes back, and exits. Best for hooks and one-off queries.

**Daemon mode** (`--daemon`): Routes commands through the running daemon via Unix socket IPC. In-memory state, periodic disk sync. Best for interactive use and the GUI.

Commands that work in both modes have identical semantics — the daemon version uses in-memory state while direct mode reads/writes `.impulse/` files directly.

## Feature Flags

| Flag | Default | Commands Affected |
|------|---------|-------------------|
| `office-support` | Yes | `office` |
| `monty-support` | No | `extract`, `compute-injection`, `calc`, `exec` |
| `datafusion-support` | No | Advanced analytics |
