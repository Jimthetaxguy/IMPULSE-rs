---
title: Codex Rust code wiki baseline lane
description: Generate and verify a source-cited code wiki for a pinned clean IMPULSE origin/main snapshot.
updated: 2026-08-28
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, code-wiki, rust, documentation, provenance]
---

# Codex Rust Code Wiki Baseline

## Lane Facts

- Owner: Codex
- Role: documentation-only code-wiki producer lane
- Branch: `agent/codex-impulse-code-wiki-20260828`
- Worktree: isolated linked worktree; its machine-local path is intentionally not recorded
- Base: `origin/main` at `a611e06b82539c0e7c3d9edb7fde9905ce4380ac`
- Base tree: `885ecc1f8117134df63c64b37abd73500d8a0548`
- Owned paths: `impulse-rs/openwiki/**`, narrow README/T5 documentation corrections, and this work card
- Shared/blocked paths: authority documents, runtime semantics, Cargo manifests and lockfiles, daemon/UI/task/memory/retrieval code, and every concurrent lane
- Plan/spec: this work card; the wiki is documentation evidence, not a product interface or authority change
- Latest status: complete / handoff; reviewed regeneration, exact-binary replay, local workspace tests, public-hygiene gates, and independent semantic review passed

## Snapshot and Producer Receipt

- Source scope: tracked inputs under the repository-relative `impulse-rs/` Rust workspace
- Base commit: `a611e06b82539c0e7c3d9edb7fde9905ce4380ac`
- Source commit: `d8b35da77ac7ea61a47b901db5ec0ab4824fc9c6`
- Source tree: `50839f3ee2f0cd275be5c670248a51a06c4358e6`
- Source state: isolated branch snapshot; uncommitted canonical-checkout changes were excluded
- Generator: QuireWiki commit `de833b93a60694117de438600b63e4cda58c8d34`, tree `f7332b5f7f9d700497c60f16c946bee249ef472c`
- Producer binary SHA-256: `07571c145a5291d901419421b1b336a149d36f3217be6fbe6148a464b7c0985c`
- Indexed input: 360 tracked files, 7,587 chunks, inventory hash `12956dfaf51b7b7949c262a33541611109cd2f8bba1d576b717d90a7ff5898c5`
- Source evidence retrieval time: `2026-08-29T03:23:15Z` (preserved because the evidence was unchanged)
- Exact-binary replay time: `2026-08-29T03:41:02Z`
- Generation policy: code mode, extractive claims only, no LLM/provider call, visibility `public`

The canonical IMPULSE checkout has preserved uncommitted step-model and
history-test work. This lane starts from the exact clean remote snapshot and
must not copy, mask, or modify that WIP. The receipt identifies the source
commit/tree, generator commit, producer hash, inventory hash, and source
retrieval time. A producer binary hash identifies the executable used; it is
not a reproducible-build claim.

## Independent CI Boundary

