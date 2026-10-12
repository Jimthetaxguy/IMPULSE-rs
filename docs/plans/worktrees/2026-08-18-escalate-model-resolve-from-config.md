---
title: Escalate Model Resolve-From-Config Wiring
description: Work card for escalate-model-resolve-from-config
updated: 2026-08-18
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, step-model, adr-0015]
---

# Escalate Model Resolve-From-Config Wiring

## Lane Facts
- Owner: Cursor cloud agent
- Role: implementer
- Branch: `cursor/escalate-model-resolve-from-config-9eb3`
- Worktree: `/workspace`
- Owned paths: `impulse-rs/src/agent/mod.rs`; `impulse-rs/src/state/config.rs`; `impulse-rs/src/state/config_keys/`; `impulse-rs/src/handlers/agent.rs`; `impulse-rs/src/ui/types.rs`; `impulse-rs/src/ui/mod.rs`; `impulse-rs/src/daemon/handlers.rs`; `impulse-rs/src/integration_tests.rs`; `docs/decisions/0015-harness-owned-step-model.md`; this work card
- Blocked/shared paths: `Cargo.toml`; `Cargo.lock`; `AGENTS.md`; `CLAUDE.md`; protocol/spec docs; Origin; `impulse-rs/src/monty/routing.rs`; PRs 24–26; `bind_governed_step` / Ion / `handle_chat` expansion
- Plan/spec: close the #27 wiring gap. `impulse_agent_escalate_model` must reach CLI query and TUI through the same `resolve_from_config` constructor the daemon cache uses. Not a router.
- Verification: `cargo test -p impulse-rs --lib --locked resolve_from_config escalate_model step_model`; `cargo test -p impulse-rs --lib --locked`; `cargo fmt --all -- --check`; `cargo clippy -p impulse-rs --lib --locked -- -D warnings`
- Latest status: PR #32 opened (draft). Do not merge. CI green: Test (ubuntu-latest), Test (macos-latest), Lint, Build (release).

## Decisions
- 2026-08-18: Branch from current `main`. Do not merge. Do not stack on PRs 24–26. Do not invent a router.
- 2026-08-18: Add `impulse_agent_escalate_model` as an OptionalString State config key and pass it through `resolve_from_config`. Apply it in `ImpulseAgent::new` onto `HarnessStepContext.escalate_model`.
- 2026-08-18: Keep ADR-0015 policy: escalate only after Failed/Inconclusive verify or `VerificationFailed`. Stay on `current_model` otherwise. No `token_tracker`. Do not name a reason `Escalate`.
- 2026-08-18: Do not expand `bind_governed_step` to Ion/`handle_chat`. Constructor wiring only.

## Changes
- Added `impulse_agent_escalate_model` OptionalString State config key.
- `ImpulseAgentConfig` / `ImpulseAgent::new` copy the value onto `HarnessStepContext.escalate_model`.
- `resolve_from_config` takes the escalate model. CLI configure/status/query, TUI `TuiState::new`, and the daemon cache all pass the config key.
- ADR-0015 consequence line updated: wiring is no longer later work. Policy is unchanged.

## Tests
- `cargo test -p impulse-rs --lib --locked escalate`: 19 passed (constructor, CLI helper, TUI `TuiState::new`, existing AfterVerifierFailure policy).
- `cargo test -p impulse-rs --lib --locked step_model`: 19 passed.
- `cargo test -p impulse-rs --lib --locked resolve_from_config`: 8 passed.
- `cargo test -p impulse-rs --lib --locked`: 1701 passed, 4 ignored, 3 failed (pre-existing governed-verification fixtures; not this lane).
- `cargo clippy -p impulse-rs --lib --locked -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- GitHub Actions run 32106439814: CI / Test (ubuntu-latest), CI / Test (macos-latest), CI / Lint, CI / Build (release) all green.

## Handoff Notes
- On `main` after #27, `set_escalate_model` and `bind_governed_step` do not exist. The daemon cache already calls `resolve_from_config` with four args and never copies an escalate model. This lane closes that gap in the constructor rather than adding a daemon-only setter.
