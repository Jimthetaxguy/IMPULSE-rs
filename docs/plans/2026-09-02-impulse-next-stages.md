---
title: Impulse Next Stages
description: Dependency-ordered plan for the next stages of the Impulse control plane, from a twelve-agent survey, three lens plans, and two judges
updated: 2026-09-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [plan, roadmap, stages, governance, ion, sandbox, memory]
---

# Impulse-rs: next stages

Checked at `origin/main` `36bda00` (2026-09-02). Produced by a twelve-agent pass: six subsystem
readers with file-and-line evidence, three staged plans from different lenses (meta-harness
capability, safety and governance first, product shippability), two adversarial judges, and one
synthesis. Both judges chose the safety-and-governance plan (24 and 23 of 25) and asked for the
same grafts from the other two; those are folded in below. Every effort statement is a
dependency, never a duration.

## 1. Where the code stands

The full Rust gate on `36bda00` is green: build clean, 2310 tests passed, 0 failed, 9 ignored,
strict Clippy and rustfmt clean, CI green on all three merges this week. The root crate is about
90k lines with 2574 test functions.

| Subsystem | Live today | Evidence |
|---|---|---|
| Governed lifecycle | Revisioned, idempotent task record; profiled `rust_workspace_v1` claim, verification, and review derived by the daemon; operator approval yields one `pending_review` memory candidate | `src/state/governed_task.rs:985-1248`, `src/governed_producers.rs:70-105,1079-1205`, `state/memory_candidate.rs:92-174` |
| Ion runtime | REPL with a six-tool registry; Anthropic-only tool loop bounded by the ADR-0017 `LoopContract` (10 rounds, 180 s, three-call streaks); guardrail-scanned y/N and `CONFIRM` gate; env-scrubbed `bash_exec`; bounded `document_read` for xlsx, csv, docx | `src/ion_repl/registry.rs:75-103`, `src/loop_contract.rs:30-36`, `src/ion_repl/chat.rs:290-384`, `src/ion_repl/tool_document.rs:57-80` |
| Desktop cockpit | Dioxus shell, eval bridge, PTY runtime with pre-PTY governed registration, daemon-truth telemetry, five rail views, about 250 in-crate tests | `impulse-desktop/src/{host_bridge,runtime,daemon_ops}.rs` |
| Memory and retrieval | GENOME (JSON) plus `HISTORY.jsonl`; FTS5 keyword search; review-first injection; candidates stop at `PendingReview` | `impulse-ops/src/memory_candidate.rs:76-80`, `src/retrieval/` |
| Decision record | ADR-0001 to 0015 and 0017 merged; 0016 drafted on a stale branch; 0014 proposed with kernel types not yet wired | `docs/decisions/README.md:43-45`, `src/{basis,settlement}.rs` |
| Engineering health | The documented gate (CLAUDE.md) is human-run only; CI runs the root package's tests, not `--workspace`; docs validator red on main; no branch protection | `.github/workflows/ci.yml:28,44`, `docs/validate_docs.py:41` |

| Risk | Severity | Evidence |
|---|---|---|
| Any same-user socket client, including a launched Builder holding `IMPULSE_SOCKET_PATH`, can mint `accepted` | P0 | `src/daemon/handlers.rs:982-1000`; `state/governed_task.rs:1379-1387` (kind-only check); no peer credentials anywhere |
| Builder mutates the live canonical tree; verification alone is snapshot-scoped | P0 (sandbox) | `VISION.md:244-246`; `governed_producers.rs:536-546` |
| Ion `file_write` and `bash_exec` run with empty path roots, so one `y` allows host-wide writes | P1 | `ion_repl/tool_bridge.rs:80-88`; `tooling/traits.rs:310-338` |
| A crash between a producer's side effect and its receipt re-runs the producer | P1 | `handlers.rs:1233-1270,1913-2000`; `state/persistence.rs:117-129` |
| The Codex packaged-acceptance lane, including the host-invoke envelope fix, is five commits local-only | P1 | no remote ref; `host_bridge.rs:61-68` versus `:311` |
| Git-fixture flakes; five copies of `init_git_repo` | P1 | `ion_repl/{tool_verify:140,mod:315,chat:1153}.rs`, `handlers/ion.rs:189`, `tests/ion_verify_cli.rs:29` |
| Semantic retrieval degrades silently to SHA-256 digests | P1 | `memory-pipeline/retrieval_embed.py:20-24`, `requirements.txt:4-6` |
| Tool output flows verbatim into context; `GuardTarget::ToolCall` is unused | P2 | `chat.rs:84-91,483-489`; `guardrail/types.rs:55` |

