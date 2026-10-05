---
title: Code cleanup and review sweep
description: Work card for code-cleanup-20261004
updated: 2026-10-04
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, cleanup, review]
---

# Code cleanup and review sweep

## Lane Facts
- Owner: claude (Opus 5.5, session with James; goal "continue cleaning up and reviewing the code")
- Role: reviewer and implementer
- Branch: `claude/code-cleanup-20261004`, from `origin/main` at `7481457`
- Worktree: `../IMPULSE-rs.wt-cleanup`
- Owned paths: this card; fixes for review findings in modules the pending branch stack does not
  touch (see Blocked paths)
- Shared paths edited: `CLAUDE.md` (Architecture section only), and
  `impulse-rs/src/handlers/config.rs` (`6159ab7`, `99e858a`) although it is listed as blocked below.
  `git merge-tree` trial merges of this branch with all nine pending branches were clean on
  2026-10-04.
- Blocked paths (edited by the unmerged stack: photon, blackboard, model provider, Dioxus 0.7):
  `impulse-rs/src/ion_repl/**`, `impulse-rs/src/llm_backends/**`, `impulse-rs/src/model_endpoint/**`,
  `impulse-rs/src/state/config.rs`, `impulse-rs/src/handlers/config.rs`, `impulse-rs/src/test_support.rs`,
  `impulse-rs/src/lib.rs`, `impulse-rs/impulse-desktop/{Cargo.toml,src/ui.rs,src/views.rs,tests/desktop_contract.rs}`,
  `Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, `VISION.md`, `CONTEXT.md`
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status:
  - review pass, adversarial refutation round, and six verification rounds complete;
  - each round found and fixed regressions in the previous round's fixes; the fourth found two
    P2s, the fifth one pre-existing P2 and only P3 regressions, and the sixth one P2 in a
    round-5 fix, all fixed;
  - behavior changes carry regression tests, nearly all checked by reverting the fix and watching
    the test fail;
  - gate evidence goes in the PR description;
  - not merged; needs a PR and review.

## Fixed on this branch
| Area | Commits |
|---|---|
| Agent harness and API mode | `95bdf4e` reply captured again (stdout was never piped since 2026-07-11), summary cut on a char boundary; `3b8768e` API-mode requests no longer re-send every past turn |
| Hooks and CLI exit codes | `b2d8638` hook stdin never blocks; `c2afe88` PreToolUse guard hook blocks with exit 2 and reads stdin JSON, custom rules survive disable/enable; `99e858a`, `1c74f05` failing commands exit non-zero; `278942b` `daemon --stop` refuses instead of lying or starting a daemon |
| State and storage | `6159ab7` `init` keeps existing files; `dddee6d` only the daemon reconciles producer reservations; `fe05552` locks wait instead of failing, a session survives a failed history append; `58400bb` JSONL appends are one write and survive torn tails; `049749d` atomic writes remove their temp file on failure and sync the directory |
| Tools and sandboxing | `cc062a9` `file_write` checks and writes the same physical path; `d76ce7a` `bash_exec` and manifest tools bound output while reading it, `timeout_secs` past the session limit is capped by name, timed-out manifest tools are killed with their process group; `e512d2f` built-in Block rules catch ordinary command shapes; `2f3dbc5` ADR-0021 states what Monty's `max_memory` really bounds |
| Voice | `fbba874` webhook requires a secret unless `--allow-unauthenticated`, bounded header and body reads, TCP confirmations are never trusted |
| Retrieval and memory | `6a060c7` keyword hits rank strongest first and match words; `e1698f8` embedding subprocess output is drained while it runs; `e5c5dc3` rebuilds touch only their scope and never promoted memory; `75dd809` search paging and `--total` work |
| Daemon | `cdd585f` startup fallbacks are logged once logging exists; `7642a49` control characters rejected in tracked paths, tool names, summaries |
| Governed tasks | `5d60aa0` a retried promotion lands, superseded pins are discardable, decided memory candidates still match |
| Desktop and terminal | `6b9e3de` PTY writes no longer hold the runtime lock; `f978ba7` a missing working directory is refused instead of spawning in `$HOME` |
| Other | `a42c5d4` sccache setup edits cargo config without duplicating keys; `c19a1dd` session-start honors the configured injection mode; `5e28d78` workbench snapshots keep reviewed artifacts; `0e98643` steward parses real transcripts; `e63942c`, `2caece0` CLAUDE.md current |

## Refutation round (2026-10-04)
Three read-only reviewers, each with its own source export and build cache, tried to refute every
commit above with reproductions. They confirmed 2 P1, 6 P2 and about 20 P3 findings, several of
them regressions in this branch's own fixes. Fixed in 11 commits (`f805419`..`88fef92`), each
behavior change with a test that fails against the previous code:

- P1 `f805419`: a failed JSONL append truncated records other processes had appended and synced
  (58400bb's repair; reproduced across processes: 52-59 lost, now 0).
- P1 `0abe1a1`: a blocked PTY write still held the cockpit's one-at-a-time command queue, so
  `close` waited behind it; pane input now goes through a per-pane writer thread.
- P2 `6bdd921`: a retried promotion took any branch at the accepted commit for its own swap (now
  requires its reflog entry); a live Builder's worktree with an incomparable pin is stopped first.
- P2 `d033932`: CLI-run manifest tools stay in the CLI's process group (Ctrl-C, terminal reads).
- P2 `45bcbac`: guard Block rules match git's subcommand position, quoted and wildcard refspecs,
  `--mirror`, redirections, any case; no longer block a commit message that mentions force-push.
- P2 `385b4ea`: the guard hook is anchored on `$CLAUDE_PROJECT_DIR/.impulse`, blocks payloads it
  cannot read, checks NotebookEdit, and keeps a user's override of a built-in rule.
- P2 `25bf884`: sccache setup keeps cargo config valid with dotted `build.*` keys, quoted headers,
  a BOM, and multi-line strings; only an sccache program counts as configured.
- P3 batches `aebc335`, `19d41f4`, `388967e`, `88fef92`: CLI/daemon/state, tools/voice/MCP/Monty
  docs, retrieval, and the agent's turn history (details in each commit message).

Verification round: the same three reviewers reran their probes against the fixes. 26 of 29
findings were fixed. They found three more regressions in the fixes themselves, each fixed with a
test:

| Regression | Commit |
|---|---|
| sccache key inserted inside a multi-line array; the planner now edits only one-line configs | `b283c3f` |
| A short write left a torn `MEMORY.jsonl` tail that fails every load; that single-writer log now cuts back on error | `ae147f7` |
| Guard patterns lost quoted `-C`/`-c` values and `main -f;` | `8fcb200` |

The partial fixes it found are completed in `986ba23` and `af63095`:
- agent-configure validates everything before writing;
- bidi controls are refused in every validated field and in direct mode;
- the guard hook blocks when its `.impulse` is missing, and on edits without text;
- AgentAssist remembers the request;
- ProcessTool keeps its process group, with `tooling-run` handling Ctrl-C;
- the semantic prefilter applies only at a full pool;
- pane input is capped by bytes.

Also from the recorded items:
- `021b0f5`: one daemon per project by `flock`, and backoff after accept errors.
- `b8cee15`: bounded benchmark work; the unused, unsound path validator is deleted.
- `1fc1e7d`: tools default to the session's impulse dir.

Second verification round: the reviewers reran everything against `c5846b7`. Every earlier item
held. In the latest fixes they confirmed four issues and suspected five more; all nine are fixed:

| Issue | Commit |
|---|---|
| One pane write over the 16 MiB cap was refused even with nothing queued | `84493f1` |
| The semantic candidate pool grew with the page window, so later pages could rank another set | `84493f1` |
| Right-to-left and Arabic letter marks were refused as bidi controls | `d2789af` |
| In a project without `.impulse`, the guard hook blocked every call, `impulse-rs init` included | `d2789af` |
| The daemon refused to start where `flock` is unsupported | `d2789af` |
| An unreadable log length let the memory-log cut-back erase the log | `d2789af` |
| The benchmark trusted one run's time; the timing loop now stops at a deadline | `59b4e4b` |
| An escaped quote inside a quoted `-c` value hid a force push from the guard | `59b4e4b` |
| A Ctrl-C handler install error read as an interruption in `tooling-run` | `59b4e4b` |

`3c0ba8d` fixes guard mismatches from the same probe that no commit had claimed:
- a force push to a branch whose name contains `main` (`feature/main-menu`) was blocked;
- `--all` with a force flag was not blocked;
- `rm` with `\/` or `--rec` was not blocked.

Five of the nine fixes have tests that fail against the previous code. The other four have no
test:
- the candidate pool no longer takes the page as an input, so there is nothing to vary;
- the lock fallback, the cut-back guard and the Ctrl-C install error depend on OS failures a test
  cannot provoke.

Two more recorded items are fixed:
- `d718b92`: the Keychain provider lists this service's secrets natively; `list` never parsed
  `security` output.
- `ba9c873`: the voice webhook handles at most 64 connections at once and closes the rest.

Third verification round (against `3c0ba8d`):
- Reviewer C confirmed both of `84493f1`'s fixes with probes:
  - a 17 MiB write to a reading pane goes through;
  - eight concurrent large writers to a stalled pane admit one;
  - pages of 300 and 1200 matches agree with earlier pages;
  - putting the old pool formula back reproduced the inconsistency.

  Its one cosmetic suspicion is fixed in `6c48541`: a failed writer's error now comes before the
  byte cap.
- Reviewer A confirmed every `d2789af` and `d718b92` item. It found one P2 regression in
  `d2789af`, fixed in `cf901d5`: Claude Code started in a subdirectory sets `CLAUDE_PROJECT_DIR`
  there, so the hook ran on the built-in rules and skipped the project's custom Block rules. The
  hook now uses the nearest `.impulse` up to the repository root. It was checked end to end with
  the built binary. A project outside any git repository, started in a subdirectory, still runs
  on the built-in rules.

  The same commit:
  - treats EACCES from `flock` as a held lock;
  - refuses unknown lock errors;
  - runs the Keychain `list` non-interactively, like `get`, `set` and `delete`.
- Reviewer B confirmed:
  - the benchmark deadline: its repro now stops at 2.04 s instead of running 5.94 s;
  - the escaped-quote fix;
  - Ctrl-C after the new wait;
  - the claimed guard shapes;
  - that the webhook cap leaks no permits.

  It found:
  - A P2 regression in `3c0ba8d`, fixed in `bf647d8`. A newline counted as a space inside a
    command, so pushing main followed by any line with an `-f`-style flag, such as
    `tail -f build.log`, was blocked.
  - In the same pattern, three bypasses, all blocked again in `bf647d8`: a redirection right after
    the ref, brace expansion, and a line continuation between the flag and the ref.
  - A webhook client with no secret that never finished its headers held a slot for the whole
    30 s request budget, and could reconnect to keep it. The header read now has a 5 s deadline
    (`a5b940e`).

Recorded items fixed after round 3:
- `c6b7a07`: `dir_size` counts links instead of following them, so a link back up a target tree
  no longer multiplies the walk, and `build_health` walks under `spawn_blocking`.
- `ce98af3`: a triple quote inside a one-line string or comment no longer hides a configured
  sccache wrapper from the status check.
- `9cc82f2`: webhook replies close cleanly. A 401 or 413 left the request body unread, and
  closing with unread bytes reset the connection, often before the client read the reply.
- `0e758d1`: the webhook decodes chunked request bodies, with caps on the decoded body, the
  encoded bytes and each line, instead of answering 400.
- `c245765`: the harness CLI's output is captured with caps (8 MiB of reply, 64 KiB of stderr), and a
  reply over the cap is an error instead of being kept.
- `b39dbdd`: each benchmark run gets only the time left before the deadline, so a run that starts
  just before it no longer overshoots by up to the sandbox's 5 s budget.
- `3e5f00f`: `end_session` claims the session under the state lock and writes its history after
  releasing it, instead of holding the lock across the append's fsync; a failed append puts the
  session back.
- `4daabf5`: every build-hygiene tool (`build_health`, sweep, wipe, clean-all, the sccache tools,
  `tool_availability`) runs its walks and `cargo`/`sccache` subprocesses off the async runtime.
- `5eff5de`: `python_exec`, `calculator` and the benchmark run the sandbox off the async runtime.
- The docs contract check passes. `7cfad38` closed out the two stale May lane cards. The next
  commit reviewed the collaborative coding guide against current practice:
  - its gate matches `CLAUDE.md`;
  - it covers per-lane target dirs and regression tests shown to fail with the fix reverted;
  - it adds a review-before-ready section.

Fourth verification round (against `5eff5de`), over the 13 code commits since `3c0ba8d`:
- Every claimed fix held except the two below.
- Two P2s:
  - `efa9c7b` fixes a regression in `3e5f00f`. `end_session` took the session out while writing
    its history, so another session's save dropped it, and a failed append lost it (14 of 20
    trials). It now stays in place, marked as ending, until its history is written.
  - `47b8ed8` fixes a pre-existing bug. Project discovery followed directory links out of the
    search root, listing one outside project 31 times. `clean-all`'s fallback would have run
    `cargo clean` there each time. Discovery now visits each real path once, inside the root.
- P3s fixed:
  - `47b8ed8`: `dir_size` counted every hard link.
  - `11ec722`: a body that stops arriving now gets a 10 s deadline and a 408; Transfer-Encoding
    with Content-Length gets 400; any coding but `chunked` gets 501.
  - `226f404`: an empty `.git` file no longer ends the guard's `.impulse` search.

Fifth verification round (against `226f404`):
- Every round-4 fix held. Reviewer A's race probe lost an ending session 0 of 20 times (was 14),
  and a 64-session stress run showed no deadlock, duplicate or stranded mark.
- One P2, pre-existing, fixed in `bd65e39`. Without cargo-clean-all, `clean-all` ran a bare
  `cargo clean` per project. That cleans the configured build directory, which with this machine's
  global `build.target-dir` is shared. It now passes `--target-dir <project>/target`.
- P3s fixed:
  - `bd65e39`: sweep, wipe and clean-all skip a project whose `target` links out of the root;
    a native sweep had deleted files through such a link.
  - `926657b`: a regression in `47b8ed8`, where overlapping search roots dropped deep projects.
  - `c84ca88`: tracking an ending session is refused instead of being silently lost.
  - `23c8562`: the guard's repository check follows git's own rules.
  - `7312ca7`: malformed header lines get 400.

Sixth verification round (against `7312ca7`):
- Every round-5 fix held, with no false positives on realistic webhook traffic.
- One P2, fixed in `fb48db2`: `cargo clean --target-dir X` also removes cargo's configured
  `build.build-dir`, so `bd65e39` could still delete a shared directory. A relative scan path also
  made it clean nothing. The fallback now removes the project's own `target/` itself.
  `projects_to_clean` also skips any `target/` without cargo's `CACHEDIR.TAG`, as cargo does.
- P3s fixed:
  - `db9644d`: a FIFO named `.git` hung the guard hook, a regression in `226f404`. Gitfiles are now
    read with git's exact format.
  - `4c711ed`: control characters and non-token field names are refused, a non-UTF-8 header block
    gets 400 instead of a misleading 401, and the request line must be exact.

Still recorded, not fixed:
- `file_write`'s check-then-write race against a concurrent same-user process swapping symlinks
  (needs directory-fd opens).
- A write root naming a single file is refused. Confirmed by reading: the atomic write puts its temp
  file in the parent directory, which lies outside such a root, and `file_write` refuses that on
  purpose. Allowing it means choosing between a temp file outside the root and a non-atomic in-place
  write. No default root is a single file.
- A manifest tool that reads the terminal is stopped by the OS and ends at its timeout.
- A bare `git push --force` while on main is not caught (the rule cannot see the current branch).
- The recursive-delete Block rule stays broad (any absolute or home path) because narrowing it is
  a policy choice.
- sccache setup refuses a config with any multi-line value, including a `[build]` table that
  `25bf884` used to edit (by design since `b283c3f`).
- The Keychain `list` matches on the server alone, while `get` and `delete` also require the
  default authentication type. Matching that too needs the approval-gated round-trip test
  (`#[ignore]`, it writes to the login keychain) to confirm how the attribute reads back.
