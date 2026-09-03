---
title: Ion Tool Floor
description: Work card for claude/ion-tool-floor-20260902 (Stage 1 sandbox roots, untrusted-output envelope, loop evidence, hermetic git fixture)
updated: 2026-09-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, ion, sandbox, guardrail, loop-contract, prompt-injection]
---

# Ion Tool Floor

## Lane Facts

- Owner: Claude (Fable 5.1), Stage 1 of `docs/plans/2026-09-02-impulse-next-stages.md`.
- Role: implementation lane for the ion REPL tool sandbox floor.
- Branch: `claude/ion-tool-floor-20260902`.
- Worktree: `.worktrees/ion-tool-floor-20260902` (repository-relative).
- Base: `origin/main` at `36bda00`.
- Owned paths:
  - `impulse-rs/src/ion_repl/**` (`mod.rs`, `chat.rs`, `tool_bridge.rs`, `router.rs`,
    `tool_document.rs`, `tool_verify.rs` -- the latter two grew real sandbox-enforcement logic in
    review round 1, not just the `ReplContext` field addition and fixture swap noted originally)
  - `impulse-rs/src/guardrail/defaults.rs`
  - `impulse-rs/src/test_support.rs`
  - `impulse-rs/src/handlers/ion.rs` (fixture replacement only)
  - `impulse-rs/tests/ion_verify_cli.rs` (fixture hardening only)
  - `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`
  - `CONTEXT.md` (one glossary entry), this work card
- Blocked/shared paths (per the assignment, not touched): `impulse-rs/src/daemon/**`,
  `impulse-rs/src/state/**`, `impulse-rs/src/governed_producers.rs`,
  `impulse-rs/impulse-desktop/**`, `impulse-rs/impulse-ops/**`, `.github/**`,
  `Cargo.toml`/`Cargo.lock` (no new dependencies), `CLAUDE.md`, `AGENTS.md`,
  `impulse-rs/src/loop_contract.rs` (read-only).
- Plan/spec: `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`. No ADR
  (assignment scope).
- Verification (isolated `CARGO_TARGET_DIR`, per the shared-target-dir memory note):
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `cargo build --no-default-features`, `python3 docs/validate_docs.py --all`.
- Latest status (post review round 2): `cargo build --workspace` clean; `cargo test --workspace`
  2379 passed / 0 failed / ~4 ignored across the full workspace (main lib crate 1859 passed / 0
  failed / 5 ignored); strict Clippy clean across the workspace; rustfmt clean; `cargo build
  --no-default-features` clean; `docs/validate_docs.py --all` reports only the 4 pre-existing
  failures (ADR-0014 `status: proposed`, three stale March docs) -- the updated spec file
  validates cleanly. `cargo test --lib -- ion_repl` run three times in a row: 177 passed / 0
  failed each time, no flake. See "Review round 1" and "Review round 2" below for what changed
  since the initial PR.

## Decisions

- 2026-09-02: Write roots stay fixed to `repo_root` unconditionally -- not widened by `/allow` and
  not widened by a literal `CONFIRM`. `CONFIRM` on an out-of-sandbox write means "I saw the target
  and approve attempting it," not "bypass the sandbox." Only read roots are extensible, one path
  at a time via `/allow <path>`.
- 2026-09-02: The sandbox-escape verdict reuses the existing `decide_approval` literal-`CONFIRM`
  machinery (added earlier for guardrail `Block` matches) rather than inventing a second approval
  tier -- a sandbox escape is exactly as serious as a guardrail block, not a separate concept the
  human has to learn.
- 2026-09-02: The per-turn untrusted-output escalation lives on `ReplToolExecutor` as an
  `AtomicBool` (not `Cell`), since `ToolExecutor: Send + Sync` requires it and `execute` takes
  `&self`.
- 2026-09-02: Three new `GuardTarget::ToolCall` rules are `Warn`-tier, not `Block`-tier -- they are
  signal for the untrusted-output escalation mechanism, not independent hard stops on their own
  (a `Warn` match alone doesn't force `CONFIRM`; the per-turn escalation it triggers does, for
  *later* gated calls, which is the actual defense).
