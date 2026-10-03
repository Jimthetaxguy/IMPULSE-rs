---
title: Ion SQLite blackboard — off-context agent state
description: Work card for ion-blackboard-20261003
updated: 2026-10-03
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Ion SQLite blackboard — off-context agent state

## Lane Facts
- Owner: claude (Opus 5.5, interactive session with James)
- Role: implementer
- Branch: `claude/ion-blackboard-20261003`, stacked on `claude/ion-scout-subagent-20261002`
  (photon, ADR-0022, and the unpushed PRD commit `e1f4657`)
- Worktree: `../IMPULSE-rs.wt-blackboard`
- Owned paths: `impulse-rs/src/blackboard/`, `impulse-rs/src/ion_repl/tool_blackboard.rs`,
  `impulse-rs/src/ion_repl/tool_search.rs`, `docs/decisions/0023-sqlite-blackboard-off-context-state.md`,
  this card
- Shared paths edited: `impulse-rs/src/ion_repl/{chat,mod,registry,tool_claim}.rs`,
  `impulse-rs/src/daemon/{mod,tests}.rs`, `impulse-rs/src/governed_producers.rs` (clean-subject
  exemption), `impulse-rs/src/handlers/config.rs` (`impulse init` ignore list),
  `impulse-rs/src/state/config.rs` (new `blackboard` section), `impulse-rs/src/lib.rs`; one-line
  `blackboard: None` additions to `ReplContext` literals in `tool_{bridge,document,photon,verify}.rs`
  tests; `CONTEXT.md`, `CLAUDE.md`, `docs/decisions/README.md`
- Plan/spec: ADR-0023
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo clippy --all-targets --no-default-features --features office-support -- -D warnings`,
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented, reviewed, and fixed; see Handoff Notes

## Decisions
- 2026-10-03: own file `.impulse/blackboard.db`. There is no session-state SQLite to share
  (session state is JSON), and `retrieval.db` is a rebuildable index.
- 2026-10-03: `rusqlite` (already a dependency, bundled SQLite), connection per operation.
- 2026-10-03: live keys are insert-only; one atomic upsert guarded on expiry.
- 2026-10-03: "governed task result path" means the claim summary. It was hard-capped at 4 KiB and
  refused above; it now spills to a `blackboard:` artifact id. The Ion tool-result path spills too.
- 2026-10-03: `delegate_task` and `approve_gate` are reserved in `ORCHESTRATOR_TOOL_SURFACE`, not
  built. Dynamic tool advertisement waits for the orchestrator role.

## Handoff Notes
- 2026-10-03 adversarial review (one refutation pass, reproductions in a scratch copy). Confirmed
  correct: the expiry-guarded upsert (live/expired/NULL-TTL cases, `?7` binding), the expression
  index is used, and claim previews validate at every threshold from 512 B to 1 MiB with ASCII,
  multi-byte, and whitespace-led summaries. Findings and fixes:
  - P1 a typo in `blackboard` stopped `State`, so the daemon, from loading → `Config` keeps the
    section raw; `BlackboardConfig::from_section` validates for consumers, which fall back to
    defaults with a report.
  - P2 injected text split across two fetch pages evaded the guard → a fetch also scans the whole
    entry (`entry_text_for_scan`).
  - P2 compaction dropped the spill key → `ToolExecutor::compaction_note` keeps it.
  - P2 staged-worktree claim evidence died on Discard → claim summaries go to the canonical
    `.impulse` derived from the daemon socket path (also closes the staged-worktree gap).
  - P3 claim spill bypassed nonblank/NUL checks → checked first; the preview drops leading
    whitespace.
  - P3 failed submissions left permanent rows → deleted unless the daemon records the claim.
  - P3 the reference was larger than a 513-byte result → minimum threshold 1,024.
  - P3 `search_tools` with schemas rendered 13 KB inline → no longer exempt from spilling.
  - P3 synchronous SQLite in async → spill, hashing, and the whole-entry read run on the blocking
    pool.
- Remaining known gaps (ADR-0023): Supervisor review sees only a spilled claim's preview; no daemon
  IPC endpoint; no per-agent row ownership; the orchestrator role (`delegate_task`,
  `approve_gate`, dynamic tool advertisement) is not built.