At the pinned source commit, exact-head GitHub CI was red in run
[`33039876475`](https://github.com/Jimthetaxguy/IMPULSE-rs/actions/runs/33039876475)
on `ion_repl::history::tests::test_save_returns_err_when_path_has_no_writable_parent`.

This failure is independent of the documentation lane. A passing QuireWiki gate
does not make repository CI green and must not be presented as code or release
readiness.

## Goal and User-Visible Outcome

Create a deterministic, source-cited map of the pinned Rust workspace so
builders and agents can navigate source evidence without treating generated
prose as daemon, task, memory, policy, capability, approval, or acceptance
truth.

The baseline is intentionally partial. Root `AGENTS.md`, `CONTEXT.md`, the
canonical Rust contract, and live code remain authoritative.

## Non-Goals

- No runtime wiki ingestion or MCP configuration.
- No `ProjectOpsSnapshot` schema change or typed wiki field.
- No task, memory, retrieval, capability, permission, or governed-work mutation.
- No automatic change to authority documents.
- No product-code or Cargo-manifest change.
- No adoption of external proposal material as repository authority.
- No claim that generated pages fully describe IMPULSE architecture.

## Acceptance Criteria

- Every generated claim is extractive and carries a repository-relative citation.
- The public wiki visibly identifies its source commit, source tree, clean-input limitation, generator commit, producer hash, and generation mode.
- The wiki states that it is partial and non-authoritative.
- Root authority documents are read before generated pages.
- No personal machine path, secret, private input metadata, generated binary, or database state enters the candidate.
- Rendered lists, tables, commands, and source links remain usable Markdown.
- Architecture pages are emitted only when a real architecture source exists in scope.
- An unchanged-source scan reports every planned page clean.
- QuireWiki's structural and visibility gates pass.
- Public-hygiene and documentation validation results are recorded exactly.
- The independently red exact-head CI result remains visible and is not conflated with the wiki gate.

## Decisions

- 2026-08-28: Establish the cited code map before adding a consumer or shared knowledge protocol.
- 2026-08-28: Keep generated pages as reviewable evidence rather than operational truth.
- 2026-08-28: Use the existing generic `ArtifactEnvelope` only in a later read-only consumer slice.
- 2026-08-28: Require explicit source and producer receipts because calendar freshness alone does not identify the indexed tree.
- 2026-08-28: Treat the structural wiki gate as necessary but not sufficient for public publication.
- 2026-08-28: Use external proposals only as private review input; do not copy their provenance or policy into public artifacts.
- 2026-08-28: Use precise agent vocabulary: IMPULSE is a local control plane and harness manager around model-owned loops, with Ion as its native runtime. Environment access is not authorization, and orchestrator, Supervisor, verifier, and operator remain distinct roles.

## Changes

- Generated an initial source-cited Rust workspace baseline under `impulse-rs/openwiki/`.
- Added an exact source and producer receipt boundary.
- Added a maintainer-owned instruction boundary subordinate to repository authority.
- Corrected narrow workspace/roadmap documentation drift without changing runtime semantics.
- Kept authority documents, Cargo metadata, task state, memory state, policy, and capabilities unchanged.

## Verification

Completed for the final candidate:

- Exact-generator structural and visibility gate: passed after a forced `init --visibility public` replay from the preserved release binary.
- Exact-generator scan: 360 files and 7,587 chunks; all seven planned pages reported `dirty=false`.
- Unchanged-source update: returned `no-op (inventory unchanged)`; all wiki bytes were identical before and after.
- Exact-binary replay byte identity: generated output first reproduced hash `f33f23ab5123322b31fda2e0bd2f986e8503159b5c19d47ea11c5d213ad08503`; after the human receipt clarification and Markdown EOF whitespace normalization, an unchanged-source update preserved the finalized complete-wiki hash `8a6f416417ece8c90c86635a65d986e58fb9cb52343c46a69bf204f8171e6080` byte-for-byte.
- Indexed-input check: every inventory path was tracked.
- Public hygiene: no machine path or private proposal identifier was found; `gitleaks detect --no-git --redact=20 --source impulse-rs/openwiki --config /dev/null` reported no leaks.
- Rust verification: `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, and `cargo test --workspace` passed locally. The full library segment reported 1,709 passed and five ignored tests; the remaining workspace, integration, and doc-test targets also passed.
- Transient-test audit: one daemon-session test failed only during concurrent ROSA/IMPULSE full-suite execution; it passed alone with one test thread, and the complete IMPULSE workspace rerun then passed. No product code was changed to mask the shared-process race.
- Repository diff checks: `git diff --check` passed and the generator created no nested `AGENTS.md`, `.staging/`, or `.store/` artifact.
- Documentation validator: red on one pre-existing metadata issue only—`decisions/0014-work-item-and-comparative-settlement.md` uses unsupported status `proposed`. This branch does not modify that file.
- Independent review: no blocker; citation integrity, architecture, Markdown, authority order, receipt hashes, public visibility, one-H1/opening structure, source links, six-crate mapping, and exclusion of private proposal provenance all passed.
- Hosted CI boundary: no exact-head hosted result for source commit `d8b35da77ac7ea61a47b901db5ec0ab4824fc9c6`; the nearest exact-base result remains the independently red run documented above.

## Handoff Notes

Do not treat this documentation baseline as product or release readiness. It
remains subordinate to the canonical contract, live Rust implementation, and
independently executed hosted CI.

If the documentation baseline is later accepted, the next product slice is a
versioned read-only wiki snapshot artifact with project, tree, and dirty-state
validation plus review-only actions.
