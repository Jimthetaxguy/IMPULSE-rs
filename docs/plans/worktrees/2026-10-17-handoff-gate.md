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
Local, 2026-10-17, stable toolchain (rustc 1.99 via `impulse-rs/rust-toolchain.toml`):

- `cargo test --lib handoff -- --test-threads=8` — 41 passed, 0 failed, 0 ignored. The filter also matches older handoff-named tests in delegation, injection, and orchestration; those passed too.
- `cargo test --lib ion_repl::registry::tests::test_with_defaults_registers_ion_verify` — 1 passed.
- `cargo clippy -p impulse-rs --lib --tests -- -D warnings` — exit 0.
- Touched Rust files checked with `rustfmt +1.83.0 --edition 2021 --check` (the stable 1.99 rustfmt rewrites unrelated files; those were left alone).
- `cargo test --workspace -- --test-threads=8` ran until `impulse-rs` lib. Packages that finished: impulse-desktop lib 176 passed; desktop_contract 90; host_surface 9; runtime 22 passed / 1 ignored; views_ssr 7; impulse-ion 23 passed / 1 ignored; impulse-ops lib 167; governed_producer_contract 8; governed_task_contract 5; memory_candidate_contract 5. `impulse-rs` lib: 2410 passed, 5 failed, 4 ignored. Cargo then stopped (fail-fast), so later packages did not run.
- Those 5 failures are pre-existing on this machine and are not in the handoff gate. They are governed-verification tests whose first command is `cargo fmt`. The verifier scrubs the child environment down to PATH/HOME/TERM/locale/temp. This VM keeps the toolchain in `RUSTUP_HOME=/usr/local/rustup`, which is not on that allowlist, so the child rustup exits with "no default is configured" before any crate command. A scrubbed `cargo fmt` from `/tmp` reproduces that error. The handoff tests are among the 2410 that passed.

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

## 2026-10-19 review repair lane
- Owner: Codex; role: repair the current PR findings in an isolated checkout.
- Branch: `agent/codex-handoff-review-fixes`, based on PR #96 at `dbf4df0`.
- Owned paths: handoff module, handoff tool, `impulse-step-model/examples/selector_baselines.rs` (format and Clippy single-pattern fix), this card; shared registration and manifests remain untouched.
- Acceptance: validate the exact normalized workspace used by dispatch; require read and write permission for project-local ledger state; retain each sender's URL evidence; serialize slow/live operations without age-based lock theft.
- Verification: focused regressions, workspace build/test/Clippy/format; record actual outcomes before commit.

- Repair decisions: the tool normalizes and validates one workspace path, preserving blank-input refusal; project-local state must be readable and writable before I/O. Fetch lookup uses sender plus normalized URL, re-keying existing schema-1 rows on load. The ledger lock uses the standard library OS lock on a stable file and releases on process exit; age does not revoke live ownership.
- Added regression coverage: denied ledger reads, external configured home, padded absolute workspace escapes, normalized allowed workspace, blank workspace, two senders sharing a URL, legacy fetch-row migration, aged live lock exclusion, and process-exit lock release.
- Existing CI lint drift in `selector_baselines.rs` was formatted; all-target Clippy additionally required replacing a single-pattern match with `if let`. Its four existing example tests passed.

- Final verification on the repair checkout: `cargo build --offline --workspace`, `cargo test --offline --workspace` (3,169 passed / 0 failed / 9 ignored including doc tests), `cargo clippy --offline --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` passed. `cargo test --offline --lib handoff -- --test-threads=8`: 50 passed / 0 failed / 0 ignored. `cargo test --offline -p impulse-step-model --example selector_baselines`: 4 passed / 0 failed.
- Ignored tests remain ignored; no live Keychain or provider-wire checks were enabled. Ready for a fast-forward push to existing PR #96 after verifying its remote head is unchanged. No merge or deployment.
