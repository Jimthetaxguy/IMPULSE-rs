# CLAUDE.md — Impulse

> Local control plane and harness manager for AI coding agents.
> Product north star: [`VISION.md`](VISION.md)
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Collaboration playbook: [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=control-plane foundations + governed runtime producers + accepted-run review candidates; Next=stronger same-user actor authorization + full launched Builder/Supervisor proof; Later=explicit memory promotion/dismissal + general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.

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
- `impulse-rs/` — main CLI + daemon + ratatui TUI (`impulse-rs` binary); library crate `impulse_rs` (`src/lib.rs`, TUI_SPEC.md T5) also backs a second, independent `ion` binary (`src/bin/ion.rs`) — bare `ion` drops into a rustyline REPL (`src/ion_repl/`, T6/T7: `/help`/`/quit`/`/clear`/`/verify`/`/tools` slash commands, `.impulse/ion_history` persistence); `/verify` dispatches through a `ReplTool` registry (`src/ion_repl/registry.rs`) holding `ion_verify` (read-only spec-a gate, `tool_verify.rs`) plus write-capable tools bridged from `src/tooling::ToolRegistry` (`file_read`, `file_write`, `bash_exec`, via `tool_bridge::DynamicToolBridge`) — `ion` is a full coding agent, not a read-only verify console (TUI_SPEC.md section 2.3); `ion verify` shares `handlers::ion::handle_ion_verify` with `impulse-rs ion-verify`. Free-text lines (T8) reach `src/ion_repl/chat.rs`'s `ChatState`, wrapping one `llm_backends::Agent` per session so conversation history survives across turns (`/clear` truly clears it; missing `ANTHROPIC_API_KEY` degrades to a one-line notice, not a panic). **T9 (final REPL-roadmap item, landed):** every chat turn now exposes the session's `ReplToolRegistry` to the model as Anthropic tool-use schemas (`ReplTool::json_schema`) via `ChatState::turn`'s new `&ReplToolRegistry`/`&ReplContext` params, so free text like "verify my diff" can trigger `ion_verify`/`file_write`/`bash_exec` conversationally, not only via slash commands. The tool-use request/execute/`tool_result` loop lives in `llm_backends::mod.rs` (`Agent::chat_with_tools`, provider-agnostic — no `ion_repl` dependency), capped at `DEFAULT_MAX_TOOL_ROUNDS = 10` round trips and erroring with `AgentError::ToolLoopLimitExceeded` rather than looping forever; `ion_repl::chat::ReplToolExecutor` adapts the REPL's own registry to the new `llm_backends::ToolExecutor` trait. `AnthropicProvider::chat` (`llm_backends/anthropic.rs`) sends `ChatRequest::tools` as the wire `"tools"` array and parses `tool_use` blocks + `stop_reason` out of the response via a new `format_anthropic_messages` helper (block-array `content` for tool_use/tool_result messages, plain string otherwise); OpenAI/Minimax accept the new `ChatResponse` fields (`stop_reason`, `tool_calls`) but don't populate them yet. **Confirmation gate (same-day adversarial-review follow-up):** T9 made `bash_exec`/`file_write` reachable from raw model output for the first time (previously registered but never dispatched) with no confirmation step — unlike `claude`/`codex`, which prompt before write/bash by default. `ion_repl::chat::ReplToolExecutor` now gates `CONFIRMATION_REQUIRED_TOOLS` (`bash_exec`, `file_write`) behind a `confirm` hook (`confirm_via_stdin` in production: prints the pending call, reads `y`/`N`, default deny); a decline short-circuits before `ReplTool::run` is ever called, so nothing executes. `ion_verify`/`file_read` stay ungated (read-only). `ChatState::with_confirm` is the test-only DI seam. **Env scrubbing (same-day follow-up to the confirmation gate):** the confirmation gate stops an unapproved `bash_exec` call but not an approved one that innocuously leaks secrets — a command like `env` or `printenv ANTHROPIC_API_KEY`, once a user approves it, would print the `ion` process's own env (API keys/tokens) into `tool_result` content that flows straight back into the model's context and the REPL transcript. `bash_exec.rs`'s `Command` now calls `.env_clear()` before spawn and re-adds only `ENV_ALLOWLIST` (`PATH`, `HOME`, `TERM`, `LANG`, `LC_ALL`, `TMPDIR`, `TMP`, `TEMP`) — an allowlist, not a denylist, matching Principle #5's deny-by-default philosophy: everything not explicitly named is dropped rather than trying to enumerate every secret name. A defensive `is_secret_like` heuristic (case-insensitive substring match on `KEY`/`TOKEN`/`SECRET`/`PASSWORD`/`_PAT`/`CREDENTIAL`) guards the allowlist itself via `debug_assert!`. **Env-scrub extraction + `ProcessTool` fix (same-day follow-up audit):** a sweep of the ~40 `Command::new` call sites in `src/` and `impulse-ion/src/` found `src/tooling/external.rs`'s `ProcessTool::execute` (manifest-defined tools under `.impulse/tools.d/`, reached via the daemon's `InvokeTool` IPC endpoint and the `tooling-run` CLI handler, both human/GUI-driven — not currently reachable from any LLM tool-calling loop, since `ion_repl`'s registry uses `ToolRegistry::with_defaults()`, which excludes `ProcessTool`) had the same full-env-inheritance gap as `bash_exec` once had. `ENV_ALLOWLIST`/`is_secret_like`/the scrub logic moved to a shared `src/tooling::env_scrub` module (`scrub_and_allowlist_env(cmd, extra_allowlist)`); `external.rs` now calls it with the manifest's own `env_allowlist` as the per-tool extra allowlist, fixed as defense-in-depth ahead of `ProcessTool` potentially being bridged into `ion_repl` later. `impulse-ion/src/pi_adapter.rs`'s `bash` launcher subprocess was reviewed and intentionally left on full env inheritance — it's a developer-configured launcher script, not itself running LLM-generated shell text, and plausibly needs its own credentials (e.g. a MiniMax key) to authenticate; scrubbing it would risk a functional regression with no corresponding secret-leak vector today. **Wall-clock timeout on the tool loop (Opus adversarial-review follow-up, finding S2, same-day):** Opus flagged that the tool loop is uninterruptible and had no wall-clock budget — `ReplSession::run` only handles Ctrl-C around `readline()`, not during `handle_line().await`, so a model doing the full `DEFAULT_MAX_TOOL_ROUNDS = 10` rounds, each potentially waiting on a 30s `bash_exec`, could block the REPL for several minutes with no way to abort. Full mid-`.await` interruptibility (cancellation tokens threaded through the readline/event loop) was deliberately scoped out as a much larger change; instead `Agent::chat_with_tools`/`chat_with_tools_capped` (`llm_backends/mod.rs`) now wrap the *entire* multi-round exchange — not any single round — in `tokio::time::timeout` against a new `DEFAULT_TOOL_LOOP_TIMEOUT = Duration::from_secs(180)` constant, erroring with a new `AgentError::ToolLoopTimedOut { seconds }` variant instead of hanging. The round-loop body was extracted into a free fn `run_tool_loop` (borrows `&dyn LlmProvider`/`&str`/etc., never `&mut Agent`) so the future passed to `tokio::time::timeout` never holds a long-lived mutable borrow of `Agent`; history is only committed via `self.history = working` on the success path, so both a round-cap error and a timeout leave `self.history` untouched, matching `ToolLoopLimitExceeded`'s existing invariant (proven by a new `test_chat_with_tools_capped_timeout_returns_error_when_timeout_hit` test using a short timeout override via the private `chat_with_tools_capped_timeout` seam, mirroring how the round-cap test uses a small `max_rounds` override). `ion_repl::mod.rs`'s `respond()` renders `ToolLoopTimedOut` through the existing generic `format!("Chat failed: {err}")` branch (same as `ToolLoopLimitExceeded`) rather than adding a dedicated notice — the `Display` message is already self-explanatory. **Guardrail-scanned confirmation (ROSA reverse-transfer, same-day follow-up):** the flat `confirm_via_stdin` y/N prompt made the human eyeball raw command/content text with no assistance, unlike sibling project ROSA's `ApprovalGrant`/`Gate` design, which computes a `RiskClass` from a guardrail scan before ever asking (see `impulse-ion/TUI_SPEC.md` "ROSA reverse-transfer comparison" for the original gap analysis). Rather than build a new risk taxonomy, `bash_exec`'s `command` and `file_write`'s `content` are now scanned through the pre-existing `src/guardrail` module (`GuardEngine`/`GuardAction::{Block,Warn,Log}`, already used by PreToolUse hooks) via a new `guard_verdict_for`/`guard_scan` pair in `chat.rs` (`GuardTarget::Bash`/`GuardTarget::FileWrite` respectively, `GuardConfig::default()` — no user-config-file loading in this REPL context), before `self.confirm` runs. `ReplToolExecutor::confirm`'s signature grew a third `&GuardVerdict` (`Option<GuardResult>`, the most severe match) parameter so the UX can react to severity: no-match/`Log` keeps the plain y/N; `Warn` keeps y/N but `confirm_via_stdin` now also prints the guardrail's reason and rule id; `Block` is no longer a simple y/N — a new pure `decide_approval(verdict, response)` function requires the literal, case-sensitive string `CONFIRM` (a bare `y`/`yes` does not satisfy it), matching ROSA's model of holding Block-tier for a stronger gate than a reflexive approval — `ion` has no separate operator to escalate to (single-user REPL), so `CONFIRM` is the adapted equivalent of ROSA's "held for operator decision". A new `pub(crate) struct ApprovalGrant` (private fields, private `new()`) is the structural piece carried over from ROSA's unforgeable-token `ApprovalGrant`/`Gate`: `ReplToolExecutor::execute` only mints one after a genuine `true` from `self.confirm`, and holds it in scope through the `tool.run()` call for gated tools — a type-level reminder that execution only happens downstream of a minted grant. Unlike ROSA's version it is not re-checked at dispatch time and is not threaded through `ReplTool::run`'s own signature (single-process synchronous call site, not a queued/async dispatch system, so ROSA's `ensure_grant_covers` TOCTOU defense doesn't apply), and it carries only the matched `GuardAction`, not a `RiskClass` taxonomy. **Known gap vs. the built-in rule set:** `guardrail::defaults::builtin_rules()` ships 9 rules, all targeting `GuardTarget::Bash` (force-push-main, bulk-git-add, rm-rf-root, drop-table as Block; binary/artifact/env-file staging + chmod-777 as Warn; deploy-commands as Log) — there is currently no built-in `GuardTarget::FileWrite` rule (e.g. for secret-shaped content), so `file_write` scanning is wired correctly but will not match anything against today's defaults; regression tests prove the wiring with an equivalent custom rule. **Deliberately out of scope:** this only scans the tool call's own arguments at confirmation time, not the system prompt or injected context before the model decides to request a call — ROSA's fuller "never lowering the floor across every channel" design would need hooking into the chat loop's message construction, a materially larger change. **Keychain secret-argv fix (same day, Opus sweep finding):** `src/credentials/keychain.rs`'s `KeychainProvider::set` called `security add-internet-password -w <value>` with the secret as a bare CLI argument — visible to any other local process via `ps aux`/`ps -ef` for the child's lifetime; `security -h`'s own usage text warns against this and recommends passing `-w` with no value to trigger an interactive stdin prompt instead. Fixed by spawning with `Stdio::piped()` and writing the value to the child's stdin twice (the prompt asks for entry + confirmation), never putting it in argv; proven end-to-end with a macOS-gated round-trip regression test (`test_set_get_delete_round_trips_via_stdin_prompt_not_argv`) doing a real Keychain set/get/delete. **Test-marker collision fix (same day, Fable adversarial review finding):** `agent/mod.rs`'s and `bash_exec.rs`'s orphan-kill regression tests each computed a "unique" sleep-duration marker as `format!("9.{}", std::process::id() % 1000)` — identical formula, same process, so two tests in the same `cargo test` run could theoretically collide on the same marker value. Switched all three occurrences (one in `agent/mod.rs`, two in `bash_exec.rs`) to a nanosecond-timestamp-based marker (`SystemTime::now().duration_since(UNIX_EPOCH).subsec_nanos() % 1000`), matching the existing PID+nanos precedent in `storage::atomic_write_path`.
- `impulse-rs/impulse-ops/` — shared control-plane protocol and models: supervisor policy/actions, telemetry, workbench snapshots, artifacts, daemon requests/responses, and the agent-platform registry
- `impulse-rs/impulse-term/` — PTY/session/context core (PTY + vt100 + WriteQueue + context bridge)
- `impulse-rs/impulse-desktop/` — Dioxus cockpit, typed host bridge, workspace registry, PTY runtime integration, and desktop MCP surface; it projects backend truth rather than owning it
- `impulse-rs/impulse-ion/` — Ion harness contract v0 (transport-agnostic `HarnessRequest`/`HarnessResponse` types + `PiAdapter`, the Rust-side caller of harness #2/Pi-on-MiniMax; drives `impulse-rs ion-verify`, see `impulse-ion/TUI_SPEC.md` for the ion-cli agent roadmap, 23 tests)

**Legacy:** `impulse-gui` / egui is frozen. It receives compile-maintenance only until the Dioxus desktop host reaches parity. Tauri-shaped code is also compatibility-only, not a new product scaffold target.

**Execution surfaces:**
- **Direct mode** — stateless, per-action (for hooks). Read → process → write → exit.
- **Daemon mode** — long-running Unix socket authority for the TUI and Dioxus cockpit. In-memory state with periodic sync.
- **Desktop mode** — Dioxus Desktop cockpit with xterm.js terminal bridge, backed by Rust daemon/runtime state. Tauri-shaped command/event code is compatibility-only.

**IPC Protocol (PROTOCOL_VERSION = 6):**

The daemon exposes a JSON-line Unix socket protocol. Key endpoint groups:

| Group | Endpoints | Purpose |
|-------|-----------|---------|
| Agent Coordination | `AgentAssist` | AI coordination with context enrichment via extracted insights |
| Agent Specialized | `AgentReviewCode`, `AgentAnalyzeError`, `AgentSummarizePane` | Per-task agent assistance |
| Delegation | `RegisterDelegation`, `CompleteDelegation`, `ListDelegations` | Phase 1B cross-agent delegation tracking |
| Conflict Resolution | `GetConflictHistory`, `ClearResolvedConflicts` | File conflict tracking and resolution |
| Agent Pool | `GetAgentPool` | All sessions grouped by role (Phase 2B) |
| Governed Tasks | `RegisterGovernedTask`, `GetGovernedTask`, `ListGovernedTasks`, `MutateGovernedTask`, `SubmitGovernedClaim`, `RunGovernedVerification`, `RunGovernedSupervisorReview` | Durable revisioned task state plus daemon-owned profiled producers and operator-required acceptance |

Responses use `AgentAssistResult` (with `recommendations` + `pane_summaries`) or `AgentSpecializedResult` (for review/analyze/summarize). Full protocol spec: [`docs/IPC-PROTOCOL.md`](docs/IPC-PROTOCOL.md).

**Daemon agent-IPC freeze fix + bounded reads (same-day Opus sweep, outside the `ion_repl` hardening above):** a fresh read-only sweep of `src/daemon/` and `src/agent/` (deliberately outside that day's `ion_repl` work) found two real bugs. **(1) Un-timed harness subprocess held across a shared mutex:** `agent::ImpulseAgent::harness_query_structured` (`src/agent/mod.rs`) spawned the configured CLI harness (`claude`/`codex`/`gemini`) via `tokio::process::Command::output().await` with no timeout and no `kill_on_drop` — matching the bug class already fixed in `bash_exec.rs`/`pi_adapter.rs` earlier that day. Worse, all three daemon call sites (`SupervisorChat`, `AgentAssist`, `AgentReviewCode`/`AgentAnalyzeError`/`AgentSummarizePane` in `src/daemon/handlers.rs`) called `agent.query(...).await` while still holding the `cached_agent: Arc<Mutex<Option<ImpulseAgent>>>` guard returned by `get_or_init_agent` — so a single wedged harness child froze `cached_agent.lock().await` for every other agent-related daemon request, not just the hung one. Fixed in two layers: `agent/mod.rs` gained `cmd.kill_on_drop(true)` plus `tokio::time::timeout(DEFAULT_HARNESS_TIMEOUT = 120s, cmd.output())`, erroring with a new `AgentError::HarnessTimedOut { command, seconds }` variant (test/DI seam: `harness_query_structured_with_timeout`, mirroring `chat_with_tools_capped_timeout`'s pattern). `daemon/handlers.rs`'s `get_or_init_agent` was replaced with a `checkout_agent`/`checkin_agent` pair: `checkout_agent` takes the `ImpulseAgent` out of the `Option` and drops the `MutexGuard` before returning it *owned*; callers run their query on the owned value with no lock held, then `checkin_agent` puts it back with a fresh short lock — the mutex is now held only for the instant of a get/init, never across the subprocess `.await`, at all three call sites. **(2) Unbounded daemon/MCP request-line reads:** `daemon/mod.rs`'s connection loop called `reader.read_line(&mut line).await?` and only checked `line.len() > MAX_REQUEST_SIZE` *after* the full (possibly enormous) line was already buffered — a local client could send unbounded non-newline bytes and OOM the daemon before the check ever ran. `mcp/server.rs`'s stdio and `serve_tcp` (bound to `127.0.0.1`, reachable by any local process) paths had the same read with no size guard at all. Fixed with a new `daemon::{read_bounded_line, BoundedLine, MAX_REQUEST_SIZE}` (wraps the reader in `AsyncReadExt::take(max_bytes + 1)` composed with `AsyncBufRead::read_until`, so peak memory for one line read is capped regardless of what the peer sends) shared by both `daemon/mod.rs` and `mcp/server.rs`; an oversized line closes the connection rather than attempting an unbounded drain to resynchronize.

**Current managed-agent turn invariant (introduced in protocol v3):** The checkout/checkin layer described above was superseded after deterministic tests proved that it could double-initialize the cached agent, overwrite `session_history`/`recommendations`/`pane_summaries`, and leave the cache empty when a handler was cancelled. `try_lock_agent_for_turn` retains the agent in-place under a Tokio mutex for one bounded query. Concurrent turns fail fast with typed `Busy { resource: agent_turn, retry_after_ms: 250 }` instead of queueing past the client's response budget; unrelated daemon endpoint groups remain independent; dropping a cancelled handler releases the guard without removing the cached instance. Provider timeouts prevent the original indefinite-wait failure. Protocol v4 added the daemon-owned governed-task request family and snapshot state. Protocol v5 adds daemon-owned profiled claim, verification, and Supervisor-review producers without weakening this turn invariant. Governed Supervisor review is API-only, history-free, tool-free, and temperature-zero; generic external harness configuration fails closed before spawning because it cannot provide a structurally read-only turn.

**Current profiled governed-producer invariant:** A Dioxus Builder launch using `rust_workspace_v1` supplies exact acceptance criteria and can register only from the clean canonical Git worktree root at a committed `HEAD`. The daemon derives the Worker and Verifier records, verifies the claimed commit in a detached worktree with fixed Rust commands, and binds a strict Supervisor envelope to the task revision, claim, verification, subject, and acceptance-criteria digest. The CLI uses injected `IMPULSE_PROJECT_ID`, `IMPULSE_GOVERNED_TASK_ID`, `IMPULSE_SOCKET_PATH`, and `IMPULSE_CONTROL_CLI`; Ion additionally exposes `governed_submit_claim`. The packaged executable is `impulse-rs`, while governed panes invoke `"$IMPULSE_CONTROL_CLI" --daemon governed-claim`, `"$IMPULSE_CONTROL_CLI" --daemon governed-verify`, or `"$IMPULSE_CONTROL_CLI" --daemon governed-review`; `--daemon` is a global flag and must precede the subcommand. Verification executes host-trusted Rust code and is not an OS sandbox. Dioxus currently displays terminal command guidance rather than producer buttons. Actor IDs remain same-user provenance. Persisted receipts plus the per-task lock cover replay/concurrency, not daemon crash between side effect and receipt; durable producer reservations and accepted-run memory promotion remain future work.

**Data lives in `.impulse/`:**
- `HISTORY.jsonl` — append-only session log (committed)
- `GENOME.md` — permanent decisions and preferences (committed)
- `LIVE_STATE.json` — active session state (ephemeral)
- `config.json` — runtime configuration
- `GOVERNED_TASKS.json` — daemon-owned governed task records and idempotency receipts
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
