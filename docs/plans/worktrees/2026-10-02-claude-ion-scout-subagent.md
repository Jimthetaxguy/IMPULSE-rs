---
title: Ion scout — disposable min-agent subagent tool
description: Work card for ion-scout-subagent-20261002
updated: 2026-10-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Ion scout — disposable min-agent subagent tool

## Lane Facts
- Owner: claude (Opus 5.5, interactive session with James)
- Role: implementer
- Branch: `claude/ion-scout-subagent-20261002`
- Worktree: `../IMPULSE-rs.wt-scout`
- Owned paths: `impulse-rs/src/ion_repl/tool_scout.rs`, this card,
  `docs/superpowers/specs/2026-10-02-ion-scout-subagent-design.md`
- Shared paths edited: `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`,
  `impulse-rs/src/ion_repl/{mod,registry,chat}.rs`, `impulse-rs/src/llm_backends/anthropic.rs`
  (new `anthropic_api_origin` helper), `CONTEXT.md`
- Plan/spec: `docs/superpowers/specs/2026-10-02-ion-scout-subagent-design.md`
- Verification: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo check --all-targets --no-default-features --features office-support`
- Latest status: implemented; see Handoff Notes for gate evidence

## Decisions
- 2026-10-02: in-process library call, not the `min-agent` CLI. The public `min-agent` 0.2.0 API
  (`Connection`, `ModelProfile`, `HttpModelClient`, `Workspace`, `run`, `Budget`, `ModelClient`)
  is enough, so `min-agent-rs` needs no change. Crash isolation via subprocess is a later option.
- 2026-10-02: `min-agent` is an optional git dependency pinned to `56411f0`, behind the default
  `scout-subagent` feature.
- 2026-10-02: spend is capped per session (5 runs) instead of a confirmation prompt; the tool has
  no side effects. Refused calls do not consume a slot.
- 2026-10-02: scout budget is a static half of `ION_DEFAULT_WALL_CLOCK`. Ion tools do not receive
  the parent loop's remaining time; carving from it is a follow-up.

## Handoff Notes
- Gate on this branch (2026-10-02, isolated `CARGO_TARGET_DIR`): `cargo test --workspace` 3127
  passed, 0 failed, 10 ignored across 35 suites (`main` baseline: 3112 / 0 / 9; +15 scout tests,
  +1 ignored live test); `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all -- --check` clean; `cargo build --workspace` clean;
  `cargo check --all-targets --no-default-features --features office-support` clean.
  `docs/validate_docs.py --all` has no metadata errors from this lane; its contract check still
  fails on `main`'s pre-existing stale-document warnings.
- New lockfile crates: `min-agent`, the `cap-std` family (`cap-primitives`, `ambient-authority`,
  `fs-set-times`, `io-extras`, `io-lifetimes`, `maybe-owned`, `winx`, `rustix-linux-procfs`),
  and reqwest's rustls stack (`quinn*`, `webpki-roots`, `lru-slab`, `rand_pcg`) because
  `min-agent` builds reqwest with `rustls-tls`.
- No live provider round trip yet: `ANTHROPIC_API_KEY` was not set in the session. The loop is
  proven against `min-agent`'s real `run` and `Workspace` with a scripted `ModelClient`.
- Follow-ups: live scout round trip; parent-deadline carving; stdio/MCP surface for external
  harnesses (needs `min-agent` to lift its MCP deferral); optional parallel scouts (ADR-0014).
