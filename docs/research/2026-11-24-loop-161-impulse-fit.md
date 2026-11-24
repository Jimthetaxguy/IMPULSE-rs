# Loop 161 fit check: TideSand (arXiv 2611.31118)

Handoff: 2026-11-24-impulse-loop-161 (from Grokky). Row: arxiv-2611.31118-loop161 in research-ai-patterns synthesis/register/rows/2026-11-24-loop-161.csv at a8b138a (PR #91). Read from the row summary only; the paper's results were not read.

## Decision: partial fit, evaluation side only

The picker does not depend on timing. `decide_step_model` in `impulse-rs/src/agent/step_model.rs` chooses the configured model, or the escalation model after a verifier or attestation failure. No duration, latency, or token count is an input, so contention on a busy machine cannot change which model it names.

Two numbers on the evaluation side can carry contention noise:

1. `impulse-rs/impulse-step-model/examples/selector_baselines.rs` reads a `latency_ms` field from each run record and reports p50 and p95 latency per policy (`p50_ms`, `p95_ms`). If those runs were recorded on the shared box while other bots were busy, the latency comparison between policies includes machine load, not just model speed. The pass and cost columns are not timing-based.
2. The tool loop has a wall-clock budget (`LoopContract.budget.wall_clock`, default 180 seconds in `impulse-rs/src/llm_backends/mod.rs`). A run that times out on a loaded machine is recorded as a failure, so heavy contention could turn a slow pass into a fail and leak into pass rates. This is likely rare at 180 seconds but is not measured.

## What this means

No picker change. If selector_baselines latency is ever used to compare policies, record runs one at a time or note machine load alongside each run, and treat latency from the shared box as indicative only. TideSand's scheduler itself is a sandbox-runtime idea and is out of the Impulse picker lane. Nothing is scheduled until the open selector_baselines branch is decided.

Checkout read: /workspace/merge-all/IMPULSE-rs at 2ba676d (Oct 19); main is now 921f5c9, and the Nov changes since were desktop-only per earlier checks, so this is likely current but not re-verified against main.

## Correction to the Loop 160 note

While reading `selector_baselines.rs` for this check, I saw it already compares the current selector against a single best fixed model fitted on the train split (from Handoff Curves, arXiv 2610.15993). The Loop 160 note proposed adding that baseline as new work for VidRouteBench; it already exists. The VidRouteBench row is therefore already covered, not a next loop.
