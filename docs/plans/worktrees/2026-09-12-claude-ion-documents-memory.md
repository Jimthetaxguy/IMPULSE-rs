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

## Review round 2 (2026-09-12)

A second adversarial pass (fixtures at
`/private/tmp/claude-501/-Users-jamespustorino-code-IMPULSE-rs/c575264d-f5e1-49ac-b2c6-0834ed929caf/scratchpad/review54-r2/`)
CONFIRMED isolation itself (self-referencing fixture -> child signal 6, parent alive at 37 MB;
process group reaped including a grandchild; stdin null; env exactly `HOME`/`PATH`/`TMPDIR`;
`current_exe()` works via symlink/PATH/relative invocation; a legitimate 600-page, 9.9 MB PDF
renders in 0.68s at 54 MB child RSS) plus the round-1 encryption fixtures and sandbox defaults,
and all round-1 nits. Three round-1 fixes were REFUTED and are corrected here; one missing test
(P3) was added.

### P1 REFUTED (d): `child.wait_with_output()` is unbounded — a rogue child drove the PARENT to 3.2 GB RSS

`run_pdf_extraction_child` (the code that reads the isolated child's output) used
`child.wait_with_output()`, which buffers the WHOLE child stdout/stderr with no cap at all —
process isolation bounds what a CRASHING child can do to the parent, but says nothing about a
child that stays alive and just produces too much output. The reviewer's own rogue-child fixture
streaming ~1 GiB of valid-looking JSON drove the PARENT process to 3.23 GB RSS and produced an
*accepted* 1,073,741,825-character "document" — isolation alone does not bound a well-behaved-
looking but overproducing child.

**Fix:** `read_capped` (stdout — refuses and reports `exceeded` once more than the JSON-encoded
worst case for the requested `max_chars`/`max_pages` would arrive, via `AsyncReadExt::take(cap +
1)`, the same pattern `daemon::read_bounded_line` already uses) and `read_capped_tail` (stderr —
bounded to 64 KiB, keeping the tail via a drain-on-overflow loop, since diagnostic text has no
fixed budget to violate and always succeeds with whatever fits). On `exceeded`, the whole isolated
process group is killed immediately (`ProcessGroupGuard::kill_now`) rather than waiting for the
child to finish producing arbitrarily more.

**A genuine second bug found while building the fix, not by the reviewer:** the first
implementation read stdout and stderr via `tokio::join!` (wait for both to resolve). The rogue-
child integration test then hung for the FULL 30s wall-clock timeout instead of returning
promptly: once `read_capped` stops draining stdout past its cap, the child (still alive, blocked
on its next stdout `write()` because the pipe is full and nobody is reading past the cap) never
closes ANY of its pipes, including stderr — so `tokio::join!`, which needs both futures to resolve
before continuing, waited on a stderr read that could only ever complete once the child exited,
which only happens once we kill it, which only happens once `tokio::join!` returns. Fixed by
reading each pipe on an INDEPENDENT `tokio::spawn`ed task: the stdout task's result (exceeded or
not) is awaited and acted on first, aborting the stderr task before killing the child if exceeded,
rather than requiring both to resolve together.

**Test:** `tests/fakes/rogue-stdout-shim.sh` — NOT a production `MOCK_MODE` branch, a standalone
shell shim (mirroring the existing `tests/fakes/ion-verify-stub-gate*.sh` precedent) that ignores
every argument and streams far more than any reasonable computed cap. Pointed at as `exe` directly
in `tests/pdf_extraction_isolation.rs`'s
`test_extract_pdf_refuses_a_rogue_child_that_exceeds_the_stdout_bound`, bypassing PDF parsing
entirely to exercise the bounded-read code path in isolation. **Measured after the fix:** the
integration test process's own peak RSS while running this test is **9.3 MB** (down from the
unfixed parent's reported 3.2 GB), completing in 0.01s.

### P2 REFUTED: `BoundedSink` bounds output volume and wall clock, not memory — a compression bomb reached 5.55–7.0 GB regardless of `max_chars`

The round-1 fix's own doc comment claimed "the primary bound is still the sink" — false.
`pdf-extract`'s own internal decompression of a `FlateDecode` stream's content happens lazily,
deep inside `output_doc_page`, well BEFORE `BoundedSink`'s first write-time check could ever run.
The reviewer's `textbomb2g.pdf` (4.2 MB) inflates 476:1 before that first write and peaked at 5.55
GB in the child / 6.44 GB total; passing `--max-chars 1000` made no difference (1.92 GB peak) —
the character budget was never the bound that mattered, and `RLIMIT_AS` is not kernel-enforced on
macOS.

**Fix:** `preflight_pdf_streams` — the PDF analogue of `preflight_container`'s zip-container check
— iterates every `FlateDecode`-filtered stream object in the loaded `Document` and inflates it
through `inflate_bounded` (a streaming `flate2::read::ZlibDecoder`, discarding each chunk
immediately after counting it, so peak memory is the decoder's own window plus one 64 KiB read
buffer, never proportional to how much the stream would actually inflate to), refusing with a
typed error once a per-stream cap or the combined total exceeds 64 MiB (reusing
`MAX_DECOMPRESSED_BYTES`'s value, a new `MAX_PDF_DECOMPRESSED_BYTES`/
`MAX_PDF_STREAM_DECOMPRESSED_BYTES` pair since the PDF and zip-container checks are independent
mechanisms). Runs before any page renders, in BOTH the parent's cheap pre-check (`precheck_pdf`)
and the child's own independent, authoritative check (`internal_pdf_text::extract`) — new direct
dependency `flate2` (already transitively present via `lopdf`/`zip`/`image`; `cargo audit`
unchanged, zero new advisories). Only `FlateDecode` is checked — documented as a limitation, not a
silent gap: it is the PDF ecosystem's dominant filter and the one this attack class actually uses;
`LZWDecode`/`ASCII85Decode`/`RunLengthDecode` cannot reach comparable ratios, and `DCTDecode` is
JPEG data `PlainTextOutput` never reads.

