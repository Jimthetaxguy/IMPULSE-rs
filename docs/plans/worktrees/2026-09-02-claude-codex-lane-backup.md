---
title: Codex Packaged-Acceptance Lane Backup + Envelope Fix Extraction
description: Work card for claude-codex-lane-backup-20260902
updated: 2026-09-02
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, handoff, backup, dioxus, host-bridge]
---

# Codex Packaged-Acceptance Lane Backup + Envelope Fix Extraction

## Lane Facts
- Owner: Claude
- Role: backup and extraction (not the packaging lane itself)
- Branch: `claude/codex-lane-backup-20260902`
- Worktree: `.worktrees/codex-lane-backup-20260902`
- Owned paths: `impulse-rs/impulse-desktop/src/host_bridge.rs` (envelope fix only),
  `CONTEXT.md` (one new glossary entry), this lane card
- Blocked/shared paths: everything else in `impulse-desktop`, `.github/**`, `scripts/**`,
  `Cargo.toml`/`Cargo.lock`, `AGENTS.md`, `CLAUDE.md`
- Plan/spec: handoff item from the Impulse-rs Stage 0 branch-age-hygiene audit (unpushed
  Codex lane branch, P1 risk — single-machine work is unbacked work)
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  plus targeted `impulse-desktop` host_bridge envelope tests and the `desktop-app` feature build
- Latest status: complete. Codex branch backed up to `origin`. Envelope fix cherry-picked
  cleanly (only docs conflicts, resolved) onto `origin/main` (36bda00). Full gate green.

## Decisions
- 2026-09-02: Pushed `agent/codex-dioxus-packaged-acceptance-20260830` to `origin` unchanged
  (plain `git push`, no rewrite) to satisfy branch-age-hygiene. Did not open a PR for that
  branch — it is Codex's own lane and its packaged-DMG acceptance run is still incomplete.
- 2026-09-02: `git cherry-pick 265c183` (the envelope fix) applied cleanly against
  `host_bridge.rs` with no functional conflict — the only conflicts were in `CONTEXT.md`,
  `docs/plans/EGUI-DECOMMISSION.md`, and the Codex lane's own work card, all doc files that had
  diverged because three earlier Codex-lane commits (`7e25412`, `ecd9ce5`, `11b105c`) were not
  cherry-picked. Confirmed `265c183` has no code dependency on those three commits: the only
  symbol it references, `live_host_bridge_script`, already exists on `main` from an
  already-merged commit. No earlier commits were pulled in.
- 2026-09-02: Resolved `CONTEXT.md` by keeping only the new "Dioxus host invoke wire" glossary
  entry; dropped the commit's "release-readiness proof" section since that entry already exists
  on `main` (added by an earlier, already-merged commit) and would have been a duplicate.
- 2026-09-02: Resolved `docs/plans/EGUI-DECOMMISSION.md` by reverting it to `HEAD` entirely —
  the conflicting hunk was 2026-08-29/2026-08-30 packaging-lane checkpoint narrative that
  describes work-in-progress on the whole packaged-acceptance lane, not the envelope fix itself,
  and is out of scope for this narrow extraction.
- 2026-09-02: Removed `docs/plans/worktrees/2026-08-30-codex-dioxus-packaged-acceptance.md`
  (`git rm`) rather than reintroducing it — that lane card belongs to Codex's own worktree lane,
  not this extraction, and git reported it as deleted-in-HEAD/modified-in-commit.
- 2026-09-02: Kept `git cherry-pick -x` so the resulting commit records
  `(cherry picked from commit 265c183f124c4fcfcb495b9449f49fd3a97f7a4d)`; author identity on both
  commits is `Jimthetaxguy <pustorino.james@gmail.com>`, so no separate porting-glue commit was
  needed.

## Changes
- `impulse-rs/impulse-desktop/src/host_bridge.rs`: `HostInvokeResponse` is no longer directly
  `Serialize`; a new private `HostInvokeResponseEnvelope` (`kind: "host_invoke_result"`, `id`,
  `ok`, `result`, `error`) is the only serializable Rust → JS wire shape, built by
  `host_invoke_response_envelope()`. Both live-bridge send paths (normal worker results and
  queue-rejection results) now go through that helper instead of serializing
  `HostInvokeResponse` directly, so every response the Rust host emits carries the discriminator
  the strict JavaScript router requires to settle the matching promise.
- `CONTEXT.md`: added the "Dioxus host invoke wire" glossary entry documenting the
  request/response `kind` discriminator contract.

## Tests
- `cargo build --workspace` — clean.
- `cargo test --workspace` — all green; representative package totals: `impulse-rs` lib
  1790 passed/5 ignored, `impulse-desktop` lib 149 passed, `impulse-desktop` `views_ssr`
  22 passed/1 ignored, `impulse-term` 93 passed, `impulse-ops` 64 passed. Zero failures across
  the full run.
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
- `cargo fmt --all -- --check` — zero diffs.
- `cargo test -p impulse-desktop --lib host_bridge::` — 25/25 passed, including
  `host_invoke_response_envelope_marks_both_arms_for_the_js_router` (asserts `kind` on both the
  ok and error arms) and `live_host_bridge_invoke_promise_settles_from_the_rust_response_envelope`
  (a real Node smoke that loads `live_host_bridge_script()` and proves a resolve and a typed
  reject both settle from the emitted envelope; confirmed it actually ran, not skipped —
  `node v22.22.3` present).
- `cargo test -p impulse-desktop --features desktop-app` — 155 lib tests + 22 desktop-contract +
  7 views_ssr passed, 0 failed, ran headless (no display needed for this crate's test suite).

## Handoff Notes
- Pushed ref: `agent/codex-dioxus-packaged-acceptance-20260830` now exists on `origin` at
  `265c183f124c4fcfcb495b9449f49fd3a97f7a4d` (verified via `git ls-remote --heads`).
- The remaining four Codex packaging commits (`69b2491`, `7e25412`, `ecd9ce5`, `11b105c` plus
  `265c183` in place on that branch) stay on Codex's own lane branch — this extraction only
  landed the envelope fix on `main`. Codex should rebase its lane onto `main` once this PR
  merges, since `main` will then already carry the envelope fix and Codex's branch would
  otherwise reapply it redundantly.
- No functional code depended on the three skipped commits; only documentation content did
  (packaging-lane status narrative), and that was deliberately left out of this extraction.
