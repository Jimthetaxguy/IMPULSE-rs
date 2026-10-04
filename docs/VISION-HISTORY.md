---
title: Vision History
description: How Impulse's vision changed from February to October 2026, reconstructed from commits, ADRs, branches, and Cargo history
updated: 2026-10-04
type: doc
category: core
phase: all
status: active
audience: builders
tags: [vision, history, archaeology, adr, roadmap]
---

# Vision History

This is the story of Impulse as the Git history tells it. Every claim below points at a commit,
an ADR, or a branch you can inspect. Where the record is indirect (mostly before the first commit),
the text says so. Commit dates are author dates; a few commits were authored before they were
committed, and that is noted where it matters.

The repository holds about 630 commits reachable from about 120 local and remote branch refs, two root
commits, and twenty-three ADRs (0016 exists only on a branch, 0022 only on the photon lane, and 0023
only on the blackboard lane).

## The short version

| When | What Impulse was | Marker |
|---|---|---|
| before 2026-02-25 | A TypeScript/Bun memory tool, then a Rust rewrite | Indirect: notes inside the first commit |
| 2026-02-25 | "Your AI remembers. Silently." A memory sidecar for coding agents | `1d92ba9`, ADR-0001 to 0005 |
| 2026-02-25 to 03-31 | A meta-agent that monitors, injects, extracts, refines (MIER), with an egui cockpit | `1d92ba9` session notes, `6814eea`, Ralph Plans 2 to 5 |
| 2026-04-15 to 06-25 | A search for the desktop shell: egui, then Tauri 2 with Dioxus, then Dioxus alone | ADR-0007, `3ad15c9`, ADR-0008, ADR-0009 |
| 2026-07-11 | Ion: Impulse's own coding agent | `ae4e87d` to `7cb9e18`, one day |
| 2026-07-12 | "Local control plane and harness manager"; `VISION.md` is written | `1f5253d` |
| 2026-07-13 to 07-16 | Governed tasks: claim, verify, review, operator acceptance | ADR-0010 to 0013 |
| 2026-08 | Settlement kernel, harness-owned step model, research | ADR-0014, ADR-0015, unmerged ADR-0016 |
| 2026-09 | Hardening the governed slice and Ion's tool floor | ADR-0017 to 0021 |
| 2026-10-02 to 10-03 | Photon, typed model endpoints, the PRD | `630d43c`, `99ea337`, ADR-0022, `e1f4657` |

## Before the first commit

The first commit (`1d92ba9`, 2026-02-25) is not the beginning. It arrives with a contract already
at version 1.3 (`docs/spec/RUST-CANONICAL-CONTRACT.md`, dated 2026-02-24), five accepted ADRs, a
`.impulse/GENOME.md` stamped 2026-02-23, and a 764-line `HANDBOOK.md`. Three notes inside it
describe what came before:

- `docs/HONEST-ROADMAP.md` (created 2026-02-21, updated 2026-02-24) calls itself the output of "Session 5 critique
  (21 iterations of adversarial analysis)" and says it "was originally written during the
  TypeScript/Bun era (Session 5, pre-Rust pivot)". It complements a `PRODUCT-SPEC-v2.md` that is
  not in this repository.
- `HANDBOOK.md` lists `impulse/` as "Deprecated, Old TypeScript project" and `harness/` as
  "Pre-pivot reference", and records "TypeScript → Rust pivot: Solved binary distribution".
- ADR-0001 describes Phase 1 hooks as "Four shell scripts (invoking Bun CLIs)", and ADR-0004 and
  the best-practices guide still carry TypeScript examples.

There was also an earlier name. Commit `1b52769` (2026-03-07) adds `.impulse/` to `.gitignore`
"(renamed from .cockpit/ which was already ignored)" and fixes "gitignore gaps from Cockpit→Impulse
rename"; `1ea01b9` (2026-03-27) later removes "Cockpit legacy". The first commit's docs index links
`archive/research/cockpit-feature-plan.md`.

So the prehistory is: a TypeScript/Bun memory tool, critiqued adversarially (the roadmap records a
fifth session of 21 iterations), rewritten in Rust for single-binary distribution, with a working
name of Cockpit. Part of that code survives: `1b52769` (2026-03-07) added the pre-pivot harness under
`archive/harness/` (`@cockpit/harness`, run with `bun src/index.ts`), and it is still on `main`. The
old `impulse/` TypeScript project is not in this repository.

