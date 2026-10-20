---
date: 2026-09-18
kind: point-in-time map
source: James paste — Building Agentic Harnesses in Rust (Harness Handlers)
against: ADR-0011, ADR-0014, ADR-0015, ADR-0017, ADR-0018, ion-tool-sandbox spec, open PR #65
status: HOLD for stamp before any follow-on write
---

# Rust agentic-harness note → Impulse. Keep / port / hold (2026-09-18)

Opened the paste Grokky assigned, ADR-0014, ADR-0015, ADR-0011/0017/0018 heads, the ion-tool-sandbox spec, and open PR #65. Still unsure whether James wants ADR-0014 ratified before any WorkItem code. Next write after stamp is merge or close #65 — not a new crate.

## Three steals mapped

| Steal from the note | Impulse already owns it | Where |
| --- | --- | --- |
| Host-owned authority (model proposes; harness authorizes) | Yes | ADR-0011 four-party attestation; ADR-0018 socket actor provenance; ion sandbox roots + confirmation (spec `2026-09-02-ion-tool-sandbox-and-untrusted-output.md`); PR **#65** puts `governed_submit_claim` on the same confirm floor as `bash_exec` / `file_write` |
| Typed effects ≠ bare capability | Partially | ADR-0014 effect classes (Pure / Reversible / Compensatable / Irreversible) bound fan-out at planning time. Not yet a full Rust `AuthorizedOp` vs `RequestedOp` type pair across every tool |
| Auto is a policy preset, not a bypass | Yes in spirit | Confirmation-required tools and sticky `untrusted_seen` (plain `y` declines after untrusted output). There is no “Auto mode” that skips the daemon operator path |

## Keep

- Keep the four layers the note names as **roles**, not new packages: model/protocol ≈ Ion + providers; governance ≈ daemon governed run + tool confirm; operator console ≈ desktop / TUI; session/daemon ≈ existing daemon. Do not invent a fifth crate for “control system.”
- Keep **ADR-0015**: `decide_step_model` only names G for one step. Critics (DiffGemma / Jev / TypeSafe) stay outside the runtime. The note’s “discrete in-loop decisions” do not license growing the picker.
- Keep **#65** as the live close for claim authority: model-issued claim hits `Allow?` / `CONFIRM`; declined confirm never opens the socket.

## Port (thin, only if a real miss appears)

- Port the note’s “requested vs authorized” wording into docs next to ADR-0011/0018 if a reader still confuses worker claim with operator accept — one glossary paragraph, not a type rewrite.
- Port bounded-channel / process-lifetime language only when a named adverse fixture fails (runaway stdout, child ignoring SIGTERM). ADR-0017 already owns loop budgets; do not duplicate it.

## Hold

- Hold a new `AuthorizedOperation` crate or MCP-shaped protocol engine.
- Hold DiffGemma / langchain-typesafe / System One wiring in Impulse runtime (same HOLD as AIden/Codee notes).
- Hold rewriting ADR-0014 into “the authority ADR.” Effect-class fan-out stays 0014; operator provenance stays 0018; step model stays 0015.
- Hold merge of #65 until James stamps.

## Verdict

**KEEP** existing ADRs + ion confirm floor. **PORT** nothing until a miss. **HOLD** new trees and critic SDKs. Open PR #65 is the only stamp-gated write this map names.

## James gap list (2026-09-18 evening) — contracts before copy-paste

The HTML report stays **reference only**. Several of its sketches do not enforce the guarantees around them. James named the gaps below. Map each to Impulse; do not treat the report as an implementation spec.

### Gaps and Impulse ownership

| Gap | What James requires | Impulse today | #65? |
| --- | --- | --- | --- |
| Stale attempt edits the working tree | Attempt-specific workspace → candidate changeset → verify → coordinated promote; current-attempt check and accept must be one step | **ADR-0019** already: staged Builder worktree, promote after acceptance, discard on reject. Detached verify worktrees exist. First demo should prove: launch → cancel/limit → supervise cleanup → reject stale → workspace authoritative → recover after restart | **No.** #65 does not stage workspaces |
| Child coding-agent CLI | Host cannot authorize every FS/network op inside the child; ProjectFs wraps host callers only | Document as trust boundary (cap-std ambient API limit). Confinement is OS/sandbox profile or explicit trust assumption — not a Rust wrapper on Impulse code paths | **No** |
| Cancel drops future while child continues | Supervisor retains ownership and cleanup after the caller stops waiting; do not release concurrency permits until the workload stops or transfers | Partial: process/PTY supervision exists; Tokio `select!` cancel-vs-run sketches must not be copied. Port only when an adverse fixture fails | **No** |
| Reusable vs consumed budgets | Reusable (slots, connections): release when stopped. Consumed (tokens, bytes, money): cancel does not refund; uncertain remote needs explicit accounting state | ADR-0017 loop budgets are round/wall-clock/no-progress. Token/cost accounting is host-owned evidence, not refund-on-cancel | **No** |
| Observed vs durable journal | ToolFinished to live UI before persist can show success that vanishes after crash. Observed completion ≠ durable acceptance | Daemon-owned governed records aim at durable transitions; outbox/cursor not fully specified. Tighten before copying the report’s sequence diagram | **Partial at most:** #65 keeps a *declined* claim off the socket (no claim record). It does not fix journal ordering for finished tools |
| Private `authorized(...)` across crate | Visibility must not leak constructors | Follow Rust privacy: construction stays inside governance; expose a checked preparation service | **N/A** (doc/code hygiene) |
| Approval expiry after queue wait | Re-check expiry, resource version, policy, attempt state immediately before execute | Confirm-at-call for tools is immediate; long-queue admission re-check is not a single API yet | **No** — #65 confirms at the moment of the claim tool call |
| operation_id / attempt_id / request_id / idempotency_key | Distinct identities; new MCP request ID ≠ safe retry | Hold as vocabulary on governed task + attempt; do not invent a fourth ID product | **No** |

### Operation contract classes (James)

1. Candidate file/artifact → stage, verify, promote (ADR-0019 path).
2. Authoritative local mutation → coordinate version check and mutation.
3. Remote effect → destination idempotency / conditional write, or uncertain + reconcile.
4. Lost contact after submit → record uncertainty; reconcile before retry.

### #65 verdict (explicit)

**#65 stays separate.** It closes the Ion tool-floor miss: model-issued `governed_submit_claim` joins `bash_exec` / `file_write` on the confirmation floor; declined confirm never reaches `run()`. It does **not** close stale-attempt workspace integrity, child-CLI confinement, cancel-vs-supervisor ownership, budget accounting splits, or durable-vs-observed journal ordering.

Stamp board unchanged: merge #65 when James wants that floor locked. First architecture demo after that is one ADR-0019 failure path (above), not a new crate and not a second map page.
