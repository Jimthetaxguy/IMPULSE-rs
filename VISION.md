# IMPULSE — Feed the impulse to build.

- **Status:** Living product north star
- **Updated:** 2026-10-04
- **Canonical implementation contract:** [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
- **Current code boundary map:** [`docs/ARCHITECTURE-CLARIFICATION.md`](docs/ARCHITECTURE-CLARIFICATION.md)
- **How the vision got here:** [`docs/VISION-HISTORY.md`](docs/VISION-HISTORY.md)
- **Product requirements:** [`docs/spec/2026-10-03-impulse-ion-photon-prd.md`](docs/spec/2026-10-03-impulse-ion-photon-prd.md)
- **Evidence base for this revision:** `claude/ion-blackboard-20261003` (stacked on the photon and
  typed-endpoint lane): 3,225 workspace tests passed, 0 failed, 10 ignored on 2026-10-03, clippy
  `-D warnings` clean with and without default features.

Every statement below is tagged **[built]**, with the code or ADR that proves it, or
**[not built]**, with what exists today instead. A thesis is labeled as a thesis. Code paths are
relative to `impulse-rs/`.

## In one sentence

Impulse is a local, Rust, terminal-native application that runs coding agents (its own, Ion, and
external ones such as Claude Code and Codex), keeps their context small, bounds what they can
touch, and accepts their work only on observed evidence and an operator's decision.

## Thesis

**Computing 3.0 is objective-compiled computing.** In the first era people wrote instructions; in
the second, models wrote them from prompts. In the third, a person states an objective with
acceptance criteria, and a harness compiles it into bounded agent work, checks the result against
the criteria, and records the judgment that accepted it. The objective, not the prompt, is the
source program.

**Impulse is meant to become a compiler for professional judgment.** A professional's judgment is
the part of the work that survives tool changes: what counts as done, what is too risky to run, what
evidence is enough, what is worth remembering. Impulse turns each of those into a typed, versioned
artifact the harness enforces instead of prose an agent may ignore.

What already compiles, and where:

| Judgment | Compiled into | Status |
|---|---|---|
| What counts as done | Exact acceptance criteria bound to a governed task (`rust_workspace_v1`, ADR-0011, ADR-0012) | [built] |
| What evidence is enough | Daemon-run detached verification of the claimed commit (ADR-0012, ADR-0019) | [built] |
| Whether the work is good | Supervisor review bound to task revision, claim, verification, and criteria digest (ADR-0012) | [built] |
| Who may accept | Operator-class connections only (ADR-0018) | [built] |
| What is too risky to run | Guardrail rules scanned before every gated Ion tool call (`src/guardrail`, `ion_repl::chat`) | [built] |
| How long work may run | Typed loop budgets with termination evidence (ADR-0017) | [built] |
| What is worth remembering | Review-only memory candidates, promoted or dismissed by the operator (ADR-0013, ADR-0020) | [built, daemon endpoint and UI not built] |
| Which model does which step | Harness-owned step model and typed endpoints (ADR-0015, ADR-0022) | [stage 1 built] |

The thesis stays a thesis until one objective runs end to end through launched Builder and
Supervisor runtimes and produces exactly one accepted memory candidate. That proof is the next
forcing slice ([First complete slice](#first-complete-slice)).

## Two modes

| Mode | Purpose | Status |
|---|---|---|
| `impulse run` | One person and Ion, Impulse's own coding agent, in one workspace | [not built as a subcommand] Today Ion ships as the `ion` binary (`src/bin/ion.rs`, REPL in `src/ion_repl/`). `impulse-rs run` opens the ratatui workbench. |
| `impulse harness` | An orchestrator that delegates to Ion, Claude Code, or Codex workers under governance | [not built] Today the daemon and the Dioxus cockpit launch governed Builder panes (ADR-0010, ADR-0019) and the operator orchestrates by hand. |

The two modes share one daemon, one `.impulse/` directory, one governed-task store, and one
blackboard. The orchestrator's tool surface is fixed at five names (below); the rest of the
catalog is reached through search.

## MCP on the wire, CLI in the sandbox

Across a process boundary Impulse speaks a typed protocol. Inside an agent's sandbox it gives the
agent a command line.

- **On the wire [built].** The daemon serves a versioned JSON-line protocol over a Unix socket
  (`PROTOCOL_VERSION` 9, `impulse-ops/src/lib.rs`, `docs/IPC-PROTOCOL.md`). `impulse-rs mcp` serves
  the dynamic tool registry over MCP on stdio or loopback TCP (`src/mcp/server.rs`), with bounded
  request reads. The Dioxus cockpit exposes its own MCP surface (`impulse-desktop/src/mcp.rs`).
- **In the sandbox [built].** A governed Builder pane receives `IMPULSE_CONTROL_CLI` and runs
  `"$IMPULSE_CONTROL_CLI" --daemon governed-claim`, `governed-verify`, and `governed-review`. A
  command costs the agent no tool schema, works in any harness that can run a shell, and documents
  itself with `--help`.

The reason is the context tax. Every MCP tool a model is offered costs its schema on every turn,
whether it is used or not. A CLI costs nothing until it is called.

## Context: the blackboard and demand paging

Large results do not belong in a model's context, and a model does not need every tool schema on
every turn.

- **Blackboard [built, ADR-0023].** `.impulse/blackboard.db` (SQLite through `rusqlite`) holds
  off-context results keyed by `task_id`, with TTL purge on open, at daemon start, and on an
  interval. Live keys are insert-only (`src/blackboard/mod.rs`).
- **Spill [built].** Any Ion tool result over 4 KiB (configurable, 1 KiB to 1 MiB) is stored and
  replaced by a reference with a preview and a hash; the guard scan still sees the full output
  (`ion_repl::chat::ReplToolExecutor::finish_result`). A governed claim summary over the 4 KiB
  claim limit is stored in the canonical project's blackboard and travels as a `blackboard:`
  artifact id.
- **Demand paging [built].** `blackboard_fetch` returns one window of at most 8 KiB, optionally
  narrowed by an RFC 6901 JSON pointer (`src/blackboard/projection.rs`). Context compaction keeps
  the key, so a spilled result stays reachable after its reference leaves history.
- **Three meta-tools [built].** `blackboard_store`, `blackboard_fetch`, and `search_tools`
  (`src/ion_repl/tool_blackboard.rs`, `tool_search.rs`). `search_tools` returns names and one-line
  descriptions, and schemas only when asked.
- **Five core tools [defined, not enforced].** `ORCHESTRATOR_TOOL_SURFACE`
  (`src/ion_repl/registry.rs`) fixes the orchestrator at `search_tools`, `blackboard_store`,
  `blackboard_fetch`, `delegate_task`, `approve_gate`, with a test that keeps it at five.
  `delegate_task` and `approve_gate` are reserved names. Ion still advertises its full registry
  every turn; adding tools to a turn only after `search_tools` finds them is not built.

## The four-layer truth model

Completion is never a worker's word. Four layers, each owned by a different party, each recorded:

1. **Claim [built].** The worker says what it did. Builder claims go through the CLI or Ion's
   confirmation-gated `governed_submit_claim`; the daemon derives actor and Git subject itself
   (`src/ion_repl/tool_claim.rs`, ADR-0012).
2. **Evidence [built].** The daemon verifies the claimed commit in a detached worktree with fixed
   Rust commands. A staged Builder worktree is materialized, promoted, or discarded only by the
   daemon (ADR-0019, protocol v9). Durable producer reservations reconcile interrupted runs
   (`PRODUCER_RESERVATIONS.json`).
3. **Supervisor [built].** Review is API-only, tool-free, history-free, and temperature-zero, bound
   to the task revision, claim, verification, subject, and acceptance-criteria digest. A generic
   external harness fails closed because it cannot guarantee a read-only turn (ADR-0012).
4. **Operator [built].** Acceptance requires an operator-class connection, established by peer
   credentials plus a per-daemon-run capability (ADR-0018). Promote and Discard for staged work are
   operator controls in the cockpit (ADR-0019).

Runtime exit is never acceptance, and an accepted run only proposes memory (ADR-0013).

## Three-layer isolation

| Layer | What it bounds | Status |
|---|---|---|
| **Capability sandbox** | What code and tools can reach | [built] `calculator` and `python_exec` run in the in-process Monty interpreter with no filesystem, network, process, or host-function access, 64 MiB and 5 s limits (ADR-0021). Ion's bridged tools run under sandbox roots: writes only in the repo root, reads in the repo root plus explicit `/allow` grants (`ReplContext::sandbox_tool_context`). Photon reads through a `cap-std` capability. `bash_exec` gets a scrubbed environment allowlist (`src/tooling/env_scrub.rs`) and a whole-process-group kill on timeout. |
| **MicroVM** | What a shell command or verification can touch at the OS level | [not built] There is no Firecracker integration. Today: a staged Builder works in its own Git worktree (ADR-0019), and verification "executes host-trusted Rust code and is not an OS sandbox" (`CLAUDE.md`). Shell commands from Ion require confirmation, with guardrail-driven escalation to a typed `CONFIRM`. |
| **Budget caps** | How long and how much | [partly built] Every Impulse-owned loop runs under a `LoopContract`: Ion defaults to 10 rounds and 180 s with repeated-call and same-error breakers (ADR-0017, `src/loop_contract.rs`). Photon is capped at 5 runs per session and half Ion's wall clock (`PHOTON_SESSION_LIMIT`). Daemon harness turns time out at 120 s. Token and money budgets are not built. |

## Runtimes and models

- **Ion [built].** Impulse's own coding agent: REPL, tool-calling loop, confirmation gate,
  untrusted-output envelope, sandbox roots, loop contract (`src/ion_repl/`, `src/llm_backends/`).
- **Photon [built, ADR-0022 stage 1].** A disposable read-only subagent Ion calls inside one tool
  call: a fresh `min-agent` run with `list_files`, `read_file`, `search_text`, keeping no
  transcript, memory, or session (`src/ion_repl/tool_photon.rs`, default feature
  `photon-subagent`).
- **External harnesses [built].** Claude Code, Codex, Gemini CLI, and Cursor from one agent
  registry (`impulse-ops/src/agent_registry.rs`), launched into PTYs with a controlled environment.
  Impulse cannot see or replace their internal loops; governance of them is stated by observed
  enforcement, never by parity claims.
- **Typed model endpoints [stage 1 built, ADR-0022].** An endpoint is a wire protocol
  (`anthropic_messages`, `openai_chat`, `openai_responses`), a base URL with its path, auth by
  environment-variable name, a model id, and an output limit, in named `config.json` profiles
  assigned to roles (`src/model_endpoint/`). Photon resolves endpoints today. Ion's own provider
  path, a provider trait with capability metadata, one retry/fallback policy, and local-first
  routing are stage 2 [not built].
- **Step model [built, ADR-0015].** The harness chooses the model for each step, including
  escalation after a verifier failure (`impulse-step-model`).

## Memory

Memory is a governed service, not a transcript. `GENOME.md` is hand-curated; `HISTORY.jsonl` is an
append-only session log; FTS5 and semantic retrieval index both (`src/retrieval/`). Accepted runs
stage review-only candidates (ADR-0013); promotion writes `MEMORY.jsonl` and a separate
`GENOME_PROJECTION.md` (ADR-0020) [state and types built; the daemon decision endpoint, cockpit
controls, and Ion integration are not built]. The blackboard is working state, not memory: it
expires.

## Surfaces

- **Dioxus cockpit [built, ADR-0008].** `impulse-desktop`, Dioxus 0.6.3 with an xterm.js terminal
  bridge, governed-task evidence and decision cards, Promote and Discard controls. It projects
  daemon truth and owns none.
- **ratatui workbench and CLI [built].** `impulse-rs run` and the `impulse-rs` subcommands.
- **egui [legacy].** Removed from the workspace on 2026-04-17; source frozen.

## First complete slice

The thesis is proven by one objective, not by a partial version of every subsystem:

1. Register a project and an objective with exact acceptance criteria. [built]
2. Launch a Builder (Ion, Claude Code, or Codex) into a staged worktree under policy. [built]
3. The Builder works, keeping large results on the blackboard. [built for Ion]
4. The Builder claims; the daemon verifies the claimed commit. [built]
5. A launched Supervisor runtime reviews against the criteria digest. [API review built; launched
   Supervisor runtime not proven end to end]
6. The operator accepts; the daemon promotes the staged work. [built]
7. Exactly one memory candidate appears, and the operator promotes or dismisses it. [candidate
   built; promotion endpoint and controls not built]

Steps 5 and 7 are the open work. Until they close, the vertical slice is not complete.

## Built today

| Area | Evidence |
|---|---|
| Daemon, protocol v9, governed tasks and producers | `src/daemon/`, `impulse-ops/src/governed_task.rs`, ADR-0011, ADR-0012, ADR-0019 |
| Actor provenance | `src/daemon/actor_provenance.rs`, ADR-0018 |
| Ion and its tool floor | `src/ion_repl/`, ADR-0017 |
| Photon | `src/ion_repl/tool_photon.rs`, ADR-0022 |
| Typed endpoints, stage 1 | `src/model_endpoint/`, ADR-0022 |
| Blackboard, spill, paging, meta-tools | `src/blackboard/`, `src/ion_repl/tool_blackboard.rs`, `tool_search.rs`, ADR-0023 |
| Monty sandbox | `src/tools/python.rs`, ADR-0021 |
| Memory candidates and promotion state | `src/state/`, ADR-0013, ADR-0020 |
| Step model | `impulse-step-model/`, ADR-0015 |
| Tests | 3,225 passed, 0 failed, 10 ignored on the evidence branch above; property and fuzz harnesses over governed parsers (PR #59) |

## Not built

- `impulse run` and `impulse harness` as the two top-level modes.
- The orchestrator role: `delegate_task`, `approve_gate`, and tool advertisement driven by
  `search_tools`.
- Firecracker (or any microVM) isolation for shell commands and verification.
- Token and money budgets.
- ADR-0022 stage 2: Ion on typed endpoints, a provider trait, capability metadata, one endpoint
  policy, local-first routing.
- The memory decision endpoint, cockpit controls, and Ion memory integration (ADR-0020).
- A daemon IPC endpoint for the blackboard, so external harnesses and the cockpit can use it.
- A launched Supervisor runtime proven end to end through the governed path.
- General role contracts and capability negotiation across runtimes.
- Cross-platform proof beyond macOS for the cockpit.

## Non-goals

- Reimplementing proprietary coding harnesses inside Impulse.
- Claiming equal control over every runtime.
- Replacing terminal workflows with a conventional IDE.
- Making the cockpit's component tree the source of truth.
- Keeping every token forever, or promoting every agent statement to memory.
- Giving a supervisor unrestricted builder permissions.
- Building every role, provider, or multi-project feature before the first complete slice works.

## Open decisions

1. Hierarchy and durable ids for project, workspace, role, runtime, instance, session, and pane,
   plus governed-task reassignment and resume.
2. The runtime-adapter contract and capability negotiation, including enforcement strength.
3. The orchestrator role contract: what `delegate_task` and `approve_gate` may do, and how a
   delegated worker's claim enters the four-layer model.
4. Memory promotion authority, semantic validation, correction, and forgetting (ADR-0020 open
   items).
5. Credential grants, revocation, and cross-project prevention.
6. The microVM boundary: which commands and verifications must run inside it.
7. Token and money budgets, and measured control-plane overhead targets.
8. Whether any low-risk verification profile may relax operator-required acceptance.
9. ADR-0016 (governed harness evolution, drafted on `agent/claude-harness-evolution-20260826`,
   never merged): whether the harness may propose changes to itself from execution evidence.

Until these land as ADRs, do not split out `ROLES.md`, `RUNTIMES.md`, or a replacement
architecture schema. This file, the canonical contract, and the ADR set are the sources of truth.
