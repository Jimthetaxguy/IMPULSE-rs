---
title: Dioxus Release Truth
description: Work card for codex-dioxus-release-truth-20260829
updated: 2026-08-29
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, release, dioxus, ci]
---

# Dioxus Release Truth

## Lane Facts

- Owner: Codex root orchestrator; read-only release, CI, and product-contract reviewers assist.
- Role: Isolated integration lane for CI portability and truthful Dioxus packaging.
- Branch: `agent/codex-dioxus-release-truth-20260829`.
- Worktree: `.worktrees/dioxus-release-truth-20260829` (repository-relative).
- Base: exact `origin/main` commit `a611e06b82539c0e7c3d9edb7fde9905ce4380ac`.
- Owned paths: `.github/workflows/ci.yml`, `.github/workflows/release.yml`,
  `impulse-rs/scripts/build-macos-app.sh`, the
  Dioxus-owned macOS bundle resources/tests needed by that script, the narrow failing history
  regression, the two feature-gated Dioxus ownership repairs in `ui.rs`/`views.rs`, and this work
  card; `README.md`, `CONTEXT.md`, and `docs/plans/EGUI-DECOMMISSION.md` carry the matching stable
  release-readiness vocabulary and status.
- Blocked/shared paths: canonical dirty worktree changes; `Cargo.toml`, `Cargo.lock`, `AGENTS.md`,
  `CLAUDE.md`, canonical contract/index files, legacy EGUI removal, Tauri removal, signing,
  notarization, installation, tag creation, release publication, and runtime/live-state mutation.
- Plan/spec: R1 preferred Dioxus path in `docs/plans/EGUI-DECOMMISSION.md`; keep release truth and
  destructive legacy retirement as separate commits/lanes.
- Verification: exact failing test; script syntax and contract tests; Dioxus desktop feature build;
  deterministic app-bundle/DMG inspection; full workspace build/test, strict Clippy, formatting,
  docs validators, diff checks, and leak scan. Live packaged launch remains a separate blocker.
- Starting remote status: exact base-head Ubuntu CI fails one nonportable history fixture; active
  tag packaging copies excluded `impulse-gui` output and has never produced a tag or GitHub
  release.

## Decisions

- 2026-08-29: Preserve the canonical dirty `main` tree and implement only from a fresh
  `origin/main` worktree.
- 2026-08-29: Prefer a tested Dioxus `.app`/DMG recipe over the CLI-only fallback; do not claim a
  shipped release until hosted readiness passes, license/signing/notarization/live acceptance are
  closed, and separately authorized publication succeeds.
- 2026-08-29: Treat MiniMax and other helpers as proposal/review inputs only; local code, product
  contracts, exact-head tests, and remote evidence remain authoritative.

## Changes

- Frozen the exact GitHub CI failure, a restrictive-umask reproduction, and the active packaging
  mismatch against exact `origin/main`.
- Converged three independent reviewers and MiniMax proposal review on explicit locked package
  builds, deterministic artifact inspection, and a non-publishing, non-distributable candidate
  boundary.
- The first native candidate build reached code that default CI never compiled and exposed two
  Dioxus `rsx!` move/borrow errors; both are now frozen as release-gate regressions.
- Normal CI and the manual readiness workflow build and inspect candidates but retain no
  downloadable artifacts while this public repository's license and release access remain
  unresolved.

## Acceptance Criteria

- The exact Linux CI regression is portable and still proves `save()` returns `Err` for an
  impossible parent path.
- No active release script or workflow references or copies `impulse-gui`.
- The packaging recipe explicitly builds the feature-gated `impulse-desktop` binary and the
  `impulse-rs` companion plus native `ion` sibling for every requested macOS architecture.
- The app bundle names the Dioxus executable, carries versioned Dioxus-owned metadata/assets, and
  passes deterministic artifact inspection before DMG creation.
- No Developer ID bundle signing, notarization, tag, release, install, or live runtime mutation
  occurs in this lane without separate authority; toolchain-generated ad-hoc Mach-O signatures may
  be present.
- All named verification gates pass on the exact commit, or any host/CI-only gap is recorded as a
  blocker rather than represented as completion.

## Tests

- History failure: focused test passes normally and under restrictive `umask 0777`.
- Dioxus source: feature-gated desktop check and strict feature-gated Clippy pass after the two
  minimal `rsx!` ownership repairs; focused governed-task and artifact SSR tests pass.
- Packaging contract: 8/8 portable positive/adversarial tests pass; shell syntax and YAML parsing
  pass; active workflow/script guards contain no EGUI, publication, Developer ID signing, or
  destructive paths.
- Native package: optimized arm64 Dioxus host, `impulse-rs` companion, and native `ion` sibling
  build; staged `.app`, DMG checksum, read-only mount, Mach-O load commands, closed asset allowlist,
  notice, and mounted `.app` all verify.
- Workspace: formatting, all-target locked check, strict all-target locked Clippy, full locked tests,
  and locked release workspace build pass. The main library reports 1,709 passed / 5 ignored; all
  Dioxus, integration, terminal, and doc-test harnesses complete without failures.
- Documentation: validator self-test passes. Repository-wide validation remains red on one
  pre-existing unsupported ADR status and three untouched stale-document findings.
- Leak scan: full-worktree scan reports three generic-key matches only in two unchanged
  `origin/main` blobs; current and base Git object IDs match exactly. The staged Gitleaks scan
  passes.

## Handoff Notes

- Canonical `main` has six unstaged tracked changes and two untracked paths; none belong to this
  lane and none will be copied, staged, reverted, or committed here.
- Public release remains blocked on license choice, hosted universal readiness proof, live packaged
  bridge/companion acceptance, authorized signing, and notarization/stapling.
- Linux arm64 is not an evidenced candidate: the shipping HTTP graph uses native OpenSSL, and the
  former unexercised workflow installed only a cross linker rather than a target OpenSSL sysroot.
- P2 provenance hardening remains: action major tags, Rust `stable`, and `*-latest` runner images
  float even though Cargo inputs and the source commit are locked.