## February: a memory sidecar

The first README opens with **"Your AI remembers. Silently."** and describes "a terminal-native
sidecar for AI coding agents that preserves session continuity across tools and conversations".
The problem it names is forgetting: agents "forget everything between sessions", so you re-explain
the architecture every time.

The first five ADRs all serve that problem:

- **ADR-0001**: Claude Code is the primary integration target; OpenCode is a later thin adapter.
- **ADR-0002**: knowledge lives in three plain-text files in `.impulse/` (`GENOME.md`,
  `HISTORY_INDEX.md`, `LIVE_STATE.json`), with "no database, no embeddings, no vector store in
  Phase 1". The code used `HISTORY.jsonl` from the first commit.
- **ADR-0003 to 0005**: progressive search, decision extraction at session end, distribution.

The code already went further than the ADRs. Day-one `Cargo.toml` pulls in `ratatui`,
`portable-pty`, `vt100`, `rusqlite` (with FTS5 retrieval in `retrieval.db`), `pyo3` behind a
`monty-support` flag, `calamine`, and `datafusion`. The first commit also carried a separate Tauri 2
app crate (`impulse-rs/src-tauri`). A second commit the same day (`dd89278`) deleted it and added the
`impulse-term` and egui `impulse-gui` crates.

The first commit (`1d92ba9`) also carries the first pivot inside the memory product. Its
`SESSION-2026-02-25-META-AGENT.md` describes "transforming Impulse from a terminal multiplexer into
a **meta-agent** that manages the cognitive state of AI coding agents", built on MIER: Monitor,
Inject, Extract, Refine. Memory stopped being a passive file and became something Impulse pushed
into and pulled out of running agents.

Two pieces from that first week still matter. The first `CLAUDE.md` lists principle 6,
**Review Before Apply** ("Never auto-inject without consent"), which later grows into
operator-required acceptance and review-only memory candidates. And the guardrail engine
(`a6ad41b` to `5fac11c`, 2026-02-27), built to evaluate Claude Code PreToolUse hooks, is the same
engine that scans Ion's tool calls before confirmation today.

## March: the cockpit and multi-agent orchestration

March is mostly the egui cockpit and code quality, driven by Ralph loops (Ralph Plan 2 alone is
"40 loops of systematic Impulse enhancement", `438ea49`). Highlights:

- Conflict detection and a notification bus (`d8dc898`, 2026-03-03); a visual signal bus in the GUI.
- `sem`-based semantic diffs and the ATCC v1 agent-friendly CLI contract (`930a573`, `499e946`).
- **OpenSquirrel** (`5640cae`, `6814eea`, 2026-03-17): shared types for agent status, roles,
  delegation, and machine targets, with delegation tracking "inspired by OpenSquirrel's JSON code
  fences and Hermes Agent's restricted child toolsets". Sessions gain `role` and
  `parent_session_id`. This is the first time agents are modeled as a pool with roles rather than
  as sessions that need memory.
- Ralph Plan 3 marks the "agent harness COMPLETE" (`b55f8de`, 2026-03-31), the IPC protocol is
  rewritten to version 2 (`e612548`), and the contract's IPC section first uses the words
  "Supervisor (EGUI control plane)" (`312a6a5`).
- The same day, `b3b57b2` "assess and clean obsolete vision docs" deletes the dashboard-design doc
  from `docs/vision/` and archives the TUI augmentation vision. The other vision docs the first
  commit shipped (real-time injection, intent detection, dynamic CLI, and others) are still on
  `main`.

Early April is cockpit polish: themes, a command palette, a PTY write queue, a README rewritten "for
post-redesign state" (`ee6e2cd`) that still leads with "Your AI remembers. Silently."

## April to June: finding the desktop shell

The product kept its memory pitch while the question underneath changed to "what window do the
agents live in?" The answer changed several times. The very first commit carried a Tauri 2 stub,
removed the same day in favor of egui.

1. **egui** was the cockpit through March.
2. **Tauri 2 + Dioxus + xterm.js** (ADR-0007, `46dae08`, 2026-04-15). The same day `55f1499` marks
   egui as legacy across the contract. `3ad15c9` drops `impulse-gui` from the workspace (authored
   2026-04-17, committed 2026-05-14, with 19 GB of build caches archived). The source stays in the
   tree, outside the workspace.
