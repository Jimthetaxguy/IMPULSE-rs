---
date: 2026-10-22
kind: point-in-time map
author: Claude Code (overnight /goal run), from a read-only sweep of Codex, Gemini/Antigravity, Claude, and ledger notes
against: main at ff867b0, open PRs #140-#144
status: proposals only; nothing here is decided
---

# Primitive candidates after blueprint 2.1 (2026-10-22)

PRs #140-#142 implement three items of the 1B agent-ledger blueprint
("blueprint 2.1"). The blueprint file itself (`agent-data/1B/blueprint.md`) is
not on this machine, so the status below is rebuilt from the PR descriptions,
and ranked changes #1, #5 and later are unknown until James supplies it.

## Blueprint 2.1 status

| Item | State | Where |
| --- | --- | --- |
| #2 lease fencing generation (stale-worker test) | Primitive done, PR open. No lease expiry, heartbeat, takeover request, or launcher injection of the generation yet | #140 |
| Rule 4 effect lifecycle (lost-response test) | Done for local producers, PR open. No remote effects | #141 |
| #4 verifier-only `evidenced_status` (false-completion test) | Read-time projection, PR open. No durable promises table | #142 |
| #3 two-channel event log (published vs private) | Not started | — |
| Context compiler v0 | Not started (`src/context_lifecycle/` has injector, templates, intent) | — |

## Candidates, checked against the code

1. **Lease renewal and takeover.** #140 adds the fence, but nothing ever bumps
   the generation from outside. The first slice is an operator-class
   (ADR-0018) socket request that issues the takeover, plus exporting the
   generation where the desktop launcher already exports
   `IMPULSE_GOVERNED_TASK_ID` (`impulse-desktop/src/runtime.rs`). This
   sequences after #140.
2. **Two-channel event log (blueprint #3).** This is the prerequisite for a
   durable promises table and for restating open promises at the context
   tail. First slice: an `EventChannel` on governed events and a
   `published_events()` projection.
3. **Remote-effect reconcile probe.** #141 covers local producers only. Before
   any governed producer pushes, opens a PR, or calls a webhook, that effect
   needs an idempotency key and a `reconcile()` probe that must run before a
   retry (gap list in `2026-09-18-rust-agentic-harness-map.md`).
4. **Durable step-model decisions.** `src/agent/step_model.rs` says durable
   ledger attachment is later work. Persisting each decision against its
   governed task is what a paired harness-change evaluation would need.
5. **Governed ledger on SQLite.** `GOVERNED_TASKS.json` is rewritten whole on
   each mutation. `rusqlite` is already a dependency (blackboard, ADR-0023).
   Sequence this after #140/#141, which add schema-version gating.
6. **Pre-mutation restore point.** Ion writes under the repo root. Git covers
   tracked files but not untracked or ignored ones. A restore point would copy
   those before the first `file_write`/`bash_exec` of a session.

## Looked at and set aside

- **Argument-bound, expiring `ApprovalGrant`.** Proposed to stop arguments
  from changing between approval and execution. In `ReplToolExecutor::execute`
  (`src/ion_repl/chat.rs`) the input `Value` is owned, confirmed
  synchronously, and moved straight into `tool.run`, with no queue in
  between. A digest and expiry would add ceremony without closing a gap. This
  becomes relevant only if approvals are ever queued (daemon or MCP path).
- **Claim-blind Supervisor review.** The Codex research report ranks this
  first: the judge should not see the worker's narrative. But
  `supervisor_review_prompt` (`src/governed_producers.rs`) already gives the
  Supervisor only command exit codes and digests, never output. Dropping
  `claim_summary_untrusted` would leave it the task, the criteria, and pass/fail
  bits, so in practice it could only restate the verifier. Done properly, this
  needs the Supervisor to get real evidence (a bounded diff, for example)
  instead of the claim. That changes the Supervisor contract and is James's
  call.

## Contradictions found between agents' notes and the code

- Gemini's architecture note says commit-OID fencing already rejects zombie
  workers. That fences promotion only; #140 exists because a stale worker could
  still submit claims.
- #140 says it could not find where the launcher sets the task environment. It
  is in `impulse-desktop/src/runtime.rs`.
- The Codex research snapshot reviewed `wip/impulse-monty-stub-removal-20261022`
  (now #144), not main.
- ADR-0016 (evolution plane) and ADR-0024 (orchestration hierarchy) are still
  `draft`; new orchestration primitives should attach to them or to
  ADR-0011/0012 rather than start a new thread.

Sources: `~/code/_working-files/ai-harness-research/2026-10-22/research-report.md`,
`~/.gemini/antigravity/brain/304fb422-c739-421c-a1b7-b93393e8c81f/agent_native_primitives_and_architecture.md`,
`docs/plans/2026-09-18-rust-agentic-harness-map.md`, PR descriptions #140-#142.
