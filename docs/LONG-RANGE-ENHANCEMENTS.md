---
title: Long-Range Enhancement Roadmap
description: PR-organized enhancement backlog across 8 themed lanes
version: '1.0'
updated: 2026-03-17
type: doc
category: roadmap
phase: all
status: active
audience: builder
tags: [roadmap, enhancements, planning, backlog]
last_updated: 2026-03-17
authors:
  - name: James Pustorino
    role: Creator
---

# Long-Range Enhancement Roadmap — Impulse

> **Updated:** 2026-03-17
> **Purpose:** Organize the full enhancement backlog into themed lanes with PR-sized work packages.
> **Roadmap anchor:** [`ROADMAP-PLAN.md`](./ROADMAP-PLAN.md)
> **Risk register:** [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md)
> **Contract:** [`spec/RUST-CANONICAL-CONTRACT.md`](./spec/RUST-CANONICAL-CONTRACT.md) wins all conflicts.

---

## Governance

This document is the **third pillar** of Impulse planning:

| Document | Role | Scope |
|----------|------|-------|
| [`ROADMAP-PLAN.md`](./ROADMAP-PLAN.md) | Execution sequence | What we're doing now and next |
| [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md) | Risk register | What's unproven and risky |
| **This document** | PR backlog | The full queue organized by theme |

**Rules:**
- Items in **Now** lanes are ready for immediate work.
- Items in **Next** lanes are ready once their dependencies land.
- Items in **Later** lanes are ready only after Now+Next complete and validation evidence exists.
- If HONEST-ROADMAP.md validation fails, dependent items here must be revised.
- RUST-CANONICAL-CONTRACT.md wins any conflicts.

**Pattern provenance:** Many enhancements borrow patterns from external analysis. Sources are cited inline: `[desloppify]`, `[Hermes]`, `[OpenSquirrel]`, `[Agent Harness Analysis]`.

---

## Summary

| Lane | Theme | PRs | Stage | Goal |
|------|-------|-----|-------|------|
| 1 | Validation & Evidence | 4 | Now | Unblock product claims |
| 2 | Daemon-Truth Completion | 4 | Next | Complete authority transfer |
| 3 | Memory Quality | 5 | Next/Later | Fix documented limitations |
| 4 | Agent Orchestration | 5 | Later | Multi-agent safety |
| 5 | Retrieval Evolution | 4 | Later | Progressive search |
| 6 | External Integration | 3 | Later | Platform adapters |
| 7 | Operational Polish | 5 | Later | UX and ergonomics |
| 8 | Distribution & Adoption | 3 | Later | <15s install |
| **Total** | | **33** | | |

---

## Lane 1: Validation & Evidence

**Stage:** Now
**Goal:** Resolve unvalidated assumptions from HONEST-ROADMAP.md. Must complete before Later-stage work begins.

### PR 1.1 — SessionStart stdout injection validation harness

| Field | Value |
|-------|-------|
| **Size** | S |
| **Depends** | — |
| **Key files** | `impulse-rs/tests/hook_validation/` (new), `src/hooks/mod.rs` |
| **Success** | Pass/fail evidence recorded in HONEST-ROADMAP.md. Verified: Claude Code treats SessionStart hook stdout as usable system context. |

Build a test harness that registers a SessionStart hook emitting a known marker string, then verifies Claude Code surfaces it as system context in the next session. This is the single most important validation — if it fails, the injection mechanism must be redesigned.

### PR 1.2 — PreCompact survival validation harness

| Field | Value |
|-------|-------|
| **Size** | S |
| **Depends** | — |
| **Key files** | Test script, `docs/HONEST-ROADMAP.md` update |
| **Success** | Pass/fail evidence recorded. Verified: PreCompact hook output survives compaction and appears in post-compaction context. |

Write a 10-line bash PreCompact hook that outputs `MUST SURVIVE: TEST CONTENT`. Trigger compaction. Verify the content appears post-compaction.

### PR 1.3 — GENOME.md usefulness A/B evaluation framework

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 1.1 |
| **Key files** | `docs/guides/HOOK-VALIDATION-GUIDE.md`, evaluation script |
| **Success** | Subjective assessment over 1 week: does Claude Code reference GENOME.md content? Does it prevent context re-discovery? Results documented. |

Manually curate a 20-line GENOME.md for a real project. Use it daily for 1 week with the validated SessionStart injection. Assess impact.