- 2026-09-02: `resolved_paths_for` only inspects the path-bearing argument each tool actually uses
  (`path` for `file_read`/`file_write`, `cwd` for `bash_exec`) rather than attempting to parse
  paths out of `bash_exec`'s free-text `command` -- out of scope per the plan ("OS-level sandboxing
  and egress allowlists" is a later stage).
- 2026-09-02: `tests/ion_verify_cli.rs` keeps its own copy of the hermetic git fixture rather than
  trying to share `test_support::init_git_repo` -- it is a separate integration-test crate compiled
  against a non-`--cfg test` build of the library, so it structurally cannot see a
  `#[cfg(test)]`-gated module. Documented in place on both copies.

## Changes

- `ion_repl::ReplContext` gains `allowed_read_roots: Vec<PathBuf>`, `effective_repo_root()`
  (falls back to `current_dir()` when `repo_root` is unset -- only `ReplContext::default()`, i.e.
  tests), and `sandbox_tool_context()` (builds the `ToolContext` every bridged tool runs under:
  `allowed_write_roots = [repo_root]`, `allowed_read_roots = [repo_root] + allowed_read_roots`, all
  capabilities still granted).
- New `apply_allow(ctx, args)` handles `/allow <path>`; new router `SlashCommand::Allow(Vec<String>)`
  and `SlashCommand::Loop`; `KNOWN_COMMANDS` grows to include `/allow` and `/loop`.
- `tool_bridge::DynamicToolBridge::run` builds its `ToolContext` from
  `ctx.sandbox_tool_context()` instead of the previous unrestricted
  `ToolContext::with_all_capabilities()`.
- `ion_repl::chat`: `resolved_paths_for`, `sandbox_escape_verdict`, `most_severe_verdict`,
  `untrusted_seen_verdict` (new); `ConfirmFn`/`ReplToolExecutor::confirm` grow a fourth parameter
  (`&[PathBuf]`, the resolved paths); `confirm_via_stdin` prints them; `ReplToolExecutor` gains
  `untrusted_seen: AtomicBool`; every tool result is scanned via `guard_scan(GuardTarget::ToolCall,
  ...)` and wrapped via new `wrap_untrusted_tool_output`/`raw_tool_result_content` (replacing the
  unwrapped `tool_result_content`, kept `#[cfg(test)]`-only as a thin composition for tests).
- `guardrail::defaults::builtin_rules()` gains 3 `GuardTarget::ToolCall` rules
  (`warn-tool-output-injection-phrase`, `warn-tool-output-role-override`,
  `warn-tool-output-credential-shaped`); built-in rule count 10 -> 13.
- `ion_repl::mod::respond` appends a one-line `loop_trip_summary` to a chat reply whenever
  `ChatState::last_loop_report` shows `LoopTermination::Tripped`; new `/loop` command prints the
  full report as pretty JSON via `loop_report_text`.
- `test_support::init_git_repo()` (new, hermetic: `GIT_CONFIG_GLOBAL=/dev/null`,
  `GIT_CONFIG_NOSYSTEM=1`, `HOME` redirected into the tempdir, stderr captured on failure) replaces
  four in-crate copies (`ion_repl::{mod,chat,tool_verify}`, `handlers::ion`);
  `tests/ion_verify_cli.rs`'s copy is hardened with the same env vars in place (kept separate --
  see Decisions).

## Tests

- `tool_bridge`: 6 new tests proving the sandbox -- write inside/outside `repo_root`, read
  outside `repo_root` refused without a grant, read succeeds once `/allow`-granted, write to a
  granted-but-not-`repo_root` path still refused.
- `chat`: `most_severe_verdict` severity ordering; `resolved_paths_for` per-tool field selection
  (including the empty-`cwd` case); `sandbox_escape_verdict` inside/outside/empty; end-to-end
  acceptance tests -- out-of-root write refused before any I/O even with a plain `y`, a literal
  `CONFIRM` still refused by the underlying sandbox, in-root write succeeds with a plain `y`,
  instruction-shaped `file_read` output escalates a following innocuous `bash_exec` to `CONFIRM`,
  a `bash_exec` before any untrusted output is not escalated, every successful tool result is
  wrapped in the envelope.
