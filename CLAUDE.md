# CLAUDE.md — Impulse

> Local control plane and harness manager for AI coding agents.
> Product north star: [`VISION.md`](VISION.md)
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Collaboration playbook: [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=control-plane foundations + governed runtime producers + accepted-run review candidates; Next=stronger same-user actor authorization + full launched Builder/Supervisor proof; Later=memory promotion/dismissal daemon/UI/runtime wiring + general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.

---

## What Impulse Is

Impulse is a terminal-native **local control plane and harness manager**. It runs external coding-agent harnesses such as Claude Code and Codex, and it also contains Ion, an Impulse-native coding runtime. Memory is a first-class platform service alongside process supervision, tools, telemetry, messaging/handoffs, policy, credentials, artifacts, and verification.

```
 Dioxus cockpit / TUI / CLI
              │
              ▼
 Impulse daemon + shared control-plane contracts
              │
        runtime/PTY boundaries
        ├── external coding CLIs
        └── Ion native runtime
```

Impulse can strongly control launch conditions, working-directory/project scoping, process lifecycle, exposed tools, credentials, and observable policy gates. Structural filesystem enforcement depends on the selected runtime or sandbox. Impulse cannot replace hidden prompts, proprietary reasoning loops, or unsupported internals of third-party CLIs.

### Stable Product Identities

- **Role** — behavioral obligations, permissions, tools, context, and completion rules. It is independent of runtime and UI position.
- **Runtime** — the execution engine or harness integration (Claude Code, Codex, Ion, or another CLI/API agent).
- **Agent instance** — one running identity assigned a role, runtime, project/workspace target, and current status.
- **Session** — bounded recorded work by an agent instance; it is not the process or pane.
- **Task** — an assignment plus acceptance/verification criteria; one session may touch more than one task and a task may span sessions.
- **Pane** — a cockpit viewport or terminal attachment; never a security or policy boundary.

The live code has registry-driven desktop launch identity, a supervisor-specific `SupervisorPermissionPolicy`, and a narrow coordinator/worker `AgentRole`; it does **not** yet have the generalized role contract above. A common runtime-adapter trait and capability-negotiation protocol remain future ADR work. Do not describe all runtimes as structurally equivalent or fully governed.

---

## Collaborative Agentic Coding

Read [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md) before mutating the repository.

Required operating facts for any non-trivial lane:

- owner, role, branch, and worktree path
- owned files/directories and blocked/shared paths
- plan/spec link and acceptance criteria
- verification commands
- lane work card under `docs/plans/worktrees/<date>-<lane-slug>.md`

Multiple orchestrators may run in parallel only when their lanes have disjoint ownership or an explicit handoff/integration lane. Do not infer ownership from silence.

---

## Principles

### 1. Never Panic, Always Return Result

Every function returns `Result<T>`. No `unwrap()` on production paths. Use `thiserror` for error enums, `anyhow` for application errors.

**Error handling rules:**
- `thiserror` enums: every variant must have a `#[error("...")]` with meaningful context. Test `Display` output in `mod tests`.
- `anyhow` usage: always chain `.context("what we were doing")` — never bare `?` on I/O or parse operations.
- `unwrap()` is only acceptable in: tests, `Default` impls where failure is impossible, and `main()` after argument parsing.
- `expect("msg")` is acceptable in `main()` and test setup — never in library code.
- Every `Result`-returning function must have at least one test exercising the `Err` path.

### 2. Atomic Writes

All file operations use temp file + rename. Temp file names include PID + timestamp to avoid collisions. Never write directly to the target path.

### 3. Input Validation at Boundaries

Sanitize user-supplied IDs before using as filesystem paths or SQL components. Validate protocol data on socket boundaries. Allowlist table names for PRAGMA queries.

### 4. Dirty Flag State Management

In-memory state tracks whether it's been modified. Sync to disk only when dirty. Always persist on Drop/exit.

### 5. Capability-Based Tool Access

Dynamic tools use deny-by-default capabilities. The registry enforces: exists → capability check → param validation → execute.

### 6. Review Before Apply

Context injection defaults to review mode — surface what *would* be injected and let the user decide. Never auto-inject without consent.

### 7. Build Optimal, Not Just Build

Before implementing, consider alternative approaches. Choose the simplest solution that works. Avoid over-engineering.

---

## Architecture

**Workspace (Rust-first control plane with a Dioxus cockpit):**
- `impulse-rs/` — main CLI, daemon, and ratatui TUI (`impulse-rs` binary, the `default-run`). The library crate `impulse_rs` (`src/lib.rs`) also backs the `ion` binary (`src/bin/ion.rs`): bare `ion` opens a rustyline REPL (`src/ion_repl/`; slash commands in `router.rs`: `/help`, `/quit`, `/clear`, `/verify`, `/tools`, `/allow`, `/loop`; history in `.impulse/ion_history`). Ion is a full coding agent, not a verify console: free text goes to `chat::ChatState`, which runs `llm_backends::Agent::chat_with_tools` with the session's `ReplToolRegistry` (`registry.rs`: `ion_verify`, `governed_submit_claim`, `document_read` under `office-support`, plus `file_read`, `file_write`, `bash_exec`, `memory_search`, and `genome_read` bridged from `src/tooling`). `ion verify` and `impulse-rs ion-verify` share `handlers::ion::handle_ion_verify`. Ion's design history and roadmap live in [`impulse-ion/TUI_SPEC.md`](impulse-rs/impulse-ion/TUI_SPEC.md); the current safety invariants are:
  - **Confirmation gate:** `CONFIRMATION_REQUIRED_TOOLS` (`bash_exec`, `file_write`, `governed_submit_claim`) run only after the user approves, and a decline returns before `ReplTool::run`. `bash_exec` commands and `file_write` content are scanned by `src/guardrail` first; a Block-tier match requires the literal `CONFIRM` rather than `y` (`decide_approval`), and gated tools execute only while holding a minted `ApprovalGrant`. The scan covers the tool call's own arguments, not the prompt or injected context.
  - **Sandbox:** bridged tools write only under the repo root and read only under the repo root plus paths granted with `/allow` (`ReplContext::sandbox_tool_context`); `memory_search` and `genome_read` may also read an explicitly set `IMPULSE_HOME`, which they validate themselves.
  - **Environment scrub:** `bash_exec`, manifest `ProcessTool`s, governed verification, and the PDF text-extraction child clear the environment and re-add only `ENV_ALLOWLIST` plus any per-tool allowlist (`src/tooling/env_scrub.rs`). The Pi launcher in `impulse-ion` deliberately keeps the full environment because it is developer-configured and needs its own credentials.
  - **Bounded tool loop:** one exchange is capped at `DEFAULT_MAX_TOOL_ROUNDS` (10) and `DEFAULT_TOOL_LOOP_TIMEOUT` (180 s), both sourced from `loop_contract`; on either error the conversation history is left unchanged.
  - **Credentials:** the macOS Keychain provider uses the native Security framework, so secrets never appear in a subprocess argv.
- `impulse-rs/impulse-ops/` — shared control-plane protocol and models: supervisor policy/actions, telemetry, workbench snapshots, artifacts, daemon requests/responses, and the agent-platform registry
- `impulse-rs/impulse-term/` — PTY/session/context core (PTY + vt100 + WriteQueue + context bridge)
- `impulse-rs/impulse-desktop/` — Dioxus cockpit, typed host bridge, workspace registry, PTY runtime integration, and desktop MCP surface; it projects backend truth rather than owning it
- `impulse-rs/impulse-ion/` — Ion harness contract v0 (transport-agnostic `HarnessRequest`/`HarnessResponse` types + `PiAdapter`, the Rust-side caller of harness #2/Pi-on-MiniMax; drives `impulse-rs ion-verify`, see `impulse-ion/TUI_SPEC.md` for the ion-cli agent roadmap)

**Legacy:** `impulse-gui` / egui is frozen. It receives compile-maintenance only until the Dioxus desktop host reaches parity. Tauri-shaped code is also compatibility-only, not a new product scaffold target.

**Execution surfaces:**
- **Direct mode** — stateless, per-action (for hooks). Read → process → write → exit.
- **Daemon mode** — long-running Unix socket authority for the TUI and Dioxus cockpit. In-memory state with periodic sync.
- **Desktop mode** — Dioxus Desktop cockpit with xterm.js terminal bridge, backed by Rust daemon/runtime state. Tauri-shaped command/event code is compatibility-only.

**IPC Protocol (PROTOCOL_VERSION = 9):**

The daemon exposes a JSON-line Unix socket protocol. Key endpoint groups:

| Group | Endpoints | Purpose |
|-------|-----------|---------|
| Agent Coordination | `AgentAssist` | AI coordination with context enrichment via extracted insights |
| Agent Specialized | `AgentReviewCode`, `AgentAnalyzeError`, `AgentSummarizePane` | Per-task agent assistance |
| Delegation | `RegisterDelegation`, `CompleteDelegation`, `ListDelegations` | Phase 1B cross-agent delegation tracking |
| Conflict Resolution | `GetConflictHistory`, `ClearResolvedConflicts` | File conflict tracking and resolution |
| Agent Pool | `GetAgentPool` | All sessions grouped by role (Phase 2B) |
| Governed Tasks | `RegisterGovernedTask`, `GetGovernedTask`, `ListGovernedTasks`, `MutateGovernedTask`, `SubmitGovernedClaim`, `RunGovernedVerification`, `RunGovernedSupervisorReview`, `PromoteGovernedOutcome`, `DiscardGovernedStagedWorktree` | Durable revisioned task state plus daemon-owned profiled producers, staged-worktree materialization/promotion/discard (operator-class), and operator-required acceptance |

Responses use `AgentAssistResult` (with `recommendations` + `pane_summaries`) or `AgentSpecializedResult` (for review/analyze/summarize). Full protocol spec: [`docs/IPC-PROTOCOL.md`](docs/IPC-PROTOCOL.md).

**Daemon agent-turn invariant (protocol v3 onward):** `SupervisorChat`, `AgentAssist`, `AgentReviewCode`, `AgentAnalyzeError`, and `AgentSummarizePane` share one cached `ImpulseAgent`. `try_lock_agent_for_turn` holds it in place under a Tokio mutex for one bounded query. A concurrent turn fails fast with typed `Busy { resource: agent_turn, retry_after_ms: 250 }` instead of queueing past the client's response budget; unrelated endpoint groups stay independent; a cancelled handler releases the guard without removing the cached instance. (An earlier checkout/checkin design was replaced because it could double-initialize the agent and lose `session_history`, `recommendations`, and `pane_summaries` on cancellation.) CLI harness subprocesses (`claude`, `codex`, `gemini`) run with `kill_on_drop` under `DEFAULT_HARNESS_TIMEOUT` (120 s, `AgentError::HarnessTimedOut`). Request lines on the daemon socket, the MCP server (stdio and loopback TCP), and the other line-protocol readers go through `daemon::read_bounded_line`, capped at `MAX_REQUEST_SIZE` (10 MiB); an oversized line closes the connection. Protocol v4 added the daemon-owned governed-task request family and snapshot state. Protocol v5 adds daemon-owned profiled claim, verification, and Supervisor-review producers without weakening this turn invariant. Governed Supervisor review is API-only, history-free, tool-free, and temperature-zero; generic external harness configuration fails closed before spawning because it cannot provide a structurally read-only turn.

**Current profiled governed-producer invariant:** A Dioxus Builder launch using `rust_workspace_v1` supplies exact acceptance criteria and can register only from the clean canonical Git worktree root at a committed `HEAD`. The daemon derives the Worker and Verifier records, verifies the claimed commit in a detached worktree with fixed Rust commands, and binds a strict Supervisor envelope to the task revision, claim, verification, subject, and acceptance-criteria digest. The CLI uses injected `IMPULSE_PROJECT_ID`, `IMPULSE_GOVERNED_TASK_ID`, `IMPULSE_SOCKET_PATH`, and `IMPULSE_CONTROL_CLI`; Ion additionally exposes `governed_submit_claim`. The packaged executable is `impulse-rs`, while governed panes invoke `"$IMPULSE_CONTROL_CLI" --daemon governed-claim`, `"$IMPULSE_CONTROL_CLI" --daemon governed-verify`, or `"$IMPULSE_CONTROL_CLI" --daemon governed-review`; `--daemon` is a global flag and must precede the subcommand. Verification executes host-trusted Rust code and is not an OS sandbox. Dioxus exposes operator-only Promote and Discard controls (ADR-0019) and still shows terminal command guidance for claim, verify, and review, which remain CLI-driven. Actor IDs alone are provenance, not authorization; ADR-0018 classifies operator connections using peer credentials plus a per-daemon-run capability, with deliberate same-UID token discovery outside its protection.

Durable `PRODUCER_RESERVATIONS.json` entries now wrap verification, Supervisor review, and ADR-0019 staged-worktree promotion. The side effect and its governed-task receipt persist inside one `with_reservation` closure; interrupted attempts reconcile to `NeedsRerun`. A panic leaves the entry open, and the journal does not roll back arbitrary effects or guarantee exactly-once execution.

ADR-0020's state and request-type contracts implement candidate decisions, `MEMORY.jsonl`, and a separate `GENOME_PROJECTION.md`; its daemon decision endpoint, Dioxus Memory Promote/Dismiss controls, and Ion memory integration remain deferred. `GENOME.md` stays hand-curated. ADR-0020 remains recorded as `review`. See the [canonical contract](docs/spec/RUST-CANONICAL-CONTRACT.md) for source pointers and the distinction from live staged-worktree Promote/Discard controls.

**Data lives in `.impulse/`:**
- `HISTORY.jsonl` — append-only session log (committed)
- `GENOME.md` — permanent decisions and preferences (committed)
- `LIVE_STATE.json` — active session state (ephemeral)
- `config.json` — runtime configuration
- `GOVERNED_TASKS.json` — daemon-owned governed task records and idempotency receipts
- `PRODUCER_RESERVATIONS.json` — durable producer intent, receipt references, and interrupted-attempt recovery
- `DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json` — bounded ambiguous launch/exit mutations awaiting daemon reconciliation
- `retrieval.db` — search index (rebuildable)

---

## Code Style

| Convention | Rule |
|------------|------|
| Error handling | `thiserror` enums + `anyhow` application errors |
| File I/O | Atomic (temp + rename), unique temp names |
| State | `RwLock` + dirty flag + sync on Drop |
| Naming | `PascalCase` structs, `snake_case` functions, `SCREAMING_SNAKE` constants |
| Testing | Unit tests in `mod tests` per file, integration tests with `DaemonGuard` RAII |
| Feature flags | `office-support` (default), `monty-support`, `datafusion-support` (opt-in) |

---

## Testing Standards

### Test Quality Bar

Every test must assert observable behavior — not just "doesn't panic." Tests that only `println!` output without assertions are not acceptable. Every `#[test]` function must contain at least one `assert!`, `assert_eq!`, `assert_ne!`, or `assert!(result.is_err())`.

### Required Test Patterns

| Pattern | When Required | Example |
|---------|--------------|---------|
| **Happy path** | Every public function | `assert_eq!(parse("valid"), Ok(expected))` |
| **Error cases** | Every function returning `Result<T>` | `assert!(parse("").is_err())` |
| **Boundary conditions** | Numeric inputs, collections, strings | Empty vec, zero, max value, empty string |
| **Serde round-trip** | Every type with `Serialize + Deserialize` | `assert_eq!(from_json(to_json(&val)), val)` |
| **Display/From impls** | Every `thiserror` enum | `assert!(format!("{}", err).contains("expected text"))` |

### Serde Round-Trip Requirement

All types deriving `Serialize` and `Deserialize` must have a round-trip test proving `deserialize(serialize(value)) == value`. This catches field renames, missing defaults, and `#[serde(flatten)]` breakage. Pattern:

```rust
#[test]
fn round_trip_my_type() {
    let original = MyType::default();
    let json = serde_json::to_string(&original).unwrap();
    let recovered: MyType = serde_json::from_str(&json).unwrap();
    assert_eq!(original, recovered);
}
```

### Unsafe Code Policy

All `unsafe` blocks must have:
1. A `// SAFETY:` comment documenting every invariant the block relies on
2. Precondition validation before the unsafe call (never inside the block)
3. A dedicated test that exercises the unsafe path (not just the precondition checks)

### `#[allow(...)]` Policy

Lint suppressions must be justified. Rules:

| Suppression | Acceptable When | Must Include |
|-------------|----------------|--------------|
| `#[allow(dead_code)]` | Serde deserialization fields, Phase-gated features | `// dead_code: <reason>` comment |
| `#[allow(clippy::too_many_arguments)]` | Temporary — track in a cleanup issue | `// TODO: refactor to struct params` comment |
| `#[allow(clippy::*)]` (other) | False positive or intentional design | `// clippy: <reason>` comment |
| `#![allow(...)]` (file-level) | Never acceptable in new code | Must be broken into per-item allows |

New `#[allow(dead_code)]` requires proof: grep the codebase for callers first. If truly dead, delete it instead of allowing it.

### Property-Based Testing

Use `proptest` for functions with combinatorial input spaces. Add `proptest` as a `[dev-dependencies]` entry when first used.

**When to use:** any function where behavior should hold for ANY valid input, not just specific test cases.

```rust
use proptest::proptest;

// Path sanitization: never produces traversal sequences
proptest! {
    #[test]
    fn test_sanitize_path_never_contains_traversal(path in "[a-zA-Z0-9/_.-]+") {
        let result = sanitize_path(&path).unwrap();
        prop_assert!(!result.contains(".."));
        prop_assert!(!result.contains("//"));
    }
}

// Config round-trip with random data
proptest! {
    #[test]
    fn test_config_roundtrip_random(
        sessions in prop::collection::vec("[a-z]+", 0..10),
        max_age in 1u64..1000,
    ) {
        let config = Config { sessions, max_age };
        let json = serde_json::to_string(&config)?;
        let recovered: Config = serde_json::from_str(&json)?;
        prop_assert_eq!(config, recovered);
    }
}
```

**Strategy reference:**
- `any::<u64>()` — any u64 value
- `"[a-zA-Z0-9]+"` — regex string strategy
- `prop::collection::vec(any::<String>(), 0..100)` — vector of random strings
- `(any::<u32>(), "[a-z]+")` — tuple combining strategies

### Test Helpers

Centralize shared test utilities. Do not duplicate factory functions across modules.

| Helper Type | Location | Purpose |
|-------------|----------|---------|
| State factories | `#[cfg(test)]` in owning module | `test_state() -> (TempDir, Arc<State>)` |
| Mock tools | `src/tooling/` test module | `EchoTool`, `WriteTool` |
| Daemon guards | `src/integration_tests.rs` | `DaemonGuard` RAII cleanup |
| Assertion helpers | Near usage site | `assert_error_contains()` |

When a helper is used by 3+ modules, extract to a shared `#[cfg(test)]` module.

### Test Naming Convention

Use descriptive names: `test_<function>_<scenario>_<expected_result>`

```rust
// Good
#[test] fn test_parse_config_empty_input_returns_default() { ... }
#[test] fn test_guard_evaluate_blocked_action_returns_exit_1() { ... }
#[test] fn test_agent_error_display_includes_provider_name() { ... }

// Bad
#[test] fn test_parse() { ... }
#[test] fn test_guard_2() { ... }
```

### Test Density Targets

| Module Type | Target | Current (as of 2026-06-14) |
|-------------|--------|---------|
| Core (state, daemon, agent) | 3.0 tests/KLOC | ~1.5 (state ~80 tests, agent harness +24, daemon protocol +2) |
| Handlers | 2.0 tests/KLOC | ~32 tests/KLOC (362 tests across 12/19 files, 11,183 LOC) — target exceeded |
| Tooling | 2.0 tests/KLOC | ~17.1 (84 tests, 4,920 LOC) |
| UI/TUI | 1.0 tests/KLOC | ~0.4 |

**Why tooling is well-tested (17.1/KLOC):** Dynamic tools execute arbitrary user commands. Failure → data corruption or security breach. High density catches parameter injection, output parsing bugs, and rollback failures.

**Why core is low (1.2/KLOC):** Core modules are critical but large. Trend toward 3.0/KLOC by adding: session lifecycle corner cases (rapid start/end, duplicate IDs), daemon reconnection/recovery (socket errors), agent harness error cases (missing context, malformed JSON).

**Why handlers now exceed target (~32/KLOC):** The dispatch routers and shared helpers are heavily covered — `direct_dispatch.rs` (117 tests), `common.rs` (84), `daemon_dispatch.rs` (69), `injection_handlers.rs` (18), `guard.rs` (17), `agent.rs` (16), `session.rs` (12), `config.rs` (12), `memory.rs` (7), `describe.rs` (4), `mod.rs` (4), `system.rs` (2). The remaining **7 zero-test files are all thin CLI print-wrappers** that delegate to already-tested modules (`build_hygiene`, `semantic_diff`, `tooling`, etc.): `build.rs`, `office.rs`, `plugin_handlers.rs`, `retrieval.rs`, `semantic_diff_handlers.rs`, `stewardship_handlers.rs`, `tooling_handlers.rs`. Adding "does not panic" tests to these would be the println-only anti-pattern called out above — prefer testing the underlying modules, or extract any non-trivial decision logic out of the handler before testing it.
| Integration | Covers CLI commands + daemon IPC | 26 tests (4 files under `tests/`) |

New modules must ship with tests meeting the target density. Existing modules should trend toward targets during regular development.

### Coverage Priority (Highest Risk, Lowest Coverage)

| Module | Risk | Why |
|--------|------|-----|
| `src/state/` | HIGH | Persistence layer — corruption means data loss. Well-tested (~80 tests covering conflict detection, audit trail, config corruption, session lifecycle, config keys). |
| `src/handlers/` | MEDIUM | User-facing CLI paths — 12 of 19 files tested (362 tests, ~32/KLOC). Remaining 7 zero-test files are thin print-wrappers; test their underlying modules instead. |
| `src/error.rs` | LOW | All 8 `AgentError` variants have Display tests. |
| `src/ui/` | MEDIUM | TUI rendering — complex layout logic, limited coverage. |

### Codebase Examples

**Good: Error Display test** (exists in `src/error.rs:AgentError`):
```rust
#[test]
fn test_agent_error_missing_api_key_display() {
    let err = AgentError::MissingApiKey { provider: "Anthropic".into() };
    assert!(format!("{err}").contains("Anthropic"));
    assert!(format!("{err}").contains("No API key"));
}
```

**Good: Serde round-trip** (exists in `src/build_hygiene/tests.rs`):
```rust
#[test]
fn test_config_round_trip() {
    let config = BuildHygieneConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let recovered: BuildHygieneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(recovered.enabled, config.enabled);
}
```

**Good: `.context()` chains** (from `src/client/mod.rs`):
```rust
serde_json::to_string(&request).context("Failed to serialize daemon request")?;
```

**Bad: println-only test** (exists in codebase — do not replicate):
```rust
#[test]
fn test_system_info() {
    let info = SystemInfo::collect();
    println!("System info: {:?}", info);  // No assertions — not a real test
}
```

### Error Handling Patterns

**Use `.context()` on all I/O and parse operations:**
```rust
// Good — context says what we were doing
let content = fs::read_to_string(&path)
    .context("Failed to read config file")?;
let config: Config = serde_json::from_str(&content)
    .context("Failed to parse config JSON")?;

// Bad — bare ? gives unhelpful "No such file or directory"
let content = fs::read_to_string(&path)?;
```

**Use `bail!`/`ensure!` for precondition checks:**
```rust
use anyhow::{bail, ensure};

ensure!(!id.is_empty(), "Session ID must not be empty");
if id.contains("..") {
    bail!("Session ID must not contain path traversal: {id}");
}
```

**Audit checklist for error handling compliance:**
```bash
# Find bare ? on I/O operations (should have .context())
cargo clippy 2>&1 | grep -i "unwrap\|expect"
# Find bare fs:: calls without .context()
git grep -n "fs::read\|fs::write\|fs::remove" -- "*.rs" | grep -v "context\|test"
# Find unwrap() outside tests and main
git grep -n "\.unwrap()" -- "*.rs" | grep -v "#\[test\]\|mod tests\|fn main\|impl Default"
```

---

## Build & Test

### Verification Gate

Run before every commit (copy-paste ready):
```bash
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

**Final-gate evidence:** Do not rely on a checked-in aggregate test count. Run the complete gate on
the current checkout and record package-level passed, ignored, and failed totals in the commit/PR
evidence. Default tests must use tracked source/fixtures and remain portable across fresh clones,
linked worktrees, and CI.

**Current verification boundaries:** the canonical Rust gate does not by itself prove a real
provider-backed Ion round trip, cross-platform Linux/Windows behavior, or generalized runtime-role
enforcement that has not been implemented. The feature-gated Dioxus binary also retains its
separate host-readiness smoke check. Re-run the full gate on the current checkout before citing any
aggregate.

**Historical verification evidence:** implementation chronology belongs in Git history and
merged change records; keep this operating guide limited to the current gate and known boundaries.

**Quick health check** (for mid-session verification):
```bash
# impulse-rs has a lib target (`impulse_rs`, since T5) backing two bins
# (impulse-rs, ion); `cargo run` without --bin is ambiguous, so
# `default-run = "impulse-rs"` is set in Cargo.toml to keep bare
# `cargo run --` invocations (used throughout tests/ and src/integration_tests.rs)
# resolving to the impulse-rs binary.
cd impulse-rs && cargo check && cargo test --bins -- --quiet 2>&1 | tail -5
```

### Full Workspace

```bash
cd impulse-rs

cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Individual crates
cd impulse-term && cargo build && cargo test && cargo clippy -- -D warnings
cd impulse-ops && cargo build && cargo test && cargo clippy -- -D warnings
```

### Test Count Verification

To verify test counts match expectations:
```bash
cd impulse-rs && cargo test --workspace 2>&1 | grep "test result:" | awk '{sum += $4} END {print "Total: " sum " passed"}'
```
Treat the command output as the authoritative count for that checkout. Preserve the complete output
in final-gate evidence instead of copying a moving aggregate into this guide.

### Pre-Commit Checklist

1. `cargo build --workspace` — zero warnings
2. `cargo test --workspace` — all tests pass; capture the current passed/ignored/failed totals from the command output
3. `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
4. `cargo fmt --all -- --check` — zero diffs
5. No new `#[allow(...)]` without justification comment
6. New `Serialize + Deserialize` types have round-trip tests
7. New `Result`-returning functions have `Err` path tests

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `IMPULSE_SESSION_ID` | Current session ID |
| `IMPULSE_HOME` | Custom `.impulse/` directory |
| `IMPULSE_SOCKET_PATH` | Custom Unix socket path |
| `ANTHROPIC_API_KEY` | For daemon chat |
| `IMPULSE_MODEL` | Chat model override |
| `ANTHROPIC_BASE_URL` | Override the Anthropic API origin (eval-harness interception, local proxy) |
| `OPENAI_BASE_URL` | Override the OpenAI API origin |
| `MINIMAX_BASE_URL` | Override the Minimax API origin |

Each `*_BASE_URL` override is the API **origin** only (scheme + host + optional port, e.g.
`http://127.0.0.1:4010`) — the provider appends its own request path. Values that are blank or
lack an `http://`/`https://` scheme are ignored in favor of the canonical default, so a typo
degrades to the real API rather than to a silently broken endpoint. Precedence is
explicit config (`BaseProvider::with_base_url`) > env var > canonical default.

Accepted risk: any `http(s)` origin is accepted (no host allowlist). An active override is
logged at provider request time (origin only, never the API key). Cleartext `http://` to a
non-loopback host is a warning; loopback HTTP (local eval harness) is not. This is a
transport override, not a model picker — see ADR-0015.
