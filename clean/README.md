# Impulse-RS — clean branch (`clean/dioxus-pty-orchestrator`)

A terminal-native AI coding-agent harness built as a 5-crate Rust workspace.
This branch is a from-scratch re-architecture aligned with the canonical
contract in `docs/spec/RUST-CANONICAL-CONTRACT.md` and the conventions of the
sibling projects (ROSA, operator, elevenlabs-mcp-custom, pdf-inspector).

## What's here

| Crate | Purpose | LOC | Tests |
|---|---|---|---|
| `impulse-contracts` | Typed vocabulary (sessions, events, tools, IDs, errors) — no I/O, no async | ~1.3k | 29 |
| `impulse-workspace` | Per-folder project registry, with `touch` / `unregister` / `find_by_path` | ~600 | 15 |
| `impulse-runtime` | PTY orchestrator + session state + tool dispatcher + 5 backend adapters | ~1.8k | 21 |
| `impulse-mcp` | `rmcp 0.3` stdio server, 8 tools, in-memory event log | ~700 | 6 |
| `impulse-desktop` | Dioxus 0.6.3 web + desktop host with 4-view shell | ~500 | 8 |

**Total: 79 tests passing, `cargo clippy --workspace --all-targets` clean.**

## Architecture

```
   ┌─────────────────────────────────────────────────────────┐
   │                Dioxus 0.6.3 Shell (web/desktop)         │
   │            4 views: Terminal · Workspaces ·            │
   │                  Sessions · Health                     │
   └──────────────────────┬──────────────────────────────────┘
                          │ subscribes to events
                          ▼
   ┌─────────────────────────────────────────────────────────┐
   │  impulse-runtime  (PTY orchestrator)                   │
   │  ┌──────────┐  ┌────────────┐  ┌───────────────────┐  │
   │  │ Backend  │  │  Tool      │  │  Session state    │  │
   │  │ adapter  │  │ dispatcher │  │  machine          │  │
   │  │ (5 impl) │  │ (4 semas)  │  │  (8 phases)       │  │
   │  └────┬─────┘  └────────────┘  └───────────────────┘  │
   │       │                                                │
   │       ▼ portable-pty                                   │
   │  ┌──────────┐                                          │
   │  │  PTY     │  ──► stdout/stderr bytes ──► broadcast   │
   │  │  child   │                                          │
   │  └──────────┘                                          │
   └──────────────────────┬──────────────────────────────────┘
                          │ orchestrator API
                          ▼
   ┌─────────────────────────────────────────────────────────┐
   │  impulse-mcp  (rmcp 0.3 stdio server)                   │
   │  8 tools: list/register/unregister workspaces,         │
   │           list/start/end sessions, read orchestrator   │
   │           log, get_health                              │
   └─────────────────────────────────────────────────────────┘
```

## Backend adapters

The runtime ships 5 pluggable backend adapters — each one knows how to spawn
its coding-agent CLI as a real PTY child:

| Adapter | CLI binary | Resume | Approval prompt |
|---|---|---|---|
| `ClaudeCodeAdapter` | `claude` | `--resume <sess>` | "Do you want to proceed?" |
| `CodexAdapter` | `codex` | `resume <sess>` | "Approve? [y/N]" |
| `GeminiCliAdapter` | `gemini` | `--resume <sess>` | "Allow? [y/n]" |
| `OpenCodeAdapter` | `opencode` | `--session <sess>` | contains "approve" |
| `GenericCliAdapter` | user-supplied | n/a | never |

Each adapter injects `IMPULSE_SESSION=<sess>` so the child can correlate its
output back to the session id.

## Workspace

This is **not** a fork — it lives inside the same repo, on a new branch off
`main`. The original `impulse-rs/` crate, the legacy `impulse-gui/`, and
`impulse-desktop/` (the in-progress Dioxus shell from `codex-dioxus-host-goal-cleanup`)
remain untouched. The new code is in `clean/`.

To use it:

```bash
cd clean
cargo build --workspace
cargo test --workspace         # 79 tests
cargo run -p impulse-mcp -- --workspace-root /path/to/project
cargo run -p impulse-desktop --no-default-features --features desktop
```

## What's NOT in this branch

To keep the scope focused and the contract clean, the following were
deliberately left for follow-up work:

- **Dioxus SSR + Playwright smoke tests** — the Dioxus 0.6 SSR adapter in
  this toolchain is partial; the desktop variant is the canonical test
  surface.
- **EGUI / Tauri host adapters** — superseded by the Dioxus shell; the
  existing `impulse-gui` (egui) and the Tauri-shaped bridge in
  `impulse-desktop` are kept for parity but not used by the new code.
- **Canonical contract docs** — `docs/spec/RUST-CANONICAL-CONTRACT.md`
  should be updated to point at the new crate layout. The current contract
  still describes the pre-clean architecture.
- **CI / pre-commit hooks** — `.github/` and `.pre-commit-config.yaml`
  should be added before publishing.

## Alignment with the canonical contract

This branch implements the **desktop shell contract** (section 2 of the
canonical contract) on a fresh spine:

- Dioxus Desktop is the desktop host (✓)
- Dioxus 0.6.3 is the UI framework (✓)
- xterm.js mount point is reserved in `TerminalView` (placeholder; the
  actual eval()-based bridge is the next deliverable)
- Tauri is dropped (legacy-only)
- egui is dropped (legacy/frozen)
- `impulse-term` is replaced by the in-crate `pty` module — same
  `portable-pty` / `vt100` spine, simpler ownership

The **daemon IPC contract** (section 4 of the canonical contract) is
implemented as the `impulse-runtime::Orchestrator` in-process API; the
Unix-socket JSONL transport can be layered on top in a follow-up.

## Building

Requires Rust 1.82+ (uses workspace `resolver = "3"` via `edition = "2021"`,
or set `edition = "2024"` once the toolchain stabilizes).

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```
