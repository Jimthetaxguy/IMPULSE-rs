---
title: Honest Roadmap - Historical Hook and Memory Validation Register
description: Historical adversarial risk register retained for unresolved hook, compaction, extraction, and memory-quality validation
version: '1.2'
status: active
phase: historical
type: reference
category: validation
audience: builder
tags: [roadmap, honest, limitations, critique]
updated: 2026-07-12
last_updated: 2026-07-12
---

# Impulse: Historical Hook and Memory Validation Register

> **Version:** 1.2 | **Status:** Active in narrow historical scope | **Updated:** 2026-07-12
> **Source:** Session 5 critique (21 iterations of adversarial analysis)
> **Current product authority:** [`../VISION.md`](../VISION.md) and
> [`spec/RUST-CANONICAL-CONTRACT.md`](spec/RUST-CANONICAL-CONTRACT.md)

---

## What This Is

This is a **historical TypeScript/Bun-era critique with one retained active purpose**: tracking
unresolved evidence for Claude hook injection, compaction survival, extraction quality, and the
behavioral usefulness of durable memory. It is not the current whole-product roadmap, architecture
contract, control-plane risk register, or implementation sequence.

Use `VISION.md`, the Rust canonical contract, the user-story map, and test traceability for current
product direction. Use this file only when validating the hook/memory hypotheses in Assumptions
A-D below. Phase labels, TypeScript paths, npm distribution, three-file architecture, and claims
that Impulse has no daemon or database are preserved as historical evidence and must not direct
current control-plane work.

> **Current routing note (2026-07-12):** this file remains the narrow validation register for hook
> injection, compaction survival, extraction quality, and `GENOME.md` usefulness. It is not
> canonical for the governed supervisor/builder slice, runtime roles, capability negotiation,
> desktop authority, or the Now/Next/Later roadmap.

> **Note (2026-02-24):** This document was originally written during the TypeScript/Bun era (Session 5, pre-Rust pivot). The project has since been rewritten in Rust (`impulse-rs`). Many corrections below have been addressed by the Rust rewrite — see inline status markers. The document is retained for its analytical rigor and as a record of assumptions that were tested.

---

## Historical Memory-Sidecar Promise

The claims in this section record what the February critique believed or had validated at that
time. They are not a current inventory of the Rust control plane.

### What the Phase-Era Design Could Deliver

- **Cross-session memory** for solo AI coding workflows
- **Automatic extraction** of architectural decisions at session end (LLM-based)
- **Basic multi-agent file awareness** (advisory — agents can see what others are editing)
- **Zero-infrastructure Phase 1** — no databases, no daemons, no ML models
- **Human-readable, git-tracked** project memory

### Phase-Era Limitations Recorded by the Critique

| Limitation | Current Capability | When Fixed | Status |
|-----------|-------------------|------------|--------|
| Contradiction resolution | Append-only. Stale decisions persist until manually pruned. | Phase 2 (semantic dedup) | Open |
| Structural agent conflict prevention | Advisory only. Agents can ignore LIVE_STATE.json. | Phase 1.5 (PreToolUse hook) | Open |
| Guaranteed extraction quality | LLM misses implicit decisions; 40-char dedup is brittle | Phase 2 (semantic dedup) | Open |
| Zero-dependency install | ~~Requires Bun runtime on PATH (npm install)~~ Compiled Rust binary | ~~Phase 1.5~~ | **Resolved** (Rust) |
| Fast deferred extraction | ~~Cannot run synchronously~~ Rust binary startup is <5ms | ~~Phase 1~~ | **Resolved** (Rust) |
| Team privacy separation | Everything in GENOME.md goes to git | Phase 1.5 (PROJECT.md/PERSONAL.md split) | Open |

---

## Retained Hook and Memory Validation Risks

**These remain evidence-gated.** If they fail, revise the affected hook or memory claim; do not
infer that the wider control-plane architecture has failed.

### Assumption A: SessionStart stdout injection

> The spec says: "stdout → Claude receives as system context"

**UNVERIFIED.** The exact mechanism by which SessionStart hook stdout becomes Claude Code system context is not documented with a working example. There are two possibilities:
1. Injected as a system message (what we want)
2. Injected as a tool_result in conversation history (different behavior)

**Validation:** Run `cargo run -- validate-hooks --platform claude-code`, register the generated `SessionStart` sentinel hook, and verify Claude Code treats the emitted marker as usable startup context in the next session.

---

### Assumption B: PreCompact stdout survival

> The spec says: "PreCompact hook → stdout survives compaction"

**UNVERIFIED.** The mechanism by which PreCompact hook output "survives" compaction is not documented with a working example.

**Validation:** Write a 10-line bash PreCompact hook that outputs `MUST SURVIVE: TEST CONTENT`. Trigger a compaction. Verify the content appears in the post-compaction context.

---

### Assumption C: Extraction hypothesis

> The spec assumes: injecting GENOME.md into system prompt "improves agent behavior"

