# CLAUDE.md — Impulse

@AGENTS.md

> Product north star: [`VISION.md`](VISION.md)
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Collaboration playbook: [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=control-plane foundations + governed runtime producers + accepted-run review candidates; Next=stronger same-user actor authorization + full launched Builder/Supervisor proof; Later=memory promotion/dismissal daemon/UI/runtime wiring + general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.

## Claude Code

[`AGENTS.md`](AGENTS.md) is the project context: layout, toolchain, commands, conventions, deploy, and guardrails. The `@AGENTS.md` line above imports it. Add project facts there so this file cannot drift.

The identity lines in the blockquote stay because `docs/validate_docs.py` requires them in this file as well as in `AGENTS.md`. They are the same sentences, not a second body of guidance.

- Claude Code feature procedures are in [`.claude/skills/impulse-development/SKILL.md`](.claude/skills/impulse-development/SKILL.md). Use that skill when adding a CLI command, daemon IPC message, Dioxus view, dynamic tool, or guardrail.
- Guardrail rules are the Guardrails section of [`AGENTS.md`](AGENTS.md) and the code in `impulse-rs/src/guardrail/`. This file has no separate guardrail section.
- Property-based testing (cited from the `proptest` comment in `impulse-rs/Cargo.toml`) is the "Property-based testing" paragraph in [`AGENTS.md`](AGENTS.md).
- `impulse-rs/src/tooling/env_scrub.rs` still names "CLAUDE.md Principle #5" for deny-by-default child environments. That rule is principle 5, Capability-based tools, in [`AGENTS.md`](AGENTS.md).