- `guardrail::defaults`: the three new `ToolCall` rules match their target text and do not fire
  for `Bash`/other targets; a benign tool result yields no verdict; built-in count assertion
  updated to 13.
- `ion_repl::mod`: `ReplContext` accessors (`effective_repo_root`, `sandbox_tool_context`);
  `apply_allow` usage/absolute-path/accumulation; `/allow` and `/loop` routed end-to-end through
  `respond`; `/loop` before any turn reports "no report yet"; `/loop` after a completed turn
  prints the pretty-JSON report.
- `test_support`: `init_git_repo` produces a repo with a resolvable `HEAD`; two calls yield
  independent directories.
- `router`: `/allow` and `/loop` routing (with-args, no-args, `KNOWN_COMMANDS` updated).

## Review round 1 (adversarial probe crate, same day)

An adversarial review with a probe crate of live reproductions (`tests/attacks.rs`, A1-A11)
returned "needs changes" against the PR as originally opened. All findings addressed on this same
branch, gated, and re-pushed (no force-push -- new commit(s) on top).

**P1 -- `document_read` bypassed the sandbox entirely.** `tool_document::resolve_document_path[_with_cap]`
accepted any absolute path and any relative `../` traversal unconditionally -- it never consulted
`ReplContext::sandbox_tool_context` at all, unlike every bridged tool. Fixed: the function now
takes `&ReplContext` (signature change, all call sites updated) and refuses via
`tool_ctx.is_path_allowed(&path, false)` before checking the file even exists. Same treatment for
`tool_verify::IonVerifyTool::run`'s model-supplied `repo` argument (it ships the resolved repo's
diff to an external API) -- refused before `run_ion_verify` is ever called if it resolves outside
the sandbox. Tests: `..` traversal and absolute-outside cases for both, plus positive
`/allow`-granted-path cases.

**P1 -- `bash_exec` shell text was fully unconstrained.** `resolved_paths_for` only ever extracted
`cwd`; the command's own TEXT (`echo x > <outside>/f`, `cat <outside>/id_rsa`,
`cd <outside> && ...`) ran behind a plain y/N with no path check at all. Fixed with an explicitly
**advisory** heuristic (`bash_command_escape_candidates`/`bash_command_verdict`): tokenizes the
command, flags absolute-path tokens, `..`, `~`, `$HOME`, and an escaping `cd` target, and
escalates to literal `CONFIRM` listing the offending tokens -- documented in the function doc
comment, the verdict's own `reason` text, `CONTEXT.md`, and the spec as advisory, not enforcement.
`CONTEXT.md`, the spec (Decisions 1/2/2b), and this card's earlier "Handoff Notes"/PR-body wording
corrected to state the enforced boundary is `file_read`/`file_write`/`document_read`/`ion_verify`
paths and `bash_exec`'s `cwd` only, and that `governed_submit_claim` is a separate, non-bridged,
ungated tool this sandbox does not cover at all.

**P2 -- tool errors were not enveloped or scanned.** `ReplToolExecutor::execute`'s `Err` arm
bypassed both the untrusted-output envelope and the `GuardTarget::ToolCall` scan, even though
error text (`ToolError::PathNotAllowed` and similar) echoes caller-supplied paths verbatim. Fixed
by extracting the shared `observe_and_wrap` method, called from both the `Ok` and `Err` arms.

**P2 -- envelope delimiters were fixed literals.** Content could include the literal footer text
and close the envelope early in the model's eyes. Fixed with a per-call random 8-hex-char nonce
(`uuid::Uuid::new_v4`, already a workspace dependency) embedded in both header and footer.