**Measured peak RSS after the fix** (this checkout's release build, `/usr/bin/time -l`, invoking
`internal-pdf-text` directly against the reviewer's own fixtures):

| Fixture | Before (reviewer's numbers) | After (measured this round) |
|---|---|---|
| `textbomb500.pdf` (1.05 MB) | 2.41 GB / 48.8s | **13.1 MB / 0.56s** |
| `textbomb2g.pdf` (4.2 MB) | 7.0 GB / 178.5s | **22.2 MB / 0.03s** |
| `legit10m.pdf` (9.9 MB, legitimate, 600 pages) | 54 MB / 0.68s (round 1 baseline) | **51.2 MB / 0.73s** (no regression) |

Both bombs land well within the reviewer's "target: tens of MB" and are refused before any page
rendering begins; the legitimate document's output is unchanged (verified: real extracted text in
the JSON output).

**Doc corrections:** `tool_document.rs:1144-1148`'s "the primary bound is still the sink" claim
replaced with a corrected comment naming `preflight_pdf_streams` as the actual memory bound (three
layers now stated explicitly: sink bounds output/time, preflight bounds memory, isolation bounds
blast radius); `internal_pdf_text.rs`'s module doc gained the same distinction; the spec's PDF
section and its new "Review round 2" section state it explicitly.

### P2 REFUTED: the `/Encrypt` raw scan is bypassed by a legal name hex-escape

`/Encr#79pt` (ISO 32000-1 §7.3.5: `#79` = hex 0x79 = ASCII `y`) has no literal `/Encrypt` byte
sequence anywhere in the file but decodes to the identical name `Encrypt` — `lopdf` parses it,
decrypts with the empty user password, and `document_read` returned the plaintext. Verified
against the reviewer's own `enc_emptyuser_hexname.pdf` fixture directly: before the fix,
`internal-pdf-text` against this file exits 0 with real text; after, it is refused identically to
the literal-bytes case.

**Fix (both halves the review asked for):**
1. `pdf_declares_encryption` gained a two-tier scan: the fast literal-bytes check first (the
   common case, one pass, no allocation), then, only if that finds nothing, a fallback
   (`decode_pdf_name_token`) that walks every `/` in the file, decodes that name token's `#XX`
   escapes, and compares the decoded bytes against `Encrypt`. Bounded by `MAX_DOCUMENT_BYTES` like
   every other check here.
2. Belt and braces: after `Document::load`, both `precheck_pdf` (parent) and
   `internal_pdf_text::extract` (child) additionally re-assert directly against the PARSED
   trailer dictionary (`doc.trailer.get(b"Encrypt")`) that no `Encrypt` key exists — a
   structurally independent path from the byte scan. `lopdf`'s auto-decrypt-on-load transparently
   decrypts object CONTENTS but does not remove the trailer's own `/Encrypt` reference, so this
   check catches the same case even if the byte scan somehow had a further gap.

**The false-positive inconsistency the review flagged was verified empirically, not just noted:**
ran both `encrypt_in_content.pdf` (literal `/Encrypt` as page text in an UNCOMPRESSED content
stream) and `encrypt_in_compressed_content.pdf` (identical text, `FlateDecode`-compressed) through
`internal-pdf-text` directly. The first is refused (false positive — the scan cannot distinguish
page text from a real trailer key when reading raw file bytes); the second parses normally (the
compressed on-disk bytes never contain the literal string). Documented in the spec as accepted,
not fixed further: a false positive fails closed, the safe direction, and closing the gap would
require decompressing every stream before the encryption scan can run — the opposite ordering
`preflight_pdf_streams` deliberately assumes is not yet safe to do.

### P3: missing regression test for an explicit out-of-sandbox `impulse_dir`

The round-1 tests for `memory_search`/`genome_read`'s sandbox defaults called `tool.execute(...)`
directly, bypassing `ToolRegistry::execute`'s `validate_paths` step entirely — they proved the
ctx-default SELECTION logic, not that an out-of-sandbox EXPLICIT value is actually denied.  Added
three new tests in `tool_bridge.rs`, through the real `DynamicToolBridge::run` ->
`ctx.sandbox_tool_context()` -> `ToolRegistry::execute` -> `validate_paths` path: an explicit
out-of-sandbox `impulse_dir` is refused for both tools, and (positive control) the identical path
granted via `/allow` succeeds — proving the refusal is about the sandbox specifically, not merely
"any path outside `repo_root` fails."

### Gate evidence (review round 2, this checkout)

```
cd impulse-rs
cargo build --workspace                                    # clean
cargo test --workspace                                     # 2036 passed / 0 failed / 5 ignored
                                                             # (lib) + all integration binaries
                                                             # green, including 11/11 in
                                                             # tests/pdf_extraction_isolation.rs
cargo clippy --workspace --all-targets -- -D warnings       # clean
cargo fmt --all -- --check                                  # clean
cargo build --no-default-features                           # clean, zero warnings
cargo test --no-default-features --lib -- ion_repl::registry  # 5/5 passed
cargo audit                                                 # unchanged: 11 pre-existing
                                                             # vulnerabilities / 18 warnings;
                                                             # flate2 (the one new direct
                                                             # dependency this round) not flagged
                                                             # by name; Cargo.lock diff is a
                                                             # single added line
python3 docs/validate_docs.py --all                         # only pre-existing failures on main
```

Isolated re-run of `tests/pdf_extraction_isolation.rs` (11 tests, including the new rogue-shim
test): 11/11 passed, 1.75s standalone / 0.05s inside the full `cargo test --workspace` run above.

One `clippy::byte_char_slices` warning was caught and fixed mid-round (a test's
`[b')', b'>', ...]` array literal rewritten as the byte-string form clippy suggested,
`*b")>]}/% \t\n\r"`) before the final clean gate above.

Full lib total across all three review rounds combined: 2036 passed, 0 failed, 5 ignored (net +19
from round 2's 2017, matching the 19 new tests added this round: 16 in `tool_document.rs`
covering `decode_pdf_name_token`/hex-escape detection/`inflate_bounded`/`stream_uses_flate_decode`/
`preflight_pdf_streams`/`read_capped`/`read_capped_tail`/`max_child_stdout_bytes`, plus 3 P3
sandbox-denial tests in `tool_bridge.rs`).

## Review round 3 (2026-09-12)

Coordinator relayed a third adversarial pass against PR #54 at `ea9c7ea`: round-2's five fixture
measurements reproduced, `read_capped`/the deadlock fix held (5x rogue shim, no zombies), encryption
failed closed on every variant, sandbox tests were real, `flate2` audit was unchanged -- **but the
memory bound was refuted twice more**, plus a false-refusal and an over-tight cap. Fixtures supplied
read-only at `review54-r3/fix/` in this session's scratchpad; reproduced against them directly below
in addition to new tracked fixtures added to this repo (`tests/pdf_extraction_isolation.rs`,
`src/handlers/internal_pdf_text.rs`'s own test module) so the regression coverage stays portable
across fresh clones, linked worktrees, and CI, per this project's verification-gate requirement.

### F0 P0 (CONFIRMED): the parent still parsed PDF structure — `precheck_pdf` called `Document::load` before the preflight ever ran

`precheck_pdf` (`tool_document.rs`, now deleted) called `pdf_extract::Document::load` IN THE PARENT
to get a cheap page count, ahead of `preflight_pdf_streams`. `lopdf::Document::load` eagerly,
unconditionally decompresses every `/Type /ObjStm` object stream as part of loading — with no hook
to intercept or bound it — and stores the inflated bytes back into the object with `/Filter`
stripped. The preflight, which only ever runs AFTER `load` returns, therefore counted zero for this
class of stream: the memory had already been spent in the PARENT by the time any check could run.

Reproduced on the reviewer's `objstm_bomb2g.pdf` (2.04 MB on disk, a Catalog/Pages/Page/Font all
packed into one ObjStm padded with a ~2000 MiB ignored comment): the review reported parent RSS
2,078 MB, returning "OK".

**Fix, structural, not a patch:** the parent's entire PDF-specific job before spawning the child is
now `pdf_encryption_prescan` — a raw byte scan for `/Encrypt`, nothing else (`tool_document.rs`).
`Document::load`, the trailer-based encryption re-check, `preflight_pdf_streams` (moved and
extended, see F1 below), the page-count cap, and rendering are now exclusively the child's job
(`handlers::internal_pdf_text::extract`), which is authoritative for all of it. The parent never
parses attacker-controlled PDF structure again, full stop.

### F2 MEDIUM (CONFIRMED): `RLIMIT_AS` is silently a no-op on macOS — the child had no real memory bound on the primary platform

Even confined to the child, `Document::load`'s eager ObjStm inflation cannot be pre-counted by any
preflight — there is no hook between "lopdf decides to inflate an object stream" and "lopdf has
already inflated it." The existing defense-in-depth layer, `RLIMIT_AS` (set via `pre_exec` in
`run_pdf_extraction_child`), turned out to be accepted by macOS's `setrlimit` but silently NOT
kernel-enforced there — the review measured a child reaching 4.12 GiB RSS despite a 1 GiB limit.