- The office tools still parse synchronously on the runtime thread when called over MCP or the
  daemon; they are not exposed over the webhook. They need their own `spawn_blocking`: a blanket
  wrapper would break the kill-on-drop cancellation that process tools rely on.
- A webhook client that reconnects as soon as it is cut still keeps every slot: the header and
  body deadlines only shorten each hold, and the 2 s drain after a refusal holds one too. This
  needs per-source or accept-rate limits.
- `sweep`, `wipe` and `clean_all` work run under `spawn_blocking` keeps going after the request
  that started it is cancelled (it used to block the runtime instead). Stopping it needs a stop
  flag through the deletion loops.
- In the guard's `.impulse` search the nearest one wins, so a nested `.impulse` with guardrails off
  shadows the root's rules for a session started under it; a nested worktree's `.git` file ends
  the search. Outside a git repository a session started in a subdirectory gets the built-in
  rules.
- Discovery canonicalizes every directory to stay inside its root, about 2.2 times the old
  walk time (15 µs per directory); small next to sizing build directories.
- A webhook connection can hold its slot for about 17 s before a 408: up to 5 s of headers, 10 s
  of body and the 2 s drain, all within the 30 s request timeout.
- With cargo-clean-all or cargo-sweep installed, `clean-all` and `sweep` run those tools per search
  path instead of the native code. The tools bypass the link, `CACHEDIR.TAG` and root checks, and
  cargo-sweep may find a shared global build directory through `cargo metadata` (suspected; neither
  tool is installed here). Whether to prefer the native code is a decision for James.