### PR 1.4 — Extraction quality benchmark on real transcripts

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — |
| **Key files** | Benchmark script, extraction prompt tuning |
| **Success** | Precision/recall measured on 3-5 real Claude Code JSONL transcripts. Sampling strategy tuned if needed. Results documented. |

Apply the extraction prompt to real session transcripts. Measure what it captures vs misses. Identify whether beginning+end sampling is sufficient or if mid-session content matters.

---

## Lane 2: Desktop Daemon-Truth Completion

**Stage:** Next
**Goal:** Make the daemon the authoritative source of desktop shell state. Completes the execution sequence from [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](./plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md).

### PR 2.1 — Terminal telemetry publication via PublishTerminalOps

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | — |
| **Key files** | `impulse-desktop/src/`, `impulse-ops/src/lib.rs`, `impulse-rs/src/daemon/mod.rs` |
| **Success** | Desktop terminal surfaces emit `TerminalOpsReport` on tab spawn, shutdown, tier change, compaction, injection, intervention change, and 2-second heartbeat. |

Implement the `PublishTerminalOps { report: TerminalOpsReport }` daemon IPC request. Terminal panes publish telemetry to the daemon rather than maintaining local-only state.

### PR 2.2 — Daemon telemetry overlay with stale/purge rules

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 2.1 |
| **Key files** | `src/daemon/mod.rs`, `src/daemon/telemetry_store.rs` (new) |
| **Success** | Daemon merges telemetry onto durable snapshot by `session_id` then `agent id`. Unmatched telemetry exposed as ephemeral agents. Stale after 10s, purged after 60s. Covered by tests. |

Implement the overlay rules from ROADMAP-PLAN.md: build durable snapshot first, overlay fresh telemetry, mark stale, purge old.

### PR 2.3 — Remove desktop shadow merges for workbench surfaces

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 2.2 |
| **Key files** | `impulse-desktop/src/` side-panel components |
| **Success** | Overview, Agents, Context, Artifacts render exclusively from daemon snapshot. No local-only state for these surfaces. |

Remove local shadow merge logic from desktop views. All workbench surfaces read from `ProjectOpsSnapshot` via daemon IPC.

### PR 2.4 — Artifact action round-trip through daemon snapshot

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 2.3 |
| **Key files** | `impulse-desktop/src/`, daemon artifact handlers |
| **Success** | Apply/acknowledge artifact → visible state change arrives only via daemon snapshot refresh, not frontend-local mutation. Manual acceptance verified. |

---

## Lane 3: Memory Quality

**Stage:** Next (3.1, 3.2, 3.5) / Later (3.3, 3.4)
**Goal:** Address the 5 documented memory limitations from HONEST-ROADMAP.md.

### PR 3.1 — PROJECT.md / PERSONAL.md privacy split

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 1.3 (need extraction quality baseline) |
| **Key files** | Extraction logic, `impulse init` command, `.gitignore` template |
| **Success** | Extraction prompt classifies items as team vs personal. `.impulse/PROJECT.md` committed; `.impulse/PERSONAL.md` gitignored. Personal preferences never appear in git history. |
| **Source** | HONEST-ROADMAP Correction 5 |

### PR 3.2 — .gitattributes union merge strategy for append-only files

| Field | Value |
|-------|-------|
| **Size** | S |
| **Depends** | PR 3.1 |
| **Key files** | `impulse init` command handler, `.gitattributes` template |
| **Success** | `impulse init` generates `.gitattributes` with `merge=union` for `PROJECT.md` and `HISTORY.jsonl`. Two developers using Impulse on the same repo can merge without conflicts on append-only files. |
| **Source** | HONEST-ROADMAP Correction 6 |

### PR 3.3 — Semantic deduplication for GENOME.md entries

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 5.1, PR 5.2 (sqlite-vec + embeddings required) |
| **Key files** | Extraction pipeline, retrieval module |
| **Success** | Replace 40-char substring dedup with embedding-based similarity (threshold 0.85). False positive rate < 5% on test corpus. |
| **Source** | HONEST-ROADMAP limitation: "40-char dedup is brittle" |

### PR 3.4 — Contradiction detection and resolution

| Field | Value |
|-------|-------|
| **Size** | XL |
| **Depends** | PR 3.3 |
| **Key files** | `src/memory/contradiction.rs` (new), extraction pipeline |
| **Success** | Detect conflicting decisions (e.g., "use PostgreSQL" vs "use SQLite"). Surface contradictions for human review. Mark stale entries. Contradiction count in `impulse health` output. |
| **Source** | HONEST-ROADMAP limitation: "Append-only. Stale decisions persist." |

