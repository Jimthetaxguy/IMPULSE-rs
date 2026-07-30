---
title: Impulse vs. Omnigent — Comparative Analysis
description: Point-in-time comparison of Impulse against omnigent-ai/omnigent, the open-source Python meta-harness, across architecture, harness integration, governance, sandboxing, and memory
version: '1.0'
updated: 2026-07-30
type: research
category: competitive-analysis
phase: all
status: complete
audience: builders
tags: [research, meta-harness, competitive, omnigent, harness, governance, sandboxing, memory]
---

# Impulse vs. Omnigent — Comparative Analysis

> **Point-in-time record (2026-07-30).** Compares the Impulse repository at commit `2504557`
> against [omnigent-ai/omnigent](https://github.com/omnigent-ai/omnigent) at `main`
> (v0.8.0.dev0, one commit after the v0.7.0 release of 2026-07-27). Omnigent facts come from a
> fresh source read of that checkout, not from marketing material. This supersedes nothing;
> `docs/spec/COMPETITIVE-POSITIONING.md` remains the (superseded) memory-plugin-era landscape doc
> and did not cover omnigent.

---

## 1. What each project is

Both projects answer the same question — *how do you run, govern, and coordinate heterogeneous
coding-agent harnesses?* — from nearly opposite ends of almost every axis.

**Impulse** is a terminal-native **local control plane and harness manager**: a Rust daemon plus
CLI/TUI/Dioxus-desktop surfaces on one machine, for one user, wrapping external CLIs (Claude Code,
Codex) over PTYs and providing an Impulse-native runtime (Ion). Its center of gravity is
**governed outcomes**: daemon-owned governed tasks, detached verification with digest evidence,
strict supervisor review, operator-required acceptance, and provenance-gated memory.

**Omnigent** is an open-source (Apache-2.0) **meta-harness with a client–server architecture**: a
Python/FastAPI server, per-machine runner processes, and harness subprocesses, fronted by web,
Electron, native iOS/Android, terminal REPL, VS Code, and Slack clients. Its center of gravity is
**breadth of integration**: ~28 registered harness variants across 11+ vendors normalized behind
one event stream, one policy engine, and one session store, plus multi-user collaboration and
cloud-sandbox execution.

## 2. Scale and maturity

| Dimension | Impulse | Omnigent |
|---|---|---|
| Language | Rust (workspace of 5 crates) | Python 3.12+ core, TypeScript/React web |
| Core size | ~131K LOC Rust (incl. in-file tests) | ~362K LOC Python core; ~186K LOC web TS; ~11K Electron; native Swift/Kotlin apps |
| Tests | ~1,950 `#[test]`/`#[tokio::test]` fns | ~15,246 `def test_*` (~561K LOC under `tests/`) + 269 vitest files + Playwright visual-regression suite |
| Persistence | `.impulse/` files (JSONL/JSON/MD) + SQLite retrieval index | SQLAlchemy over Postgres/SQLite (+ Cloudflare D1, Databricks Lakebase), 14 tables, 88 Alembic migrations |
| Release state | Unreleased; repo history restarted 2026-07-11 (50 commits) | v0.7.0 on PyPI (2026-07-27); PR numbers in the thousands; Homebrew tap, one-line installer, desktop download |
| Team shape | Single-operator, agent-assisted | Community project with many contributors and CI at scale (79 workflow files) |
| Deployment | One machine, Unix socket | 17 deploy targets (Docker, Render, Railway, Fly, Cloudflare, Modal, Databricks Apps, K8s, …) |

Omnigent is roughly 5–6× larger and is a shipping product with real users. That asymmetry frames
the whole comparison: Impulse cannot and should not compete on breadth; the useful question is
where the architectures genuinely diverge and what each has that the other lacks.

## 3. Architecture

**Convergences.** Both projects independently landed the same load-bearing decisions:

- A long-running backend is the source of truth; UIs project state and are never authoritative
  (Impulse: "pane is never a policy boundary"; omnigent: server DB rows, clients are SSE/WS
  projections).
- Append-only event logs as the session record (Impulse: `HISTORY.jsonl`, governed-task event
  chains; omnigent: `conversation_items` ordered by a position counter).
- Deny-by-default environment allowlists for spawned processes (Impulse: `tooling::env_scrub`;
  omnigent: `inner/agent_env.py` — their commit notes even record the same discovery, that several
  harness spawns inherited the full host env before the fix).
- The same bug classes fixed in the same season: secrets in argv/env, unbounded socket reads,
  orphaned child processes, un-timed subprocess awaits.

**Divergences.**

- **Process topology.** Impulse: one daemon, direct PTY children, one Unix socket protocol
  (`PROTOCOL_VERSION = 6`). Omnigent: a three-tier split — *server* (never runs agent code, never
  holds LLM keys) ⇄ *runner* (user-side, dials out over a WebSocket tunnel) ⇄ *harness
  subprocess* (itself a FastAPI app over a Unix socket, one per conversation). The split is what
  buys phone→laptop execution and multi-replica servers; it also costs three processes and a
  database before the first token streams. Impulse's single-daemon model is the "minimize
  control-plane overhead" bet stated in VISION.md.
- **State.** Impulse deliberately keeps human-readable files (`GENOME.md` you can `cat`);
  omnigent is DB-first with binary UUID keys, zero foreign keys, compressed opaque columns — built
  for multi-tenant sync, hostile to `cat`.
- **Realtime.** Omnigent runs SSE per-session live-tails (no replay; snapshot + dedupe) plus a
  WebSocket watch-set protocol for session lists, with documented HTTP/2 requirements. Impulse's
  equivalent is daemon polling/IPC round trips; nothing in Impulse today needs cross-device
  convergence.

## 4. Harness integration — omnigent's center of gravity, Impulse's open ADR

This is the starkest gap, and the most instructive one.

Omnigent's registry (`harness_plugins.py`) covers ~28 harness ids across two tracks:

- **SDK/subprocess track** — the harness process drives the vendor SDK/CLI headlessly and bridges
  omnigent tools into the vendor's tool-calling interface (claude-sdk, codex, pi, openai-agents,
  cursor, copilot, and more).
