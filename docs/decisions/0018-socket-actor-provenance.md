---
title: "ADR-0018: Socket Actor Provenance"
description: Peer credentials plus a per-run operator capability classify every daemon connection, so only an operator surface can mint accepted
status: review
created: 2026-09-02
updated: 2026-09-02
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, daemon, governance, security, provenance]
---

# ADR-0018: Socket Actor Provenance

## Status

Proposed and implemented on lane `claude/socket-actor-provenance-20260902`; accepted on merge.
Closes the follow-up ADR-0012 and ADR-0013 both name and defer: "actor IDs remain same-user
provenance", the P0 in the 2026-09-02 staged plan.

## Context

ADR-0011 made operator acceptance the only path to `accepted`. ADR-0012 moved claim, verification,
and Supervisor review to daemon-owned producers so a client cannot forge evidence. ADR-0013 made an
accepted run project exactly one review-only memory candidate. Every one of those decisions rests on
one unproven assumption: that the process sending `RecordOperatorDecision` is the human's operator
surface.

Nothing checked it. The daemon accepted any connection on its Unix socket and the state layer's
only actor check was `require_actor(&decision.actor, GovernedActorKind::Operator)` — a check on a
field the *caller fills in*. A profiled Builder is launched with `IMPULSE_SOCKET_PATH` in its
environment by design, so the runtime under review could open the same socket, name itself an
operator, and approve its own governed task. The handler said so in a comment: no request-level
property distinguished the two callers, so the mutation was allowed and the boundary was documented
rather than enforced. Every downstream guarantee — operator-required acceptance, the authenticated
half of the evidence chain, and any future memory promotion or external event intake — inherited
that gap.

Two facts about a Unix socket connection can be established without new dependencies:

- **Peer credentials.** `SO_PEERCRED` on Linux and `LOCAL_PEERCRED` on macOS report the connecting
  process's uid to the kernel; `tokio::net::UnixStream::peer_cred` exposes both. systemd and D-Bus
  have used exactly this for local authorization for two decades. It is unforgeable — and, on its
  own, useless here, because a launched Builder runs as the same user and passes it.
- **A capability.** In the object-capability sense (Miller, *Robust Composition*, 2006), authority
  travels with an unforgeable reference rather than with an identity claim. A per-daemon-run secret
  that the operator surface holds and a launched runtime is never given separates the two callers
  that peer credentials cannot.

Impulse already scrubs every inherited `IMPULSE_*` variable from a pane before spawning it
(`impulse-term/src/backend.rs`), which is precisely the mechanism that keeps a capability out of a
governed runtime.

## Decision

1. **Every accepted connection is classified, once, at the socket boundary.** A connection is
   `NonOperator` by default — including a launched governed runtime holding `IMPULSE_SOCKET_PATH` —
   and becomes `Operator` only by presenting this daemon run's capability. The class is connection
   state, never derived from request payload, and a request cannot assert it.
2. **The daemon mints one operator capability per run.** 32 bytes from the platform CSPRNG, hex
   encoded, written atomically at mode 0600 beside the socket (`impulse.sock` ->
   `impulse.operator-cap`, matching how the PID file is placed) once the socket is bound and locked
   down, and removed when the accept loop exits. A capability never outlives the run that issued it.
3. **Presentation is a connection-scoped request.** `PresentOperatorCapability { token }` (protocol
   v7) raises *that* connection if, and only if, the peer uid equals the daemon's own uid **and**
   the token matches, compared in length-independent constant time. A rejected presentation leaves
   the class untouched, so a caller can neither downgrade an authenticated connection nor probe the
   token one character at a time. The token never appears in a response, a log line, a traced
   request name, or `Debug` output.
4. **Acceptance and profiled lifecycle marks require operator class.** `RecordOperatorDecision` is
   rejected from a non-operator connection with a typed `ActorProvenanceError::OperatorClassRequired`,
   before the idempotency receipt is consulted, so the task revision is unchanged and a replayed
   approval is refused too. For a task with a verification profile, `MarkRunning`,
   `MarkLaunchFailed`, and `MarkRuntimeExited` are also operator-only: a Builder submits claims
   through the daemon-owned producer requests, it does not narrate its own lifecycle.
   Caller-composed (non-profiled) tasks keep their existing lifecycle behavior, since no daemon-owned
   producer chain governs them.
5. **Provenance is recorded on the decision, by the daemon.** `OperatorDecision` gains a
   serde-defaulted `authentication: OperatorAuthentication` of `Declared` or
   `CapabilityAuthenticated`. `OperatorDecisionInput` deliberately has **no** such field: the value
   is stamped from the connection class by `State::mutate_governed_task_authenticated`, so a client
   payload cannot assert it. Records written before this ADR load as `Declared`. History replay
   carries the stored value forward rather than re-deriving it.
6. **The candidate assurance reads that stamp.** `AcceptedRunSourceAssurance` gains
   `DaemonProfiledEvidenceAuthenticatedOperator`, produced only when daemon-profiled evidence and an
   authenticated operator meet. There is deliberately no authenticated variant for caller-composed
   evidence: when the evidence chain was composed by a client, the weaker half of the chain sets the
   label. `ACCEPTED_RUN_MEMORY_DERIVATION_VERSION` moves to 2, since the same accepted run can now
   derive a different assurance, digest, and id.
