---
title: Monty sandbox replaces python3 -c for calculator and python_exec
description: Work card for monty-calculator-20260926
updated: 2026-09-26
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Monty sandbox replaces python3 -c for calculator and python_exec

## Lane Facts
- Owner: claude (Fable 5.1 subagent running an overnight slice for James)
- Role: implementer
- Branch: `agent/claude-monty-calculator-20260926`
- Worktree: `.worktrees/monty-calculator-20260926`
- Owned paths: `impulse-rs/src/tools/python.rs`, `impulse-rs/src/tooling/builtin/calculator.rs`,
  `impulse-rs/src/tooling/builtin/python_exec.rs`, `impulse-rs/src/tooling/builtin/build_health.rs`,
  `impulse-rs/src/handlers/system.rs`, `impulse-rs/src/tools/health.rs`, `impulse-rs/src/tools/system.rs`,
  `docs/decisions/0021-monty-sandbox-for-calculator-and-python-exec.md`, this card
- Blocked/shared paths edited under the requester's explicit instruction: `impulse-rs/Cargo.toml`,
  `impulse-rs/Cargo.lock`, `HANDBOOK.md`, `CONTEXT.md`, `docs/decisions/README.md`, `docs/INDEX.md`,
  `docs/SUMMARY.md`, `docs/SUMMARY.yaml`
- Plan/spec: ADR-0021
- Verification: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
  `cargo test --no-default-features --lib`, `python3 docs/validate_docs.py --all`
- Latest status: implemented and verified on 2026-09-26; full gate green (see Handoff Notes); branch pushed for James's review, no PR opened

## Decisions
- 2026-09-26: use the in-process `monty` crate with zero host functions, mounts, or inputs. `monty-pool`
  (subprocess workers, crash isolation) is the follow-up, not this slice.
- 2026-09-26: `rust-version` moves from 1.82 to 1.96 because `monty` 1.0.0 requires it. CI and release
  workflows already use `dtolnay/rust-toolchain@stable`, so no workflow change is needed.
- 2026-09-26: the unused optional `datafusion` requirement moves from 43 to 55. Its arrow 53 tree capped
  `chrono < 0.4.40`, and `monty` needs chrono 0.4.40 or newer. No code imports `datafusion`.
- 2026-09-26: the calculator keeps PR #64's math-only input restriction. Redundant under Monty, harmless,
  and removing it is a separate decision.

## Changes
- `impulse-rs/src/tools/python.rs`: `execute_python`/`execute_python_with_timeout` run code in the in-process
  `monty` interpreter (no host functions, mounts, or inputs); `PythonResult` gains `fault`
  (`syntax`, `unsupported`, `runtime`, `timeout`, `memory`, constants in `tools::python::fault`); limits 64 MiB heap,
  5 s wall clock, 16 MiB print cap, `time.sleep` returns at once; a Rust panic inside the interpreter becomes
  `Err`; the `python3` spawn, the version/availability subprocess probes, and the unused `execute_script` are gone.
- `tooling/builtin/python_exec.rs`: sandbox description, `fault` in the JSON result, `timeout` param honored only
  as a shorter budget (default 5). `tooling/builtin/calculator.rs`: sandbox description; PR #64's math-only
  restriction kept. `handlers/system.rs`: removed the "install Python 3" preflight. `tooling/builtin/build_health.rs`:
  probes host `python3` on PATH itself. `handlers/direct_dispatch.rs`: Calc/Exec tests assert `Ok`.
- `impulse-rs/Cargo.toml`: `rust-version` 1.82 -> 1.96; `monty`/`monty-types` 1.0.0 (`tzdb`); unused optional
  `datafusion` 43 -> 55. `Cargo.lock`: chrono 0.4.39 -> 0.4.45, uuid 1.21.0 -> 1.26.1, new monty/ruff/arrow trees.
- Docs: ADR-0021 plus index rows (`docs/decisions/README.md`, `docs/INDEX.md`, `docs/SUMMARY.md`, `docs/SUMMARY.yaml`);
  `HANDBOOK.md` no longer marks the PyO3 `src/monty/` stub Complete; `CONTEXT.md` gains the Python sandbox term.

## Tests
- Red first: the ten new sandbox tests were added against the CPython path; eight failed for the intended reasons
  (CPython executed `subprocess.run`, read `/etc/passwd`, ran `os.system`, allocated the 200M-element list; timeouts
  were `Err`, syntax/runtime faults were `None`). Then the Monty implementation turned them green.
- `cargo test` (default features): 2467 passed, 0 failed, 7 ignored.
- `cargo test --no-default-features --lib`: 2213 passed, 0 failed, 5 ignored.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, `cargo clippy --all-targets -- -D warnings`,
  `cargo clippy --workspace --all-targets -- -D warnings`: all clean.
- End-to-end probes through the built `impulse-rs exec`/`calc` binary: subprocess, open, `__import__`, `os.getenv`,
  socket, memory, time, and syntax all fault with no host effect; `print(2 + 2)` and `calc "2 + 2"` succeed.

## Handoff Notes
- Gate exit codes: fmt rc=0; clippy-ci rc=0; clippy-all rc=0; test-default rc=0; test-nodefault rc=0; clippy-ws rc=0; docs-validate rc=1. `docs-validate` fails only on the three stale documents that already fail on `main`
  (`docs/guides/COLLABORATIVE-AGENTIC-CODING.md` and two May 2026 work cards); this lane adds none.
- Verification ran in an isolated `CARGO_TARGET_DIR` because seven cargo processes from other sessions held the
  shared `~/.cargo-target` build-directory lock during this session.
- Open follow-ups are listed in ADR-0021: `monty-pool` crash isolation, remove or build the PyO3 stub, close
  `agent/grok-calculator-math-only-20260917`, revisit the `security-framework` pin, report monty's loose chrono
  floor upstream, fix the `docs/CLI-COMMANDS.md` claim that `monty-support` gates `calc`/`exec`.
- Run record: `~/code/_working-files/20260926-003000-claude-impulse-monty-calculator-run.md`.
