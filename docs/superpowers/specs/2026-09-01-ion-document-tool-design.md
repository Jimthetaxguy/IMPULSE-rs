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
  and refused above `MAX_PDF_PAGES` (4096) before any page is rendered. Also before any page is
  rendered, every `FlateDecode`-filtered stream object is inflated once through a bounded,
  discard-the-bytes counting decoder (`preflight_pdf_streams`, the PDF analogue of
  `preflight_container`'s zip-container check), refusing above a 64 MiB per-stream/total cap --
  see "Review round 2" below for why this, not the character budget, is what actually bounds
  memory. Inside the child, each page's text is ALSO checked against the character budget *per
  write*, not once per page after the fact -- true check-before-push, the same discipline as the
  workbook and Word streamers, but a bound on OUTPUT VOLUME and WALL CLOCK, not memory.
  `PlainTextOutput` implements only the text-output half of `pdf_extract::OutputDev` -- it has no
  image-handling code at all, so an embedded image can never reach this tool's output regardless
  of what the PDF contains, and neither can annotation or `AcroForm` field text (`/Annots`): only a
  page's own `/Contents` stream is rendered. An encrypted PDF is refused via a name-escape-aware
  raw byte scan for `/Encrypt` in the file, run before any parser touches it, plus an independent
  "belt and braces" check against the parsed trailer after `Document::load` -- `lopdf::Document::load`
  silently authenticates a PDF whose *user* password is empty, so `doc.is_encrypted()` after
  loading cannot be trusted (see "Review round 1"), and this tool makes no password attempt of any
  kind. A page
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

### Review round 2 (2026-09-12): unbounded child stdout, a compression bomb, and a name escape

A second adversarial pass refuted three round-1 claims and added one missing test.

- **P1.** `run_pdf_extraction_child` used `child.wait_with_output()`, which buffers the whole
  child stdout/stderr with no cap -- a rogue or compromised child streaming ~1 GiB of output drove
  the PARENT to ~3.2 GB RSS and produced an accepted, multi-gigabyte-character document. Fixed
  with `read_capped` (stdout: refuse and kill the child once the JSON-encoded worst case for the
  requested budget would be exceeded) and `read_capped_tail` (stderr: bounded to 64 KiB, keeping
  the tail, which never refuses since there is no fixed budget diagnostic text could violate).
  Both pipes are read on independent `tokio::spawn`ed tasks rather than joined with something like
  `tokio::join!`: a child blocked writing past the stdout cap never closes ANY of its pipes,
  including stderr, so waiting for both to resolve together would hang until the wall-clock
  timeout instead of returning promptly. Regression test:
  `tests/fakes/rogue-stdout-shim.sh` (a shell script that ignores every argument and writes far
  more than any reasonable stdout cap) stands in for `exe` directly, proving the parent detects
  and kills it well under a second.
- **P2.** `BoundedSink` bounds output volume and wall clock, never memory: `pdf-extract`'s own
  internal decompression of a stream's content happens BEFORE `BoundedSink` ever sees a
  character, and is itself completely unbounded. The review's `textbomb2g.pdf` (4.2 MB) inflated
  476x before the first sink write, reaching multiple GB of RSS in the earlier fix regardless of
  how small `max_chars` was set. Fixed with `preflight_pdf_streams` -- the PDF analogue of
  `preflight_container`'s zip-container check -- run before any page renders, in both the
  parent's cheap pre-check and the child's own independent check: it inflates every
  `FlateDecode`-filtered stream object through a streaming decoder into a counting sink,
  discarding bytes as it counts them, refusing with a typed error once a per-stream cap (64 MiB)
  or the combined total (64 MiB) is exceeded. Peak memory after the fix, measured on this
  checkout's release build: `textbomb500.pdf` (1.05 MB) refuses in ~0.56s at ~13 MB peak RSS;
  `textbomb2g.pdf` (4.2 MB) refuses in ~0.03s at ~22 MB peak RSS -- both far under the review's
  "tens of MB" target, and both refused before any page rendering begins. The 600-page, 9.9 MB
  legitimate fixture (`legit10m.pdf`) still renders correctly after the fix, in ~0.73s at ~51 MB
  peak RSS (matching the review's own pre-fix baseline of ~0.68s/54 MB, confirming no regression
  to the happy path). Only `FlateDecode` streams are checked -- a documented limitation: it is
  both the PDF ecosystem's dominant filter and the one this attack class actually uses; the
  alternatives (`LZWDecode`, `ASCII85Decode`, `RunLengthDecode`) cannot reach comparable inflation
  ratios, and `DCTDecode` is JPEG image data `PlainTextOutput` never reads.
- **P2.** The raw `/Encrypt` byte scan is a literal-bytes-only match, bypassed by a legal PDF name
  hex-escape (ISO 32000-1 §7.3.5): `/Encr#79pt` (`#79` = hex 0x79 = ASCII `y`) parses to the
  identical name `Encrypt` in `lopdf` (and every conformant parser) but contains no literal
  `/Encrypt` byte sequence at all. Verified against the review's own `enc_emptyuser_hexname.pdf`
  fixture: before the fix this parsed successfully and returned plaintext; after, it is refused
  identically to the literal-bytes case. Fixed with a two-tier scan -- the fast literal check
  first (the common case), then, only if that finds nothing, a fallback that decodes every `/`
  token's escapes (`decode_pdf_name_token`) before comparing -- plus a second, structurally
  independent "belt and braces" check directly against the *parsed* trailer dictionary
  (`doc.trailer.get(b"Encrypt")`) after `Document::load`, in both the parent and the child:
  `lopdf`'s auto-decrypt-on-load transparently decrypts object CONTENTS but does not remove the
  trailer's own `/Encrypt` reference, so this check catches the same case through an independent
  path. **The false-positive path is genuinely inconsistent, verified empirically rather than
  merely asserted:** a page whose own text contains the literal characters `/Encrypt`, stored in
  an UNCOMPRESSED content stream (`encrypt_in_content.pdf`), is refused -- the scan runs over raw
  file bytes and cannot distinguish page text from a real trailer key. The identical text stored
  in a `FlateDecode`-COMPRESSED content stream (`encrypt_in_compressed_content.pdf`) parses
  normally, because the compressed on-disk bytes never contain the literal string at all. This is
  accepted, not fixed further: a false positive fails closed (the safe direction), and resolving
  the inconsistency would require decompressing every stream before the encryption scan can even
  run, which is exactly the ordering `preflight_pdf_streams` (the memory bound) does NOT want to
  assume is already safe to do.
- **P3.** Added the missing regression test that an EXPLICITLY-supplied out-of-sandbox
  `impulse_dir` is denied for both `memory_search` and `genome_read`, through the real
  `DynamicToolBridge`/`ToolRegistry::execute`/`validate_paths` path rather than by calling each
  tool's `execute()` directly (which the round-1 tests did, proving only the ctx-default selection
  logic, not that the sandbox actually denies an out-of-bounds explicit value).

