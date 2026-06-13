# 2026-06-13 12:52 EDT — codex — retro Dioxus design integration

Status: complete

## Scope

Integrate the Claude Design export at `docs/design/2026-06-13-anthropic-2a4KuevwMQ3tBaBgQ-Mi_w/` into `impulse-desktop` without changing backend command contracts.

## Design Source

- `impulse-ui/README.md` says to read chat transcripts first, then primary design files and imports.
- `impulse-ui/chats/chat1.md` lands on a retro broadcast Dioxus/backend implementation.
- `impulse-ui/project/IMPULSE-DESIGN-SPEC.md` defines the principles, hierarchy, tokens, and retro broadcast constraints.
- `impulse-ui/project/dioxus-impl/INTEGRATION.md` maps the retro shell to `impulse-desktop`.

## Intended Edits

- Preserve the fetched design bundle under `docs/design/`.
- Add a Dioxus-loaded CRT skin asset for existing `ui.rs` class names.
- Add Rust theme helpers for status dots, status labels, and compact counts.
- Update the desktop shell to render one hero, a stat trio, and pending-review row semantics.
- Add SSR contract tests for the skin and backend-bound formatting.

## Verification

- `cargo fmt --all -- --check`
- `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop`
- `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check --workspace`

## Result

Committed as `21bb24b feat(impulse-desktop): integrate retro dioxus design skin`.
The branch keeps `DesktopShell()` for existing callers and adds
`DesktopShellWithSnapshot(ProjectOpsSnapshot)` for backend-bound Dioxus rendering.
