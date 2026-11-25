# Loop 163 fit check: DAGGER, Runway Model Router, open-weight frontier skills

Handoff: 2026-11-25-impulse-loop-163 (from Grokky). Rows 0-2 of synthesis/register/rows/2026-11-25-loop-163.csv on research-ai-patterns branch research/2026-11-25-daily (08133e3, PR #92). Numbers are from abstracts and Runway's own study; not checked. Code read from /workspace/merge-all/IMPULSE-rs at 2ba676d (Oct 19), so line numbers are likely current but not re-verified against main 921f5c9. Follows docs/research/2026-11-25-loop-162-impulse-fit.md (4ab53cf).

## 1. DAGGER (2611.31252): partial fit, evaluation only; merges with the round-count field

The picker does not score choices: `decide_step_model` (`impulse-rs/src/agent/step_model.rs:120`) returns the configured model, or `escalate_model` after a verifier failure. Per-step scoring from sampled runs would make it a learned router, which stays out of lane. On the evaluation side, `selector_baselines` scores whole runs only (`Run` at `impulse-rs/impulse-step-model/examples/selector_baselines.rs:23`: pass, cost_usd, latency_ms), so errors that carry from one step into the next are invisible. Per-step data exists only as `StepModelRecord` (`step_model.rs:38`), logged through `tracing::info!` at `step_model.rs:155`, with no durable store. This merges with the held rounds field: record a rounds count per run first, and per-step outcomes later only if a durable step log is approved.

## 2. Runway Model Router: partial fit, audit half only; merges with neither held item

Taking a priority and cost cap per call is a gateway feature and stays out of lane; the picker has no cost input ("Cost is unused in v0", `step_model.rs:118`). The audit half partly exists: each pick already returns `StepModelDecision {model, reason}` and logs `StepModelRecord`, but cost is not recorded next to the pick. A later logging-only addition could put the call's actual cost into `StepModelRecord`, which also feeds `cost_usd` in selector_baselines. Separate from the two held items.

## 3. Frontier skills on open-weight models (2611.31480): partial fit, evaluation only; merges with cheapest-within-margin

The live picker does not rank models, so there is no tie to break. In `selector_baselines`, `best_fixed` (`selector_baselines.rs:74`) picks the highest pass rate and breaks exact ties on lower total cost (`selector_baselines.rs:88-89`), then on name; latency is reported (p50 at line 68) but never used in the choice. Latency as a further tie-breaker belongs inside the held cheapest-within-margin report: among models within the margin, prefer lower cost, then lower p50 latency, keeping the Loop 161 caveat that latency from the shared box carries load noise. The fleet half (agents routing around eval controls) is left to Grokky.

No code changed. All three are held for Jim behind the open selector_baselines branch.