## 2. Plan principles

- **Dependency-ordered, never time-estimated.** Effort is what must land first.
- **Disjoint lanes.** Nothing in `src/daemon/*`, `impulse-desktop/*`, `impulse-ops/src/lib.rs`,
  `scripts/*`, or `.github/workflows/*` is edited until the two Codex lanes merge or a written
  handoff exists in the activity ledger.
- **ADR first for contract changes** (trust boundary, protocol bump, new durable record); a spec
  suffices for consuming existing primitives.
- **Research-grounded.** Each stage cites in-repo precedent plus primary sources.
- **Gate every lane** on an isolated `CARGO_TARGET_DIR` and record package-level pass, ignore,
  and fail totals on the lane card.

## 3. Stages

### Stage 0: close merged-but-unfinished work

**Goal:** a truthful decision record and a clean tree before any new ADR is filed.

- Open a small PR from `claude/document-read-hardening-20260902` for the streamed-docx and
  chart-sheet-skip change (`d352139`, already rebased onto main): add a chart-sheet fixture and an
  inflating-docx fixture, run the full gate, and land it. Until it merges, a Word file with a
  table of contents or a workbook with a chart sheet trips known gaps.
- Merge [#42](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/42), the README, VISION, and
  CONTEXT reframing around the control plane (docs only, already open).
- ADR-0014: add `proposed` to `STATUSES` in `docs/validate_docs.py:41` so the validator is green,
  then ratify or narrow it (Decision 3).
- ADR-0017: move `review` to accepted in the front matter, `docs/decisions/README.md`, and
  `docs/INDEX.md`.
- CLAUDE.md line 100: replace the nine-rule, round-cap-only description with the current truth
  (ten rules including `block-write-secret`; `LoopContract` trips; `ToolLoopStalled`), and replace
  the June test-density table with a pointer to the gate.
- Commit the dirty ADR-0015 addendum and skill-reference edits; move `sandbox-agent-analysis.md`
  to `docs/research/2026-08-22-sandbox-agent-analysis.md`; add `.cursor/` to `.gitignore`; add
  `.github/dependabot.yml` so the failing dependabot runs stop.
- Ask Codex, through the ledger, to push `agent/codex-dioxus-packaged-acceptance-20260830` and
  open the `host_bridge.rs` invoke-envelope fix as a standalone PR. Do not push another agent's
  lane.
- Rebase and amend the ADR-0016 draft (docs only): retitle its "candidate ADR-0017" to ADR-0019,
  add `LoopReport` as a diagnosis input, cite the tracked sandbox analysis. Do not file it as
  accepted.

**Depends on:** nothing. **Acceptance:** `python3 docs/validate_docs.py --all` exits 0; `git
status` clean on main; the docx follow-up PR gated. **ADR:** none. **Unblocks:** every ADR filing
below.

### Stage 1: Ion tool floor (sandbox, untrusted output, loop evidence, hermetic fixtures)

**Goal:** the y/N prompt stops being the only barrier between a model-issued call and the host.

- `DynamicToolBridge::run` derives its `ToolContext` from `ReplContext`: write roots limited to
  `repo_root`, read roots the repo plus explicit `/allow <path>` grants (new router command). The
  confirmation prompt prints the resolved absolute path; a target outside `repo_root` escalates
  to the literal `CONFIRM`.
- An untrusted-content envelope in `tool_result_content`; the first `GuardTarget::ToolCall`
  rules; a per-turn "untrusted seen" flag that escalates later gated calls.
- `respond()` prints the `LoopReport` on any loop trip, and a `/loop` command shows the last
  report.
- One hermetic `test_support::init_git_repo()` (`GIT_CONFIG_GLOBAL=/dev/null`,
  `GIT_CONFIG_NOSYSTEM=1`, `HOME` in the temp dir, stderr captured) replacing the five copies.
- GENOME format fix: JSON stays canonical, `Genome::to_markdown()` renders a sibling on write;
  `genome_read` and the MCP `impulse://genome` resource serve the rendering.
- Three real-file workflow tests (a spreadsheet receipt, a Markdown plan, a GENOME with two
  decisions) driven by a fake provider.

**Depends on:** nothing; every path is outside both Codex lane cards. **Acceptance:** an
out-of-root write is refused before `ToolRegistry::execute`; an instruction-shaped tool result
forces `CONFIRM` on a following innocuous `bash_exec`; one `init_git_repo` definition
workspace-wide; workflow tests pass under `cargo test --workspace`. **Owned paths:**
`src/ion_repl/{tool_bridge,mod,router,chat,tool_document,tool_verify}.rs`,
`src/guardrail/{defaults,types}.rs`, `src/test_support.rs`, `src/memory/mod.rs`,
`src/tooling/builtin/genome_read.rs`, `src/mcp/server.rs`, `tests/fixtures/`. **Research:**
`tool_bridge.rs:24-26` names this follow-up itself; Greshake et al. 2023 (arXiv:2302.12173);
Beurer-Kellner et al. 2025 (arXiv:2506.08837); CaMeL (arXiv:2503.18813); ROSA's
`ApprovalGrant`. **Spec:** `docs/superpowers/specs/2026-09-xx-ion-tool-sandbox-and-untrusted-output.md`;
no ADR. **Unblocks:** Stage L tier 1 with a real safety floor; Stage 5's Ion Builder running
root-bounded; the Stage 1b readers.

### Stage L: live acceptance sessions (cross-cutting)

**Goal:** prove with real providers and real documents what the gate cannot: the loop contract
and the document tool against a live model, and the governed lifecycle as one process.

- **Tier 1, Ion alone.** Launch `ion` in a scratch directory with a fresh `IMPULSE_HOME` and a
  real key supplied by the owner through the environment. Real invoice, budget workbook, and
  lease letter; everyday questions. Evidence: the `LoopReport` for every turn, the confirmation
  prompts seen, any same-error or repeated-call trips on genuine tool failures. Runs after the
  Stage 0 docx PR; better after Stage 1.
- **Tier 2, daemon plus governed Builder on a scratch repo.** Register a `rust_workspace_v1`
  task, launch Claude Code or Codex as the Builder, claim, verify, review, decide. This is the
  manual form of Stage 5. Before Stage 2 it must run on a single-user machine session, because
  the Builder's socket can still approve.
- **Tier 3, packaged cockpit.** Consume the Codex mounted-DMG acceptance once it lands; do not
  duplicate it.

**Depends on:** Stage 0 for tier 1; Stage 0 and the Codex lanes for tier 2. **Acceptance:** each
tier written up in `_working-files` with reports and governed-task records attached; every
failure becomes a fixture in the owning lane. **Unblocks:** a truthful "live today" line and the
fixture corpus Stages 1b and 5 need.

### Stage 1b: Ion capability track (parallel lane after Stage 1, off the critical path)

**Goal:** Ion becomes provider-neutral and useful beyond code, under the same loop contract.

- OpenAI `tools` and `tool_calls` plus MiniMax tool schemas replacing the stubs at
  `llm_backends/anthropic.rs:542-544,661-663`; a per-provider formatter so `format_messages`
  (`:193-207`) cannot drop tool messages; `IMPULSE_PROVIDER` in `ChatState::from_env`.
- Bridge `memory_search` and `genome_read` into `ReplToolRegistry` (ungated, read-only).
- `max_context_chars` on `LoopBudget` with tool-result-first compaction in `run_tool_loop`; a new
  `LoopTrip::ContextBudget`; the history-untouched-on-error invariant preserved.
- `document_read` gains pdf (text layer), txt, and md under the existing caps; delete
  `document_extract`, whose default path errors (`document_extract.rs:104-108`).
- Anthropic `cache_control` on the assembled system-and-tools prefix.

**Depends on:** Stage 1. **Acceptance:** a fake-provider round trip per provider; the
compaction test keeps `tool_use` stubs; `--no-default-features` still builds; MCP `tools/list`
no longer advertises `document_extract`. **Owned paths:** `src/llm_backends/{anthropic,mod}.rs`,
`src/ion_repl/{registry,chat,tool_document,context.rs}`, `src/loop_contract.rs`, `src/office/`,
`src/tooling/builtin/{document_extract,mod}.rs`, `Cargo.toml` (one PDF line). **Research:**
VISION.md lines 38 and 119-123; the deep-agent-kit ContextManifest assessment; MemGPT tiering
(arXiv:2310.08560). **Spec:** two design specs; no ADR. **Unblocks:** the external-harness
parity claim; ADR-0016 Phase C inputs.

### Stage 2: socket actor provenance (ADR-0018; after the Codex lanes merge)

**Goal:** `accepted` becomes provenance-enforced.

- Peer credentials on accept (`UnixStream::peer_cred()`); a per-daemon-run 0600 operator
  capability beside the socket; connection classes operator and non-operator.
- Reject `RecordOperatorDecision`, and `MarkRunning`, `MarkLaunchFailed`, `MarkRuntimeExited` for
  profiled tasks, from non-operator connections at `handlers.rs:982-1018`, building on the
  merged `ProcessRequestContext.daemon_identity` rather than forking it.
- Desktop and TUI clients present the capability; governed panes never receive it: extend
  `remove_inherited_impulse_env` (`impulse-term/src/backend.rs`) and the env list at
  `runtime.rs:2237-2250`.
- `AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator`; bump
  `ACCEPTED_RUN_MEMORY_DERIVATION_VERSION` so existing ledgers reconcile.
- Protocol v8 after the lane's v7, or fold into v7 if rebased together.

**Depends on / blocked by:** both Codex lanes merged (they own `daemon/*`,
`impulse-ops/src/lib.rs`, `impulse-desktop/src/*`), or a narrow written handoff (Decision 6).
**Acceptance:** a plain `DaemonClient` approve returns a typed unauthorized error with the
revision unchanged; a capability-bearing approve yields `accepted` plus one authenticated
candidate; a spawned pane's env lacks the capability; a pre-existing `MEMORY_CANDIDATES.json`
reconciles. **Research:** `handlers.rs:986-999`; the governed-runtime-producers plan lines
100-103; ADR-0012 lines 128-131; `SO_PEERCRED` and `LOCAL_PEERCRED` as used by systemd and
D-Bus; Miller 2006 on capabilities. **Unblocks:** Stages 4, 5, 6 and any event intake.

### Stage 3: durable producer reservation journal (state layer now; handler wiring after Stage 2)

**Goal:** no producer side effect can be silently re-executed after a crash.

- `src/state/producer_reservation.rs` with `reserve`, `release`, and `reconcile` through the
  private-file helper (`state/memory_candidate.rs:44-80`); reconcile at `State::new` marks an
  interrupted producer needs-rerun with the request id in the event chain.
- Wire `RunGovernedVerification` and `RunGovernedSupervisorReview` to reserve before the side
  effect and release with the receipt.

**Depends on:** state half, nothing; handler half, Stage 2's merge window. **Acceptance:** an
injected crash after `run_verification` returns leaves one open reservation on reload and a
replayed request does not re-run cargo (execution counter at `governed_producers.rs:1080-1081`);
a forged reservation fails ledger validation. **ADR:** an amendment to ADR-0012's consequences
plus a spec. **Research:** the desktop lifecycle outbox (`daemon_ops.rs:1061-1068`); Helland
2012 on idempotence; omnigent's dead-letter classifier. **Unblocks:** Stage 4's promote.

### Stage 4: Builder staged-worktree world scope and governed loop binding (ADR-0019)

**Goal:** Builders mutate a disposable worktree; an authenticated approval promotes.

- `WorldScope::{ReadOnlySnapshot, DisposableScratch, StagedAuthoritative, Authoritative}` on the
  registration (serde default `Authoritative`); a profiled registration materializes
  `.impulse/worktrees/<task_id>` through the existing `git worktree add --detach` path
  (`governed_producers.rs:536-546`); the PTY cwd is the staged root.
- A `PromoteGovernedOutcome` producer, allowed only after `accepted`, reserved through Stage 3,
  fast-forward only if the canonical HEAD equals the initial OID, else
  `PromotionBlocked{canonical_head}`; reject removes the worktree.
- Report `filesystem.scoped` honestly as mediated (git-level, not OS) in the compatibility
  preview, never structural.
- Graft: `LoopContract::governed_builder()` (max claim cycles, per-task wall clock, same-failure
  streak) evaluated in `apply_mutation` on `SubmitClaim`, tripping to `escalated` with
  `GovernedTaskEventKind::LoopTripped`; a `loop_report_digest` on the claim; an advisory heartbeat
  through `PublishTerminalOps` that never mutates `review_state`.

**Depends on:** Stages 2 and 3; the Codex lanes. **Acceptance:** the canonical tree stays
byte-identical through `awaiting_operator`; promote fast-forwards or blocks correctly; a restart
mid-promote reconciles without a second fast-forward; a loop trip replays equal; an old
`GOVERNED_TASKS.json` loads. **Research:** the sandbox analysis P0 item; ADR-0017 rule 6; ROSA's
world-scope vocabulary; E2B and Codex workspace-write precedent. **Unblocks:** Stage 5, ADR-0016
evaluation worlds, ADR-0014 fan-out.

### Stage 5: composed launched-runtime proof (Ion plus an external harness)

**Goal:** VISION's first complete vertical slice closed for both runtime paths.

- Add `impulse-desktop` as a root-crate dev-dependency (no cycle; it depends only on
  `impulse-ops` and `impulse-term`) so `tests/governed_process_flow.rs` can drive a real
  `DesktopRuntime` with `CARGO_BIN_EXE_impulse-rs` and `CARGO_BIN_EXE_ion`, with no `#[ignore]`.
- Ion path: staged worktree, `governed_submit_claim`, `governed-verify`, `governed-review`
  against a loopback fake provider through `ANTHROPIC_BASE_URL`, authenticated approve, promote,
  exactly one authenticated candidate, restart equality.
- External-harness path: a fake `claude` or `codex` on `PATH`, registry-detected, same lifecycle
  through the CLI `governed-claim`.
- `DEFAULT_HARNESS_TIMEOUT` (`agent/mod.rs:54`) moved onto `LoopContract::harness_query()`.

**Depends on:** Stages 1, 2, 4. **Acceptance:** both end-to-end tests pass on Ubuntu and macOS;
the Supervisor call count is one, with `tools: []` and temperature zero; a negative variant
proves the Builder socket cannot approve; the traceability marker for the vertical slice moves
to strong. **Blocked by:** CI must run `cargo test --workspace` (Stage H handoff). **Unblocks:**
Stage 6, ADR-0016 Phase C, the adapter-contract ADR.

### Stage 6: scoped memory promotion and dismissal (ADR-0020)

**Goal:** VISION step 10 live.

- `MemoryRecord{id, kind, scope, source: CandidateRef|OperatorManual, valid_from, superseded_by,
  digest}`; `MemoryCandidateStatus::{Promoted, Dismissed}`; `DecideMemoryCandidate` accepted only
  on operator-class connections with receipts; an append-only `MEMORY.jsonl` with GENOME
  regenerated as a projection.
- The Dioxus Memory view gains Promote and Dismiss, operator mode only.

**Depends on:** Stages 2 and 5. **Acceptance:** a promoted record is indexed and searchable and
a pending candidate is not (ADR-0013 rule 9); replay is idempotent; a tampered ledger fails
closed. **Research:** Zep and Graphiti (arXiv:2501.13956); A-MEM (arXiv:2502.12110); the
do-not-unify and wire-gate rules.

### Stage H: handoffs (requests, not plan edits)

- Codex: widen CI to the documented gate plus the docs validator; branch protection on main; a
  `--skip-live-host` split in `build-macos-app.sh`.
- Desktop track after Stages 2 and 4: producer buttons for verify and review only
  (`ui.rs:1726-1739`); EGUI retirement R0 to R5 with an archive manifest; Cmd-K and Settings
  removal.

## 4. Critical path

Codex pushes and merges its two lanes, then Stage 2 (ADR-0018), then Stage 3's handler wiring,
then Stage 4 (ADR-0019), then Stage 5, then Stage 6 (ADR-0020). Stages 0, 1, 1b, L tier 1, and
Stage 3's state layer run in parallel off that path. Stage 5 also needs Stage 1.

## 5. Explicitly deferred

- The runtime adapter contract, drift bench, and typed Supervisor-Builder messaging (VISION
  decisions 2, 3, 5): after Stage 5 proves the lifecycle worth generalizing.
- ADR-0016 implementation, phases A to D: needs Stage 4 worlds, Stage 5 evidence, and the
  bootable candidate-harness evaluator the draft itself names as missing.
- ADR-0014 wiring and fan-out: needs cost attribution and per-task worktrees.
- A second verification profile (node or manifest): after Stage 3.
- The loop-draft remainder (HALF_OPEN probes, checkpoint and replay, event intake): intake is
  hard-blocked behind Stage 2.
- SSE streaming and mid-await cancellation: after Stage 1b caching; must keep
  history-untouched-on-error.
- OS-level sandboxing and egress allowlists: Stage 4 is a git-level scope only.
- Semantic-embedding repair, multi-project routing, preview DMG signing: separate plans.

## 6. Open decisions for the owner

1. **License.** MIT in Cargo versus an Apache-2.0 file and no root LICENSE. Recommend Apache-2.0
   everywhere; it matches the only tracked license text and gives patent-grant clarity.
2. **`document_extract`.** Recommend delete; the real-systems rule forbids the stub and
   `document_read` supersedes it.
3. **ADR-0014.** Recommend narrowing to "accepted for kernel types only, fan-out rule 10
   deferred" so the validator baseline and the two waiting goals clear.
4. **Three March docs** (LONG-RANGE-ENHANCEMENTS, the two RUST-MULTI-AGENT guides). Recommend
   `status: archive`; nothing cites them as authority.
5. **PDF crate.** Recommend `pdf-extract` behind `office-support`, text layer only.
6. **Codex lane handoff.** Recommend waiting for merge unless the mounted-DMG gate stalls again;
   then take `daemon/{mod,handlers}.rs` and `impulse-ops/src/lib.rs` by ledger consent.
7. **Embedding default.** Recommend switching to the Ollama embedder and making the SHA-256
   fallback a hard error.
8. **Branch and worktree cleanup batch** (twelve remote branches that are ancestors of main, six
   overdue `backup/*` to tags). Wire-gated; approve as one batch.
9. **Branch protection on main.** Owner-only GitHub setting; enable once CI runs the documented
   gate.
10. **Live acceptance tier 1.** Say when a key is in the environment and which documents to use;
    tier 1 can run headless with evidence captured.
