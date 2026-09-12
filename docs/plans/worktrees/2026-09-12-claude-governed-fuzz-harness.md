---
title: Governed Fuzz Harness
description: Work card for claude/governed-fuzz-harness-20260912 (property-based/fuzz test harnesses over the parsers the last two weeks of adversarial reviews kept breaking)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, proptest, fuzz, governed, ion, testing]
---

# Governed Fuzz Harness

## Lane Facts

- Owner: Claude (Fable 5.1).
- Role: implementation lane adding property-based/fuzz-style test harnesses
  over parsers that the last two weeks of adversarial reviews (2026-08-27
  through 2026-09-12) kept finding real bugs in via hand-written example
  tests: the shared Git config include-chain parser, the porcelain `-z`
  status classifier, the no-Git HEAD reader, the ion REPL shell-text
  heuristic and untrusted-output envelope, the docx/xlsx streaming
  extractors, and the tool sandbox path check.
- Branch: `claude/governed-fuzz-harness-20260912`, **stacked on PR #53's
  branch `claude/adr0019-p1-fixes-20260912` at `637afaf`** (the config-include
  parser this lane tests most heavily lives there, not on `main`).
- Worktree: `.worktrees/governed-fuzz-harness-20260912`.
- Owned paths:
  - New `#[cfg(test)] mod proptests { ... }` blocks in `impulse-rs/src/governed_producers.rs`,
    `impulse-rs/src/ion_repl/chat.rs`, `impulse-rs/src/ion_repl/tool_document.rs`,
    `impulse-rs/src/tooling/traits.rs`, `impulse-rs/src/loop_contract.rs`
  - One regression fix inside an existing test in `impulse-rs/src/ion_repl/chat.rs`
    (see Findings below -- test-only, no production code touched)
  - `impulse-rs/Cargo.toml` (`[dev-dependencies]` `proptest = "1"` line only)
    and `impulse-rs/Cargo.lock`
  - `impulse-rs/proptest-regressions/.gitkeep` (tracked placeholder so the
    directory exists in a fresh clone/CI; proptest writes minimized
    counterexamples here on a failing run, and per proptest's own
    convention they are committed, not ignored -- a fresh clone or CI run
    that can't see a previously discovered failing case would silently
    lose it)
  - `docs/superpowers/specs/2026-09-12-governed-parser-property-tests.md`
  - This work card
- Blocked/shared paths (not touched): any production code in
  `governed_producers.rs`, `impulse-ops/src/governed_task.rs`, `chat.rs`,
  `tool_document.rs`, `tooling/traits.rs`, or any other module; `.github/**`;
  `CLAUDE.md`; `AGENTS.md`.
- Plan/spec: `docs/superpowers/specs/2026-09-12-governed-parser-property-tests.md`
  (full invariant table, oracle per invariant, scope decisions, findings).
- Verification:
  ```
  cd impulse-rs
  export CARGO_TARGET_DIR=<isolated dir>
  cargo build --workspace && cargo test --workspace && \
    cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
  PROPTEST_CASES=2000 cargo test --lib -- proptests
  python3 ../docs/validate_docs.py --all
  ```

## Decisions

- 2026-09-12: **`proptest` cases follow the library's own `PROPTEST_CASES`
  env override**, not a hand-rolled `cases()` helper -- proptest already
  reads that variable for every `proptest! {}` block's default (256 cases),
  so no extra plumbing was needed; the assignment's suggested pattern
  (`PROPTEST_CASES=2000 cargo test --lib -- proptests`) works out of the
  box.
- 2026-09-12: **Differential tests against real `git` are not skipped** --
  git 2.50.1 is present in every environment this gate runs in, so
  `config_include_closure_matches_git_show_origin` and
  `read_head_oid_without_git_matches_git_rev_parse_for_generated_shapes` run
  unconditionally, each with its own lowered `ProptestConfig::with_cases`
  (48 and 40 respectively) to keep real subprocess spawns from dominating
  the default suite's wall-clock time.
- 2026-09-12: **The docx property generator uses single-level tables**, with
  one hand-built pinned regression test for nested-table flattening instead
  of a recursive oracle. See the spec's "Scope: what was NOT built" section
  for the full reasoning (nested tables flatten into the containing cell's
  buffer via shared, not per-depth, `cell_depth`/`row_depth` counters --
  modeling that generically is separately valuable work, not a gap in this
  lane).
