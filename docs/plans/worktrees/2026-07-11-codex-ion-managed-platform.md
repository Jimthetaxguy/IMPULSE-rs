---
title: Ion managed-platform integration
description: Register Ion in Impulse's canonical agent catalog and desktop lifecycle plane
updated: 2026-07-11
type: doc
category: planning
phase: all
status: draft
audience: builders
tags: [worktree, ion, agent-registry, desktop, mcp]
---

# Ion managed-platform integration

## Lane facts

- Owner: Codex (`codex/ion-managed-platform`)
- Worktree: `.worktrees/ion-managed-platform`
- Dependency base: the current verified daemon-truth forward-port is `dbac22f` + `ae8fcd0`; implementation activates from their rebased equivalents on Claude's next clean main tip.
- Goal: make the already-real Ion CLI selectable, launchable, detectable, and observable through the same canonical manager surfaces as other terminal agents.
- Owned paths:
  - `impulse-rs/impulse-ops/src/agent_registry.rs`
  - `impulse-rs/impulse-desktop/src/runtime.rs`
  - `impulse-rs/impulse-desktop/src/ui.rs`
  - focused desktop/MCP/bridge tests if the registry-driven surface needs explicit proof
  - this work card, this lane's working note, and the narrow `CONTEXT.md` vocabulary update
- Blocked/shared paths: Claude completed Ion T1-T9 plus env scrubbing, bounded tool loops, ApprovalGrant, and FileWrite guardrails through `a5184e2`; those implementation paths, `CLAUDE.md`, and Cargo manifests remain outside this lane. Claude's current daemon-agent concurrency work must stabilize before this stacked manager lane is integration-ready.

## Acceptance criteria

- `AgentRegistry::builtin()` exposes an `ion` descriptor with the real `ion` command and no invented headless arguments.
- The desktop wire type has a stable `ion` slug/label and resolves its launch command through the canonical registry.
- Opening an `ion` terminal detects Ion rather than Shell, without classifying unrelated words containing `ion` as Ion.
- The Dioxus workspace launcher presents Ion and parses it without the old unknown-to-Codex fallback.
- MCP platform listing includes Ion automatically from the canonical registry.
- Serde/host-bridge round trips preserve the new registry-backed platform ID.
- Claude-owned dirty paths remain untouched.

## Verification contract

- Focused `impulse-ops` registry tests.
- Focused `impulse-desktop` runtime/UI/MCP/host-bridge tests.
- `cargo check --workspace`.
- `cargo clippy --workspace -- -D warnings`.
- `cargo fmt --all -- --check`.
- Workspace tests, isolating only documented pre-existing/environmental failures.
- Independent generic-agent and MiniMax adversarial review before commit.

## Audit outcome

- Deferred behind the desktop-to-daemon truth wire. Adding Ion as a seventh closed
  `AgentPlatformKind` would deepen the mismatch with the runtime-extensible registry.
- Ion is now a real coding-agent surface with T9 tool calling and guarded mutating tools;
  the remaining gap is manager identity/lifecycle integration, not Ion capability.
- Before this card becomes implementation-active, platform identity must become
  registry-driven, command detection must stop using unsafe substring matching, and
  the actual `ion` executable must be resolvable from the desktop launch environment.
- The legacy context/harness enums remain a separate compatibility follow-up; they are
  not required for generic PTY management but are required for Ion-specific parsing.
