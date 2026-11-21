# Loop 154 register rows: fit with Impulse work in flight

Handoff 2026-11-21-grokky-impulse-loop-154. Source: research-ai-patterns `synthesis/register/rows/2026-11-21-loop-154.csv` at commit a05feb5 (PR #88). Every figure below is AIden's reading of an arXiv abstract. None was reproduced, and nothing here is adopted.

Work in flight that a row could attach to:
- Selector baselines (branch `grok/selector-baselines-regressions-20261116`, draft PR form waiting on Jim's Submit). It compares policies task by task against a reference policy, offline only.
- The approval-path change ("Impulse go", offered, waiting on Jim). It would route MCP `tools/call` and the daemon's `InvokeTool` through chat's approval check.
- `decide_step_model` stays a thin picker that names a model. No learned router, no gateway, no new crate.

## Decisions

**2611.25526 MATE: partial fit, approval work only.** The runtime guardrail maps a structured action record to allow, block, or ask the user. That is the same shape as the approval check the "Impulse go" change would enforce on every tool-call path, so it supports that scope. It adds nothing new to build. The "verified route instead of a model call" half does not fit, because it skips the picker entirely and would be a new subsystem. Hold that half.

**2611.26309 Client Exits: no fit.** It is about how a chat client renders typed markers. It is not about model selection, and no Impulse work in flight touches chat rendering. If it is useful at all, it belongs with ROSA or the desktop surface, not here.

**2611.26332 Rigel-Ladon: weak fit, later, evaluation only.** It forecasts the accuracy of a router that scores K options from measurements at small K. `decide_step_model` is a rule-based picker, not a K-option scorer, so the law does not apply to it directly. The one usable idea is to report selector accuracy as the model pool grows in `selector_baselines`, next to ADAPT's pool-size threshold (Loop 152). That would be a follow-up after the current selector_baselines PR merges, not a change to the picker.

**2611.25146 EverydayBench: fits selector_baselines as the next increment.** The useful finding is that no single model led every task type. `selector_baselines` already has a choose-at-start-by-category policy. The natural next step is to break the newly-passed and newly-failed lists down by task category, so a regression in one category is visible even when the overall pass rate holds. Its three-layer scoring (hard checks, re-fetched citations, rubric) is for cpa-agent-tool, not Impulse. Queue the per-category breakdown behind the current PR. Do not start it until that PR merges.

**2611.25357 Pro2 Agent: no fit now.** An explicit list of open problems as long-run state is a run-state design question for ROSA or a future harness ADR, not model selection. Nothing in flight touches it. Hold.

**2611.24651 Piccolo: no fit, declined.** It proposes a trained decision model to score options. Adopting it would make `decide_step_model` a learned router, which this lane rules out. Declined.

## Net

Two rows touch work in flight. MATE backs the waiting approval change without adding scope. EverydayBench gives the next selector_baselines increment once the current PR merges. Rigel-Ladon is a later evaluation idea. Client Exits, Pro2, and Piccolo do not fit. No work is scheduled from these rows; everything still waits on Jim's two decisions: submitting the selector_baselines PR, and "Impulse go" for the approval change.
