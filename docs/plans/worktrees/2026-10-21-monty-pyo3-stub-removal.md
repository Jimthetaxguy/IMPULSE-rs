# Lane: remove the unbuilt PyO3 Monty stub

- Owner: Grok, asked by James on 2026-10-21 ("catch these up")
- Role: implementer
- Branch: `main` at `ff867b0`, even with `origin/main`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs` (only worktree)
- Goal: stop the tree from advertising a Python extension that does not compile. `calc` / `exec` already run in `src/tools/python.rs` through the `monty` 1.0 crate (ADR-0021).

## Owned paths

- `impulse-rs/Cargo.toml` (drop optional `pyo3` and the `monty-support` feature)
- `impulse-rs/Cargo.lock` (drop `pyo3` from the `impulse-rs` package dependency list only)
- `impulse-rs/src/monty/python.rs` (delete)
- `impulse-rs/src/monty/mod.rs`, `impulse-rs/src/monty/routing.rs`
- `CONTEXT.md`, `AGENTS.md`, `CLAUDE.md`, `HANDBOOK.md`
- `docs/CLI-COMMANDS.md`, `docs/decisions/0021-monty-sandbox-for-calculator-and-python-exec.md`
- `docs/CODE-REVIEW-SUMMARY.md`, `impulse-rs/impulse-ion/TUI_SPEC.md`

## Left alone

- Pre-existing uncommitted `impulse-rs/Cargo.lock` additions (`ashpd`, `rfd`, `windows-sys`, and the rest of that drift). `pyo3` stays in the lock because `jiter` depends on it.
- `src/monty/` keyword routing, KDB, and swarm.
- `src/tools/python.rs` and the `monty` / `monty-types` dependencies.
- `datafusion-support`.
- ADR-0021 follow-ups other than 2 and 6 (`monty-pool`, the math-only branch, the security-framework pin, the chrono floor).

## Acceptance

- No `monty-support` feature, no `pyo3` direct dependency, no `src/monty/python.rs`.
- `docs/CLI-COMMANDS.md` does not say that feature gates `calc` or `exec`.
- `cargo check -p impulse-rs --locked --offline` and the `monty` / `tools::python` lib tests pass.

## Verification

2026-10-21, from `impulse-rs/`:

- `cargo check -p impulse-rs --locked --offline` finished in 32.89s.
- `cargo test -p impulse-rs --locked --offline --lib monty::` — 15 passed.
- `cargo test -p impulse-rs --locked --offline --lib tools::python::` — 21 passed.

Full workspace `cargo test` / clippy was not run. Not committed.
