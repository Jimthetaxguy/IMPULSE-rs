---
title: CLI Release Workflow Repair
description: Isolated repair of retired Intel runners and stale desktop release packaging
updated: 2026-09-14
type: doc
category: planning
phase: all
status: review
audience: builders
tags: [worktree, lane, release, verification]
---

# CLI Release Workflow Repair

## Lane Facts

- Owner: Codex release-workflow lane; root integrator owns merge decisions.
- Role: implementation and verification.
- Branch: `codex/cli-release-workflow-20260914` (repository collaboration convention).
- Worktree: `.worktrees/cli-release-workflow-20260914`, based on `origin/main` at `552d38867856e479e436d33baba3e8f4f6f8dec3`.
- Owned paths: `.github/workflows/release.yml`, `impulse-rs/scripts/build-macos-app.sh`, release guidance in `README.md` and `CONTEXT.md`, R1 status in `docs/plans/EGUI-DECOMMISSION.md`, and this work card.
- Blocked/shared paths: Rust source and manifests/locks, GUI/desktop implementations, unrelated workflows and prior worktrees.
- Plan/spec: `docs/plans/EGUI-DECOMMISSION.md` Track A/R1 explicitly chosen CLI-only fallback.
- Verification: complete Rust workspace build/check/test/strict Clippy/fmt; tag-equivalent native CLI build/copy/smoke/checksum; legacy bundler refusal before side effects; workflow validation; documentation validators.
- Latest status: release implementation reviewed; Rust workspace gates passed. All local release, Rust, documentation, and independent scheduler-example gates passed. Draft PR/hosted checks are the next boundary. Canonical and prior PR worktrees remain preserved.

## Decisions

- Keep all four existing CLI target/artifact names; move Intel macOS to GitHub's supported `macos-15-intel` runner.
- Remove the required stale DMG job and make the old bundler refuse clearly before building or changing artifacts. Preserve the old script in the lane's external backup and Git history.
- Tagged releases are CLI-only until Dioxus packaging is independently verified. Track C stays blocked; no GUI replacement or legacy GUI revival.
- Verify release builds on pull requests with read-only permissions, and restrict GitHub Release publication to pushed version tags. No release tags or dispatches are part of this task.
- Use a private APFS clone of the completed prior IMPULSE target cache for local verification; never write to the preserved source cache.

## Acceptance Criteria

- Release automation uses a supported Intel runner, builds/copies the intended CLI binary for all four targets, and cannot depend on excluded GUI output.
- PR validation cannot publish a release. Tag releases include explicit CLI assets and checksums with an honest desktop-availability note.
- The historical bundler command fails before tool invocation or artifact mutation.
- Retained Rust surfaces and docs pass required verification; local native evidence and untested cross-target limits are recorded separately from hosted CI.

## Changes

- Replaced retired Intel macOS image and fragile ARM Linux cross-compilation with supported native runners; retained every CLI target and artifact name.
- Build only the locked `impulse-rs` release binary; copy and smoke-test the exact uploaded file.
- Run the four-target build and checksum packaging on relevant PRs with read-only permissions. Gate publication to version-tag pushes with an explicit five-file asset list.
- Removed the stale DMG job. The retained historical command now refuses before builds, signing, deletion, or artifact mutation.
- Documented the selected R1 CLI-only fallback and its desktop/Track C limits.
- Root completed prerequisite documentation corrections: archived the historical 33-candidate backlog with its dates/tables retained, refreshed both Rust multi-agent guides against the current source contracts, bounded the illustrative JoinSet scheduler and propagated errors, and aligned ADR-0014 metadata with review while preserving pending ratification. These files already failed validation on the unchanged base.

## Tests

- Passed `cargo fmt --all -- --check`, `cargo build --locked --workspace`, `cargo check --locked --workspace --all-targets`, `cargo test --locked --workspace`, and `cargo clippy --locked --workspace --all-targets -- -D warnings`, using an isolated target-cache clone and three Cargo jobs.
- Passed actionlint 1.7.12 and Bash syntax checks for every workflow shell step and the compatibility entry point.
- Exercised the retired command with default, universal/DMG/version, and signing arguments: all refused without invoking build/mutation tools or changing existing app/DMG fixtures.
- Executed the exact four-artifact checksum recipe against test fixtures: complete set passes; missing ARM Linux asset fails. These are recipe tests, not cross-target binary evidence.
- Initial docs `--contract` and `--all` failed on four base-identical inherited documentation issues; originals and failure logs remain preserved. Root-owned substantive corrections address them without validator changes.
- Passed the locked `aarch64-apple-darwin` release build and exact workflow staging commands against the real native binary. Root, daemon, and MCP serve help-parser smoke commands passed; Mach-O arm64 inspection and SHA-256 verification passed. The smoke cwd stayed empty.
- Initial `--version` smoke failed because the shipped parser does not expose that flag; preserved the failed run and corrected the new test to exercise supported help interfaces. No runtime behavior changed.
- Local Cargo test aggregate: 3,064 passed, zero failed, nine intentionally ignored across 35 result summaries.
- Other three native targets and the complete real four-binary package await hosted PR checks. Local checksum recipe fixtures are not substitutes for those builds.
- Final documentation contract/all/self-test gates passed: 185 valid metadata documents, zero invalid.
- Independent extraction of the exact new Pattern 5 scheduler example passed offline `cargo check --tests` and `cargo test`: five tests cover empty/success inputs, a deterministic four-task scheduling window, active concurrency across 17 candidates, and evaluator/join failures in both drain paths.

## Handoff Notes

- Root owns final merge decisions. Normal branch push and a draft PR are authorized; no tags, package publication, signing, secrets, or manual release dispatch.
- Rollback is a normal revert of this isolated change; existing GUI source and prior artifacts remain preserved.
