# IMPULSE — Feed the impulse to build.

**One governed cockpit for many coding agents.**

Impulse names both the creative urge to make something and the force that sets work in motion. The product exists to protect that first spark, then help it compound across agents, tools, projects, and sessions instead of dissolving into terminal sprawl and repeated context setup.

Everyone has felt it: the itch to open Claude Code or Codex and disappear into a build. Impulse is built for that moment.

Impulse is a terminal-native **local control plane and harness manager** for AI software-engineering agents. It launches and manages heterogeneous coding runtimes, places them in explicit workspaces, supervises their processes, and augments them with shared memory, tools, telemetry, artifacts, policy, and verification.

Claude Code, Codex, and similar CLIs keep their own internal coding loops. Ion is the Impulse-native coding runtime. Impulse governs the operating conditions around those loops; it does not claim to replace or fully control proprietary runtime internals.

> **Live foundation:** the Rust workspace provides PTY lifecycle, daemon workbench contracts and
> telemetry, registry-backed desktop platform identity, supervisor-specific permissions,
> explicit product-role/task launch preflight, capability-checked tools, memory/retrieval,
> artifacts, credentials, verification, Ion's native REPL/tool loop, and daemon-owned governed
> runtime producers. The profiled path keeps runtime exit, worker claim, verifier evidence,
> Supervisor judgment, and operator approval separate while deriving producer provenance inside
> the daemon. Accepted runs now stage deterministic, read-only memory candidates with source
> assurance and evidence references; they do not write curated project memory. The macOS developer
> package path builds the Dioxus cockpit and `impulse-rs` companion together, verifies signatures
> and local xterm assets, and defines a real eval-bridge/PTY/ordered-shutdown acceptance smoke. A
> fresh receipt from that smoke is required release evidence rather than an assumption made from
> source presence. The companion validates and watches its exact desktop parent so abrupt owner loss
> drains, syncs, and removes daemon runtime files; AppKit Quit plus live-PTY proof remains a separate
> release-hardening gate.
>
> **Target:** runtime-independent role contracts, adapter capability negotiation, typed agent
> messaging, and stronger structural enforcement across supported runtimes. See
> [`VISION.md`](VISION.md) for the north star and explicit live-versus-target boundary.

## Why

Using several coding agents today usually means several unrelated terminals, permission models, context stores, and completion claims. Impulse brings those runtimes into one observable environment while preserving their terminal-native workflows. Persistent memory preserves continuity; the wider control plane handles managed launch, project scoping, coordination, intervention, and evidence-backed completion. Structural filesystem isolation depends on the selected runtime or sandbox rather than on the cockpit alone.

## What It Does

- **Managed agent terminals** — Spawns, monitors, writes to, resizes, focuses, and closes PTY-backed agent processes inside explicit workspace roots
- **Daemon workbench truth** — Serves the authoritative agent, context, artifact, and intervention snapshot over versioned IPC
- **Role and policy foundations** — Preflights an explicit Builder role/task against conservative launch capabilities and enforces a concrete supervisor permission policy; generalized role composition remains a later boundary
- **Governed task truth** — Registers a durable task before PTY launch, optionally binds it to exact acceptance criteria plus a daemon-attested clean Git `HEAD`, applies revisioned/idempotent lifecycle mutations, and reserves `accepted` for an explicit operator decision against current passing evidence
- **Accepted-run review queue** — Derives one versioned, provenance-bearing pending memory candidate from each accepted governed run while keeping `GENOME.md` and `HISTORY.jsonl` unchanged
- **Typed platform tools** — Exposes capability-checked Rust tools through native registries, MCP, and runtime-specific bridges
- **Session tracking** — Records files touched, tools used, and decisions made
- **Persistent memory** — Project genome (decisions/preferences) and session history survive across sessions
- **Context injection** — Relevant past context is surfaced in new sessions via review-first injection
- **Multi-agent observability** — Tracks active agents, tasks, terminal telemetry, delegations, and intervention recommendations
- **Retrieval search** — FTS5 keyword search plus feature-gated, fallback-safe semantic search across session history and genome
- **Context stewardship** — Monitors context window usage and proposes cleanup strategies
- **Artifacts and verification** — Runs the closed `rust_workspace_v1` profile in a detached Git worktree, persists daemon-derived command digests and outcomes without raw output, and keeps worker claims, verifier results, Supervisor verdicts, and operator decisions as distinct records
- **Credential services** — Selects configured credential providers without treating secret values as agent memory
- **External + native runtimes** — Wraps terminal CLIs and provides Ion's direct model/tool loop in the same product

