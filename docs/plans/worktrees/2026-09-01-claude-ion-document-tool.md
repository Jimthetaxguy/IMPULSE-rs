---
title: Ion Document Read Tool
description: Work card for claude-ion-document-tool-20260901 (bounded, pageable document analysis inside Ion's tool loop)
updated: 2026-09-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, ion, document-analysis, tools, primitives]
---

# Ion Document Read Tool

## Lane Facts

- Owner: Claude (Fable 5.1), iteration 2 of goal `impulse-primitives-meta-harness-2026-09`.
- Role: implementation lane for one beyond-software capability.
- Branch: `claude/ion-document-tool-20260901`. Originally stacked on the loop-contract lane
  (PR #39); rebased onto `main` once #39 merged as `5286597`.
- Worktree: `.worktrees/ion-document-tool-20260901` (repository-relative).
- Owned paths:
  - `impulse-rs/src/ion_repl/tool_document.rs` (new)
  - `impulse-rs/src/ion_repl/registry.rs`, `impulse-rs/src/ion_repl/mod.rs` (module line and
    help test), `impulse-rs/src/ion_repl/chat.rs` (gate comment and one test)
  - `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`
  - `CONTEXT.md` (one glossary entry), this work card
  - `impulse-rs/Cargo.toml` and `impulse-rs/Cargo.lock`, limited to the optional `zip`
    dependency (`deflate` feature only) under `office-support` added for the container guard
    (one line each; `zip 0.6` with `deflate` was already in the tree through `docx-rs`)
- Blocked/shared paths: everything owned by the live Codex lanes (`impulse-rs/src/daemon/*`,
  `impulse-rs/impulse-desktop/*`, `.github/workflows/*`, `impulse-rs/scripts/*`); `Cargo.toml`,
  `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, canonical contract; `src/office/*` and
  `src/tooling/document/*` (read, not modified).
- Plan/spec: `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`.
- Verification (isolated `CARGO_TARGET_DIR`): `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 docs/validate_docs.py --all`.
- Latest status: Codex review findings addressed (see review follow-up below) and gated on this
  lane (rebased on `main` at `5286597`): `cargo build --workspace` clean, `cargo test --workspace`
  2310 passed / 0 failed / 9 ignored, strict Clippy clean, rustfmt clean, `--no-default-features`
  build and registry/help tests pass without the tool. `docs/validate_docs.py --all` reports only
  failures that pre-exist on `main`; all lane docs validate.
  [#40](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/40) merged at `7bfcb74` in the same
  minute the review-fix commit was pushed, so the fix did not land with it; it is carried
  unchanged on branch `claude/document-read-hardening-20260902` (worktree
  `.worktrees/document-read-hardening-20260902`) as
  [#41](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/41), the same tree re-gated there.

## Decisions

- 2026-09-01: Build an Ion-native `ReplTool` over `office::parse_document` rather than bridging
  the existing `document_parse` dynamic tool. The dynamic tools return whole documents in one
  payload; a tool loop under ADR-0017 needs an outline plus character-offset paging so one large
  workbook cannot flood the context budget.
- 2026-09-01: Read-only, so ungated like `file_read`. Relative paths resolve against the REPL's
  launch directory; absolute paths are accepted because everyday documents live outside repos.
- 2026-09-01: Workbook text is produced by the tool itself from a streamed cell reader, so sheet
  names and section offsets come from the workbook and header-shaped cell text is just text;
  `csv` and `docx` still go through the `office` parsers with their layouts reconstructed.
- 2026-09-01: `max_chars` above the cap is clamped, not rejected; zero is rejected.
- 2026-09-01 (pre-review follow-up): a 20-agent adversarial pre-review of the diff (four lenses,
  every finding refuted or confirmed independently) drove these decisions before marking the PR
  ready: a 10 MiB source cap; parsing on the blocking pool so the ADR-0017 wall clock can fire;
  sections carry whole-document offsets so Word sections are reachable and the table is in the
  same coordinates as the paged text; windows snap to the last complete line; the rendered table
  is capped at 32 rows and shown only on the first page; the description tells the model to read
  what it needs rather than page exhaustively within the ten-round loop budget; Unicode
  case-insensitive sheet matching; truthful messages for empty worksheets; error messages lead
  with the caller's path so distinct failures never share a same-error signature; registration is
  gated on `office-support` like `src/tooling/document`.

## Changes

- `document_read` tool: `outline`, `sheet`, `offset`, `max_chars`; payload `DocumentWindow`
  with a section table (name, offset, chars), totals, `truncated`, and `next_offset`; rendered
  text carries the section table on the first page and an explicit continuation hint with the
  remaining size.
- Registered in `ReplToolRegistry::with_defaults` under `office-support` (six tools).

## Tests

- Unit: schema, argument parsing and clamping (including a blank `sheet`), serde defaults for a
  bare `{"path"}`, character windows (multi-byte, line-boundary snapping, hard cut for a single
  long line), streamed row and gap rendering, CSV and paragraph layout reconstruction with
  offsets and the mismatch fallback, Unicode and sharp-s sheet selection, truthful
  empty-workbook and non-spreadsheet errors, path resolution errors including legacy `xls`, the
  size cap, inflation measured against hand-built deflated zips (single and cumulative entries),
  serde round trips, bounded and continuation-aware rendering.
- Fixture-backed: CSV read and line-boundary paging loop, outline mode, generated XLSX with a
  header-shaped cell, an empty sheet, sheet selection and a truthful not-found error, a workbook
  with cells at `A1` and `XFD1048576` rendered as two gap markers, a workbook that trips the text
  and cell budgets, generated DOCX paragraphs with offsets, argument, path, corrupt-workbook and
  corrupt-CSV errors, and inflation measured on real documents. These are the first end-to-end
  tests of document reading in the repository.
- Executor: `document_read` never consults the confirmation hook and returns a bounded window.
- Registry and help text assert the tool is present with `office-support` and absent without.

## Review follow-up (Codex review on `7bfcb74`, 2026-09-01)

- P2, decompressed size unbounded for xlsx/docx: the 10 MiB cap only bounded the archive. Three
  rounds of adversarial checks shaped the fix. A declared-size check was refuted because honest
  containers still balloon (cells at `A1` and `XFD1048576` make the dense-grid parser attempt a
  550 GB vector and abort the process; shared-string references materialize one copy per cell)
  and forged headers bypass it. A regex scan of the worksheet XML was refuted because calamine
  accepts spellings the scan did not (cells without `r` positioned sequentially, single quotes,
  lowercase columns, namespaced tags, case-insensitive part names). The final design stops
  guessing at XML: every container entry is inflated once through a 64 MiB cap
  (`preflight_container`, seam `preflight_container_with_limit`), and workbooks are streamed cell
  by cell through calamine's own cell reader into text the tool writes itself
  (`extract_workbook`, `SheetBodyBuilder`) under a 16 million character and 2 million cell budget,
  so the dense parser is never invoked and sheet names and offsets come from the workbook rather
  than from re-parsing text. Legacy `.xls` is refused because it has no streaming reader.
  Fixtures pin each bound: inflation measured against deflated zips, a real corner-cell workbook
  that now renders as two gap markers, and a workbook that trips the text and cell budgets.
  Requires the optional `zip` dependency (with `deflate`) noted under owned paths.
- P2, blank `sheet` silently meant "whole workbook": a supplied blank selector is now rejected
  with a typed argument error; absent or `null` still means the whole document.
- P2, `to_lowercase` is not Unicode case folding: a first fix (upper-then-lower casing) was
  refuted for regressing capital `ẞ` and conflating dotless `ı` with `i`. Matching now uses
  Unicode lowercasing plus the Latin multi-character folds (`ß`/`ẞ` to `ss`, long `ſ`, the `ﬀ`
  ligature family), so `Straße`, `STRAẞE`, and `STRASSE` match while Turkish names stay distinct;
  no normalization, and the spec says so.

## Stage 0 continuation (2026-09-02): docx streaming and chart-sheet skip

Continued in worktree `.worktrees/document-read-hardening-20260902` on local branch
`claude/document-read-hardening-20260902`, stacked on `main` at `36bda00` (PR #41), and pushed
to the **remote** branch `claude/document-read-hardening-stage0-20260902`. The remote name
differs because PR #41 was squash-merged from `claude/document-read-hardening-20260902`, whose
remote head still carries the pre-squash commits; publishing there would have required a
force-push, so this stage took a fresh remote name instead and nothing was rewritten.
Owned paths for this stage add `impulse-rs/Cargo.toml` and
`Cargo.lock`, limited to the optional `quick-xml 0.31` dependency under `office-support`
(`calamine` already pins that exact version, so the lock gains one line and no new crate).

- **Word extraction no longer builds the docx object tree.** A fourth adversarial pass found the
  last unbounded step: `docx-rs` materializes a document many times the size of the XML, so a
  file well under every earlier cap that inflates to 64 MiB of empty paragraphs could still
  exhaust memory. `word/document.xml` is now streamed event by event through `quick-xml`
  (`extract_word`), with the accumulator split out as `WordTextBuilder` so its boundaries are
  testable without a document.
- **Chart and dialog sheets no longer fail the workbook.** `worksheet_cells_reader` returns
  `XlsxError::NotAWorksheet` for them; they are now skipped, keeping their workbook position, the
  way calamine's own range reader does.
- **Defects found in the paused WIP and fixed here.** (1) The character budget was only checked
  when a paragraph was flushed, so one paragraph of millions of runs could grow to the whole
  64 MiB inflate cap before any check ran; every buffer in flight is now counted as it grows.
  (2) The outline grew one section per ten paragraphs with no ceiling, and the section table
  travels in the payload — capped at 4096, with the last section absorbing the tail so reported
  offsets stay truthful. (3) Table cells each became their own line, losing row structure; a row
  is now one tab-separated line, matching the workbook rendering, with cell paragraphs joined by
  spaces and in-cell tabs and breaks rendered as spaces so a cell cannot forge a column break.
  (4) Deletions, tracked moves, field instruction codes, and `mc:Fallback` were only excluded
  incidentally (by matching `w:t` alone); a `w:t` inside `w:del` would have leaked, and
  `w:moveFrom` duplicated its `w:moveTo`. Those subtrees are now skipped explicitly.
  (5) `word/document.xml` was read with no size bound of its own when `preflight_container` had
  not run; it is now read through the 64 MiB container cap regardless.
- **Deliberately unchanged.** Headers, footers, footnotes, endnotes, and comments stay out of
  scope (sibling parts, not `word/document.xml`); nested tables flatten into the containing row;
  `from_extraction`'s paragraph arm stays because the adapter is written against any
  `ExtractionResult`, not against one caller. `impulse-ion/TUI_SPEC.md` needed no change: it does
  not describe `document_read`.
- **Fixtures** are built in a temp directory like the existing ones (the convention this module
  already uses) rather than committed as binaries, so nothing untracked or opaque enters the tree.
  The chart-sheet workbook and every docx are hand-written XML because neither the workbook writer
  nor the docx builder in the tree can emit a chart sheet, a revision mark, a field code, or
  malformed XML. The spec's Testing section carries the fixture table.

Gate at the first push of this stage (isolated `CARGO_TARGET_DIR`); superseded by the review
round 1 gate below:

- `cargo build --workspace`: clean.
- `cargo test --workspace`: **2324 passed, 0 failed, 9 ignored** across 30 test binaries —
  impulse-desktop 248/0/1, impulse-ion 23/0/1, impulse-ops 85/0/0, impulse-rs 1841/0/7,
  impulse-step-model 12/0/0, impulse-term 115/0/0. `impulse-gui` is excluded from the workspace.
  `tool_document` itself: 50 passed (36 before this stage).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- `cargo build --no-default-features` and
  `cargo test --no-default-features --lib -- ion_repl::registry`: 4 passed, tool still absent
  without `office-support`.
- `python3 docs/validate_docs.py --all`: the four failures that pre-exist on `main` only
  (ADR-0014's `proposed` status and three March documents past the staleness threshold).

## Review round 1 (adversarial review of PR #49 at `fc61cf1`, 2026-09-02)

The review measured the claims rather than reading them: 64 MiB of `<w:p/>` extracted in 296 ms
and 6.5 MB RSS, billion-laughs and DOCTYPE internal subsets were refused, and the skip-subtree,
namespace, outline-cap, and chart-sheet claims all held. Two P2s contradicted what the PR body and
spec said, and both are now closed.

- **P2, cell text could still forge a column or a row break.** `w:tab` and `w:br` elements were
  neutralized inside a table, but a literal U+0009/U+000A, a numeric reference (`&#9;`, `&#10;`),
  or a CDATA section passed straight through, so a two-cell row could render as three columns or
  two lines; outside a table, `x&#10;y` became two lines. Fixed at the point document text enters
  (`push_word_text`): `\n` and `\r` always become spaces, and `\t` as well inside a row, one
  character for one so counts stay exact. `WordTextBuilder::push_line` repeats the newline
  normalization as a backstop. Because the mapping is now absolute — one output line is exactly
  one paragraph or one table row — `w:br`/`w:cr` became spaces everywhere rather than newlines
  outside a table, which is a deliberate behavior change from the first push: it is what makes
  `window`'s line snapping and the section spans exact instead of approximately right.
- **P2, a nested table silently erased the containing cell's own text.** `Start tc` cleared the
  cell unconditionally, so `<w:tc><w:p>OUTER</w:p><w:tbl>…INNER…</w:tbl></w:tc><w:tc>RIGHT</w:tc>`
  rendered `INNER\t\tRIGHT` — OUTER gone, and a column that did not exist. A `cell_depth` counter
  now clears and flushes only at the outermost `w:tc`, so an inner table merges into the text of
  the cell holding it and the row keeps its real column count. An unmatched `</w:tc>` closes
  nothing.
- **P2, the "peak memory is the budget plus a single XML event" claim was false.** The text was
  pushed into the paragraph buffer *before* `check_pending` ran, so a 50 MiB single `w:t` peaked
  at 114 MB even with `max_chars=1000`; separately quick-xml's open-element stack scales with
  nesting depth (9.6 M levels measured at 126 MB). Both stay bounded by the 64 MiB part cap
  (worst measured ≈145 MB). The budget is now checked *before* every push, so no long-lived
  buffer overshoots, and the module doc and spec now say the real bound: roughly twice the part
  cap, from quick-xml's event buffer plus its open-element stack — the part cap is what does the
  work, not the character budget.
- **Fail-closed part read.** The 64 MiB `take` failed *open*: a 66 MiB part parsed whatever prefix
  fit and returned `Ok`. It now takes one byte past the cap and refuses with a typed too-large
  error when that headroom is consumed. A read error is held until after that check, so a part cut
  off at the cap is reported as too large — which it is — rather than as malformed.
- **Case-insensitive part lookup.** OPC part names compare case-insensitively and calamine
  resolves workbook parts that way, so `word/document.xml` is now found however it is cased. Where
  a container declares the part twice, the last entry wins (noted in the spec, not fixed).
- **CDATA UTF-8.** `from_utf8_lossy` silently produced replacement characters; invalid UTF-8 in a
  CDATA section is now a typed error, matching how `Event::Text` already behaved.
- **Message and spec wording.** Skipped chart, dialog, and macro sheets are described as
  non-worksheet parts rather than folded into "empty worksheets" in the sheet-not-found and
  empty-workbook errors, the rendered note, and the schema. The spec now names `w:fldSimple`
  (cached result kept, `w:instr` attribute never read) and records that DrawingML `<a:t>` and OMML
  `<m:t>` text is extracted by local-name matching, because a reader sees it too.
- **Left with a note, as the review allowed.** A duplicate `word/document.xml` entry resolves to
  the last one; a self-closing `<w:tc/>`, which the schema does not permit, drops that column.
  Both are recorded in the spec.

Eight regression tests were added for these: control characters and CDATA smuggled into cells,
breaks inside a plain paragraph, the nested-table case, `w:fldSimple`, a case-varied part name,
non-UTF-8 CDATA, the oversized part read directly without the preflight, and two unit tests for
the normalization itself.

Gate after round 1 (isolated `CARGO_TARGET_DIR`, current checkout):

- `cargo build --workspace`: clean.
- `cargo test --workspace`: **2332 passed, 0 failed, 9 ignored** across 30 test binaries —
  impulse-desktop 248/0/1, impulse-ion 23/0/1, impulse-ops 85/0/0, impulse-rs 1849/0/7,
  impulse-step-model 12/0/0, impulse-term 115/0/0. `tool_document` itself: 58 tests (50 at the
  first push, 36 before this stage).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- `cargo build --no-default-features` and
  `cargo test --no-default-features --lib -- ion_repl::registry`: 4 passed, tool still absent.
- `python3 docs/validate_docs.py --all`: only the four failures that pre-exist on `main`.

## Handoff Notes

- The first `cargo test --lib -- ion_repl` run on this lane failed eleven tests at temp-directory
  creation, including pre-existing `tool_verify` tests; two immediate reruns and the full gate
  passed. Treated as a transient host temp-dir hiccup, recorded here in case it recurs.
- `src/tooling/builtin/document_extract.rs` still returns "Python extraction not yet wired" for
  its default path; it was left untouched and is a candidate for a later iteration.
