# Ralph Plan 4 — Validation & Daemon-Truth EGUI

> **Plan:** Ralph Plan 4
> **Started:** 2026-03-31
> **Goal:** Validate core hook assumptions + begin daemon-truth EGUI integration
> **Previous plan:** Ralph Plan 3 (30 loops, codebase reduction + agent harness wiring — COMPLETE)

---

## Context

Ralph Plan 3 (ALL 30 LOOPS COMPLETE ✓) delivered:
- Codebase reduction: 132,442 → 77,867 LOC (−54,575, −41%)
- Agent harness: 10/10 features wired (context→prompts, intent classification, coordination pipeline, conflict history IPC, JSON harness protocol, session awareness, specialized IPC)
- Module extraction: render_panels.rs → 5 modules, daemon/mod.rs → mod.rs + protocol.rs + handlers.rs
- Test density: Tooling at 17.1 tests/KLOC (84 tests)

**This plan (Ralph Plan 4)** addresses the roadmap's immediate next steps:
1. **Lane 1 (Validation)** — Prove the core hook injection assumptions actually work
2. **Lane 2 (Daemon-Truth EGUI)** — Begin making the daemon the authoritative source for EGUI surfaces

**Why validate first?** Lane 1 unblocks Lane 4 (Agent Orchestration). If hook injection fails validation, several roadmap items need redesign.

---

## Phase 1: Validation (PRs 1.1, 1.2, 1.4)

> **Goal:** Measure whether the core hook assumptions hold water
> **Approach:** Build test harnesses, run them, document pass/fail evidence
> **Note:** PR 1.3 (GENOME usefulness A/B) is manual/1-week — skip for now

### Loop 1: PR 1.1 — SessionStart stdout injection validation harness

**What it does:** SessionStart hook emits a marker string → verify it surfaces in next session context.

**Implementation:**
- Create `impulse-rs/tests/hook_validation/session_start_test.rs`
- Generate a temp project dir with `.impulse/` initialized
- Register a SessionStart hook that emits `IMPULSE_TEST_MARKER=hooks_are_working`
- Spawn a sub-process that runs Claude Code/OpenCode in that dir
- Capture the next session's system context
- Verify the marker string appears

**Success criteria:** Marker string found in session context output.

**IPC endpoints needed:**
- `GetLastSessionContext` — fetch the context from the most recent session

### Loop 2: PR 1.2 — PreCompact survival validation harness

**What it does:** PreCompact hook outputs known content → trigger compaction → verify content survives.

**Implementation:**
- Create `impulse-rs/tests/hook_validation/precompact_survival_test.rs`
- SessionStart sets up a PreCompact hook emitting `MUST_SURVIVE: TEST_CONTENT`
- Trigger compaction via `steward compact`
- After compaction, read `.impulse/context/current-task.md`
- Verify `MUST_SURVIVE: TEST_CONTENT` is present

**Success criteria:** Marker content present in post-compaction context.

### Loop 3: PR 1.4 — Extraction quality benchmark on real transcripts

**What it does:** Run extraction on 3-5 real Claude Code JSONL transcripts, measure precision/recall.

**Implementation:**
- Create `impulse-rs/tests/hook_validation/extraction_benchmark.rs`
- Use 3-5 saved session transcripts (from `.claude/sessions/` or exported JSONL)
- Run extraction pipeline on each
- Manual sampling: does extracted content match what actually happened?
- Report: capture rate, false positive items, missed items

**Success criteria:** Documented capture rate on real sessions.

---

## Phase 2: Daemon-Truth EGUI Start (PR 2.1)

> **Goal:** Terminal telemetry publication — terminal panes publish to daemon, not local state
> **Note:** This is a large PR (size L). Focus on the core publication mechanism only.

### Loop 4: Design `PublishTerminalOps` IPC request

**What it does:** Define the `PublishTerminalOps { report: TerminalOpsReport }` daemon request.

