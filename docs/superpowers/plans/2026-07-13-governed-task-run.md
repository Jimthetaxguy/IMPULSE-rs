# Governed Task Run Implementation Plan

## Outcome

One serialized, restart-safe flow proves that a governed Builder launch is registered before its PTY,
produces a typed claim and verification record, receives a revision-bound supervisor judgment and
operator decision, and remains visible from daemon truth after the runtime disappears.

## Dependency order

1. **Shared contract**
   - Add governed task identities, records, lifecycle, events, inputs, and validation helpers to
     `impulse-ops`.
   - Add request variants and a backwards-compatible `ProjectOpsSnapshot.governed_tasks` field.
   - Bump the shared daemon protocol version and update exhaustive protocol compatibility tests.
2. **Daemon authority** (blocks desktop integration)
   - Add a project-local persistent ledger with serialized mutation, expected-revision CAS, and
     request-id replay handling.
   - Route register/get/list/mutate requests through the daemon.
   - Project the ledger into ops snapshots.
   - Prove transition invariants, restart recovery, and concurrent decisions.
3. **Runtime pre-PTY registration** (blocks complete launch proof)
   - Return a command-capable client beside the daemon telemetry sink.
   - Inject a mockable task gateway into `DesktopRuntime`.
   - Register after final preflight/agent-id generation and before reservation/PTY creation.
   - Propagate the task ID through runtime records, snapshots, child environment, and telemetry.
   - Record launch failure, running state, and runtime exit without inferring completion.
4. **Operator surface**
   - Render governed task state from `ProjectOpsSnapshot` in the Supervisor surface.
   - Add acknowledged host commands for claim/verification/judgment/approval mutations needed by the
     vertical slice.
   - Wait for daemon snapshot reconciliation before presenting a successful decision.
5. **Contract/documentation alignment**
   - Update the canonical contract, story map, traceability, context glossary, architecture, and
     README status language so implemented facts and future direction remain distinct.
6. **Verification and review**
   - Run focused tests after each dependency boundary.
   - Run independent contract, concurrency/security, and UI/runtime reviews.
   - Run formatting, workspace check, strict Clippy, workspace tests, docs validation, host smoke,
     real-Ion checks, diff check, and leak scan before committing.

## Explicit non-goals

- Cryptographic actor identity or multi-user authorization.
- Automatic durable-memory promotion.
- General runtime-adapter trait composition.
- Cross-project daemon routing from one desktop instance.
- Treating arbitrary terminal output as verification evidence.