**Fix:** a memory-watchdog background thread (`handlers::internal_pdf_text::spawn_memory_watchdog`
/`current_peak_rss_bytes`), started at the very top of `extract()` before any PDF parsing at all
(including `Document::load`), polling this process's own peak RSS via `libc::getrusage` roughly
every 10ms and self-terminating via `std::process::exit(PDF_MEMORY_CEILING_EXIT_CODE = 137)` the
instant it crosses `PDF_CHILD_MEMORY_LIMIT_BYTES` (kept at 1 GiB, now justified in its doc comment
against the new 512 MiB total-decompression cap plus JSON-stdout/baseline overhead). `getrusage`'s
`ru_maxrss` unit inconsistency (BYTES on macOS/BSD, KILOBYTES on Linux) is normalized in
`current_peak_rss_bytes`. `run_pdf_extraction_child` checks this specific exit code BEFORE its
generic unix-signal check, mapping it to a typed "exceeded its memory ceiling" error rather than
folding it into a generic parse failure. `RLIMIT_AS` stays as real, kernel-enforced defense-in-depth
on Linux; a `tracing::debug!` (outside the `pre_exec` closure, which cannot safely log) now states
the macOS limitation explicitly at spawn time. **Isolation on macOS is honestly "crash containment
plus the watchdog," not "RLIMIT_AS" — documented as such, not implied otherwise.**

Both `current_peak_rss_bytes` (the `unsafe` `libc::getrusage` call) and `spawn_memory_watchdog` have
dedicated unit tests (`internal_pdf_text.rs`) exercising the real code path, not just precondition
checks, per this project's unsafe-code policy.

### F1 P1 (CONFIRMED): the preflight only ever counted `FlateDecode` — an LZW-filtered bomb reached 4+ GiB while reporting "OK"

