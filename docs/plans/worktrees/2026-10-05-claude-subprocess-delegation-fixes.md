---
title: Subprocess, semantic diff, delegation and webhook fixes
description: Work card for subprocess-delegation-fixes-20261005
updated: 2026-10-05
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, subprocess, semantic-diff, delegation, webhook, review]
---

# Subprocess, semantic diff, delegation and webhook fixes

## Lane Facts
- Owner: claude (Opus 5.5, under James's standing goal "continue cleaning up and reviewing the
  code")
- Role: implementer, from a read-only review of four modules no earlier lane had covered:
  `src/process_util.rs` with the `sem` runner, `src/delegation/`, `src/notification/` and
  `src/token_tracker/`. The review reported 9 findings with reproductions (4 P2, 5 P3).
- Branch: `claude/subprocess-delegation-fixes-20261005`, from `origin/main` at `7481457`
- Worktree: `.worktrees/term-context-20261005` (reused for incremental builds; the term-context
  branch it was made for is pushed)
- Owned paths: `src/process_util.rs`, `src/semantic_diff/{mod,runner}.rs`,
  `src/handlers/semantic_diff_handlers.rs`, `src/delegation/{mod,types,tracker}.rs`, the delegation
  handler in `src/daemon/handlers.rs` and its test in `src/daemon/tests.rs`,
  `src/notification/mod.rs`, the delegation section of `docs/IPC-PROTOCOL.md`, this card
- Blocked paths: `Cargo.toml`, `Cargo.lock`, `CONTEXT.md`, `src/ui/lifecycle.rs` (owned by
  `claude/term-context-fixes-20261005`, which takes the TUI webhook finding)
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented in `e427f87` (process), `31105c5` (sem), `5ea1d25` (delegation)
  and `e00da7e` (webhook); verification round fixed on top; gated; pushed. Not merged; needs a PR.

## Findings fixed
- P2, `run_with_timeout` (the `sem` runner and the secrets-manager CLI proxy): the deadline only
  covered `wait`. A child that exited while a process it started still held stdout kept the call
  waiting for that process (`sem` with a backgrounded child took 33 s against a 30 s limit), and
  nothing capped the output. The wait for the pipes now ends at the same deadline (plus a 250 ms
  drain grace once the child exits), stdout past 32 MiB kills the child and fails the call, stderr
  keeps its first 64 KiB, and a killed child is reaped for at most 2 s.
- P2, `sem diff` output: current sem names its fields in camelCase (`changeType`, `entityName`,
  `entityType`, `filePath`, per its README's "real output"), which the parser didn't read, so every
  real change came out as "modified unknown (unknown) in unknown". Any other JSON (null, a number,
  `{"error": ...}`) also became one fabricated change, which session end then stored. The parser
  now reads current sem output and the earlier shapes straight into typed records (skipping each
  change's full source text), and refuses anything else: unknown shapes, a change without a type,
  name, entity type or file, and unknown change types. sem's `reordered` counts as moved.
- P2, delegation tracker: nothing removed a delegation, and each kept its whole context snapshot
  (up to the 10 MiB request size) although nothing reads it. It now holds at most 256 (a full
  tracker drops its oldest finished delegation, and refuses a new one only when all are still
  active), keeps 64 KiB of the snapshot, and refuses a spec or a completion over 256 KiB of text.
- P3, delegation completion: `CompleteDelegation` was accepted again for a finished delegation and
  replaced the result the coordinator had already been handed. Completing, failing or assigning a
  worker to a finished delegation is now an error (`DelegationError`, a `thiserror` enum that also
  replaces the `Option`/`bool` returns).
- P3, conflict webhook: the client followed redirects. A 301 to 303 turned the POST into a
  body-less GET whose 200 was reported as delivered; a 307 or 308 re-sent the payload to whatever
  origin it named. Redirects are no longer followed and count as a failed delivery, and a client
  that fails to build is an error instead of a silent fallback to a default client (which would
  follow redirects and have no timeouts).
- P3, `sem` arguments: refs, file names and entity names starting with `-` reached sem as options.
  `sem diff` treats arguments after `--` as pathspecs, so a ref starting with `-` is refused
  (no git ref name starts with one); `sem blame` and `sem impact` get `--` before the name.
- P3, `sem-diff --session-id` ran `sem diff` twice and discarded the first result; `sem-status`
  waited on `sem --version` with no timeout (now 5 s).

Also fixed in passing: `test_webhook_fails_for_invalid_url` resolved a `.local` name over mDNS on
every run (16.7 s across three attempts); it now uses a closed loopback port (1.7 s). The IPC
protocol doc's delegation examples could not have been sent: the spec lacked the required `task`,
and `diff_summary` used `insertions` instead of `lines_added`/`lines_removed`.

## Verification round
A read-only reviewer reran the review's reproductions against these fixes, probed them (26 probe
tests, the full suite three times, the sem and process tests ten times each) and read sem's source
for its real output. It confirmed that both P2 claims fell short:
- The context snapshot was cut but kept its allocation (`String::truncate` frees nothing): a 4 MiB
  snapshot still held 4 MiB. It is now shrunk after the cut.
- The size limits counted text only, so 3.3 million empty target files (134 MB once parsed)
  passed as zero bytes. Every list entry now also counts 64 bytes.

And these P3s, all fixed:
- One entity with an empty name failed the whole sem diff. sem names a JSON entity by its key,
  and an npm lockfile's root package is keyed `""`, so adding a lockfile failed every session-end
  capture. Empty names are kept; a missing name is still refused.
- `sem blame` and `sem impact` never parsed sem's output, before or after the first round (the
  first card blamed camelCase; the shapes differ). Blame entries are flat (`name`, `type`,
  `lines`, a nullable `commit`, `summary`) and impact has an `entity` object, `dependents` and
  `impact.total`. Both are now read in those shapes, taken from sem's `blame.rs` and `impact.rs`;
  `SemanticBlameEntry::commit` is an `Option` for uncommitted lines.
- With a sem cloud login, `sem diff` prints its answer and then runs a relations pass of minutes,
  past the timeout. Every sem command now sets `SEM_LOCAL=1`, sem's own switch for local runs.
- sem's JSON carries each change's full source, and nested entities repeat their parent's text,
  so a complete answer can pass the 32 MiB default cap; `sem diff` now allows 128 MiB.
- A stopped child got SIGKILL only, which a launcher can't pass on: sem's npm wrapper died and left
  the real sem running. Children now get SIGTERM, then SIGKILL after 250 ms.
- After the call gave up, the stderr reader kept draining a backgrounded writer forever (a `yes`
  ran on at 60% CPU). Readers now close their pipe at the next write, so the writer gets SIGPIPE.
  A process that holds a pipe without writing still keeps its reader thread until it exits.
- A tracker full of abandoned delegations refused every new one until the daemon restarted, since
  nothing in the protocol fails or cancels one. A full tracker now also drops its oldest
  delegation pending or in progress for an hour or more.
- The sem-status regression test passed against the old handler, which ignores the test seam and
  found no `sem` on PATH. It now checks that the fake `sem` ran `--version`.
- A nested `with_test_sem` reset the outer setting instead of restoring it.
- The session-end warning printed only the outermost error (`{}`), hiding sem's own message; it
  prints the chain (`{:#}`) in both modes. Not covered by a test.
- On Linux a script written and run at once can fail with ETXTBSY while another test's fork holds
  it; the fake `sem` helper runs each script once first, retrying while busy. Not reproducible on
  macOS.
- The size error said the delegation was rejected when a completion was refused; it names the
  refused part now.

## Second verification round
A second reviewer probed the round's four commits (17 probes; the lib suite three times; the
process and sem tests fifteen times at 16 threads) against sem's source back to v0.5.0 and its
npm wrapper under node 22. No P1 or P2. Fixed from its P3s:
- Tool-trace records were weighed like strings (64) though one is three `String` headers (72
  bytes) plus allocations, and parsed lists kept up to double their room, so a completion could
  hold about twice the stated 256 KiB. Records now weigh 128, and stored lists are shrunk.
