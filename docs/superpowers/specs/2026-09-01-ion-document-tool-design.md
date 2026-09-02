---
title: Ion Document Read Tool Design
description: Design spec for document_read, a bounded and pageable document-analysis tool inside Ion's tool loop
updated: 2026-09-02
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
| `path` | Required. Relative paths resolve against the REPL's launch directory. Formats: `xlsx`, `csv`, `docx`. Legacy `xls` is refused because its binary format has no streaming reader and cannot be bounded. Files over 10 MiB are refused. An `xlsx`/`docx` container is inflated once, entry by entry, through a 64 MiB cap before parsing, so a forged central directory cannot hide a decompression bomb. Workbooks never reach the dense-grid parser: cells are streamed one at a time through calamine's cell reader into the tool's own text under a 16 million character and 2 million cell budget, so two cells at opposite corners of a sheet cost two gap markers and a shared string costs only the cells that render it; a chart or dialog sheet holds no cells and is skipped rather than failing the workbook. Word documents are streamed the same way, through quick-xml (see "Word streaming"). `csv` text is checked against the character budget after parsing, where the parser's memory is already bounded by the 10 MiB file cap. These bound the parser's inputs; they are not an OS sandbox. |
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

## Word streaming

Building the docx object tree was the last unbounded step in the pipeline. `docx-rs` materializes
a document many times the size of the XML it parses, so a small file that inflates to 64 MiB of
empty paragraphs — within every earlier cap — could still exhaust memory. Word documents are
therefore streamed the way workbooks are: `word/document.xml` is read event by event through
`quick-xml` (already in the tree under `calamine`) and written into the tool's own text.

- **Layout.** One line per non-empty paragraph. A table row is one line with its cells
  tab-separated, matching how workbook rows are rendered, and the paragraphs inside one cell are
  joined with spaces. Outside a table `w:tab` stays a tab and `w:br`/`w:cr` a newline; inside one
  they become spaces, so cell text cannot forge a column or row break. Nested tables are flattened
  into the row that contains them. Blank paragraphs produce no line, no section, and no growth.
- **Excluded content.** `w:del` and `w:moveFrom` subtrees (a tracked deletion, and the source half
  of a tracked move, which would otherwise duplicate its `w:moveTo` counterpart),
  `w:instrText`/`w:delInstrText` field instruction codes such as `MERGEFIELD` (the field *result*
  a reader sees is a sibling `w:t` and is kept), and the `mc:Fallback` half of an
  `mc:AlternateContent` pair, whose text repeats the `mc:Choice` used instead. Matching is on the
  local name, so a writer's namespace prefix does not matter.
- **Bounds.** Every buffer that holds text on its way to the output — the paragraph, the table
  cell, the table row — is counted against the character budget as it grows, so neither one
  enormous paragraph nor one enormous row can balloon between checks; peak memory is the budget
  plus a single XML event. `word/document.xml` is itself read through the 64 MiB container cap
  even when the container preflight has not run, and `quick-xml` resolves only the five predefined
  XML entities, so no declared or nested entity expands here. The outline is capped at 4096
  sections: past the cap no new section starts and the last one absorbs the remaining text, so
  every offset reported stays truthful while the section table, which travels in the payload,
  stays bounded.
- **Out of scope.** Only `word/document.xml` is read. Headers, footers, footnotes, endnotes, and
  comments live in sibling parts and are deliberately not extracted.

## Error handling

Missing or malformed arguments (including a blank `sheet`), unsupported extensions (the message
lists supported ones), legacy `xls`, missing files, directories, files over the size cap,
containers over the inflation cap, workbooks over the cell or character budget, extracted text
over the character cap, unparseable files (the message carries the parser's reason), unknown
sheets (the message lists available non-empty sheets, which excludes chart and dialog sheets
because they hold no cells), sheet selection
on a workbook whose sheets are all empty, sheet selection on non-spreadsheets, a docx with no
`word/document.xml`, and malformed XML inside it all return
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

Word streaming and chart-sheet skipping add their own fixtures, all built in a temp directory the
way the earlier ones are — the accumulator's boundaries (an empty document, a budget hit exactly,
section grouping, the section cap) are unit-tested directly, without a document:

| Fixture | Proves |
|---|---|
| Workbook with one worksheet and one chart sheet, hand-built because the workbook writer in the tree cannot emit a chart sheet | the worksheet reads; the chart sheet is skipped without failing the workbook, and asking for it by name still says truthfully that it is not among the non-empty sheets |
| Docx of 50,000 blank paragraphs (self-closing and whitespace-only) plus a short real tail | the text, section table, and window are all the size of the tail, and the reported size is the file on disk |
| Docx with a tracked deletion written both ways (`w:delText` and a `w:t` inside `w:del`), a `MERGEFIELD` field, and a tracked move | deleted, moved-from, and instruction text are absent while the field result and the visible tail are kept |
| Docx with a table whose cell holds two paragraphs and a tab | rows render tab-separated, cell paragraphs join with spaces, and a tab inside a cell cannot forge a column |
| Docx whose `word/document.xml` alone inflates past the 64 MiB container cap, from a file far under the source cap | the typed inflation refusal, without OOM |
| Docx with one paragraph split across 500 runs, read under a 64-character budget | the budget refusal, and the same document reading cleanly under the real budget |
| Docx with a mismatched end tag, and a container with no `word/document.xml` | typed errors rather than a panic |

## Out of scope

PDF and plain-text formats, the stubbed `document_extract` tool, a `/doc` slash command,
per-session path sandboxing, and caching parsed documents across calls.