## Quick Start

```bash
cd impulse-rs
cargo build --workspace

# Terminal 1: start the control-plane daemon
cargo run -- daemon

# Terminal 2: launch the feature-gated Dioxus cockpit
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop

# Or build and lifecycle-smoke the signed local macOS application
bash scripts/build-macos-app.sh --smoke
open Impulse.app

# Or stay entirely terminal-native
cargo run -- run

# Launch the Impulse-native coding runtime
cargo run --bin ion
```

The packaged executable is `impulse-rs`. Governed producer commands are daemon-only global-mode
invocations: `impulse-rs --daemon governed-claim`, `impulse-rs --daemon governed-verify`, and
`impulse-rs --daemon governed-review`. A governed pane supplies the exact executable path through
`$IMPULSE_CONTROL_CLI` when its connected gateway can resolve the packaged/control executable;
profiled panes require that routing and fail closed without it. Every governed child receives a
harness-owned `$IMPULSE_HOME` for the exact selected project, and caller overrides cannot replace
it. Ordinary panes have inherited producer-routing variables removed.
The global `--daemon` flag is still required.
Run the injected path as `"$IMPULSE_CONTROL_CLI" --daemon ...` so paths containing spaces remain
one executable argument.

`impulse-rs init` adds only exact daemon/runtime-owned state, socket, retrieval-cache, governed-task
and memory-candidate ledger/temp, and Desktop-outbox paths to the repository `.gitignore`; it never
adds a blanket `.impulse/` rule.
Durable project inputs such as `.impulse/GENOME.md`, `.impulse/config.json`, and
`.impulse/impulse-capabilities.json` therefore remain available to commit unless an existing
operator-owned blanket rule already hides them. Init preserves but warns about that broader rule.

In the Dioxus cockpit, use the governed Builder launch path:

1. Register an existing absolute workspace directory.
2. Choose a runtime from the current platform catalog.
3. Enter the required task and each exact acceptance criterion.
4. Commit every workspace change, including `Cargo.lock` when Cargo would otherwise generate it, so
   the canonical Git worktree root and the later detached verification checkout stay byte-exact.
5. Launch only after the cockpit reports every mandatory requirement satisfied and the daemon
   attests the initial commit OID.

The current cockpit is organized around the product hierarchy rather than memory statistics:
a project boundary, project-scoped workers, the focused assignment and terminal, and
evidence-before-acceptance review. Project setup and worker assignment are progressive rather than
one permanent launch form. Once the daemon or first governed runtime establishes the project, the
window keeps that binding; other project labels remain visible but locked while their workers,
notes, tasks, evidence, and audit rows stay excluded. A mismatched daemon response blocks
project-scoped data and mutations until restart. The review surface exposes only that project's
task evidence and explicitly says
when no model-backed Supervisor runtime has been launched. Runtime-powered Supervisor launch remains
part of the next complete vertical slice. A normal packaged launch no longer promotes the
home-level `~/.impulse` memory
fallback into a daemon/project boundary. Without an explicit standard project socket, oversight
starts disconnected; the first governed launch
atomically attaches or starts the daemon for its selected project before task
registration. The daemon must attest the same canonical project id, repository root, and local
`.impulse` state root before the gateway becomes launchable; symlinked external state and
cross-project task registrations fail closed. Confirmation and payload validation occur before
that activation, and every rejection is audited without mutating project state. Project memory,
review state, telemetry, and governed commands bind together.