7. **A superseded ledger reconciles rather than failing startup.** Candidates persisted at an older
   derivation version are pruned when `MEMORY_CANDIDATES.json` loads, and re-derived from
   authoritative governed-task truth by the existing reconcile pass. Governed tasks remain the
   source of truth; the projection stays independently replaceable, per ADR-0013.
8. **Clients present, panes never receive.** `DaemonClient` resolves a capability from
   `IMPULSE_OPERATOR_CAPABILITY` or the published file and presents it before a governed mutation,
   on the same connection. `remove_inherited_impulse_env` strips every inherited `IMPULSE_*` key
   before a pane spawns, so a governed runtime is never handed the capability; the explicit governed
   env overlay must never add it back.

## Consequences

- A launched Builder that opens the daemon socket and sends `RecordOperatorDecision` now receives a
  typed refusal naming the request and how to present a capability, and the governed task is
  byte-identical afterwards. Operator-required acceptance is enforced rather than documented.
- **This is a structural boundary, not a cryptographic one against a same-uid adversary.** The
  capability file is mode 0600 in a mode 0700 directory owned by the daemon's uid; a same-uid
  process that deliberately goes looking for it can read it. What the design guarantees is that a
  launched runtime is never *given* the capability and its environment is scrubbed of every
  `IMPULSE_*` key. Defeating it requires a runtime that deliberately hunts for a file it was never
  told about — a different and far more visible act than sending one request to a socket path it was
  handed. Real isolation needs a separate socket per class or an OS sandbox; neither is in this ADR.
- The assurance label is honest about a mixed chain. An authenticated operator approving
  caller-composed evidence still reads `caller_composed_evidence_declared_operator`, because the
  evidence is the weaker half. Only `daemon_profiled_evidence_authenticated_operator` claims both.
- Existing `MEMORY_CANDIDATES.json` ledgers are rewritten once on the first load after upgrade: the
  candidate ids change, because the id is a digest over the derivation version. Nothing else about
  the accepted runs changes, and no `GENOME.md` or `HISTORY.jsonl` write is involved.
- Protocol v7 is additive. An old client that never presents a capability keeps working for every
  request except the two families above, where it now gets a typed error instead of a silent
  success. The Dioxus cockpit's own socket client
  (`impulse-desktop/src/daemon_ops.rs`) must present the capability before its Approve/Reject
  buttons work again; that one-step change is recorded as a handoff in the lane card, since
  `impulse-desktop` belongs to another lane.
- Not adopted here: a second socket for the operator surface, an OS-level sandbox for launched
  runtimes, capability rotation within a run, per-request signatures, and multi-user daemons. Event
  intake from outside the machine — deferred by ADR-0017 pending exactly this authorization — is
  now unblocked but not implemented.

## Verification

This decision is represented when tests prove:

1. every changed contract type round-trips through serde, and an `OperatorDecision` persisted
   without an `authentication` key loads as `Declared`;
2. a capability is 64 lowercase hex characters, differs per run, never renders in `Debug`, is
   written at mode 0600, and round-trips through its published file;
3. presentation raises a connection only for a matching peer uid and a matching token, and every
   rejection path returns its own typed error and leaves the class unchanged;
4. against a real daemon, an approval from a raw socket connection that never presents the
   capability is refused, the task is byte-identical afterwards, and no memory candidate is staged;
   the same approval through `DaemonClient` yields `accepted`, exactly one candidate, and a
   `capability_authenticated` decision record;
5. against a real daemon, a profiled task's `MarkRunning` is refused from a non-presenting
   connection and accepted from the operator surface;
6. a spawned pane's environment never carries `IMPULSE_OPERATOR_CAPABILITY`;
7. daemon-profiled evidence plus an authenticated operator derives
   `DaemonProfiledEvidenceAuthenticatedOperator`, caller-composed evidence never does, and a
   client-supplied `authentication` key in the mutation payload is ignored;
8. a `MEMORY_CANDIDATES.json` written at the previous derivation version reconciles on reload
   instead of failing startup, leaving governed-task truth untouched.

Source of truth: `impulse-rs/src/daemon/actor_provenance.rs`, `impulse-rs/src/daemon/mod.rs`,
`impulse-rs/src/daemon/handlers.rs`, `impulse-rs/src/state/{governed_task,memory_candidate}.rs`,
`impulse-rs/impulse-ops/src/{governed_task,memory_candidate}.rs`,
`impulse-rs/tests/socket_actor_provenance.rs`.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`0013-deterministic-accepted-run-memory-candidates.md`](0013-deterministic-accepted-run-memory-candidates.md)
- [`0017-canonical-loop-contract.md`](0017-canonical-loop-contract.md)
- `docs/IPC-PROTOCOL.md` (protocol v7)
- `docs/plans/worktrees/2026-09-02-claude-socket-actor-provenance.md`