Two doc-comment corrections from this pass: `tool_document.rs`'s `PDF_CHILD_MEMORY_LIMIT_BYTES`
comment previously claimed "the primary bound is still the sink" -- false, per P2 above --
and `internal_pdf_text.rs`'s module doc gained a note distinguishing what `BoundedSink` bounds
(output volume, wall clock) from what `preflight_pdf_streams` bounds (memory).

### Review round 3 (2026-09-12): the parent still parsed PDF structure, and the preflight missed LZW

A third adversarial pass refuted round 2's memory bound twice more, found a related false refusal,
and found the total cap refused ordinary documents. All five findings share one root cause: only
the child may ever touch PDF structure, and everything the preflight cannot bound-count must be
refused, not silently skipped.

- **F0 (P0).** `precheck_pdf` (the parent's "cheap" page count) called `pdf_extract::Document::load`
  IN THE PARENT before `preflight_pdf_streams` ran. `lopdf::Document::load` eagerly, unconditionally
  decompresses every `/Type /ObjStm` object stream as part of loading, with no hook to intercept or
  bound it -- so a crafted ObjStm blew up the PARENT itself before any preflight ever got a chance
  to refuse it. The review's `objstm_bomb2g.pdf` (2.04 MB on disk) drove the PARENT to 2,078 MB RSS
  and still reported "OK"; it scales to ~10 GB at the 10 MiB source-file cap. Fix, structural: the
  parent's ENTIRE PDF-specific job before spawning the child is now `pdf_encryption_prescan` -- a
  raw byte scan for `/Encrypt`, nothing else. `precheck_pdf` is deleted. Page count, the
  trailer-based encryption re-check, `preflight_pdf_streams`, and rendering all moved into
  `handlers::internal_pdf_text`, which is now sole and authoritative for all PDF structure parsing.
- **F2.** Even confined to the child, `Document::load`'s eager ObjStm inflation cannot be
  pre-counted -- there is no hook between "lopdf decides to inflate an object stream" and "lopdf has
  already inflated it." `RLIMIT_AS` (the round-1 defense-in-depth layer) turned out to be accepted
  but silently NOT kernel-enforced on macOS (`setrlimit` returns success; a 1 GiB limit let a child
  reach 4.12 GiB RSS in testing) -- so on the primary development platform, the child had no memory
  bound at all for this path. Fix: a memory-watchdog background thread
  (`spawn_memory_watchdog`/`current_peak_rss_bytes`), started before any PDF parsing, polling this
  process's own peak RSS (`getrusage`, normalized for the BYTES-on-macOS/KILOBYTES-on-Linux
  `ru_maxrss` unit inconsistency) roughly every 10ms and self-terminating via
  `std::process::exit(PDF_MEMORY_CEILING_EXIT_CODE = 137)` the instant it crosses
  `PDF_CHILD_MEMORY_LIMIT_BYTES` (kept at 1 GiB). The parent checks this specific exit code BEFORE
  its generic signal check, mapping it to a typed "exceeded its memory ceiling" error rather than a
  generic parse failure. `RLIMIT_AS` stays as real, kernel-enforced defense-in-depth on Linux; on
  macOS, isolation is honestly "crash containment plus the watchdog," not "RLIMIT_AS" -- documented
  as such rather than implied otherwise. Reproduced on the review's own `objstm_bomb2g.pdf`: the
  child now peaks at ~1.08 GB RSS (bounded near the ceiling, not the review's original unbounded
  2+ GB) and exits 137; the parent's own RSS stays negligible throughout, since it never touches the
  file's PDF structure at all.
