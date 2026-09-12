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
- The stale `impulse-rs/.impulse/impulse-capabilities.json` (dated 2026-03-27) was regenerated for
  real in review round 1 via `impulse-rs tooling-reload` (see that section below), not hand-edited.

## Review round 1 (2026-09-12)

Adversarial review of PR #54 (probe crate, measured in release mode; fixtures at
`/private/tmp/claude-501/-Users-jamespustorino-code-IMPULSE-rs/c575264d-f5e1-49ac-b2c6-0834ed929caf/scratchpad/review54/`)
returned "needs changes": two P0s in the PDF path, two P2s, and several nits. txt/md, the sandbox
check before open, the page-tree hardening (cycles, 50k-deep `Kids`, 180k objects), rule 9, and the
audit claim all held and needed no changes. Everything below is fixed on this branch.

### P0-1 (CONFIRMED): self-referencing Form XObject stack-overflow-aborts the whole `ion` process

A 677-byte PDF whose Form XObject content stream draws itself (`/X0 Do` inside `/X0`'s own content
stream) makes `pdf_extract::output_doc_page`'s content-stream interpreter recurse until the
thread's stack is exhausted — a Rust stack-overflow guard-page hit, which calls `abort()`, not an
unwinding panic. `spawn_blocking`'s `JoinError` containment (what every other `document_read` kind
still relies on) only catches unwinding panics, so this killed the entire `ion` process from an
ungated tool.