### PR 3.5 — State re-validation on load (safety net pattern)

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — (can land independently) |
| **Key files** | `src/state/session.rs`, `src/state/live_state.rs` |
| **Success** | On session resume, LIVE_STATE.json is validated against actual `.impulse/` contents. Stale entries removed. Re-resolved state matches source of truth. Covered by tests simulating stale state. |
| **Source** | `[desloppify]` Pattern 4.3: "persisted state is never trusted alone — it's validated against source of truth" |

---

## Lane 4: Agent Orchestration

**Stage:** Later
**Goal:** Multi-agent safety, delegation tracking, and structural coordination. Builds on already-committed delegation types in `impulse-ops`.

### PR 4.1 — Delegation detection from agent output

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — |
| **Key files** | `src/delegation/detector.rs`, `src/context_lifecycle/extractor.rs` |
| **Success** | Detect delegation markers from JSON code fences and natural language patterns. >90% detection rate on synthetic test corpus. |
| **Source** | `[OpenSquirrel]` JSON code fences, `[Hermes]` delegation patterns |

### PR 4.2 — Delegation lifecycle tracking in daemon

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 4.1 |
| **Key files** | `src/delegation/tracker.rs`, `src/daemon/mod.rs` |
| **Success** | Register/complete/list delegations via daemon IPC. Frozen context snapshot at delegation time. Depth-limited to MAX_DEPTH=2. Delegation outcomes logged to HISTORY.jsonl. |
| **Source** | `[Hermes]` depth limits, frozen snapshots, restricted child toolsets |

### PR 4.3 — Structural conflict blocking via PreToolUse hook

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 1.1 (hook validation must pass) |
| **Key files** | Hook generation, `src/hooks/mod.rs`, LIVE_STATE tracking |
| **Success** | PreToolUse hook checks LIVE_STATE.json for file locks. Conflicting edits blocked with user-facing advisory. False positive rate < 1%. |
| **Source** | HONEST-ROADMAP Phase 1.5 coordination tier |
| **Gate** | HONEST-ROADMAP validation evidence must exist before starting |

### PR 4.4 — Agent status detection in terminal context bridge

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 2.1 |
| **Key files** | `impulse-term/src/context.rs`, `impulse-desktop/src/` terminal/status components |
| **Success** | Detect Idle/Working/Blocked/Starting from terminal screen text patterns. Status badges visible in desktop tab bar with correct colors. |
| **Source** | `[OpenSquirrel]` AgentStatus enum with exhaustive match |

### PR 4.5 — Anti-anchoring context injection (blind packet pattern)

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 1.3 |
| **Key files** | Context injection pipeline, `src/injection/` |
| **Success** | When replaying context, evidence (file changes, tool usage) is separated from prior decisions. Agents form independent judgments. A/B comparison shows no anchoring effect on agent behavior. |
| **Source** | `[desloppify]` Pattern 4.4: "blind packet system prevents agents from seeing target scores or prior assessments" |

---

## Lane 5: Retrieval Evolution

**Stage:** Later
**Goal:** Progressive search per ADR-0003: FTS5 → sqlite-vec → hybrid semantic.

### PR 5.1 — sqlite-vec extension loading and virtual table setup

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — |
| **Key files** | `src/retrieval/mod.rs`, `src/retrieval/vector.rs` (new) |
| **Success** | sqlite-vec loads as SQLite extension. Vector virtual table created with 384-dim columns. DELETE+INSERT workflow (no UPSERT on virtual tables). Sub-100ms KNN queries on 10K vectors. |

### PR 5.2 — Embedding pipeline for session history

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 5.1 |
| **Key files** | `src/retrieval/embedding.rs` (new), retrieval indexer |
| **Success** | Generate embeddings via sentence-transformers (all-MiniLM-L6-v2, 22MB). Index HISTORY.jsonl entries into sqlite-vec. <200ms insertion per entry. |

### PR 5.3 — Hybrid keyword + semantic search with score fusion

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 5.2 |
| **Key files** | `src/retrieval/search.rs`, search CLI commands |
| **Success** | Combined FTS5 keyword results with sqlite-vec KNN results via reciprocal rank fusion. Search quality improvement measurable on test queries. Explainability metadata includes both scores. |

