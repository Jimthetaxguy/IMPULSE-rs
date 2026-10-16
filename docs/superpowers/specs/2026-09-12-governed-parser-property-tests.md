---
title: Governed Parser Property Tests
description: Property-based and fuzz-style test harnesses over the parsers adversarial review kept re-breaking in the two weeks before 2026-09-12, their oracles, and every finding
updated: 2026-09-12
type: specification
category: testing
phase: all
status: active
audience: builders
tags: [spec, proptest, fuzz, governed, ion, parser, security]
---

# Governed Parser Property Tests

Work card: `docs/plans/worktrees/2026-09-12-claude-governed-fuzz-harness.md`.

## Why

Between 2026-08-27 and 2026-09-12, three adversarial reviews
(`docs/plans/worktrees/2026-09-01-claude-ion-document-tool.md`,
`2026-09-02-claude-ion-tool-floor.md`, `2026-09-12-claude-adr0019-p1-fixes.md`)
each found real bugs in hand-written example-based tests over the same shape
of code: a byte/text parser with an adversarial input space (Git config
syntax, porcelain status records, shell command text, OOXML container
formats). Example tests prove the cases someone thought of; they do not
prove the invariant. This lane adds `proptest` (new `[dev-dependencies]`
entry, `impulse-rs/Cargo.toml`) and a property-test module next to each
target function, so the invariant itself is checked over a wide generated
input space, with a differential oracle (real `git`) wherever one exists.

**Update (2026-09-12, PR review round 2):** this branch was rebased onto
`main` after `#53`'s round-3 commit made the config-include-chain parser
byte-wise (`config_include_paths`/`join_continued_lines`/`config_value`/
`expand_config_path` now take/return `&[u8]`/`Vec<u8>`, and
`config_include_paths` returns `Result`, for a real correctness reason:
the previous UTF-8 decode silently skipped every include directive in a
file containing one non-UTF-8 byte anywhere). This lane's tests were
adapted to the new signatures with no invariant weakened; full detail in
the lane card's "PR review round 2" section. Invariant 1e below (the
`Result`-is-`Err`-only-for-the-documented-case property) is new as of that
adaptation.

## Scope: what was NOT built

Per the assignment, this lane owns **only** new test modules, the
`proptest` dev-dependency line (+`Cargo.lock`), this spec, and the lane
card. No production code in `governed_producers.rs`, `chat.rs`,
`tool_document.rs`, or `tooling/traits.rs` was modified. Where a property
test surfaced a real discrepancy between documented and actual behavior
(one case, see Findings below), the test was written to pin the *actual*
(correct, and in that case safer) behavior, and the discrepancy is reported
rather than silently fixed.

Three deliberate scope-narrowing decisions, each named at its test site:

1. **Config-include differential oracle covers `[include]`, not
   `includeIf`.** `includeIf` condition evaluation (`gitdir:`, `onbranch:`,
   `hasconfig:`) is its own complex grammar Git evaluates internally;
   replicating it in the test harness would be testing the oracle, not the
   code. Instead, `config_include_treats_includeif_like_include_regardless_of_condition`
   asserts the code's own documented choice directly (conditions are never
   evaluated -- an `includeIf` pins its target unconditionally), which is a
   *documented divergence* from Git's own behavior, not a gap.
2. **`~/`-prefixed include paths are not covered by the differential
   harness.** Making the harness's own process and the code-under-test agree
   on `$HOME` needs a global env-var mutation, which is unsafe to do inside
   a `proptest!` body that may run cases from more than one thread. Existing
   example tests already cover `~/` expansion directly.
3. **`extract_word`'s property generator uses single-level tables.** A
   `w:tbl` nested inside a `w:tc` does not produce its own line -- every
   paragraph inside the nested table's cells flattens into the SAME
   `cell`/`row` buffer as the containing cell, because `cell_depth`/
   `row_depth` are shared counters incremented on every `w:tr`/`w:tc`
   regardless of nesting and only flush at the outermost close. Modeling
   that flattening generically in the oracle is a materially different (and
   separately valuable) piece of work; the required depth-8 nesting is
   reached instead via `w:tbl>w:tr>w:tc>mc:AlternateContent>mc:Choice>w:p>w:r>w:t`,
   and one hand-built, pinned regression test
   (`extract_word_nested_table_flattens_into_the_containing_row`) locks down
   the documented flattening behavior directly.