**P2 -- batch order dependence / per-turn reset was too narrow.** A batch
`[bash_exec, file_read(poisoned)]` would confirm `bash_exec` before the poisoned `file_read`
result was scanned, and poisoned content persists in history across turns regardless of a
per-turn flag. Fixed: `untrusted_seen` moved from a fresh-per-`turn()` `ReplToolExecutor` field to
`ChatState` itself (`AtomicBool`, borrowed by reference into each turn's executor), sticky for the
whole session, reset only by `ChatState::clear` (paired with clearing history).

**P2/nit -- `/allow` hardening.** A grant resolving to `/`, `$HOME`, or an ancestor of the repo
root now prints a loud warning (still grants it -- explicit human request); a bare `/allow` lists
current grants instead of only a usage message; an empty or nonexistent path is refused outright.

**Nits -- wording.** `chat.rs`'s module doc and the spec no longer claim denial happens "before
`ToolRegistry::execute`" as if that were the general rule -- the confirmation-layer escalation
(gated tools only) forces a human decision, and the sandboxed `ToolContext` (checked inside
`ToolRegistry::execute` via `validate_paths`, before any I/O) is the actual authority either way,
including after a `CONFIRM`. The "resolved paths ... gated call or not" doc claim was corrected:
`resolved_paths_for` only runs inside the `CONFIRMATION_REQUIRED_TOOLS` branch, so an ungated
`file_read` denial surfaces only as that tool's own error text, not a dedicated notice -- documented
rather than changed, since computing paths for every tool call would be a larger behavior change
than this round of fixes scoped for. The three `GuardTarget::ToolCall` rules' known false-positive
rate (README prose, ordinary "you are now ready" phrasing, freshly generated placeholder secrets)
is now documented directly in `src/guardrail/defaults.rs` and the spec.

## Review round 2 (re-probe of the round-1 fixes, same day)

Re-probing confirmed every round-1 fix holds (358 lib tests green under the re-probe's filter,
symlinked dirs/files denied, two-turn sticky escalation proven, the nonce envelope inert against a
replayed footer). Two tightening items:

1. **Advisory bash heuristic near-misses.** `echo x >/tmp/f` (redirect glued to the path, no
   whitespace) and `cat ${HOME}/.ssh/id_rsa` (brace-expansion form of `$HOME`) were not escalated.
   `split_shell_tokens` now splits on shell metacharacters (`<>|&;()`) as well as whitespace, with a
   defensive strip of any metacharacter still glued to a token's front, and `${HOME}` is checked
   alongside `$HOME`. Three regression tests added; known inherent misses (`python3 -c
   "open('/tmp/x')"`, `H=/etc; cat $H/passwd`) stay documented as advisory limits -- see the spec's
   new "Review round 2" section.
2. **PR body rewritten** to match post-fix behavior (session-sticky `untrusted_seen`, the full
   round-1 fix list, `governed_submit_claim` named as a conscious carry-forward gap, the "warns but
   still grants" `/allow` semantics, current gate totals) -- also folded into `CONTEXT.md` and the
   spec's Decision 1.

Latest status after round 2: gate re-run clean end to end (see the top-of-card status line, kept
current). `cargo test --lib -- ion_repl` three-in-a-row: 177 passed / 0 failed each time.

## Handoff Notes

- Deliberately not built this lane (out of scope per the assignment): GENOME Markdown rendering,
  the three real-file workflow tests named in the plan's Stage 1 bullet list, provider-neutral
  tool calls, context budgets.
- `bash_exec`'s shell command text is only advisorily flagged, never confined (see "Review round
  1" above) -- full confinement needs either shell-syntax parsing/rewriting (fragile for a security
  boundary) or a real OS-level sandbox, both explicitly Stage 4 in the next-stages plan.
- The `/allow`-granted read roots are session-scoped and in-memory only (`ReplContext`, not
  persisted to `.impulse/`); they do not survive a REPL restart.
- No new dependencies were added; the workspace `Cargo.toml`/`Cargo.lock` are untouched (`uuid` for
  the envelope nonce was already a workspace dependency).
- `governed_submit_claim` is a separate, non-bridged `ReplTool` that mutates daemon-owned governed
  task state, ungated, and out of scope for every fix in this lane -- flagged for a future lane's
  attention if that tool ever needs the same treatment.
