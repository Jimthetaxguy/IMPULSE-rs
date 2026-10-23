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

## From the Cursor and Grok sources (second sweep)

Sources: `~/.cursor/plans/` and the Cursor `state.vscdb` composer threads
(mostly execution and PR-landing work, no new primitives); grok-build in
`~/.grok` (memory-v2 `topics/impulse-rs.md`, the 2026-09-17 authority-hole
sessions, the 2026-10-21 "propose state" session); grokbot in `~/.grokbot`
(daemon state only) with its autonomous logs under `~/Desktop/.remember/`;
and the Grok-built `~/code/min-agent-rs` and `~/code/tools/context-lane`.

7. **Total tool-call budget per loop run.** `LoopBudget` capped rounds,
   wall clock, streaks and context, but one round runs every call the model
   batched, so ten rounds of fan-out were unbounded. *Implemented in a
   follow-up PR* (`max_tool_calls`, `LoopTrip::ToolCallBudget`, Ion default
   50, governed Builder unchanged).
8. **Redact credential-shaped tool output** before it reaches the model or
   the blackboard. The tool-output guardrail rules only warn
   (`src/guardrail/defaults.rs`), and `wrap_untrusted_tool_output`
   (`src/ion_repl/chat.rs`) passes content through unchanged. Prior art:
   `min-agent-rs/src/redact.rs`.
9. **Run Monty in a subprocess worker (`monty-pool`).** ADR-0021 records that
   `max_memory` does not bound gradual growth, and the interpreter runs inside
   the daemon. The first slice is one worker under a parent-side hard timeout,
   tested so that a crashing program leaves the daemon alive. Upstream now
   publishes `monty-pool` 1.3.0 (async `Pool` → `Checkout`, mounts, snapshot
   restore), but its `MontyTransport::Subprocess` spawns a separate `monty`
   child binary, so adopting it means building, shipping and locating that
   binary next to `impulse-rs`. That packaging decision comes first.
10. **A calculator without Python.** `calculate()` still interpolates into a
    Python program and requires `Capability::PythonExec`, so voice policy
    classifies a calculator call as mutating. A pure-Rust parser for the
    allowlisted grammar would remove the over-gate, but it is not a drop-in.
    The grammar admits `**` and `//`, so `2**1000` needs exact big integers,
    and results are Python `str()` output (`5.0`, `1e+20`,
    `0.30000000000000004`). A parser that differs silently changes answers,
    and exact parity means a bignum dependency. Dropping `PythonExec` while
    the expression still runs in the daemon's interpreter is a policy change
    too: `9**9**9` is bounded only by Monty's limits. Decide the semantics
    first.
11. **Proposal-acceptor rules for context compiler v0** (Grok, 2026-10-21).
    The model proposes a lane document and a Rust acceptor refuses unsourced
    facts, reads without excerpts, and self-assigned ids or budgets.
    Redaction runs before anything is written. Prior art: `context-lane`.
12. **License mismatch.** `LICENSE` is Apache-2.0 but every `Cargo.toml` says
    MIT. James needs to pick one.

13. **Token and cost budgets per loop run** (ChatGPT research, rebuilt by
    Gemini on 2026-10-22 as
    `~/code/chatgpt-analysis/output/v2_surfaces/extracted_kits/impulse_contracts_kit`,
    from the "Tighten IMPULSE Contracts" conversations). Its
    `ResourceBudget` independently lands on 50 tool calls per run, and adds
    `model_input_tokens`/`model_output_tokens`/`model_cost_microunits`.
    `LoopBudget` has none of these, yet every `ChatResponse` already carries
    `Usage`, so a token budget is the natural next slice after the tool-call
    cap. The same kit's `RequestedInvocation → PreparedInvocation →
    AuthorizedInvocation` typestate is the full form of the "AuthorizedOp vs
    RequestedOp" gap in the 2026-09-18 map. Do not port its
    `InvocationService::authorize` as written: it loads and then subtracts the
    atomic budget in two steps, so two concurrent authorizations can both pass
    the check.

### Full recheck of Codex, Gemini and Claude sources

The recheck covered 30 Codex rollouts, 32 Gemini brain notes and 13
transcripts, 12 Claude IMPULSE sessions, and the memory, plans and `.remember`
files. Nothing in them contradicts the work above. Additions:

- **Ledger in one transaction** (Gemini primitives note §2.3), refining
  candidates 2 and 5: write each governed event and its state change in one
  SQLite transaction, export JSONL from it, and use `STRICT` tables with
  `CHECK` on status columns.
- **Staged compaction** (Gemini, 2026-10-11 learnings): deterministic elision
  at ~60% of the context budget and an LLM summary at ~85%. Today only the
  deterministic stub pass exists.
- **Compact compiler errors**: parse `cargo --message-format=json` into short
  diagnostics for the model. Not on main.
- **Read-before-write**: refuse `file_write` to an existing file the session
  has not read. Not on main.
- **Semantic interaction checks for parallel lanes** (Codex research report):
  separate worktrees isolate lanes but do not prove their edits compose; run
  one final verification on the combined tree.
- **Planner hook before the first mutation** (Codex research report),
  evaluated against no plan and a self-plan.
- **Monty**: `monty-alloc` must be the global allocator for `max_memory` to
  bound gradual growth (Codex, 2026-10-20). Upstream 1.2.0 shipped
  2026-10-18; main pins 1.0.0. A pool must also discard a worker after any
  resource-limit failure. This sharpens candidate 9.

Also noted: `ToolRegistry::with_defaults` logs an error and returns an empty
registry. Its production callers (agent discovery, voice adapter) could
therefore advertise zero tools silently; `try_with_defaults()?` would fail
closed. Grok Build's per-prompt `rewind_points.jsonl` snapshots give a
concrete shape for candidate 6.

Stale claims in those sources: the Cursor 2026-09-02 transcript describes a
`block-tool-output-secret` rule and a 32 KiB output cap that are not on
main. Grok memory says `PythonExecTool` ignores its timeout (it doesn't) and
that main fails to compile `impulse-desktop` (fixed).

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

## Two fencing designs exist

`origin/feat/governed-fencing-generation` (a cloud agent, 2026-10-22, no PR)
is a second implementation of blueprint change #2, stacked on #141's WIP
commits. It advances the generation on every acknowledged launch
(`launch_fencing_generation()`, already injected by the desktop launcher),
where #140 advances it only on an explicit `TakeOverWorker`. Both answer an
idempotent replay from its receipt before checking the fence. #140 now carries
the review fixes (launch injection, no takeover while a claim is under review),
so it is the one to land; keep the other branch as a reference until #140
merges, then close it.

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