`window()`'s `max_chars == 0` boundary is deliberately excluded from the
budget-respecting property tests (which start at `max_chars = 1`) and
pinned as its own deterministic characteristic test instead -- see
"Findings" below.

## Running the suite

```bash
cd impulse-rs
export CARGO_TARGET_DIR=<isolated dir>   # shared-target-dir hazard, see MEMORY.md
cargo test --lib -- proptests            # default 256 cases/test (proptest's own default)
PROPTEST_CASES=2000 cargo test --lib -- proptests   # deeper local run
```

`PROPTEST_CASES` is proptest's own environment override, honored by every
`proptest! {}` block below with no extra plumbing. Two blocks pin an
explicit lower case count instead of following `PROPTEST_CASES` --
`config_include_closure_matches_git_show_origin` and
`read_head_oid_without_git_matches_git_rev_parse_for_generated_shapes` (both
`ProptestConfig::with_cases(...)`, real `git` subprocess per case) and
`extract_word_never_panics_and_preserves_documented_invariants`/
`extract_word_never_panics_on_arbitrary_document_xml_bytes`/
`extract_word_never_panics_on_arbitrary_whole_file_bytes` (real zip
encode/decode per case) -- kept low so `cargo test --lib -- proptests`
stays a fast, no-`PROPTEST_CASES`-needed sanity gate; raise the cases
argument directly in source for a deeper one-off local run of those
specific tests.

## Invariant table