Current built-in profiles mediate workspace targeting and process lifecycle. Structural filesystem
scope is optional and currently unsupported, so the initial Builder profile is allowed but visibly
degraded. The canonical working directory selects where the process starts; cwd mediation is not a
filesystem sandbox.

Every governed launch is registered with the daemon before a PTY is created. The child receives
trusted project, task, memory-root, and available socket/control-CLI routing through its launch
environment; the profiled Builder path additionally receives and requires its verification
profile. The daemon requires the canonical Builder assignment and recomputes its
runtime compatibility from the daemon-owned registry before accepting the task; caller-supplied
compatibility cannot strengthen that result. External runtimes can submit intent with
`"$IMPULSE_CONTROL_CLI" --daemon governed-claim --summary "..."`; Ion can call the typed
`governed_submit_claim` tool. The daemon derives the assigned Worker and clean Git subject.

The next terminal commands are `"$IMPULSE_CONTROL_CLI" --daemon governed-verify` and
`"$IMPULSE_CONTROL_CLI" --daemon governed-review`. Verification materializes
the claimed commit in a detached worktree and runs the fixed `rust_workspace_v1` sequence: format,
locked workspace check, locked strict Clippy, and locked workspace tests. The profile requires a
committed, regular, non-symlink root `Cargo.lock`. Passing evidence requires both a clean Git subject and an
unchanged, bounded before/after byte manifest of the detached source tree, including ignored paths;
generated source-tree output therefore cannot hide behind `.gitignore`. The v1 profile rejects
source-tree symlinks so verification cannot follow mutable bytes outside the committed checkout.
This executes host-trusted Rust build scripts and tests; environment scrubbing, timeouts, output
hashing, and process-group cleanup do **not** make it an OS sandbox. Supervisor review is an
API-only, tool-free, history-free, temperature-zero model
turn whose strict response must bind the current task revision, record IDs, subject, and
acceptance-criteria digest. A generic external harness configuration fails closed before spawn.

Runtime exit updates execution state but never accepts the work. The Supervisor view renders
daemon-owned evidence and currently guides the operator to those terminal commands rather than
exposing producer buttons. A model may recommend acceptance, but only the operator can approve it.
That is a transition-policy distinction, not strong actor authentication: another process running
as the same OS user remains inside the current socket trust boundary.
Accepted and rejected operator outcomes are terminal; a later opposite decision cannot rewrite that
result or orphan a staged candidate.
After approval, the daemon persists the accepted task first and then derives one pending candidate
in owner-only `.impulse/MEMORY_CANDIDATES.json`. That file is a deterministic materialized view of
accepted governed evidence, not a second acceptance authority. An identical acceptance replay or
daemon-start reconciliation repairs a missing projection; orphaned or source-mismatched candidates
fail closed. The candidate deliberately excludes worker claim prose and Supervisor/operator
rationales. Its ID hashes the exact JSON bytes from a fixed ordered, versioned source struct (no
maps, floats, or Unicode semantic normalization), and its source assurance distinguishes
daemon-profiled evidence from caller-composed evidence without overstating declared operator identity.

The Dioxus Memory view exposes these candidates as **Pending review — not stored in GENOME**. V1
has no promote, edit, or dismiss action. Candidate staging never mutates `GENOME.md` or
`HISTORY.jsonl`; explicit semantic promotion/dismissal remains a later contract.
Persisted request receipts make replay idempotent and the per-task daemon lock prevents concurrent
duplicate producer execution. This is not crash-safe exactly-once execution: if the daemon exits
after a command/model side effect but before its receipt is durably stored, a retry can run that
producer again. A durable producer reservation journal remains the next reliability boundary.
The next forcing function is one process-level workflow that launches Builder and Supervisor
runtimes through acceptance and observes exactly one staged candidate. Stronger same-user actor
authorization and multi-workspace daemon routing remain open; explicit candidate promotion and
dismissal come later. Session, memory, hook, and verification commands remain available through
`cargo run -- --help`.

