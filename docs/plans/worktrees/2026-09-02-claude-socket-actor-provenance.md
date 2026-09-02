---
title: Socket Actor Provenance Lane
description: Work card for the ADR-0018 socket actor provenance lane (operator capability, connection classes, authenticated acceptance)
updated: 2026-09-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Socket Actor Provenance Lane

## Lane Facts
- Owner: Claude (Fable 5.1)
- Role: implementation lane
- Branch: `claude/socket-actor-provenance-20260902`
- Worktree: `.worktrees/socket-actor-provenance-20260902`
- Owned paths: `impulse-rs/src/daemon/**`, `impulse-rs/src/state/{governed_task,memory_candidate}.rs`,
  `impulse-rs/impulse-ops/src/{governed_task,memory_candidate,lib}.rs` (type/protocol additions only),
  `impulse-rs/impulse-term/src/backend.rs`, `impulse-rs/src/client/**`, `impulse-rs/tests/**`,
  `docs/decisions/0018-socket-actor-provenance.md`, `docs/decisions/README.md`, `docs/INDEX.md`,
  `docs/SUMMARY.md`, `docs/SUMMARY.yaml`, `docs/IPC-PROTOCOL.md`, `CONTEXT.md` (one glossary entry),
  this card.
- Blocked/shared paths: `impulse-rs/impulse-desktop/**` (Codex lane), `.github/**`,
  `impulse-rs/scripts/**`, `Cargo.toml`/`Cargo.lock`, `CLAUDE.md`, `AGENTS.md`,
  `impulse-rs/src/governed_producers.rs`.
  Taken by necessity with a handoff note: `docs/validate_docs.py` (protocol-version markers only).
- Plan/spec: `docs/plans/2026-09-02-impulse-next-stages.md` Stage 2; ADR-0018.
- Verification: isolated `CARGO_TARGET_DIR`; `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 docs/validate_docs.py --all`.
- Latest status: complete; draft PR open.

## Decisions
- 2026-09-02: `ProcessRequestContext.daemon_identity` named in the plan does **not** exist on
  `origin/main` `36bda00` — it belongs to the unmerged Codex lane. This lane adds one
  `connection_class: ConnectionClass` field to `ProcessRequestContext` instead of forking the
  struct, so the eventual merge is a single-field conflict rather than a structural one.
- 2026-09-02: the capability is presented with a dedicated connection-scoped
  `PresentOperatorCapability` request rather than a new envelope field. `DaemonClient` opens one
  connection per request, so a connection-scoped handshake costs one extra round trip only on the
  requests that need it, and the `{"type","data"}` envelope shape is untouched (smaller merge
  surface with the Codex lane).
- 2026-09-02: operator authentication is stamped by the daemon onto the persisted
  `OperatorDecision` record, never accepted from `OperatorDecisionInput`. Adding a field to
  `OperatorDecisionInput` would also have broken `impulse-desktop/src/ui.rs:1855`, a blocked path.
- 2026-09-02: only one new `AcceptedRunSourceAssurance` variant
  (`DaemonProfiledEvidenceAuthenticatedOperator`). Caller-composed evidence never claims
  authenticated provenance: the weaker half of the chain dominates the assurance label.
- 2026-09-02: `PROTOCOL_VERSION` 6 -> 7. If the Codex packaged-acceptance lane (also v7) merges
  first, this lane rebases to v8; see handoff notes.

## Changes
- `impulse-rs/src/daemon/actor_provenance.rs` (new): `OperatorCapability`, `ConnectionClass`,
  `ConnectionProvenance`, `ActorProvenanceError`, peer-uid check, governed-mutation authorization.
- `impulse-rs/src/daemon/mod.rs`: per-run capability generation, 0600 atomic write beside the
  socket, removal on shutdown, per-connection provenance, `PresentOperatorCapability` handling.
- `impulse-rs/src/daemon/protocol.rs`: new request variant, `PROTOCOL_VERSION` marker test.
- `impulse-rs/src/daemon/handlers.rs`: `ProcessRequestContext.connection_class`; enforcement in
  `handle_governed_task_request`.
