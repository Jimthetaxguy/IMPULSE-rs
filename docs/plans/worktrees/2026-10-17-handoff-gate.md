---
title: Handoff gate
description: Work card for the findings handoff gate and stale shared-artifact writes
updated: 2026-10-19
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Handoff gate

## Lane Facts
- Owner: cursor agent
- Role: implement the findings handoff gate
- Branch: `cursor/handoff-gate-4741`
- Worktree: `/workspace`
- Owned paths: `impulse-rs/src/handoff/`, `impulse-rs/src/tooling/builtin/handoff.rs`, this card
- Blocked/shared paths: `impulse-rs/src/lib.rs`, `impulse-rs/src/tooling/builtin/mod.rs`, `impulse-rs/src/ion_repl/registry.rs`, `impulse-rs/src/ion_repl/chat.rs`, `impulse-rs/src/orchestration/mod.rs` (registration and a doc pointer only)
- Plan/spec: this card and `impulse-rs/src/handoff/mod.rs`
- Verification: `cargo test --lib handoff` and `cargo test --lib ion_repl::registry::tests::test_with_defaults_registers_ion_verify` (cargo accepts one filter). Clippy: `cargo clippy -p impulse-rs --lib --tests -- -D warnings`.
- Latest status: verified locally; PR open for review, not merged

## Decisions
- 2026-10-17: A findings handoff is a typed `Claim` with a required `ClaimSource` (file, commit, or URL). Deserialization rejects a claim that omits `source`.
- 2026-10-17: Subject match is deterministic. The sender declares a subject key. The gate compares it to the file's `impulse-subject:` line, the commit's `Impulse-Subject:` trailer, or the URL host. No model call.
- 2026-10-17: A URL resolves only when the sender recorded a fetch (status and body digest) through the same tool. The gate does not fetch the network itself.
- 2026-10-17: Refusal is `state: pending`, `accepted: false`, with the failures and the next step. There is no warning acceptance.
- 2026-10-17: Two agents writing one shared artifact from the same read is not a citation failure. `read_shared` / `write_shared` on the same `handoff` tool refuse a write whose recorded read is not the current generation.
- 2026-10-17: `orchestration::write_handoff` stays a task-routing note. It does not accept findings.

## Changes
- `impulse-rs/src/handoff/` is the gate. `dispatch` is the library entry. The `handoff` dynamic tool is the agent entry, registered in `ToolRegistry::with_defaults` and bridged into the Ion REPL.
- Ledger: `.impulse/HANDOFF_GATE.json` (gitignored with the rest of `.impulse/`), written atomically.
- `orchestration::write_handoff` is unchanged except a doc pointer. It still writes a task-routing note.

## Tests
Local, 2026-10-19, stable toolchain (rustc 1.99 via `impulse-rs/rust-toolchain.toml`):

- `cargo test --lib handoff -- --test-threads=8` — 58 passed, 0 failed, 0 ignored. The filter also matches older handoff-named tests in delegation, injection, orchestration, and integration; those passed too. The gate module and `handoff` tool account for 44 of them.
- `cargo clippy -p impulse-rs --lib --tests -- -D warnings` — exit 0.
- `cargo clippy -- -D warnings` (the Lint job command, from `impulse-rs`) — exit 0.
- `cargo fmt --check` (the Lint job command, from `impulse-rs`) — exit 0. Touched handoff files are clean. The earlier Lint failure was `cargo fmt --check` rewriting `impulse-step-model/examples/selector_baselines.rs` under rustfmt 1.10.0-stable; that file was already formatted on this branch and was not edited again.
- The full workspace suite was not re-run for this repair. Governed-verification tests can still fail in a scrubbed environment because `RUSTUP_HOME` is not on the verifier's allowlist. Those tests were not changed.

What the new tests prove:

- `test_submit_refuses_claim_sourced_from_a_different_domain` — a claim whose subject is `billing.example.com` and whose only source is a recorded fetch of `https://auth.example.com/...` stays pending. The fix names both hosts. The ledger status is pending.
- `test_submit_accepts_clean_handoff_whose_source_matches_the_claim` — the same claim with a billing host is accepted, then `read` returns next step `none`.
- `test_write_shared_refuses_second_writer_based_on_a_stale_read` — two readers of generation 1; the first writer commits generation 2; the second writer, still based on generation 1 a few minutes later, is refused with `stale_read` and the artifact content is unchanged.
- File, commit, and missing-source cases: a missing file, a missing commit, a commit trailer for a different subject, a file `impulse-subject:` for a different subject, a URL with no recorded fetch, and a claim JSON object without `source` all stay pending. A matching file and a matching commit are accepted.
- A claim value cannot be deserialized without `source`. `WorkflowResult::pending` rejects an empty failure list. Resubmitting an accepted id does not replace it. A path containing `..` is refused before any read. A write with no recorded read is refused.

## Handoff Notes
- Scenario (b) is a stale-read check on `write_shared`, not a citation failure. `file_write` was left alone so ordinary file writes do not grow a generation protocol.
- The desktop MCP tool list was not extended. Agents reach the gate through the daemon tool registry and the Ion REPL, which already wrap `ToolRegistry`.
- The gate does not fetch URLs. The sender records status and body digest first.
- No daemon IPC endpoint, no change to `GOVERNED_TASKS.json` / `MEMORY.jsonl` / `GENOME.md`, no new crate dependency, no secrets, no `.env`.

## 2026-10-19 review repair
- Branch: `cursor/handoff-gate-4741` (PR #96). No new PR.
- The workspace string is trimmed once, then rejected when it is empty, contains `..` or NUL, or does not canonicalize to a directory. That `PathBuf` is the read-root check and the claim root for `git -C` and file reads. A file claim is refused when its canonical path leaves the workspace, and the resolved file is opened with `O_NOFOLLOW`.
- The ledger directory is `project_impulse_dir` (the repo `.impulse/`, including when Ion's `IMPULSE_HOME` points elsewhere). It must pass both the read-root and write-root checks.
- The lock file stores `pid` and a token. A contender reclaims it only when that pid is not alive. Drop deletes the file only when the token still matches. `git` subprocesses are killed after 30 seconds.
- Schema 2 stores fetches as sender, then URL. A schema 1 ledger is migrated on load, including a sender/URL composite key. Any other schema is a corrupt-ledger error that names schema 2 and how to fix the file.
- Regression tests cover a padded absolute workspace, a `..` workspace, a symlink that leaves the workspace, a ledger outside the read roots, an external `IMPULSE_HOME`, a live lock older than 10 seconds, two senders recording one URL, and schema 1 migration.
