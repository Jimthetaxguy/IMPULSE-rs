# IMPULSE OpenWiki Evidence Instructions

This file is human-owned. `quirewiki init` and `quirewiki update` must preserve
it.

## Authority Order

Before using this wiki, read the repository [agent guidance](../../AGENTS.md),
[project context](../../CONTEXT.md), and
[canonical Rust contract](../../docs/spec/RUST-CANONICAL-CONTRACT.md).

Those sources and the live Rust implementation outrank every generated page.
Generated content is partial, non-authoritative evidence. It never owns daemon,
task, memory, policy, capability, approval, or acceptance truth.

## Public Baseline Receipt

- Source scope: indexed inputs under the repository-relative `impulse-rs/` Rust workspace
- Source commit: `d8b35da77ac7ea61a47b901db5ec0ab4824fc9c6`
- Source tree: `50839f3ee2f0cd275be5c670248a51a06c4358e6`
- Source state: isolated branch snapshot; preserved canonical-checkout WIP is excluded
- Generator commit: QuireWiki `de833b93a60694117de438600b63e4cda58c8d34`
- Generator tree: `f7332b5f7f9d700497c60f16c946bee249ef472c`
- Producer binary SHA-256: `07571c145a5291d901419421b1b336a149d36f3217be6fbe6148a464b7c0985c`
- Source inventory: 360 indexed files, 7,587 chunks, hash `12956dfaf51b7b7949c262a33541611109cd2f8bba1d576b717d90a7ff5898c5`
- Artifact inventory: seven generated content pages, one generated index, this human-owned instruction file, and the generated `.last-update.json` scan receipt
- Generation mode: code, deterministic extractive claims only, no LLM or provider call
- Source evidence retrieval time: `2026-08-29T03:23:15Z` (preserved because the evidence was unchanged)
- Exact-binary replay time: `2026-08-29T03:41:02Z`
- Visibility policy: `public`

The producer hash identifies the executable used; it is not a
reproducible-build claim. Omitted files, APIs, and architecture areas are not
evidence that they do not exist.

No architecture page is emitted because the scanned nested Rust workspace has
no top-level `ARCHITECTURE.md`. Root context and the canonical contract remain
outside this generated scan scope and must be consulted directly.

## Claims and Citations

Every generated prose claim must carry a repository-relative `(path:line)`
citation and a source extract that still matches that span. Tables and fenced
blocks retain visible source-span attribution without manufacturing prose
claims. Open the cited source before calling or changing an API.

Prefer public APIs and crate boundaries. Private helpers, tests, archives, and
vendor assets must not be presented as current product architecture.

## Gate and CI Boundary

`quirewiki scan` is the read-only, zero-LLM preview. `init` and `update` stage
candidate output and publish it only after audience-policy and structural gates
pass.

A passing wiki gate verifies generated metadata, Claim/Extract bindings,
citations, schemas, links, and path safety. It does not prove architecture
completeness, repository tests, security, release readiness, or CI health. No
exact-head hosted CI result for this documentation branch has been verified;
the wiki gate does not supersede or mask that boundary.

## Proposal and Update Boundary

External proposal packages were private review inputs only; none of their
content, names, machine-local provenance, trust tiers, fixtures, or policy was
copied or ingested into this public baseline.

- Preserve this file unless a maintainer intentionally updates its policy or receipt.
- Refresh the receipt whenever the source or producer changes.
- Do not automatically edit authority documents or product code.
- Do not promote wiki output into task, memory, policy, capability, approval, or acceptance state.