- **Native TUI track** — the real vendor TUI runs in a runner-owned tmux pane; omnigent injects
  messages via `tmux send-keys` and a per-vendor *forwarder* mirrors the transcript back through a
  documented `external_*` event envelope. Eleven vendors, ~1.9 MB of Python of per-vendor bridge
  plumbing; approval mirroring ranges from structured JSON-RPC (codex) to literally scraping
  `capture-pane` output and answering with arrow-key keystrokes (goose).

Two design moves stand out as directly relevant to Impulse's open ADRs #2/#3 (runtime-adapter
contract, capability negotiation):

1. **A declarative, typed capability matrix in code.** `HarnessCapabilities` is a frozen dataclass
   with enum axes — `IntegrationMode` (sdk-in-process / cli-subprocess / acp-subprocess /
   native-tui / native-server), `Elicitation` (none / hook / jsonrpc / approval-mirror /
   sse-permission), `Resume` (none / warm-reattach / cold-only), auth model, plus tri-state
   booleans where `None` means "no claim." It is served over the API (`GET /v1/harnesses`) so UIs
   can render honest per-harness support.
2. **The matrix is executable.** `tests/harness_bench` live-probes each harness per capability
   dimension (13 probes: streaming, tool calling, interrupt, policy allow/ask/deny, fork replay,
   cost tracking, …), derives verdicts (`SUPPORTED/PARTIAL/UNSUPPORTED/NOT_APPLICABLE/UNKNOWN/
   SKIPPED`), and reconciles them against the declared flags — a declared ✓ that behaves ✗ becomes
   a `DRIFT` cell and a non-zero exit. Comments in the registry are candid about which cells are
   live-verified vs. "declared best-effort by integration mode."

This is a working implementation of exactly what VISION.md demands — *"a future
generalized/dynamic adapter contract must report rather than hide those differences"* — and of the
enforcement-strength honesty Impulse insists on. Impulse today has the static ADR-0010 launch
preflight (trusted code-owned capability table, repeated backend-side before PTY creation) and a
narrow harness abstraction; the adapter trait is still future work. Omnigent's elicitation
taxonomy and drift-bench design are the two artifacts most worth studying when those ADRs land.

Worth noting the honesty cuts both ways: omnigent's own docs admit the bench has no probes for
steering, live-queue, resume, images, or compaction, and that most non-P0 harnesses carry
best-effort declarations.

## 5. Governance — two different questions

Both projects lead with "govern your agents," but they govern different things.

**Omnigent governs actions in flight.** Its policy engine is a first-class, three-persona system:

- Verdicts ALLOW/DENY/ASK across six phases (`request`, `tool_call`, `tool_result`, `response`,
  `llm_request`, `llm_response`), with an explicit fail-closed phase set (`tool_call`, `request`)
  and a documented rationale for why `tool_result` fails open.
- Three stacking scopes evaluated session → agent-spec → server-admin, so a user's DENY
  short-circuits and the admin gets the last word on ALLOW/ASK. Sub-agents inherit the root
  session's policies; cost-approval state is stored on the root so one approval covers the spawn
  tree.
- Trust properties enforced by construction: `sys_add_policy` (the tool by which an agent installs
  policies) unconditionally ASKs — an agent cannot silently self-govern; approvals resolve at a
  dedicated resource-scoped URL requiring edit permission, never as a generic in-band event; a
  declined ASK applies no side effects.