- The `SEM_LOCAL` test passed against the revert when the environment already exported
  `SEM_LOCAL=1`; it also checks the command itself now.
- "Blast radius" was sem's reach within its default two levels, reported as the total; the
  depth is passed explicitly and the output names it.
- The weight error said "bytes" for a weight; the test helpers' docs said "whole command line"
  for a prefix match.

Recorded, not changed:
- SIGTERM first means a child whose TERM handler signals its own group (`trap 'kill 0' TERM`)
  signals the caller, which shares that group. None of the programs run this way does; it is in
  the `run_with_timeout` doc.
- A leftover that holds a pipe without writing keeps the output its reader had read (up to the
  cap) as well as the thread, until it exits; the callers are short-lived CLI processes.
- A full tracker can drop a delegation still being worked on after an hour (the protocol has no
  way to say it is alive), and its later completion gets `NotFound`. Age is wall-clock time, so
  a clock stepped back delays staleness.
- Adding or deleting a JSON file of roughly 40 MB or more can still pass the 128 MiB cap
  (sem's JSON entities repeat their children's text); parsing from the pipe would remove the cap.

## Decisions
- **No process-group isolation for `run_with_timeout`.** Both callers run in CLI processes. A
  child in its own group stops getting the terminal's Ctrl-C, and a secrets manager that prompts
  on the terminal (`op` unlock) would be stopped by SIGTTIN; the cleanup branch hit the first of
  these with manifest tools (`d033932`). So the child stays in the caller's group, and at the
  deadline only the child is killed, not processes it started (the review's third reproduction
  still shows such a grandchild surviving). The call itself now returns on time either way.