**Implementation:**
- Add `PublishTerminalOps` variant to `daemon protocol.rs` `DaemonRequest` enum
- Define `TerminalOpsReport` in `impulse-ops/src/lib.rs` if not already present
- Fields: `source_id`, `published_at`, `agents`, `context`, `interventions`
- Implement handler in `daemon/mod.rs` — stores report in ephemeral daemon memory
- Add unit tests for the new IPC path

**Files touched:** `src/daemon/protocol.rs`, `src/daemon/mod.rs`, `impulse-ops/src/lib.rs`

### Loop 5: Wire terminal panes to publish telemetry

**What it does:** `impulse-term` terminal panes send `PublishTerminalOps` on key events.

**Events to publish:**
- Tab spawn
- Tab shutdown
- Tier change (compact/inject/intervention change)
- 2-second heartbeat

**Implementation:**
- In `impulse-term/src/` or wherever terminal pane state lives, wire publish calls
- On spawn: emit initial `TerminalOpsReport`
- On heartbeat: emit updated report
- On state change: emit updated report

**Files touched:** `impulse-term/src/` (terminal panes), `impulse-ops/src/lib.rs`

### Loop 6: Verify full publish/subscribe loop

**What it does:** End-to-end test of terminal → daemon → EGUI snapshot flow.

**Implementation:**
- Spawn daemon
- Open `impulse-gui`
- Create terminal tab
- Verify `SubscribeOps` response includes terminal telemetry
- Verify telemetry stale/purge rules apply (10s stale, 60s purge)

---

## Phase 3: Documentation Sync

### Loop 7: Sync all docs post-Phase 1 + 2

**What it does:** Ensure all docs reflect current state after PRs 1.1, 1.2, 1.4, 2.1.

**Files to check:**
- `docs/spec/RUST-CANONICAL-CONTRACT.md` — capability matrix (PR 1.1, 1.2 done)
- `docs/HONEST-ROADMAP.md` — validation evidence recorded
- `docs/ROADMAP-PLAN.md` — PR 2.1 marked in progress/done
- `CLAUDE.md` — if new CLI commands added

**Run:** `python3 docs/validate_docs.py --contract`

---

## Phase 4: Planning Checkpoint + Plan 5

### Loop 8: Metrics audit

**Measure:**
- LOC: `find impulse-rs/src -name "*.rs" | xargs wc -l | tail -1`
- Test count: `cargo test 2>&1 | grep "test result"`
- PRs complete: 1.1, 1.2, 1.4, 2.1

### Loop 9: Create Ralph Plan 5

**Based on what landed:**
- If PRs 1.1/1.2 passed → Lane 1 complete, start PR 1.3 (GENOME A/B, manual) + Lane 2.2 (daemon overlay)
- If PRs 1.1/1.2 failed → Update HONEST-ROADMAP.md immediately, redesign required
- Lane 2.2 (daemon overlay with stale/purge rules) — next logical step after 2.1

---

## Working Log

| Loop | Task | Status | Notes |
|------|------|--------|-------|
| 1 | PR 1.1 SessionStart validation harness | | |
| 2 | PR 1.2 PreCompact survival harness | | |
| 3 | PR 1.4 Extraction quality benchmark | | |
| 4 | Design PublishTerminalOps IPC | | |
| 5 | Wire terminal panes to publish telemetry | | |
| 6 | Verify publish/subscribe loop | | |
| 7 | Sync docs post-Phase 1+2 | | |
| 8 | Metrics audit | | |
| 9 | Create Ralph Plan 5 | | |

---

## Current Metrics (Baseline from Ralph Plan 3)

| Metric | Value |
|--------|-------|
| Total LOC | 77,867 |
| Source LOC | 58,664 |
| Tests | 1,002 |
| Agent features | 10/10 |
| Build/Clippy/Fmt | CLEAN |
| `#[allow(dead_code)]` | 9 (all justified) |

---

## Success Criteria

1. PR 1.1 passes → SessionStart hook stdout injection validated
2. PR 1.2 passes → PreCompact content survival validated
3. PR 1.4 complete → Extraction quality measured on real transcripts
4. PR 2.1 complete → Terminal panes publish telemetry to daemon
5. All verification gates pass: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
6. `python3 docs/validate_docs.py --contract` passes
