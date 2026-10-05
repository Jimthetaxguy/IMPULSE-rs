---
title: LLM provider and daemon client fixes
description: Work card for llm-client-fixes-20261005
updated: 2026-10-05
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, llm-backends, daemon-client, review]
---

# LLM provider and daemon client fixes

## Lane Facts
- Owner: claude (Opus 5.5, under James's standing goal "continue cleaning up and reviewing the
  code")
- Role: implementer, from a read-only review of two modules no earlier review had covered:
  `src/llm_backends/` (providers, the tool loop, base-URL overrides) and `src/client/` (the
  daemon client). The review reported 15 findings, 13 reproduced against loopback mock servers
  with a fake key.
- Branch: `claude/llm-client-fixes-20261005`, from `origin/main` at `7481457`
- Worktree: `.worktrees/term-context-20261005` (shared with the other 2026-10-05 lanes, one branch
  at a time, for incremental builds)
- Owned paths: `src/llm_backends/{anthropic,mod}.rs`, `src/client/mod.rs`, the inner-agent setup
  in `src/agent/mod.rs`, the base-URL paragraph in `CLAUDE.md`, this card
- Shared paths: `src/agent/mod.rs` is also edited by `claude/code-cleanup-20261004`, in other
  regions; the trial merge is clean.
- Blocked paths: `Cargo.toml`, `Cargo.lock`, `CONTEXT.md`, `src/daemon/handlers.rs` (the governed
  registration lock below belongs with the cleanup branch's governed fixes)
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented, verification round fixed, gated and pushed; not merged; needs a PR.

## Findings fixed
- P2, secret leak: the providers followed redirects, and reqwest drops `Authorization` on a hop to
  another host but not `x-api-key`, so a 307 delivered the Anthropic key and the whole prompt to a
  host nobody configured (and an https-to-http hop would have sent them in cleartext). The client
  follows no redirects now; a 3xx is an error naming the origin it pointed to.
- P2, hang: the daemon client's timeout covered only the read. A daemon that stopped reading
  blocked the write of any request over the socket buffers (a few kilobytes on macOS) forever. One
  deadline now covers connecting, the capability handshake, writing and reading.
- P2, memory: provider bodies were read whole. A success over 16 MiB is now an error, and an error
  body is read to 64 KiB and quoted to 2,000 characters (it reaches the REPL, logs and daemon
  clients).
- P2, wrong results from OpenAI-style replies: a reply with no choice (empty, missing or null)
  was a successful empty answer; MiniMax's `base_resp` failures (often HTTP 200) were too, or with
  `choices: null` a parse error that dropped MiniMax's reason; and `finish_reason: "stop"` beside
  tool calls ended the turn and dropped the calls. Each is an error now (MiniMax 1004 is an
  authentication error and 1002 a rate limit), and tool calls make a tool-use turn whatever the
  stop reason says, except a token-limit stop.
- P2 (second half; the first is fixed on the cleanup branch by `3b8768e`): the API agent sent
  temperature 0.7 and a 4,096-token cap whatever the configuration said (0.3 and 2,048 by default).
  `Agent` carries both settings now, and `ImpulseAgent` sets them from its config.
- P3, malformed tool turns: a tool-use stop with no call completed as an empty answer, calls
  beside another stop were dropped, a call without an id ran and was answered with
  `tool_use_id: ""`, and two calls sharing an id both ran. The loop refuses all of these before
  running anything; an id may not repeat anywhere in the history, which also keeps compaction
  (which tracks results by id) from skipping results that shared an empty id.
- P3, base-URL overrides: the loopback check trusted any host starting with `127.`
  (`127.attacker.example`); an override with credentials was logged verbatim and had its user name
  read as the host; a scheme-less override was dropped silently; a path doubled the provider's own
  (`/v1/v1/...`). Overrides are parsed as URLs now: only a plain `http(s)` origin is used, anything
  else is ignored with a warning that names the reason but never the value, and a refused explicit
  URL falls back to the environment one before the default.
- P3: the daemon client read replies with no size limit (now 64 MiB, through the daemon's own
  bounded reader); a request over the daemon's 10 MiB limit was written anyway and surfaced as a
  broken pipe (now refused before writing, naming the limit); and a timed-out governed request was
  sent again while the first attempt could still be running (now not retried after a timeout).
- P3: a client that failed to build fell back to `Client::new()`, which panics in that case and
  would have followed redirects; the build error is now returned by each request. Timeouts read as
  "invalid response: error decoding response body"; transport errors now say what happened (a
  timeout, with which limit, or the cause chain).

## Verification round
A read-only reviewer probed both commits (29 probes, loopback only) and confirmed one P1 that this
lane introduced: an override the stricter parser refused fell back to the provider's public API,
with the key and the whole prompt. That hit common values that had worked or failed safely before:
OpenRouter's `https://openrouter.ai/api`, Cloudflare gateway paths, Ollama's
`http://localhost:11434/v1`, credentials in the URL, IPv6 zone ids. The only signal was a tracing
warning most entry points never show. Fixed: an override that is set but can't be used fails every
request with an error naming the variable, never falling back; a path prefix and credentials are
kept again (credentials and paths are never logged); only a missing scheme or host, a query or a
fragment are refused. CLAUDE.md no longer says a typo degrades to the real API.

Also fixed from the round:
- A malformed 200 body was quoted whole in the error (serde quotes an unexpected value in full);
  it is cut to 2,000 characters.
- Rejecting a call id used in an earlier turn wedged a session with servers that reuse one id per
  response (every later tool turn failed). Within one response a shared id is still refused; an
  id repeated from an earlier turn is renamed `{id}-{n}`, in the history and in its result.
- `model_context_window_exceeded` (output cut off at the context window) counts as a token-limit
  stop, so its tool blocks are not run.
- `ImpulseAgent::with_test_provider` still sent the old sampling; and honouring the config's
  2048-token default halved the API agent's replies (harness and governed requests had always
  used the setting). The default is 4096, the API agent's previous cap.
- `base_resp` and `error` are read loosely: a numeric-string status code counts, and `error`
  values of null, `false`, `""` or `{}` are no error.
- `localhost.` and IPv4-mapped loopback (`[::ffff:127.0.0.1]`) count as loopback.

Recorded: `impulse-desktop/src/daemon_ops.rs` has its own daemon client with the same gaps
(unbounded reply reads, no size check before writing, retries after a read timeout); and a reply
cut off at the token limit without tool calls still returns as complete (in harness mode too).

## Not fixed
- F11 (`daemon --stop` only pings): fixed on `claude/code-cleanup-20261004` by `278942b`.
- F13's daemon side: `RegisterGovernedTask` takes no per-task lock, so two concurrent
  registrations of one task can still race. The client no longer creates that race after a
  timeout; a lock in `daemon/handlers.rs` belongs with the cleanup branch's governed fixes.
- History design note from the review: tools already run in earlier rounds leave history if a
  later round fails, so a retry can repeat their side effects.
- `BaseProvider::with_base_url` (the documented explicit override) is reachable only inside the
  module; no provider constructor exposes it.

## Evidence
- Revert proofs: 18 cases, each fix reverted alone; every case fails its tests. The client-build
  fallback (F12) has none: reverting it would send the fake key to the real API.
- One test hung instead of failing on its first revert proof: it wrote an oversized request into
  an in-memory pipe nobody read, so the reverted check blocked on the full pipe. It drains the
  pipe concurrently now and fails in 0.2 s against the revert.
- Gate (`CARGO_TARGET_DIR` isolated per lane, `*_BASE_URL` unset): build clean;
  `cargo test --workspace` 3132 passed, 0 failed, 9 ignored; clippy clean with and without
  default features; fmt clean; `python3 docs/validate_docs.py` 189/189.
- Verification round: 9 more revert proofs, all failing against their reverts; none can reach the
  network (the unusable-override test checks `endpoint()`, since a test calling `chat` would reach
  the real API if the fail-closed rule regressed). Gate after the round: `cargo test --workspace`
  3138 passed, 0 failed, 9 ignored; clippy clean with and without default features; fmt clean.

## Handoff
- Open a PR when James approves; run a verification round first.
