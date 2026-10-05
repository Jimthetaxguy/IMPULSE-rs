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
- Shared paths edited: `CLAUDE.md` (Architecture section only)
- Blocked paths (edited by the unmerged stack: photon, blackboard, model provider, Dioxus 0.7):
  `impulse-rs/src/ion_repl/**`, `impulse-rs/src/llm_backends/**`, `impulse-rs/src/model_endpoint/**`,
  `impulse-rs/src/state/config.rs`, `impulse-rs/src/handlers/config.rs`, `impulse-rs/src/test_support.rs`,
  `impulse-rs/src/lib.rs`, `impulse-rs/impulse-desktop/{Cargo.toml,src/ui.rs,src/views.rs,tests/desktop_contract.rs}`,
  `Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, `VISION.md`, `CONTEXT.md`
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: review pass complete. 32 commits: 29 fixes and 3 documentation corrections.
  Behavior changes carry regression tests, and most were checked by reverting the fix and watching
  the test fail. Gate evidence goes in the PR description. Not merged; needs a PR and review.

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
- **Agent harness output** is still collected with `wait_with_output`; it is the user's configured
  CLI under a 120 s timeout, so lower risk than tool output.
- **Multi-process `State`** never reloads and rewrites whole files without a lock (accepted for
  hooks in SECURITY-REVIEW Issue 3); the legacy TUI still auto-types context into PTYs.
- **Docs contract check** fails on three documents past the 120-day staleness threshold
  (`COLLABORATIVE-AGENTIC-CODING.md` and two May lane cards); they need a real review, not a date
  bump.

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
