# Governed Runtime Producers Implementation Plan

## Outcome

One closed-loop Rust profile proves that a launched Builder can request a completion claim, while
the project daemon independently binds that claim to a clean Git commit, executes bounded
verification, obtains a strict model-backed Supervisor verdict, and leaves final acceptance to the
operator. Automatic producer truth cannot be supplied through the generic mutation endpoint.

## Dependency order

1. **Shared producer contract**
   - Add `rust_workspace_v1`, optional registration/task profile fields, claim/verify/review request
     DTOs, and a strict Supervisor response envelope to `impulse-ops`.
   - Add shared daemon request variants and bump protocol compatibility with serde defaults for old
     payloads.
   - Prove validation and wire round trips before daemon implementation.
2. **Git-subject and bounded-process core** (blocks every producer)
   - Add one `governed_producers` module that resolves the canonical Git root/OID, requires a clean
     worktree, checks initial-OID ancestry, expands only the fixed Rust profile, and runs direct argv
     with scrubbed env, timeout/reaping, and streamed digests/counts/truncation without production
     output previews.
   - Unit-test non-Git/dirty/mismatched subjects, command failure, output truncation, timeout, and
     absence of inherited secret-shaped environment variables.
3. **Daemon-owned claim and verification**
   - Route specialized producer requests separately from caller-composed mutations.
   - Derive Worker/Verifier actors and subject/evidence payloads inside the daemon.
   - Share one per-task lock across producer and lifecycle mutations, then preserve
     expected-revision CAS and persisted request-id replay semantics without rerunning a completed
     verification on retry within that boundary.
   - Reject automatic producer mutations through the generic endpoint for profiled tasks.
4. **Daemon-owned Supervisor turn**
   - Build a bounded, injection-aware prompt from typed records only.
   - Reuse the daemon's single cached Impulse Agent turn lock, but permit only API mode for this
     history-free, tool-free, temperature-zero review; generic harness mode fails closed.
   - Strict-parse the full response envelope, validate every bound ID/revision, derive the Supervisor
     actor, bind the acceptance-criteria digest, and record through the existing state transition.
5. **Agent and operator surfaces**
   - Add daemon-only `impulse-rs --daemon governed-claim` using injected project/task/socket
     routing; launched panes use `"$IMPULSE_CONTROL_CLI" --daemon governed-claim`.
   - Add `governed_submit_claim` to Ion's typed REPL tool registry.
   - Add explicit acceptance-criteria/profile launch fields and inject project/socket/profile env.
   - Render daemon-backed evidence plus routed `"$IMPULSE_CONTROL_CLI" --daemon governed-verify` /
     `"$IMPULSE_CONTROL_CLI" --daemon governed-review` guidance in Dioxus; retain explicit operator
     approval. Producer buttons remain out of scope.
6. **Process-level proof and documentation**
   - Run real init in a fresh Git repository, commit its durable project inputs, start a real daemon
     on a temporary Unix socket, use the real CLI plus deterministic Rust child processes for
     claim/verification, then assert the durable `awaiting_supervisor` record after daemon restart.
     This process test does not perform Supervisor review or operator acceptance.
   - Use an in-process deterministic API provider for strict Supervisor binding and operator-only
     acceptance; separately prove generic harness review fails before spawn.
   - Reconcile README, VISION, CONTEXT, architecture, IPC, CLI, canonical contract, story map, and
     traceability without claiming strong actor authentication, generalized runtimes, or semantic
     memory promotion.
7. **Review and release gate**
   - Run independent state/security, subprocess, Supervisor, integration, and docs reviews.
   - Repair all P0/P1 findings.
   - Run format, workspace check, strict Clippy, workspace tests, docs validation, Dioxus host smoke,
     real sibling-Ion smoke, diff check, real-systems scan, and staged Gitleaks before committing.

## Explicit non-goals

- Dirty-worktree or untracked-file subject attestation.
- Arbitrary/project-authored verification commands.
- Node, Python, or generalized runtime verification profiles.
- Cryptographic same-user process identity, peer-PID binding, or sandbox attestation.
- Automatic acceptance, semantic `GENOME.md` writes, retrieval indexing, or injection.
- Persistent/launched Supervisor terminal, multi-project routing, task reassignment, or scheduler policy.
- Claims that Claude Code, Codex, or other external CLIs receive a native typed-tool transport.
- Crash-safe exactly-once producer execution; persisted receipts do not replace a future durable
  pre-side-effect reservation journal.

## Completion evidence

- Shared contract and protocol tests cover defaults, validation, and exhaustive variant mapping.
- Unit tests cover Git and process invariants rather than only mocked state transitions.
- The process test observes actual child exit codes/output limits and persisted task records.
- UI/host tests prove profiled cards expose command guidance, operator controls wait for daemon
  acknowledgements, and no optimistic state is applied.
- Full repository gates are green on the final diff.
