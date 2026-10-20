---
schema: quirewiki-page@1
id: concept.quickstart
type: concept
title: Quickstart
status: draft
confidence: high
visibility: public
freshness:
  class: evolving
  review_after: "2026-11-27"
sources:
  - uri: Cargo.toml
    id: source.68b5adcb475d
    hash: "blake3:0ae685b6830d88b61dc428968dfdc302360c3ab87f5aeb8a2593a37a53d6f578"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: README.md
    id: source.0a096ba47097
    hash: "blake3:4e1e0ebbf36a3ad141653547fe6976c9aa7105929c22d30d885c128b8fd6e9b4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
claims:
  - id: claim.38da4eaf2310
    claim_kind: extracted
    confidence: high
    cite: "README.md:3"
    source: source.0a096ba47097
    extract: extract.38da4eaf2310
  - id: claim.4322fa4006da
    claim_kind: extracted
    confidence: high
    cite: "README.md:17-19"
    source: source.0a096ba47097
    extract: extract.4322fa4006da
  - id: claim.05968137e921
    claim_kind: extracted
    confidence: high
    cite: "README.md:20"
    source: source.0a096ba47097
    extract: extract.05968137e921
  - id: claim.499eb6fc6f21
    claim_kind: extracted
    confidence: high
    cite: "README.md:21-22"
    source: source.0a096ba47097
    extract: extract.499eb6fc6f21
  - id: claim.d7245d5f8034
    claim_kind: extracted
    confidence: high
    cite: "README.md:30"
    source: source.0a096ba47097
    extract: extract.d7245d5f8034
  - id: claim.ac4883364729
    claim_kind: extracted
    confidence: high
    cite: "README.md:41"
    source: source.0a096ba47097
    extract: extract.ac4883364729
  - id: claim.a31aa9db12d1
    claim_kind: extracted
    confidence: high
    cite: "README.md:58-59"
    source: source.0a096ba47097
    extract: extract.a31aa9db12d1
  - id: claim.3f3a0f2490e4
    claim_kind: extracted
    confidence: high
    cite: "README.md:58-59"
    source: source.0a096ba47097
    extract: extract.3f3a0f2490e4
  - id: claim.c21d234c24da
    claim_kind: extracted
    confidence: high
    cite: "README.md:63"
    source: source.0a096ba47097
    extract: extract.c21d234c24da
  - id: claim.cfa79873ab47
    claim_kind: extracted
    confidence: high
    cite: "README.md:64-65"
    source: source.0a096ba47097
    extract: extract.cfa79873ab47
  - id: claim.4e6596f314b7
    claim_kind: extracted
    confidence: high
    cite: "README.md:66"
    source: source.0a096ba47097
    extract: extract.4e6596f314b7
  - id: claim.e5bab646b9c4
    claim_kind: extracted
    confidence: high
    cite: "README.md:83-86"
    source: source.0a096ba47097
    extract: extract.e5bab646b9c4
  - id: claim.75bbffc50cec
    claim_kind: extracted
    confidence: high
    cite: "README.md:83-86"
    source: source.0a096ba47097
    extract: extract.a197764b1b5a
  - id: claim.e04a6d3a1caf
    claim_kind: extracted
    confidence: high
    cite: "README.md:90-92"
    source: source.0a096ba47097
    extract: extract.e04a6d3a1caf
  - id: claim.8cf2cd9f79f0
    claim_kind: extracted
    confidence: high
    cite: "README.md:90-92"
    source: source.0a096ba47097
    extract: extract.8cf2cd9f79f0