### PR 5.4 — Three-tier working set for context management

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 5.3 |
| **Key files** | `src/context_lifecycle/`, stewardship module |
| **Success** | Hot (no compression) → Warm (mask/prune) → Cold (targeted summaries). Separate-model stewardship for context management. Token budget respected. |
| **Source** | `deep-research-compaction.md` three-tier working set design |

---

## Lane 6: External Integration

**Stage:** Later
**Goal:** Adapter layer for agent platforms beyond Claude Code.

### PR 6.1 — OpenCode plugin adapter (actual SDK rewrite)

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 1.1, PR 1.2 (validated hooks inform adapter design) |
| **Key files** | `impulse-plugin/` (complete rewrite) |
| **Success** | Plugin uses actual OpenCode Plugin SDK signatures. `experimental.chat.system.transform` mapped to session-start. `tool.execute.after` mapped to tracking. "Extract on next start" implemented for missing session.end. |
| **Source** | `[Agent Harness Analysis]` Section 1 — SDK gap analysis |

### PR 6.2 — MCP server exposing Impulse tools to Claude Code

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — (additive) |
| **Key files** | New `impulse-mcp/` crate or binary |
| **Success** | MCP tools appear as `mcp__impulse__*` in Claude Code. Exposed: `impulse_read_genome`, `impulse_update_genome`, `impulse_read_history`, `impulse_agent_status`. |
| **Source** | `[Agent Harness Analysis]` Section 2.5 — MCP integration |

### PR 6.3 — Dangerous command detection patterns

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — |
| **Key files** | `src/safety/patterns.rs` (new), coordinator integration |
| **Success** | 28 regex patterns for dangerous commands (rm -rf, DROP, privilege escalation). Warnings surfaced in daemon and desktop shell. False positive rate < 2%. |
| **Source** | `[Hermes]` approval.py — 28 dangerous command patterns |

---

## Lane 7: Operational Polish

**Stage:** Later
**Goal:** Agent control UX, artifact ergonomics, debug tooling. Follows ROADMAP-PLAN.md "Follow-On Order After Daemon Truth."

### PR 7.1 — Blocked-work indicators in the Tauri desktop shell

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 4.4 |
| **Key files** | `impulse-desktop/src/` overview and agent surfaces |
| **Success** | Visual indicators when agents are blocked on permissions, errors, or conflicts. Operator can see at a glance which agents need attention. |

### PR 7.2 — Focus/handoff/restart affordances in agent view

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 7.1 |
| **Key files** | `impulse-desktop/src/` agent surfaces, daemon commands |
| **Success** | One-click focus on specific agent, handoff context to new agent, restart stalled agent. Actions round-trip through daemon. |

### PR 7.3 — Review/apply artifact UX cleanup

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 2.4 |
| **Key files** | `impulse-desktop/src/` artifact surfaces |
| **Success** | Clearer confirmation flows for risky artifact actions. Better post-action result presentation. Stronger intentionality around apply/re-run/handoff flows. |

### PR 7.4 — Grid/Pipeline/Focus terminal view modes

| Field | Value |
|-------|-------|
| **Size** | L |
| **Depends** | PR 4.4 |
| **Key files** | `impulse-desktop/src/` terminal surfaces |
| **Success** | Grid (2xN tiled with status badges), Pipeline (coordinator→worker flow with arrows), Focus (single terminal expanded). Mode selector in toolbar. |
| **Source** | `[OpenSquirrel]` ViewMode enum: 1→full, 2→split, 4→2x2 |

### PR 7.5 — IMPULSE_DEBUG=1 diagnostic mode

| Field | Value |
|-------|-------|
| **Size** | S |
| **Depends** | — |
| **Key files** | CLI command handler, logging config |
| **Success** | `IMPULSE_DEBUG=1` enables verbose stderr trace. `impulse debug` shows PATH, API key presence, file states, hook config, daemon status. |

---

## Lane 8: Distribution & Adoption

**Stage:** Later
**Goal:** Make Impulse installable in <15 seconds per the "10-Second Setup" standard from HONEST-ROADMAP.md.

### PR 8.1 — shell-init command for eval-based setup

| Field | Value |
|-------|-------|
| **Size** | S |
| **Depends** | PR 1.1 (hooks must be validated) |
| **Key files** | New CLI subcommand |
| **Success** | `eval "$(impulse shell-init zsh)"` registers hooks automatically. Supports bash/zsh/fish. |

### PR 8.2 — Homebrew formula for binary distribution

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | — |
| **Key files** | New homebrew-impulse repo, CI workflow |
| **Success** | `brew install impulse-memory` works. Formula builds from source or downloads prebuilt binary. |

