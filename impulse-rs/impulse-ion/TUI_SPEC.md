# Ion Interactive TUI/REPL — Spec & Roadmap

**Status:** proposed · **Captured:** 2026-07-11 · **Author:** review/spec session (read-only)
**Direction from James:** running `ion` with no arguments must drop into an interactive
chat loop (like Claude Code's `claude` binary). The existing `ion-verify` one-shot
becomes one internal *tool* the chat loop can invoke — not the whole CLI surface.

Related:
- `~/.ai-memory/docs/ion-harness/spec-a-harness-contract-v0.md` — HarnessRequest/HarnessResponse contract (keystone).
- `impulse-rs/impulse-ion/src/lib.rs` — contract types + validation (spec-a §6 rules as code).
- `impulse-rs/impulse-ion/src/pi_adapter.rs` — PiAdapter: drives `launch-gate.sh --mode rpc` over stdin/stdout JSONL.
- `impulse-rs/src/handlers/ion.rs` — current `ion-verify` CLI handler.
- `impulse-rs/src/cli.rs` (`Commands::IonVerify`, ~L594) and `src/handlers/direct_dispatch.rs` (~L562) — current wiring.

---

## 1. Current-state findings (as-reviewed)

### 1.1 Architecture facts that constrain the design

| Fact | Consequence |
|---|---|
| `impulse-ion` is a deliberately tiny contract crate (deps: `serde`, `serde_json`, `thiserror`, `dirs`; 578 LOC total). Its Cargo description says "transport-agnostic HarnessRequest/HarnessResponse types". | Do NOT put the TUI in `impulse-ion`. Keep it pure. The `ion` binary belongs in the `impulse-rs` main crate (second `[[bin]]`), which already has clap, tokio, ratatui, and the LLM backends. |
| `impulse-rs` already depends on **ratatui 0.28 + crossterm 0.28** (`src/ui/` — full multi-pane monitor TUI with runner/lifecycle/pane_manager). | No new heavyweight deps needed for a full-screen path later. But `src/ui` is a *monitor dashboard*, not a chat loop — don't force the chat into it. |
| **No rustyline/reedline anywhere in the workspace** (checked Cargo.lock). | A readline loop needs exactly one new dep. `rustyline` (small, sync, battle-tested) is the recommended minimal add. |
| `impulse-rs` already has a chat substrate: `src/llm_backends/mod.rs` — `LlmProvider` trait + `ChatSession::chat(&mut self, user_message) -> AgentResult<String>` with history + `clear_history()`, Anthropic backend, `ANTHROPIC_API_KEY` / `IMPULSE_MODEL` env. `Commands::Chat` is currently one-shot (`--session-id --message`). | The REPL is mostly plumbing: readline loop → `ChatSession` → print. Reuse, don't rebuild. |
| `src/tooling/` has a capability-based, deny-by-default `Tool` trait (`async fn execute`, `required_capabilities()`, registry enforcing exists → capability → param-validate → execute). | The chat loop's tool layer should register through this registry (or a thin REPL-command layer that delegates to it) rather than inventing a third tool abstraction. |
| `PiAdapter::verify()` is **fully blocking** (`std::process::Command`, blocking `BufRead::read_line`), called from inside `async fn handle_ion_verify` on the tokio runtime. | Acceptable for a one-shot CLI; in a REPL it will freeze the loop. Must be wrapped in `tokio::task::spawn_blocking` (or given an async variant) before it becomes a chat tool. |
| `handle_ion_verify` calls `std::process::exit(1)` on gate failure (matching the `handlers::guard` gate-command exit-code convention). | Correct for a shell gate; fatal for a REPL (would kill the whole chat session). The verdict→exit-code decision must move to the CLI edge only. |
| `PiAdapter` spawns with `stderr(Stdio::inherit())`. | In any TUI (raw mode or even a readline prompt) inherited child stderr will corrupt the screen. Must become `Stdio::piped()` + surfaced as a log when called from the loop. |

### 1.2 Code-quality review of the ion-verify wiring

Overall: **good** — doc comments explain the spec-a linkage, `anyhow::Context` chains
are consistent with sibling handlers, request/response are `validate()`d, and the
lenient JSONL parser in `pi_adapter.rs` is genuinely well-tested (embedded-newline
recovery, escaped-quote state machine, garbage bound, ignored live smoke test).
`impulse-ion` itself meets the repo's serde round-trip + Err-path test bar.

Gaps to fix (ordered; G1–G3 are prerequisites for the REPL, G4–G6 are hygiene):

- **G1 — `std::process::exit(1)` inside the handler.** Makes the core logic
  untestable (the handler's own test comment admits the exit branch is not
  observable) and unusable as an in-process tool. Split into a pure
  `run_ion_verify(...) -> Result<HarnessResponse>` plus a thin CLI wrapper that
  maps `!response.passed()` to the exit code. This mirrors how the repo already
  treats "thin CLI print-wrappers" vs tested logic (see CLAUDE.md handler notes).
- **G2 — blocking child-process I/O on the async runtime.** `adapter.verify(&request)`
  blocks a tokio worker thread for the full gate round trip (Node 22 + MiniMax
  network latency). Wrap in `spawn_blocking` when called from the handler and from
  any future tool.
- **G3 — no timeout on the gate child.** `MAX_RESPONSE_LINES` bounds *garbage*, but
  a hung child that writes nothing blocks `read_line` forever, and `child.wait()`
  has no deadline. Add a configurable timeout (default on the order of a few
  minutes) with child kill-on-timeout. Without this, one hung gate freezes the REPL.
- **G4 — request-defaults duplication.** The default `capability_allowlist`,
  `verdict_priority`, `model_role`, and `Context { read_only: true, .. }` are
  hand-rolled in three places (handler, handler test, `pi_adapter.rs` live test,
  plus `lib.rs` worked example). Add a constructor in `impulse-ion`:
  `HarnessRequest::verify(repo_path, diff_ref, description) -> Self` (validated by
  construction), and use it everywhere. This is the natural seam the chat tool will
  call too.
- **G5 — no fake-gate integration test.** `PiAdapter::with_launch_script` exists
  precisely for this but is only used to assert the default path. Add a test using
  a stub shell script fixture (under `tests/fakes/` per repo policy) that echoes a
  canned `HarnessResponse`, covering: pass, fail (CHANGES REQUESTED), contract-violating
  response (warning path), non-zero exit, and hang→timeout once G3 lands. Today the
  entire spawn/verify/exit-status path has zero non-live coverage.
- **G6 — smaller issues.**
  - Hardcoded launcher path `~/.ai-memory/docs/ion-harness/pi-gate/launch-gate.sh`
    with only a test-time override; add an env override (e.g. `ION_GATE_LAUNCHER`)
    per the real-systems "paths come from config/env" rule.
  - In `--json` mode, a spec-a contract violation is only an stderr warning — not
    machine-readable. Include a `contract_violation` field in the JSON envelope (or
    a wrapper object) so scripted callers can branch on it.
  - `diff_ref` is passed through unvalidated; a cheap sanity check (non-empty,
    no leading `-`) avoids confusing gate-side failures.
  - `handlers/ion.rs` tests assert only "does not panic" on the print path — right
    at the edge of the repo's println-only anti-pattern. After G1, test
    `run_ion_verify` against the fake gate instead.

---

## 2. Recommended approach

### 2.1 Interaction model: readline REPL first, ratatui later (optional)

Claude Code's `claude` is an *inline* prompt loop, not a full-screen app. Match that:

- **Phase 1 (this spec's scope): `rustyline`-based readline loop.** One new dep.
  Prompt (`ion ❯ `), multiline-capable input, persistent history at
  `.impulse/ion_history`, Ctrl-C cancels the current line, Ctrl-D / `/quit` exits.
  Output is plain streamed text — works in any terminal, in tmux/zellij panes, and
  composes with the existing `src/ui` monitor without competing for raw mode.
- **Phase 2 (optional, later): ratatui "inline viewport" upgrade** for spinners,
  verdict tables, and a status line — the crates are already in-tree. Do not start
  here; a full-screen chat is strictly more code and blocks nothing.

Rejected: reedline (larger dep surface, no in-tree precedent), building the chat
inside the existing `src/ui` dashboard (different concern: monitoring vs conversing).

### 2.2 Where the code lives

Keep `impulse-ion` a pure contract crate. Host the binary in the main crate:

```
impulse-rs/                       (main crate — already has clap/tokio/llm/tooling)
├── Cargo.toml                    + [[bin]] name = "ion", path = "src/bin/ion.rs"
│                                 + rustyline = "15"   (workspace-level pin)
├── src/
│   ├── bin/ion.rs                # thin entrypoint: clap parse → repl::run() or one-shot
│   ├── ion_repl/
│   │   ├── mod.rs                # ReplSession: loop { readline → route → render }
│   │   ├── router.rs             # input → SlashCommand | ChatTurn
│   │   ├── tools.rs              # ReplTool trait + registry (wraps src/tooling where possible)
│   │   ├── tool_verify.rs        # ion-verify as a ReplTool (spawn_blocking + timeout)
│   │   ├── render.rs             # verdict/finding/log formatting (reuses handlers::ion text fns)
│   │   └── history.rs            # .impulse/ion_history load/save
│   └── handlers/ion.rs           # refactored: pure run_ion_verify + CLI wrapper (G1)
└── impulse-ion/                  # unchanged contract crate (+ HarnessRequest::verify ctor, G4)
```

`ion` and `impulse-rs` remain two binaries of one crate: zero code duplication, one
test suite, and `cargo install`/bundling picks both up.

### 2.3 Tool model inside the chat loop

Input routing in `ion_repl::router`:

1. Line starts with `/` → **slash command** (deterministic, no LLM):
   `/verify [--repo P] [--diff-ref R] [description…]`, `/help`, `/tools`, `/clear`
   (clears `ChatSession` history), `/quit`. `/verify` calls the `ion_verify`
   ReplTool directly.
2. Anything else → **chat turn** via `llm_backends::ChatSession` (Anthropic,
   `ANTHROPIC_API_KEY`). No key → friendly one-line notice; slash commands still work.
3. **Later:** expose ReplTools to the model as Anthropic tool-use definitions so
   free-text like "verify my last commit" triggers `ion_verify` via tool calling.
   The ReplTool trait below is shaped so this is additive, not a rewrite.

```rust
/// One callable capability inside the ion chat loop.
/// Deliberately mirrors src/tooling::Tool so tools can be adapted from the
/// existing registry; kept separate only for the REPL-specific render hook.
#[async_trait]
pub trait ReplTool: Send + Sync {
    fn name(&self) -> &'static str;              // "ion_verify"
    fn usage(&self) -> &'static str;             // for /help and LLM tool schema
    fn json_schema(&self) -> serde_json::Value;  // Anthropic tool-use input schema
    async fn run(&self, args: serde_json::Value, ctx: &ReplContext) -> Result<ToolOutcome>;
}

pub struct ToolOutcome {
    pub rendered: String,          // human text for the transcript
    pub payload: serde_json::Value, // structured result (HarnessResponse for verify)
    pub ok: bool,                  // verify: response.passed()
}
```

The `ion_verify` tool is a thin wrapper: `HarnessRequest::verify(..)` (G4) →
`spawn_blocking(PiAdapter::verify)` with timeout (G2/G3) → render verdict/findings
(reuse the formatting extracted from `handlers/ion.rs`). Every future gate
(cc-gate per spec-b, guard checks, semantic-diff) is just another `ReplTool`.
**Spec-a invariant carried over:** the tool branches only on `response.passed()` —
never NLP on prose — and verify-intent requests keep write-denial-by-omission
(`HarnessRequest::validate()` before send).

**Scope clarification (2026-07-11, James):** `ion` is not a read-only verify
console — it is a coding agent in the same category as `claude`, `codex`, or
any other CLI/TUI coding agent Impulse instruments, and CRUD/bash/write
capability is a first-class requirement of the product, not an
afterthought. `ion_verify` (and future gate tools like `cc-gate`) are *one*
ReplTool among many, not the whole tool surface. T7 must also register
write-capable tools from the existing `src/tooling::Tool` registry (file
read/write/edit, bash execution, etc. — see `src/tooling/builtin/` for what
already exists) so the chat loop can actually make code changes, not only
gate them. The write-denial-by-omission rule above is scoped narrowly to
`HarnessRequest`/verify-intent gate calls (spec-a's contract for *that*
request type) — it does not mean the `ion` REPL as a whole is write-denied.
Keep these two capability universes conceptually separate: the verify gate's
closed read-only allowlist vs. the REPL's full coding-agent tool registry.

### 2.4 CLI surface of the new binary

```
ion                     # no args → interactive REPL (the point of this spec)
ion verify [FLAGS]      # one-shot, same flags/exit codes as `impulse-rs ion-verify`
ion --help / --version
```

`impulse-rs ion-verify` stays (scripts/hooks depend on the exit-code convention);
both paths call the same `run_ion_verify` after G1. Deprecate later if desired.

---

## 3. Incremental task list (PR-sized, each independently buildable/testable)

Ordering is by dependency, not time (measure-by-dependencies rule). Each step must
pass the repo gate: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
(run from `impulse-rs/`), and update CLAUDE.md test counts if they change.

- **T1 — `HarnessRequest::verify` constructor (impulse-ion).** Add the builder +
  unit tests (round-trip, validates clean, allowlist matches spec-a §2). Refactor
  `handlers/ion.rs`, `pi_adapter.rs` live test, and `lib.rs` worked example to use
  it. No behavior change. *(Fixes G4. Blocks T3, T7.)*
- **T2 — PiAdapter hardening.** Env override `ION_GATE_LAUNCHER`; `stderr` piped
  + returned/logged instead of inherited; child timeout with kill (new
  `AdapterError::TimedOut`). Unit tests for env override + error Display. *(G3/G6.
  Blocks T7.)*
- **T3 — split `run_ion_verify` from the CLI wrapper.** Pure function returns
  `Result<HarnessResponse>`; wrapper keeps exit-code + print behavior identical
  (including `--json`, now with the machine-readable `contract_violation` field).
  Wrap the adapter call in `spawn_blocking`. *(G1/G2/G6. Depends T1. Blocks T4, T7.)*
- **T4 — fake-gate integration tests.** Stub gate script under `tests/fakes/`
  driven via `PiAdapter::with_launch_script`: pass / changes-requested /
  contract-violation / non-zero-exit / timeout. Covers `run_ion_verify` end-to-end
  without network. *(G5. Depends T2+T3. Blocks nothing but gates "done" for the fixes.)*
- **T5 — `ion` binary skeleton.** Add `[[bin]] ion`, clap enum { (none), Verify },
  `ion verify` delegating to `run_ion_verify` wrapper; bare `ion` prints a
  placeholder banner + exits 0. Integration test: `ion verify --help` parses;
  bare run emits banner. *(Depends T3.)*
- **T6 — readline REPL core.** Add `rustyline`; `ion_repl::{mod,history,router}`
  with `/help`, `/quit`, `/clear`, unknown-command message; free text echoes a
  "chat not wired yet" stub. History persisted to `.impulse/ion_history`
  (respect `IMPULSE_HOME`). Router unit tests (slash parsing, quoting, empty line).
  *(Depends T5. Independent of T1–T4.)*
- **T7 — `ion_verify` as a ReplTool.** `ReplTool` trait + registry + `/tools`;
  `tool_verify.rs` wiring `HarnessRequest::verify` → spawn_blocking adapter →
  rendered verdict. Tests against the T4 fake gate through the tool interface.
  *(Depends T4, T6.)*
- **T8 — chat turns via `ChatSession`.** Free text → `llm_backends::ChatSession`
  (Anthropic; `IMPULSE_MODEL` override honored); `/clear` clears history; missing
  key → graceful notice. Unit-test routing with a fake `LlmProvider`; live path
  behind an `#[ignore]` test per existing convention. *(Depends T6. Parallel to T7.)*
- **T9 — LLM tool-calling.** Expose registered ReplTools as Anthropic tool-use
  schemas; handle tool_use → run → tool_result round trip so "verify this diff"
  works conversationally. Requires extending the anthropic backend for tool blocks.
  *(Depends T7+T8.)*
- **T10 (optional) — ratatui inline polish.** Spinner during gate runs, colored
  verdict table, status line. *(Depends T7. Do not start before T9 is stable.)*

Parallelization: {T1,T2} → T3 → T4 is the fix chain; T5 → T6 → {T7,T8} → T9 is the
REPL chain; T6 and T1–T4 can proceed concurrently in separate lanes (disjoint files
except `handlers/ion.rs`, owned by the fix chain).

---

## 4. Non-goals / guardrails

- No TUI code in the `impulse-ion` crate — it stays a contract crate.
- No second chat/LLM stack — reuse `llm_backends`; no mock LLM outside `*.test.*`
  (real-systems rule); fake gate scripts live only under `tests/fakes/`.
- Never parse gate prose for pass/fail — `HarnessResponse::passed()` only (spec-a §5).
- Don't remove `impulse-rs ion-verify` or change its exit-code contract in this work.
- Verify-intent requests must keep the write-denied allowlist; the REPL must never
  add write capabilities to a gate request (spec-a §2/§6, enforced by `validate()`).