extracts:
  - id: extract.38da4eaf2310
    text: "**Feed the impulse to build.**"
    text_hash: "sha256:e71dc636b951a1c206f6bf77df0d76fb24d9f21bb8daaac387c1aee4f0dd53de"
  - id: extract.4322fa4006da
    text: "- **Dioxus cockpit:** `impulse-desktop` is the live, feature-gated desktop path. Dioxus and xterm.js render the system; typed Rust contracts, daemon snapshots, runtime state, and scoped persistence remain authoritative."
    text_hash: "sha256:1eefd4f19bb13c388fcb008393811e3f66a61d650107194026dbb0b6ecc33b11"
  - id: extract.05968137e921
    text: "- **ratatui workbench:** the root crate provides the terminal-native TUI."
    text_hash: "sha256:da1e7915325f6ae8e6dbab3186863e50f07de459cbe109671b8c26f950320a68"
  - id: extract.499eb6fc6f21
    text: "- **CLI and hooks:** short-lived commands initialize projects, track sessions, validate hooks, and query or update daemon-owned state."
    text_hash: "sha256:5af5ad60651fb8221a2699e8181b038bb3aadfe277771f5deaf8c8217c91da30"
  - id: extract.d7245d5f8034
    text: "From this directory:"
    text_hash: "sha256:8c71ab9bcb87c4fee97ea01c904d8ebca66803fef9658a507bec9a1413332bd4"
  - id: extract.ac4883364729
    text: "Launch the feature-gated Dioxus cockpit with:"
    text_hash: "sha256:8adbfc4f720ac9aaa0d2474ff4ce136a83adc831c07583f47aae9a53c704f325"
  - id: extract.a31aa9db12d1
    text: "`impulse-gui` is the legacy/frozen egui workbench."
    text_hash: "sha256:577d78b05f0e09ca3feeb0717b826bd69203f322978a8cf72db6c5091455cc39"
  - id: extract.3f3a0f2490e4
    text: It is excluded from the active Cargo workspace and retained only for compile maintenance while Dioxus owns the current desktop product path.
    text_hash: "sha256:893554a9797e05b0463dc5dd809b00967a2f0da9ef4a737b38d55d0ae7ec6772"
  - id: extract.c21d234c24da
    text: "- The daemon owns live control-plane and workbench snapshots while it is running."
    text_hash: "sha256:c86eef760dcb9a50e81eddbd90cb0ba7c3351b1b4528b049319da0bc92201975"
  - id: extract.cfa79873ab47
    text: "- Project-scoped persistence owns durable history, decisions, configuration, and artifacts across process restarts."
    text_hash: "sha256:baa49ac41a90080a3d8e6bb012f3f667fbb667a3a4eabce7cddd627b047a789a"
  - id: extract.4e6596f314b7
    text: "- PTY runtimes own process and terminal mechanics, then publish structured facts."
    text_hash: "sha256:79a74f0c149cad0761bb85bec08099bf78d44b402a16b5dd4f7a161309e18873"
  - id: extract.e5bab646b9c4
    text: Exact verified test totals change as branches are integrated.
    text_hash: "sha256:396accfa5024b7106bb48c79b2381022e397940e21d446aaafea4df641edb62b"
  - id: extract.a197764b1b5a
    text: "Use the repository-level [`AGENTS.md`](../AGENTS.md) and [`RUST-CANONICAL-CONTRACT.md`](../docs/spec/RUST-CANONICAL-CONTRACT.md) for the current canonical evidence rather than copying counts into this child README."
    text_hash: "sha256:0d2c61b33b305d5859e9defed0ff48ac4ce2df66d750331ec4e9e53159149570"
  - id: extract.e04a6d3a1caf
    text: "Project-local `.impulse/` state includes session history, durable decisions, live session state, and configuration."
    text_hash: "sha256:67adc5aa21800df329289e016e8279f5ba5e153968a8a4a591e5619d8cb1ca93"
  - id: extract.8cf2cd9f79f0
    text: SQLite indexes and daemon/runtime state complement those human-readable artifacts; not every operational record is intended to be committed.
    text_hash: "sha256:ac34fea17d09de8dc4a7b40e5df7ca1518bfea028275ab9a35f02ed7fdc07aa5"
---

# Quickstart

**Feed the impulse to build.** (README.md:3)

## Operator surfaces

- **Dioxus cockpit:** `impulse-desktop` is the live, feature-gated desktop path. Dioxus and xterm.js render the system; typed Rust contracts, daemon snapshots, runtime state, and scoped persistence remain authoritative. (README.md:17-19)
- **ratatui workbench:** the root crate provides the terminal-native TUI. (README.md:20)
- **CLI and hooks:** short-lived commands initialize projects, track sessions, validate hooks, and query or update daemon-owned state. (README.md:21-22)

## Run

From this directory: (README.md:30)

```bash
cargo run -- --help
cargo run -- init
cargo run -- daemon
cargo run -- run
cargo run -- session-start -n myproject -p claude-code
cargo run -- validate-hooks --platform claude-code
```

Source: (README.md:32-39)

Launch the feature-gated Dioxus cockpit with: (README.md:41)

```bash
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop
```

Source: (README.md:43-45)

## Workspace packages

| Package | Responsibility |
| --- | --- |
| `impulse-rs` | CLI, daemon, ratatui workbench, shared services, and native Ion execution path |
| `impulse-desktop` | Dioxus cockpit, xterm.js integration, typed host bridge, and desktop runtime adapters |
| `impulse-ion` | Transport-agnostic Ion harness request/response and adapter contracts |
| `impulse-ops` | Shared control-plane protocol, workbench, policy, registry, artifact, and telemetry models |
| `impulse-step-model` | Deterministic per-step model-routing policy shared across governed runtimes |
| `impulse-term` | Framework-neutral PTY lifecycle, terminal parsing, write queue, and context bridge |

Source: (README.md:49-56)

`impulse-gui` is the legacy/frozen egui workbench. (README.md:58-59)
It is excluded from the active Cargo workspace and retained only for compile maintenance while Dioxus owns the current desktop product path. (README.md:58-59)

## Authority boundaries

- The daemon owns live control-plane and workbench snapshots while it is running. (README.md:63)
- Project-scoped persistence owns durable history, decisions, configuration, and artifacts across process restarts. (README.md:64-65)
- PTY runtimes own process and terminal mechanics, then publish structured facts. (README.md:66)

## Build and verify

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Source: (README.md:76-81)

Exact verified test totals change as branches are integrated. (README.md:83-86)
Use the repository-level `AGENTS.md` and `RUST-CANONICAL-CONTRACT.md` for the current canonical evidence rather than copying counts into this child README. (README.md:83-86)

## Durable project data

Project-local `.impulse/` state includes session history, durable decisions, live session state, and configuration. (README.md:90-92)
SQLite indexes and daemon/runtime state complement those human-readable artifacts; not every operational record is intended to be committed. (README.md:90-92)

## Sources

- [Cargo.toml](../Cargo.toml)
- [README.md](../README.md)