**UNVERIFIED.** There is no direct A/B test for Claude Code agents reading GENOME.md-style files. The cited "17-32% productivity improvement" is from generic context engineering research, not coding-agent-specific studies.

**Validation:** Manually create a 20-line GENOME.md for a real project. Use it daily for 1 week. Subjectively assess: does Claude Code reference it? Does it prevent context re-discovery?

---

### Assumption D: Extraction quality on real sessions

> The spec assumes: beginning+end sampling captures the right content

**UNVERIFIED.** Real Claude Code sessions contain large file pastes, verbose tool outputs, and error messages. The sampling strategy may not capture the right content.

**Validation:** Apply the extraction prompt to 3-5 real Claude Code JSONL transcripts. Assess quality. Tune the sampling strategy based on real data.

---

## Historical Architectural Corrections

The corrections below explain the design context in which the original critique was written.
They do not override current Rust code, the daemon/control-plane contract, or current ADRs.

### Correction 1: Deferred Extraction Must Be Async

**Current spec (PRODUCT-SPEC-v2.md):** "If SessionEnd is interrupted, next SessionStart runs extraction before loading context."

**Problem:** This is synchronous. SessionStart has a <30ms latency target. Synchronous extraction (LLM call) takes 500ms-3s. This is a 20-100x violation.

**Correction:** SessionStart detects `extraction_pending` and spawns SessionEnd as a BACKGROUND process (`impulse-session-end &`). SessionStart continues immediately. Extraction completes in the background while the user starts typing.

```
SessionStart hook — corrected behavior (pseudocode):

1. If extraction_pending is set in LiveState, spawn `impulse-session-end`
   as a detached background process. Do NOT await — continue loading
   context immediately. The extraction completes asynchronously while
   the user begins their session.

2. Load GENOME.md from the working directory.

3. Continue with normal context injection.
```

---

### Correction 2: Multi-Agent Coordination Is Advisory

**Current framing (REALISTIC-FRAMEWORK.md):** "Agents self-coordinate via plain JSON."

**Honest framing:** LIVE_STATE.json coordination is advisory. Agents read it if their system prompt instructs them to. There is no structural enforcement.

**Tiers of coordination (add to all docs):**
| Phase | Type | Mechanism |
|-------|------|-----------|
| 1 | Advisory awareness | LIVE_STATE.json (informational; agents may ignore) |
| 1.5 | Structural blocking | PreToolUse hook blocks conflicting edits |
| 2 | Git-based isolation | Recommend separate git worktrees per agent |
| 3+ | Semantic | SWARM vector similarity (if file-path proves insufficient) |

---

### Correction 3: type: agent Hook Deserves Evaluation

**Current ADR-004:** "Agent hooks can take up to 50 turns — overkill for extraction."

**Correction:** "Can take up to 50 turns" ≠ "will take 50 turns." A well-designed extraction agent hook takes 2-3 turns:
1. Turn 1: Read transcript (using Read tool)
2. Turn 2: Write decisions to GENOME.md (using Edit tool)

**Benefits of type: agent over SDK call:**
- Zero API key required (Claude Code already pays for the session)
- Claude's own model does the extraction (no additional model configuration)
- Full file system access via tools

**Action:** Include type: agent evaluation in the Pre-Phase 1 spike. Compare quality and cost against SDK call. Use whichever performs better.

---

### Correction 4: npm Distribution Conflicts With Target Persona

> **Status: RESOLVED** — The Rust rewrite eliminated this entire class of problem. `cargo install --path .` produces a native binary with zero runtime dependencies.

**Original problem:** npm global install conflicted with terminal-native developers using Homebrew/Nix/Cargo. Bun/Node via mise created PATH conflicts.

**Resolution:** Impulse is now a compiled Rust binary. `cargo build --release` produces a single executable. Distribution via `cargo install` or direct binary download. No runtime required.

---

### Correction 5: GENOME.md Needs Privacy Boundary

**Current design:** Single GENOME.md, git-committed. All extracted content goes to team.

**Problem:** Personal preferences ("I work better in evenings", "I avoid testing first") end up in team git history.

**Correction (Phase 1.5):**
- `.impulse/PROJECT.md` — team decisions, committed to git
- `.impulse/PERSONAL.md` — individual preferences, added to `.gitignore`
- Extraction prompt classifies each item: team vs personal

---

### Correction 6: GENOME.md Needs Git Merge Strategy

**Current design:** No git merge strategy specified.

**Problem:** Two developers using Impulse on the same repo create merge conflicts on GENOME.md.

**Correction (add to `impulse init`):**
```
# .gitattributes
.impulse/PROJECT.md merge=union
.impulse/HISTORY_INDEX.md merge=union
```

The `union` merge strategy keeps all lines from both versions. For append-only files, this is correct: never delete, always accumulate.

---

## Historical CLI Product Notes

These setup and UX comparisons are retained for provenance. Current installation and runtime
claims come from the Rust contract and current release documentation.

### The "10-Second Setup" Standard