- The sqlite-vec search pool still follows the page window. Its order is the same for any pool
  size except between results at exactly the same distance.

## Recorded, not fixed (each needs a decision or its own lane)
- **Office tools are unbounded** (`excel_read`, `word_read`, `document_parse`, the `office` CLI,
  plugins; reachable over MCP and the daemon). Calamine's dense `worksheet_range` lets two cells at
  opposite corners of a sheet demand billions of cells; DOCX and XLSX parse with no inflation or
  file-size cap. Ion's `document_read` already has all three bounds. Recommended: move its bounded
  reader (`preflight_container`, `extract_workbook`, `extract_word`) from `ion_repl::tool_document`
  into `office` so both surfaces share one implementation, and refuse legacy `.xls` at these entry
  points too, since calamine builds every `.xls` sheet's dense grid when it opens the file.
- **Session hooks pass literal variable names.** The installed SessionStart/SessionEnd templates
  single-quote `$CLAUDE_PROJECT_NAME` and `$CLAUDE_SESSION_SUMMARY`, and Claude Code sets neither;
  its hooks get a JSON payload on stdin. Needs a design for where the session name and summary
  come from.
- **Monty memory** (ADR-0021 follow-up 1): bounding gradual growth needs `monty-alloc` as the global
  allocator (process-wide) or `monty-pool` workers under an OS memory limit.
