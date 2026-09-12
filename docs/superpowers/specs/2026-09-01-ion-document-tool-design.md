---
title: Ion Document Read Tool Design
description: Design spec for document_read, a bounded and pageable document-analysis tool inside Ion's tool loop
updated: 2026-09-12
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
| `path` | Required. Relative paths resolve against the REPL's launch directory. Formats: `xlsx`, `csv`, `docx`. Legacy `xls` is refused because its binary format has no streaming reader and cannot be bounded. Files over 10 MiB are refused. An `xlsx`/`docx` container is inflated once, entry by entry, through a 64 MiB cap before parsing, so a forged central directory cannot hide a decompression bomb. Workbooks never reach the dense-grid parser: cells are streamed one at a time through calamine's cell reader into the tool's own text under a 16 million character and 2 million cell budget, so two cells at opposite corners of a sheet cost two gap markers and a shared string costs only the cells that render it; a chart, dialog, or macro sheet is not a worksheet, holds no cells, and is skipped rather than failing the workbook. Word documents are streamed the same way, through quick-xml (see "Word streaming"). `csv` text is checked against the character budget after parsing, where the parser's memory is already bounded by the 10 MiB file cap. These bound the parser's inputs; they are not an OS sandbox. |
| `sheet` | Worksheet name, case-insensitive: Unicode lowercasing plus the Latin multi-character folds, so `Straße`, `STRAẞE`, and `STRASSE` match; dotless `ı` stays distinct from `i`; no normalization. Spreadsheets only; empty worksheets are omitted by the parser, as are chart, dialog, and macro sheets, which hold no cells. A supplied blank value is rejected rather than treated as "whole document". When set, `offset` is relative to that sheet's text. |
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

- **Layout.** One line per non-empty paragraph, and one line per table row with its cells
  tab-separated, matching how workbook rows are rendered; the paragraphs inside one cell are
  joined with spaces. That mapping is absolute — an output line is always exactly one paragraph or
  one table row — and everything else follows from it. `w:br`/`w:cr` become spaces rather than
  newlines. Document text can never contribute a `\n` or `\r`, or, inside a row, a `\t`: a literal
  control character, a numeric reference such as `&#9;`/`&#10;`, and a CDATA section are all
  normalized one character for one as the text is read, so cell text cannot forge a column or a
  row. Outside a table `w:tab` stays a tab, which cannot break a line. A nested table flattens
  into the row that contains it, and the containing cell keeps its own text. Blank paragraphs
  produce no line, no section, and no growth. `window`'s line snapping and the section spans are
  exact because of this invariant.
- **Excluded content.** `w:del` and `w:moveFrom` subtrees (a tracked deletion, and the source half
  of a tracked move, which would otherwise duplicate its `w:moveTo` counterpart),
  `w:instrText`/`w:delInstrText` field instruction codes such as `MERGEFIELD` (the field *result*
  a reader sees is a sibling `w:t` and is kept), and the `mc:Fallback` half of an
  `mc:AlternateContent` pair, whose text repeats the `mc:Choice` used instead. Matching is on the
  local name, so a writer's namespace prefix does not matter — with two deliberate consequences:
  a simple field (`w:fldSimple`) keeps its cached result because its instruction lives in a
  `w:instr` *attribute* and attributes are never read; and text a writer put in a shape or text box
  (DrawingML `<a:t>`) or an equation (OMML `<m:t>`) is extracted like any other `t`, because a
  reader sees it too.
- **Bounds.** Every buffer that holds text on its way to the output — the paragraph, the table
  cell, the table row — is checked against the character budget *before* it grows, so no
  long-lived buffer overshoots on a single event. Peak memory is **not** the budget: `quick-xml`
  holds one event in its own buffer and keeps an open-element stack that scales with nesting
  depth, and both are bounded by the part rather than by the budget, so working memory for this
  path is roughly twice the 64 MiB part cap. That cap is what does the work, and
  `word/document.xml` is held to it here even when the container preflight has not run — a part
  that exceeds it is refused with a typed error rather than answered from the prefix that fit.
  `quick-xml` resolves only the five predefined XML entities, so no declared or nested entity
  expands here. The outline is capped at 4096 sections: past the cap no new section starts and the
  last one absorbs the remaining text, so every offset reported stays truthful while the section
  table, which travels in the payload, stays bounded.
