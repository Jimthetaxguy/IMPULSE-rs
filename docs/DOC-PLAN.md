# Documentation Plan — Post-Ralph Plan 3

> **Created:** 2026-03-31
> **Purpose:** Identify stale docs, missing docs, and cross-doc sync issues
> **Based on:** Code review of impulse-rs/src after Ralph Plan 3 (Phase 3 agent harness completion)

---

## Critical: Stale Docs (Must Fix)

### 1. `docs/IPC-PROTOCOL.md` — Severely Stale

**Problems:**
- `PROTOCOL_VERSION` is 2 in `protocol.rs` but docs say 1
- Missing new Phase 3 IPC requests:
  - `AgentAssist` — AI coordination assistance with context enrichment
  - `GetConflictHistory` — conflict resolution history
  - `ClearResolvedConflicts` — clear resolved conflicts
  - `AgentReviewCode` — code review via agent
  - `AgentAnalyzeError` — error analysis via agent
  - `AgentSummarizePane` — pane activity summary via agent
  - `GetAgentPool` — all sessions grouped by role
  - `RegisterDelegation` / `CompleteDelegation` / `ListDelegations` — delegation tracking
- Missing new `DaemonResponse` variants:
  - `AgentAssistResult` — with `recommendations` + `pane_summaries`
  - `AgentSpecializedResult` — for review/analyze/summarize
- `PublishTerminalOps` is listed but TerminalOpsReport fields not documented

**Fix:** Rewrite IPC-PROTOCOL.md sections for Agent System + update version

---

## Medium: Missing Module Docs

> **Updated 2026-03-31:** After reading the actual files, all P1 module docs were already present:
> - `extractor.rs` ✓ — "Output extractor — parses agent PTY output for structured insights"
> - `intent.rs` ✓ — "Intent detection for agent activities"
> - `coordinator.rs` ✓ — "Cross-pane coordination logic" + all key functions documented
> - `agent/prompts.rs` ✓ — "System prompts for the Impulse Agent's augmentation modes"
> - `agent/mod.rs` ✓ — Good module doc covering both API and Harness modes
>
> **No action needed on P1 items.**

---

## Low: Cross-Doc Sync

### 7. `CLAUDE.md` ↔ `IPC-PROTOCOL.md` — Version drift

**Problem:** `CLAUDE.md` references IPC protocol but doesn't mention the new agent endpoints.

**Fix:** Update the Architecture section in CLAUDE.md to mention "10 agent IPC endpoints (AgentAssist, AgentReviewCode, AgentAnalyzeError, AgentSummarizePane, GetConflictHistory, ClearResolvedConflicts, GetAgentPool, delegation tracking)"

---

### 8. `docs/ROADMAP-PLAN.md` — Still references old `PROTOCOL_VERSION = 1`

**Problem:** ROADMAP-PLAN.md doesn't mention PROTOCOL_VERSION update.

**Fix:** Add note about PROTOCOL_VERSION = 2 in daemon-truth section

---

### 9. `docs/spec/RUST-CANONICAL-CONTRACT.md` — v1.5 but some sections may lag

**Problem:** Canonical contract v1.5 was just updated (test density + agent harness marker). But the IPC contract section (Section 3) should be audited for new endpoints.

**Fix:** Audit Section 3 against `protocol.rs` DaemonRequest enum

---

## Stale/Dead Docs to Potentially Remove

Assessment complete (Loop 4):

| Doc | Assessment | Action |
|-----|-----------|--------|
| `docs/vision/DASHBOARD-DESIGN.md` | Obsolete — Zellij plugin concept replaced by EGUI workbench | **DELETE** |
| `docs/vision/TUI-AUGMENTATION-VISION.md` | Superseded — Ralph Plan 4 Phase 3/4 (Loops 7-23) implement the actual TUI/UX work | **ARCHIVE** (move to `docs/archive/`) |
| `docs/vision/REAL-TIME-INJECTION-VISION.md` | Still relevant — Phase 3 per canonical contract, current phase (Now) covers retrieval/injection | **KEEP** |
| `docs/AI build_complete_guide.md` | Not Impulse-specific — James's personal 22K-line AI-Native Development guide | **REMOVE from project** (belongs in personal docs) |

**Actions to take:** Delete DASHBOARD-DESIGN.md and AI build_complete_guide.md; archive TUI-AUGMENTATION-VISION.md.

---

## Summary: Prioritized Action Items

| Priority | Item | Status | Files | Effort |
|----------|------|--------|-------|--------|
| **P0** | Fix IPC-PROTOCOL.md (stale + missing endpoints) | ✅ DONE (Loop 1, `e612548`) | `docs/IPC-PROTOCOL.md` | M |
| **P1** | Add module docs: extractor.rs, intent.rs | ✅ ALREADY DONE | `src/context_lifecycle/` | S |
| **P1** | Add coordinator.rs doc comments | ✅ ALREADY DONE | `src/agent/coordinator.rs` | S |
| **P2** | Update CLAUDE.md agent IPC section | ✅ DONE (Loop 2, `97cb83b`) | `CLAUDE.md` | S |
| **P2** | Update agent/prompts.rs module doc | ✅ ALREADY DONE | `src/agent/prompts.rs` | S |
| **P3** | Audit RUST-CANONICAL-CONTRACT.md Section 3 | ⬜ PENDING | `docs/spec/` | M |
| **P3** | Assess obsolete vision docs for removal | ⬜ PENDING | `docs/vision/` | S |

---

## Verification After Doc Updates

After any doc update, run:
```bash
python3 docs/validate_docs.py --contract
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings
```
