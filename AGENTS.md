---
title: Agent Guidelines
description: Guidelines for AI coding agents working in this repository
version: '4.2'
authors:
  - name: Impulse Maintainers
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---

# AGENTS.md — Impulse

> Guidelines for AI coding agents contributing to this project.
> Product north star: [`VISION.md`](VISION.md)
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Collaboration playbook: [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=control-plane foundations + governed runtime producers + accepted-run review candidates; Next=stronger same-user actor authorization + full launched Builder/Supervisor proof; Later=memory promotion/dismissal daemon/UI/runtime wiring + general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.

This file is the project context for every coding agent. [`CLAUDE.md`](CLAUDE.md) imports it for Claude Code. Put new project facts here.

---

## What This Project Is

Impulse is a terminal-native **local control plane and harness manager** for AI coding agents. It runs external harnesses such as Claude Code and Codex, and it contains Ion, the Impulse-native coding runtime. Memory is one platform service alongside process supervision, tools, telemetry, messaging/handoffs, policy, credentials, artifacts, and verification.

```
 Dioxus cockpit / TUI / CLI
              │
              ▼
 Impulse daemon + control-plane services
              │
       runtime adapters / PTYs
       ├── external CLI harnesses
       └── Ion native coding runtime
```

Impulse can set launch conditions, working-directory and project scope, process lifecycle, exposed tools, credentials, and observable policy gates. Structural filesystem enforcement depends on the selected runtime or sandbox. Hidden prompts, proprietary reasoning loops, and unsupported internals of third-party CLIs stay inside those CLIs.

**Identities:**

- **Role** — obligations, permissions, tools, context, and completion rules. Independent of runtime and UI position.
- **Runtime** — the execution engine (Claude Code, Codex, Ion, or another CLI/API agent).
- **Agent instance** — one running identity with a role, runtime, project target, and status.
- **Session** — bounded recorded work by an agent instance. It is the record, separate from the process and the pane.
- **Task** — an assignment plus acceptance and verification criteria. One session may touch more than one task, and a task may span sessions.
- **Pane** — a cockpit viewport or terminal attachment. A pane is a view, never a security or policy boundary.

Live code has registry-driven desktop launch identity, a supervisor-specific `SupervisorPermissionPolicy`, and a narrow coordinator/worker `AgentRole`. The generalized role contract, a common runtime-adapter trait, and capability negotiation are future ADR work. Describe each runtime by the controls that exist for it.

---

## Layout and toolchain

Package manager: **Cargo**. [`impulse-rs/rust-toolchain.toml`](impulse-rs/rust-toolchain.toml) selects channel `stable` with `clippy` and `rustfmt`. The `impulse-rs` package `rust-version` is **1.96** (floor set by monty 1.0, ADR-0021). Package version is `0.1.0`. `default-run` is `impulse-rs`, because the package has two binaries (`impulse-rs` and `ion`) and a bare `cargo run` would otherwise be ambiguous.

Workspace members in [`impulse-rs/Cargo.toml`](impulse-rs/Cargo.toml): the root package, `impulse-desktop`, `impulse-ion`, `impulse-ops`, `impulse-step-model`, and `impulse-term`. `impulse-gui` is excluded.

| Path | Role |
|------|------|
| `impulse-rs/` | CLI, daemon, ratatui TUI, and library crate `impulse_rs` (`src/lib.rs`). Bins: `src/main.rs` (`impulse-rs`), `src/bin/ion.rs` (`ion`). CLI variants live in `src/cli.rs`; `main.rs` only parses and dispatches. |
| `impulse-rs/impulse-ops/` | Shared control-plane protocol and models: supervisor policy, telemetry, workbench snapshots, artifacts, daemon requests, agent-platform registry. |
| `impulse-rs/impulse-term/` | PTY, vt100, write queue, and context bridge. New PTY behavior stays framework-neutral. |
| `impulse-rs/impulse-desktop/` | Dioxus cockpit, typed host bridge, workspace registry, PTY runtime, desktop MCP. It projects daemon truth. |
| `impulse-rs/impulse-ion/` | Ion harness contract v0 (`HarnessRequest` / `HarnessResponse`, `PiAdapter`). Design history: [`impulse-rs/impulse-ion/TUI_SPEC.md`](impulse-rs/impulse-ion/TUI_SPEC.md). |
| `impulse-rs/impulse-step-model/` | Harness-owned step-model policy (ADR-0015). |
| `impulse-rs/impulse-gui/` | Frozen egui/eframe workbench. Excluded from the workspace. |

`impulse-desktop` host smoke is a separate Node script (`impulse-rs/impulse-desktop/package.json`, private package `impulse-desktop-visual-smoke`). It is not the Rust package manager and it does not replace the Cargo gate. From `impulse-rs/impulse-desktop/`: `npm run host:smoke` (that script is `dioxus:host:smoke`).

### Ion REPL

Bare `ion` opens the rustyline REPL in `src/ion_repl/`. Slash commands in `router.rs`: `/help`, `/quit`, `/clear`, `/verify`, `/tools`, `/allow`, `/loop`. History file: `.impulse/ion_history`. Free text goes to `chat::ChatState`, which calls `llm_backends::Agent::chat_with_tools` with the session `ReplToolRegistry`. `ion verify` and `impulse-rs ion-verify` share `handlers::ion::handle_ion_verify`.

Default registry (`registry.rs`, with the default features): `ion_verify`, `governed_submit_claim`, `document_read` (`office-support`), `file_read`, `file_write`, `bash_exec`, `memory_search`, `genome_read`, `handoff`, `blackboard_store`, `blackboard_fetch`, `search_tools`, and `photon` (`photon-subagent`).

Safety invariants in the current code:

- **Confirmation gate.** `CONFIRMATION_REQUIRED_TOOLS` is `bash_exec`, `file_write`, `governed_submit_claim`, and `handoff`. Those run only after approval; a decline returns before `ReplTool::run`. `bash_exec` commands and `file_write` content are scanned by `src/guardrail` first. `decide_approval` requires the literal `CONFIRM` for a Block-tier verdict and accepts `y` / `yes` otherwise. Gated tools execute only while holding an `ApprovalGrant`. The scan covers the tool call's arguments.
- **Sandbox.** Bridged tools write under the repo root and read under the repo root plus `/allow` grants (`ReplContext::sandbox_tool_context`). `memory_search` and `genome_read` may also read an explicitly set `IMPULSE_HOME`, which they validate themselves.
- **Read-before-write.** `file_write` over an existing file is refused, before any confirmation prompt, unless this session read the whole file (from line 1, not truncated) or wrote it, and its version (mtime, length, and on Unix inode and ctime) is unchanged since (`FileVersions` in `ion_repl/chat.rs`, cleared by `/clear`). New files need no prior read.
- **Environment scrub.** `bash_exec`, manifest `ProcessTool`s, governed verification, and the PDF text-extraction child call `scrub_and_allowlist_env` (`src/tooling/env_scrub.rs`): clear the environment, then re-add `ENV_ALLOWLIST` plus any per-tool allowlist. `PiAdapter::verify` (`impulse-ion/src/pi_adapter.rs`) does not scrub; that child inherits the parent environment.
- **Bounded tool loop.** One exchange is capped at `ION_DEFAULT_MAX_ROUNDS` (10), `ION_DEFAULT_WALL_CLOCK` (180 s) and `ION_DEFAULT_MAX_TOOL_CALLS` (50 calls across all rounds, checked before each call runs) in `src/loop_contract.rs`. On any trip the conversation history is left unchanged.
- **Credentials.** The macOS Keychain provider calls the Security framework in-process (`src/credentials/keychain.rs`).

---

## Desktop shell

The graphical cockpit is **Dioxus Desktop + xterm.js**. It renders and invokes daemon contracts. Authoritative workbench state and policy stay in the Rust daemon. The ratatui TUI and the CLI stay first-class.

`impulse-gui` / egui is frozen legacy code. It is excluded from the workspace and receives compile-maintenance only. It is not a functional fallback. Deletion waits on an honest release state (Track A / R1 in [`docs/plans/EGUI-DECOMMISSION.md`](docs/plans/EGUI-DECOMMISSION.md)), which is the same plan dated 2026-08-17. Deletion does not wait on a Dioxus-parity gate. Do not add product code, views, or egui rendering surface to `impulse-gui` or `impulse-term`.

The Tauri-shaped adapter (`legacy-tauri-runtime` / `tauri-runtime` on `impulse-desktop`) is a separate compatibility layer. It stays until a packaged Dioxus app proves the host bridge and the required operator workflows. That acceptance is not the egui deletion gate.

- [`docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`](docs/spec/DESKTOP-SHELL-ARCHITECTURE.md)
- [`docs/spec/DESKTOP-STACK-TRADEOFFS.md`](docs/spec/DESKTOP-STACK-TRADEOFFS.md)
- [`docs/decisions/0008-dioxus-desktop-host.md`](docs/decisions/0008-dioxus-desktop-host.md) (active)
- [`docs/decisions/0007-desktop-shell-stack.md`](docs/decisions/0007-desktop-shell-stack.md) (superseded Tauri-era ADR)
- [`docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`](docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md) is historical migration context, not the next product goal
- [`docs/plans/EGUI-DECOMMISSION.md`](docs/plans/EGUI-DECOMMISSION.md)

`impulse-gui` imports `eframe::egui::*` on the legacy compile path.

---

## Collaborative agentic coding

Before mutating code or docs, read [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md).

Required lane facts: owner, role, branch, worktree path, owned paths, blocked or shared paths, plan or spec link, acceptance criteria, verification commands, and a work card at `docs/plans/worktrees/<date>-<lane-slug>.md`.

Shared files that need explicit ownership include `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, docs indexes, `docs/validate_docs.py`, and protocol or spec docs. Parallel orchestrators run only when owned paths are disjoint or a handoff records the sequencing. Inspect `git status --short`, the current branch, and `git worktree list` before editing.

---

## Principles

1. **Return `Result`.** Every function returns `Result<T>`. `unwrap()` on a production path is rejected. `unwrap()` is acceptable in tests, in `Default` impls where failure is impossible, and in `main()` after argument parsing. `expect("msg")` is acceptable in `main()` and test setup. `thiserror` enums need a `#[error("...")]` on every variant and a `Display` test. `anyhow` errors chain `.context("what we were doing")` on I/O and parse. Every `Result`-returning function has at least one test that hits `Err`.
2. **Atomic writes.** Temp file plus rename. Temp names include PID and timestamp. Do not write the target path in place.
3. **Validate at boundaries.** Sanitize user-supplied IDs before they become filesystem paths or SQL. Validate protocol data on the socket. Allowlist table names for PRAGMA queries.
4. **Locked read-modify-write for shared state.** Every process that opens a project (the daemon and each direct-mode hook) keeps its own in-memory copy of `.impulse/` state, so never save that copy whole. A change to a shared file runs as one read-modify-write under `Storage::lock_exclusive`: re-read the file, apply only this change, write atomically, then refresh the in-memory copy from what was written (`State::update_live_state`, `State::update_config`, and the memory-candidate writers). The lock is not re-entrant, so a change must never start another locked update; take it through `State::lock_project_state`, which fails after a bounded wait instead of hanging. Pass a change, not a finished value built from the copy (`update_impulse_agent_permissions(|policy| ...)`). Daemon-only files (`GOVERNED_TASKS.json`, `PRODUCER_RESERVATIONS.json`) are still whole-file writes, safe only while the daemon is their single writer; the startup projection rewrite of `GENOME_PROJECTION.md` is not yet locked.
5. **Capability-based tools.** Deny by default. The registry order is: exists, capability check, parameter validation, execute.
6. **Review before apply.** Context injection defaults to review mode. Show what would be injected and let the operator decide.
7. **Simplest design that works.** Prefer editing an existing file over a new abstraction for a one-time operation.

---

## Architecture

**Execution surfaces**

- **Direct** — stateless per action (hooks). Read, process, write, exit.
- **Daemon** — long-running Unix socket for the TUI and the Dioxus cockpit. In-memory state with periodic sync.
- **Desktop** — Dioxus cockpit with an xterm.js bridge, backed by daemon and runtime state. It does not own a second policy store. Tauri-shaped command and event code is compatibility-only.
- **ratatui TUI** — terminal-native operator surface.

**IPC.** `PROTOCOL_VERSION` is 9 (`impulse_ops::DAEMON_PROTOCOL_VERSION`). The daemon speaks JSON lines on a Unix socket. Full spec: [`docs/IPC-PROTOCOL.md`](docs/IPC-PROTOCOL.md). Groups used by agents:

| Group | Endpoints | Purpose |
|-------|-----------|---------|
| Agent coordination | `AgentAssist` | Coordination with extracted-insight context |
| Agent specialized | `AgentReviewCode`, `AgentAnalyzeError`, `AgentSummarizePane` | Per-task assistance |
| Delegation | `RegisterDelegation`, `CompleteDelegation`, `ListDelegations` | Cross-agent delegation tracking |
| Conflict resolution | `GetConflictHistory`, `ClearResolvedConflicts` | File-conflict history |
| Agent pool | `GetAgentPool` | Sessions grouped by role |
| Governed tasks | `RegisterGovernedTask`, `GetGovernedTask`, `ListGovernedTasks`, `MutateGovernedTask`, `SubmitGovernedClaim`, `RunGovernedVerification`, `RunGovernedSupervisorReview`, `PromoteGovernedOutcome`, `DiscardGovernedStagedWorktree` | Revisioned task state, profiled producers, staged-worktree promotion and discard, operator acceptance |

Assist responses use `AgentAssistResult` (`recommendations`, `pane_summaries`). Review, analyze, and summarize use `AgentSpecializedResult`.

**Agent-turn lock (protocol v3 onward).** `SupervisorChat`, `AgentAssist`, `AgentReviewCode`, `AgentAnalyzeError`, and `AgentSummarizePane` share one cached `ImpulseAgent`. `try_lock_agent_for_turn` holds it under a Tokio mutex for one bounded query. A concurrent turn fails with `Busy { resource: agent_turn, retry_after_ms: 250 }`. Unrelated endpoint groups stay independent. Cancelling the handler drops the guard and leaves the cached agent in place. CLI harness subprocesses (`claude`, `codex`, `gemini`) use `kill_on_drop` and `DEFAULT_HARNESS_TIMEOUT` (120 s, `AgentError::HarnessTimedOut`). Socket, MCP (stdio and loopback TCP), and the other line readers use `daemon::read_bounded_line`, capped at `MAX_REQUEST_SIZE` (10 MiB). Protocol v4 added governed-task requests. Protocol v5 added profiled claim, verification, and Supervisor-review producers. Governed Supervisor review is API-only, history-free, tool-free, and temperature-zero. A generic external harness configuration fails closed before spawn because it cannot provide a structurally read-only turn.

**Profiled governed producers.** A Dioxus Builder launch on `rust_workspace_v1` supplies exact acceptance criteria and can register only from the clean canonical Git worktree root at a committed `HEAD`. The daemon derives the Worker and Verifier records, verifies the claimed commit in a detached worktree with fixed Rust commands, and binds the Supervisor envelope to the task revision, claim, verification, subject, and acceptance-criteria digest. Injected variables: `IMPULSE_PROJECT_ID`, `IMPULSE_GOVERNED_TASK_ID`, `IMPULSE_SOCKET_PATH`, `IMPULSE_CONTROL_CLI`. Ion also exposes `governed_submit_claim`. Governed panes run `"$IMPULSE_CONTROL_CLI" --daemon governed-claim`, `governed-verify`, or `governed-review`. `--daemon` is a global flag and precedes the subcommand. Verification runs host-trusted Rust code in a detached worktree with a scrubbed environment, timeouts, output digests, and process-group cleanup. It is not an OS sandbox. Dioxus exposes operator-only Promote and Discard (ADR-0019) and still shows terminal guidance for claim, verify, and review. Only an operator decision accepts a governed task. Actor IDs are provenance. ADR-0018 classifies operator connections with peer credentials plus a per-daemon-run capability; same-UID discovery of that capability sits outside the control.

`PRODUCER_RESERVATIONS.json` wraps verification, Supervisor review, and staged-worktree promotion. The side effect and its receipt persist inside one `with_reservation` closure. Interrupted attempts reconcile to `NeedsRerun`. A panic leaves the entry open. The journal does not roll back arbitrary effects. See `impulse-rs/src/daemon/governed_wiring.rs` and `impulse-rs/src/state/producer_reservation.rs`.

ADR-0020's state and request types cover candidate decisions, `MEMORY.jsonl`, and `GENOME_PROJECTION.md`. Its daemon decision endpoint, Dioxus Memory Promote/Dismiss controls, and Ion memory integration are still deferred. `GENOME.md` stays hand-curated. ADR-0020 is recorded as `review`. Those controls are separate from the live ADR-0019 staged-worktree Promote and Discard actions. Source pointers: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md).

**`.impulse/` files.** `impulse-rs init` gitignores runtime state and leaves durable project inputs committable (`REPO_RUNTIME_GITIGNORE_ENTRIES` in `impulse-rs/src/handlers/config.rs`). It does not add a blanket `.impulse/` rule.

| File | Role | `impulse init` gitignore |
|------|------|--------------------------|
| `HISTORY.jsonl` | Append-only session log | No (committable) |
| `GENOME.md` | Hand-curated decisions and preferences | No |
| `config.json` | Runtime configuration | No |
| `impulse-capabilities.json` | Capability input | No |
| `MEMORY.jsonl` | Promoted-memory log (ADR-0020) | No |
| `GENOME_PROJECTION.md` | Reviewable projection of that log | No |
| `LIVE_STATE.json` | Active session state | Yes |
| `retrieval.db` | Rebuildable search index | Yes |
| `GOVERNED_TASKS.json` | Governed task records and idempotency receipts | Yes |
| `PRODUCER_RESERVATIONS.json` | Producer intent, receipts, interrupted-attempt recovery | Yes |
| `DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json` | Ambiguous launch/exit mutations awaiting reconciliation | Yes |
| `blackboard.db` | Off-context results and spilled tool output, TTL-purged (ADR-0023) | Yes |
| `MEMORY_CANDIDATES.json` | Private accepted-run review queue | Yes |

---

## Code conventions

| Area | Convention |
|------|-----------|
| Errors | `thiserror` enums, `anyhow` application errors, `Result<T>` everywhere |
| File I/O | Atomic temp file plus rename, unique temp names |
| State | In-memory copy + locked re-read-modify-write per change |
| Naming | `PascalCase` types, `snake_case` functions, `SCREAMING_SNAKE` constants |
| Tests | `mod tests` in the owning file; integration tests use a `DaemonGuard` |
| CLI | New commands are `clap::Subcommand` variants of `Commands` in `impulse-rs/src/cli.rs`. `direct_dispatch.rs` and `daemon_dispatch.rs` match the same variant. Daemon-only commands refuse in direct mode. A command also exposed from `ion` needs an `IonCommand` variant in `src/bin/ion.rs` whose handler calls the one shared function in `handlers::`. |
| Tools | New dynamic tools implement `DynamicTool` in `src/tooling/`. |

`thiserror` shape:

```rust
#[derive(Error, Debug)]
enum SessionError {
    #[error("No session with ID {id}")]
    NotFound { id: String },
    #[error("Failed to write state: {source}")]
    StateWrite { #[from] source: std::io::Error },
}
```

`anyhow` I/O and parse use `.context(...)`. Preconditions use `bail!` / `ensure!`.

```rust
let content = fs::read_to_string(&path).context("Failed to read config file")?;
ensure!(!id.is_empty(), "Session ID must not be empty");
```

A `Serialize + Deserialize` type has a round-trip test: `deserialize(serialize(value)) == value`. `src/error.rs` shows the `Display` pattern in `test_agent_error_missing_api_key_display`. A test that only prints is not a test; every `#[test]` asserts observable behavior.

**Property-based testing.** Use `proptest` when a property should hold for any valid input, not one fixture. `proptest` is already a dev-dependency of `impulse-rs` (`Cargo.toml`). Add it to a crate's dev-dependencies the first time that crate uses it.

**Test helpers.** `DaemonGuard` for the in-process daemon lives in `src/integration_tests.rs`. Files under `impulse-rs/tests/` define their own guards. `EchoTool` and `WriteTool` are test tools in `src/tooling/registry.rs`. When a helper is used by three or more modules, move it to a shared `#[cfg(test)]` module.

**Allow and unsafe.**

| Suppression | Rule |
|-------------|------|
| `#[allow(dead_code)]` | Include `// dead_code: <reason>`. Grep for callers first; delete the item if nothing calls it. |
| `#[allow(clippy::too_many_arguments)]` | Temporary. Add `// TODO: refactor to struct params`. |
| Other `#[allow(clippy::*)]` | Include `// clippy: <reason>`. |
| `#![allow(...)]` | Not used in new files. |

Any `unsafe` block has a `// SAFETY:` comment, validates preconditions before the block, and has a test that exercises the unsafe path.

Name tests `test_<function>_<scenario>_<expected_result>`.

### Feature flags

From `impulse-rs/Cargo.toml` `[features]` (read 2026-10-24). `calc`/`exec` use the `monty` crate and are not feature-gated.

| Flag | Crate | Default | Enables |
|------|-------|---------|---------|
| `office-support` | `impulse-rs` | Yes | Excel, Word, CSV, and PDF document read/write (`document_read` included) |
| `photon-subagent` | `impulse-rs` | Yes | Ion's read-only `photon` tool (`min-agent`) |
| `datafusion-support` | `impulse-rs` | No | DataFusion analytics |
| `desktop-app` | `impulse-desktop` | No | Dioxus desktop binary and host adapter |
| `native-macos` | `impulse-desktop` | No | macOS island bindings |
| `legacy-tauri-runtime` | `impulse-desktop` | No | Tauri compatibility adapter. `tauri-runtime` is an alias. |

`default = ["office-support", "photon-subagent"]`. The others are opt-in.

---

## Build, test, and release

### Isolated target directory

Every Cargo build, test, check, and clippy invocation sets its own `CARGO_TARGET_DIR`, for example `CARGO_TARGET_DIR=target/agent-<name>` from `impulse-rs/`. That path sits under `target/`, which `.gitignore` already ignores. The collaboration guide's per-lane directory (`~/.cargo-target-lanes/<lane-slug>`) is the same rule. A shared global target directory has caused false test failures: one lane's stale artifacts fail another lane's gate.

### Verification gate

Run from `impulse-rs/` before a commit. Replace `<name>` with the lane or agent name.

```bash
cd impulse-rs
export CARGO_TARGET_DIR=target/agent-<name>
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Record package-level passed, ignored, and failed totals from that run. Default tests use tracked sources and fixtures and stay portable across clones, linked worktrees, and CI.

This gate does not prove a real provider-backed Ion round trip, cross-platform Windows behavior, or a generalized role system that is not implemented. The feature-gated Dioxus binary has its own host-readiness smoke (`npm run host:smoke` in `impulse-rs/impulse-desktop/`). Re-run the gate on the current checkout before citing an aggregate. Keep chronology in Git history, not in this file.

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) is narrower than this gate. From `impulse-rs/` it runs `cargo test` (root package, default features), `cargo test --no-default-features --lib`, `cargo clippy -- -D warnings` (not `--workspace --all-targets`), `cargo fmt --check`, `cargo build --release`, and `cargo build --no-default-features`, plus a macOS `desktop-app` release check. When a change touches feature-gated code, also run `cargo clippy --workspace --all-targets --no-default-features -- -D warnings` with the same isolated `CARGO_TARGET_DIR`.

Crate checks use `-p` from `impulse-rs/`. They do not replace the workspace gate.

```bash
cargo test -p impulse-term
cargo test -p impulse-ops
cargo test -p impulse-desktop
cargo check -p impulse-desktop --features desktop-app --bin impulse-desktop
```

Mid-session health check (still with an isolated `CARGO_TARGET_DIR`):

```bash
cd impulse-rs && cargo check && cargo test --bins -- --quiet
```

Count tests from the command output, not from a remembered total:

```bash
cargo test --workspace 2>&1 | grep "test result:" | awk '{sum += $4} END {print "Total: " sum " passed"}'
```

Pre-commit checklist: the four gate commands above; no new `#[allow(...)]` without the justification comment; new serde types round-trip; new `Result` functions have an `Err` test.

### Run

```bash
cd impulse-rs
export CARGO_TARGET_DIR=target/agent-<name>
cargo build --workspace
cargo run -- daemon
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop
cargo run -- run
cargo run --bin ion
```

The packaged executable is `impulse-rs`. Governed producer commands are `impulse-rs --daemon governed-claim`, `governed-verify`, and `governed-review`.

### Release

Tagged releases publish the `impulse-rs` CLI for macOS (Intel and Apple silicon) and Linux (x86_64 and ARM64), with `SHA256SUMS.txt`. The release workflow builds each CLI, smoke-tests the renamed executable's root, daemon, and MCP serve help parsers on its native architecture, and checks the four-artifact checksum set. GitHub Release publication runs for pushed `v*` tags.

Desktop `.app` bundles and DMGs are unavailable until the Dioxus packaging gates in [EGUI decommission, Track A / R1](docs/plans/EGUI-DECOMMISSION.md) are met. `impulse-rs/scripts/build-macos-app.sh` exits with an explanation before it builds. The CLI-only release path does not accept the Dioxus host and does not retire the Tauri adapter. There is no hosted application deploy; Impulse runs locally.

### Doc contract check

After a documentation contract change:

```bash
python3 docs/validate_docs.py --contract
python3 docs/validate_docs.py --all
```

---

## Testing standards

| What | Required evidence |
|------|-------------------|
| Happy path | At least one assertion that the function works |
| Error path | One `Err` test per `Result`-returning function |
| Boundaries | Empty input, zero, and max where they apply |
| Serde | Round-trip equality |
| `thiserror` | `Display` contains the expected text |

### Test density (source count, 2026-10-23)

One table. Counted from source on 2026-10-23. A test is a `#[test]` or `#[tokio::test]` attribute (regex `#\[(?:\w+::)*test\b`). Lines are physical lines in the path, including comments, blanks, and the tests. Density is tests per 1,000 lines. This count is not a `cargo test` total. It replaces the 2026-04-01 figures previously in this file and the 2026-06-14 figures previously in `CLAUDE.md`.

| Category | Target | Measured 2026-10-23 | Path |
|----------|--------|---------------------|------|
| Core (state, daemon, agent) | 3.0 | 17.6 (540 tests / 30,685 lines) | `src/state`, `src/daemon`, `src/agent` |
| Handlers | 2.0 | 29.5 (459 tests / 15,581 lines, 16 of 22 files) | `src/handlers` |
| Tooling | 2.0 | 21.3 (197 tests / 9,233 lines) | `src/tooling` |
| UI/TUI | 1.0 | 5.2 (30 tests / 5,786 lines) | `src/ui` |
| Integration | Every stable CLI command | 98 `#[test]` attributes in 12 files | `impulse-rs/tests/` |

Handler files with no `#[test]` on that date: `build.rs`, `office.rs`, `plugin_handlers.rs`, `retrieval.rs`, `stewardship_handlers.rs`, `tooling_handlers.rs`. `stewardship_handlers.rs` is 381 lines of command dispatch, so an untested handler file is not automatically a thin print wrapper. Test decision logic where it exists. A print-only test still does not count. [`docs/spec/TEST-TRACEABILITY.md`](docs/spec/TEST-TRACEABILITY.md) (updated 2026-07-15) is the qualitative story map; its narrative is older than this count. Workspace pass/fail totals come from the gate command, not from this table. `impulse-gui` is outside the workspace gate; compile it only when a legacy maintenance change requires it.

The integration row counts test functions. It does not by itself prove every stable CLI command has an end-to-end test.

Coverage that still matters: daemon versus direct dispatch for each stable command; agent failures across provider, credential, timeout, and daemon boundaries; guard and confirmation behavior at real dispatch boundaries, including process exit; injection and stewardship evidence across retrieval, review, persistence, and output; shared handler helpers under cancellation.

Error-handling grep, from the repo root, for review rather than as a gate:

```bash
git grep -n "fs::read\|fs::write\|fs::remove" -- "*.rs" | grep -v "context\|test"
git grep -n "\.unwrap()" -- "*.rs" | grep -v "#\[test\]\|mod tests\|fn main\|impl Default"
```

---

## Guardrails

- Dynamic tools are deny-by-default. Registry order: exists, capability, parameters, execute.
- Ion confirmation and `decide_approval` are described under Layout (Ion REPL). Block-tier approval is the literal `CONFIRM`.
- Child processes that scrub the environment are listed under Ion REPL. Do not log API keys, URL credentials, or request paths. Logs that mention a base-URL override show the origin only.
- Context injection stays in review mode until the operator accepts it.
- Do not write secret values, token paths, or credential-file locations into docs, commits, or prompts.
- Worktree hook warnings cover force-push, mass deletion (10+ files), unpushed commits, and uncommitted changes of 50+ files. Before a mass removal: `git diff --cached --name-status`, check other worktrees, and `git branch backup-pre-delete` if you need a recovery point. `rm -rf` on project directories, `git clean -fd`, `git reset --hard`, and force-push to `main` need explicit user confirmation.

---

## Environment variables

| Variable | Purpose |
|----------|---------|
| `IMPULSE_SESSION_ID` | Current session ID |
| `IMPULSE_HOME` | Custom `.impulse/` directory |
| `IMPULSE_SOCKET_PATH` | Custom Unix socket path |
| `IMPULSE_PROJECT_ID` | Injected governed-pane project id |
| `IMPULSE_GOVERNED_TASK_ID` | Injected governed task id |
| `IMPULSE_CONTROL_CLI` | Injected path of the `impulse-rs` executable for governed panes |
| `ANTHROPIC_API_KEY` | Daemon and Ion chat credential. The name only; do not record a value. |
| `IMPULSE_MODEL` | Chat model override. Final per-step choice stays in harness-owned `decide_step_model` (ADR-0015). |
| `ANTHROPIC_BASE_URL` | Anthropic origin override |
| `OPENAI_BASE_URL` | OpenAI origin override |
| `MINIMAX_BASE_URL` | MiniMax origin override |
| `CARGO_TARGET_DIR` | Isolated Cargo output directory for this agent or lane |

`*_BASE_URL` is the origin the provider appends its own path to (`/v1/messages`, `/v1/chat/completions`, `/v1/text/chatcompletion_v2`). A blank value is unset. A value with no `http://` or `https://` scheme, no host, or a query or fragment fails the request and names the variable. It does not fall back to the public API. A trailing `/v1` is removed when the provider path already starts with `/v1/`, so an OpenAI-SDK-style base does not become `/v1/v1/...`. A path prefix such as `https://openrouter.ai/api` is kept.

Precedence is `BaseProvider::with_base_url`, then the env var, then the canonical origin. The env var is not read when a base URL is configured. Providers do not follow redirects: a 3xx response is an error that names the redirect origin. Anthropic sends its key in `x-api-key` and accepts URL credentials as Basic auth. OpenAI and MiniMax refuse URL credentials because their key is the `Authorization` header. Logs show the origin only.

Any `http(s)` origin is accepted (no host allowlist). Cleartext `http://` to a non-loopback host is a warning. Loopback is `localhost` and loopback IP addresses (`127.example.com` is not loopback). These variables change transport, not the step model (ADR-0015). Typed endpoints are the ADR-0022 proposal (`review`).
