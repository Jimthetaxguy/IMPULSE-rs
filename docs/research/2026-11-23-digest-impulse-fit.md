# November 23 digest: fit with Impulse work already in flight

Date: 2026-11-23. Handoff: 2026-11-23-grokky-impulse-digest.
Source: research-ai-patterns branch research/2026-11-23-daily, file
synthesis/register/rows/2026-11-23-digest.csv at commit 7c33f88 (PR #90).
Intent Contracts figures come from the arXiv abstract only. The harness row's longer-task
figure comes from secondary coverage; the author blog was the primary source for that row.
I did not reproduce either paper.

## arxiv:2611.24025 (Intent Contracts): partial fit for the tool path; no fit for the picker

The row asks whether Impulse's dispatch path already splits planning from writing the
tool call, with a logged contract (goal, user quote, and scope) as the receipt.

- It does not. decide_step_model only names the model for one step. The Ion loop or the
  external coding agent both plans and writes tool calls. There is no logged contract
  object on that path.
- The split itself is about approval and dispatch, not model selection. It is in the
  same family as the pending change that would put every tool call through Jim's
  approval check, including the MCP and daemon paths that currently skip it. This row
  does not make that change larger, and it is not a reason to build a new contract
  type. Held with that approval work, and decide_step_model is unchanged.

## alphaxiv:2611.harness-cg (Language Model Harnesses Can Be Compositional Generalizers): no fit for the picker

This is the same paper I already reviewed for Loop 155. The digest row points at the
author blog rather than the alphaXiv abstract, and that does not change the decision.

- Splitting a task and keeping each model call in-distribution happens in Ion's loop or
  the external coding agent, not in decide_step_model.
- Testing decompositions on short tasks first already matches selector_baselines, which
  scores the picker on short fixture tasks. The open branch
  grok/selector-baselines-regressions-20261116 extends that report. Nothing new is needed.
- If Jim wants typed sub-call context, that would be an ADR for Ion or ROSA. Held, as
  in the Loop 155 note.

No code change, no new branch work, no merge, and no message to Jim from this handoff.
