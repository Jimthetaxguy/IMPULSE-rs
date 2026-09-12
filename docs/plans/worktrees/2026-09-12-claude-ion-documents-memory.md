---
title: Ion Documents and Memory Tools
description: Work card for claude/ion-documents-memory-20260912 (document_read gains pdf/txt/md; document_extract deleted; memory_search/genome_read bridged into Ion's ReplToolRegistry)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, ion, document-analysis, memory, tools]
---

# Ion Documents and Memory Tools

## Lane Facts

- Owner: Claude (Fable 5.1), Stage 1b-B of `docs/plans/2026-09-02-impulse-next-stages.md`.
- Role: implementation lane, parallel to sibling Stage 1b-A (`claude/ion-provider-neutral-20260912`,
  owns `src/llm_backends/**`, `src/loop_contract.rs`, `src/ion_repl/chat.rs`).
- Branch: `claude/ion-documents-memory-20260912`, base `origin/main` at `7c2086c`.
- Worktree: `.worktrees/ion-documents-memory-20260912` (repository-relative).
- Owned paths:
  - `impulse-rs/src/ion_repl/{tool_document.rs,registry.rs,tools.rs,mod.rs}` (tool_bridge.rs read,
    not modified — no changes needed there; its existing `run()` already builds `ToolContext` from
    `ctx.sandbox_tool_context()`, which is what makes bridging `memory_search`/`genome_read`
    sandbox-safe with no new code)
  - `impulse-rs/src/tooling/builtin/{document_extract.rs (deleted),mod.rs}`
  - `impulse-rs/src/mcp/server.rs` (tool list test only — no production change needed; MCP
    `tools/list` already derives from `ToolRegistry::schema_json()`, so removing
    `DocumentExtractTool`'s registration was enough)
  - `impulse-rs/src/office/**` (read only — no changes; `pdf`/`txt`/`md` are handled entirely
    inside `tool_document.rs`, independent of `office::OfficeFormat`, so this crate never needed
    to change)
  - `impulse-rs/Cargo.toml` (one optional dependency line, `pdf-extract`, under `office-support`)
    and `impulse-rs/Cargo.lock` (follows)
  - `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`, `CONTEXT.md` (two glossary
    entries: `document read tool` updated, `bridged memory tools` added), `HANDBOOK.md` (one
    stale reference removed), this work card
- Blocked/shared paths (per the dispatching prompt): `impulse-rs/src/llm_backends/**`,
  `src/loop_contract.rs`, `src/ion_repl/chat.rs` (sibling Stage 1b-A lane), `src/daemon/**`,
  `src/state/**`, `impulse-desktop/**`, `.github/**`, `CLAUDE.md`, `AGENTS.md`.
- Plan/spec: `docs/plans/2026-09-02-impulse-next-stages.md` Stage 1b bullets 2 and 4;
  `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`.
- Verification (isolated `CARGO_TARGET_DIR`): `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `cargo build --no-default-features`, `cargo test --no-default-features --lib -- ion_repl::registry`,
  `cargo audit`, `python3 docs/validate_docs.py --all`.
- Latest status: see Changes/Tests/Decisions below; gate results recorded at PR time.

## Decisions

- 2026-09-12: PDF crate is `pdf-extract` (MIT), matching the next-stages plan's own recommendation
  (open decision 5: "`pdf-extract` behind `office-support`, text layer only"). It re-exports
  `lopdf` at its crate root, so no separate `lopdf` dependency was needed — including for test
  fixtures, built the same low-level way lopdf's own `create.rs`/`encrypt.rs` examples do.
- 2026-09-12: Stream PDF pages one at a time through `pdf-extract`'s *public*
  `output_doc_page`/`PlainTextOutput` API, never the crate's own whole-document
  `extract_text`/`extract_text_by_pages` helpers — both build the entire result internally before
  this tool could enforce a page-count or character cap. The page count comes from
  `lopdf::Document::get_pages` (walking the page tree once, no text extraction) and is checked
  before any page renders.
- 2026-09-12: Refuse encrypted PDFs unconditionally via `doc.is_encrypted()`, before attempting
  anything — `pdf-extract`'s own whole-document helpers try an empty password first, which would
  make "encrypted" support depend on how a given file happened to be protected. This tool never
  supplies a password.
- 2026-09-12: `txt`/`md` require valid UTF-8; invalid UTF-8 is a typed error naming the first bad
  byte's offset, not a lossy replacement — a lossy read would make offsets this tool reports not
  correspond to text the model actually sees.
- 2026-09-12: Markdown ATX heading detection is deliberately not full CommonMark (an optional
  closing `#`-run is left in the heading text rather than stripped) — this tool only needs stable
  section boundaries, not a rendered heading.
- 2026-09-12: `document_extract` deleted rather than extended (the next-stages plan's open
  decision 2, and this repo's real-systems-only norm: its default path always returned "Python
  extraction not yet wired", a permanent stub, not a 14-day TODO).
- 2026-09-12: `memory_search`/`genome_read` are bridged via `DynamicToolBridge`, exactly like
  `file_read`/`file_write`/`bash_exec`, rather than given bespoke `ReplTool` wrappers — both
  already exist as `DynamicTool`s with `impulse_dir` declared `ParamType::FilePath`, so the shared
  `ToolRegistry::execute` → `validate_paths` step already enforces the sandbox on them once
  bridged; no new sandboxing code was needed or written.
- 2026-09-12: Did not touch `memory_search`'s/`genome_read`'s internals (they still read
  `.impulse/GENOME.md` and query `retrieval::search_history`/`search_genome`) — those files are
  outside this lane's owned paths. A concurrent sibling session (ADR-0020, memory promotion) asked
  whether `memory_search` should read a new `.impulse/GENOME_PROJECTION.md` instead; declined as
  out of scope for this lane and left to that lane's own PR.

## Changes

- `document_read` gains `pdf` (text layer only, one `page` section per non-empty page, `Page N`
  named, capped at `MAX_PDF_PAGES` = 4096), `txt` (one `text` section spanning the whole
  document), and `md` (one `heading` section per ATX heading, capped at `MAX_MD_SECTIONS` = 4096)
  — all under the same `MAX_DOCUMENT_BYTES`/`ExtractBudget`/`MAX_CHARS_CAP` caps and the same
  `ReplContext::sandbox_tool_context().is_path_allowed` check every existing kind already used.
  `render()` gained one note: a PDF with `total_chars == 0` says so explicitly ("no extractable
  text layer: this PDF is likely scanned or image-only").
- `document_extract` (`src/tooling/builtin/document_extract.rs`) deleted; its registration removed
  from `src/tooling/builtin/mod.rs::register_all`; MCP `tools/list` no longer advertises it
  (verified by a new test, since `tools/list` derives from the same registry).
- `memory_search` and `genome_read` bridged into `ReplToolRegistry::with_defaults()` alongside
  `file_read`/`file_write`/`bash_exec` (registry now holds 8 tools with `office-support`, 7
  without, up from 6/5).
- `Cargo.toml`: one new optional dependency, `pdf-extract = { version = "0.12", optional = true }`,
  added to the `office-support` feature's dependency list.

## Tests

- `tool_document.rs`: pure unit tests for `parse_atx_heading` (levels 1-6, empty heading, no space
  after `#`, more than 6 `#`s, empty line) and `markdown_sections` (empty text, content before the
  first heading, spans tiling the whole text, the `MAX_MD_SECTIONS` cap with the last section
  absorbing the remainder); `extract_plain_text` boundary tests (empty file, exactly-at-budget then
  one-over, invalid UTF-8 naming the byte offset, `md` builds sections where `txt` builds one);
  `resolve_document_path` extension-gate test retargeted from a now-supported `.pdf` to `.pptx`,
  plus a new test proving `.pdf`/`.txt`/`.md` (and their uppercase spellings) resolve.
- `tool_document.rs` fixtures (built with `lopdf`, re-exported by `pdf_extract::*`, the same way
  lopdf's own `create.rs`/`encrypt.rs` examples build a PDF): a two-page PDF read end to end
  through `DocumentReadTool::run` (sections, content, `complete`); a PDF with one page and an empty
  content stream reading to zero sections/`total_chars` with the "no extractable text layer" note;
  an RC4-V1-encrypted PDF refused before any page reads; a page-count-cap test seam
  (`extract_pdf_with_page_cap`) refusing a 3-page PDF over a cap of 2, so the real 4096-page cap
  never needs a slow fixture; a character-budget refusal on a single over-budget page; a non-PDF
  file rejected as a typed parse error; `txt`/`md` read end to end (one section vs. heading
  sections, jump-by-offset round trip, invalid-UTF-8 refusal).
- `registry.rs`: `test_with_defaults_registers_ion_verify_and_write_capable_tools` updated for the
  new tool count (8/7) and the two new names; a new
  `test_with_defaults_registers_memory_search_and_genome_read_as_ungated_reads` asserting their
  schema names are reachable through the registry.
- `mcp/server.rs`: `test_mcp_tools_list_does_not_advertise_document_extract`.
- `--no-default-features` build and `cargo test --no-default-features --lib -- ion_repl::registry`
  both still pass with `document_read` absent and the registry count asserted at 7.

## Dependency and Audit

- `pdf-extract 0.12.0` (MIT), pulling `lopdf 0.42.0` (already present in the local registry cache
  from other tooling on this machine, unrelated crate), `adobe-cmap-parser`, `euclid`, `postscript`,
  `type1-encoding-parser`, `cff-parser`, `unicode-normalization`, `encoding_rs`, `log` — none of
  which appear anywhere in `cargo audit`'s output (checked by name individually). `cargo audit`
  reports 11 pre-existing vulnerabilities and 18 pre-existing warnings on this checkout, all
  attributable to crates already in the dependency tree before this lane (`h2`, `quick-xml`,
  `webbrowser`, `fxhash`, `instant`, `paste`, `proc-macro-error`, `ttf-parser`, the `unic-*`
  family, `anyhow`, `event-listener`, `glib`, `lru`, `memmap2`, `rand`) — zero of them are new
  dependencies of `pdf-extract`.

## Handoff Notes

- Sibling lane `claude/ion-provider-neutral-20260912` owns `src/ion_repl/chat.rs` and
  `src/llm_backends/**`; this lane never touched either. `CONFIRMATION_REQUIRED_TOOLS` (defined in
  `chat.rs`, read but not edited) already only lists `bash_exec`/`file_write`, so `memory_search`/
  `genome_read` land ungated automatically with no coordination needed.
- `lane-memory-adr0020` (Stage 6, memory promotion) proposed `memory_search` read a new
  `.impulse/GENOME_PROJECTION.md`; declined here as out of scope (see Decisions) and left to that
  lane's own PR against `src/tooling/builtin/memory_search.rs`.
- Not attempted: regenerating the stale, already-drifted `impulse-rs/.impulse/impulse-capabilities.json`
  (dated 2026-03-27, missing many tools added since including `document_read` itself) — pre-existing
  drift unrelated to this lane's deletion of `document_extract`; flagged separately rather than
  folded into this diff.
