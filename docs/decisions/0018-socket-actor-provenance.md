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
2. **The daemon mints one operator capability per run.** 32 bytes read from the operating system's
   CSPRNG (`/dev/urandom`; `rand` and `getrandom` are only transitive dependencies of this
   workspace), hex encoded, written atomically at mode 0600 beside the socket (`impulse.sock` ->
   `impulse.operator-cap`, matching how the PID file is placed) **before** the listener binds, so
   that "the socket is connectable" implies "the capability is on disk" — publishing after bind
   leaves a window in which a client that treats socket readiness as readiness reads no capability,
   silently acts as a non-operator, and is refused. The temp file is opened `create_new` and its mode asserted on the descriptor rather than
   the path, so the secret is only ever written into a file this process just created at 0600.
   Minting failure is returned, never absorbed into a weaker fallback. The file is removed when the
   accept loop exits; **a signal-killed daemon leaves it behind**, and nothing signals the shutdown
   path today, so in practice the file usually survives the process. This is safe rather than tidy:
   a stale token authenticates nothing, because a presentation is compared against the *running*
   daemon's in-memory capability, and the next run overwrites the file with its own. A signal
   handler that reaches the cleanup path is follow-up work, not a correctness gap.
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
   producer chain governs them. **The classification is an exhaustive match over
   `GovernedTaskMutation`, deliberately with no catch-all arm**: a mutation variant added later must
   break `authorize_governed_mutation`'s compilation and be classified on purpose, rather than
   defaulting open and becoming silently reachable from a launched Builder. That guard has already
   paid for itself once: rebasing this lane onto a `main` carrying ADR-0012's reservation journal
   broke the build on the new `NoteProducerReservationInterrupted` variant, which is now classified
   operator-only. It is reconciled in-process at ledger load and nothing legitimately submits it
   over the socket, so gating it costs nothing and stops a launched Builder forging an
   "your reservation was interrupted" note to move its own task out of a stuck reservation.
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
8. **Clients present, panes never receive.** Both socket clients — the CLI/TUI `DaemonClient` and
   the cockpit's own client in `impulse-desktop` — resolve a capability through one shared helper
   (`impulse_ops::operator_capability`) from `IMPULSE_OPERATOR_CAPABILITY` or the published file, and
   present it before a governed mutation on the same connection.
   `remove_inherited_impulse_env` strips every inherited `IMPULSE_*` key before a pane spawns, so a
   governed runtime is never handed the capability; the explicit governed env overlay must never add
   it back. A capability that is found and *refused* is surfaced, not logged and forgotten: the CLI
   prints a warning naming the provenance downgrade, and the cockpit folds the daemon's reason into
   the error its existing surface already shows. Silence there would let an operator watch an
   approval land as `declared` while believing it was authenticated.

## Consequences

- A launched Builder that opens the daemon socket and sends `RecordOperatorDecision` now receives a
  typed refusal naming the request and how to present a capability, and the governed task is
  byte-identical afterwards. Operator-required acceptance is enforced rather than documented.
- **This is a structural boundary, not a cryptographic one against a same-uid adversary, and the
  file is one `cat` away.** The capability path is a pure function of the socket path
  (`with_extension("operator-cap")`), and every governed pane is handed `IMPULSE_SOCKET_PATH` by
  design. A launched runtime that wants the capability does not have to search for it: it can derive
  the path and read the file, because 0600 in a 0700 directory stops other users, not the daemon's
  own uid. What the design guarantees is narrower and should be stated as such — a runtime is never
  *given* the capability, and its environment is scrubbed of every `IMPULSE_*` key, so minting
  `accepted` stops being an accident of holding a socket path and becomes a deliberate act of
  reading a file the runtime was never told about. That is a real change in what an honest,
  non-adversarial runtime can do by mistake, and no barrier at all to one that means to.
  Relocating the file to a path not derivable from the socket was considered and rejected this
  round: any location under the daemon's own `.impulse` home is still listable by the same uid, so
  it would buy obscurity while reading as if it bought security. Closing this needs a mechanism, not
  a new path — a separate socket per connection class, an OS sandbox for launched runtimes, or
  handing the operator surface its capability over a channel the pane has no access to. Those are
  follow-ups below, not variations on this decision.
- **A connection class authenticates the caller, not what the caller's action then executes.** The
  concurrent staged-worktree lane (ADR-0019) found a family of bypasses this decision does not
  touch. A staged Builder can write repository state that every linked worktree shares — `.git/hooks`,
  `.git/config` (where a `filter.<name>.smudge` driver still executes on checkout even with
  `core.hooksPath` disabled, and which `git config --local` reaches from inside the Builder's own
  worktree), and `.git/info/attributes` (shared, and never visible in a work-tree diff, so code
  review cannot see it) — and that state executes later during an operator-triggered promotion,
  after review has passed, under the operator's authority, without the Builder ever reaching the
  socket. It is the same shape of failure this ADR fixes, a Builder influencing an operator-authority
  outcome, reached through a different door, and it sets the limit on how far "operator-class
  connection" should be read: the capability proves who opened the connection, and nothing about the
  integrity of the work that connection's request then acts on. Any producer or promotion path must
  establish that separately; ADR-0019 does so by digesting those components at materialization and
  refusing to check anything out at promotion unless the digests still hold.