The current desktop daemon attachment is single-project: a governed launch must target the project
root bound to the running daemon. Registering additional cockpit workspace entries does not yet
provide cross-project daemon routing. The first governed launch establishes the process-lifetime
boundary. Other registered project labels remain visible but locked; their workers, tasks,
evidence, notes, and audit rows are excluded from the bound cockpit. If a different daemon project
responds, Impulse hides daemon-owned data and mutation controls until the desktop restarts against
the intended project. Switching the boundary requires a restart until multi-project daemon routing
lands.

## Architecture

```text
 Dioxus cockpit / ratatui / CLI
              |
              v
 Impulse daemon + control-plane contracts
              |
    +---------+----------+----------------+
    |                    |                |
 PTY/processes      platform registry  shared services
    |                                  memory, tools,
    |                                  telemetry, policy,
    |                                  artifacts, verification
    +----------+-------------------------+
               |
      +--------+---------+
      |                  |
 external CLI runtimes   Ion native runtime
 Claude, Codex, ...      direct model + tool loop
```

- **Direct mode:** A short-lived hook path reads state, processes one action, persists its result, and exits.
- **Daemon mode:** The long-running coordination point for workbench state, agent requests, telemetry, artifacts, and supervisor actions.
- **Desktop mode:** Dioxus + xterm.js is the cockpit. It renders state and sends commands; backend contracts remain authoritative.
- **Persistence:** Durable records include human-readable JSONL/Markdown/config artifacts, while SQLite indexes and ephemeral daemon/runtime state are intentionally not all human-readable or git-tracked.

## Key Commands

| Command | Purpose |
|---------|---------|
| `init` | Initialize `.impulse/` in current directory |
| `session-start` / `session-end` | Lifecycle management |
| `status` / `summary` / `health` | Project overview |
| `search-history` / `search-genome` | Search past sessions and decisions |
| `steward` | Context window stewardship |
| `orchestrate` / `handoff` | Cross-tool context sharing |
| `run` | Launch the 10-tab ratatui workbench |
| `daemon` | Run the foreground control-plane daemon listener |
| `governed-claim` | Ask the daemon to derive a Builder claim from the current routed task and clean Git subject |
| `governed-verify` | Ask the daemon to run the task's fixed verification profile |
| `governed-review` | Ask the configured API Supervisor to produce a strict, revision-bound verdict |

The three governed commands in this table require the global `--daemon` flag before the subcommand.

See `cargo run -- --help` for the full command list.

## Stack

- **Language:** Rust
- **TUI:** ratatui + crossterm (terminal-native operator surface)
- **Desktop:** Dioxus Desktop + xterm.js via `impulse-desktop`; egui `impulse-gui` is legacy/frozen for compile-maintenance only; Tauri-shaped code is legacy compatibility only
- **Storage:** SQLite (FTS5) + JSONL + Markdown
- **IPC:** Unix domain sockets
- **LLM:** Anthropic, OpenAI, and Minimax provider paths for daemon/Ion agent loops

## Project Structure