- 2026-09-12: **Two "must never appear" marker strategies were split from
  "must appear" strategies** in the docx generator
  (`hidden_marker_strategy` vs `safe_text_strategy`, `HIDDEN_` prefix) after
  the first run of `extract_word_never_panics_and_preserves_documented_invariants`
  found a self-inflicted false failure: a cell's own visible text and its
  `mc:Fallback` marker were generated independently and could coincide
  (`text: "S"`, `fallback_marker: Some("S")`), making the "fallback must not
  appear" assertion fail for a reason that had nothing to do with the code
  under test. Not a production finding -- a test-generator bug, fixed
  before landing.

## Changes

- `governed_producers.rs`: 15 property tests covering
  `config_include_paths`/`join_continued_lines`/`config_value`/`expand_config_path`
  (panic safety, the comment/escaped-backslash continuation invariant, a
  differential closure test against real `git config --show-origin`, the
  `includeIf`-ignores-condition invariant, and -- since main's config parser
  is now byte-wise (`&[u8]` in, `config_include_paths` `Result`-typed out;
  see "PR review round 2" below) -- an explicit `Result`-is-`Err`-only-for-
  the-documented-case property), `status_contains_subject_change`/
  `is_untracked_impulse_runtime_artifact` (panic safety, tracked-record
  always-change, root-anchored untracked exemption including the two named
  attack shapes, a full cross-check against generated mixed record streams
  including two-record renames), `read_head_oid_without_git` (a differential
  test against real `git rev-parse HEAD` across five generated ref shapes),
  and `validate_oid`/`validate_path_segment` (regex-oracle equivalence, the
  latter calling `impulse_ops::governed_task::validate_path_segment`
  directly).
- `ion_repl/chat.rs`: 13 property/regression tests covering
  `split_shell_tokens` (panic safety, metacharacter-gluing split), the
  `bash_command_escape_candidates` flagging rules (absolute path, `..`/`~`/
  `$HOME`/`${HOME}`, `cd` in/out of sandbox, one documented miss confirmed,
  one documented miss corrected -- see Findings), and
  `wrap_untrusted_tool_output` (structural envelope parse-back: exactly one
  real header/footer pair per call sharing an 8-hex nonce, content preserved
  verbatim, two calls never share a nonce, a forged in-content footer never
  mistaken for the real one).
- `ion_repl/tool_document.rs`: 21 property/regression tests -- `fold_case`
  (panic safety, idempotence, ASCII agreement), `window` (panic safety,
  budget, `next_offset`/`truncated` agreement, line-boundary/hard-cut
  invariant, past-the-end behavior, a pinned `max_chars == 0` characteristic
  -- see Findings), `SheetBodyBuilder` (panic safety, dense-grid rendering),
  `extract_word` (a depth-8 docx grammar generator --
  `w:tbl>w:tr>w:tc>mc:AlternateContent>mc:Choice>w:p>w:r>w:t` -- proving
  `sum(section.chars) == total_chars`, deleted/instruction-text exclusion,
  `mc:Fallback` exclusion and `mc:Choice`/`w:ins` inclusion, and the
  tabs-only-between-cells invariant; a pinned nested-table-flattening
  regression; panic safety on arbitrary bytes as the `document.xml` entry
  and as the whole file), and `WordTextBuilder` (panic safety, whitespace-line
  dropping, exact one-line-per-fitting-input, the `MAX_WORD_SECTIONS` cap,
  `check_pending`/`push_line` agreement).
- `tooling/traits.rs`: 8 property tests covering `secure_resolve`/
  `ToolContext::is_path_allowed` -- panic safety, determinism, the
  empty-roots-means-unrestricted contract, "allowed implies resolved-under-
  resolved-root" over a real filesystem tree with varied `..`-escape depth,
  trailing-slash invariance, a not-yet-created traversal target denied, and
  panic/hang safety on a real two-node symlink loop and a self-referential
  symlink.
- `loop_contract.rs`: 3 property tests covering `error_signature` -- panic
  safety, the `ERROR_SIGNATURE_MAX_CHARS` bound, and the documented
  first-alnum-line-or-whole-trim selection rule.
- `Cargo.toml`/`Cargo.lock`: `proptest = "1"` added to `[dev-dependencies]`
  (first use in this workspace).
- `.gitignore`: the `proptest-regressions/` ignore rule was **removed**, not
  added -- see the 2026-09-12 PR-review correction below.

## Tests