`preflight_pdf_streams` checked a stream's `/Filter` for the literal name `FlateDecode` and skipped
everything else — but `lopdf` decodes `/LZWDecode` exactly as readily. Reproduced on the reviewer's
`lzwmulti300.pdf` (1.65 MB on disk): the review measured 4.12 GiB child RSS while the preflight
still reported "OK".

**Fix:** the preflight (moved into `internal_pdf_text.rs`, see F0) now walks each stream's FULL
filter chain via `Stream::filters()` — lopdf's public, decode-ordered accessor — rather than a
bare-name check. A terminal `FlateDecode` or `LZWDecode` stage is bound-counted through a streaming
decoder (`inflate_bounded_zlib`, and new `inflate_bounded_lzw` via
`weezl::decode::Decoder::with_tiff_size_switch(BitOrder::Msb, 8)`, matching `lopdf`'s own internal
LZW parameters exactly). Any filter chain with something AFTER a terminal Flate/LZW stage, or any
filter this preflight cannot bound-count at all (`RunLengthDecode`, `DCTDecode`, `JPXDecode`,
`CCITTFaxDecode`, `Crypt`, or an unrecognized name), is refused BY NAME — deny-by-default, not
silently skipped the way every non-`FlateDecode` filter was before this round.

### F3 (found and fixed alongside F1): `[/ASCII85Decode /FlateDecode]` chains were falsely refused

A direct consequence of doing the chain-walk properly: before this fix, a legitimate document using
an `[/ASCII85Decode /FlateDecode]` filter chain was refused with "corrupt deflate stream", because
the still-ASCII85-ENCODED outer bytes were fed straight to zlib. Fixed by fully decoding an outer
`ASCII85Decode`/`ASCIIHexDecode` layer first (new `decode_ascii85_bounded`/`decode_asciihex_bounded`
— lopdf's own `decode_ascii85` is a private associated fn, so this is a from-scratch, bounded
reimplementation of the same Adobe ASCII85 algorithm lopdf itself uses) and feeding the RESULT to
the terminal Flate/LZW bounded decoder.

### F4 (CONFIRMED): the 64 MiB TOTAL cap refused ordinary documents, and the round-2 "legitimacy" fixture proved nothing