- **Daemon stop**: no stop request or signal handler exists; `daemon::mod` already lists the
  handler as follow-up work, and `--stop` now says so.
- **MCP TCP transport** is unauthenticated and opt-in; recommend a token like the voice webhook's, or
  removing it.
- **PiAdapter** speaks a protocol Pi's RPC does not implement, so `ion-verify` never returns a
  verdict through it.
- **Promotion `reset --hard`** would destroy exempt tracked memory files once the ADR-0020 decision
  endpoint ships; latent today.
- **Multi-process `State`** never reloads and rewrites whole files without a lock (accepted for
  hooks in SECURITY-REVIEW Issue 3); the legacy TUI still auto-types context into PTYs.

## Decisions
- 2026-10-04: review `main` module by module with read-only reviewers, verify every finding against
  the code before fixing, and fix only what reproduces. Findings inside blocked paths are recorded
  here for the owning branch instead of being fixed on this one.
- 2026-10-04: `CLAUDE.md`'s Ion bullet and daemon paragraph described superseded code
  (`checkout_agent`/`checkin_agent`, a `security`-CLI Keychain writer, "no FileWrite guardrail rule",
  a two-tool confirmation gate). Rewritten as current invariants with pointers; chronology stays in
  Git history and `impulse-rs/impulse-ion/TUI_SPEC.md`.

