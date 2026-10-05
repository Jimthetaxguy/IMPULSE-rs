---
title: Genome, tools and docs fixes
description: Work card for genome-tools-fixes-20261005
updated: 2026-10-05
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, genome, tools, docs, storage, review]
---

# Genome, tools and docs fixes

## Lane Facts
- Owner: claude (Opus 5.5, under James's standing goal "continue cleaning up and reviewing the
  code")
- Role: implementer, from a read-only review of four modules no earlier review had covered:
  `src/docs/`, `src/tools/`, `src/memory/` and `src/agent_discovery/`. The review reported 1 P1,
  4 P2 and 12 P3 issues, 13 reproduced.
- Branch: `claude/genome-tools-fixes-20261005`, **stacked on `claude/code-cleanup-20261004`**
  (`044fc5a`): that branch rewrites `handlers/memory.rs`, `storage/mod.rs`, `handlers/system.rs`
  and already fixes the P1 (re-running `init` wiped GENOME.md and config.json). Merge it first.
- Worktree: `.worktrees/term-context-20261005` (shared with the other 2026-10-05 lanes, one branch
  at a time)
- Owned paths: `src/storage/mod.rs` (temp names, `lock_exclusive`), `src/memory/mod.rs`,
  `src/handlers/memory.rs` (`add-decision`), `src/tooling/builtin/genome_read.rs`,
  `src/tools/{mod,init,update,system}.rs`, `src/docs/{fetch,cache}.rs`, the `docs`/`tools`
  handlers and one test in `src/handlers/system.rs`, this card
- Blocked paths: `Cargo.toml`, `Cargo.lock`, `CONTEXT.md`, `src/state/config.rs` and
  `src/handlers/config.rs` (so `model set` stays as found)
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented and gated; pushed; not merged; needs a verification round, then a PR
  after the cleanup branch.

## Findings fixed
- P2: concurrent `add-decision` calls lost decisions reported as added (two writers of 40 each
  kept 41). The read-modify-write of GENOME.md now holds an exclusive `flock` on
  `GENOME.md.lock` (`Storage::lock_exclusive`, after the daemon lock's pattern); a filesystem
  without `flock` runs unlocked with a warning. A decision that repeats the last one now says it
  wasn't added.
- P2: `system_info` with `include_env` returned every `IMPULSE_*` value to voice and MCP
  callers, including the operator capability token (`IMPULSE_OPERATOR_CAPABILITY`). Only an
  allowlist of informational variables keeps its values; any other `IMPULSE_*` name is listed
  with its value withheld.
- P2: OpenCode was installed and updated with `pip install opencode`, a PyPI name that is not
  OpenCode's (whoever registers it would get code execution through `tools init`). It uses its
  npm package `opencode-ai` now, as opencode.ai/docs says.
- P2: GENOME.md is stored as JSON, but `genome_read` looked for Markdown `## ` sections in it,
  so agents were told no decisions existed. A JSON genome is read as the Markdown it renders
  to; a hand-written Markdown genome is read as before.
- P3: atomic-write temp names were pid plus sub-second nanoseconds, which on macOS are whole
  microseconds, so two writes in one process within a microsecond collided ("File exists"; the
  review saw over 500 of 1,200 synchronized saves fail). A per-process counter is part of the
  name now.
- P3: tool version checks and installers had no time limit. Checks get 10 s and installs or
  updates 15 minutes; a check that times out is reported for that tool instead of reading as
  "not installed" (which would reinstall it). `tools update` reported the updater's output as
  the new version and now asks the tool again; `tools check` claimed every tool was "up to
  date" without checking and now reports the installed versions only.
- P3: `docs fetch` with `OPENAI_API_KEY` hid a failed OpenAI call and cached a list with every
  OpenAI model gone, marked fresh from the API; the failure is an error now and the cache is
  left alone. The fetcher's `Client::new()` fallback (panics on a TLS failure, no timeouts) is
  an error too.
- P3: a docs cache timestamp in the future never went stale; an unknown `docs` subcommand
  exited 0. The cache test used a fixed shared directory that concurrent runs deleted from under
  each other.
- P3: a line break in a recorded decision started a section of its own in the rendered genome,
  and dates rendered as Markdown links; entries stay on one line now.
- Test hazard: `test_chat_valid_modes_accepted` called `handle_chat`, which with an API key in
  the environment sent a real request to api.anthropic.com. It checks `InjectionMode::parse`.

## Not fixed
- `model set` stores a value nothing reads (`state/config.rs` is blocked here).
- The committed `impulse-rs/.impulse/impulse-capabilities.json` is out of date with the tool
  registry, and nothing checks it; the next daemon or `mcp serve` there rewrites it.
- The capability summary the TUI injects is built without `IMPULSE_CAPABILITIES_PATH`, so it
  misses external tools and custom agent registries.
- The built-in Anthropic model list has invented and retired ids; OpenAI context sizes and prices
  are guessed from model names.
- `tools/benchmark.rs`: `compare_results` has the names backwards and no callers; zero iterations
  give NaN statistics.
- Tests that can't fail or that run the machine's real CLIs (`tools list`/`update`), dead code
  (`memory::HistoryEntry`, unused benchmark and docs helpers), wrong documentation links, and a
  health check that calls an unparseable GENOME.md healthy.
- The version-check timeout uses this branch's `process_util`, which still waits for a
  grandchild holding the pipes; `claude/subprocess-delegation-fixes-20261005` fixes that.

## Evidence
- Revert proofs: 11 cases, each fix reverted alone; every case fails its tests. The docs fetch
  error has no test: the OpenAI URL is fixed, and a test would reach the real API.
- Gate on top of `044fc5a`: build clean; `cargo test --workspace` 3284 passed, 0 failed,
  8 ignored; clippy clean with and without default features; fmt clean;
  `python3 docs/validate_docs.py` 189/189.

## Handoff
- Merge `claude/code-cleanup-20261004` first, then open a PR for this branch when James approves.
- Run a verification round first.
