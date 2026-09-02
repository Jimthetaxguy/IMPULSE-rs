---
title: Ion Document Read Tool Design
description: Design spec for document_read, a bounded and pageable document-analysis tool inside Ion's tool loop
updated: 2026-09-01
type: specification
category: architecture
phase: all
status: active
audience: builders
tags: [spec, ion, document-analysis, tools]
---

# Ion Document Read Tool Design

> Iteration 2 of goal `impulse-primitives-meta-harness-2026-09`. Written in autonomous mode; the
> assumptions below stand in for the questions a live brainstorming session would have asked.

## Goal

Let Ion analyze spreadsheets and Word documents conversationally, so the native runtime is
useful for everyday work (an invoice, a budget workbook, a lease letter) and not only for code.
Because those documents come from third parties, the tool must never let a hostile file take
the `ion` process down: every input is bounded before the parser sees it.

## What already existed

- `src/office` parses `xlsx`, `xls`, `csv`, and `docx` into an `ExtractionResult` (full text
  plus typed chunks). It had no end-to-end tests with real files.
- `src/tooling/document` wraps those parsers as dynamic tools for the CLI and daemon. They return
  the entire document in one payload and were not registered in Ion's REPL tool set.

## Assumptions

- Tool results feed a model inside a loop contract (ADR-0017), so bounded output with explicit
  continuation is worth more than completeness in one call. This follows current guidance on
  writing tools for agents: return high-signal, paginated responses with a clear next step.
- The loop contract allows ten rounds per turn, so the tool cannot promise exhaustive paging of a
  large document. The outline therefore carries section offsets, the description tells the model
  to read only the windows it needs, and the continuation hint says how much remains.
- Read-only document access does not need a confirmation gate; it matches `file_read`.
- Everyday documents live outside repositories, so absolute paths must work.
- The tool is only registered with the default `office-support` feature, matching the
  `src/tooling/document` convention, so a build without the parsers never advertises it.

## Approaches considered

1. **Bridge `document_parse` through `DynamicToolBridge`.** One line, but unbounded payloads and
   no sheet selection or paging. Rejected.
2. **Add paging parameters to the dynamic tools.** Improves the CLI too, but couples the loop's
   needs to a registry shared with daemon and manifest tools. Deferred.
3. **Ion-native `ReplTool` over `office::parse_document`.** Chosen. Small, testable, and shaped
   for the tool loop; the dynamic tools stay unchanged.

## Interface

`document_read {"path", "sheet"?, "outline"?, "offset"?, "max_chars"?}`

| Field | Meaning |
|---|---|
| `path` | Required. Relative paths resolve against the REPL's launch directory. Formats: `xlsx`, `csv`, `docx`. Legacy `xls` is refused because its binary format has no streaming reader and cannot be bounded. Files over 10 MiB are refused. An `xlsx`/`docx` container is inflated once, entry by entry, through a 64 MiB cap before parsing, so a forged central directory cannot hide a decompression bomb. Workbooks never reach the dense-grid parser: cells are streamed one at a time through calamine's cell reader into the tool's own text under a 16 million character and 2 million cell budget, so two cells at opposite corners of a sheet cost two gap markers and a shared string costs only the cells that render it. `csv` and `docx` text is checked against the character budget after parsing, where the parsers' memory is already bounded by the file and inflation caps. These bound the parser's inputs; they are not an OS sandbox. |
| `sheet` | Worksheet name, case-insensitive: Unicode lowercasing plus the Latin multi-character folds, so `Straße`, `STRAẞE`, and `STRASSE` match; dotless `ı` stays distinct from `i`; no normalization. Spreadsheets only; empty worksheets are omitted by the parser. A supplied blank value is rejected rather than treated as "whole document". When set, `offset` is relative to that sheet's text. |
| `outline` | Section table and sizes only, no content. |
| `offset` | Character offset to start from: a section's offset from the outline, or the offset named by the previous continuation hint. |
| `max_chars` | Characters to return. Default 12000, capped at 32000, zero rejected. Windows end on a line boundary when one exists inside the window. |

Payload (`DocumentWindow`): path, format, document type, size, section table (index, kind,
optional sheet name, whole-document character offset, character span), selected section,
`total_chars`, `offset`, `returned_chars`, `truncated`, `next_offset`, and `content` (absent in
outline mode). Section offsets and spans are in the same coordinates as the paged text, so a Word
section is reachable by offset even though it has no name. Workbook text is written by the tool
itself (`=== Sheet: name ===`, the streamed body, a blank line), so sheet names and offsets come
from the workbook, never from re-parsing text, and a cell that happens to contain header-shaped
text is just text. Streamed rows are tab-separated; up to eight empty columns render as tabs and
wider gaps or skipped rows render as bracketed markers such as `[16383 empty columns]`.

The rendered text carries the header, then on the first page or in outline mode the section table
(at most 32 rows, with an elision count) and, for workbooks, a note that empty worksheets are
omitted. The window ends with either "complete" or "truncated, N chars remain (about K more
calls at this size); continue with offset=N, or raise max_chars". A window that had to cut inside
a single long line says so.

Parsing runs on the blocking pool so the loop contract's wall clock can still fire while a large
file parses.

## Error handling

Missing or malformed arguments (including a blank `sheet`), unsupported extensions (the message
lists supported ones), legacy `xls`, missing files, directories, files over the size cap,
containers over the inflation cap, workbooks over the cell or character budget, extracted text
over the character cap, unparseable files (the message carries the parser's reason), unknown
sheets (the message lists available non-empty sheets), sheet selection
on a workbook whose sheets are all empty, and sheet selection on non-spreadsheets all return
typed errors. Each message leads with the path or name the model supplied, so two different bad
calls never share an error signature while a repeated identical bad call still trips the loop
contract's same-error detector.

## Testing

Pure helpers are unit-tested directly, including the row and gap rendering of the streamed body,
Word and CSV layout reconstruction, Unicode sheet names, line-boundary snapping, the size cap,
inflation measured against hand-built deflated zips (a 200 KiB entry that is a few hundred bytes
on disk, and cumulative entries), and the bounded section table. Fixture tests generate CSV, XLSX
(via the workbook writer already in the dependency tree; with a header-shaped cell and an empty
sheet), a workbook with cells at `A1` and `XFD1048576` that the dense parser could not survive, a
workbook that exceeds the text and cell budgets, and DOCX (via the docx builder) in a temp
directory and drive the tool end to end, including a corrupt workbook. One executor test proves
the tool is ungated and bounded. The registry and help tests assert the tool is absent without
`office-support`.

## Out of scope

PDF and plain-text formats, the stubbed `document_extract` tool, a `/doc` slash command,
per-session path sandboxing, and caching parsed documents across calls.
