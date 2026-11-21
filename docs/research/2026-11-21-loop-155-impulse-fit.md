# Loop 155: fit with Impulse work already in flight

Date: 2026-11-21. Handoff: 2026-11-21-grokky-impulse-loop-155.
Source: research-ai-patterns branch research/2026-11-21-daily, file
synthesis/register/rows/2026-11-21-loop-155.csv at commit 71aa9f2 (PR #88).
The numbers below come from abstracts and vendor posts, and I did not reproduce them.

## x.com-NaceAI-drex-router-loop155 (Drex Router): no fit; nothing adopted

The row suggests that decide_step_model return a probability and a reason for each
candidate model, and fall back to a default when the top probability is low.

- The reason half is already done. In `impulse-step-model/src/lib.rs`, StepModelDecision
  already has a typed `reason` field (StepModelReason::Configured or
  AfterVerifierFailure). The tests check the model and the reason together, and the
  reason is serialized (`{"model":...,"reason":...}`).
- The fallback half is already done too. When no harness-state rule fires, the picker
  returns the configured model.
- The probability half does not apply. The picker is deterministic: a rule about harness
  state either fires or it does not, so there is no distribution to report. Producing a
  confidence score would mean adding a learned router, and this lane rules that out.
- Drex itself is a vendor router product. As in Loop 153, it is something to compare
  against, not something to adopt.

## alphaxiv.org-abs-2611.harness-cg-loop155 (harnesses as compositional generalizers): no fit for the picker; idea noted for later

The row suggests that the harness keep each sub-call's context small and typed, and
that decompositions be tested on short tasks before long ones.

- Splitting a task up and managing context happens in Ion's loop, or in the external
  coding agent. decide_step_model only names a model for one step, so this is not
  model-selection work.
- The part about short tasks first matches what is already in flight. The
  selector_baselines example scores the picker on short fixture tasks before it is
  trusted on long runs. The open branch grok/selector-baselines-regressions-20261116 adds
  newly-passed and newly-failed tasks to that report. Nothing new is needed.
- If Jim ever wants typed sub-call context, it would be an ADR for Ion or ROSA, not a
  change in impulse-step-model. Held.

No code change, no new branch work, no merge, and no message to Jim.
