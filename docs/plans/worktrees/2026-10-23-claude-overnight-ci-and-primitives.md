---
title: Overnight CI unblock, blueprint 2.1 review, new primitives
description: Work card for the 2026-10-22/23 overnight Claude Code lanes
updated: 2026-10-23
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Overnight CI unblock, blueprint 2.1 review, new primitives

## Lane Facts
- Owner: Claude Code (Opus 5.5), one session, with research and review subagents and MiniMax-M3 review
- Role: unblock CI, review and harden the open blueprint 2.1 PRs, add small primitives from the cross-agent research sweep
- Branches and worktrees (each worktree under `~/code/`):

  | Branch | Worktree | PR |
  |---|---|---|
  | `fix/ci-lockfile-and-benchmarker-flake` | `IMPULSE-rs-ci-unblock` | #145 |
  | `docs/primitive-candidates-20261022` | `IMPULSE-rs-docs` | #146 |
  | `feat/loop-tool-call-budget` | `IMPULSE-rs-toolcap` | #147 |
  | `feat/ion-read-before-write` | `IMPULSE-rs-rbw` | #148 |
  | `chore/monty-1.3` | `IMPULSE-rs-monty` | #149 |
  | review fixes on #140, #141, #142 | `IMPULSE-rs-pr140`, `-pr141`, `-pr142` | #140, #141, #142 |
  | `scratch/integration-20261023` | `IMPULSE-rs-integ` | local only, never pushed |

- Owned paths:
  - #145: `Cargo.lock`, `.github/workflows/ci.yml`, and the test or timing fixes listed below.
  - #147: `src/loop_contract.rs`, the tool loop in `src/llm_backends/mod.rs`.
  - #148: `src/ion_repl/chat.rs` (`FileVersions`), and a `pub(crate)` re-export in `src/tooling/mod.rs`.
  - #149: `src/tools/python.rs` tests and the lock.
  - #146: `docs/plans/2026-10-22-primitive-candidates.md`, and this card.
- Blocked/shared paths: `src/state/governed_task.rs` and `impulse-ops/src/governed_task.rs` are shared by #140 and #142, which both append tests there. Nothing new went into the governed ledger outside #140–#142. No ADR was edited.
- Plan/spec: `docs/plans/2026-10-22-primitive-candidates.md` (#146). Blueprint 2.1 is cited by #140–#142, but `agent-data/1B/blueprint.md` is not on this machine.
- Verification: from `impulse-rs`, with a `CARGO_TARGET_DIR` isolated per worktree, run `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check`.
- Latest status: all PRs are open; none are merged.
  - #145 has been green; CI re-runs after each push.
  - #149 is green.
  - The rest fail only on the three main-side flakes that #145 fixes.
  - All six code PRs merged together locally pass the gate: 3,764 passed, 0 failed.

## Decisions
- 2026-10-22: CI's Test and Lint jobs run `--workspace`, matching the CLAUDE.md gate. The root-package-only jobs let two broken `impulse-desktop` checks reach main.
- 2026-10-22: A ledger refusal of a producer receipt stays `ReceiptLost`/Unknown, not `FailedKnown`, because the effect already ran (#141).
- 2026-10-23: A takeover is refused live while a claim is under review, but replay does not re-judge it (#140).
- 2026-10-23: `max_tool_calls` defaults to 50 for Ion and stays `None` for the governed Builder, so stored loop-report digests are unchanged (#147).
- 2026-10-23: For read-before-write, only a whole-file read counts. A file's version is its mtime, length, and on Unix its inode and ctime, keyed by `resolve_for_write` (#148).
- 2026-10-23: The pane input writer is stopped before every kill, and `write_injection` checks its quiet period under the writer lock (#145).
- Set aside, each needing James's decision (details in #146):
  - argument-bound `ApprovalGrant`
  - claim-blind Supervisor review
  - output redaction
  - the Python-free calculator
  - `monty-pool` packaging
  - the default token budget

## Handoff Notes
- Merge #145 first. Then merge #142, #141 and #140; #146–#149 can go in any order. Conflict resolutions:
  - `Cargo.lock`: take #145's version, or for #149 re-run `cargo update -p monty -p monty-types`.
  - #140 against #142: both append tests. Keep #142's side and append #140's fencing test section.
- Rare flake, not fixed: `detached_materialization_bounds_hanging_smudge_filters` failed once on macOS CI. #145 now logs what Git still lists when cleanup fails, so the next failure will show the cause.
- P0, outside these lanes: home coordinates remain in public history at `dc723b5`. History rewrite or a private repo is James's call.
- After merge, delete the worktrees, the local integration branch, and `~/.cargo-target-lanes/{ci-unblock,pr140,pr140-clippy,review,review2*}`.