3. **Dioxus Desktop native host** (ADR-0008; `88985d8` "Tauri -> Dioxus native host migration",
   2026-06-14). ADR-0007 is marked superseded. The `impulse-desktop` crate (first `dioxus`
   dependency on `main`, `d1ed8e0`, 2026-05-30; the second-root branch below had one earlier)
   becomes the cockpit, and the unified Dioxus shell lands as
   PR #9 (2026-06-13).

Two parallel attempts in this period never reached `main`:

- **A second root commit.** `1eb95d7` (2026-04-22, "Initial commit: impulse-rs source code (240 .rs
  files, 4 workspace crates)") starts an unrelated history on `origin/cleanup/loop-103-onward`. On
  it, Loops 115 to 182 build an `impulse-supervisor` Dioxus prototype, split
  `impulse-term-core`/`impulse-term-dioxus`, and add Warp-style command blocks driven by an OSC 133
  parser (`ad3eaca`, `92d28c3`). The supervisor-as-terminal idea survived; the code did not.
- **The `.clean` re-architecture.** `f78cc0c` (2026-06-18, `origin/clean/dioxus-pty-orchestrator`)
  is a contracts-first rewrite in five crates: `impulse-contracts`, `impulse-workspace`,
  `impulse-runtime` (a PTY orchestrator with five backend adapters), `impulse-mcp` (`rmcp` 0.3), and
  `impulse-desktop`. ADR-0009 (2026-06-25) resolved the split by declaring the active tree
  canonical, mapping the clean concepts onto the existing `AgentRegistry` and `WorkspaceRegistry`,
  and archiving the `.clean` checkout. Its context paragraph is the clearest statement of the vision
  at that moment: an "Impulse Agent as always-on tech lead managing/monitoring/augmenting terminal
  CLI-TUI agents", with a workspace picker and type-safe Rust tools.

Meanwhile June hardened the backend: a canonical agent registry in `impulse-ops` (`b403b59`), Codex,
Gemini, and Cursor harness support, bounded timeouts everywhere, and several path-traversal fixes
(`704a60e`, `dc5f6a2`, `3f2b6a9`).

## July 11: Ion

Ion appeared in a single day. On 2026-07-11:

- `ae4e87d` scaffolds `impulse-ion`, pinning a transport-agnostic harness contract
  (`HarnessRequest`/`HarnessResponse`, "spec-a") kept outside the repo under `~/.ai-memory/`.
- `28b847a` and `f5b46ce` add a Rust adapter for a TypeScript Pi agent on MiniMax as a verification
  gate, and the `ion-verify` command.
- `35117f4` splits the main `impulse-rs` crate into lib and bin so a second binary, `ion`, can
  exist.
- `2807bf8`, `bf38b06`, `6166004`, `7cb9e18` build the REPL (T6), tools (T7), chat (T8), and
  tool-calling with a confirmation gate (T9).

The direction was set in a commit message, not an ADR. `6751d96` quotes James: write and bash
capability is "a first-class requirement of Impulse-RS via its respective coding agents, ion-cli
included, same category as claude/codex CLI/TUI tools". Ion was going to be a full coding agent,
not a read-only verify console.

The rest of that day hardened it: environment scrubbing for `bash_exec`, process-group kills,
wall-clock limits on the tool loop, the February guardrail engine wired into confirmation
(`0056a17`), and a comparison with ROSA's approval design (`2a847a4`). Monty, flagged since
February, was researched and explicitly deferred (`d0d6733`).

## July 12 to 16: the control-plane reframe and governed work