60 new tests total (15 + 13 + 21 + 8 + 3, per file below; confirmed by
`cargo test --lib -- proptests --list`), all under `#[cfg(test)] mod
proptests` blocks alongside each target's existing `mod tests`. Full
per-file breakdown is in the "Invariant table" of the spec doc. Every
property test failed against a deliberately broken/reverted version of its
target invariant before this card was written (either by temporarily
weakening the assertion or, for the
two real findings below, by directly probing the current code) -- see
Findings for the two cases that surfaced something worth reporting.

## Findings

Both are written up in full in the spec doc's "Findings" section; summary
here for anyone scanning lane cards:

1. **P3, documentation only, not a security gap** --
   `bash_command_escape_candidates`'s doc comment
   (`impulse-rs/src/ion_repl/chat.rs`, ~line 585) claims
   `python3 -c "open('/tmp/x')"` is a "known inherent miss." It is not: `(`
   and `)` are `SHELL_METACHARS`, so the command splits there and
   `unquoted()` strips the surrounding quotes from `'/tmp/x'`, exposing a
   bare `/tmp/x` that trips the absolute-path check. Verified directly
   (tokens and flagged output both captured in the spec). The tool is
   safer than documented, not less safe. Not fixed here (`ion_repl/**` is
   outside this lane's writable paths, owned by the sibling
   ion-tool-floor lane); a regression test pins the correct current
   behavior instead. The comment's OTHER example (`H=/etc; cat $H/passwd`)
   was independently reverified and does still hold as a genuine miss.
2. **Informational, not reachable through the tool** -- `window()`
   (`impulse-rs/src/ion_repl/tool_document.rs`, ~line 1220) never advances
   `next_offset` when called with `max_chars == 0`; a caller that looped
   directly on `window` with that input would spin forever. Not reachable
   through `document_read` today (the request layer already rejects a zero
   `max_chars` before calling `window`), so recorded as a pinned
   characteristic test rather than a bug -- flagged in case a future caller
   of the raw `window()` function skips that precondition.

## PR review round 1 (2026-09-12, PR #57)

Two corrections landed after the PR opened, both docs/config only -- no
test or production code changed for either.

| Finding | Fix | Verified |
|---|---|---|
| Lane card's "## Tests" said "80 new tests"; `14+13+21+8+3=59` | Corrected to 59 in the lane card and the PR description | `cargo test --lib -- proptests --list` reports 59 |
| **P2 (Codex)** -- `.gitignore:10` ignored `proptest-regressions/`, so a fresh clone or CI would silently lose any minimized counterexample proptest discovers and writes there, contradicting proptest's own convention (commit the regression corpus, don't ignore it) | Removed the ignore rule entirely; `impulse-rs/proptest-regressions/.gitkeep` added so the (currently empty -- no discovered failing case is outstanding) directory is tracked and ready to receive real seed files as they're found | `cargo test --lib -- proptests` still 59 passed, 0 failed after the change (removing an ignore rule cannot itself change test behavior; re-run was to confirm nothing else shifted) |

Codex's finding is correct and this card's original `.gitignore` rationale
(treating the corpus as a disposable "local re-run aid") was the actual
error -- proptest's own docs recommend committing `proptest-regressions/`
precisely so a regression discovered once, anywhere, is never silently
re-lost by someone else's clone or by CI.

## PR review round 2 (2026-09-12, PR #59, rebase onto main)

The coordinator rebased this branch onto `main` (now `1f866d6`, containing
merged `#53` and `#55`) and reopened it as PR #59 (the original stacked PR
#57 was closed by GitHub when #53's branch was deleted after merge -- a
rebase side effect, not a review finding). CI on #59 failed to *compile*
the lib tests: 9 errors (`E0308` mismatched types, `E0277` "can't compare
`Vec<u8>` with `String`", `E0369` `prop_assert_eq` on
`Result<Vec<PathBuf>, anyhow::Error>`).

