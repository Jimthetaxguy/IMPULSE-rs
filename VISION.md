# IMPULSE — Feed the impulse to build.

- **Status:** Living product north star
- **Updated:** 2026-07-12
- **Canonical implementation contract:** [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
- **Current code boundary map:** [`docs/ARCHITECTURE-CLARIFICATION.md`](docs/ARCHITECTURE-CLARIFICATION.md)
- **Roadmap contract:** Now=control-plane foundations; Next=one governed supervisor/builder vertical slice + hierarchy/enforcement ADR; Later=general roles + negotiated runtimes; Legacy=egui compile-maintenance only.

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
project / workspace scope
        |
        +-- task (assignment + completion criteria)
        |      |
        |      +-- agent instance
        |             +-- role
        |             +-- runtime
        |             +-- session(s)
        |             +-- terminal/API channel
        |
        +-- project memory, artifacts, policy, and verification

pane = a cockpit view onto an agent channel, never the authority
```

- A **role** defines behavior, permissions, tools, context, communication, and verification duties.
- A **runtime** is the execution engine or harness integration.
- An **agent instance** is one running identity assigned a role/runtime/scope.
- A **session** is bounded recorded work, not necessarily process lifetime.
- A **task** is an assignment and its acceptance evidence; it may span sessions.
- A **pane** is presentation and input routing, not identity or authorization.
- A **workspace target** is the filesystem root used for execution.
- A **project** is the governance boundary for memory, artifacts, policy, and verification.

The exact cardinalities and durable identifiers remain an ADR decision. This hierarchy is enough to
prevent the most harmful conflations without prematurely freezing the schema.

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

Different runtimes support different operations. A future adapter contract must report rather than
hide those differences. Relevant capabilities include:

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
- Open registry-backed desktop platform identity and launch metadata.
- Ion as a real desktop-launchable builtin platform.
- Daemon-truth terminal telemetry across lifecycle and heartbeat.
- Supervisor-specific permission/confirmation policy and daemon actions.
- Typed capability-checked tooling, MCP surfaces, and audit paths.
- Project memory, FTS5/semantic retrieval, context stewardship, and review-first injection.
- Credential provider abstraction, artifacts, delegations, and verification gates.
- Ion interactive native coding runtime with direct providers, typed tools, guardrails, and loop bounds.

This contract describes the verified local implementation. Publishing a remote release is a
separate action and must not be inferred from local branch integration.

### Target state

- General role contracts independent of model/runtime/pane.
- One adapter contract with required, optional, emulated, and unsupported operations.
- Capability negotiation with explicit enforcement strength.
- Typed cross-agent messaging, acknowledgements, routing, and project isolation.
- Role-scoped tools, credentials, context, memory, and verification policies.
- Event-driven supervisor attention and resource budgets.
- Evidence-backed task completion visible in one cockpit.

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
9. The supervisor accepts, rejects, or escalates based on evidence; the user sees and controls the decision.
10. Only verified outcomes become scoped durable memory, with an inspectable reason and source.

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

The following decisions must be resolved together before schema-specific documents split out:

1. Exact hierarchy/cardinality and durable ids for project, workspace, role, runtime, instance,
   session, task, pane, and supervisor scope.
2. Minimum runtime-adapter interface and semantics for optional/emulated operations.
3. Capability/enforcement-strength vocabulary and launch-blocking rules.
4. Role contract composition, override, persistence, and migration.
5. Cross-agent message routing, direct worker communication, acknowledgement, and isolation.
6. Memory authorship, verification, conflict resolution, correction, forgetting, and inheritance.
7. Credential grants, revocation, audit, redaction, and cross-project prevention.
8. Supervisor scheduling, attention summaries, context budget, and intervention priority.
9. Verification profiles and who has final approval by impact class.
10. Resource budgets and measurable control-plane performance targets.

Until those decisions land, do not create separate `ROLES.md`, `RUNTIMES.md`, `SUPERVISOR.md`, or a
replacement architecture schema. This north star, the canonical contract, and one future ADR set
are the compounding sources of truth.