- Twelve builtin modules: cost budgets (per-session, per-user-daily, per-subagent — the hard cap
  is a *downgrade gate* that denies only while on an expensive model), blast-radius and
  spawn-bound policies, GitHub/Google-service-aware policies that parse both MCP calls and shell
  `git`/`gh` text, a CEL evaluator for user-authored session rules, LLM-classifier policies, and a
  risk-score accumulator.

**Impulse governs outcomes and acceptance.** Omnigent has nothing comparable to the governed-task
pipeline: daemon-owned durable revisioned task records with idempotency receipts; registration
gated on a clean committed `HEAD` independently re-observed by the daemon; claims that carry only
bounded summaries while the daemon derives truth; verification in a detached worktree with a
closed argv profile, scrubbed env, streaming output digests, and byte manifests; supervisor review
as a stateless, tool-free, temperature-zero API turn whose envelope must echo the task revision,
claim/verification IDs, and acceptance-criteria digest — with generic harness review failing
closed; and operator-only final acceptance. Impulse keeps worker claim, observed evidence,
supervisor judgment, and user approval as four separately-stored, separately-disagreeable things.

In omnigent, "done" is ultimately the agent saying so plus a human reading the PR. Its flagship
orchestrator example (Polly) encodes review discipline — a reviewer from a different vendor than
the implementer, given only the diff and acceptance contract, never the worktree or transcript;
reviewers never edit; only the implementer opens the PR — but almost entirely as **prompt and
skill conventions** with a few supporting policies (`blast_radius`, `spawn_bounds`,
`headless_subagent_purpose_guard`). It is philosophically Impulse's supervisor model, enforced at
the weakest layer Impulse's own principles reject ("prompt text is not enough").

The mirror image: Impulse's in-flight guardrails (the `guardrail` module's nine builtin bash
rules, the ion REPL confirmation gate with `CONFIRM` escalation) are far thinner than omnigent's
policy engine — no spend caps, no per-phase stacking, no user-authorable rules, and the known gap
that `GuardTarget::FileWrite` has zero builtin rules.

## 6. Sandboxing and secrets

Omnigent has taken on a threat model Impulse explicitly has not: **running untrusted agent code
with OS-level containment.**

- **Linux:** bubblewrap hermetic mount namespaces (no `$HOME`, cwd read-only by default, dotfiles
  masked by binding `/dev/null`) plus a seccomp layer with argument filters — any `CLONE_NEW*`
  flag → `EPERM`, `clone3` → `ENOSYS` (documented glibc-fallback reasoning), an `AF_UNIX/INET/
  INET6`-only socket allowlist.
- **macOS:** generated seatbelt profiles (default-deny, `$HOME/Library` denied even under broad
  read grants), with documented deltas from bwrap (masking is access-deny not invisibility; no
  PID-namespace equivalent).
- **Windows:** Job Objects only, with a blunt docstring and a runtime warning that filesystem and
  network fields are advisory and unenforced.
- **Network:** an L7 TLS-MITM egress proxy with default-deny method/host/path rules, a hardened
  host grammar that forecloses NUL-truncation/CRLF/percent-encoding authority smuggling
  (explicitly credited to an Anthropic sandbox-runtime fix), and a **secretless credential proxy**
  — real secrets stay in the parent; the proxy injects them at egress, or mints placeholder tokens
  that 403 if replayed against a different host.

Impulse's stance is documented candor rather than containment: verification runs host-trusted
code, "containment measures, not an OS sandbox"; another same-user process is inside the trust
boundary; structural filesystem enforcement depends on the selected runtime. Its recent hardening
work (env-scrub allowlists, keychain-stdin fix, bounded daemon reads, harness timeouts, guardrail-
scanned confirmation) is careful process hygiene within that model.

Two honest footnotes on omnigent's side temper the gap: its native TUI panes for several
harnesses — including Polly's entire coding roster — run with `sandbox: type: none` by design, and
the recursive dotfile-masking scan defaults *off* (a nested `services/api/.env` stays readable
unless opted in). The deep sandbox exists, but the flagship multi-vendor workflow runs on policy
and convention, not isolation.

## 7. Memory — Impulse's clearest differentiation

- **Omnigent:** in-session context management is a three-layer compaction ladder (surgical
  tool-result clearing → LLM summarization → truncation). Durable memory is **outsourced** to
  Hindsight, an external memory service, surfaced as three tools (`hindsight_retain` / `_recall` /
  `_reflect`) with per-agent bank isolation. There is no provenance model, no review gate, no
  concept of what is *allowed* to become memory.