- **Part lookup.** The part is found by comparing names ASCII-case-insensitively, the way
  calamine resolves workbook parts, because OPC part names are not case-sensitive. Where a
  container declares the part twice, the last entry wins.
- **Out of scope.** Only `word/document.xml` is read. Headers, footers, footnotes, endnotes, and
  comments live in sibling parts and are deliberately not extracted. A self-closing `<w:tc/>`,
  which the schema does not allow, drops that column.

## Error handling

Missing or malformed arguments (including a blank `sheet`), unsupported extensions (the message
lists supported ones), legacy `xls`, missing files, directories, files over the size cap,
containers over the inflation cap, workbooks over the cell or character budget, extracted text
over the character cap, unparseable files (the message carries the parser's reason), unknown
sheets (the message lists the readable worksheets, which excludes empty ones and chart, dialog,
and macro sheets alike), sheet selection
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
| Table cells carrying a literal tab and newline, `&#9;`/`&#10;`, and a CDATA section | one row and three columns: text cannot forge a column or row break |
| Paragraphs with a literal newline, `&#10;`, a `w:br`, and a `w:tab` | four paragraphs in, four lines out; a tab outside a table survives |
| Table nested inside a cell that has its own text | the outer text survives and the row still has two columns |
| `w:fldSimple` with a `MERGEFIELD` instruction attribute | the cached result is kept, the instruction is not |
| Docx whose part is named `Word/Document.XML` | the part is found case-insensitively |
| CDATA section that is not valid UTF-8 | a typed error rather than replacement characters |
| The oversized part read directly, without the container preflight | the typed too-large refusal, not a silently truncated answer |

## New kinds (Stage 1b-B, 2026-09-12): pdf, txt, md

Iteration 3, `docs/plans/2026-09-02-impulse-next-stages.md` Stage 1b. Adds three more kinds
under the same caps (`MAX_DOCUMENT_BYTES`, `ExtractBudget`, `MAX_CHARS_CAP`/`DEFAULT_MAX_CHARS`,
the rendered section-table cap) and the same sandbox check
(`ReplContext::sandbox_tool_context().is_path_allowed`, enforced in `resolve_document_path_with_cap`
exactly like `xlsx`/`csv`/`docx`); `document_extract` (the stubbed CLI/daemon dynamic tool whose
default path always errored) is deleted rather than extended, per the deferred decision this spec
originally left open.

- **PDF (text layer only).** New dependency: `pdf-extract` (crate `pdf-extract`, MIT, behind
  `office-support`; re-exports `lopdf` at its crate root, so no separate `lopdf` dependency was
  added). Page rendering runs in an **isolated child process** (see "Review round 1" below for why
  -- every other kind runs in-process on the blocking pool); the page count is read from
  `lopdf::Document::get_pages` in-process first (walking the page tree once, no text extraction)
  and refused above `MAX_PDF_PAGES` (4096) before any page is rendered. Inside the child, each
  page's text is checked against the character budget *per write*, not once per page after the
  fact -- true check-before-push, the same discipline as the workbook and Word streamers.
  `PlainTextOutput` implements only the text-output half of `pdf_extract::OutputDev` -- it has no
  image-handling code at all, so an embedded image can never reach this tool's output regardless
  of what the PDF contains, and neither can annotation or `AcroForm` field text (`/Annots`): only a
  page's own `/Contents` stream is rendered. An encrypted PDF is refused via a raw byte scan for
  `/Encrypt` in the file, run before any parser touches it -- `lopdf::Document::load` silently
  authenticates a PDF whose *user* password is empty, so `doc.is_encrypted()` after loading cannot
  be trusted (see "Review round 1"), and this tool makes no password attempt of any kind. A page
  with no extractable text (a scanned/image-only page) contributes no line and no section, exactly
  like a blank Word paragraph, so a PDF with no text layer at all parses successfully to an empty
  document with zero sections -- `render` says so explicitly (`(no extractable text layer: this
  PDF is likely scanned or image-only ...)`) rather than leaving the model to wonder whether
  reading failed. One section per non-empty page (`kind: "page"`, `name: "Page N"`), same shape as
  a workbook's one section per sheet.
- **txt.** Read whole (already bounded by `MAX_DOCUMENT_BYTES` at path-resolution time) and
  requires valid UTF-8; invalid UTF-8 is a typed error naming the first invalid byte's offset
  (`String::from_utf8`'s `valid_up_to()`) rather than a lossy replacement -- this tool does not
  guess an encoding, and a lossy read would make an offset the tool reports not correspond to what
  the model actually sees. One section spanning the whole document (`kind: "text"`), matching
  `csv`'s single-section shape.
- **md.** Same whole-file UTF-8 read as `txt`, plus an outline built from ATX headings (`#`
  through `######`, requiring a following space/tab or end of line, per CommonMark's basic rule --
  a run of more than 6 `#`s, or one glued directly to text like `#tag`, is not a heading; up to 3
  leading spaces are tolerated before the `#`s, also per CommonMark). A `#`-prefixed line inside a
  fenced code block (` ``` ` to ` ``` `, or `~~~` to `~~~`) is not treated as a heading, matching
  every Markdown renderer's own behavior -- a shell comment or a Python-style ATX-looking string
  literal inside a fenced snippet must not fragment the outline. An unterminated fence (no closing
  marker before the end of the file) is treated as remaining inside the fence for the rest of the
  document, the conservative reading. Deliberately not full CommonMark beyond these two rules: an
  optional closing run of `#`s (`## Title ##`) is left in the heading text rather than stripped, a
  documented simplification for a tool that only needs stable section boundaries, not a rendered
  heading. Content before the first heading belongs to no section (mirrors Word's "no growth
  before the first non-empty paragraph"). Bounded at `MAX_MD_SECTIONS` (4096) the same way Word's
  outline is bounded at `MAX_WORD_SECTIONS`: past the cap no new section starts and the most
  recently opened one keeps absorbing text, so every offset already reported stays truthful.

### Review round 1 (2026-09-12): PDF isolation redesign

An adversarial pass against the first PDF landing found two P0s and one P2 in the in-process
design above, all now fixed by moving page *rendering* (never the cheap page-count check) into an
isolated child process:

- **P0-1.** A 677-byte PDF whose Form XObject content stream references itself (`/X0 Do` inside
  `/X0`'s own content stream) makes `pdf_extract::output_doc_page`'s content-stream interpreter
  recurse until the thread's stack is exhausted -- a Rust stack-overflow guard-page hit, which
  calls `abort()`, not an unwinding panic. `tokio::task::spawn_blocking`'s `JoinError` containment
  only catches unwinding panics, so a hostile PDF read through `document_read` -- an ungated tool
  -- killed the entire `ion` process. Fix: PDF page rendering runs in a hidden `internal-pdf-text`
  subcommand (`#[command(hide = true)]`, both `impulse-rs` and `ion` binaries share one handler),
  spawned via `tokio::process::Command` with `kill_on_drop`, a process-group guard, a 30s
  wall-clock timeout, and (unix) `RLIMIT_AS`/`RLIMIT_CPU` set via `pre_exec`. The child's crash
  kills only the child; the parent observes a signaled exit and reports a typed error.
- **P0-2.** The original per-page sink rendered a whole page into an unbounded `String` before
  checking it against the character budget -- a 4.2 MB crafted PDF (one page, a huge repeated `Tj`
  string) reached 7.0 GB RSS / 178.5s in testing. Fix: the child's sink is a `std::fmt::Write`
  implementation (`BoundedSink`) that refuses a write the instant the running total (cumulative
  across pages, not reset per page) would exceed the budget -- checked during rendering, not
  after. `RLIMIT_AS` (default 1 GiB) is a second, best-effort layer on top (not fully enforced on
  macOS; real enforcement is Linux).
- **P2-1.** `lopdf::Document::load` unconditionally tries an *empty* user password first and
  silently decrypts on success -- a common real-world case (owner-password-only protection leaves
  the user password empty) -- so `doc.is_encrypted()` after loading cannot be trusted. Fix: a raw
  byte scan for the literal `/Encrypt` token, run against the file's bytes before any parser sees
  them, in both the parent's cheap pre-check and the child's own independent check.

Two nits from the same pass, also fixed: the child iterates `lopdf::Document::get_pages()`'s own
map keys rather than assuming a contiguous `1..=N` range, and the module doc's earlier "absolute
paths are accepted" wording was corrected to name the sandbox constraint that already applied via
`resolve_document_path_with_cap` (it undersold what was actually enforced, not a security gap).

### Fixture table

| Fixture | Proves |
|---|---|
| PDF with two pages of real text, built with `lopdf` (re-exported by `pdf_extract::*`) the way `create.rs` in lopdf's own examples does | two `page` sections named `Page 1`/`Page 2`; both pages' text is in the window; render ends `complete` |
| PDF with one page and an empty content stream (no text operators at all) | zero sections, `total_chars == 0`, and `render`'s explicit "no extractable text layer" note |
| PDF whose page content stream draws a Form XObject that draws itself | a typed error naming a signal, and the calling process (the integration test) demonstrably still alive afterward -- run only via the isolated child, never in-process |
| PDF re-saved with RC4 V1 encryption, non-empty user password | the typed encrypted-PDF refusal, before any page is read |
| PDF re-saved with RC4 V1 encryption, EMPTY user password | the same refusal -- proves the byte scan, not `is_encrypted()`, is authoritative |
| PDF with more pages than an injected page-cap test seam | the typed page-count refusal, without building a real multi-thousand-page fixture |
| PDF whose single page's text exceeds an injected tiny character budget | the typed over-the-limit refusal, and promptly (proving check-before-push, not build-then-check) |
| An impossibly short injected timeout against an ordinary, fast-parsing PDF | the typed timeout refusal, deterministically, without a genuinely slow fixture |
| A single 10 MB `write_str` call against `BoundedSink` directly (no PDF/subprocess at all) | the buffer never grows past the character cap, asserted on the count, not just an error string |
| Bytes that are not a PDF at all, saved with a `.pdf` extension | a typed parse error, not a panic |
| Empty `.txt` file | empty text, zero sections |
| `.txt` exactly at a budget, then one character over | accepted, then the typed over-the-limit refusal |
| `.txt` with a byte that is not valid UTF-8 | the typed UTF-8 refusal naming the invalid byte's offset |
| `.md` with a heading, read via `outline=true` then jumped to by the heading's own offset | the section table and offset-based jump agree |
| `.md` with `MAX_MD_SECTIONS + 5` headings | exactly `MAX_MD_SECTIONS` sections, and the last one's offset+chars reaches the true end of the text |
| `.md` with a `#`-looking line inside a fenced code block | not treated as a heading |
| A previously-`.pdf`-specific "unsupported extension" test, now retargeted at `.pptx` | `pdf`/`txt`/`md` are supported extensions; a still-unsupported one still names them all in its error |

## Out of scope

Caching parsed documents across calls, a `/doc` slash command, and OCR/rendering for a
PDF with no text layer (scanned/image-only pages are reported empty, never rasterized or sent to
an OCR model). Per-session path sandboxing is no longer out of scope -- see
`docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`, which this tool's
`resolve_document_path_with_cap` already implements for every kind, including the three added
here.
