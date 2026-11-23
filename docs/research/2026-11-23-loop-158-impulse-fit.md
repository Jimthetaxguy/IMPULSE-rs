# Loop 158: fit with Impulse work already in flight

Date: 2026-11-23. Handoff: 2026-11-23-grokky-impulse-loop-158.
Source: research-ai-patterns branch research/2026-11-23-daily, file
synthesis/register/rows/2026-11-23-loop-158.csv at commit 4bd0271 (PR #90).
The numbers below come from arXiv abstracts as stated, and I did not reproduce them.

## arxiv-2611.26902-loop158 (Perceive Once, Escalate on Demand): partial fit; no code change

The row asks whether decide_step_model escalation already hands the larger model the
small model's working notes, instead of making it re-read the whole history.

- Escalation today only changes the model name. When verification or attestation has
  failed and an escalate model is configured, decide_step_model returns that model with
  the reason AfterVerifierFailure. resolve_step_model then puts that name on the chat
  request. StepModelDecision and StepModelRecord have no field for working notes or
  other state, so the larger model does not receive a handoff from the picker.
- Passing usable notes would belong in Ion's loop or the coding-agent session, which
  already hold the run's history. A learned map that copies a model's cache is out of
  scope, because this lane does not add a trained component.
- Held for later: if Jim wants this, Ion could pass a short written state when the
  model changes. Nothing is scheduled, and decide_step_model is unchanged.

## arxiv-2611.27090-loop158 (recovery in 50 harnesses): no fit for the picker

The row suggests logging each external effect with its undo step, and counting resume
as untested until a run has actually resumed from it.

- That is recovery and orchestration, not model selection. decide_step_model does not
  log effects or resume a run.
- It is consistent with a known gap: the chat path keeps no write-ahead record of
  external effects. If Jim wants that later, it belongs in Ion or ROSA, not in the
  picker. Held, and not scheduled.

No code change, no new branch work, no merge, and no message to Jim from this handoff.