## Handoff Notes
- Hook stdin hang (fixed here): `handlers::common::read_hook_stdin_payload` ran an unbounded
  `read_to_string` on any non-terminal stdin for `session-start`/`session-end` (direct and daemon
  dispatch), so a caller that left stdin as an open pipe hung forever; a `cargo test` started from
  such a shell hung in `handlers::session` and `direct_dispatch` tests. The payload's only consumer
  is hook evidence, so it is now read only when `IMPULSE_HOOK_EVIDENCE` is on, capped at 1 MiB, and
  abandoned after 2 s.
- Tests that could not fail (fixed here): six `direct_dispatch` tests ended in `let _ = result`
  (two named `..._returns_err` for handlers that deliberately fail open); they now assert the
  fail-open contract and that no session or history was created. The print-helper tests in
  `handlers/common.rs` now assert on text from new pure formatters; the session-start banner tests
  no longer race on `IMPULSE_HOOK_SENTINEL`. An ignored verify-dispatch test that asserted nothing
  was removed. `VerificationReport::success()` no longer reports a pass for zero checks.
- Worktree audit (2026-10-04): of the 34 worktrees under `.worktrees/`, 16 are clean and belong to
  merged PRs whose commits all landed (`document-read-hardening-20260902`'s extra commits landed via
  #49), and `governed-task-run`'s commits are all in `main` too. Nine are pushed but unmerged with
  no PR (`agent-cache-serialization`, `agent-truth-parity`, `code-wiki-baseline-20260828`,
  `desktop-daemon-truth-wire`, `dioxus-egui-retirement`, `dioxus-packaged-acceptance-20260830`,
  `dioxus-release-truth-20260829`, `harness-evolution-adr`, `live-daemon-truth-integration`) and
  need keep-or-close decisions. Every local-only commit is on `origin` under `backup/*` (new today:
  `backup/claude-desktop-ux-functional-fixes-local-20261004`, two 2026-07-21 desktop commits that
  never reached `main`). Five stale worktrees hold uncommitted changes (`base-url-override`,
  `governed-role-launch`, `legacy-ui-retirement-plan`, `legacy-ui-retirement-rewrite`,
  `pr51-verification-guide-20260912`); removal waits for James.
- Pending stack: the blackboard branch now carries the IMPULSE_HOME test-race fix (`b73e22d`);
  the provider (`d006855`) and VISION (`bca4bd8`) branches were restacked onto it; the provider
  tree is byte-identical to its gated tip `88843b2`.
- Restriction-lint audit (`clippy::unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`,
  `dbg_macro`, `undocumented_unsafe_blocks`) over non-test code at `7481457`: 27 sites, all
  `expect`/`unwrap` backed by local invariants (piped stdio, generated IDs, guarded `is_empty`).
- Dependabot "security update not possible" failures on `main` (2026-09-27): the three open alerts
  are transitive pins, not lockfile bumps. `rand` 0.7.3 comes from `phf_generator` 0.8 (build-time
  codegen under `selectors`), `lru` 0.12.5 from `ratatui` 0.28.1, `glib` 0.18.5 from the GTK stack
  of `wry`/`tao` (Linux webview, both Dioxus desktop and the optional legacy Tauri adapter).
