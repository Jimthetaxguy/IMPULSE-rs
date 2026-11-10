---
title: Bounded File Read Pagination Allocation
description: Preserve file_read output while retaining only requested lines.
author: codex/Codex
created: 2026-10-25
updated: 2026-10-25
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, verification, performance]
---

# Bounded File Read Pagination Allocation

## Lane Facts

- Owner: codex/Codex; implementation and verification.
- Branch: `codex/overnight-file-read`.
- Worktree: `.worktrees/overnight-file-read`.
- Base: `origin/main` at `d9b860e`.
- Owned paths: `impulse-rs/src/tooling/builtin/file_read.rs` and this card.
- Blocked/shared paths: manifests, lockfile, agent instructions, protocol/policy contracts, other tools, and all other worktrees.
- Plan/spec: this card, under the canonical Rust product contract.
- Verification: isolated `CARGO_TARGET_DIR=target/agent-overnight-file-read`; focused file-read tests, then workspace build/check/test/strict Clippy/format; documentation checks.
- Latest status: implemented and locally verified; no commits or publication. Eight documentation baseline failures remain unchanged from clean main.

## Decisions

- `file_read` currently materializes a `Vec<&str>` for every line before returning a page. A 1 MB all-newline file creates roughly 16 MB of line references on a 64-bit target even when the caller requests only 200 lines.
- Replace this with one pass counting all lines and appending only the requested lines to the output string. Extra allocation must scale with returned text, not total line count. Do not preallocate from an untrusted requested line count.
- Preserve the existing JSON shape, 1-based starting-line normalization, saturation, zero-line behavior, missing/outside-path errors, capabilities, metadata, and UTF-8/CRLF semantics.
- No public contract, permission, file-size policy, dependency, or filesystem authorization changes. This is an allocation optimization, not a claim of complete snapshot isolation or file-read hardening.

## Acceptance

1. Every returned JSON field matches the prior slicing algorithm for representative and property-generated Unicode/newline/page inputs.
2. Dense-newline files return the correct page without an all-lines collection in production.
3. Existing permission, capability-file, missing-file, and extreme-offset tests continue to pass.
4. Record before/after measurements against identical dense-file test workloads, distinguishing microbenchmark evidence from end-to-end model latency.
5. Workspace and documentation gates pass, or unrelated baseline failures are specifically recorded without being bypassed.

## Changes

- Replaced the all-lines reference vector with a single counting pass that appends only the requested page. No allocation is based on an untrusted requested line count.
- Added property-generated equivalence checks against the prior algorithm, explicit Unicode/CRLF/blank/empty/extreme-offset cases, a dense-newline regression, and an opt-in repeated-read benchmark.
- Independent read-only review by `codex/min_agent_efficiency` found no actionable code issue.

## Tests

- Focused file-read tests: 11 passed, one benchmark ignored by default and executed separately.
- Full workspace build, check, tests, strict Clippy and formatting passed using the isolated target directory. Tests: **4,020 passed, zero failed, 12 ignored**. Ignored tests are not claimed as exercised by the workspace run.
- One paired debug benchmark performed 100 real tool reads of a synthetic 1,000,000-byte all-newline file. Baseline versus optimized peak RSS: **918,208,512 versus 15,319,040 bytes**; elapsed tool-loop time: **5,202,473 versus 5,269,581 microseconds**. This demonstrates lower measured process memory in this workload, not a latency improvement, release-performance result, or production memory bound.
- Documentation metadata: 222 valid, zero invalid. The full documentation gate reports eight failures also reproduced on untouched main: two missing VISION contract markers and six stale documents. Diagnostics match apart from the expected 221-versus-222 document count; no unrelated documents were modified or gates bypassed.
- Test fixtures use synthetic temporary files only; no providers or personal files.
- Raw logs: `/tmp/impulse-overnight-file-read-20261025/`; retained copies: `../../../_working-files/overnight-codebase-improvements-evidence/2026-10-25/impulse/` relative to this worktree root.

## Handoff Notes

Original source is preserved at `/tmp/impulse-overnight-file-read-20261025/file_read.original.rs`. Rollback is a reviewed patch against the two owned paths; do not reset or discard concurrent work. Main remains untouched; integration/publication are separate steps.
