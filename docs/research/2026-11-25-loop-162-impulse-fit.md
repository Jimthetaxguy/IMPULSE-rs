# Loop 162 fit check: three model-routing rows

Handoff: 2026-11-25-impulse-loop-162 (from Grokky). Rows: synthesis/register/rows/2026-11-25-loop-162.csv on research-ai-patterns branch research/2026-11-25-daily (ff9ac2d, PR #92). All numbers are from the arXiv abstracts as AIden filed them; not checked against the papers. Code read from /workspace/merge-all/IMPULSE-rs at 2ba676d (Oct 19); likely current, not re-verified against main 921f5c9.

## 1. Control-Flow Tax (2611.32365): partial fit, evaluation only

`decide_step_model` already makes a small finite choice, but with no model involved: the configured model, or `escalate_model` after a verifier or attestation failure. That is a fixed rule, not a cheap specialist model choosing among actions, and adding one would turn the picker into a learned router, which stays out of lane. Trajectory length is not logged next to cost. `StepModelRecord` carries `tool_round` per step, but only through `tracing::info!`, and `selector_baselines` run records hold pass, cost_usd and latency_ms with no round or step count. Since the paper says savings depend on short trajectories, a later evaluation idea is a `rounds` field in the run record so cost per success can be read by trajectory length. Held for Jim.

## 2. Delegent (2611.33098): no fit for the picker; held for Ion

There is no per-subtask delegation point. The picker names one model per chat step for the whole loop; Ion's tool bridge (`src/ion_repl/tool_bridge.rs`) runs tools directly through `ToolRegistry::execute`, and `src/agent/coordinator.rs` only notices delegations that already happened inside coding-CLI panes and recommends review. Handing legwork (reading code, running tests, search) to a small local model would be a new Ion capability: a sub-call with its own model and its own result handed back. That needs an Ion ADR, not a picker change.

## 3. ShellBench (2611.32946): partial fit, evaluation only

Selection does not rank by cost when scores are close. In `selector_baselines`, `best_fixed` picks the highest train pass rate and uses lower total cost only to break an exact tie, so a model 1 point better at six times the cost still wins. The live picker does not rank models at all. A later evaluation idea: report a cost-aware choice beside it, such as the cheapest model within a set margin of the best pass rate (ideally using the Wilson lower bound from the Loop 159 note so small samples don't fake a lead). Held for Jim.

No code changed. Nothing is scheduled until the open selector_baselines branch is decided.
