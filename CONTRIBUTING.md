# Contributing To Impulse

Impulse accepts human and agentic contributions, but all work must stay scoped, traceable, and verified.

Start with:

- [`AGENTS.md`](AGENTS.md) for coding-agent rules.
- [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md) for the product contract.
- [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md) for lane, worktree, and handoff rules.

## Required Workflow

1. Inspect `git status --short`, current branch, and worktrees before editing.
2. Use a scoped plan/spec for non-trivial work.
3. Declare owned paths before working in parallel.
4. Keep a work card under `docs/plans/worktrees/<date>-<lane-slug>.md` for multi-session or parallel-lane work.
5. Run the required verification gate before claiming completion.

## Rules

- Never use `git add .` or `git add -A`; stage explicit files.
- Never run destructive cleanup without confirmation and an archive path.
- Never implement broad changes without a scoped plan/spec.
- Never claim work is complete until verification has run.
- Correct stale or conflicting active docs, or mark them superseded/deprecated/archive.
- Leave unrelated dirty files alone.

## Default Rust Gate

```bash
cd impulse-rs
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Documentation Gate

```bash
python3 docs/validate_docs.py --contract
python3 docs/validate_docs.py --all
```
