---
title: Ion Document Read Tool
description: Work card for claude-ion-document-tool-20260901 (bounded, pageable document analysis inside Ion's tool loop)
updated: 2026-09-01
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
  failures that pre-exist on `main`; all lane docs validate. PR
  [#40](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/40) is ready for review.

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

## Handoff Notes

- The first `cargo test --lib -- ion_repl` run on this lane failed eleven tests at temp-directory
  creation, including pre-existing `tool_verify` tests; two immediate reruns and the full gate
  passed. Treated as a transient host temp-dir hiccup, recorded here in case it recurs.
- `src/tooling/builtin/document_extract.rs` still returns "Python extraction not yet wired" for
  its default path; it was left untouched and is a candidate for a later iteration.
