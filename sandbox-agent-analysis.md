# Sandbox Features for Background Agents — Analysis for ROSA and Impulse

> **Date:** 2026-08-22
> **Context:** Mapping a practitioner's list of "sandbox features that matter for good background agents" against ROSA's capability architecture and Impulse's current implementation.
> **Scope:** Background agents — long-running, autonomous, operating without a human watching a terminal. Think Codex background tasks, Claude Managed Agents, or a governed Builder pane that runs for 20 minutes while you're in a meeting.

---

## Orientation: The ROSA Lens

ROSA's 8-contract capability algebra provides a framework for reasoning about what sandboxes actually do for agents. The contracts most relevant here:

- **Execution contract (X):** What computational substrate the agent runs on — process isolation, resource limits, lifecycle management.
- **World scope (W):** What the agent can see and touch — `read_only_snapshot` (observe only), `disposable_scratch` (write freely, nothing persists), `staged_authoritative` (writes staged for review before becoming real), `authoritative` (writes are live).
- **Effect contract (E):** The effect ladder — `observe → derive → propose → mutate → external_action → control_runtime` — bounding what side effects an agent is permitted to produce.
- **Assurance contract (A):** What evidence the system requires before trusting agent output — verification profiles, digest evidence, acceptance criteria.

Impulse's current architecture makes specific choices about each of these. The sandbox features below map onto them.

---

## Features That Matter

### 1. File-System Snapshot

**What it is.** Fork the sandbox's filesystem at a point in time and spin up new instances from that snapshot. E2B implements this with Firecracker microVMs and XFS reflinks — a fork completes in ~400ms on the same host via copy-on-write, meaning only pages/blocks that change after the fork actually consume storage. Codex CLI's workspace-write sandbox mode gives agents a writable copy of the project directory that doesn't affect the host.

**Why it matters for background agents.** A background agent has no human watching to undo mistakes. File-system snapshots give you:

- **Rollback points.** If verification fails after a mutation, revert to the pre-mutation snapshot instead of hoping `git checkout` catches everything (it won't catch untracked temp files, build artifacts, or config state).
- **Parallel exploration.** Fork the snapshot, run two competing approaches, compare results. The agent becomes a tree search over code states, not a linear tape.
- **Reproducible verification.** Run verification against a frozen snapshot so build artifacts from the agent's work can't contaminate the evidence.

**Mapping to ROSA/Impulse.**

This is the *world scope* contract. Impulse's governed verification already runs in a *detached worktree* (`governed_producers.rs`) — a Git-level snapshot of the committed state, with scrubbed env and closed command argv. That's a `read_only_snapshot` world scope for verification. But the *Builder's* working environment during mutation is currently the live project directory — `authoritative` scope with guardrail gates but no filesystem-level isolation.

The gap: Impulse has snapshot-at-verification but not snapshot-at-mutation. A filesystem snapshot at Builder launch would give the Builder `disposable_scratch` or `staged_authoritative` scope — it mutates freely in the snapshot, and only on acceptance does the daemon promote changes to the real worktree. This matches the governed task lifecycle: register → claim → verify → review → accept → promote.

**Priority: HIGH.** This is the single most valuable sandbox feature for Impulse's governed-outcome model. It would make the Builder→Verifier→Supervisor→Accept pipeline structurally safe — a failed verification or declined review means the snapshot is simply discarded, no cleanup needed. Implementation path: Impulse already creates detached worktrees for verification; extend that to create the Builder's worktree at registration time, then have acceptance `git merge` or `rsync` the result into the canonical tree.

---

### 2. Tunnel URLs

**What it is.** Give the sandbox a publicly-routable URL (like ngrok or Cloudflare Tunnel) so external services can reach it. The sandbox runs a dev server; the tunnel URL lets you hit it from a browser, webhook, or API callback.

**Why it matters for background agents.** When a background agent is building a web service, it needs to:

- Test webhook integrations (Stripe, GitHub, Slack) that POST to a callback URL
- Show a preview to a reviewer without them SSH-ing into the sandbox
- Run end-to-end tests against a real HTTP endpoint, not just unit tests

Without tunnel URLs, the agent can only verify its work via unit tests and static analysis — which misses an entire class of integration bugs.

**Mapping to ROSA/Impulse.**

This touches *effect contract* (external_action tier — the agent's work becomes reachable from outside) and *world scope* (the sandbox is no longer hermetic). In ROSA terms, exposing a tunnel URL is a `mutate → external_action` escalation — the agent's process starts accepting inbound connections from the world.

Impulse currently has no concept of network exposure for agent workspaces. The daemon listens on a Unix socket (local only), and governed verification runs with scrubbed env and no network grants. Adding tunnel URLs would need to be an explicit capability in the agent's launch contract — declared at registration, policy-gated, and logged.

**Priority: MEDIUM-LOW for Impulse specifically.** Impulse's current focus is coding agents doing Rust systems work, not web-service development. Tunnel URLs matter more for platforms like Vercel or Replit where the agent's output IS a web service. For Impulse, this becomes relevant when Ion or a governed Builder needs to run integration tests against a live server. Worth designing the capability slot now (so the launch contract has a place for it), but not worth building the infrastructure yet.

---

### 3. Secret Manager with Egress Allowlist

**What it is.** Two things working together:

- **Secret manager:** Inject credentials into the sandbox at runtime without the agent ever seeing the raw values. Omnigent's "secretless credential proxy" is the gold standard — real secrets stay in the parent process; the proxy injects them at the network egress layer. The agent gets a placeholder token that only works through the proxy.
- **Egress allowlist:** The sandbox can only make outbound network requests to explicitly permitted domains. Everything else fails closed. Azure Container Apps, Vercel, and Claude Managed Agents all implement this — default-deny, add specific API endpoints.

**Why it matters for background agents.** A background agent with API keys and unrestricted network is a data exfiltration risk — even without malice, a model might log credentials, send them in an error report, or include them in generated code. The threat model isn't just "malicious agent"; it's "helpful agent that doesn't understand what's secret." Egress allowlists bound the blast radius: even if the agent leaks a key, it can only reach the domains you've approved.

**Mapping to ROSA/Impulse.**

This maps directly to two things Impulse already has, partially:

1. **Env scrubbing** (`tooling/env_scrub.rs`): Impulse already does allowlist-based env clearing — `ENV_ALLOWLIST` of 8 functional vars, `.env_clear()` before spawn, `is_secret_like()` heuristic guard. This is the process-level equivalent of a secret manager. The gap: secrets that the agent *needs* (an API key for a tool it's using) currently have to be in the env allowlist, which means they're visible to the agent process. A proxy-injection model would let the daemon hold the secret and inject it at the network layer.

2. **Guardrail system** (`guardrail/`): The 10 builtin rules gate dangerous *commands*, but there's no network-level egress control. An agent approved to run `curl` can hit any endpoint. ROSA's effect ladder puts `external_action` above `mutate` for exactly this reason — reaching outside the sandbox is a higher-severity effect than modifying local files.

**Priority: HIGH for the egress allowlist concept, MEDIUM for a full proxy.** Impulse's deny-by-default philosophy (Principle #5) already aligns perfectly. The practical step: add a `network_egress_allowlist` field to the governed task registration contract, and enforce it at the process level (iptables/nftables in a network namespace, or an L7 proxy). The secretless proxy is a bigger lift but is the right long-term design — it means the governed-task pipeline never trusts the Builder with raw credentials.

---

### 4. Ability to Run Docker

**What it is.** The sandbox can build and run Docker containers inside itself. Nested containerization. This is harder than it sounds because the sandbox itself is often a container or microVM, so you need Docker-in-Docker, rootless Podman, or Sysbox.

**Why it matters for background agents.** Many real-world build/test workflows depend on Docker: multi-service integration tests, database fixtures, reproducing CI environments. An agent that can't run `docker compose up` can't work on a large class of projects. GitHub's Agentic Workflows (July 2026) added Docker Sandboxes as a first-class runtime for exactly this reason.

**Mapping to ROSA/Impulse.**

This is purely an *execution contract* concern — what the compute substrate supports. In ROSA terms, it's a capability class: `container_runtime` as a declared, policy-gated execution capability. The agent advertises it needs `container_runtime`; the launch contract either grants it (if the host supports it and policy allows) or fails closed.

Impulse's R1 rule (capability classes are a closed universe; new class = governance event) applies directly. Adding Docker support would be a new capability class, deliberately decided, not something an agent can request ad-hoc.

**Priority: LOW for Impulse today.** Impulse's governed verification uses a fixed Rust command profile (`cargo build/test/clippy/fmt`). Docker isn't in the pipeline. When Impulse adds non-Rust verification profiles or Ion needs to run multi-service projects, this becomes relevant. The design hook: the verification profile enum (`GovernedVerificationProfile`) already exists — adding a `containerized_workspace_v1` variant that expects a Dockerfile is the natural extension.

---

### 5. View stdout/stderr in UI Dashboard

**What it is.** Stream the agent's terminal output to a web or desktop dashboard in real time. Not just "did it pass/fail" — the actual build logs, test output, compiler errors, as they happen.

**Why it matters for background agents.** "Background" doesn't mean "invisible." The operator needs to:

- Spot a stuck agent (no output for 5 minutes)
- Catch a wrong-direction early (agent installing dependencies it shouldn't need)
- Debug failures without replaying the entire run
- Build trust — seeing the agent work correctly builds confidence for granting more autonomy

**Mapping to ROSA/Impulse.**

Impulse already has strong infrastructure here. The ratatui TUI shows live PTY output. The Dioxus cockpit bridges PTY streams through xterm.js. The daemon's `AgentPool` endpoint groups sessions by role. Governed verification captures streaming output digests (`CapturedStream` in `governed_producers.rs` — digest, total bytes, truncation flag).

The gap is between "PTY output viewable in the cockpit" and "structured, filterable log stream in a dashboard." The governed producers capture digests but not the full output (they retain only the tail 64KB). For background agents, the operator wants to scan stdout for patterns (errors, warnings, progress markers) without reading every line.

**Priority: MEDIUM.** Impulse has the hard part (PTY capture, daemon state projection). The missing piece is structured log forwarding — either a daemon endpoint that streams filtered PTY output, or governed producers that capture more than digests. The `PublishTerminalOps` push mechanism exists but isn't connected to the governed pipeline. Connecting them is incremental, not architectural.

---

### 6. View Logs from CLI

**What it is.** `impulse logs <task-id>` — tail the background agent's output from a terminal, without opening a dashboard.

**Why it matters for background agents.** Developers live in terminals. A background agent that requires opening a browser to check on is a context switch. `docker logs`, `kubectl logs`, `fly logs` all work this way — the dashboard exists for overview, but quick checks happen in the terminal.

**Mapping to ROSA/Impulse.**

Impulse is a terminal-native tool. The CLI already has `impulse-rs --daemon governed-claim`, `governed-verify`, `governed-review` subcommands. Adding `governed-logs <task-id>` that tails the daemon's captured output for a governed task is a natural CLI extension. The daemon already stores governed task state in `GOVERNED_TASKS.json` with event chains — adding a log-stream endpoint to the IPC protocol fits the existing architecture.

**Priority: MEDIUM-HIGH.** Easy win, high value. Impulse's identity is terminal-native. A background agent you can't `tail -f` from the terminal undercuts that identity. Implementation: add a `StreamGovernedLogs` daemon request type (protocol v7), have governed producers forward captured output through the daemon socket, CLI handler prints it.

---

### 7. Concept of "Image" (Bonus: Dockerfile)

**What it is.** The sandbox environment is defined declaratively — either as a Docker image (Dockerfile), a snapshot (E2B template), or a Nix derivation. The agent runs in a reproducible, versioned environment. "Bonus points for Dockerfile" because that's the most portable format.

**Why it matters for background agents.** Reproducibility. If a background agent succeeds on Tuesday and fails on Thursday, was it the code change or the environment? With a declared image, you can replay the exact same environment. It also enables:

- Project-specific environments (this project needs Python 3.11 + Node 18 + Postgres 15)
- Pre-warmed environments (dependencies already installed, so the agent doesn't spend 10 minutes on `npm install`)
- Environment-as-code review (the Dockerfile is diffable, reviewable, version-controlled)

**Mapping to ROSA/Impulse.**

Impulse's current environment model is "whatever's on the host." The governed verification profile (`rust_workspace_v1`) assumes Rust is installed and `cargo` is in `PATH`. There's no environment declaration — just fixed command argv and env scrubbing.

In ROSA terms, a declared image is part of the *execution contract* — it binds the agent's task to a specific computational substrate, not just a set of permissions. The ROSA concept of binding `{principal, actor, task, world}` to every capability invocation extends naturally: the `world` includes the environment definition, not just the filesystem scope.

Impulse's verification profile enum is the right hook. A `containerized_rust_v1` profile would specify a Dockerfile (or image reference), and the daemon would build/pull the image before spawning the verification worktree inside it. The governed task registration would include an environment hash alongside the Git commit hash.

**Priority: MEDIUM for the concept, LOW for Dockerfile specifically.** Impulse should design the verification profile to *accept* an environment specification now, even if the first implementation is just "host-native Rust." When non-Rust profiles or multi-project support arrives, the container-backed environment becomes necessary. Don't build Docker integration yet; do add the `environment_spec: Option<EnvironmentSpec>` field to the profile contract.

---

### 8. Burstable CPU/Memory

**What it is.** The sandbox can temporarily exceed its baseline resource allocation during peak demand (compilation, test suites) and return to a lower allocation during idle periods (waiting for API responses, thinking). Cloud providers charge for the burst, not the baseline. E2B and Modal both offer this — sandboxes "stay in standby at zero compute cost and resume in under 25ms."

**Why it matters for background agents.** Background agents have bursty workloads by nature: intense during compilation/testing, idle during LLM inference (where the compute is on the API provider's side). Fixed allocation means either paying for peak capacity during idle time, or throttling the agent during builds. Burstable pricing aligns cost with actual work.

**Mapping to ROSA/Impulse.**

This is an *execution contract* concern — resource limits are part of the computational substrate. In ROSA terms, it's a capability descriptor tag: `{execution_tier: "burstable", baseline_cpu: 2, burst_cpu: 8, memory_limit: "16GB"}`.

Impulse runs locally on one machine today, so burstable cloud compute isn't directly relevant. But the concept matters for two future scenarios:

1. **Multiple governed tasks in parallel.** If Impulse manages 3 Builders on the same machine, resource contention is real. Process-level cgroups with burstable limits prevent one runaway build from starving the others.
2. **Remote execution.** If Impulse ever dispatches governed tasks to cloud sandboxes (a "Later" roadmap item), burstable pricing is the right model.

**Priority: LOW now, MEDIUM when parallel governed tasks land.** The immediate practical step: when spawning governed producer subprocesses, consider setting process-group resource limits via cgroups (Linux) or launchd limits (macOS). Impulse already has `ProcessGroupGuard` for orphan cleanup — extending it with resource limits is natural.

---

## Features That Don't Matter (For Background Agents)

### Memory Snapshots

**What it is.** Capture the full process memory state (not just filesystem) and restore it later. Firecracker does this — full VM memory snapshot, resume in 5-30ms with all processes still running.

**Why it doesn't matter for background agents.** Background agents are *task-oriented*, not *session-oriented*. They run to completion (or failure), produce artifacts, and exit. There's no "resume from where I left off in the middle of a thought" use case. The agent's meaningful state is:

- **Filesystem:** captured by filesystem snapshots (feature #1 above)
- **Conversation history:** stored in the LLM's context or the daemon's session log (`HISTORY.jsonl`)
- **Task state:** stored in the governed task record

None of these need process-memory snapshots. Memory snapshots solve a different problem: interactive sessions that you want to pause and resume exactly (notebook environments, long-running REPL sessions). Background agents don't have that interaction model.

**Impulse relevance:** Impulse's daemon already persists everything meaningful to disk — governed task state, session history, memory candidates. Process memory is transient by design. If an agent crashes, the daemon reconstructs state from persisted records, not from a memory dump.

---

### Pause/Resume

**What it is.** Freeze the sandbox (all processes stop), pay nothing while paused, thaw it later and continue exactly where you left off.

**Why it doesn't matter for background agents.** Same reasoning as memory snapshots — background agents run to completion. You don't pause a CI job halfway through. If you want to stop an agent, you cancel it. If you want to save money during idle time, burstable compute (feature #8) is the right tool — the agent stays running but consumes minimal resources while waiting for API responses.

Pause/resume matters for *interactive* agents where a human might walk away for lunch and come back. Background agents don't have that workflow.

**Impulse relevance:** Impulse's governed task lifecycle has explicit states (registered, claimed, verified, reviewed, accepted/rejected). There's no "paused" state because the lifecycle is a forward-moving pipeline with discrete checkpoints. If the operator wants to stop, they reject or cancel — they don't freeze.

---

### Sub-Second Boot Time

**What it is.** The sandbox starts in under a second. Daytona claims 90ms, E2B ~200ms, Firecracker ~150ms.

**Why it doesn't matter for background agents.** Background agents run for minutes to hours. A 5-second boot time is noise in a 20-minute build-test-verify cycle. Sub-second boot matters for:

- **Interactive code execution** (Jupyter cells, REPL eval) where latency is perceptible
- **Serverless functions** where cold start affects request latency
- **Massive fan-out** where you're spinning up 1000 sandboxes and the boot time multiplied matters

Background agents start once and run. Whether boot takes 200ms or 5s is irrelevant to the operator experience or the economics.

**Impulse relevance:** Governed task registration, worktree creation, and Builder launch already take several seconds (Git operations, env setup). The boot time of the execution environment is a rounding error. Optimize for correctness and isolation, not startup speed.

---

## Prioritized Recommendations for Impulse

| Priority | Feature | Impulse Action | ROSA Contract |
|----------|---------|----------------|---------------|
| **P0** | File-system snapshot | Extend detached worktree from verification-only to Builder-launch scope. Builder mutates a snapshot; acceptance promotes to canonical tree. | World scope: `staged_authoritative` |
| **P1** | View logs from CLI | Add `StreamGovernedLogs` to IPC protocol. Governed producers forward captured output through daemon socket. | Assurance: observable evidence |
| **P1** | Secret manager + egress allowlist | Add `network_egress_allowlist` to governed task contract. Design (don't build yet) a secret-proxy injection model. | Effect: `external_action` gating; Execution: env isolation |
| **P2** | stdout/stderr dashboard | Connect `PublishTerminalOps` to governed pipeline. Surface structured log stream in Dioxus cockpit. | Assurance: observable evidence |
| **P2** | Image/environment spec | Add `environment_spec: Option<EnvironmentSpec>` to verification profile. First impl: host-native. Future: container-backed. | Execution: declared substrate |
| **P3** | Burstable CPU/memory | Extend `ProcessGroupGuard` with cgroup resource limits for parallel governed tasks. | Execution: resource bounds |
| **P3** | Tunnel URLs | Add `network_exposure` capability slot to launch contract. Don't build infrastructure yet. | Effect: `external_action` capability |
| **P3** | Docker support | Add `containerized_workspace_v1` variant to verification profile enum as future placeholder. | Execution: capability class |

---

## The Structural Insight

The practitioner's list reveals a clean split: the features that matter for background agents are all about **world scope** (what can the agent touch?) and **effect gating** (what side effects are permitted?). The features that don't matter are about **session lifecycle** (pause, resume, fast boot) — because background agents don't have interactive sessions.

This maps precisely to ROSA's architecture. The execution contract (X) and world scope (W) are the load-bearing contracts for background agents. The effect ladder's upper tiers (`mutate → external_action → control_runtime`) are where the real policy decisions live. And Impulse's governed-outcome model — where the daemon owns task state, evidence, and acceptance — is the right foundation for all of it.

The biggest gap isn't a missing feature; it's that the Builder currently operates in `authoritative` world scope during mutation, with only guardrail gates (regex-based command scanning) as the safety net. Moving the Builder to `staged_authoritative` scope via file-system snapshots would structurally close that gap, making the governed pipeline safe by construction rather than safe by convention.

Omnigent has taken the opposite bet — deep in-flight action governance (28 harness variants, 6-phase policy engine, 12 builtin policy modules) operating in `authoritative` scope. Impulse's bet on governed outcomes plus structural isolation is a fundamentally different (and arguably stronger) safety posture, but it requires the sandbox features this list identifies.

---

## Sources

- [E2B Sandbox Snapshots](https://e2b.dev/docs/sandbox/snapshots)
- [Modal: Best Code Execution Sandboxes for AI Agents 2026](https://modal.com/resources/best-code-execution-sandboxes-ai-agents)
- [Firecrawl: AI Agent Sandbox 2026](https://www.firecrawl.dev/blog/ai-agent-sandbox)
- [Northflank: E2B vs Modal](https://northflank.com/blog/e2b-vs-modal)
- [amux: AI Agent Sandboxing in 2026](https://amux.io/guides/ai-agent-sandboxing/)
- [Docker: Running AI Agents in GitHub Actions](https://www.docker.com/blog/running-ai-agents-in-github-actions-with-docker-sandboxes/)
- [AgentPatterns: Agent Network Egress Policy](https://agentpatterns.ai/security/agent-network-egress-policy/)
- [Pluto Security: Inside Claude Managed Agents](https://pluto.security/blog/inside-claude-managed-agents/)
- [Azure: Agent Governance Toolkit](https://techcommunity.microsoft.com/blog/linuxandopensourceblog/govern-ai-agents-using-agent-governance-toolkit-and-azure-container-app-sandboxe/4526011)
- [Blaxel: Sub-second Sandbox Startup 2026](https://blaxel.ai/blog/sub-second-sandbox-startup-2026)
- [Modal: Best Stateful Sandboxes for Long-Running Agents](https://modal.com/resources/best-stateful-sandboxes-long-running-agent-sessions)
- [Codex: Portable Sandbox Manifests](https://codex.danielvaughan.com/2026/04/17/agents-sdk-harness-portable-sandbox-manifests-codex-cli/)
- [Sandlock: Confining AI Agent Code with Unprivileged Linux Primitives](https://arxiv.org/pdf/2605.26298)
- [Impulse: Omnigent Comparison](docs/research/2026-08-09-omnigent-comparison.md) (internal)
- [Impulse: Stack Consolidation Contract](docs/research/2026-06-30-stack-consolidation-contract-and-next-steps.md) (internal)
