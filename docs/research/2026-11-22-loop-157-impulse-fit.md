# Loop 157: fit with Impulse work already in flight

Date: 2026-11-22. Handoff: 2026-11-22-grokky-impulse-loop-157.
Source: research-ai-patterns branch research/2026-11-22-daily, file
synthesis/register/rows/2026-11-22-loop-157.csv at commit 0c9adaf (PR #89).
The numbers below come from arXiv abstracts as stated, and I did not reproduce them.

## arxiv-2611.23216-loop157 (Higher-Scoring Coding Models Did Not Get More Work Done): supports the current design; no code change

The row suggests that decide_step_model score models on Jim's own outcomes, such as a
merged PR, and keep the cheaper model until that outcome shows a gap.

- The "keep the configured model until there is a gap" half is already how the picker
  works. decide_step_model returns the configured model, and it switches only when the
  harness state shows a verifier or attestation failure. The paper supports keeping that
  default and not chasing leaderboard upgrades.
- The "score models on outcomes" half would mean live learning from Jim's merge history
  inside the picker. That is a learned router, and this lane rules it out.
- The one place it could fit later is on the evaluation side. selector_baselines already
  scores policies on whether fixture tasks pass, which is an outcome and not a benchmark
  score. If Jim wants it, a later loop could add a check against real outcomes (for
  example, whether a verified run's PR merged). That is evaluation only, it would wait
  until the open grok/selector-baselines-regressions-20261116 PR merges, and it is not
  scheduled.

## arxiv-2611.19230-loop157 (CompoundingKernel): no fit for the picker; held for the blackboard ADR

The row suggests keeping candidate state and lineage in a file the controller reads, and
raising checker throughput before adding agents.

- An archive of candidates and their lineage is orchestration state, not model selection.
  The nearest Impulse work is the draft SQLite blackboard ADR on an unmerged claude/
  branch, which Jim has not numbered or approved. If that ADR moves forward, lineage of
  candidate attempts is a field worth considering there.
- "Raise checker throughput before adding agents" matches rules that already apply: the
  local cargo gate is the verifier, builds run one at a time, and the agent count stays
  at three or four. Nothing new is needed.

No code change, no new branch work, no merge, and no message to Jim.
