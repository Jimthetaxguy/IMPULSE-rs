---
title: Bound the office document tools
description: Work card for office-bounds-20261005
updated: 2026-10-05
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, office, documents, bounds]
---

# Bound the office document tools

## Lane Facts
- Owner: claude (Opus 5.5, session with James), from the open item "Office tools are unbounded"
  recorded on `claude/code-cleanup-20261004`'s card
- Role: implementer
- Branch: `claude/office-bounds-20261005`, from `origin/main` at `7481457`
- Worktree: `.worktrees/office-bounds-20261005`
- Owned paths: `impulse-rs/src/office/**`, `impulse-rs/src/tooling/document/**`, the office
  context provider in `impulse-rs/src/plugin/registry.rs`, `impulse-rs/src/handlers/office.rs`,
  this card
- Shared paths edited: `impulse-rs/src/ion_repl/tool_document.rs` (the reader moved out of it; the
  only pending edit to this file, on `claude/ion-blackboard-20261003`, is one line in a test that
  stays where it is), one assertion in `impulse-rs/src/daemon/tests.rs`, and the file column of
  `docs/superpowers/specs/2026-09-12-governed-parser-property-tests.md`. `git merge-tree` trial
  merges with every active branch are clean; the four September branches that conflict already
  conflict with `main`.
- Blocked paths: `CONTEXT.md` (uncommitted edits in the main checkout), `Cargo.toml`, `Cargo.lock`
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented in `b83f099`, `2130b2c`, `26bee37` and `2ee4a05`; a refutation
  round and a verification round, each followed by fixes, then a final check with no new
  findings. Final gate clean. Pushed; not merged; needs a PR and review.