- **No environment scrubbing for `sem`.** The review noted that sem inherits the whole
  environment. sem's output never reaches a model, and it reads its own opt-outs from the
  environment (`SEM_NO_TELEMETRY`, `DO_NOT_TRACK`, `SEM_NO_NETWORK`, `SEM_TELEMETRY`), which an
  allowlist would silently drop, turning telemetry back on. The secrets-manager CLIs need their
  tokens from the environment for the same reason. The one addition is `SEM_LOCAL=1`.
- **The context snapshot is cut, not dropped.** The protocol keeps the field and the tracker keeps
  64 KiB, so a future reader has something to use; a client is not refused for sending more.
- **Test seam without PATH changes.** sem tests run a fake `sem` script through a `#[cfg(test)]`
  thread-local (`with_test_sem`), so parallel tests never change the process environment.
- **Unique `sleep` markers.** On macOS `SystemTime` nanoseconds are whole microseconds, so a
  `subsec_nanos() % 1000` marker is always `.000`; one test's cleanup `pkill` then killed another
  test's process. Tests here use `process_util::test_sleep` (pid plus a counter, anchored pattern).
  The existing orphan tests in `bash_exec.rs` use full `as_nanos()` and are unaffected.

## Not fixed
- **`token_tracker`.** The review's P3 group: an inverted token distance, compactions of unrelated
  sessions paired, a negative duration wrapping to a huge `u64`, predictions for shrinking context,
  an empty platform ranked most efficient, and `estimate_message_tokens` overflowing `u32`. The
  module has no production caller, so these are recorded rather than fixed; fix them before
  anything calls it.
- **No real `sem` run.** The shapes come from sem's README and source (`blame.rs`, `impact.rs`,
  `diff`); no sem binary is installed here, and installing one runs third-party code.
- **Webhook retries.** A redirect or 4xx is retried like a network error (1.5 s of backoff).

## Evidence
- Revert proofs: 14 cases in the first round and 11 in the verification round, each fix reverted
  alone; every case fails its tests (one daemon test path was re-run after correcting its filter).
  The endless-writer test was not run against the uncapped code, since `yes` would fill memory
  until the deadline. The ETXTBSY warm-up and the `{:#}` warnings have no failing test.
- Gate on `e00da7e` (`CARGO_TARGET_DIR` isolated per lane): build clean; `cargo test --workspace`
  3143 passed, 0 failed, 9 ignored; clippy clean with and without default features; fmt clean;
  `python3 docs/validate_docs.py` 188/188.
- Gate after the verification round: build clean; `cargo test --workspace` 3155 passed, 0 failed,
  9 ignored; clippy clean with and without default features; fmt clean; docs 189/189.
- One end-to-end test passed vacuously during the round: BSD `yes` rewrites its argument area, so
  a `pgrep` pattern anchored at the end never matched it. The test helpers anchor at the start
  only, and the test now fails against the reverted fix.
- The five pending branches merged together (before the verification round) built clean and
  passed `cargo test --workspace`: 3361 passed, 0 failed, 8 ignored.

## Handoff
- Open a PR when James approves; trial-merge against `claude/code-cleanup-20261004` and the
  term-context branch first.
- The TUI webhook's `file_path` (the description text instead of the path) is fixed on
  `claude/term-context-fixes-20261005`.
