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

## What already existed

- `src/office` parses `xlsx`, `xls`, `csv`, and `docx` into an `ExtractionResult` (full text
  plus typed chunks). It had no end-to-end tests with real files.
- `src/tooling/document` wraps those parsers as dynamic tools for the CLI and daemon. They return
  the entire document in one payload and were not registered in Ion's REPL tool set.

## Assumptions

- Tool results feed a model inside a loop contract (ADR-0017), so bounded output with explicit
  continuation is worth more than completeness in one call. This follows current guidance on
  writing tools for agents: return high-signal, paginated responses with a clear next step.
- Read-only document access does not need a confirmation gate; it matches `file_read`.
- Everyday documents live outside repositories, so absolute paths must work.

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
| `path` | Required. Relative paths resolve against the REPL's launch directory. |
| `sheet` | Worksheet name, case-insensitive. Spreadsheets only. |
| `outline` | Sections and sizes only, no content. |
| `offset` | Character offset to start from; use `next_offset` from the previous call. |
| `max_chars` | Characters to return. Default 8000, capped at 32000, zero rejected. |

Payload (`DocumentWindow`): path, format, document type, size, section table (index, kind,
optional sheet name, character count), selected section, `total_chars`, `offset`,
`returned_chars`, `truncated`, `next_offset`, and `content` (absent in outline mode). The
rendered text repeats the header and section table and ends the window with either
"complete" or "truncated, continue with offset=N".

## Error handling

Missing or malformed arguments, unsupported extensions (the message lists supported ones),
missing files, directories, unknown sheets (the message lists available sheets), and sheet
selection on non-spreadsheets all return typed errors with the path or name in the message, so
the loop contract's same-error detector can recognize a model that keeps repeating a bad call.

## Testing

Pure helpers are unit-tested directly. Fixture tests generate CSV, XLSX (via the workbook
writer already in the dependency tree), and DOCX (via the docx builder) in a temp directory and
drive the tool end to end. One executor test proves the tool is ungated and bounded.

## Out of scope

PDF and plain-text formats, the stubbed `document_extract` tool, a `/doc` slash command,
per-session path sandboxing, and caching parsed documents across calls.
