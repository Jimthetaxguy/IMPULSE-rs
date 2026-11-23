# Neon harness row: fit with Impulse work already in flight

Date: 2026-11-22 (recorded 2026-11-23). Handoff: 2026-11-22-grokky-impulse-neon-harness.
Source: research-ai-patterns branch research/2026-11-22-daily, file
synthesis/register/rows/2026-11-22-neon-harness.csv at commit c809fa2 (PR #89).
The score pairs below come from The New Stack citing Google's Neon model card.
I did not open the model card or reproduce the figures.

## neon-harness-2026-11-22: partial fit; logging and evaluation only; no change to decide_step_model

The row says a model's benchmark score can move about as much from the harness and
the effort level as from switching models, and that every score should be recorded as
model, effort, harness, harness version, benchmark, value, source URL, and date. It
explicitly says not to change decide_step_model.

- That recording advice fits the evaluation and logging side, not the picker. The
  picker stays a thin, deterministic name choice.
- `record_step_model` in `impulse-rs/src/agent/step_model.rs` already writes a
  StepModelRecord, but only model, reason, actor, tool_round, actor_id, and the
  configured model, and only to a tracing log. It has no fields for effort, harness,
  or harness version, and it is not a durable ledger.
- `selector_baselines` compares policies on fixture tasks and also omits harness and
  effort. After the open branch grok/selector-baselines-regressions-20261116 merges,
  a later loop could add those fields to the record and to the baseline report. That
  is not scheduled, and it would not change decide_step_model.

No code change, no new branch work, no merge, and no message to Jim from this handoff.