- `impulse-rs/impulse-ops/src/governed_task.rs`: `OperatorAuthentication`;
  `OperatorDecision.authentication` (serde-defaulted).
- `impulse-rs/impulse-ops/src/memory_candidate.rs`:
  `AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator`;
  `ACCEPTED_RUN_MEMORY_DERIVATION_VERSION` 1 -> 2.
- `impulse-rs/impulse-ops/src/lib.rs`: `DAEMON_PROTOCOL_VERSION` 6 -> 7.
- `impulse-rs/src/state/governed_task.rs`: `mutate_governed_task_authenticated`; authentication
  threaded through `apply_mutation` and the history replay.
- `impulse-rs/src/state/memory_candidate.rs`: assurance derivation from the stored decision;
  stale-derivation pruning at ledger load.
- `impulse-rs/src/client/mod.rs`: capability discovery (env var then file) and presentation.
- `impulse-rs/impulse-term/src/backend.rs`: capability env var named in the scrub contract plus a
  regression test.
- `impulse-rs/tests/socket_actor_provenance.rs` (new): real-daemon proofs.
- Docs: ADR-0018, `docs/IPC-PROTOCOL.md` v7, four index files, `CONTEXT.md` glossary entry.

## Tests

Gate run on this checkout with `CARGO_TARGET_DIR` isolated from the shared global target dir.
`cargo build --workspace` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo fmt --all -- --check` clean; `python3 docs/validate_docs.py --all` shows only the four
pre-existing failures (ADR-0014 `status: proposed`, three stale March docs).

`cargo test --workspace`: **2363 passed, 0 failed, 9 ignored** (review round 2; round 1 was
2361/0/9, the pre-review commit `c156ce3` was 2353/0/9).

| Package / target | passed | failed | ignored |
|---|---|---|---|
| impulse-desktop lib | 151 | 0 | 0 |
| impulse-desktop tests/desktop_contract | 64 | 0 | 0 |
| impulse-desktop tests/host_surface | 8 | 0 | 0 |
| impulse-desktop tests/runtime | 22 | 0 | 1 |
| impulse-desktop tests/views_ssr | 7 | 0 | 0 |
| impulse-ion lib | 23 | 0 | 1 |
| impulse-ops lib | 78 | 0 | 0 |
| impulse-ops tests/governed_producer_contract | 8 | 0 | 0 |
| impulse-ops tests/governed_task_contract | 5 | 0 | 0 |
| impulse-ops tests/memory_candidate_contract | 5 | 0 | 0 |
| impulse-rs lib | 1823 | 0 | 5 |
| impulse-rs tests/governed_process_flow | 2 | 0 | 0 |
| impulse-rs tests/hook_validation_extraction_benchmark | 5 | 0 | 0 |
| impulse-rs tests/hook_validation_precompact | 5 | 0 | 0 |
| impulse-rs tests/hook_validation_session_start | 5 | 0 | 0 |
| impulse-rs tests/integration_enhancements | 11 | 0 | 1 |
| impulse-rs tests/ion_binary | 4 | 0 | 0 |
| impulse-rs tests/ion_verify_cli | 2 | 0 | 0 |
| impulse-rs tests/socket_actor_provenance (new) | 4 | 0 | 0 |
| impulse-step-model lib | 12 | 0 | 0 |
| impulse-term lib | 94 | 0 | 0 |
| impulse-term tests/backend_tests | 19 | 0 | 0 |
| impulse-term tests/boundary_tests | 3 | 0 | 0 |
| doc-tests impulse_rs | 3 | 0 | 1 |

Re-runs after review rounds 1 and 2 were clean end to end (exit 0, no failures).

Observed flake on the first gate run only, unrelated to this lane: `agent::tests::test_harness_query_kills_hung_child_instead_of_orphaning`
failed once under full-workspace parallel load and passed in isolation and on the recorded run. It
is a process-kill timing test in `src/agent/mod.rs`, untouched here.

## Review round 1 (adversarial review of PR #48 at `c156ce3`)

Verdict was "needs changes"; design claims (a)-(d) and (f) held. Everything below is folded into
the branch.

1. **Cockpit lifecycle reconciler was still broken.** The review found that `daemon_ops.rs`'s
   lifecycle reconciler and its `MarkLaunchFailed`/`MarkRuntimeExited` call sites also send
   operator-only mutations for profiled tasks, so at `c156ce3` a profiled Builder exit would have
   left the task stuck in `Running`. The coarse `requires_operator_class` over every
   `MutateGovernedTask` already covers it; added
   `a_profiled_runtime_exit_from_the_cockpit_presents_the_capability_and_reconciles`, which also
   pins that the preceding `GetGovernedTask` does *not* pay for a handshake.
2. **`authorize_governed_mutation` failed open.** It ended in `_ => Ok(())`, so a future variant
   (ADR-0019's promotion) would have become silently reachable from a Builder. Now an exhaustive
   match naming every `GovernedTaskMutation` variant, with a doc line saying a new variant must
   break compilation and be classified deliberately.
3. **Refused capabilities were silent.** Both clients logged at `debug` and continued, so an
   approval could land as `declared` while the operator believed otherwise. The CLI now prints a
   warning naming the downgrade (`capability_refusal_warning`, unit-tested); the cockpit returns
   the daemon's reason from `present_operator_capability` and folds it into the error its existing
   surface already shows — no UI restructuring.
4. **The entropy claim was false.** `generate()` concatenated two `Uuid::new_v4` values: 244 bits
   with six fixed nibbles, not the "32 CSPRNG bytes" the ADR claimed. `rand` and `getrandom` are
   only transitive dependencies and `Cargo.toml` is not this lane's to change, so it now reads 32
   bytes from `/dev/urandom` — the same kernel CSPRNG — and returns an error rather than falling
   back. `generate()` became fallible, so the capability moved behind a `OnceLock` set in `start`
   (`Daemon::new` stays infallible).
5. **Desktop had a silent data-loss shape.** A `BufReader` was built for the handshake, read one
   line, then dropped, and a second was built on the same socket. One reader is now hoisted across
   both exchanges, matching `client/mod.rs::exchange_line`.
6. **Capability location — kept, documented honestly.** The path is
   `socket_path.with_extension("operator-cap")` and every pane is handed `IMPULSE_SOCKET_PATH`, so
   the bypass is one `cat`. Relocation under `IMPULSE_HOME` with a per-run id was considered and
   rejected: any location under the daemon's own home is still listable by the same uid, so it
   buys obscurity while reading as if it bought security. ADR Consequences now say plainly that the
   file is derivable from the socket path, and the follow-up list names the mechanisms that would
   actually close it (per-class socket, OS sandbox, out-of-band hand-off).
7. **`write_capability_file` hardened.** Stale temp removed, then `create_new(true)`, with the mode
   asserted on the descriptor via `File::set_permissions` rather than on the path.
8. **Protocol collision proposal recorded** — see the handoff notes below and the PR body.

Nits: `#[serde(deny_unknown_fields)]` added to `OperatorDecisionInput` (a payload asserting its own
`authentication` is now rejected at the boundary rather than ignored — the two smuggling tests
assert the rejection); the `prune_superseded_derivations` / `MemoryCandidateStatus` hazard and the
untyped-refusal-on-the-wire gap are both recorded in the ADR's follow-up list, no code change.

