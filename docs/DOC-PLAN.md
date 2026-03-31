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

### 2. `src/agent/harness.rs` — No Public module-level doc

**Problem:** `harness.rs` has excellent file-level rustdoc and method docs, but there's no `pub mod harness` re-export entry in `agent/mod.rs` doc comment explaining the harness protocol's role.

**Fix:** Add `//! The [`harness`] module — structured JSON protocol for CLI harness mode.` to `agent/mod.rs`

---

### 3. `src/context_lifecycle/extractor.rs` — Module doc missing

**Problem:** Ralph Plan 3 activated intent classification at 9 extraction sites. The `extractor.rs` module (511 lines) has no module-level doc explaining what it does.

**Fix:** Add file-level doc comment describing the extraction pipeline

---

### 4. `src/context_lifecycle/intent.rs` — Module doc missing

**Problem:** Intent classification is a key Phase 3 feature. Module has no doc.

**Fix:** Add file-level doc comment

---

### 5. `src/agent/coordinator.rs` — Missing doc for top-level items

**Problem:** 1,119 lines. Has `CoordinationResult` and `Recommendation` types used in IPC responses. Several public functions lack doc comments.

**Fix:** Add doc comments for `CoordinationResult`, `run_full_coordination()`, `aggregate_pane_summaries()`

---

### 6. `src/agent/prompts.rs` — Module doc exists but may be stale

**Problem:** 405 lines. Module doc exists but doesn't mention the new structured `HarnessRequest`/`HarnessResponse` flow.

**Fix:** Update module doc to mention harness protocol integration

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

These docs describe features or designs that are now obsolete or replaced:

| Doc | Reason |
|-----|--------|
| `docs/vision/DASHBOARD-DESIGN.md` | EGUI workbench replaced Dashboard concept |
| `docs/vision/TUI-AUGMENTATION-VISION.md` | TUI workbench already built |
| `docs/vision/REAL-TIME-INJECTION-VISION.md` | Lane 5 (retrieval) covers this now |
| `docs/AI build_complete_guide.md` | Unclear purpose, may be obsolete |

**Action:** Assess each for deletion or archival.

---

## Summary: Prioritized Action Items

| Priority | Item | Files | Effort |
|----------|------|-------|--------|
| **P0** | Fix IPC-PROTOCOL.md (stale + missing endpoints) | `docs/IPC-PROTOCOL.md` | M |
| **P1** | Add module docs: extractor.rs, intent.rs | `src/context_lifecycle/` | S |
| **P1** | Add coordinator.rs doc comments | `src/agent/coordinator.rs` | S |
| **P2** | Update CLAUDE.md agent IPC section | `CLAUDE.md` | S |
| **P2** | Update agent/prompts.rs module doc | `src/agent/prompts.rs` | S |
| **P3** | Audit RUST-CANONICAL-CONTRACT.md Section 3 | `docs/spec/` | M |
| **P3** | Assess obsolete vision docs for removal | `docs/vision/` | S |

---

## Verification After Doc Updates

After any doc update, run:
```bash
python3 docs/validate_docs.py --contract
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings
```