| # | Target | File | Invariant | Oracle |
|---|---|---|---|---|
| 1a | `config_include_paths`, `join_continued_lines`, `config_value`, `expand_config_path` | `governed_producers.rs` | Never panic on arbitrary bytes/text | none (panic-safety) |
| 1b | `join_continued_lines` | `governed_producers.rs` | A comment line never continues, and never absorbs a pending continuation; an even (escaped) trailing-backslash count never continues | direct (generated mixed `Plain`/`Continuing`/`EscapedBackslash`/`Comment` line sequences) |
| 1c | `config_include_paths` (BFS closure) | `governed_producers.rs` | For generated `[include]` trees (relative / nested-relative / absolute / quoted / backslash-continued paths, each leaf non-empty), the include closure equals the set of `file:` origins `git config --file <root> --list --show-origin --includes` reports | **real `git`**, not skipped |
| 1d | `config_include_paths` | `governed_producers.rs` | `includeIf` sections pin their target regardless of the condition text (documented divergence from Git, not a gap) | direct |
| 1e | `config_include_paths` | `governed_producers.rs` | On unix, `Result` is never `Err` for arbitrary bytes -- the one documented failure case (a non-UTF-8 include path where paths are not bytes) is compiled out under `#[cfg(unix)]`, provably rather than merely unfalsified | direct, `#[cfg(unix)]`-gated |
| 2a | `status_contains_subject_change`, `is_untracked_impulse_runtime_artifact` | `governed_producers.rs` | Never panic on arbitrary `-z` bytes | none |
| 2b | `status_contains_subject_change` | `governed_producers.rs` | Any non-`??` record (including a rename's second, no-XY-prefix record) is always a change | direct |
| 2c | `status_contains_subject_change` | `governed_producers.rs` | An untracked path is exempt iff it is one of the exact named `.impulse/...` spellings or a documented `.tmp.`/`worktrees/` prefix family -- root-anchored: `.impulse/MEMORY_CANDIDATES.json.evil` and `x/.impulse/...` are never exempt | direct + cross-check against `is_untracked_impulse_runtime_artifact` over generated mixed-record streams including two-record renames |
| 3a | `read_head_oid_without_git` | `governed_producers.rs` | Never panics; agrees with `git rev-parse --verify HEAD` on success/failure and on the OID value, across loose, packed-only, loose+packed, detached, and symbolic-to-missing ref shapes | **real `git`** |
| 3b | `validate_oid` | `governed_producers.rs` | Accepts iff length in {40, 64} and every byte is `[0-9a-f]` | regex-shaped oracle |
| 3c | `validate_path_segment` (`impulse-ops`) | `governed_producers.rs` (calls the `impulse_ops` function directly) | Accepts iff 1-128 ASCII `[a-zA-Z0-9_-]` chars, not `.`/`..`, not leading `.` | regex-shaped oracle |
| 4a | `split_shell_tokens` | `ion_repl/chat.rs` | Never panics; a token glued to a `SHELL_METACHARS` character with no whitespace still splits; no empty tokens | direct |
| 4b | `bash_command_escape_candidates` | `ion_repl/chat.rs` | Never panics; flags any bare absolute-path token, any token containing `..`/`~`/`$HOME`/`${HOME}`, and any `cd` target resolving outside the sandbox root; does NOT flag an in-sandbox `cd` | direct, against a real `ToolContext` sandbox root |
| 4c | `bash_command_escape_candidates` | `ion_repl/chat.rs` | Documented miss (`H=/etc; cat $H/passwd`, variable indirection) stays a miss | direct, see Findings for the OTHER documented-miss claim that did NOT hold |
| 5a | `wrap_untrusted_tool_output` | `ion_repl/chat.rs` | For arbitrary content (including forged header/footer-shaped text), exactly one real header and one real footer sharing an 8-hex-char nonce, content preserved verbatim between them | direct (structural parse-back) |
| 5b | `wrap_untrusted_tool_output` | `ion_repl/chat.rs` | Two calls (even with identical content) use different nonces | direct |
| 6a | `extract_word` | `ion_repl/tool_document.rs` | Never panics on a generated well-formed docx grammar (depth 8 via `w:tbl>w:tr>w:tc>mc:AlternateContent>mc:Choice>w:p>w:r>w:t`); `sum(section.chars) == total_chars`; deleted/instruction text never appears; `mc:Fallback` never appears, `mc:Choice` always does; `w:ins` text always appears; a table row's line has exactly `cells-1` tabs | direct (generator is its own oracle: every visible/hidden marker is asserted present/absent by construction) |
| 6b | `extract_word` | `ion_repl/tool_document.rs` | A nested `w:tbl` flattens into the containing row's single line rather than producing its own | pinned hand-built regression |
| 6c | `extract_word` | `ion_repl/tool_document.rs` | Never panics on arbitrary bytes as the `word/document.xml` entry, or as the whole file; always `Result`, never a panic | none |
| 6d | `WordTextBuilder` | `ion_repl/tool_document.rs` | Never panics; drops whitespace-only lines; a fitting non-blank line becomes exactly one output line with `chars()` growing by `line.chars().count() + 1`; section table capped at `MAX_WORD_SECTIONS`; `check_pending` and `push_line` agree on refusal | direct |
| 6e | `SheetBodyBuilder` | `ion_repl/tool_document.rs` | Never panics; a dense row-major grid renders as tab-separated rows matching a hand-computed expectation, with `chars` matching the rendered text's own length | direct |
| 6f | `window` | `ion_repl/tool_document.rs` | Never panics; `returned_chars <= max_chars` and matches content length; `next_offset` is `Some` iff truncated and, when so, `== start + returned` and `< total`; truncation ends on `\n` or is a documented hard-cut; an offset past the end yields an empty complete window | direct |
| 6g | `fold_case` | `ion_repl/tool_document.rs` | Never panics; idempotent; matches `str::to_lowercase` for plain ASCII letters | direct |
| 7a | `secure_resolve` / `ToolContext::is_path_allowed` | `tooling/traits.rs` | Never panics on arbitrary path strings, with or without configured roots | none |
| 7b | `is_path_allowed` | `tooling/traits.rs` | Deterministic (same input, same verdict) | direct |
| 7c | `is_path_allowed` | `tooling/traits.rs` | Empty roots mean unrestricted (documented) | direct |
| 7d | `is_path_allowed` | `tooling/traits.rs` | `true` implies the resolved candidate starts with a resolved root, over a real filesystem tree with `..`-escape depth varied | direct |
| 7e | `is_path_allowed` | `tooling/traits.rs` | A trailing slash never changes the verdict for an existing in-sandbox directory | direct |
| 7f | `is_path_allowed` | `tooling/traits.rs` | A `..`-traversal to a not-yet-created file outside the root is denied (the `secure_resolve` not-yet-created-target fallback path) | direct |
| 7g | `is_path_allowed` | `tooling/traits.rs` | Never panics or hangs on a real two-node symlink loop or a self-referential symlink | direct |
| loop | `error_signature` | `loop_contract.rs` | Never panics; bounded by `ERROR_SIGNATURE_MAX_CHARS`; selects the first (trimmed) line containing an alphanumeric character, falling back to the whole trimmed content, capped | direct |

## Findings

Two findings came out of writing these tests. Neither required a production
fix under this lane's ownership; both are pinned as regression tests so a
future change that regresses the *actual* (documented-or-not) behavior is
caught.

### Finding 1 -- stale "known inherent miss" doc comment, `bash_command_escape_candidates` (P3, documentation only, not a security gap)

**File:** `impulse-rs/src/ion_repl/chat.rs`, doc comment on
`bash_command_escape_candidates` (~line 585, "Known inherent misses") and
the now-corrected test in the same file's `proptests` module.

The doc comment lists `python3 -c "open('/tmp/x')"` as an example the
heuristic cannot catch, reasoning that the path lives inside a nested
interpreter's own string literal. That reasoning is stale: `(` and `)` are
themselves entries in `SHELL_METACHARS`, so `split_shell_tokens` splits the
command at the parenthesis, isolating `'/tmp/x'` as its own token; `unquoted()`
then strips the surrounding single quotes, exposing a bare `/tmp/x` that
trips the `starts_with('/')` check. Verified directly:

```
tokens: ["python3", "-c", "\"open", "'/tmp/x'", "\""]
flagged: ["'/tmp/x'"]
```

**The command IS flagged today.** The tool is more protective than its own
comment claims, not less -- this is the opposite of a security gap. Left
uncorrected here because `ion_repl/**` is outside this lane's writable
paths (owned by the sibling ion-tool-floor lane); a regression test
(`bash_command_escape_candidates_flags_a_path_inside_nested_interpreter_string_via_paren_splitting`)
pins the correct, current behavior so a refactor that drops parens from
`SHELL_METACHARS` (which WOULD reopen this exact miss) is caught. The
owning lane should correct the doc comment's example to a case that is
still genuinely a miss, or drop the example. The second documented miss in
the same comment (`H=/etc; cat $H/passwd`, shell variable indirection) was
independently verified and does still hold.

### Finding 2 -- `window()`'s `max_chars == 0` never advances the offset (informational, not reachable through the tool)

**File:** `impulse-rs/src/ion_repl/tool_document.rs`, `window()` (~line 1220).

`window(text, offset, 0)` for `offset < total` always returns
`next_offset == Some(offset)` -- the exact offset it was given, unadvanced.
A caller that looped directly on `window` with `max_chars == 0` would never
terminate. This is not reachable through `document_read`: the request layer
already rejects a zero `max_chars` before `window` is ever called (per this
module's own decisions log, "`max_chars` above the cap is clamped, not
rejected; zero is rejected"). Recorded as a pinned characteristic test
(`window_max_chars_zero_never_advances_the_offset_below_total`) rather than
a bug, since `window` is a lower-level function whose only caller already
enforces the precondition -- flagged here so a future caller of `window`
that skips that enforcement inherits the finding, not a fresh investigation.

## Case-count and runtime notes

Full `cargo test --lib -- proptests` run (default cases, `PROPTEST_CASES`
unset): see the lane card's Verification table for the exact totals and
wall-clock time on the checkout this landed against. The heavier
differential/subprocess-per-case tests are individually capped (see
"Running the suite" above) specifically so the default run stays fast
enough to run on every `cargo test --lib` pass without a separate
integration-test opt-in flag.