## Problem
The office entry points parsed with no bounds: the `office` CLI (`parse`, `sheets`, `chunk`,
`extract-smart`), the `excel_read`, `word_read` and `document_parse` tools (reachable over MCP
and the daemon's `InvokeTool`), and the office context provider. Calamine's dense
`worksheet_range` turned two cells at opposite corners of a sheet into a grid of about 17 billion
cells; `.docx` went through docx-rs's object tree; neither format had a file-size or inflation
cap; legacy `.xls` was opened with calamine's `.xls` reader, which builds every sheet's grid as it
opens the file. The tools also parsed on the async runtime thread. Ion's `document_read` already
had all of these bounds.

## What changed (`b83f099`)
- `office::bounded` (new) holds Ion's bounded reader, moved from `ion_repl::tool_document`: the
  file cap (10 MiB), the zip-inflation preflight (64 MiB), the cell-streaming workbook extractor
  with gap markers, the quick-xml Word streamer, their builders, and their unit and property
  tests. Error messages lead with a label the caller supplies. New in the module:
  `check_extension` (refuses `.xls` with the reason), `check_source_file` (regular file, size
  cap), `extract_csv` (UTF-8, read stops one byte past the cap), and `read_document`, which
  applies them in that order before the streaming extractor.
- Ion keeps thin wrappers that pass `document_read: '<path>'`, so its messages and payloads are
  unchanged; 116 tests before the move = 99 still in Ion + 17 moved with the builders.
- `office::parse_document`, `excel::parse_excel` and `word::parse_word` read through
  `read_document` and convert to the old `ExtractionResult`; each chunk is now the exact span of
  `content` its section covers. A Word table reads as tab-separated rows instead of `[Table]`.
  `excel::get_sheet_info` counts extents from streamed cells under the same caps.
- `.xls` is refused everywhere, and the office provider no longer registers for it (it now
  registers from its own `formats()` list, so the two cannot drift apart).
- The three tools parse in `spawn_blocking`.
- `excel::read_range` and `word::get_word_stats` are removed: no callers anywhere in the
  workspace, and both built the unbounded structures this lane removes.

## Follow-ups
- `2130b2c`: Ion's `document_read` ran its container preflight (up to 64 MiB of inflation) on the
  async runtime thread before handing the parse to the blocking pool, contrary to its module doc.
  The preflight now runs in the same `spawn_blocking` closure as the parse.
- `26bee37`, from the refutation round:
  - P2: with overflow checks on (debug and test builds), calamine panics on some malformed
    workbooks, such as an inverted `<dimension ref="B2:A1">` or a cell reference whose row or
    column overflows `u32`. The tools and Ion contain that through `spawn_blocking`, but the
    office CLI and context provider call the reader synchronously. `bounded::contain_panics` now
    wraps `read_document` and `get_sheet_info`. This predates the branch: the old dense reader
    reached the same calamine code.
  - P3: a sheet's header and closing blank line were not counted against the character budget,
    so a long sheet name could carry the text past it. They count with the body now, for Ion too.
- `2ee4a05`, from the verification round:
  - P2, older than this branch: reading a workbook took time that grew with the square of its
    sheet count, because calamine finds each sheet by scanning every sheet name and every entry
    name. A 155 KB file listing 64K cell-less sheets took 12.6 s in a debug build, and the
    inflation cap alone allows about two million of them. A workbook may now list at most
    `MAX_SHEETS` (4,096) sheets, and a container may hold at most `MAX_CONTAINER_ENTRIES`
    (16,384) entries. Ion shares both caps.
  - P3, a regression in `26bee37`: a sheet's header was charged while its body was still empty,
    so a sheet whose only cell renders as an empty string, which is skipped, made a fitting
    document fail. The header now counts once the sheet has text.
  - P3: calamine's out-of-range shared-string panic happens in every build, since bounds checks
    stay on in release; the doc and the panic test cover it.

## Evidence
- Red phase, against the old office code: 13 of the new tests failed for the intended reason
  (dense tabs for two far cells, `[Table]`, no inflation check, a 10 MiB + 1 byte CSV read whole,
  no `.xls` refusal, chunks joined with blank lines, and the tools' turn counter at zero because
  they parsed inline); two preservation tests passed (`get_sheet_info` extents, sheet and CSV
  chunks). Handing the old `parse_word` an `.xlsx` left docx-rs spinning at full CPU until the
  test process was stopped; `parse_word` now refuses any extension but `.docx`, and docx-rs no
  longer reads documents.
- Revert proofs on the new code, each failing its tests: `read_document`'s size check and
  preflight, `sheet_info`'s preflight and cell cap, the chunk-span cursor, the `.xls`
  explanation, and `extract_csv`'s own cap.
- Gate on `b83f099`: build clean; `cargo test --workspace` 3137 passed, 0 failed, 9 ignored
  (impulse-rs unit tests 2411 passed, 5 ignored; `pdf_extraction_isolation` 17 passed); clippy
  clean with default features and with `--no-default-features`; fmt clean.
- Refutation round: one reviewer with its own export and build cache confirmed one P2 and three
  P3s, each with a reproduction. Fixed: the P2 and the budget P3 (`26bee37`), and the commit
  message wording. Not a gap: its clippy point, since the gate runs clippy with default features
  too. Areas it checked and found sound: Ion's CSV path through the new office code, chunk
  slicing, sheet extents, the order of checks, FIFOs, directories and symlinks, the Word
  streamer, `spawn_blocking` error mapping, and stale references.
- Revert proofs for `2130b2c` and `26bee37`: the preflight placement (a ticker task gets no turn
  before `run` fails), panic containment, the sheet-listing wrap, and the header budget each
  fail their tests when reverted.
- Verification round on `26bee37`: ten malformed workbooks through `parse_document`,
  `parse_excel` and `get_sheet_info` all return "the parser panicked" errors; no calamine call
  remains outside `contain_panics` or `spawn_blocking`; text never exceeds the budget at any
  budget from 0 up; Ion's error chains are byte-identical after the preflight move. It found the
  three items fixed in `2ee4a05`.
- Revert proofs for `2ee4a05`: both sheet caps, the entry cap, the empty-body header check, and
  the shared-string containment each fail their tests when reverted.
- Gate on `26bee37`: build clean; `cargo test --workspace` 3141 passed, 0 failed,
  9 ignored (impulse-rs unit tests 2415 passed, 5 ignored; `pdf_extraction_isolation` 17
  passed); clippy clean with default features and with `--no-default-features`; fmt clean;
  `python3 docs/validate_docs.py` 189 of 189 valid, this card included.
- Final check on `2ee4a05`: no new findings. In a debug build the sheet cap reads a real
  4,096-sheet workbook (4,104 entries, 1.85 MB) in about 0.95 s and refuses 4,097 sheets in about
  0.2 s; the empty-string sheet reads at a budget of exactly 20; every panic is still contained.
  The worst case the caps allow, 4,096 sheets with 16,384 entries whose names are built to make
  every comparison long, took 0.63 s for `parse_document` and for `get_sheet_info` (measured
  separately, debug build).
- Final gate on `2ee4a05`: build clean; `cargo test --workspace` 3145 passed, 0 failed,
  9 ignored (impulse-rs unit tests 2419 passed, 5 ignored; `pdf_extraction_isolation` 17
  passed); clippy clean with default features and with `--no-default-features`; fmt clean.

## Recorded, not fixed
- docx-rs now only builds test fixtures. Moving it to dev-dependencies, or replacing the fixtures
  with hand-written XML, needs a `Cargo.toml` change (blocked here).
- `CONTEXT.md` describes Ion's document bounds; it could say the office entry points share them
  now (blocked here).
- `check_source_file` checks by path before the file is opened, as Ion's path check does: a
  same-user process that swaps a FIFO in between the check and the open can still block a reader.
  A race-free check opens without blocking and inspects the handle.
- A cancelled tool call's blocking parse runs to completion; the caps bound how long that takes.
- The office tools still read any path their caller names; they have no sandbox of their own.
- The preflight inflates with the project's `zip` 0.6.6, while calamine reads with its own `zip`
  2.4.2. If the two ever disagreed about an archive's entries, calamine could inflate an entry
  the preflight never measured, notably the shared strings it loads whole. The reviewer could not
  construct such an archive; aligning the versions needs a `Cargo.toml` change.
