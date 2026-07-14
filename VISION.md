# IMPULSE — Feed the impulse to build.

- **Status:** Living product north star
- **Updated:** 2026-07-13
- **Canonical implementation contract:** [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
- **Current code boundary map:** [`docs/ARCHITECTURE-CLARIFICATION.md`](docs/ARCHITECTURE-CLARIFICATION.md)
- **Roadmap contract:** Now=control-plane foundations + daemon-owned governed runtime producers; Next=accepted-run memory promotion + stronger same-user actor authorization + full launched-runtime proof; Later=general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.
- **Current governed slice:** profiled pre-PTY Builder registration, exact acceptance criteria, daemon-attested clean Git subjects, daemon-derived claims and detached Rust verification, strict API-only Supervisor review, and operator-required acceptance.
- **Next governed slice:** review-only accepted-run memory promotion, stronger local actor authorization, and one full process proof with launched Builder and Supervisor runtimes.

## The promise

An impulse is the creative urge to make something. It is also the force that changes momentum.
Impulse exists to protect both: keep the original intent coherent, then accelerate it through a
managed system of coding agents without forcing the builder to become the full-time dispatcher,
historian, permission clerk, and completion auditor.

The product promise is simple:

> Start with the urge to build. Give it the right agents, context, tools, boundaries, and evidence.
> Keep the human in command while the work compounds.

Impulse is a terminal-native local control plane and harness manager for AI software-engineering
agents. It is Rust-first and designed to minimize control-plane overhead while preserving the CLI
workflows that already make Claude Code, Codex, and other coding harnesses useful. That is a design
goal, not an unbenchmarked performance claim.

## Why now, and why Impulse

Modern coding agents are individually capable but operationally fragmented. Each runtime has its
own session model, permissions, tools, context behavior, and completion language. Running several
of them across several projects produces terminal sprawl, duplicated work, context bleed, silent
conflicts, and unverified claims.

Impulse is differentiated by treating those agents as workers inside one governed local system:

- It preserves terminal-native runtimes instead of replacing them with a generic IDE imitation.
- It makes roles and policy more important than model/vendor identity.
- It supports both wrapped third-party harnesses and an Impulse-native runtime.
- It treats memory as a governed platform service with provenance, not an indiscriminate transcript dump.
- It separates worker claims, observed evidence, supervisor judgment, and user approval.
- It keeps the cockpit replaceable by making backend contracts authoritative.

## Product hierarchy

These concepts must stay separate:

```text
project (governance scope) <------> workspace target (execution root)
        |                                      |
        +-- task                               +-- agent process / PTY
        |   (assignment + criteria)                 |
        |          |                                +-- terminal/API channel
        |          +-- agent instance <-------------+
        |                 +-- role
        |                 +-- runtime
        |                 +-- session(s)
        |
        +-- memory, artifacts, policy, and verification

pane = a cockpit view onto an agent channel, never the authority
```

- A **role** defines behavior, permissions, tools, context, communication, and verification duties.
- A **runtime** is the execution engine or harness integration.
- An **agent instance** is one running identity assigned a role/runtime/scope.
- A **session** is bounded recorded work, not necessarily process lifetime.
- A **task** is the broader assignment concept. A **governed task** is today's durable daemon-owned
  carrier for that assignment and its acceptance evidence; it may outlive one runtime process.
  Reassignment/resume under a different agent or runtime is not implemented yet.
- A **pane** is presentation and input routing, not identity or authorization.
- A **workspace target** is the filesystem root used for execution.
- A **project** is the governance boundary for memory, artifacts, policy, and verification.

ADR-0011 makes the governed task ID durable and distinct from agent/session identity. The remaining
cardinalities, durable project identity, and task reassignment rules still require an ADR. This
hierarchy is enough to prevent the most harmful conflations without prematurely freezing the rest
of the schema.

## Roles, especially the supervisor

Roles are the stable product abstraction. Initial roles may include supervisor, builder, reviewer,
planner, researcher, tester, documentation, security review, and release.

A role contract eventually needs:

- allowed and prohibited actions;
- tool and credential grants;
- readable/writable project and filesystem scopes;
- context and memory visibility;
- message routes;
- planning/reporting/verification obligations;
- escalation and approval conditions;
- the minimum enforcement strength a runtime must provide.

The supervisor is structurally different from a builder. Its purpose is to observe workers, detect
conflicts or drift, request evidence, coordinate handoffs, curate durable memory, and escalate to the
user. A supervisor role should normally receive read/inspect/message/review capabilities while
source editing, destructive commands, and direct completion are omitted or separately approved.

Prompt text is not enough. The restriction must be represented by backend policy, tool omission,
filesystem/process boundaries, command mediation, audit, and confirmation gates wherever the
selected runtime supports them. A left-side terminal pane can represent the supervisor; it cannot
create the role merely by being leftmost or by running a particular model.

## Two runtime paths

### External harness path

Claude Code, Codex, OpenCode-compatible surfaces, and future CLIs already own internal agent loops.
Impulse wraps them with a controlled working directory, environment, PTY, process lifecycle,
project instructions, hooks, MCP/tool bridges, credentials, and telemetry. Interruption, restart,
and resume controls remain runtime- and adapter-capability dependent.

Impulse cannot replace hidden vendor prompts, proprietary reasoning, undocumented context
compression, or unsupported internal tool mechanics. External-runtime governance must therefore be
described by observed enforcement strength, never by blanket parity claims.

### Ion native path

Ion calls model providers directly and can let Impulse own prompt construction, tool schemas, loop
bounds, context assembly, memory retrieval, events, approvals, and structured results. It is the
path where role contracts can become most direct and testable.

Supporting Ion does not require abandoning established coding CLIs. Both paths belong in one
control plane and should share platform services where their capabilities allow it.

## Capability negotiation

Different runtimes support different operations. A future generalized/dynamic adapter contract must
report rather than hide those differences. Relevant capabilities include:

ADR-0010 makes one narrow part live: Dioxus previews a static, trusted code-owned launch-capability
comparison and the backend repeats it before PTY creation. This is conservative preflight over
declared wrapper support, not runtime probing, model-internal governance, or the future generalized
adapter negotiation contract.

- structured tools and MCP;
- hooks and structured events;
- custom instructions/system prompts;
- read-only or scoped filesystem modes;
- process interrupt, restart, and resume;
- typed status/events;
- secret isolation;
- session restoration;
- confirmation and policy interception.

Role assignment must produce a compatibility result: requirements satisfied structurally,
satisfied through Impulse mediation, advisory only, or unsupported. Unsupported mandatory
requirements must block launch; advisory degradation must be visible to the user.

## Shared platform services

Agents launched through Impulse should gain a coherent set of services even when the integration
mechanism differs by runtime:

- **Memory and retrieval:** durable decisions, session history, search, provenance, review-first injection.
- **Tools:** typed schemas, capability checks, invocation audit, errors, versions, and runtime bridges.
- **Telemetry:** process, task, command, test, tool, artifact, context, blocked, idle, and approval events.
- **Messaging and handoffs:** typed, acknowledged, scoped agent-to-agent/supervisor communication.
- **Policy:** role obligations, permissions, confirmation, guardrails, and enforcement strength.
- **Credentials:** least-privilege provider access without secret values entering logs or memory.
- **Artifacts:** typed outputs with project/agent/session provenance and review actions.
- **Verification:** project-specific commands, evidence capture, supervisor review, and user approval.

For Ion these can be direct typed calls. For external CLIs they may be exposed through MCP, hooks,
local sockets, generated commands, files, or mediated PTY operations. The conceptual capability can
be shared without pretending its enforcement is identical.

## Memory and context isolation

Memory is one of Impulse's strongest services and one of its largest trust risks. Durable memory
must distinguish facts, decisions, preferences, hypotheses, and temporary state; carry source and
scope; support correction/forgetting; and avoid treating worker prose as verified truth.

Every memory, event, artifact, tool call, and message needs an explicit scope such as global user,
project, repository, branch, task, agent, or session. Broad supervisor visibility must never imply
broad worker visibility. Secrets must not become memory, terminal logs, model context, or
cross-project payloads.

## Events, attention, and resources

The supervisor cannot consume every terminal token. Impulse should publish meaningful state
changes, summarize noisy output, suppress repetition, prioritize blockers/conflicts/approvals, and
retain enough raw provenance for inspection.

Events are durable only when they matter for audit, replay, memory, or evidence; high-volume
terminal details can remain ephemeral or summarized. Resource policy should bound concurrent
agents, model/API budgets, output retention, indexing, context size, and idle activity. The guiding
principle is useful supervision per unit of attention, not surveillance volume.

## Verification and evidence

"Done" is a policy result, not a phrase emitted by a worker. A completion record can include:

- acceptance criteria mapped to changed behavior;
- build, test, lint, format, security, and integration commands;
- exact command outcomes and output references;
- required/forbidden file checks;
- diff and artifact review;
- supervisor judgment;
- explicit user approval for high-impact actions.

Impulse must keep four things distinct: worker claim, observed evidence, supervisor judgment, and
user approval. Each can disagree with the others, and the interface must show that disagreement.

### Current governed-task boundary

That distinction is now executable for profiled governed Builder launches. The daemon owns a
persistent task record and append-only typed events; mutations carry an idempotency request ID and
expected revision. A `rust_workspace_v1` launch requires exact acceptance criteria, the canonical
Git worktree root, a clean committed `HEAD`, and an initial OID independently re-observed by the
daemon before PTY creation.
Execution state (`registered`, `running`, `launch_failed`, `runtime_exited`) changes independently
from review state. Passing verification may reach `awaiting_supervisor`; only a supervisor
`recommend_accept` verdict can reach `awaiting_operator`; only an operator approval can produce
`accepted`.

On restart, the daemon replays the stored event/record chain and requires one valid idempotency
receipt per revision before trusting the materialized state. This catches corrupt or incoherent
files; it is not a signature against a same-user process capable of rewriting the entire ledger.
Receipts deduplicate already-persisted requests, and one per-task lock serializes live producer and
lifecycle mutations. Producer execution is not crash-safe exactly-once: daemon death after a
command/model side effect but before its durable receipt can cause a retry to repeat that side
effect. A durable producer reservation journal is required to close that boundary.

The Builder supplies only a bounded summary and artifact IDs through the env-routed
`"$IMPULSE_CONTROL_CLI" --daemon governed-claim` command or Ion's typed
`governed_submit_claim` tool. The packaged executable is `impulse-rs`; the environment variable
retains the exact launched path. The daemon derives the assigned Worker actor and current clean Git
OID. It then verifies the exact claimed commit in a detached Git
worktree with a closed format/locked-check/locked-strict-Clippy/locked-test argv profile, a required
committed regular non-symlink root `Cargo.lock`, a symlink-free source tree, a scrubbed environment,
bounded timeouts, streaming output digests,
process-group cleanup, and a bounded before/after byte manifest that includes ignored source-tree
paths. Command evidence persists a
display-safe executable and fixed arguments plus SHA-256 digests, byte counts, and truncation flags;
raw output is not part of the task record.

The real daemon/CLI process test proves claim submission, detached verification, durable evidence,
and restart recovery only through `awaiting_supervisor`. Separate in-process daemon handler tests
prove strict API-only Supervisor review and operator-only acceptance.

This verifier executes host-trusted Rust build scripts, proc macros, and tests. Detached checkout,
environment scrubbing, timeout, and cleanup are containment measures, not an OS sandbox. Projects
that are not trusted to execute locally must not use this profile.

Supervisor review is a stateless, tool-free, temperature-zero API turn. Its strict JSON envelope
must echo the exact task revision, claim and verification IDs, subject revision, acceptance-criteria
count, and acceptance-criteria digest. Generic coding-harness configuration fails closed before
spawn because Impulse cannot guarantee a structurally read-only harness turn. Dioxus shows the
result and guides the operator to the terminal producer commands; producer buttons are not live.

Typed actor kinds provide provenance and transition checks, not cryptographic same-user role
authentication. The daemon socket directory/socket/PID file are restricted to the local user, but
another process running as that user is inside the current trust boundary. Project identity is
currently derived from the bound project directory name, one daemon adapter is bound to one
project, task reassignment/resume is not implemented, and the global task/receipt maps do not yet
have pagination or archival. These limits must remain visible until stronger contracts replace them.

## User control and trust

Users must be able to inspect which runtime and role are active, what each agent can access, why a
context item was selected, what is remembered, which tool or credential boundary applied, what data
crossed a scope, why the supervisor intervened, and whether an action was automatic or approved.

The backend is authoritative, but authority must remain legible. Every meaningful policy decision
needs a reason and an audit path. The user can interrupt, narrow, revoke, review, or reject.

## Live foundation versus target state

### Live foundation

- Rust daemon and versioned workbench protocol/read models.
- PTY spawn/write/resize/focus/exit lifecycle and process cleanup.
- Dioxus Desktop host/bridge plus ratatui and CLI operator surfaces.
- Registry-backed desktop platform identity and launch metadata.
- Explicit product-role/task launch preflight, backend mandatory-capability gate, and typed
  compatibility telemetry.
- Daemon-owned governed task records with pre-PTY registration, revisioned/idempotent mutations,
  independent execution/review state, durable lifecycle events, and operator-required acceptance.
- Explicit `rust_workspace_v1` registrations with exact criteria and daemon-attested clean initial
  Git OIDs; env-routed Builder claims through CLI and Ion's `governed_submit_claim` tool.
- Daemon-owned detached Rust verification and strict criteria-digest-bound, API-only, tool-free,
  history-free Supervisor review. Generic external harness review fails closed.
- Dioxus governed-task evidence and decision cards backed by acknowledged host commands; no
  optimistic task state. Profiled cards currently guide terminal producer commands rather than
  exposing producer buttons.
- Owner-only, write-ahead desktop lifecycle outbox with cross-process locking for launch/exit
  reconciliation across ambiguous daemon transport failures. Abrupt desktop death before an exit
  intent exists still requires a future runtime lease/orphan-reconciliation contract; missing task
  targets are retained until a future durable registration-tombstone/expiry policy can resolve them.
- Ion as a real desktop-launchable builtin platform.
- Daemon-truth terminal telemetry across lifecycle and heartbeat.
- Supervisor-specific permission/confirmation policy and daemon actions.
- Typed capability-checked tooling, MCP surfaces, and audit paths.
- Project memory, FTS5/semantic retrieval, context stewardship, and review-first injection.
- Credential provider abstraction, artifacts, delegations, and verification gates.
- Ion interactive native coding runtime with direct providers, typed tools, guardrails, and loop bounds.

This contract describes the current repository implementation. Inclusion in a tagged release is
a separate action and must not be inferred from repository state alone.

### Target state

- General role contracts independent of model/runtime/pane.
- One adapter contract with required, optional, emulated, and unsupported operations.
- Capability negotiation with explicit enforcement strength.
- Typed cross-agent messaging, acknowledgements, routing, and project isolation.
- Role-scoped tools, credentials, context, memory, and verification policies.
- Event-driven supervisor attention and resource budgets.
- Review-only accepted-run memory promotion, stronger same-user actor authorization, and a
  process-level proof across launched Builder and Supervisor runtimes.

## First complete vertical slice

The first proof of the full product is one governed workflow, not a partial version of every
subsystem:

1. The user opens Impulse and selects a registered project/workspace.
2. The user selects a supervisor role and a compatible runtime.
3. Impulse negotiates capabilities and shows enforcement strength/degradation before launch.
4. The user launches one builder with an explicit task, role, workspace, tools, and policy.
5. The builder works in its normal terminal/native runtime while Impulse captures typed state changes.
6. The supervisor receives prioritized summaries and can request context, evidence, or revision.
7. The builder uses at least one Impulse-provided typed tool and produces a provenance-bearing artifact.
8. Project verification runs and records exact evidence.
9. The supervisor recommends acceptance, requests changes, or escalates based on evidence; the
   operator explicitly approves or rejects an accept recommendation.
10. Only accepted, verified outcomes become scoped durable memory, with an inspectable reason and source.

The explicit Builder preflight and daemon-owned governed producers establish the authoritative
parts of steps 3-5 and 8-9: registration precedes PTY creation, exact criteria and a clean Git
subject are bound to the task, the daemon derives claim and verification truth, strict API review is
revision-bound, runtime exit is never acceptance, and the operator owns final approval. Real-process
and restart tests prove the CLI claim and detached verification path; handler tests prove strict
Supervisor binding and operator-only acceptance. The remaining proof must compose launched Builder
and Supervisor runtimes through the complete path and promote only an accepted result to a
review-only memory candidate. The complete multi-runtime workflow is therefore not yet closed.

## Success criteria

- A user can manage heterogeneous coding agents without losing their terminal-native strengths.
- Role, runtime, instance, session, task, pane, workspace, and project are never conflated in state or UI.
- Mandatory role requirements block incompatible launches; degradation is explicit.
- The daemon remains the operational source of truth across cockpit restarts.
- Project context, credentials, memory, and messages do not silently cross scopes.
- Tool calls and supervisor interventions are auditable and understandable.
- Completion status is backed by observed evidence rather than worker prose.
- The control plane remains locally operable and its overhead is measured before performance claims.

## Non-goals

- Reimplementing every proprietary coding harness inside Impulse.
- Pretending all runtimes expose equivalent control or tool semantics.
- Replacing terminal workflows with a conventional IDE.
- Making the Dioxus component tree the source of backend truth.
- Storing every terminal token forever or promoting every agent statement to memory.
- Giving a continuous supervisor unrestricted builder permissions.
- Building every role, provider, or multi-project feature before one complete vertical slice works.

## Open ADR decisions

ADR-0010 accepts the product-role/task launch preflight, ADR-0011 accepts the daemon-owned
governed-task lifecycle, and ADR-0012 accepts the first daemon-owned producer profile. The following
broader decisions remain open and must be resolved before schema-specific documents split out:

1. Remaining hierarchy/cardinality and durable ids for project, workspace, role, runtime, instance,
   session, pane, and supervisor scope, plus governed-task reassignment/resume.
2. Minimum runtime-adapter interface and semantics for optional/emulated operations.
3. Generalized and dynamic capability negotiation beyond the static desktop preflight, including
   discovery, attestation freshness, emulation, and post-launch re-evaluation.
4. Role contract composition, override, persistence, and migration.
5. Cross-agent message routing, direct worker communication, acknowledgement, and isolation.
6. Memory authorship, verification, conflict resolution, correction, forgetting, and inheritance.
7. Credential grants, revocation, audit, redaction, and cross-project prevention.
8. Supervisor scheduling, attention summaries, context budget, and intervention priority.
9. Verification profiles and whether any future low-risk policy may relax the current
   operator-required final approval rule.
10. Resource budgets and measurable control-plane performance targets.

Until those decisions land, do not create separate `ROLES.md`, `RUNTIMES.md`, `SUPERVISOR.md`, or a
replacement architecture schema. This north star, the canonical contract, and one future ADR set
are the compounding sources of truth.
