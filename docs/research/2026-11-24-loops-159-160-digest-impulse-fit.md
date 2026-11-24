# Loops 159 and 160 plus the November 24 digest: fit with Impulse work already in flight

Date: 2026-11-24. Handoffs: 2026-11-23-grokky-impulse-loop-159,
2026-11-24-grokky-impulse-loop-160, 2026-11-24-grokky-impulse-digest.
Sources: research-ai-patterns rows in 2026-11-23-loop-159.csv (branch research/2026-11-23-daily,
PR #90), and 2026-11-24-loop-160.csv and 2026-11-24-digest.csv (branch research/2026-11-24-daily,
PR #91). Numbers are from arXiv abstracts or a README as stated; none were reproduced.

## arxiv-2611.27390-loop159 (evidence sufficiency and a Wilson bound): no fit for the picker; one evaluation idea held

- The AfterVerifierFailure path escalates on an observed fact: verification or attestation
  failed. The paper escalates on a predicted score from a separate 4B model. Adding that
  scorer would put a learned component into decide_step_model, which this lane rules out.
- The Wilson lower bound itself is useful on the evaluation side. selector_baselines could
  report a lower bound on each policy's pass rate, so a small fixture set does not look
  more certain than it is. Held until the open selector_baselines PR merges; not scheduled.

## arxiv-2611.28148-loop160 (SYLVA, step-level scoring): partial fit, evaluation only

- StepModelRecord already has one row per step (model, reason, actor, tool_round,
  configured), so a failure can be traced to a step rather than to the whole run. It
  only goes to tracing today, not to a durable ledger.
- Scoring each step on its own is a reporting change for selector_baselines, not a
  picker change. Escrow settlement, markets, and the context-aware router do not fit.
  The fleet-handoff half is held for Jim and is outside this note.

## arxiv-2611.28099-loop160 (VidRouteBench, router against the best fixed mix): fits as an evaluation check

- This is the clearest fit in the batch. selector_baselines already compares policies
  against a reference policy. Adding "the best single fixed model" as a baseline would
  show whether harness-state escalation gains anything over a fixed choice. The paper's
  own result (no per-prompt gain on LLM arms) is a reason to keep the picker thin.
- This is a candidate for the next selector_baselines loop, alongside the per-category
  breakdown from Loop 154, after the open PR merges. Not scheduled, and no change to
  decide_step_model.

## arxiv-2611.30412-loop160 (DMC-5, append-only log and a short decision view): no fit for the picker

- Compressing context for the acting model is Ion's or the coding agent's work.
  decide_step_model does not build model input.
- The append-only raw log matches a known gap: the chat path keeps no write-ahead record.
  Held for Ion or ROSA, as in the Loop 158 note.

## github-gogoymh-git-ontology-agent-digest1124 (typed mid-run steering): no fit for the picker; held for Ion or ROSA

- decide_step_model has no input for a new user request, and it should not get one.
- In a main checkout from Nov 2 (commit c46e9ee), I found no code that labels a mid-run
  request as a supplement, correction, cancel, and so on. ion_repl/chat.rs notes that
  full mid-round interruption is not built. The governed task ledger records events with
  provenance, and that would be the natural place for typed steering if Jim wants it. That
  would need an ADR for Ion or ROSA. Not scheduled. I checked an older checkout, not
  today's main, so this is a likely answer, not a verified one.

No code change, no new branch work beyond this note, no merge, and no message to Jim from these handoffs.