### PR 8.3 — impulse init setup wizard

| Field | Value |
|-------|-------|
| **Size** | M |
| **Depends** | PR 3.2, PR 8.1 |
| **Key files** | CLI init command, platform detection |
| **Success** | Interactive first-run: detect platform (Claude Code/OpenCode), generate hook config, create `.impulse/` directory, generate `.gitattributes`. Total setup < 15 seconds. |

---

## Dependency Graph

```
Lane 1 (Validation) ─────────────────────────────────────────────────────
  1.1 (SessionStart) ──→ 1.3 (GENOME usefulness) ──→ 3.1, 4.5
  1.2 (PreCompact)   ──→ 6.1 (OpenCode adapter)
  1.4 (extraction quality) — independent

Lane 2 (Daemon-Truth) ───────────────────────────────────────────────────
  2.1 (telemetry pub) ──→ 2.2 (overlay) ──→ 2.3 (remove shadows) ──→ 2.4 (artifact round-trip)
       └──→ 4.4 (agent status)

Lane 3 (Memory Quality) ─────────────────────────────────────────────────
  3.5 (state re-validation) — independent
  3.1 (privacy split) ──→ 3.2 (gitattributes) ──→ 8.3 (setup wizard)
  3.3 (semantic dedup) ←── 5.1 + 5.2
  3.3 ──→ 3.4 (contradiction resolution)

Lane 4 (Orchestration) ──────────────────────────────────────────────────
  4.1 (delegation detection) ──→ 4.2 (lifecycle tracking)
  4.3 (structural blocking) ←── 1.1
  4.4 (agent status) ←── 2.1 ──→ 7.1, 7.4
  4.5 (anti-anchoring) ←── 1.3

Lane 5 (Retrieval) ──────────────────────────────────────────────────────
  5.1 (sqlite-vec) ──→ 5.2 (embeddings) ──→ 5.3 (hybrid search) ──→ 5.4 (three-tier)
       └──→ 3.3 (semantic dedup)

Lane 6 (Integration) ────────────────────────────────────────────────────
  6.1 (OpenCode) ←── 1.1 + 1.2
  6.2 (MCP) — independent
  6.3 (safety patterns) — independent

Lane 7 (Polish) ─────────────────────────────────────────────────────────
  7.1 (blocked indicators) ←── 4.4 ──→ 7.2 (affordances)
  7.3 (artifact UX) ←── 2.4
  7.4 (view modes) ←── 4.4
  7.5 (debug mode) — independent

Lane 8 (Distribution) ───────────────────────────────────────────────────
  8.1 (shell-init) ←── 1.1
  8.2 (Homebrew) — independent
  8.3 (setup wizard) ←── 3.2 + 8.1
```

---

## Roadmap Alignment

| Roadmap Stage | Lanes | PRs |
|---------------|-------|-----|
| **Now** | Lane 1 (all), Lane 2 (all) | 1.1, 1.2, 1.3, 1.4, 2.1, 2.2, 2.3, 2.4 |
| **Next** | Lane 3 (3.1, 3.2, 3.5), Lane 4 (4.1, 4.2) | 3.1, 3.2, 3.5, 4.1, 4.2 |
| **Later** | Lane 3 (3.3, 3.4), Lane 4 (4.3–4.5), Lanes 5–8 | All remaining PRs |

**Independent PRs** (can land at any time): 3.5, 6.2, 6.3, 7.5, 8.2

---

## "Do Not Build" Reminders

These items from HONEST-ROADMAP.md remain gated behind evidence:

| Feature | Gate | Relevant PR |
|---------|------|-------------|
| SWARM vector injection | File-lock coordination fails in production | Not in this document |
| mem0 integration | Single-call extraction measured insufficient | PR 3.4 covers a lighter alternative first |
| Neo4j graph memory | Entity relationship traversal specifically needed | Not in this document |
| Structural blocking | Hook validation evidence exists | PR 4.3 (gated) |

---

*This document complements [`ROADMAP-PLAN.md`](./ROADMAP-PLAN.md) (execution sequence) and [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md) (risk register). It does not replace either.*

*Pattern sources: [Agent Harness Analysis](research/AGENT-HARNESS-ANALYSIS.md) (OpenCode, Claude Code, OneContext, Desloppify, Hermes Agent), [deep-research-compaction.md](research/deep-research-compaction.md), [RECONCILIATION-ANALYSIS.md](research/RECONCILIATION-ANALYSIS.md)*
