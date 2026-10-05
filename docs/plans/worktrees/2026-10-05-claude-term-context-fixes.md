---
title: Terminal core and context lifecycle fixes
description: Work card for term-context-fixes-20261005
updated: 2026-10-05
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, terminal, context, review]
---

# Terminal core and context lifecycle fixes

## Lane Facts
- Owner: claude (Opus 5.5, unattended overnight under James's standing goal "continue cleaning
  up and reviewing the code")
- Role: implementer, from a read-only review of the two least-reviewed areas that process
  untrusted terminal text: `src/context_lifecycle/` and `impulse-term/src/`
- Branch: `claude/term-context-fixes-20261005`, from `origin/main` at `7481457`
- Worktree: `.worktrees/term-context-20261005`
- Owned paths: `impulse-term/src/{backend,paste,context}.rs`,
  `impulse-term/tests/backend_resilience.rs` (new), `src/context_lifecycle/{templates,detector,
  types,extractor,parser,intent}.rs`, `src/ui/{lifecycle,terminal_pane,types}.rs`, this card
- Shared paths: `impulse-term/src/backend.rs` is also edited by `claude/code-cleanup-20261004`, in
  other regions (the write path); the new tests are in their own file so the two branches do not
  both append to `backend_tests.rs`.
- Blocked paths: `impulse-desktop/src/{ui.rs,runtime.rs}` (pending branches and uncommitted work),
  `Cargo.toml`, `Cargo.lock`, `CONTEXT.md`
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented in `8a4c4b7`, `cfe8645`, `e9aff9f`, `7db19bf`, `a14eeb6` (the
  verification round) and `660a763` (the final round); pushed. Not merged; needs a PR and
  review.

## Findings fixed
The review reported 16 findings, each with a reproduction (2 P1, 7 P2, 7 P3). Fixed here:

- P1, terminal reader: a resize that cut a wide character made vt100 0.15 panic on the next
  redraw, which killed the PTY reader thread. Output stopped, the exit callback never ran, and
  `kill()` waited forever, because a killed child cannot finish exiting while nothing drains its
  terminal. The reader now catches a vt100 panic (fresh screen at the same size) and a panicking
  output callback, and always marks the terminal dead and runs the exit callback when it ends.
  `kill()` reaps with a 5 s deadline (`kill_within` is the test seam). Terminals are at least 3x3,
  since vt100 0.15 also panics on ordinary escapes below that.
- P1, injected context: text printed in one pane was typed into another agent's input
  unescaped, so it could close the `<impulse-context>` block around it and pose as instructions.
  Relayed text now has `<` and `>` escaped, control characters turned into spaces, a 200
  character cap, and a line marking it as quoted data.
- P2, TUI history reads: pages read out to 200 lines, but vt100 0.15 can scroll back only one
  screen; past that its offset arithmetic underflows (a panic in debug builds, pages counted twice
  in release, so 50 added lines read as 78). The tick now reads the rows directly above the
  screen, with two screen clones per pane instead of up to nine.
- P2, intent store: every tick fed every stored insight again, so it grew without bound. Each
  insight is fed once, and the store keeps at most 100 intents per agent.
- P2, conflicts: every tick announced every file conflict again (feed entry, notification,
  webhook), which also undid the operator's resolution. A conflict is announced once, and a
  resolution holds while the conflict is still reported.
- P2, compaction: one compaction message left on screen counted again on every scan, and Claude
  Code's countdown line ("Context left until auto-compact") counted as a compaction. A
  compaction now counts once per new line, the countdown is ignored, and the post-compaction
  injection waits for the per-pane injection cooldown like threshold refreshes.
- P2, paste: pasted text could end a bracketed paste with its own `ESC[201~`, after which the
  rest reached the program as typed input. Every ESC (and the one-character CSI) is removed.
- P3: `scrollback_len` returned the current offset rather than the history (now the history, up
  to one screen); `scrollback_text` with an offset past one screen panicked (clamped); Codex and
  OpenCode paths came back lowercased; a passing `test result: ok. ... 0 failed` counted as an
  error in both extractors; the oldest insights of the first pane were relayed instead of the
  newest from all panes; and the extractor took time quadratic in the line count.

## Verification round (`a14eeb6`)
The same reviewer reran its reproductions against the fixes and probed them. Fixed:
- The egui panel can now scroll back; its scroll badge then shrank the terminal by a row, the
  stored offset passed the new height, and vt100 0.15 underflowed on every later frame. The
  renderer clamps the offset to the current height.
- `kill_within` held the child lock while it waited, so `is_alive()` could stall for the whole
  deadline (2.8 s in the review). It now takes the lock per poll.
- Conflict announcements were de-duplicated against the 20-entry display list: past 20 conflicts
  the dropped ones were announced every tick, and a new pair of panes on the same file was never
  announced. Announcements now have their own set, keyed by kind, description and panes.
The final round (`660a763`) found one more: the history above the screen was read one row at a
time while the screen was read with wrapped rows joined, so a long line that wrapped in the
history reached the parser in pieces (a `Write(` with a 90-character path, 30 lines up, gave no
insight). Wrapped history rows are now joined. Nothing else was confirmed; the tick took 36.7 ms
per pane with 10k lines of scrollback, down from 267 ms.

Held up under the round: a fuzz run at small sizes with resizes (six parser panics recovered, no
other panics, the final output rendered, `kill()` returned, one exit callback), the quoting, and
the one-screen reads, which no longer panic or count pages twice.

## Conflict path in notifications (later review)
A review of `src/notification/` (lane `claude/subprocess-delegation-fixes-20261005`) found that
the conflict notification and webhook carried the recommendation's description ("Multiple agents
modifying: src/main.rs") as their `file_path`. This branch owns the tick, so the fix is here:
`Recommendation::conflict_file` reads the path back next to where the coordinator formats it
(`FILE_CONFLICT_PREFIX`), and the tick and the resolution handler use it. A tick test with two
panes on one file and a loopback webhook checks the bus event and the webhook body; with the old
line the event carried the description.

## Recorded, not fixed
- History reads reach one screen (the vt100 0.15 limit), so an error, compaction or `Write(` that
  scrolls further between extraction passes (every 30 s) is missed; before this branch debug
  builds panicked there and release builds counted overlapping pages twice. Deeper reach needs a
  newer vt100 or a line log fed from the output stream.
- A second compaction whose message is identical to one still on screen is not counted.
- The desktop side of the P1: `close_agent` holds the `lifecycle_events` lock across `kill()`,
  and the launch-failure paths kill before opening the launch gate. With the reader fix and the
  deadline the worst case is a 5 s stall instead of a hang; reordering belongs on top of
  `claude/code-cleanup-20261004`, which rewrites `runtime.rs`.
- The TUI auto-injects context by default (`context_lifecycle_enabled: true`), while principle 6
  says injection defaults to review mode. A decision for James.
- The token estimate counts raw output bytes, so spinner redraws inflate it (280 KB of redraws
  showing two lines estimated at 175k tokens). Choosing the measure is a design decision.
- The desktop decodes each 1024-byte chunk on its own (`TextDecoder.decode()` without
  `{stream: true}` in `impulse-desktop/src/ui.rs`), garbling multibyte text at chunk boundaries.
  `ui.rs` is blocked here.
- The legacy egui `ContextBridge` estimate counts visible characters only (egui is frozen).
- vt100 0.15 limits history reads to one screen and panics on some screen states. The latest is
  0.16.2. Its changelog fixes scrollback offsets in `Grid::visible_rows` (0.16.0, #11), likely
  the underflow behind the one-screen limit, and a cursor out of bounds after a resize (0.16.2);
  the wide-character panic is not mentioned. The upgrade moves `set_size` and `set_scrollback`
  from `Parser` to `Screen` (through `screen_mut()`), removes the title and bell accessors (unused
  here), and makes `Cell::contents` return `&str`: about 20 call sites in `impulse-term` and
  `src/ui`. It needs `Cargo.toml` and `Cargo.lock`, which other branches also edit, so it belongs
  in a dependency lane.
- The TUI writes each injection with a blocking `write_input` on its own thread. A pane that is
  not reading its input stalls the whole tick once the message overfills the tty's input queue:
  a test pane running `sleep 30` held one tick for 30 s. Writing through the per-pane write queue,
  or off the UI thread, would remove that.
- Unconfirmed: real Claude Code tool lines may not match the parser's `Write(`/`Edit(` patterns,
  and the injection's trailing `\n` may not submit in raw-mode TUIs.

## Evidence
- Red: each fix was reverted on its own and its tests run, 25 reverts in all; 24 failed as
  intended. The compaction-cooldown test at first passed reverted (its pane never read input, so
  the reverted injection blocked and failed); its pane now drains input and the test fails
  reverted. Two round-2 reverts first missed the original bug and were redone faithfully. The
  `kill_within` lock test did not fail reverted here, because the killed child was reaped within
  240 ms (portable-pty sends SIGHUP, waits about 200 ms, then SIGKILL); it guards the stall the
  review measured without reproducing it in this environment.
- Gate on `198ff26` (the four review commits, before a one-line move for a clean merge with
  `claude/code-cleanup-20261004`): build clean; `cargo test --workspace` 3131 passed, 0 failed,
  9 ignored (impulse-rs unit tests 2397, `backend_resilience` 5, `backend_tests` 19); clippy clean
  with default features and with `--no-default-features`; fmt clean.
- Gate on `a14eeb6`: build clean; `cargo test --workspace` 3134 passed, 0 failed, 9 ignored
  (impulse-rs unit tests 2398 passed and 5 ignored, `backend_resilience` 6, `backend_tests` 19,
  `panel_scrollback` 1); clippy clean with default features and with `--no-default-features`;
  fmt clean.
- Final gate on `660a763`: build clean; `cargo test --workspace` 3135 passed, 0 failed, 9 ignored
  (impulse-rs unit tests 2399 passed and 5 ignored, `backend_resilience` 6, `backend_tests` 19,
  `panel_scrollback` 1); clippy clean with default features and with `--no-default-features`;
  fmt clean.
- Trial merges: clean with every active branch, including `claude/code-cleanup-20261004` after
  the clamp line moved above the line that branch also inserts after.
