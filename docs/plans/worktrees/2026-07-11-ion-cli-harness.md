---
title: Ion CLI harness — spec-a contract crate + Rust adapter for the Pi verification gate
description: Work card for the ambient /loop building impulse-ion and wiring it to the promoted Pi (MiniMax) harness
updated: 2026-07-11
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, ion, harness, verification-gate, minimax, pi]
---

# Ion CLI harness — spec-a contract crate + Rust adapter

## Lane Facts
- Owner: Claude Code (ambient /loop session)
- Role: Build the Ion harness boundary inside impulse-rs per the design-lane specs at
  `~/.ai-memory/docs/ion-harness/spec-{a,b,c}-*.md`. Spec-a defines the contract; spec-b
  records that TS Pi (MiniMax-backed) is already bring-up-complete and promoted as
  Ion harness #2 outside this repo (`~/.ai-memory/docs/ion-harness/pi-gate/`). This lane's
  job is the Rust-side adapter that speaks spec-a `HarnessRequest`/`HarnessResponse`
  and drives that already-promoted `pi --mode rpc` process — i.e. give Impulse (the
  operator cockpit) an in-process way to call the verification gate.
- Branch: main (no dedicated worktree — additive new crate + module, no shared-file
  structural refactor)
- Worktree: repo root
- Owned paths:
  - `impulse-rs/impulse-ion/` (new crate — contract types, landed in ae4e87d)
  - `impulse-rs/impulse-ion/src/pi_adapter.rs` (new — RPC adapter, this lane)
  - `docs/plans/worktrees/2026-07-11-ion-cli-harness.md` (this card)
- Blocked/shared paths:
  - `impulse-rs/Cargo.toml` workspace members list — append-only touch (adding the
    `impulse-ion` member), no reordering/removal of existing members.
  - `impulse-rs/src/**` (main daemon/CLI) — not wired into the CLI yet in this lane;
    that is a follow-on once the adapter itself is proven against a live `pi` process.
  - `impulse-gui` (frozen legacy) — untouched.
- Plan/spec:
  - `~/.ai-memory/docs/ion-harness/spec-a-harness-contract-v0.md` (contract, done — impulse-ion crate)
  - `~/.ai-memory/docs/ion-harness/spec-b-pi-gate-bringup.md` (Pi bring-up status + 3 recorded deviations: no bash for the verifier process itself is superseded by a tool_call interceptor, MiniMax-M2.7 not M2.5, lenient JSON parsing needed)
  - `~/.ai-memory/docs/ion-harness/pi-gate/launch-gate.sh` — the exact invocation this adapter must reproduce in Rust (Node 22 pin, MiniMax key sourced from `~/.local/share/opencode/auth.json`, `--tools read,grep,find,ls,bash` + `bash-gate-ext.ts` interceptor, `--no-session`)
- Verification: `cd impulse-rs && cargo build -p impulse-ion && cargo test -p impulse-ion && cargo clippy -p impulse-ion -- -D warnings && cargo fmt --check -p impulse-ion`, plus a live smoke test spawning the real `pi --mode rpc` process on a trivial diff (read-only — no repo mutation).
- Latest status: spec-a contract crate landed (commit ae4e87d, 6 tests, clippy-clean).
  Next increment: `pi_adapter.rs` — spawn `launch-gate.sh --mode rpc`, write one
  `HarnessRequest` JSON line to stdin, read one `HarnessResponse` JSON line back
  (with the lenient-parsing normalization spec-b §status note (3) calls out — Pi has
  no `--json-schema` enforcement).

## Decisions
- 2026-07-11: Do not re-implement the Pi bring-up (spec-b) — it is already B5-complete
  and promoted outside this repo. This lane only builds the Rust caller side.
- 2026-07-11: Reuse `launch-gate.sh` unchanged as the process entrypoint rather than
  re-deriving the Node22/MiniMax-key/extension flags in Rust — one launcher, no drift,
  per spec-b's own "one gate implementation, zero drift" principle.
- 2026-07-11: Adapter wiring into the main Impulse CLI/daemon (`src/**`) is deliberately
  deferred to a follow-on lane once `pi_adapter.rs` round-trips against a live process;
  keeps this increment small and independently verifiable.

## Follow-ups noted, not actioned this lane
- 9 stale worktrees under `.claude/worktrees/wf_31361071-0c4-*` (from a prior Workflow
  run, all behind `main`) are disk/clutter candidates but not touched here — need a
  human or a dedicated cleanup lane to confirm nothing in them is unmerged/wanted
  before `git worktree remove`.
