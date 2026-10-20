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
  `src/tools/{mod,init,update,system,list}.rs`, `src/docs/{fetch,cache}.rs`, the `docs`/`tools`
  handlers and one test in `src/handlers/system.rs`, this card. Round 2 adds the tool path check
  (`src/tooling/traits.rs`, a test in `file_read.rs`), the TUI's text cuts (`src/ui/`), the
  init header (`context_lifecycle/templates.rs`, a test in `injector.rs`), the MCP genome
  resource, `PLATFORMS.md`, and the temp names in `file_write.rs`, `retrieval/`,
  `notification/mod.rs`, `daemon/actor_provenance.rs` and `impulse-ops/src/lib.rs`.
- Blocked paths: `Cargo.toml`, `Cargo.lock`, `CONTEXT.md`, `src/state/config.rs` and
  `src/handlers/config.rs` (so `model set` stays as found)
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: round 2 (fixes from verification round 1) implemented and gated; needs
  verification round 2, then a PR after the cleanup branch.

## Findings fixed
- P2: concurrent `add-decision` calls lost decisions reported as added (two writers of 40 each
  kept 41). The read-modify-write of GENOME.md now holds an exclusive `flock`
  (`Storage::lock_exclusive`); a filesystem without `flock` runs unlocked with a warning. A
  decision that repeats the last one now says it wasn't added. (Round 2 moved the lock from a
  `GENOME.md.lock` file onto the `.impulse` directory; see below.)
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

## Round 2: fixes from verification round 1
Round 1 confirmed the lock across processes (300 of 300 decisions kept, 151 without it) and found
every earlier test meaningful, but reported two regressions in this lane's fixes, one leak the
`system_info` fix missed, and a TUI panic outside the lane.
- P2, regression: the lock file `.impulse/GENOME.md.lock` stayed behind untracked, in neither
  `impulse init`'s ignore list nor the governed cleanliness exemptions, so after one
  `add-decision` every governed claim and registration in that project failed as "dirty". The
  lock is on the `.impulse` directory itself now, so nothing is created. (A data file can't be
  locked, because the atomic write renames a new file over it; the directory stays the same
  file.) The directory is created first, which fixes the second regression: `add-decision`
  failed in a project without `.impulse/`. A planted lock-file symlink no longer matters either.
  The lock covers the whole directory and is not re-entrant, as its documentation says.
- P2: `file_read` is on the voice allowlist as read-only, so it runs unconfirmed, and the default
  read roots include `.impulse`. Through it, a voice webhook (or MCP) call read
  `.impulse/sockets/impulse.operator-cap`, the daemon's operator token, while `system_info` showed
  that token as withheld. `ToolContext::is_path_allowed` now refuses any `*.operator-cap` path for
  reading and writing, whatever the roots. The check runs after symlinks are resolved, and the
  extension is compared without case. That covers `file_read`, `file_write` and Ion's sandbox
  check.
- P2, outside the lane and already there: the TUI genome view panicked when byte 30 of a decision
  fell inside a multi-byte character (an em dash was enough). The same byte cut was in ten more
  TUI places (session names and ids, preferences) and in the context-init header, which cut the
  session id at byte 8 outside the TUI. All of them cut on a character boundary now
  (`ui::visualization::prefix_on_char_boundary`; the header takes eight characters).