- **The capability file is same-uid writable, and that is an availability exposure, not an
  authentication one.** Anything that can read the file can also overwrite it. Doing so does not
  forge operator class: a presentation is compared against the *running* daemon's in-memory
  capability, never against the file, so a rewritten file authenticates nothing. What it does is
  make the real operator's client present a token the daemon will refuse — a denial of service on
  the approval path, self-correcting on daemon restart. Pinned by a regression test rather than left
  as an inference from the code.
- The assurance label is honest about a mixed chain. An authenticated operator approving
  caller-composed evidence still reads `caller_composed_evidence_declared_operator`, because the
  evidence is the weaker half. Only `daemon_profiled_evidence_authenticated_operator` claims both.
- Existing `MEMORY_CANDIDATES.json` ledgers are rewritten once on the first load after upgrade: the
  candidate ids change, because the id is a digest over the derivation version. Nothing else about
  the accepted runs changes, and no `GENOME.md` or `HISTORY.jsonl` write is involved.
- Protocol v7 is additive. An old client that never presents a capability keeps working for every
  request except the two families above, where it now gets a typed error instead of a silent
  success. Anything with a hand-rolled socket client — including test harnesses that write JSON
  lines directly — must present the capability before an approval, or it will now be refused.
  The concurrent packaged-acceptance lane also bumps the protocol to 7; the proposal on the table is
  that this decision keeps v7 and that lane rebases to v8, with the exact conflict list recorded on
  this lane's work card.
- The Dioxus cockpit's own socket client (`impulse-desktop/src/daemon_ops.rs`) presents the
  capability on the connection it is about to use. This covers both operator surfaces there: the
  Approve/Reject buttons, and the lifecycle reconciler that marks a profiled task's runtime exit —
  which is operator-only under rule 4, so without it a profiled Builder's exit would be refused and
  the task would stay stuck in `Running`.
- Not adopted here: a second socket for the operator surface, an OS-level sandbox for launched
  runtimes, capability rotation within a run, per-request signatures, and multi-user daemons. Event
  intake from outside the machine — deferred by ADR-0017 pending exactly this authorization — is
  now unblocked but not implemented.

### Follow-ups this decision does not close

1. **A mechanism that survives a same-uid adversary.** A per-class socket, an OS sandbox, or an
   out-of-band hand-off of the capability to the operator surface. Until one of those lands, the
   boundary is the structural one described above and must be described that way.
2. **`prune_superseded_derivations` must become a status-preserving migration.** Dropping and
   re-deriving a candidate is lossless only while `MemoryCandidateStatus` has exactly one variant.
   When ADR-0020 (or ADR-0019's promotion work) adds `Promoted`/`Dismissed`, a prune would silently
   revert an operator's review decision to `PendingReview`; the load path must migrate the status
   forward instead of discarding the record.
3. **No signal handler reaches the capability cleanup path.** Nothing notifies `shutdown_notify`,
   so the accept loop's cleanup is unreachable in practice and a killed daemon leaves its
   capability file (and socket) behind. Harmless today for the reason in decision 2, but a daemon
   that tidies up on SIGTERM is the honest end state.
4. **Refusals are untyped on the wire.** A rejection is `DaemonResponse::Error { message }` with no
   machine-readable code, so clients match on prose. The protocol already has a precedent for a
   typed variant (`Busy { resource, retry_after_ms }`); an `Unauthorized { request, reason }` would
   follow it, and is deliberately deferred to avoid widening this lane's overlap with the concurrent
   packaged-acceptance lane, which edits the same protocol file.

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
   instead of failing startup, leaving governed-task truth untouched;
9. the cockpit client presents the capability on the same connection as the mutation and only for
   gated requests, a profiled `MarkRuntimeExited` from the cockpit still reconciles, and a refused
   or absent capability still sends the request so the daemon's own error surfaces.

Source of truth: `impulse-rs/src/daemon/actor_provenance.rs`, `impulse-rs/src/daemon/mod.rs`,
`impulse-rs/src/daemon/handlers.rs`, `impulse-rs/src/state/{governed_task,memory_candidate}.rs`,
`impulse-rs/impulse-ops/src/{governed_task,memory_candidate,operator_capability}.rs`,
`impulse-rs/src/client/mod.rs`, `impulse-rs/impulse-desktop/src/daemon_ops.rs`,
`impulse-rs/tests/socket_actor_provenance.rs`.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`0013-deterministic-accepted-run-memory-candidates.md`](0013-deterministic-accepted-run-memory-candidates.md)
- [`0017-canonical-loop-contract.md`](0017-canonical-loop-contract.md)
- `0019-*` (staged Builder worktree scope): the other half of the same boundary — this decision
  authenticates the connection, that one establishes the integrity of the repository state an
  authenticated request then acts on.
- `docs/IPC-PROTOCOL.md` (protocol v7)
- `docs/plans/worktrees/2026-09-02-claude-socket-actor-provenance.md`
