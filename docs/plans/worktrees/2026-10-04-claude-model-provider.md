---
title: Model provider trait — ADR-0022 stage 2
description: Work card for model-provider-20261004
updated: 2026-10-04
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Model provider trait — ADR-0022 stage 2

## Lane Facts
- Owner: claude (Opus 5.5, interactive session with James)
- Role: implementer
- Branch: `claude/model-provider-20261004`, stacked on `claude/ion-blackboard-20261003`
- Worktree: `../IMPULSE-rs.wt-provider`
- Owned paths: `impulse-rs/src/model_provider/`, this card
- Shared paths edited: `impulse-rs/src/model_endpoint/{mod,min_agent_bridge}.rs` (capabilities,
  orchestrator role, fallbacks, policy, shared `load_config`), `impulse-rs/src/ion_repl/{chat,mod,
  tool_photon,tool_blackboard}.rs`, `impulse-rs/src/llm_backends/mod.rs`
  (`ProviderSelectionError::InvalidEndpointConfig`), `impulse-rs/src/test_support.rs`,
  `impulse-rs/Cargo.toml` (`futures`, `tower`, reqwest `stream`), `impulse-rs/src/lib.rs`,
  `docs/decisions/0022-typed-model-endpoints.md`, `docs/decisions/README.md`, `CONTEXT.md`
- Plan/spec: ADR-0022 stage 2 amendment; LangChain provider research in James's iCloud
  `Claude Research/langchain-provider-abstraction-research.md`
- Verification: full four-command gate plus
  `cargo clippy --all-targets --no-default-features --features office-support -- -D warnings`,
  `python3 docs/validate_docs.py`, and opt-in `IMPULSE_OLLAMA_IT=1 cargo test --lib -- --ignored ollama`
- Latest status: implemented, reviewed, fixed; pushed

## Decisions
- 2026-10-04: native async trait instead of the min-agent route ADR-0022 rule 7 named (James's
  directive; min-agent's client is blocking). Photon keeps min-agent.
- 2026-10-04: the research mapped Ion to "fast, cheap" and Photon to "balanced". In Impulse Ion is
  the main coding agent and Photon a disposable reader, so roles map to configured profiles, not
  built-in tiers.
- 2026-10-04: `openai_responses` is refused by name at build time rather than half-implemented.
- 2026-10-04: Ion uses the new path only when `model_endpoints` assigns it; the legacy
  `IMPULSE_PROVIDER` path is untouched otherwise.
- 2026-10-04: fixed a latent test race from the blackboard lane: blackboard and spill tests now
  hold `test_support::impulse_home_unset()`, because `blackboard_dir()` reads `IMPULSE_HOME`.

## Handoff Notes
- Real-system evidence: local Ollama (`qwen3:8b`, `llama3.1:8b`) passed generate, stream, a
  `file_read` tool call, and an Ion turn from `config.json`. The Ollama server was started for the
  run and stopped after.
- Adversarial review (2026-10-04): 3 P1, 3 P2, 2 P3 groups, all reproduced and fixed with
  regression tests; recorded in ADR-0022's stage 2 amendment.
- Gate: cargo test --workspace 3292 passed, 0 failed, 12 ignored; clippy clean with and without
  default features; fmt clean; docs 195/195.
- Not done: native Responses provider, per-step model choice across endpoints, cost/token budgets,
  latency/quality routing, photon on the new trait.