- P3:
  - `tools list` showed a check that timed out as "not installed"; it says "unknown" with the
    reason now (`CliTool::check_error`).
  - `tools update` said "updated to X" when nothing changed; it says "unchanged at X".
  - `PLATFORMS.md` still told readers to `pip install opencode`.
  - `docs status` printed "0 seconds ago" next to STALE for a stamp in the future; it says the
    stamp is in the future.
  - An empty `OPENAI_API_KEY` counted as a key, and with failed fetches now errors, it broke
    `docs fetch`. A blank key is no key.
  - A decision without `tags`, or the GUI scaffold's `"last_updated": null`, made the genome
    unreadable: the TUI showed it empty, `genome_read` found no sections, and `add-decision`
    failed. Both parse now. The three lists stay required, so JSON that isn't a genome is never
    read as an empty one that `add-decision` would write over.
  - The MCP `impulse://genome` resource served the stored JSON labelled `text/markdown`; it serves
    the rendered Markdown now, and JSON that isn't a genome as JSON.
  - The temp-name collision fixed in storage was also in `file_write` (46 to 100 of 480
    concurrent same-file writes failed), `retrieval/mod.rs`, `retrieval/pageindex.rs`,
    `notification/mod.rs` and `daemon/actor_provenance.rs`. They share
    `storage::unique_temp_token` now. In `impulse-ops`'s `atomic_write_path`, the temp name
    didn't even include the target, so two artifacts saved into one directory in the same
    microsecond shared a temp file. It has its own counter and the target's name now.

## Not fixed
- `init` can wipe a concurrent decision: `seed_if_missing` (`src/handlers/config.rs`, blocked
  here) checks and writes GENOME.md without the lock, and round 1 lost the decision in 259 to 290
  of 400 trials. The fix is to seed while holding `Storage::lock_exclusive`. The legacy GUI
  scaffold and the `file_write` tool also write GENOME.md unlocked.
- The unlocked fallback on a filesystem without `flock` warns through `tracing`, which the CLI
  doesn't set up, so the warning is silent there. On non-Unix platforms the lock does nothing.
- On NFS the directory can't be locked: NFS emulates `flock` with byte-range locks, which need a
  descriptor open for writing, and a directory can't be opened that way. `add-decision` falls
  back to running unlocked there (EBADF joins the fallback errors). Round 1's lock file did work
  on NFS; the trade is a clean workspace everywhere else.
- The lock waits without a limit and is not re-entrant (documented; no caller nests it).
- A timeout kills only the direct child (`process_util`, owned by the subprocess lane).
- `docs list --force` with a key and an unreachable API errors instead of listing the built-in
  models, now that a failed fetch is an error. Separately, and older, a keyless fetch stamps the
  built-in list `"source": "api"`.
- `add-decision` and the rendered genome drop fields `Genome` doesn't model, such as a top-level
  `patterns` list.
- Ten seconds may be too short for a cold Node-based CLI's `--version` under load (suspected,
  untested: the real CLIs could reach the network).
- When the model asks Ion for a capability file, Ion's confirmation prompt calls the path
  "outside the session's sandbox roots" and suggests `/allow`, which can't grant it
  (`ion_repl/chat.rs`, blocked here). The tool still refuses the path after confirmation.
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
- Revert proofs: 11 cases in round 1 and 20 in round 2, each fix reverted alone; every case
  fails its tests. The docs fetch error has no test: the OpenAI URL is fixed, and a test would
  reach the real API. The blank-key rule is tested as a function for the same reason.
- Round 1 gate on top of `044fc5a`: build clean; `cargo test --workspace` 3284 passed,
  0 failed, 8 ignored; clippy clean with and without default features; fmt clean;
  `python3 docs/validate_docs.py` 189/189.
- Round 2 gate: build clean; `cargo test --workspace` 3304 passed, 0 failed, 8 ignored
  (`impulse_rs` lib 2567/0/4, its integration tests and binaries 256/0/2, `impulse_desktop`
  176/0/0, `impulse_ops` 169/0/0, `impulse_term` 94/0/0, `impulse_ion` 23/0/1,
  `impulse_step_model` 16/0/0, doc tests 3/0/1); clippy clean with and without default
  features; fmt clean; `python3 docs/validate_docs.py` 190/190.

## Handoff
- Merge `claude/code-cleanup-20261004` first, then open a PR for this branch when James approves.
- Verification round 2 next; this lane is done when a round finds no P2.
