# T5 Implementation Notes — `ion` binary skeleton

> Historical implementation plan. T5 has landed: the workspace now builds the
> `ion` binary and exercises its command surface. The detail below preserves
> the pre-implementation design record and must not be read as current status.

**Status:** implemented; historical dependency analysis follows (T5 originally depended on T3's `run_ion_verify` split, while T4 supplied the behavioral fake-gate coverage).
**Spec:** `impulse-rs/impulse-ion/TUI_SPEC.md` §2.2, §2.4, task T5.
**Originally written:** 2026-07-11 as a read-only pre-implementation spec session.

Current state verified against the repo as of this note:

- `impulse-rs/Cargo.toml` already has an **explicit** `[[bin]] name = "impulse-rs", path = "src/main.rs"` (lines 20–22). It is a pure binary crate (no `[lib]`), workspace root with members `impulse-{desktop,ion,ops,term}`.
- `Commands::IonVerify` lives at `src/cli.rs:594` with flags `--repo <Option<String>>`, `--diff-ref <String, default "HEAD~1..HEAD">`, `--description <String, default "Verify the pending diff.">`, `--json <bool>`.
- Dispatch: `src/handlers/direct_dispatch.rs:562` → `handlers::ion::handle_ion_verify(repo, diff_ref, description, json).await`.
- `handlers/ion.rs` today is the monolithic `handle_ion_verify` (builds request, spawn_blocking adapter, prints, `std::process::exit(1)` on failure). T3 splits it into pure `run_ion_verify(...) -> Result<HarnessResponse>` + a CLI wrapper that keeps print/exit behavior.
- Integration tests (`tests/*.rs`) use plain `std::process::Command` spawning `cargo run -- -c <impulse_dir> ...` from `env!("CARGO_MANIFEST_DIR")`; no `assert_cmd`/`escargot` anywhere; `DaemonGuard` RAII exists but is daemon-only (irrelevant to `ion`).

---

## 1. Cargo.toml change

Add a second `[[bin]]` block directly below the existing one. Nothing else needs restructuring — a crate may have any number of `[[bin]]` targets sharing the same `src/`; the explicit `impulse-rs` bin entry already present means we just add a sibling. Note: because the crate has **no `[lib]` target**, `src/bin/ion.rs` cannot `use impulse_rs::...` — it must reach shared code via a `#[path]` module include or by calling only what it declares itself (see §2 resolution R2).

```toml
[[bin]]
name = "impulse-rs"
path = "src/main.rs"

[[bin]]
name = "ion"
path = "src/bin/ion.rs"
```

(Keep the existing `impulse-rs` block as-is; just append the `ion` block after it, before `[package.metadata.bundle]`.)

No new dependencies for T5. `rustyline` arrives in T6, not here.

Sanity check after edit: `cargo build --bins` produces both `target/debug/impulse-rs` and `target/debug/ion`.

## 2. `src/bin/ion.rs`

Design decisions resolved (spec was silent):

- **R1 — clap shape:** `Option<Command>` subcommand on the top-level parser, so bare `ion` parses cleanly (no subcommand required) and `ion verify` is a subcommand. This mirrors `claude`'s "bare = interactive" surface and leaves `None` as the slot T6 replaces with the REPL.
- **R2 — module path to `run_ion_verify`:** `impulse-rs` is a binary-only crate (no lib target), so `src/bin/ion.rs` **cannot** do `use impulse_rs::handlers::ion::run_ion_verify`. Two options:
  - (a) *Chosen:* keep the T3 wrapper logic reachable by having `ion.rs` call `impulse_ion` + a small amount of local glue — **rejected**, it would duplicate handler logic.
  - (b) *Actually chosen:* add a minimal `[lib]` target is the clean long-term fix, but it is a bigger change (main.rs declares ~40 `pub mod`s; a lib split touches every module). For T5, use **`#[path]` includes** of only the modules the wrapper needs — also fragile because `handlers/ion.rs` pulls in `handlers::common::print_json` etc.
  - **Final resolution:** promote the crate to lib+bin the *minimal* way — add `src/lib.rs` that is just `pub mod ...;` re-exports moved out of `main.rs`, and slim `main.rs` to `use impulse_rs::...` + `main()`. This is the standard Cargo pattern, it is mechanical, and CLAUDE.md's "Quick health check" note ("impulse-rs is a binary crate (no lib target), so use --bins not --lib") must be updated when it happens. **If the T5 implementer wants to defer the lib split**, the fallback that compiles today is `#[path = "../handlers/mod.rs"] mod handlers;` — but that recompiles the module tree into the second bin and drags in the whole handler surface; prefer the lib split. Flag this in the PR description either way.
- **R3 — flag parity:** `ion verify` takes the *same four flags* as `impulse-rs ion-verify` (`--repo`, `--diff-ref`, `--description`, `--json`) with identical defaults, and the same exit-code convention (non-zero on gate failure), because spec §2.4 says "one-shot, same flags/exit codes".
- **R4 — banner:** plain single-purpose text + exit 0. No color, no deps. Includes the version from `CARGO_PKG_VERSION` so the banner test has something stable to assert on. Text chosen: `ion — Ion interactive harness (REPL coming soon; try 'ion verify --help')`.
- **R5 — no `--impulse-dir`/`--daemon`/`--format` globals:** the `ion` binary does NOT inherit `Cli`'s global flags. It is a fresh, minimal surface; verify doesn't need `.impulse/` state. T6 adds `IMPULSE_HOME` handling for history only.

Copy-paste skeleton (compiles only after T3 lands and the lib split from R2 is done; adjust the `use` line to the final path — spec names the function `run_ion_verify` in `handlers::ion`):

```rust
//! `ion` — Ion harness binary (TUI_SPEC.md T5 skeleton).
//!
//! Bare `ion` prints a placeholder banner (T6 replaces this with the readline
//! REPL). `ion verify` is a one-shot gate run sharing `run_ion_verify` with
//! `impulse-rs ion-verify` — same flags, same exit-code convention.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ion", version, about = "Ion interactive harness", long_about = None)]
struct IonCli {
    #[command(subcommand)]
    command: Option<IonCommand>,
}

#[derive(Subcommand)]
enum IonCommand {
    /// Run the Ion verification gate (harness #2 — Pi on MiniMax) against a diff
    Verify {
        /// Repository path to verify (defaults to the current directory)
        #[arg(long)]
        repo: Option<String>,
        /// Git ref range to verify, e.g. HEAD~1..HEAD
        #[arg(long, default_value = "HEAD~1..HEAD")]
        diff_ref: String,
        /// Task description passed to the gate
        #[arg(long, default_value = "Verify the pending diff.")]
        description: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = IonCli::parse();
    match cli.command {
        None => {
            print_banner();
            Ok(())
        }
        Some(IonCommand::Verify {
            repo,
            diff_ref,
            description,
            json,
        }) => {
            // T3's thin CLI wrapper: runs the pure run_ion_verify, prints the
            // verdict (text or --json), and maps !response.passed() /
            // contract violation to process exit 1. Reuse it verbatim so the
            // two binaries cannot drift.
            impulse_rs::handlers::ion::handle_ion_verify(repo, diff_ref, description, json).await
        }
    }
}

fn print_banner() {
    println!(
        "ion {} — Ion interactive harness (REPL coming soon; try 'ion verify --help')",
        env!("CARGO_PKG_VERSION")
    );
}
```

Wrapper choice: delegate to the **CLI wrapper** (`handle_ion_verify` post-T3), not the pure `run_ion_verify`, so exit-code + `--json` + contract-violation printing stay in exactly one place. If T3 renames the wrapper, follow the rename. Do not re-implement the verdict→exit mapping in `ion.rs`.

Note `tokio::main`: needed because the wrapper is `async` (spawn_blocking inside). Matches `src/main.rs`.

## 3. Integration test — `tests/ion_binary.rs`

Conventions matched from `tests/integration_enhancements.rs`: raw `std::process::Command`, `CARGO_MANIFEST_DIR`-anchored cwd, `String::from_utf8_lossy` helpers, no external test crates. One deviation, deliberate: use `env!("CARGO_BIN_EXE_ion")` (stable since Rust 1.43, well under the crate's 1.82 MSRV) instead of nested `cargo run --bin ion --`. Cargo builds all bins before running integration tests, so the env var always points at a fresh binary, and it avoids cargo-inside-cargo lock contention that the existing daemon tests tolerate but a fast smoke test shouldn't pay for. `DaemonGuard` is not relevant — no daemon involved.

```rust
//! Integration tests for the `ion` binary skeleton (TUI_SPEC.md T5).
//!
//! Scope is deliberately tiny: CLI parsing + banner. Gate behavior is covered
//! by the T4 fake-gate tests against `run_ion_verify`; these tests must never
//! spawn the real Pi gate (no network, no launch-gate.sh).

use std::process::Command;

fn ion() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ion"))
}

#[test]
fn test_ion_bare_run_prints_banner_and_exits_zero() {
    let output = ion().output().expect("Failed to run ion binary");
    assert!(
        output.status.success(),
        "bare `ion` must exit 0, got {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ion") && stdout.contains("Ion interactive harness"),
        "banner missing from stdout: {stdout}"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "banner should include the crate version: {stdout}"
    );
}

#[test]
fn test_ion_verify_help_parses_and_lists_flags() {
    let output = ion()
        .args(["verify", "--help"])
        .output()
        .expect("Failed to run ion verify --help");
    assert!(
        output.status.success(),
        "`ion verify --help` must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--repo", "--diff-ref", "--description", "--json"] {
        assert!(stdout.contains(flag), "help must document {flag}: {stdout}");
    }
}

#[test]
fn test_ion_unknown_subcommand_fails_with_clap_error() {
    let output = ion()
        .arg("frobnicate")
        .output()
        .expect("Failed to run ion with bad subcommand");
    assert!(
        !output.status.success(),
        "unknown subcommand must be a parse error"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("frobnicate") || stderr.to_lowercase().contains("unrecognized"),
        "clap error should name the bad subcommand: {stderr}"
    );
}
```

Every test asserts observable behavior (CLAUDE.md test-quality bar); none touch the gate, so they are hermetic and fast.

## 4. Checklist for the implementing agent

1. Confirm T3 merged: `grep -n "pub async fn run_ion_verify\|fn run_ion_verify" src/handlers/ion.rs` — note the wrapper's final name/signature and adjust the `ion.rs` delegate call.
2. Decide/confirm the R2 lib-split status. If T1–T4 already introduced `src/lib.rs`, just `use impulse_rs::handlers::ion::...`. If not, do the mechanical lib split (move the `pub mod` list from `main.rs` into a new `src/lib.rs`, `main.rs` becomes `use impulse_rs::*`-style glue) as the first commit of T5, then update CLAUDE.md's "binary crate (no lib target) … use --bins not --lib" note.
3. Add the `[[bin]] ion` block (§1). `cargo build --bins`.
4. Add `src/bin/ion.rs` (§2).
5. Add `tests/ion_binary.rs` (§3).
6. Repo gate from `impulse-rs/`: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`. Integration test count goes 26 → 29; update the two CLAUDE.md count lines (Verification Gate + Pre-Commit Checklist) and the Architecture blurb if the lib split happened.
7. Do NOT touch `impulse-ion/` (contract crate stays pure — spec §4), and do NOT remove/alter `impulse-rs ion-verify`.

## Ambiguities resolved (summary)

| # | Question the spec left open | Resolution | Why |
|---|---|---|---|
| R1 | Required vs optional subcommand | `Option<IonCommand>` | bare `ion` must parse; `None` is T6's REPL slot |
| R2 | How `src/bin/ion.rs` reaches `handlers::ion` | Minimal lib+bin split (`src/lib.rs`) | crate has no lib target today; `#[path]` include is the fragile fallback |
| R3 | `ion verify` flag surface | Identical 4 flags/defaults to `IonVerify` | spec §2.4 "same flags/exit codes" |
| R4 | Banner content | name + `CARGO_PKG_VERSION` + REPL-coming hint | stable assertion target for the test |
| R5 | Inherit `impulse-rs` globals (`-c`, `--daemon`, `--format`)? | No | fresh minimal surface; verify needs no `.impulse/` state |
| R6 | Test harness style | `CARGO_BIN_EXE_ion` + std `Command` | matches repo's no-assert_cmd convention while avoiding nested-cargo cost |