The next day the product changed its description of itself. `1f5253d` (2026-07-12, "define agent
control plane boundaries") creates `VISION.md`:

> Impulse is a terminal-native local control plane and harness manager for AI
> software-engineering agents.

The same commit makes the README subtitle "One governed cockpit for many coding agents" (it reached
`main` through `3df9d9f`, PR #13). `ARCHITECTURE-CLARIFICATION.md` is rewritten and now says that
"its memory-sidecar framing is no longer the product contract". In `VISION.md`, memory survives as
"a governed platform service with provenance, not an indiscriminate transcript dump", and the
problem statement moves from forgetting to fragmentation and trust: "terminal sprawl, duplicated
work, context bleed, silent conflicts, and unverified claims".

Four ADRs in three days turned "governed" into code:

- **ADR-0010** (2026-07-13): product role launch contract; Builder launches require a task.
- **ADR-0011** (2026-07-13): the daemon-owned governed task lifecycle.
- **ADR-0012** (2026-07-14): daemon-owned producers for claim, verification, and Supervisor review.
- **ADR-0013** (2026-07-15): accepted runs produce deterministic memory candidates for review.

ADR-0013 is where the February product comes back. Memory is no longer captured silently; it is
proposed from accepted, verified work and waits for an operator. Governed controls reached the
Dioxus cockpit on `main` through PRs #14 and #18 to #20 (2026-07-14 and 15); a fuller cockpit
commit on another lane (`2626698`, 2026-07-16) was never merged. An ElevenLabs voice bridge landed
as PR #22 the same week.

## August: research, the kernel, and the step model

August looked outward and inward at once.

- **Competitive research** (PR #23): comparisons with omnigent, bbarit-agent-oss, and the Unified
  Deep Agent Kit.
- **ADR-0014** (2026-08-10, still in review): work-item identity and comparative settlement, a
  "kernel" with fail-closed basis checks (`909330f`, `6705a66`).
- **ADR-0015** (2026-08-17): the harness, not a gateway or the model, chooses the model for each
  step. Per-provider base-URL overrides (`88e8465`), escalation from config (PR #32), a ROSA import
  addendum (`b2b38cb`), and extraction into the `impulse-step-model` crate (PR #33).
- **ADR-0016, never merged.** `7c21cce` (2026-08-26, `agent/claude-harness-evolution-20260826`)
  drafts a "governed harness evolution plane" from the AutoSaddler paper: typed harness patches
  proposed from execution evidence, evaluated in detached worktrees, and promoted by the operator.
  The number is skipped on `main`. It is the most ambitious idea in the history that has not been
  built.

## September: hardening the governed slice

September turned the July contracts into something an adversary could not easily break. The
early-September governed-slice lanes went through numbered review rounds recorded in the commit log
("review round 1", "round 2", "round 3"): 12 of the month's 31 merged PRs show them, and three more
mention an adversarial review.

- **ADR-0017** (`5286597`): a canonical loop contract with typed budgets and termination evidence.
- **Ion's tool floor** (PR #46): sandbox roots, an untrusted-output envelope, and loop evidence.
  `document_read` (PR #40) adds bounded, pageable reading of office documents.
- **ADR-0018** (2026-09-02): socket actor provenance, so operator-class actions need more than a
  claimed identity.
- **ADR-0019** (2026-09-02, relanded as PR #50 on 2026-09-12): a Builder works in a staged
  worktree, with operator Promote and Discard in the cockpit (PR #58) and protocol v9 (PR #52).
- **ADR-0020** (PR #56): scoped memory promotion and dismissal, the decision half of ADR-0013.
- **Property-based and fuzz harnesses** over "the parsers reviews kept breaking" (PR #59), the
  first `proptest` dependency on `main`.
- **ADR-0021** (2026-09-26): `calculator` and `python_exec` run in the in-process Monty
  interpreter. The `monty-support` flag had sat in `Cargo.toml` since the first commit; seven months
  later Monty arrived as a sandbox rather than as the computed-routing engine the February handbook
  imagined.

On 2026-09-02 the README and VISION were reframed again (`a897dc2`, PR #42). The subtitle became
"Local control plane for terminal-native coding agents", and VISION gained a one-sentence summary:
"the local operating environment for coding agents: it launches and scopes heterogeneous terminal
runtimes, supervises their work, and holds completion to observed evidence and human approval".

## October: Photon, typed endpoints, and the PRD

On the `claude/ion-scout-subagent-20261002` lane, not yet on `main`:

- `630d43c` (2026-10-02) gives Ion a disposable read-only subagent, first called **scout**, built on
  the external `min-agent` crate (pinned by commit). It answers one question about the repository
  and keeps nothing.
- `99ea337` (2026-10-03) renames it **photon** to match "the physics naming of Impulse and Ion. A
  photon is emitted, carries information, has no mass, and is absorbed."
- `30db48f` and **ADR-0022** make model endpoints typed protocols rather than vendors, so photon,
  and later Ion, can run on any of three wire protocols.
- `e1f4657` records James's PRD. It restates the product in the plainest terms yet: "Impulse is the
  whole application, and it is the meta harness inside that application." A session is one task,
  one workspace, one working agent (Ion, Claude, or Codex), and the record Impulse writes for it.

The SQLite blackboard (ADR-0023, `claude/ion-blackboard-20261003`) stacks on that lane. It reverses
part of ADR-0002's "no database" stance, for the same reason ADR-0002 existed: what an agent
produces should outlive the agent's context.

## What was tried and left behind

| Direction | When | What happened | Evidence |
|---|---|---|---|
| TypeScript/Bun implementation | before 2026-02-21 | Rewritten in Rust for binary distribution | `HANDBOOK.md`, `HONEST-ROADMAP.md` in `1d92ba9` |
| The name Cockpit | before 2026-03-07 | Renamed to Impulse; legacy removed 2026-03-27 | `1b52769`, `1ea01b9` |
| Zellij WASM plugin dashboard | planned Phase 3 | Only a stub status-bar plugin was committed (`zellij-plugins/memory-status-bar`); Impulse grew its own TUI and GUI | ADR-0002, `HONEST-ROADMAP.md`, `1d92ba9` |
| Early vision docs | 2026-02-25 | Dashboard design deleted and TUI augmentation archived; the rest remain | `b3b57b2` |
| egui GUI | 2026-02 to 2026-04 | Removed from the workspace; source kept frozen | `3ad15c9`, ADR-0007 |
| Tauri desktop shell | 2026-02-25 (stub, removed the same day); 2026-04-15 (ADR-0007) | Superseded by the Dioxus native host | `1d92ba9`, `dd89278`, ADR-0007, ADR-0008, `88985d8` |
| Second-root supervisor and command blocks | 2026-04-22 to 04-23 | Unmerged branch | `origin/cleanup/loop-103-onward` |
| Contracts-first 5-crate rewrite | 2026-06-18 | Archived; concepts mapped onto the active tree | `origin/clean/dioxus-pty-orchestrator`, ADR-0009 |
| "No database" memory | ADR-0002 | Eroded: `retrieval.db` from day one, now the blackboard | ADR-0002, ADR-0023 |
| Governed harness evolution plane | 2026-08-26 | Drafted, never merged | `agent/claude-harness-evolution-20260826` |

Many other branches that look unmerged are not abandoned. Lanes in this repository are usually
squash-merged or relanded through a PR, and the branch is kept as a backup (`backup/...`,
`archive/...`), so their content is usually on `main`. Not always: `codex/dioxus-egui-retirement`
(`2626698`) is one that never landed.

## The through-line

Read in order, the vision changes its subject three times while keeping its object.

1. **Remember for the agent** (February). Agents forget, so Impulse keeps the memory.
2. **Watch the agents** (February to June). Several agents run at once, so Impulse monitors,
   injects, detects conflicts, and gives them a cockpit.
3. **Govern the agents** (July onward). Agents claim things, so Impulse separates the claim, the
   observed evidence, the supervisor's judgment, and the operator's approval.
4. **Own an agent** (July and October). Impulse runs its own agent, Ion, and Ion sends out a
   reader, Photon, so the harness controls the whole loop for at least one runtime.

The object stays the same: one person working with AI coding agents who should not have to be,
in VISION's words, "the full-time dispatcher, historian, permission clerk, and completion auditor".

Some things have not changed since the first commit:

- **Rust, terminal-native, local.** Fixed by the pre-history rewrite and never revisited.
- **The `.impulse/` directory**, with `GENOME.md` and `LIVE_STATE.json` where ADR-0002 put them,
  and `HISTORY.jsonl` in place of ADR-0002's `HISTORY_INDEX.md`.
- **Review before apply.** Principle 6 in February, operator-required acceptance in July, memory
  candidates that wait for a decision in July and September.
- **The guardrail engine.** Written for Claude Code hooks in February, scanning Ion's tool calls in
  July.
- **Adversarial review as a working method.** The "Session 5 critique" before the first commit,
  Ralph loops in March, and numbered review rounds on most early-September governed-slice lanes.

The biggest shift is in what counts as the product. In February it was memory. Since July it has
been the boundary around the agents: who launched them, what they may touch, what they claim, what
was verified, and who accepted it. Memory is one service inside that boundary rather than the
reason the boundary exists.