**Fix (structural, not a pre-scan):** PDF page rendering moved into a hidden `internal-pdf-text`
subcommand (`#[command(hide = true)]` in `cli.rs`; `src/handlers/internal_pdf_text.rs`; shared by
both `impulse-rs` and `ion` binaries), spawned by
`ion_repl::tool_document::run_pdf_extraction_child` via `tokio::process::Command` with
`kill_on_drop(true)`, `process_group(0)` + a synchronous-from-Drop `ProcessGroupGuard` (mirroring
`agent::ImpulseAgent::harness_query_structured`'s established pattern exactly), a 30s wall-clock
timeout, env scrubbed via the shared `tooling::env_scrub`, stdin closed, and (unix, `pre_exec`)
`RLIMIT_AS`/`RLIMIT_CPU` set via the `libc` crate — already a direct workspace dependency
(`[target.'cfg(unix)'.dependencies] libc = "0.2"`; verified with `cargo tree -i libc`, no
Cargo.toml/Cargo.lock change needed). The child's crash, abort, OOM, or hang becomes a typed
`document_read` error naming the cause (signal number, non-zero exit with the child's own stderr,
or a timeout) — never a parent crash.

**Test (integration, real subprocess, `tests/pdf_extraction_isolation.rs`):**
`test_extract_pdf_self_referencing_xobject_crashes_only_the_child_not_this_process` builds the
self-referencing-XObject fixture with `lopdf` directly, asserts the call returns a typed `Err`
naming a signal, then runs a follow-up async sleep AND a second, independent extraction to
completion — proving the test process itself (not just "some process") is still alive and the
tokio runtime is healthy. This must be an integration test, not a lib unit test: calling the
crashing path directly in-process, even inside one `#[test]`, would abort the whole `cargo test`
binary, not just fail one test — isolation is exactly what makes it safe to test at all, and can
only be exercised from outside the process it protects.

### P0-2 (CONFIRMED): unbounded per-page sink reached 7 GB RSS on a small crafted PDF

The previous per-page sink rendered a whole page into an unbounded `String` before checking it
against the character budget. Reviewer numbers: a 1.05 MB PDF reached 2.41 GB RSS / 48.8s; a 4.2 MB
one reached 7.0 GB / 178.5s, straddling the 180s loop-contract timeout, which cannot cancel
`spawn_blocking`. The existing budget fixture used a tiny page, so it passed with or without the
bug — a false sense of coverage.

**Fix:** the child's sink (`internal_pdf_text::BoundedSink`, a `std::fmt::Write` impl) refuses a
write the instant the RUNNING total across every page processed so far (seeded from the previous
page's final count, never reset) would exceed the budget — checked per write during rendering
(`PlainTextOutput::output_character` calls `write!` once per glyph plus spacing), not once per page
after the fact. This is what makes "check-before-push, same discipline as
`extract_workbook`/`extract_word`" actually true for PDF; it was not before. `RLIMIT_AS` (1 GiB
default) is a second, best-effort layer on top (not fully kernel-enforced on macOS; real
enforcement is Linux) — the sink is the primary bound.

**Tests:** `internal_pdf_text::tests` unit-tests `BoundedSink` directly (no PDF/subprocess at all):
accepts writes up to exactly the cap; refuses the write that would exceed it *without growing the
buffer at all* (asserted on `buf.chars().count()`/`count`, not just an error string, per the
review's explicit ask); refuses a single 10 MB write (mirroring the textbomb fixture's one giant
`Tj` string) without buffering any of it; and proves the seeded `count` carries the cumulative
total across pages, not a per-page-reset one. The integration suite's
`test_extract_pdf_over_budget_page_is_refused_promptly` additionally asserts wall-clock elapsed
time stays well under 10s for an over-budget page, proving the refusal is prompt (check-before-push)
rather than build-then-check.

### P2-1 (CONFIRMED): `doc.is_encrypted()` is unreliable — `lopdf::Document::load` silently decrypts an empty-user-password PDF

`lopdf::Document::load` (`authenticate_and_setup_encryption` internally) unconditionally tries an
*empty* user password first and silently decrypts on success — a common real-world case
(owner-password-only protection leaves the user password empty so any reader can open it) — so
`doc.is_encrypted()` after loading can report `false` for a PDF that is, in fact, encrypted. The
previous refusal relied on exactly that call.

**Fix:** `pdf_declares_encryption` — a raw byte scan for the literal `/Encrypt` token, run against
the file's bytes BEFORE any parser (`lopdf::Document::load` included) ever sees them, in both the
parent's cheap pre-check (`precheck_pdf`) and the child's own independent, authoritative check. A
false positive (the literal bytes appearing as ordinary page text) fails closed, the safe
direction; a false negative would require a spec-violating writer, since `/Encrypt` in the
trailer/XRef-stream dictionary is never itself inside a compressed object stream.

**Tests:** unit tests on the pure byte-scan function plus real fixtures (`write_encrypted_pdf`
with `user_password: ""` in the integration suite —
`test_extract_pdf_refuses_encryption_even_with_an_empty_user_password` — and a non-empty-password
variant, mirroring the review's `enc_emptyuser.pdf`/`enc_userpw.pdf`).

### Item 3: native `ReplTool` wall clocks

The child timeout (30s, `PDF_CHILD_TIMEOUT_SECS`) covers `pdf`. `docx`/`xlsx` streaming does not
get an equivalent timer: both are already a bounded function of `MAX_DOCUMENT_BYTES`/
`MAX_DECOMPRESSED_BYTES` (source file and inflated container capped before any parser runs), and
neither has a code path that can loop or recurse on attacker-controlled structure the way a PDF's
Form XObjects can — there is no unbounded-recursion class of bug available to them the way P0-1
found for PDF. Documented in `tool_document.rs`'s `PDF_CHILD_TIMEOUT_SECS` doc comment and the spec.

### P2-2/P2-3 (CONFIRMED): `memory_search`/`genome_read`'s `impulse_dir` default was neither `IMPULSE_HOME`-aware nor sandbox-checked

Both tools defaulted an omitted `impulse_dir` to the bare literal `".impulse"`, resolved relative
to the process's own working directory — not `IMPULSE_HOME`/`$HOME/.impulse`, and unrelated to
`repo_root`. Worse, `src/tooling/executor.rs`'s generic `validate_paths` only checks parameters a
caller actually supplied, so an OMITTED `impulse_dir` was invisible to the sandbox check entirely
— outside the impulse-rs repo the tools silently returned nothing (wrong directory), and pointing
them at the real home via `/allow` still didn't help since the default itself never resolved there.

**Fix:** `ReplContext::sandbox_tool_context` now sets `ToolContext.impulse_dir` from
`history::impulse_home()` (the same `IMPULSE_HOME`/`$HOME/.impulse` resolution `.impulse/ion_history`
itself uses) and adds it to the read roots explicitly (deduplicated against `repo_root` when they
coincide) — it is Impulse's own state directory, not arbitrary host filesystem, so granting it read
access is not a sandbox widening. `memory_search`/`genome_read`'s `execute()` methods now default
from `ctx.impulse_dir` instead of the bare literal; an explicitly-supplied `impulse_dir` is
unchanged (still checked by the pre-existing `validate_paths` path). `registry.rs`'s doc comment,
which originally claimed "no new sandboxing code needed," was corrected in place rather than
silently rewritten.

**Note on scope:** `src/tooling/builtin/{memory_search,genome_read}.rs` were not in this lane's
original owned-paths list; this fix required editing them directly (the default lives in each
tool's own `execute()`). Flagged here explicitly since it is a deliberate, reviewer-directed
expansion of scope, not a silent one.

**Tests:** `ion_repl::tests::test_sandbox_tool_context_limits_write_to_repo_root_and_extends_reads_with_allow_grants`
updated (env-dependent `impulse_home()` now asserted by membership/order, not a hardcoded literal
list) plus a new dedup test; `memory_search`/`genome_read` each gain a "defaults from ctx when
omitted" test and an "explicit value still overrides ctx" test.

### Nits

- `markdown_sections` now tracks fenced code blocks (` ``` `/`~~~`, up to 3 leading spaces) so a
  `#`-looking line inside a fence is never treated as a heading, and `parse_atx_heading` now
  tolerates up to 3 leading spaces before the `#`s, both per CommonMark's actual rules.
- The `internal_pdf_text` child iterates `lopdf::Document::get_pages()`'s own map keys rather than
  assuming a contiguous `1..=N` range.
- `tool_document.rs`'s stale "absolute paths are accepted" module doc (undersold the sandbox
  constraint that already applied via `resolve_document_path_with_cap`) was corrected.
- `impulse-rs/.impulse/impulse-capabilities.json` was regenerated for real via the existing, narrow
  `impulse-rs tooling-reload` CLI command (validates external manifests + rewrites the manifest
  from the live `ToolRegistry`; touches nothing else — deliberately NOT `impulse-rs init`, which
  would also overwrite `GENOME.md`/`config.json`/`LIVE_STATE.json`). Diff: `document_extract`
  removed; unrelated pre-existing drift (`bash_exec`/`file_write` had never been captured in the
  committed snapshot either) also resolved as a side effect.
- The spec (`docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`) now states annotation/
  `AcroForm` text is never extracted (only a page's own `/Contents` stream is rendered) and carries
  a full "Review round 1" section plus updated fixture table.

### Also addressed in this fix round (routed via the coordinator, not from the PDF probe)

- **`src/ion_repl/mod.rs`'s `MissingApiKey` notice hard-coded `ANTHROPIC_API_KEY`** regardless of
  the actually-configured provider (`IMPULSE_PROVIDER=openai`/`minimax`), a handoff from sibling PR
  #55 (`claude/ion-provider-neutral-20260912`, which owns `chat.rs`/`llm_backends` and could not
  land the one-line fix itself since `mod.rs` is this lane's owned path). Fixed: `respond()`'s
  `MissingApiKey { provider }` arm now interpolates the actual provider via a new
  `missing_api_key_notice(provider: &str)` function. Tests: the existing
  `test_respond_chat_turn_missing_api_key_prints_graceful_notice_not_panic` updated, plus a new
  `test_missing_api_key_notice_names_the_actual_provider_not_a_hardcoded_anthropic` covering the
  `openai`/`minimax` cases explicitly (per the coordinator's request). This closes PR #55's handoff.

### Gate evidence (review round 1, this checkout)

```
cd impulse-rs
cargo build --workspace                                    # clean
cargo test --workspace                                     # 2017 passed / 0 failed / 5 ignored (lib)
                                                             # + all integration binaries green,
                                                             # including 10/10 in the new
                                                             # tests/pdf_extraction_isolation.rs
cargo clippy --workspace --all-targets -- -D warnings       # clean
cargo fmt --all -- --check                                  # clean
cargo build --no-default-features                           # clean, zero warnings
cargo test --no-default-features --lib -- ion_repl::registry  # 5/5 passed
cargo audit                                                 # unchanged: 11 pre-existing
                                                             # vulnerabilities / 18 warnings, zero
                                                             # attributable to libc (already a
                                                             # direct dep) or any crate touched
                                                             # this round
python3 docs/validate_docs.py --all                         # only pre-existing failures on main
```

Full lib total across both review rounds combined: 2017 passed, 0 failed, 5 ignored (net +14 from
round 1's 2003, reflecting new tests added minus the 6 old PDF fixture tests that moved to the
integration suite because they now require a real compiled binary).