**Root cause:** `#53`'s round-3 commit (merged to `main`, landed under this
branch after the rebase) made the shared Git config include-chain parser
byte-wise, for a real correctness reason unrelated to this lane -- the
first version decoded the file as UTF-8 and skipped every include directive
in a file the moment one non-UTF-8 byte appeared anywhere in it (a Latin-1
byte in a comment or a user's name, say), silently leaving Git-honored
includes unpinned. The signatures this lane's tests called against changed
underneath them:

| Function | Before (what this lane's tests were written against) | After (`#53` round 3, on `main`) |
|---|---|---|
| `config_include_paths` | `fn(bytes: &[u8], base: &Path) -> Vec<PathBuf>` | `fn(bytes: &[u8], base: &Path) -> Result<Vec<PathBuf>>` |
| `join_continued_lines` | `fn(text: &str) -> Vec<String>` | `fn(bytes: &[u8]) -> Vec<Vec<u8>>` |
| `config_value` | `fn(raw: &str) -> Option<String>` | `fn(raw: &[u8]) -> Option<Vec<u8>>` |
| `expand_config_path` | `fn(value: &str, base: &Path) -> PathBuf` | `fn(value: &[u8], base: &Path) -> Result<PathBuf>` |

**Fix, in `governed_producers.rs`'s `mod proptests` only (no production
code touched, matching this lane's ownership):**
- Every string-strategy generator feeding these four functions switched to
  a byte-vector strategy (`proptest::collection::vec(any::<u8>(), ..)`) or
  `.as_bytes()` at the call site.
- `join_continued_lines_never_continues_past_a_comment`'s two invariant
  checks (comment lines survive verbatim; escaped-backslash lines never
  merge) now compare `&[u8]` slices instead of `&str`s.
- `our_include_closure` (the differential-oracle helper) and
  `config_include_treats_includeif_like_include_regardless_of_condition`
  both now `.expect(...)` the `Result` before using the `Vec<PathBuf>` --
  appropriate here because every input they feed the parser is a generated,
  well-formed config file, where the only documented failure mode (a
  non-UTF-8 include path on a non-unix target -- see
  `path_from_config_bytes`'s `#[cfg(not(unix))]` arm) cannot occur.
- **New invariant added**, per the fix instructions ("add a property that
  arbitrary bytes never panic and that the `Result` is `Err` only for the
  documented cases"): `config_include_paths_is_infallible_on_unix_for_arbitrary_bytes`
  (`#[cfg(unix)]`) asserts `config_include_paths(&bytes, base).is_ok()` for
  4096-byte arbitrary input. This is provably true, not merely
  unfalsified-by-this-generator: on unix, `path_from_config_bytes` (which
  `expand_config_path` is the only fallible caller of) always returns `Ok`
  because an `OsStr` *is* bytes there, so the one documented `Err` path is
  compiled out entirely under `#[cfg(unix)]` -- there is no byte sequence
  that can reach it on this platform. This lane's own arbitrary-bytes panic
  test (`config_include_paths_never_panics_on_arbitrary_bytes`) is kept
  unconditional and separate, since panic-safety must hold regardless of
  platform or `Result` variant.

No invariant from the original 59 tests was weakened, dropped, or had its
assertion loosened to make the byte-wise types fit -- every fix was a type
adaptation at the call site, confirmed by re-running the differential
oracles (`config_include_closure_matches_git_show_origin` against real
`git`, still passing) and the full property suite.

**Gate on `main` at `1f866d6` + this branch's three commits (isolated
`CARGO_TARGET_DIR=target-fuzz`):**
- `cargo build --workspace`: clean
- `cargo test --workspace`: **2769 passed, 0 failed, 9 ignored**
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo fmt --all -- --check`: clean
- `cargo test --lib -- proptests`: **60 passed, 0 failed**
- `python3 docs/validate_docs.py --all`: only the same 4 pre-existing
  failures (ADR-0014 `status: proposed`, three stale March docs)

## Handoff Notes

- **Superseded by "PR review round 2" above.** This branch was originally
  stacked on PR #53 (`claude/adr0019-p1-fixes-20260912` at `637afaf`), not
  `main` -- the coordinator rebased it onto `main` (`1f866d6`, #53 and #55
  now merged) and it now lives as PR #59; the original stacked PR #57 was
  closed by GitHub as a rebase side effect when #53's source branch was
  deleted. The config-include-chain functions this lane's differential
  tests exercise (`config_include_paths`, `join_continued_lines`,
  `digest_config_chain`) now live on `main` directly, in their `#53`
  round-3 byte-wise form -- see "PR review round 2" for the signature
  changes and how this lane's tests were adapted.
- No production code changes anywhere in this diff. Every file this lane
  touches production-owned by another lane got test-module-only additions;
  the two findings above are reports for the owning lanes, not fixes.
- `PROPTEST_CASES=2000 cargo test --lib -- proptests` deep-run result and
  timing: recorded in the PR body / final gate evidence for this checkout
  (see git history for the exact numbers at merge time -- not duplicated
  here per this repo's "no checked-in aggregate test count" convention).