- **F1 (P1).** `preflight_pdf_streams` only ever counted `FlateDecode` streams. `lopdf` decodes
  `/LZWDecode` exactly as readily; the review's `lzwmulti300.pdf` (1.65 MB) reached 4.12 GiB child
  RSS while the preflight still reported "OK". Fix: the preflight now walks each stream's FULL
  filter chain via `Stream::filters()` (lopdf's public, decode-ordered accessor) rather than just
  checking for a bare `FlateDecode` name: a terminal `FlateDecode` or `LZWDecode` stage is
  bound-counted through a streaming decoder (`inflate_bounded_zlib`/`inflate_bounded_lzw`, the
  latter via `weezl::decode::Decoder::with_tiff_size_switch(BitOrder::Msb, 8)`, matching `lopdf`'s
  own internal parameters exactly so it decodes the identical byte stream). Reproduced on the
  review's own `lzwmulti300.pdf`: now refused via the TOTAL cap (`... inflates to more than
  536870912 bytes combined, over the limit`) at ~25 MB peak RSS in ~1.85s, not 4+ GB.
- **F3.** A direct consequence of F1's fix, and a real correctness gap it closed along the way: a
  `[/ASCII85Decode /FlateDecode]` filter CHAIN was previously refused with "corrupt deflate stream"
  on any legitimate document using it, because the still-ASCII85-encoded outer bytes were fed
  straight to zlib. Fix: an outer `ASCII85Decode`/`ASCIIHexDecode` layer is now fully decoded first
  (bounded, cheap -- roughly 5:4/2:1 expansion, and stream content is already bounded by the 10 MiB
  source-file cap), and the RESULT fed to the terminal Flate/LZW bounded decoder. Reproduced on the
  review's own `chain_a85_flate_legit.pdf`: previously refused, now extracts successfully; the
  matching bomb variant `chain_a85_flate.pdf` is still correctly refused. Any filter this preflight
  cannot bound-count at all (`RunLengthDecode`, `DCTDecode`, `JPXDecode`, `CCITTFaxDecode`, `Crypt`,
  or anything chained after a terminal Flate/LZW stage) is now refused BY NAME -- deny-by-default,
  not silently skipped the way every non-`FlateDecode` filter was in round 2. Reproduced on the
  review's own `rlbomb.pdf` (`RunLengthDecode`): refused naming the filter, at ~20 MB peak RSS.
- **F4.** The round-2 64 MiB TOTAL cap refused ordinary documents -- a 900-page, 2.87 MB
  Flate-compressed text PDF inflates to 73.6 MB, over the old cap -- while the round-2 "legitimacy"
  fixture (`legit10m.pdf`) has NO compressed streams at all, so it never actually exercised this cap
  either way. Fix: the per-stream cap stays 64 MiB (`MAX_PDF_STREAM_DECOMPRESSED_BYTES`); the total
  cap rises to 512 MiB (`MAX_PDF_TOTAL_DECOMPRESSED_BYTES`), justified against
  [`MAX_DOCUMENT_BYTES`] (10 MiB source-file cap: 512 MiB is a ~51x expansion ceiling on the largest
  input this tool ever reads) and the separate, independent `MAX_EXTRACTED_CHARS` budget (16 million
  characters of FINAL output text is capped regardless of how much raw decompressed PDF-operator
  bytes it took to render that much). A new fixture (`write_legit_multi_page_flate_pdf`, 25
  genuinely Flate-compressed pages) proves the raised cap actually admits large real documents,
  extracting successfully; the round-2 bombs (`textbomb500.pdf`/`textbomb2g.pdf`) still refuse
  correctly. (The review's own `legit64.pdf` fixture was also reproduced, but its Python generator
  has an unrelated pre-existing bug -- `/F1`'s font reference resolves to the Pages object, not a
  Font -- which panics `pdf_extract::get_name_string` during rendering regardless of this fix;
  isolation still contains it cleanly, exit 101, the parent surfaces a typed parse error, ~34 MB
  peak RSS, no memory blowup. Not a security regression, and out of scope to patch a third-party
  crate's font-resolution panic; the new purpose-built fixture proves F4's actual claim instead.)
- **F5 (doc corrections, no code change).** Two round-2 doc numbers were re-measured and corrected.
  The production stdout cap for the DEFAULT budget (16,000,000 chars / 4,096 pages) is exactly
  128,266,240 bytes (`max_child_stdout_bytes`'s formula, derived from `MAX_EXTRACTED_CHARS`, not
  from whatever `max_chars` a caller happens to pass) -- this was already correct, just previously
  understated in prose. Parent RSS on a rogue child (`tests/fakes/rogue-stdout-shim.sh`) through the
  DEFAULT budget (not an artificially small one) measures ~139 MB, not the ~9.3 MB an earlier,
  smaller-budget-only measurement implied -- both numbers are real, they were just measuring
  different budgets; the ~139 MB figure is the one a production caller actually experiences.

Two structural additions from this pass, beyond the fixes themselves: a `weezl` direct dependency
(already transitively present via `lopdf`/`gif`/`tiff` at the same 0.1.x line, so no new download or
version conflict) for the bounded LZW decoder, and a `--memory-limit-bytes` hidden CLI flag on
`internal-pdf-text` (`Option<u64>`, defaulting to the production ceiling when omitted) so integration
tests can prove the watchdog mechanism fires end to end, through the real parent exit-code mapping,
using a small fixture against a small injected ceiling rather than needing a genuinely gigabyte-scale
bomb in the default test suite (`extract_pdf_with_exe_timeout_and_memory_limit` is the corresponding
Rust-side seam, mirroring the existing exe/timeout injection pattern). `RLIMIT_AS` is deliberately
NOT affected by this override, so a test using a low injected ceiling exercises the watchdog
specifically, never `RLIMIT_AS`.

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
| A rogue shim (`tests/fakes/rogue-stdout-shim.sh`) that ignores every argument and streams far more than any reasonable stdout cap, pointed at as `exe` directly | the typed "exceeded its output bound" refusal, detected and the child killed well under a second |
| `BoundedSink::write_str` fed a single 10 MB write, and fed writes seeded with a prior page's running count | peak buffer size never exceeds the cap; the cap is cumulative across pages, not reset per page |
| A `FlateDecode` stream compressing 1,000,000 repeated bytes down to a few KB, fed to `inflate_bounded`/`preflight_pdf_streams` with a small injected cap | the typed "over the limit" refusal, and the true inflated size when under cap |
| A PDF whose page content stream is itself a `FlateDecode` bomb (compact on disk, inflates far past 64 MiB), read through the real `internal-pdf-text` child | refused before any page renders, at tens of MB of peak RSS, not gigabytes |
| A PDF whose trailer names `/Encr#79pt` (ISO 32000-1 §7.3.5 hex escape) instead of the literal `/Encrypt` | the same encrypted-PDF refusal as the literal form -- proves the scan decodes name escapes, not just literal bytes |
| The identical literal text `/Encrypt`, once in an uncompressed content stream and once in a `FlateDecode`-compressed one | refused in the first case (false positive, documented and accepted), parses normally in the second -- the scan operates on raw file bytes only |
| `memory_search`/`genome_read`, called through the real `DynamicToolBridge`/`ToolRegistry::execute` path with an explicit `impulse_dir` outside the sandbox | refused by `validate_paths` before either tool's own `execute` runs; the identical path granted via `/allow` succeeds |
| A raw-bytes-constructed PDF whose Catalog/Pages/Page/Font all live inside one `/Type /ObjStm` object stream, padded with an ignored comment, run through the real tool with a small INJECTED memory-watchdog ceiling (`extract_pdf_with_exe_timeout_and_memory_limit`) | the typed "exceeded its memory ceiling" refusal; the same fixture succeeds under a generous injected ceiling, proving the refusal is specifically the watchdog, not an unrelated parse error |
| An 80 MB (decoded) `LZWDecode`-filtered stream, run through the real tool at PRODUCTION caps | the typed "over the limit" refusal, end to end (not just at the unit-level `preflight_pdf_streams` seam) |
| A `[/ASCII85Decode /FlateDecode]` chain over legitimate, small compressed text, run through the real tool | extracts successfully (previously refused with "corrupt deflate stream") |
| A 25-page, genuinely Flate-compressed legitimate document whose combined inflated size clears the OLD 64 MiB total cap but stays under the new 512 MiB one | extracts successfully, all 25 page sections present |

## Out of scope

Caching parsed documents across calls, a `/doc` slash command, and OCR/rendering for a
PDF with no text layer (scanned/image-only pages are reported empty, never rasterized or sent to
an OCR model). Per-session path sandboxing is no longer out of scope -- see
`docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`, which this tool's
`resolve_document_path_with_cap` already implements for every kind, including the three added
here.