```
impulse-rs/          # Rust implementation (canonical)
  impulse-ops/       # Shared control-plane protocol, workbench, policy, artifact models
  impulse-term/      # PTY lifecycle, parser, write queue, terminal context
  impulse-desktop/   # Dioxus cockpit, host bridge, workspace/runtime/MCP adapters
  impulse-ion/       # Ion harness contract + adapter crate
  src/
    main.rs          # CLI entry + command routing
    storage/         # Atomic file operations
    state/           # In-memory state + dirty flag sync
    daemon/          # Unix socket server
    agent/           # External harness coordination
    ion_repl/        # Impulse-native coding-agent runtime
    llm_backends/    # Direct model provider/tool-loop boundary
    retrieval/       # FTS5 + semantic search
    injection/       # Context injection engine
    stewardship/     # Context window management
    token_tracker/   # Token tracking algorithm
    credentials/     # Keychain + socket proxy
    tooling/         # Capability-checked dynamic tools
    tools/           # Tool management + utilities
    docs/            # Documentation fetcher
    ui/              # TUI rendering
docs/                # Documentation
  spec/              # Canonical contract
  research/          # Analysis and research
  guides/            # Developer guides
memory-pipeline/     # Python research tooling
```

## Documentation

- **Product north star:** [`VISION.md`](VISION.md)
- **Start here:** [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
- **User stories:** [`docs/spec/USER-STORY-MAP.md`](docs/spec/USER-STORY-MAP.md)
- **Test traceability:** [`docs/spec/TEST-TRACEABILITY.md`](docs/spec/TEST-TRACEABILITY.md)
- **Governed lifecycle ADR:** [`docs/decisions/0011-governed-task-run-lifecycle.md`](docs/decisions/0011-governed-task-run-lifecycle.md)
- **Governed producer ADR:** [`docs/decisions/0012-daemon-owned-governed-runtime-producers.md`](docs/decisions/0012-daemon-owned-governed-runtime-producers.md)
- **Accepted-run candidate ADR:** [`docs/decisions/0013-deterministic-accepted-run-memory-candidates.md`](docs/decisions/0013-deterministic-accepted-run-memory-candidates.md)
- **Agent guidelines:** [`AGENTS.md`](AGENTS.md)
- **Meta-Harness synthesis:** [`docs/research/META-HARNESS-RUST-MULTI-AGENT.md`](docs/research/META-HARNESS-RUST-MULTI-AGENT.md)
- **Rust multi-agent guide:** [`docs/guides/RUST-MULTI-AGENT-PATTERNS.md`](docs/guides/RUST-MULTI-AGENT-PATTERNS.md)
- **Full index:** [`docs/INDEX.md`](docs/INDEX.md)
- **Detailed historical reference:** [`HANDBOOK.md`](HANDBOOK.md)

## Real systems

Production paths use real local processes, operating-system services, and model providers; there
is no production `MOCK_MODE` switch. Tests use fakes only inside test targets.

| System | Configuration / secret name | Current verification boundary |
| --- | --- | --- |
| Claude Code, Codex, shell, and Ion processes | Executable registry + `PATH`; Ion can resolve its built sibling | PTY/runtime tests plus an ignored real-Ion sibling launch proof |
| Anthropic | `ANTHROPIC_API_KEY` or `CLAUDE_API_KEY` | Real provider path; missing credentials fail explicitly |
| OpenAI | `OPENAI_API_KEY` | Real provider path; missing credentials fail explicitly |
| MiniMax | `MINIMAX_API_KEY` | Real provider/harness path; missing credentials fail explicitly |
| macOS Keychain | Native signed-in-user Keychain; live test additionally requires `IMPULSE_RUN_LIVE_KEYCHAIN_TEST=1` and explicit wire-gate approval | Deterministic native-API tests; ignored live round trip is never part of the default suite |
| Credential socket / CLI proxy | Provider configuration; the CLI proxy may invoke Infisical, Doppler, or Vault while retaining the provider key names above | Provider contract tests |

There is not yet one budget-capped, opt-in command that exercises every remote LLM provider in a
single integration suite. Treat that as a real-systems verification gap, not as permission to add
mock provider behavior to shipping code.

## Tests

```bash
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

This repository is public, but release licensing is not yet coherent: Cargo manifests declare
MIT while [`impulse-rs/LICENSE`](impulse-rs/LICENSE) contains Apache-2.0. Treat licensing as an
explicit pre-release decision; this draft PR does not silently choose or rewrite the license.