The round-2 fix set BOTH the per-stream and total decompression caps to 64 MiB. The review found a
900-page, 2.87 MB Flate-compressed text PDF inflates to 73.6 MB, over the old TOTAL cap — while
`legit10m.pdf` (round 2's own "this still works" fixture) has NO compressed streams at all, so it
never actually exercised this cap either way.

**Fix:** per-stream cap stays 64 MiB (`MAX_PDF_STREAM_DECOMPRESSED_BYTES`); total cap rises to 512
MiB (`MAX_PDF_TOTAL_DECOMPRESSED_BYTES`), justified in its doc comment against `MAX_DOCUMENT_BYTES`
(10 MiB source-file cap: 512 MiB is a ~51x expansion ceiling on the largest input this tool ever
reads) and the separate, independent `MAX_EXTRACTED_CHARS` budget (16M characters of FINAL text,
capped regardless of how much raw decompressed PDF-operator bytes it took to render that much). A
new, purpose-built fixture (`write_legit_multi_page_flate_pdf`, 25 genuinely Flate-compressed pages)
proves the raised cap actually admits large real documents.

The reviewer's own `legit64.pdf` was also run against the real tool: it PANICS during rendering
(`pdf_extract::get_name_string`, "deref") — a pre-existing bug in its own Python generator script
(`/F1`'s font reference resolves to the Pages object, not a Font, from a duplicate-assignment typo
in `gen_r3.py`), unrelated to this fix and outside this lane's scope to patch in a third-party
crate. Isolation still contained it cleanly: child exit 101 (a plain Rust panic unwind, not a
signal), the parent reported a typed "could not be parsed" error, ~34 MB peak RSS, no memory
blowup, no parent impact whatsoever. Not a security regression — the new purpose-built fixture
above proves F4's actual claim (the raised cap admits real documents) instead.

### F5 (doc corrections, no code change)

Two round-2 doc numbers were re-measured and corrected, not changed in behavior:
- The production stdout cap for the DEFAULT budget (16,000,000 chars / 4,096 pages) is exactly
  **128,266,240 bytes** (`max_child_stdout_bytes`'s formula, derived from `MAX_EXTRACTED_CHARS`, not
  from whatever `max_chars` a caller happens to pass) — this was already correct in code, just
  understated in prose.
- Parent RSS on a rogue child (`tests/fakes/rogue-stdout-shim.sh`) through the DEFAULT budget (not
  an artificially small one) measures **~139 MB** — re-measured directly this round (see RSS table
  below), not the ~9.3 MB a smaller-budget-only measurement previously implied. Both numbers are
  real; they were measuring different budgets. ~139 MB is what a production caller actually
  experiences at the default budget, and is the expected, correct size of the stdout-cap buffer at
  that budget (~128 MB) plus baseline process overhead — not a bug.
- On macOS, isolation is crash containment plus the watchdog, not `RLIMIT_AS` (see F2).

### New test seam: injectable memory-watchdog ceiling

`internal-pdf-text` gained a hidden `--memory-limit-bytes` flag (`Option<u64>`, defaulting to the
production `PDF_CHILD_MEMORY_LIMIT_BYTES` when omitted), and `tool_document.rs` gained
`extract_pdf_with_exe_timeout_and_memory_limit` (mirroring the existing exe/timeout injection
pattern `extract_pdf_with_exe_and_timeout` already used). This lets
`tests/pdf_extraction_isolation.rs` prove the watchdog fires END TO END — through the real parent
exit-code mapping, not just the unit-level `spawn_memory_watchdog` test — using a small (4 MB
padded) ObjStm fixture against a small (1 MB) injected ceiling, rather than needing a genuinely
gigabyte-scale bomb in the default test suite. `RLIMIT_AS` is deliberately NOT affected by this
override (it stays hardcoded to the production constant in `pre_exec`), so a test using a low
injected ceiling exercises the watchdog specifically, never `RLIMIT_AS`.

### RSS/behavior table (review round 3, this checkout, debug build, `/usr/bin/time -l`, macOS)

All reviewer fixtures read directly from `review54-r3/fix/` (read-only scratchpad), run through the
real `internal-pdf-text` child at PRODUCTION defaults (`--max-chars 16000000 --max-pages 4096`,
default 1 GiB memory ceiling) unless noted:

| Fixture | On disk | Result | Peak RSS | Notes |
|---|---|---|---|---|
| `objstm_bomb2g.pdf` | 2.04 MB | refused, exit 137 | ~1.08 GB | watchdog fires (F0/F2); review's original unbounded parent RSS was 2,078 MB — now the PARENT never touches this file's structure at all |
| `lzwmulti300.pdf` | 1.65 MB | refused ("inflates to more than 536870912 bytes combined") | ~25 MB | 512 MiB TOTAL cap catches it (F1); review's original was 4.12 GiB |
| `legit64.pdf` | 2.87 MB | panics in `pdf_extract` (pre-existing fixture-generator bug, unrelated to this fix) | ~34 MB | isolation contains it cleanly: child exit 101, parent typed error, no blowup (F4) |
| `textbomb500.pdf` | 1.05 MB | refused | ~22 MB | round-1/2 regression, still holds |
| `textbomb2g.pdf` | 4.20 MB | refused | ~32 MB | round-1/2 regression, still holds |
| `legit10m.pdf` | 9.91 MB | succeeds (8.93 MB text) | ~63 MB, 7.84s | no compressed streams; proves nothing about the decompression caps either way (unchanged from round 2) |
| `chain_a85_flate.pdf` (bomb) | 1.38 MB | refused | ~25 MB | F1/F3 chain-walk catches an ASCII85+Flate bomb |
| `chain_a85_flate_legit.pdf` | 695 B | succeeds | ~20 MB | F3 fix, reviewer's own legit chain fixture |
| `chain_ahx_flate.pdf` (bomb) | 2.20 MB | refused | ~27 MB | ASCIIHexDecode+Flate chain, same fix |
| `chain_ahx_flate_legit.pdf` | 757 B | succeeds | ~20 MB | ASCIIHexDecode+Flate chain, same fix |
| `lzwbomb.pdf` (round-2 scale) | 1.51 MB | refused ("could not decode ... invalid code") | ~24 MB | still safely denied; the reviewer's own hand-rolled Python LZW encoder produces a stream even `lopdf`'s real (also `weezl`-based) decoder would not decode either — deny-by-default fails closed regardless of the specific reason |
| `rlbomb.pdf` | 401 KB | refused ("uses filter 'RunLengthDecode'") | ~20 MB | deny-by-default names the filter (F3) |
| `predictor.pdf` | 9.8 KB | succeeds | ~31 MB | Predictor-filtered legitimate stream unaffected |
| rogue shim, DEFAULT budget | n/a | refused ("exceeded its output bound") | **~139 MB** | F5: the correct number at the default budget (~128 MB computed stdout cap + baseline), not ~9.3 MB |
| rogue shim, small (1000-char) budget | n/a | refused | ~9 MB | the small-budget case round 2 measured; both are correct, for their own budgets |

Tracked-fixture coverage (portable, CI-safe, no gigabyte-scale allocations) for the same findings:
`tests/pdf_extraction_isolation.rs` gained
`test_extract_pdf_refuses_an_objstm_bomb_via_the_memory_watchdog`,
`test_extract_pdf_objstm_document_succeeds_under_a_generous_memory_limit`,
`test_extract_pdf_refuses_an_lzw_bomb_end_to_end`,
`test_extract_pdf_ascii85_plus_flate_legitimate_chain_extracts_successfully`, and
`test_extract_pdf_large_legitimate_flate_document_clears_the_new_512mib_cap` (16 tests total in this
file now, up from 11); `src/handlers/internal_pdf_text.rs`'s own test module gained the moved and
extended `preflight_pdf_streams`/decoder unit tests plus the watchdog tests (21 tests total in that
file now, previously untested at the unit level since the preflight lived in `tool_document.rs`).

### Gate evidence (review round 3, this checkout)

```
cd impulse-rs
cargo build --workspace                                    # clean
cargo test --workspace                                     # 2664 passed / 0 failed / 9 ignored
                                                             # across every crate; impulse-rs lib
                                                             # alone: 2046 passed / 0 failed / 5
                                                             # ignored; tests/pdf_extraction_
                                                             # isolation.rs: 16/16 passed
cargo clippy --workspace --all-targets -- -D warnings       # clean
cargo fmt --all -- --check                                  # clean
cargo build --no-default-features                           # clean, zero warnings
cargo test --no-default-features --lib                      # 1914 passed / 1 failed / 5 ignored --
                                                             # the 1 failure
                                                             # (daemon::tests::tests::
                                                             # test_plugin_registry_initialized_
                                                             # after_init) reproduces IDENTICALLY
                                                             # on the pre-round-3 baseline (ea9c7ea)
                                                             # with this branch's changes stashed;
                                                             # pre-existing, unrelated to this lane
                                                             # (a --no-default-features feature-
                                                             # gating gap in a daemon plugin-
                                                             # registry test, outside src/ion_repl
                                                             # and src/handlers/internal_pdf_text)
cargo audit                                                 # unchanged: 11 pre-existing
                                                             # vulnerabilities / 18 warnings on both
                                                             # this branch and the pre-round-3
                                                             # baseline; weezl (the one new direct
                                                             # dependency this round) not flagged by
                                                             # name; Cargo.lock diff is a single
                                                             # added line, matching the flate2
                                                             # precedent from round 2
```

Isolated re-run of `tests/pdf_extraction_isolation.rs` (16 tests, 5 new this round): 16/16 passed in
7.06-7.66s across repeated runs (the two new ~80 MB-scale fixtures -- the real-cap LZW bomb and the
25-page legitimate document -- account for most of the wall time; no fixture in this file needs
gigabyte-scale allocation, per this project's portability requirement for tracked test fixtures).

Full lib total across all four review rounds combined: 2046 passed, 0 failed, 5 ignored (net +10
from round 2/3's 2036: `internal_pdf_text.rs`'s own module now has 21 tests total, up from its
pre-round-3 baseline of 4 (the original `BoundedSink` tests, unchanged) -- +17 net, covering the
moved-and-extended preflight/decoder/watchdog tests; `tool_document.rs` lost 10 tests exercising
`precheck_pdf`/`preflight_pdf_streams`/`inflate_bounded`/`stream_uses_flate_decode`, which no longer
exist in that module (moved to `internal_pdf_text.rs` above), and gained 3 new
`pdf_encryption_prescan` tests in their place -- -7 net. +17 and -7 nets to +10).

## Review round 4 (2026-09-12)

Coordinator relayed round-4 verification of PR #54 at `3a3679e`: F0 (parent peak 4.7 MB on the
ObjStm bomb, never parses PDF structure), F2 (watchdog fires at 1 GiB + 4.4% slack, its exit code
137 distinguishable from a `SIGKILL`, watchdog started before `Document::load`), F3, F5, encryption,
caps, a 100-page/206 MB Flate control document, and a zero-conflict merge with `main` all
CONFIRMED, against 197 lib tests plus 16 isolation tests. Three new findings, all usability/evidence
rather than security holes in the fix itself, plus a required merge with `main` (which had landed
#53 and #55 since this branch was cut) before the final gate.

**Merge with `main`:** `git merge origin/main --no-edit` -- clean, auto-merged (`ort` strategy), one
conflict-shaped hunk in `CONTEXT.md` (both branches added unrelated sections) resolved
automatically by git with no manual intervention needed. Post-merge `cargo build --workspace`
verified clean before continuing.

### NEW-1 HIGH (CONFIRMED): deny-by-default refused 10.4% of a real-PDF sample over filters that cannot inflate at all

Round 3's `stream_filter_chain_inflated_bytes` refused ANY filter it could not bound-count, by
design (deny-by-default). The review ran this against 221 real PDFs from the reviewing machine: 23
(10.4%) were refused SOLELY for `DCTDecode` (a JPEG-compressed image embedded alongside ordinary
text), and every scanned PDF (`CCITTFaxDecode`/`JBIG2Decode`) was refused outright, with no way to
read even the pages that DID carry a text layer.

The key insight the fix relies on: a filter `lopdf` itself cannot decode is, by construction, also
one `pdf-extract` never reads through at all -- `PlainTextOutput` has no image-handling code path,
and `Stream::decompressed_content` itself returns `Unimplemented` for anything outside
`Flate`/`LZW`/`ASCII85`. Such a filter therefore cannot inflate the way a real decompressor can; it
was being refused for a danger it structurally cannot pose.

**Fix (`stream_filter_chain_inflated_bytes`, `internal_pdf_text.rs`):** a filter this preflight
cannot decode now counts `stream.content.len()` (the still-compressed, on-disk size, already
bounded by `MAX_DOCUMENT_BYTES`) and the chain walk ends there -- UNLESS a `FlateDecode`/`LZWDecode`
stage appears LATER in the same chain, in which case it is still refused by name, deny-by-default:
an opaque filter feeding a real decompression stage is exactly the case this preflight cannot
safely trust blindly. New tests: `test_preflight_pdf_streams_accepts_a_standalone_undecodable_
filter_review_round_4_new1` (unit, `internal_pdf_text.rs`) and
`test_preflight_pdf_streams_refuses_an_undecodable_filter_preceding_flate_deny_by_default` (the
kept refusal case) plus, per the review's explicit ask, an end-to-end integration test
(`test_extract_pdf_with_dct_image_still_extracts_its_text`,
`tests/pdf_extraction_isolation.rs`) building a real fixture with BOTH a `DCTDecode`-filtered image
XObject stream AND a genuine text content stream, proving the text extracts successfully despite
the image stream sitting alongside it in the document. Reproduced against the review's own
`rlbomb.pdf` (`RunLengthDecode` as the sole filter): previously refused, now succeeds (exit 0).

### NEW-2 MEDIUM (CONFIRMED): the preflight was stricter than the decoder it bounds

`inflate_bounded_zlib` errored on the FIRST zlib read failure and propagated that as a document-level
refusal. `lopdf::Stream::decompress_zlib` (the real decoder `pdf-extract` actually uses) is more
lenient: `read_to_end` keeps whatever decoded successfully before a read error, and only when
NOTHING decoded at all does it retry as raw deflate (skipping the 2-byte zlib header) before giving
up and returning empty output -- it never fails the whole document over one stream its own real
decoder would also have partially or fully recovered from. The review found 2/221 real PDFs refused
with "corrupt deflate stream" that `lopdf` tolerates. Separately, `[/FlateDecode /FlateDecode]`
(double-Flate compression, a real if uncommon legal PDF construction) was refused outright as "a
filter after FlateDecode", with no way to count it even though it is exactly as bound-countable as a
single stage, just twice.

**Fix:** `inflate_bounded_zlib` is now backed by a new bound-MATERIALIZING
`inflate_bounded_zlib_to_vec` (via a shared `read_bounded` helper) that mirrors `lopdf`'s own
zlib-then-raw-deflate-fallback tolerance exactly, and treats a stream that still fails as
contributing 0 bytes (it cannot inflate to anything if neither this preflight nor `lopdf`'s own
decoder can get a single byte out of it) rather than failing the document. `stream_filter_chain_
inflated_bytes` now recognizes `[..., FlateDecode, FlateDecode]` as the one non-terminal exception:
the first stage is bound-materialized (not merely counted) and fed into a second bounded decode
pass. New unit tests: `test_inflate_bounded_zlib_tolerates_a_truncated_stream_review_round_4_new2`,
`test_inflate_bounded_zlib_returns_zero_for_completely_undecodable_bytes`,
`test_preflight_pdf_streams_allows_a_legitimate_double_flate_chain_review_round_4_new2`, and
`test_preflight_pdf_streams_refuses_a_double_flate_bomb_review_round_4_new2` (the double-Flate BOMB
variant still refuses correctly).

**Tradeoff, disclosed rather than hidden:** switching `inflate_bounded_zlib` from discard-while-
counting to bound-materialize-then-measure (needed to make the double-Flate chain's first stage's
real bytes available to the second decode pass) raises peak RSS for a REFUSED bomb from ~22-32 MB
(round 3) to ~90-97 MB (round 4, re-measured against the review's own `textbomb500.pdf`/
`textbomb2g.pdf`/`chain_a85_flate.pdf`/`chain_ahx_flate.pdf` fixtures below) -- still trivially
bounded by the 64 MiB per-stream cap plus baseline process overhead, and nowhere near the 1 GiB
watchdog ceiling or the 512 MiB total-decompression cap, but a real, non-zero cost worth stating
plainly rather than letting the round-3 numbers stand uncorrected.

### NEW-3 MEDIUM (CONFIRMED, evidence gap): the F4 regression fixture was 8x under the cap it was meant to prove

`test_extract_pdf_large_legitimate_flate_document_clears_the_new_512mib_cap` (25 pages x 4000 lines
of real text) inflates to only 7.76 MiB combined -- 8x UNDER the OLD 64 MiB cap. It would have
PASSED against the pre-F4 code just as well as the post-F4 code, so it proved nothing about the cap
change it exists to regression-test; its doc comment's "well over the OLD 64 MiB total cap" claim
was simply wrong arithmetic (25 x 4000 x ~80 bytes/line = 8,000,000 bytes = 7.63 MiB, not
"well over 64 MiB").

**Fix, both halves of the review's suggestion:** (1) `write_legit_multi_page_flate_pdf` was
redesigned to pad each page's content stream with a PDF COMMENT (`% PPP...`, ignored by every
conformant content-stream tokenizer, `pdf-extract` included) rather than real text -- this decouples
"decompressed byte count" from "extracted character count," which real text padding cannot do
(padding far enough past 64 MiB in real TEXT would also blow well past the independent 16-million-
character extraction budget, refusing the fixture for an unrelated reason and proving nothing about
the cap under test). The resized fixture (5 pages x 15 MiB padding = 75 MiB combined, each page's
own 15 MiB safely under the 64 MiB PER-STREAM cap so the fixture specifically exercises the TOTAL
cap) now genuinely clears the OLD 64 MiB cap while staying under the new 512 MiB one, and the test
itself asserts both bounds explicitly rather than trusting a comment's arithmetic again. (2) A new,
faster, independent unit test (`test_preflight_pdf_streams_with_caps_accepts_a_real_multi_stream_
document_over_64mib_review_round_4_new3`, `internal_pdf_text.rs`) proves the identical claim
directly against `preflight_pdf_streams_with_caps` at the REAL production caps, without needing to
render any pages through `pdf_extract` at all (a `Document` needs no page tree for this function,
which only ever walks `doc.objects.values()`) -- a second, structurally independent confirmation
that the 512 MiB cap change works as intended.

### RSS/behavior table (review round 4, re-measured, this checkout, debug build, `/usr/bin/time -l`, macOS)

Same reviewer fixtures as round 3 (`review54-r3/fix/`), re-run at PRODUCTION defaults after the
NEW-1/NEW-2 fixes:

| Fixture | Result | Peak RSS (round 3) | Peak RSS (round 4) | Notes |
|---|---|---|---|---|
| `objstm_bomb2g.pdf` | refused, exit 137 | ~1.08 GB | ~1.12 GB | unchanged mechanism (watchdog); within normal run-to-run variance |
| `lzwmulti300.pdf` | refused (512 MiB total cap) | ~25 MB | ~25 MB | unaffected by NEW-1/NEW-2 (terminal LZWDecode, no chain) |
| `legit64.pdf` | panics (pre-existing fixture-generator bug) | ~34 MB | ~34 MB | unchanged, unrelated to this lane |
| `textbomb500.pdf` | refused | ~22 MB | ~92 MB | NEW-2 tradeoff: materializing to the 64 MiB per-stream cap before measuring, vs. discard-while-counting |
| `textbomb2g.pdf` | refused | ~32 MB | ~97 MB | same NEW-2 tradeoff |
| `legit10m.pdf` | succeeds | ~63 MB | ~63 MB | no compressed streams, unaffected |
| `chain_a85_flate.pdf` (bomb) | refused | ~25 MB | ~94 MB | same NEW-2 tradeoff (ASCII85+Flate chain materializes the Flate stage) |
| `chain_a85_flate_legit.pdf` | succeeds | ~20 MB | ~20 MB | unaffected |
| `chain_ahx_flate.pdf` (bomb) | refused | ~27 MB | ~96 MB | same NEW-2 tradeoff |
| `chain_ahx_flate_legit.pdf` | succeeds | ~20 MB | ~20 MB | unaffected |
| `lzwbomb.pdf` | refused (LZW decode error) | ~24 MB | ~24 MB | unaffected -- NEW-2 only changed zlib's error tolerance, not LZW's |
| `rlbomb.pdf` (`RunLengthDecode`) | **now succeeds** (was refused) | n/a (refused) | ~21 MB | **NEW-1 in effect**: a standalone opaque filter is no longer refused |
| `predictor.pdf` | succeeds | ~31 MB | ~35 MB | unaffected, normal run-to-run variance |

All numbers stay far under every relevant ceiling (64 MiB per-stream, 512 MiB total, 1 GiB
watchdog) -- the round-3-to-4 RSS increases are a disclosed, bounded tradeoff (see NEW-2 above), not
a regression toward unbounded behavior.

### Gate evidence (review round 4, this checkout)

```
cd impulse-rs
git merge origin/main --no-edit                            # clean, auto-merged, no conflicts
cargo build --workspace                                    # clean
cargo test --workspace                                     # 2794 passed / 0 failed / 9 ignored
                                                             # across every crate; impulse-rs lib
                                                             # alone: 2161 passed / 0 failed / 5
                                                             # ignored; tests/pdf_extraction_
                                                             # isolation.rs: 17/17 passed
cargo clippy --workspace --all-targets -- -D warnings       # clean
cargo fmt --all -- --check                                  # clean
cargo build --no-default-features                           # clean, zero warnings
cargo test --no-default-features --lib                      # 2023 passed / 1 failed / 5 ignored --
                                                             # the 1 failure
                                                             # (daemon::tests::tests::
                                                             # test_plugin_registry_initialized_
                                                             # after_init) is the same pre-existing,
                                                             # unrelated failure confirmed in round 3
                                                             # (reproduces identically on the
                                                             # pre-round-3 baseline ea9c7ea)
cargo audit                                                 # unchanged: 11 pre-existing
                                                             # vulnerabilities / 18 warnings, same as
                                                             # before this round's changes and the
                                                             # pre-round-3 baseline
```

Isolated re-run of `tests/pdf_extraction_isolation.rs` (17 tests, 1 new this round --
`test_extract_pdf_with_dct_image_still_extracts_its_text`): 17/17 passed in 4.33-4.52s.

Full lib total for this round: 2161 passed, 0 failed, 5 ignored on this checkout (post-merge with
`main`, which itself added tests independent of this lane's PDF work -- not a like-for-like
comparison against round 3's 2046, since `main`'s #53/#55 merge landed its own new tests in between;
`internal_pdf_text.rs`'s own module alone went from 21 tests (round 3) to 27 (round 4, +6: two
zlib-tolerance tests, two double-Flate tests, and two NEW-1 standalone/precedes-Flate tests).

**Second merge with `main` (post-fix, same round):** between pushing the round-4 fixes above and
this final gate, `main` advanced twice more (#56 memory promotion/ADR-0020, #59 property-based/fuzz
parser harnesses), which the GitHub PR view surfaced as `mergeable: CONFLICTING` after the first
push -- not caused by anything in this lane, but by `main` moving again while the coordinator's
round-4 message was in flight. `git merge origin/main --no-edit` reproduced exactly one real
conflict, in `impulse-rs/Cargo.lock` (a mechanical lockfile conflict from #59's new `proptest`/
fuzz-harness dependencies landing in the same region `weezl`'s round-3 addition touched) --
`CONTEXT.md`, `Cargo.toml`, and `tool_document.rs` all auto-merged cleanly with no manual
intervention. Resolved by taking `main`'s `Cargo.lock` (`git checkout --theirs`) and letting
`cargo build` patch in the one entry this lane's `Cargo.toml` needs (`weezl`, already present as a
transitive dependency via `lopdf` in both parents, so no new download) -- a 92-line net diff, not a
disruptive full `cargo generate-lockfile` regeneration (which was tried first and produced an
~1,095-line diff reordering unrelated entries; reverted in favor of the minimal patch). Full gate
re-run clean after this second merge (see totals below); no files this lane owns were touched by
either `main` merge beyond `Cargo.lock`/`Cargo.toml`.

### Gate evidence (post-second-merge, this checkout, final)

```
cd impulse-rs
git merge origin/main --no-edit                            # 1 real conflict (Cargo.lock, resolved
                                                             # via checkout --theirs + cargo build);
                                                             # CONTEXT.md/Cargo.toml/tool_document.rs
                                                             # auto-merged clean
cargo build --workspace                                    # clean
cargo test --workspace                                     # 2928 passed / 0 failed / 9 ignored
                                                             # across every crate; impulse-rs lib
                                                             # alone: 2272 passed / 0 failed / 5
                                                             # ignored; tests/pdf_extraction_
                                                             # isolation.rs: 17/17 passed
cargo clippy --workspace --all-targets -- -D warnings       # clean
cargo fmt --all -- --check                                  # clean
cargo build --no-default-features                           # clean, zero warnings
cargo test --no-default-features --lib                      # 2113 passed / 1 failed / 5 ignored --
                                                             # the same pre-existing, unrelated
                                                             # daemon::tests::tests::
                                                             # test_plugin_registry_initialized_
                                                             # after_init failure confirmed in
                                                             # rounds 3 and 4
cargo audit                                                 # unchanged: 11 pre-existing
                                                             # vulnerabilities / 18 warnings
python3 docs/validate_docs.py --all                         # only pre-existing failures on main
```