- **Impulse:** memory is a governed platform service — committed `GENOME.md`/`HISTORY.jsonl`,
  FTS5/semantic retrieval, review-first injection (Principle #6), and the accepted-run
  memory-candidate pipeline: only operator-accepted, verification-backed outcomes stage
  deterministic candidates in a separate owner-only ledger, structurally excluding worker prose,
  with promotion to curated memory still a future explicit operator action.

Nothing in omnigent distinguishes a verified fact from agent prose before it is retained. This is
the axis where Impulse's design is most clearly ahead in kind, not just in emphasis.

## 8. Multi-agent orchestration

Omnigent's sub-agents are real sessions (`kind="sub_agent"`, parent/root conversation ids, visible
in a session-tree UI), spawnable declaratively (agent-as-tool YAML, each with its **own harness
and model** — the cross-vendor routing primitive) or at runtime (`sys_session_send` addressed by
agent+title, inbox-wake delivery with retry/ack semantics). Polly demonstrates the full pattern:
per-task git worktrees, ≤6 dispatches per turn enforced by policy, model routing via a live
per-worker model catalog, cross-vendor review, and honest per-harness cancellation semantics.

Impulse today has the narrower coordinator/worker `AgentRole`, delegation IPC endpoints, and the
supervisor permission policy; typed cross-agent messaging is open ADR #5, and the composed
Builder+Supervisor process proof is the stated Next. Impulse's bet is that the generalized role
contract (behavioral obligations + enforcement strength, independent of runtime) is worth
specifying before scaling out; omnigent shipped the scale-out first and is retrofitting governance
as policies.

## 9. What Impulse should take from omnigent

Apache-2.0 licensing makes omnigent a legitimate design quarry. Ranked by fit with the current
roadmap:

1. **The executable capability matrix + drift bench** (ADRs #2/#3). The pattern — typed
   declarations in code, served to UIs, reconciled against live probes with `DRIFT` failing CI —
   is exactly "report rather than hide," and its `Elicitation`/`Resume`/`IntegrationMode` enums
   are a ready-made vocabulary for Impulse's enforcement-strength reporting. A Rust equivalent
   could start as an `impulse-rs` test binary probing the two harnesses Impulse already wraps plus
   Ion.
2. **Policy trust primitives.** The unconditional ASK on policy self-modification, the explicit
   fail-closed phase set, and resource-scoped approval endpoints (approval as a permissioned
   resource, never a generic event) are small, high-leverage invariants that fit directly into the
   guardrail/confirmation work already in flight (the ROSA reverse-transfer moved the same
   direction with `ApprovalGrant`).
3. **Cost governance.** Impulse has a `token_tracker` but no budget policy. Omnigent's
   `cost_budget` design — soft ask-thresholds, hard cap as a *model-downgrade gate* rather than a
   stop, root-session approval covering the spawn tree — is a complete, thought-through shape.
4. **Delivery-ambiguity handling.** Omnigent's forwarders own idempotency via a shared
   "may-have-been-delivered" classifier plus a dead-letter JSONL — directly relevant to Impulse's
   documented daemon-crash-between-side-effect-and-receipt gap and the planned durable producer
   reservations.
5. **Egress + credential proxy patterns** — if/when Impulse takes on the untrusted-code threat
   model. The host-grammar hardening notes alone are a checklist of authority-parsing CVE classes.

## 10. What Impulse has that omnigent lacks

1. **Structural acceptance.** The entire governed-task pipeline — criteria-bound registration,
   daemon-attested subjects, detached digest-evidenced verification, revision-bound tool-free
   supervisor review, operator-only acceptance. Omnigent's equivalent is convention.
2. **Governed memory with provenance.** Review-first injection and verification-gated candidates
   vs. an external retain/recall service with no gate.
3. **Local minimalism.** One Rust binary + daemon + files-you-can-read vs. server + database +
   runner + harness subprocess + tmux + web stack. For the single-operator terminal-native user,
   Impulse's overhead floor is structurally lower.
4. **A four-way claim/evidence/judgment/approval separation** carried through storage and UI,
   not just prose.

## 11. Strategic read

Omnigent occupies the breadth quadrant with community velocity Impulse cannot match: every vendor
CLI, every device, every cloud, multi-user. Competing there is a losing race and VISION.md's
non-goals already say so. The defensible ground is exactly where omnigent is structurally weak —
**evidence-based acceptance, governed memory, and a minimal local control plane** — and those are
weaknesses of architecture-plus-priority, not oversight: retrofitting daemon-attested acceptance
into a convention-driven multi-tenant Python system is much harder than adding a policy module.

Two cautions. First, omnigent's policy engine plus Polly-style conventions can approximate "good
enough" verification for many users long before Impulse's full vertical slice is proven — the
moat is real only once the composed Builder+Supervisor proof lands. Second, omnigent demonstrates
that the honesty culture Impulse treats as a differentiator (enforcement-strength candor,
documented gaps) is reproducible at scale — their capability bench *executes* their honesty.
Impulse's edge must therefore rest on the structural guarantees themselves, not on being the only
project willing to state its limits.