| Tool | Setup | Time |
|------|-------|------|
| atuin | `brew install atuin` + `eval "$(atuin init zsh)"` + `source ~/.zshrc` | 12 seconds |
| zoxide | `brew install zoxide` + `eval "$(zoxide init zsh)"` | 8 seconds |
| **Impulse Phase 1.5** | `brew install impulse-memory` + `eval "$(impulse shell-init zsh)"` + `impulse auth setup` | ~15 seconds |

Current Phase 1 (npm) is closer to 30-60 seconds with manual steps.

### The "Invisible Tool" Standard

Best-in-class tools disappear into the workflow. Impulse must meet this standard:
- Session start: `[impulse] Loaded 45 lines of project memory` (stderr) — visible without asking
- Session end: `[impulse] Extracted 3 new decisions` (stderr) — confirmation it worked
- No other noise. No configuration required for default operation.

### Debug Standard

Every tool needs a debug mode. Impulse needs:
```bash
IMPULSE_DEBUG=1 impulse-session-start  # Verbose execution trace to stderr
impulse debug                           # Diagnostic: PATH, API key, file states, hook config
```

---

## Retained Hook and Memory Evidence Checklist

Complete the applicable checks before strengthening hook or durable-memory claims:

- [ ] **Hook injection**: bash SessionStart outputs known text → verify it appears as system context in Claude Code
- [ ] **PreCompact survival**: bash PreCompact outputs known text → verify it appears post-compaction
- [ ] **JSONL format**: Inspect 3 real Claude Code transcripts, document exact format of user/assistant/tool messages
- [ ] **type: agent hook**: Test 2-turn agent SessionEnd hook. Measure quality vs SDK call.
- [ ] **Extraction hypothesis**: Manually curate GENOME.md for 1 week. Assess whether Claude Code uses it.

**Go/No-go gates:**
- If hook injection fails → redesign injection mechanism
- If PreCompact fails → redesign compaction strategy
- If extraction hypothesis fails → reconsider core value proposition
- If type: agent wins → use it as primary extraction path (no API key needed)

---

## Historical "Do Not Build" List

This table captured February sequencing assumptions. Several entries have since changed or landed;
it is not an active backlog. Current sequencing comes from `VISION.md` and the user-story map.

The original sequencing rule was to build these only when deployed evidence required them:

| Feature | Current Status | Evidence Required |
|---------|---------------|-------------------|
| SWARM vector injection | Planned for Phase 3 | File-lock coordination fails in production |
| Custom WASM Zellij plugins | Planned for Phase 3 | `tail -f LIVE_STATE.json` reported insufficient |
| Python memory pipeline | Planned for Phase 2 | FTS5 demonstrably fails on real queries |
| mem0 integration | Planned for Phase 3 | Single-call extraction measured insufficient |
| Neo4j graph memory | Planned for Phase 3 | Entity relationship traversal specifically needed |
| MCP server wrapper | Planned for Phase 2 | Direct file reading demonstrably insufficient |

---

## Historical Document Correction Queue

This queue is retained as a record of the TypeScript-era critique. It is not an active correction
queue and must not be used to route current implementation work.

The original queue recorded these proposed corrections:

1. **PRODUCT-SPEC-v2.md** — Add "Honest Limitations" section. Fix deferred extraction to async. Reframe multi-agent coordination as "advisory."

2. **ADR-002 (Three Files)** — Add PROJECT.md/PERSONAL.md split. Add .gitattributes union merge. Add concurrent write risk mitigation detail.

3. **ADR-004 (Extraction)** — Add type: agent hook as alternative/preferred path. Fix deferred extraction to async. Add spike validation requirement.

4. **ADR-005 (Distribution)** — Add Phase 1.5 binary distribution plan. Add eval shell-init alternative to settings.json patching.

5. **PHASE1-CHECKLIST.md** — Add Pre-Phase 1 spike as first priority. Fix deferred extraction to async spawn. Add gitattributes step. Add format version header.

6. **REALISTIC-FRAMEWORK.md** — Clarify "advisory" vs "enforcement" for multi-agent coordination. Update distribution to include binary path.

---

## Current Scope Summary

This document now has one current job: hold the four hook/memory validation risks and their
evidence checklist without presenting a legacy phase plan as the product roadmap.

The original critique changed three things in its historical design context:

1. It added a pre-implementation hook and memory validation spike.

2. It reframed file-based multi-agent awareness as advisory rather than structurally enforced.

3. It identified npm distribution as a mismatch for the target persona.

Those conclusions are provenance, not current control-plane authority. Current live-versus-target
truth belongs to `VISION.md`, `RUST-CANONICAL-CONTRACT.md`, `USER-STORY-MAP.md`, and
`TEST-TRACEABILITY.md`.

---

_Created: 2026-02-21 | Source: Session 5 Ralph Loop, 21 iterations_
_Updated: 2026-02-24 | Added Rust-era status markers_
_Scoped: 2026-07-12 | Retained only as the hook and memory validation register_
