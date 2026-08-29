---
title: Codex Rust code wiki baseline lane
description: Generate and verify a source-cited code wiki for a pinned clean IMPULSE origin/main snapshot.
updated: 2026-08-28
type: doc
category: planning
phase: all
status: active
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
- Owned paths: `impulse-rs/openwiki/**`, two source-truth-only crate comments, and this work card
- Shared/blocked paths: authority documents, runtime semantics, Cargo manifests and lockfiles, daemon/UI/task/memory/retrieval code, and every concurrent lane
- Plan/spec: this work card; the wiki is documentation evidence, not a product interface or authority change
- Latest status: baseline generated and independently reviewed; public commit remains blocked on reviewed regeneration and final gates

## Snapshot and Producer Receipt

- Source scope: tracked inputs under the repository-relative `impulse-rs/` Rust workspace
- Source commit: `a611e06b82539c0e7c3d9edb7fde9905ce4380ac`
- Source tree: `885ecc1f8117134df63c64b37abd73500d8a0548`
- Source state: clean input snapshot; uncommitted canonical-checkout changes were excluded
- Generator basis: QuireWiki `f06482f8d837b678802b348ed00f61ec842289fc`; final reviewed-regeneration receipt is pending
- Generation mode: code, extractive claims only, no LLM or provider call

The canonical IMPULSE checkout has preserved uncommitted step-model and
history-test work. This lane starts from the exact clean remote snapshot and
must not copy, mask, or modify that WIP. The final receipt will identify the
source commit/tree, generator commit, producer hash, inventory hash, and source
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

## Changes

- Generated an initial source-cited Rust workspace baseline under `impulse-rs/openwiki/`.
- Added an exact source and producer receipt boundary.
- Added a maintainer-owned instruction boundary subordinate to repository authority.
- Corrected two stale crate-level comments without changing runtime semantics.
- Kept authority documents, Cargo metadata, task state, memory state, policy, and capabilities unchanged.

## Verification

Completed for the first candidate:

- Exact-generator structural gate: passed.
- Exact-generator scan: all planned pages reported clean.
- Indexed-input check: every inventory path was tracked.
- Public-hygiene scan: no secret or personal-identifier finding in generated pages.
- Independent review: citation integrity passed; architecture, Markdown, authority-order, provenance-display, and public-card corrections were required.

Still required before commit:

- Regenerate with the reviewed Markdown and architecture planner.
- Record the final source and producer receipt.
- Verify an unchanged-source update is a no-op or byte-identical.
- Run repository documentation validators and candidate-text hygiene scans.
- Re-run the exact-generator gate, scan, rendered-Markdown review, and public-hygiene checks.

## Handoff Notes

Do not commit the current candidate bytes as a public baseline until the required
corrections pass independent review.

If the documentation baseline is later accepted, the next product slice is a
versioned read-only wiki snapshot artifact with project, tree, and dirty-state
validation plus review-only actions.