## Review round 2 (verification of PR #48)

Round 2 confirmed all eight round-1 items and found no bypass under a fresh wrong-then-right and
misspelled-field attack. Three Low findings, all folded in:

1. **"A capability never outlives the run that issued it" was false.** Nothing notifies
   `shutdown_notify` and there is no signal handler, so the accept loop's cleanup is unreachable in
   practice: a killed daemon leaves both the capability file and the socket behind. The ADR and the
   code comment now say so, and explain why it is safe rather than tidy (a presentation is compared
   against the *running* daemon's in-memory value; the next run overwrites the file). A signal
   handler is recorded as ADR follow-up 3, not added this round. The `OnceLock::set` comment was
   also describing a hazard the line does not prevent — `set` drops the losing value — and now names
   what actually holds, the stale-socket liveness check.
2. **A refused capability was retried up to nine times.** `send_with_read_timeout` returned `Err`
   for "refusal plus daemon error", which `send_acknowledged` retries 3x and `mutate_current_once`
   multiplies by its own revision loop — contradicting that function's "retry only ambiguous
   transport failures" contract. A completed exchange carrying a refusal is now a terminal
   `Ok(WorkbenchDaemonResponse::Error { .. })`; `ok_result` still turns it into the caller's `Err`
   with both the daemon's refusal and the capability reason intact.
   `a_refused_capability_is_terminal_and_makes_exactly_one_connection` pins the connection count.
3. **`deny_unknown_fields` on the presentation payload, and a discarded refusal.** Serde has no
   variant-level `deny_unknown_fields`, so the payload became a named
   `impulse_ops::operator_capability::OperatorCapabilityPresentation` carried as a newtype variant —
   which serializes to the identical wire shape (`{"type": ..., "data": {"token": ...}}`), so this
   is a Rust-API change only. Both `DaemonRequest` and `WorkbenchDaemonRequest` use it. Separately,
   an `Ok` response after a refusal (an ungated request on a non-operator connection) no longer
   discards the refusal: it goes to `eprintln!`, which is that file's existing warning channel
   (`daemon_ops.rs:523`, `:1244`, `:1350`), rather than `tracing`, which the file does not use.

**Found while fixing these — a real ordering bug, not a test flake.** The capability was published
*after* `UnixListener::bind`, so a client that treats socket-connectable as readiness (which is what
every client here does, including `start_daemon` in the integration tests) could find no capability
file, silently act as a non-operator, and be refused. Publishing now happens before bind, so "the
socket is connectable" implies "the capability is on disk". The trade, documented at the call site:
a failed bind leaves a file for a daemon that never listened, which authenticates nothing and is
overwritten by the next run; the concurrent-double-start race it does not fix already exists for the
PID file and is what the liveness check is there to catch.

## Handoff Notes

### Conflict surface with the Codex lane `agent/codex-dioxus-packaged-acceptance-20260830`
That lane (5 commits, unpushed) touches `src/daemon/{mod,handlers,protocol}.rs` and bumps the
protocol to v7. Expected conflicts, all mechanical:

1. `impulse-ops/src/lib.rs` — `DAEMON_PROTOCOL_VERSION`. Both lanes set 7. Whichever merges second
   becomes 8, and updates the two `docs/validate_docs.py` markers plus `docs/IPC-PROTOCOL.md`
   headings accordingly.
2. `src/daemon/handlers.rs` — `ProcessRequestContext` gains `connection_class` here and
   `daemon_identity` there. Resolution: keep both fields; they are independent.
3. `src/daemon/handlers.rs` — `handle_governed_task_request` grew a third parameter
   (`connection_class`). If the Codex lane also changed that signature, keep both parameters.
4. `src/daemon/mod.rs` — the accept loop's `ConnectionContext` literal and the `handle_connection`
   destructuring both gain fields. Keep both lanes' fields.
5. `src/daemon/protocol.rs` — one new `DaemonRequest` variant plus its `request_type_name` arm.
   Additive; keep both.
6. `docs/validate_docs.py` — protocol-version required markers. This lane took the file for a
   three-line change (`**Protocol version: 7**`, `"protocol_version": 7`, and the new `### v7`
   changelog heading); the other lane needs the same lines. Take one version, not both.
7. `impulse-desktop/src/views.rs` — **one blocked-path line taken by necessity.** Adding
   `AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator` makes that crate's
   display `match` non-exhaustive, so the workspace would not build. The edit is a single arm
   rendering `"daemon-profiled evidence · authenticated operator"`, with no behavior change. Take
   either version on conflict.

### Cross-lane note from `claude/staged-worktree-scope-20260902` (ADR-0019)
That lane reports it also edits `impulse-rs/src/state/memory_candidate.rs`: inside
`derive_accepted_run_memory_candidate` it derives `accepted_task_revision` from
`operator.based_on_revision + 1` and relaxes the coherence check from `== task.revision` to
`<= task.revision`, so a post-acceptance promotion mutation does not make the projection bail. This
lane edits the same function (the assurance mapping) and `MemoryCandidateLedger::load` (stale
derivation pruning). The two hunks are in different parts of the file and are independent; on
conflict, keep both. That lane also asks that its future `PromoteGovernedOutcome` endpoint be
operator-class — `actor_provenance::authorize_governed_mutation` is where that check belongs, and
adding a `RecordPromotion`/`MaterializeStagedWorktree`/`DiscardStagedWorktree` arm there is the
whole change.

### Protocol version proposal (owner decision)
Both this lane and the Codex packaged-acceptance lane bump `DAEMON_PROTOCOL_VERSION` to 7. Since
#48 is an open PR and the Codex lane is five unpushed local commits, the proposal is: **#48 keeps
v7 and the Codex lane rebases to v8.** Whichever lane takes v8 also updates the three
version-keyed markers in `docs/validate_docs.py` (`**Protocol version: N**`,
`"protocol_version": N`, `### vN — <heading>`) and the matching headings in
`docs/IPC-PROTOCOL.md`. Exact conflict list is immediately below.

### Desktop client change: DONE, ownership block lifted for it
The ownership block on `impulse-desktop/src/daemon_ops.rs` was lifted for exactly this one-step
change, delivered as its own commit (`feat(desktop): present operator capability on daemon
connection (ADR-0018 handoff)`) so it can be dropped independently. Scope: capability presentation
on the connection about to carry a governed mutation, discovery through the shared
`impulse_ops::operator_capability` helper, one hoisted reader, refusal folded into the existing
error surface, three tests. No other desktop file was touched for it, and nothing around the
insertion was restructured — the Codex lane rewrites roughly 700 lines of that file.

### Original handoff description (superseded by the commit above, kept for the record)
`impulse-desktop/src/daemon_ops.rs` has its own socket client
(`WorkbenchDaemonRequest`/`send_acknowledged`). It sends `MutateGovernedTask` with
`RecordOperatorDecision` from the cockpit's Approve/Reject buttons (`ui.rs:1854`). After this lane
merges, those approvals are rejected with `operator capability required` until the desktop client
presents the capability. The change is one step in its connect path, mirroring
`impulse-rs/src/client/mod.rs`:

- resolve the capability: `std::env::var("IMPULSE_OPERATOR_CAPABILITY")`, else read
  `<socket_path>.operator-cap` (trimmed);
- immediately after connecting and before the real request, write one line
  `{"type":"PresentOperatorCapability","data":{"token":"<token>"}}` and read one response line;
- treat a failure as non-fatal for non-governed requests, and surface the daemon's typed error for
  governed ones.

The desktop host runs as the same user as the daemon and is not a governed pane, so it is entitled
to read the capability file. Governed panes must never receive `IMPULSE_OPERATOR_CAPABILITY`:
`impulse-desktop/src/runtime.rs`'s explicit governed env list must not add it (the PTY scrub in
`impulse-term/src/backend.rs` already strips every inherited `IMPULSE_*` key, including this one,
before that list is applied).

### Boundary this lane does not cross
The capability file is mode 0600 in a mode 0700 directory, owned by the daemon's uid. A same-uid
process that deliberately goes looking for it can still read it: this is a structural boundary
(a launched runtime is never *given* the capability, and its environment is scrubbed), not a
cryptographic one against a same-uid adversary. ADR-0018 states this explicitly.
