# CONTEXT — Impulse Ubiquitous Language

> **Read this first.** Shared L0/L1 vocabulary for Impulse. Update a term here when its stable
> meaning changes; put detailed history in plans/ADRs rather than growing this glossary.
>
> Entries are tagged **`[code]`** (live in the current repository implementation) or
> **`[vocabulary]`** (the product contract; "Closest in code" names today's partial carrier).
>
> Cross-agent contract: `AGENTS.md`. Product contract:
> `docs/spec/RUST-CANONICAL-CONTRACT.md`. Product north star: `VISION.md`. Current boundary map:
> `docs/ARCHITECTURE-CLARIFICATION.md`.

---

## What Impulse is

Impulse is a **terminal-native local control plane and harness manager for AI software-engineering
agents**. It launches and scopes heterogeneous runtimes, supervises their work, and holds completion
to observed evidence and human approval. Shared services include tools, telemetry, handoffs,
policy, credentials, artifacts, verification, and memory. Claude Code, Codex, and similar CLIs
retain their internal loops; Ion is the native runtime. Dioxus, ratatui, and the CLI are operator
surfaces; daemon/runtime contracts remain authoritative. Memory is one platform service, not the
whole product.

Workspace: `impulse-rs/` (Cargo workspace). Main crates: `impulse-rs`, `impulse-ops`,
`impulse-term`, `impulse-desktop`, and `impulse-ion`; `impulse-gui` is legacy/frozen.

---

## Identity and hierarchy

### role — `[vocabulary]`
The stable behavioral contract assigned to an agent: obligations, permissions, tools, context,
communication, and evidence; independent of model, runtime, process, session, and pane.
- **Closest in code:** the narrow launch-time `AgentRoleId`/`AgentRoleAssignment` contract plus the
  concrete `SupervisorPermissionPolicy`. `AgentRole::{Coordinator, Worker}` remains legacy pane and
  delegation topology, not product-role identity.
- **Boundary:** generalized role composition is not live. ADR-0011's narrow governed Builder task
  lifecycle is live and remains distinct from a general role system.

### runtime — `[vocabulary]`
The engine that executes an agent role. External runtimes are wrapped CLI harnesses (for example,
Claude Code or Codex); Ion is a native direct-provider/tool-loop runtime.
- **Closest in code:** `impulse-desktop/src/runtime.rs`, `src/agent/`, `src/ion_repl/`, and
  `src/llm_backends/`.
- **Boundary:** there is not yet one common runtime-adapter trait or capability-negotiation protocol.

### agent platform id — `[code]`
An open, validated string identity (`AgentPlatformId`) owned by `AgentRegistry` across desktop,
host, MCP, browser, reducer, launcher, and snapshot paths. The registry adds Ion, derives capability
manifests, resolves its sibling binary, and fails closed on unknown/blank IDs. Declared platform and
observed command remain distinct so wrappers stay visible; legacy closed enums remain only for
wire/disk compatibility.
- **Source of truth:** `impulse-ops/src/agent_registry.rs`.

### role launch compatibility — `[code]`
A static preflight comparing caller-supplied product-role requirements with trusted Rust-owned
runtime declarations. Strength is ordered `unsupported` < `advisory` < `mediated` < `structural`;
mandatory gaps block and optional gaps degrade. Dioxus previews the result, while the desktop
runtime re-evaluates before agent-id reservation or PTY creation. Working-directory mediation is
not filesystem sandboxing.
- **Source of truth:** `impulse-ops/src/role_assignment.rs`, `impulse-desktop/src/runtime.rs`, and
  ADR-0010.

### agent instance — `[vocabulary]`
One running identity with a platform/runtime, role, workspace target, process state, and telemetry.
It is not interchangeable with its session or pane.
- **Closest in code:** `AgentRuntime`, `AgentRuntimeSnapshot`, and desktop runtime records.

### session — `[code]`
A bounded, persisted unit of work with a start, tracked activity, and recorded end. Verification
may gate that end. A session may link to an agent instance but does not own its process.
- **Source of truth:** `WorkbenchDaemonRequest::{CreateSession, EndSession}` and CLI
  `session-start` / `session-end --verify`.

### governed task — `[code]`
A daemon-owned assignment plus exact acceptance criteria and four distinct truth layers: worker
claim, verification evidence, Supervisor judgment, and operator decision. A profiled Builder binds
the canonical clean Git subject and exact shared Builder assignment; the daemon independently
recomputes runtime compatibility, derives producer actors, and creates automatic records. Execution
remains independent of review, and only operator approval accepts. Resume/reassignment is future
work.
- **Source of truth:** `impulse-ops/src/governed_task.rs`, `src/state/governed_task.rs`, ADR-0011,
  and ADR-0012.

### accepted-run memory candidate — `[code]`
A deterministic pending-review projection of one accepted governed task, persisted in owner-only
`MEMORY_CANDIDATES.json` with versioned source assurance/evidence and repairable from task truth. It
is not curated memory and never mutates `GENOME.md`/`HISTORY.jsonl`. Since ADR-0020 it carries a
review status (`pending_review`, `promoted`, `dismissed`) and a re-derivation carries that status
forward rather than resetting it.
- **Source:** `impulse-ops/src/memory_candidate.rs`, `src/state/memory_candidate.rs`, ADR-0013,
  ADR-0020.

### memory record — `[code]`
One durable curated memory fact: `{id, kind, scope, project_id, source, valid_from, superseded_by,
digest}`, where `source` is the candidate it was promoted from or an operator's own note. Its id and
digest are both built from one SHA-256 over its content, deliberately excluding `valid_from`, so a
replayed decision re-derives the same identity after a crash. Records live only in append-only,
hash-chained `.impulse/MEMORY.jsonl`; only the `project` scope is writable today.
- **Boundary:** a record is never edited in place. Supersession is a later log entry, not a rewrite.
- **Source:** `impulse-ops/src/memory_candidate.rs`, `src/state/memory_record.rs`, ADR-0020.

### promotion — `[code]`
The operator-class decision that turns one pending candidate into a memory record: append to
`MEMORY.jsonl`, then commit the candidate ledger (status, CAS revision, committed log head, receipt),
then regenerate the projection and mark the retrieval index dirty. Its sibling is dismissal, which
requires a nonblank reason and produces no record. Both are terminal, idempotent by request id, and
refused on a candidate that no longer matches its accepted governed-task derivation.
- **Boundary:** the decision input carries no authentication field; the daemon stamps provenance
  from the connection class, as it does for an operator decision (ADR-0018).
- **Source:** `impulse-ops/src/memory_wiring.rs`, `src/state/memory_candidate.rs`, ADR-0020.

### projection — `[code]`
`.impulse/GENOME_PROJECTION.md`: the deterministic, byte-stable markdown rendering of the currently
valid memory records, regenerated wholesale and never hand-edited or merged into. It is what a
runtime memory tool reads; the raw candidate ledger is never exposed to one.
- **Boundary:** distinct from `GENOME.md`, which stays hand-curated by `impulse memory add` and is
  never regenerated — raw candidates, promoted records, and the projection are separate artifacts.
- **Source:** `src/state/memory_record.rs`, ADR-0020.

### producer reservation — `[code]`
A durable record of intent to run a daemon-owned producer side effect (verification, Supervisor
review, and — reserved for ADR-0019 — promote), persisted independently of `GOVERNED_TASKS.json` in
owner-only, digest-verified `PRODUCER_RESERVATIONS.json`. Reserved before the side effect, released
with a receipt reference once the effect and its governed-task mutation are both durable; a
reservation still open on reload is reconciled to `NeedsRerun` and noted on the owning governed
task's own event chain. Since the 2026-09-12 handler wiring it is load-bearing, not just
observable: `RunGovernedVerification`, `RunGovernedSupervisorReview`, and `PromoteGovernedOutcome`
each run their side effect *and* persist its governed-task receipt inside one `with_reservation`
closure, the in-memory `acquire_governed_producer_lock` stays as the concurrency optimization it
always was, a same-revision duplicate is refused with the journal's typed error, and a rerun after
an interrupted attempt is distinguishable at the wire through `pending_rerun_reason`. It is not
panic-safe: an in-process panic is treated exactly like a crash, never as an ordinary `Err`.
- **Source of truth:** `src/state/producer_reservation.rs`, `src/daemon/governed_wiring.rs`,
  ADR-0012 amendment (2026-09-02) and its handler-wiring note (2026-09-12).

### task — `[vocabulary]`
The broader product assignment concept. A governed task is today's durable carrier; delegations,
`AgentRuntime.current_task`, and Ion `Task` contracts remain separate legacy/specialized carriers
until a future hierarchy ADR reconciles their cardinalities.

### pane — `[code]`
A UI/terminal viewport attached to an agent channel. It is a presentation and input-routing object,
not an authorization or identity boundary.
- **Source of truth:** `src/ui/pane_manager.rs`, `src/ui/terminal_pane.rs`, and desktop runtime ids.

### project — `[vocabulary]`
A logical codebase/governance boundary for memory, artifacts, policy, and verification. Today it is
usually represented by one registered workspace root and one daemon-owned `ProjectOpsSnapshot`.

### workspace target — `[code]`
The explicit working-directory/project root in which an agent process operates. Several agent
instances may share a workspace; a cockpit can register and switch among several workspaces.
Structural filesystem enforcement depends on the selected runtime or sandbox.
- **Source of truth:** `impulse-desktop/src/workspace.rs` and `WorkspaceTarget` in runtime models.

---

## Platform services

### daemon / workbench truth — `[code]`
The long-running coordination point that owns project workbench snapshots, session operations,
supervisor actions, artifacts, governed tasks, and telemetry overlays over a versioned JSON-line
Unix socket.
- **Source of truth:** `impulse-ops/src/lib.rs` and `src/daemon/{mod,protocol,handlers}.rs`.
- **Desktop daemon-truth wire:** PTY lifecycle facts publish as `TerminalOpsReport` on change and
  heartbeat, then return through daemon `SubscribeOps` snapshots. Local
  `agent_runtime_update`/`agent_snapshot` messages own terminal mechanics only and cannot overwrite
  `ProjectOpsSnapshot`. Subscription freshness is distinct from publish degradation; lifecycle
  delivery uses a reentrant FIFO, natural exits reap records, and runtime agent ids remain one-use
  routing addresses until the protocol carries explicit incarnations. The adapter currently binds
  one daemon project; cross-workspace daemon routing remains a protocol follow-up.
- **Governed task wire:** protocol v5 adds specialized claim/verify/review requests whose callers
  omit derived truth. The daemon attests clean Git subjects, verifies detached committed code, and
  binds strict API-only Supervisor output. Profiled registration requires the shared canonical
  Builder assignment and the exact compatibility result recomputed from the daemon registry;
  generic producer mutations fail for profiled tasks.
- **Candidate wire:** protocol v6 adds serde-defaulted `ProjectOpsSnapshot.memory_candidates` only;
  it defines no candidate mutation request.
- **Staged wire:** protocol v9 adds `PromoteGovernedOutcome` and `DiscardGovernedStagedWorktree`
  and folds staged materialization into `RegisterGovernedTask`. Every producer request answers with
  a **producer acknowledgement** — the governed task flattened into the response object plus
  `replayed` and an optional `pending_rerun_reason` — so a pre-v9 client reading a bare
  `GovernedTaskRun` is unaffected; the discard acknowledgement adds `discarded_root` and, when the
  discard drops the only ref to an accepted-but-blocked commit, `unreferenced_accepted_commit`.

### managed agent turn — `[code]`
One exclusive, bounded use of the cached `ImpulseAgent`. Concurrent turns fail fast with typed
`Busy { resource: agent_turn, retry_after_ms }`; cancellation releases the guard without removing
the cached agent or losing history.
- **Source of truth:** `try_lock_agent_for_turn` and agent request handlers in
  `src/daemon/handlers.rs`; provider timeouts bound the turn.

### step-model policy — `[code]`
The pure, provider-neutral runtime-control policy that selects a final model from a host-resolved
current/configured model and optional same-provider escalation candidate. Applications decide
whether inference runs; provider layers own availability and transport; each host records the
returned reason in its own evidence domain.
- **Source of truth:** `impulse-step-model`, the adapter and arena record in
  `src/agent/step_model.rs`, and ADR-0015.

### loop contract — `[code]`
The declared budget an Impulse-owned loop runs under (`LoopContract`: round cap, wall clock,
repeated-call and same-error streak limits), the per-run breaker that evaluates every trip
condition, and the typed `LoopReport` every run leaves behind. A trip is an execution fact, never
a review outcome. Today it bounds the Ion tool loop and, through
`LoopContract::governed_builder()`, one governed Builder task's claim cycles.
- **Source of truth:** `src/loop_contract.rs`, `Agent::chat_with_tools` in
  `src/llm_backends/mod.rs`, and ADR-0017.

### context budget — `[code]`
The characters of working conversation history a loop may carry into one model round
(`LoopBudget::max_context_chars`; 200,000 for the Ion tool loop, unset for the governed Builder).
It is the one budget the loop tries to *fit* before it stops: over budget, the loop compacts
tool-result content oldest-first into a bounded stub that keeps the `tool_use` id and names the
tool, and only trips `LoopTrip::ContextBudget` when compaction cannot recover enough room. Two
things are never compacted: prose, and the results the current run produced (which the model has
not been shown yet) — a result carried in from an earlier completed turn is ordinary compactible
history. A stub quotes the model-supplied tool name as an escaped, length-bounded JSON
string and is re-wrapped in the executor's own untrusted-output framing via
`ToolExecutor::wrap_compaction_stub`. Which results are already compacted is tracked by
`tool_use_id` in a `CompactedResults` set that travels with the working history (committed on
success, discarded on error), never by matching the stub's text against tool output. Measured in characters, not tokens, at the widest wire
rendering of each input, so the number is exact, reproducible, and never below what any of
Anthropic, OpenAI, or MiniMax actually sends. `LoopReport::compactions` records how many results a
run compacted.
- **Source of truth:** `LoopBudget`/`LoopTrip`/`LoopReport` in `src/loop_contract.rs`,
  `enforce_context_budget` in `src/llm_backends/mod.rs`, and ADR-0017's 2026-09-12 addendum.

### wire format — `[code]`
The per-provider rendering of a chat request (`llm_backends::WireFormat`: `Anthropic` block arrays
against OpenAI-style `tool_calls` plus `role: "tool"` messages; MiniMax speaks the OpenAI shape).
Selecting one is mandatory, so no provider can inherit a formatter that drops tool blocks.
`IMPULSE_PROVIDER` picks which transport a host builds and fails closed on an unknown value; it
never picks a model — ADR-0015's step model stays the only model picker.
- **Source of truth:** `format_messages_for` in `src/llm_backends/anthropic.rs`,
  `provider_from_env`/`build_provider` in `src/llm_backends/mod.rs`, `ChatState::try_from_env`.

### tool sandbox roots — `[code]`
The filesystem boundary every path-checking ion REPL tool (`file_read`, `file_write`, `bash_exec`'s
`cwd`, `document_read`, `ion_verify`'s `repo`, and — Stage 1b-B, bridged the same way as
`file_read`/`file_write` rather than through a bespoke path check — `memory_search`'s and
`genome_read`'s `impulse_dir`) resolves against: a session's `ReplContext.repo_root`
is its fixed write root (never widened, not even by a literal `CONFIRM`). Reads are additionally
granted (unconditionally, not via `/allow`) under Impulse's own home directory
(`history::impulse_home()`, i.e. `IMPULSE_HOME`/`$HOME/.impulse`) — review round 1, P2-2/P2-3:
`memory_search`/`genome_read`'s own default previously resolved relative to the process's working
directory with no relationship to this sandbox at all, and an omitted `impulse_dir` parameter was
invisible to the generic `validate_paths` check besides. `ReplContext.allowed_read_roots` is a
further read-only extension list grown one path at a time via `/allow <path>` (which refuses an
empty or nonexistent path; a bare `/allow` lists current grants;
a grant of `/`, `$HOME`, or an ancestor of the repo root still succeeds -- the human explicitly
asked for it -- but prints a loud warning first, since it effectively disables the read sandbox).
`ReplContext::sandbox_tool_context` builds the `ToolContext` every one of those tools actually
checks paths against -- `tool_bridge::DynamicToolBridge::run` for the bridged tools,
`resolve_document_path`/`IonVerifyTool::run` directly for the other two, all against the same roots.
`ion_repl::chat::ReplToolExecutor` independently checks a *pending* `file_write`/`bash_exec cwd`
call's resolved path(s) before confirmation, escalating an out-of-sandbox target to a literal
`CONFIRM` gate; a heuristic scan of `bash_exec`'s shell TEXT (absolute-path tokens -- including
one glued directly to a redirect/pipe with no whitespace, e.g. `>/tmp/f` or `</etc/passwd` -- plus
`..`, `~`, `$HOME`/`${HOME}`, and an escaping `cd` target) escalates the same way but is explicitly
advisory, not enforcement -- the sandbox does not confine what a shell command's own text can touch
beyond its `cwd`, and known misses (a path built at runtime, e.g. inside a nested `python3 -c`
string, or reached through a shell variable indirection) stay documented as advisory limits rather
than chased with a bigger regex. Every tool result (success or error) sent back to the model is
wrapped in a nonce-delimited "untrusted tool output" envelope (so content cannot forge the closing
delimiter) and scanned against `GuardTarget::ToolCall` built-in rules (documented
false-positive-prone); a match sets a flag sticky for the rest of the SESSION (not just the turn,
to survive both out-of-order batch confirmation and content persisting in history) that escalates
every later gated call to the same `CONFIRM` gate, reset only by `/clear`. `governed_submit_claim`
is a separate, non-bridged `ReplTool` that mutates daemon-owned governed-task state, is ungated,
and is a conscious carry-forward gap: the sticky `untrusted_seen` escalation does not reach it
because it never goes through `ReplToolExecutor`'s `CONFIRMATION_REQUIRED_TOOLS` gate at all.
- **Source of truth:** `src/ion_repl/{mod,chat,tool_bridge,tool_document,tool_verify}.rs`,
  `src/guardrail/defaults.rs`, and
  `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`.
### world scope — `[code]`
The filesystem authority a launched runtime works under, declared on the governed task record:
`read_only_snapshot`, `disposable_scratch`, `staged_authoritative`, `authoritative` (the serde
default, so every pre-ADR-0019 ledger loads unchanged). Only the last two are materializable
today; the others fail registration closed. A `staged_authoritative` Builder works in a disposable
Git worktree at `<workspace>/.impulse/worktrees/<task id>`, created from the daemon-attested
initial OID, and only a separate `PromoteGovernedOutcome` step after operator acceptance
fast-forwards the canonical branch — or reports `PromotionBlocked{canonical_head}` if that head
moved. It is a Git-level boundary, so the compatibility preview reports `filesystem.scoped` as
**mediated**, never structural: nothing stops the process from writing outside the checkout.
Three invariants the scope depends on, all hardened by the 2026-09-12 post-merge review: a staged
task has **no** launch working directory until its worktree is materialized (`MarkRunning` refuses
it, and `launch_working_directory` returns an error rather than the canonical workspace); every
daemon-owned producer — claim *and* verification — observes that staged root rather than the
canonical checkout; and the shared-repository-configuration pin is a digest of **raw file bytes**
(including files reached through `include`/`includeIf`) that promotion compares before spawning any
Git process, since asking Git a question inside a repository whose configuration is in question is
not a neutral act. Since protocol v9 the scope is reachable end to end: `RegisterGovernedTask`
materializes the staged worktree as part of registration (so the checkout exists before any PTY
launch), and `PromoteGovernedOutcome`/`DiscardGovernedStagedWorktree` are live endpoints — all
three operator-class, checked before any state read, with a blocked promotion answered as a
*successful* response carrying the typed outcome rather than an error, and a drifted configuration
pin answered as a typed `StagedConfigRefusal` whose remedy is discard-and-re-materialize. Claim,
verification and promotion each compare the pin before spawning Git; **discard deliberately does
not** — it removes a checkout and materializes no files, so no driver can fire, and an unpinned or
drifted worktree is exactly the one an operator most needs to be able to reclaim.
- **Source of truth:** `WorldScope` and `StagedWorktree` in
  `impulse-rs/impulse-ops/src/governed_task.rs`, the staged producers in
  `impulse-rs/src/governed_producers.rs`, the endpoints in `impulse-rs/src/daemon/governed_wiring.rs`,
  and ADR-0019.

### staged control — `[code]`
A Dioxus cockpit affordance that drives one ADR-0019 staged-worktree endpoint: **Promote** and
**Discard**, rendered beside Approve/Reject on the operator board and only for a
`staged_authoritative` run. A control is offered exactly when the daemon would accept it —
`governed_outcome_is_promotable` and `staged_worktree_is_discardable` in
`impulse-ops/src/governed_wiring.rs` are the single predicates both sides read — and a control that
is not offered always names the rule that withheld it rather than greying out silently. Two
renderings carry ADR-0019's own wording requirements: a **blocked-promotion banner** (typed per
`PromotionBlockedReason`, showing the canonical head plus a per-reason remedy, and saying the run
stays accepted and the checkout stays active — it is an execution fact, never an error), and a
**discard confirmation** that states what the discard costs and shows the unreferenced accepted
commit's OID *before* anything is sent.
- **Source of truth:** `promote_control_state`, `discard_control_state`,
  `blocked_promotion_notice`, `discard_cost_notice`, and `staged_config_refusal_notice` in
  `impulse-rs/impulse-desktop/src/ui.rs`; the gateway methods in
  `impulse-rs/impulse-desktop/src/{runtime,daemon_ops}.rs`.

### document read tool — `[code]`
Ion's read-only `document_read` tool: reads `xlsx` by streaming cells through calamine's cell
reader under a character and cell budget (never the dense-grid parser; a chart or dialog sheet
holds no cells and is skipped rather than failing the workbook), `docx` by streaming
`word/document.xml` event by event through quick-xml so the object tree, many times the size of
the XML, is never built, `csv` through the `office` parser, and `txt`/`md` as whole-file UTF-8
reads (`md` additionally outlined by ATX heading, fence-aware) — files up to 10 MiB; containers
inflated through a 64 MiB cap first; legacy `xls` refused; parsing on the blocking pool — and
returns a section outline with whole-document offsets plus a bounded character window that ends on
a line boundary and names the next offset, so a model inside a loop contract can jump to and page
through an everyday document without flooding its context. Ungated like `file_read`; absolute
paths are accepted, subject to the same sandbox as every other path this tool resolves; registered
only with the default `office-support` feature. `document_extract` (a separate, stubbed CLI/daemon
dynamic tool whose default path always errored) was deleted rather than extended when
`document_read` gained `pdf`.

**`pdf` runs in an isolated child process, not in-process (review round 1 on PR #54).** A
self-referencing Form XObject makes `pdf_extract::output_doc_page` recurse until the thread's
stack is exhausted -- `abort()`, not an unwinding panic, so `spawn_blocking`'s `JoinError`
containment (what every other kind here still relies on) cannot catch it. `document_read` is
ungated, so an in-process crash there previously killed `ion` outright. Page rendering is now
isolated in a hidden `internal-pdf-text` subcommand (`extract_pdf`/`run_pdf_extraction_child` in
`tool_document.rs`, shared by both `impulse-rs` and `ion` via `handlers::internal_pdf_text`),
spawned with `kill_on_drop`, a `ProcessGroupGuard`, a 30s wall-clock timeout, and (unix)
`RLIMIT_AS`/`RLIMIT_CPU`. Its `BoundedSink` (`std::fmt::Write`) refuses a write the instant the
running character total would exceed budget -- true check-before-push, since the earlier
per-page-then-check design reached multi-GB RSS on a small crafted file. Encryption is refused via
a raw `/Encrypt` byte scan run before any parser touches the file, not `doc.is_encrypted()` after
loading: `lopdf::Document::load` silently authenticates a PDF whose *user* password is empty.
Annotation/`AcroForm` text is never extracted (only a page's own `/Contents` stream is rendered).

**Review round 2 refuted three round-1 claims:** the unbounded parent-side `child.wait_with_output()`
read (~3.2 GB RSS from a rogue child, fixed with `read_capped`/`read_capped_tail` on independent
tasks), `BoundedSink` never having bounded memory in the first place (`pdf-extract`'s own stream
decompression happens before `BoundedSink` ever runs; fixed with a `preflight_pdf_streams` counting
pass), and a legal PDF name hex-escape (`/Encr#79pt`) bypassing the raw `/Encrypt` byte scan (fixed
with a name-escape decoder plus a trailer-dictionary belt-and-braces check).

**Review round 3 found the parent itself still parsed PDF structure, and the preflight missed
LZW.** The "cheap" in-process page count (`precheck_pdf`, since deleted) called
`pdf_extract::Document::load` in the PARENT -- but `lopdf::Document::load` eagerly decompresses
every `/Type /ObjStm` object stream during loading with no hook to bound it, so a crafted ObjStm
blew up the PARENT before any preflight could run (the review's `objstm_bomb2g.pdf`, 2.04 MB on
disk, drove the parent to 2,078 MB RSS). Fixed structurally: the parent's entire PDF-specific job
before spawning the child is now a raw `/Encrypt` byte scan (`pdf_encryption_prescan`) -- page
count, the trailer re-check, the preflight, and rendering are now exclusively the child's job,
authoritatively. The preflight also only ever counted `FlateDecode`; `LZWDecode` (`lzwmulti300.pdf`
reached 4.12 GiB RSS) is now walked via each stream's full filter chain
(`Stream::filters()`), with an outer `ASCII85Decode`/`ASCIIHexDecode` layer decoded first (fixing a
real false-refusal on legitimate `[/ASCII85Decode /FlateDecode]` chains along the way) and
deny-by-default for any filter this preflight cannot bound-count. Since `Document::load`'s own
eager ObjStm inflation still cannot be pre-counted even confined to the child, a memory-watchdog
thread (`spawn_memory_watchdog`, polling `getrusage`-derived peak RSS every ~10ms,
self-terminating via a distinct exit code the parent maps to a typed error) is the real bound for
that path -- and the ONLY enforced bound on macOS specifically, since `RLIMIT_AS` there is accepted
by `setrlimit` but silently not kernel-enforced (confirmed empirically). The total decompression
cap also rose from 64 MiB to 512 MiB (per-stream stays 64 MiB): the old total cap refused ordinary
multi-hundred-page documents that were never actually proven to pass it, since the round-2
"legitimacy" fixture had no compressed streams at all.
- **Source of truth:** `src/ion_repl/tool_document.rs`, `src/handlers/internal_pdf_text.rs`,
  `tests/pdf_extraction_isolation.rs`, `tests/fakes/rogue-stdout-shim.sh`, and
  `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md` (full review round 1/2/3 detail
  and fixture table).

### bridged memory tools — `[code]`
`memory_search` and `genome_read` (`src/tooling/builtin/{memory_search,genome_read}.rs`) are
ordinary `src/tooling::DynamicTool`s, registered in `ToolRegistry::with_defaults()` since before
Stage 1b-B and already reachable from the CLI/daemon/MCP; that lane's addition was bridging them
into Ion's `ReplToolRegistry` (`registry.rs::with_defaults`) via `DynamicToolBridge`, the same
mechanism as `file_read`/`file_write`/`bash_exec`, rather than writing new `ReplTool` wrappers.
They are ungated (read-only, `Capability::FileSystemRead` only).

**`impulse_dir` default and validation (review round 1 P2-2/P2-3, corrected in review round 5, P1
Codex items 2/3, corrected again in review round 6, MEDIUM REFUTED):** the default is
`<repo_root>/.impulse` (the PROJECT's own state directory, where `GENOME.md`/`retrieval.db`
actually live) — never `history::impulse_home()`'s `$HOME/.impulse` fallback, which a normal launch
(no `IMPULSE_HOME` set) resolved to and which has no relationship to the project. `IMPULSE_HOME` is
honored only when explicitly set and non-blank (and trimmed before use, since round 6: a padded env
value used to deny itself). An explicit `impulse_dir` override is no longer routed through the
shared `ToolRegistry::execute` → `validate_paths` → `ctx.allowed_read_roots` check (declaring it
`ParamType::FilePath`, round 1's approach, forced widening those SHARED roots to cover an
out-of-repo `IMPULSE_HOME`, which would have also authorized `file_read`/`document_read` to reach
it). Both tools declare `impulse_dir` as `ParamType::String` and call a shared
`tooling::builtin::resolve_and_validate_memory_dir` helper themselves. **Round 6 fix:** round 5's
check was `{ctx.impulse_dir, IMPULSE_HOME}` — but `sandbox_tool_context` also SETS `ctx.impulse_dir`
to `IMPULSE_HOME` when it's configured, so with `IMPULSE_HOME` set the "closed set" collapsed onto
one value and an explicit project `.impulse` override was wrongly denied. `ToolContext` gained a
`project_impulse_dir: PathBuf` field, set independently of `impulse_dir` by `sandbox_tool_context`
to `repo_root.join(".impulse")` (mirrors `impulse_dir`'s own default for every other constructor, so
non-`ion_repl` callers see no change); the check is now `{ctx.impulse_dir, ctx.project_impulse_dir,
trimmed(IMPULSE_HOME)}` — three independent members that never collapse. An `/allow` grant still
does NOT extend this tool-scoped reach (unchanged from round 5). `history::impulse_home()` itself is
untouched, still governing `.impulse/ion_history` only.

**`memory_search` is read-only for real now (review round 5, MEDIUM/Cursor; hardened in review round
6, MEDIUM REFUTED):** it used to call `retrieval::search_history`/`search_genome`, which open
`RetrievalStore` write-capable (`create_dir_all` + `Connection::open`, which creates `retrieval.db`,
plus WAL pragma writes that create `-wal`/`-shm` sidecars) — so pointing this ungated tool at any
`/allow`-granted directory with no existing index could CREATE real files there. It checks for
`retrieval.db`'s existence itself (reporting a typed "No retrieval index found" when absent, no side
effects) and, when present, opens via `RetrievalStore::open_read_only`. **Round 6 fix:** a plain
`SQLITE_OPEN_READ_ONLY` open of the WAL-mode index still needed to create/touch `-shm`/`-wal`
sidecars for reader coordination (persisting after `Drop`) and failed outright in a
permission-restricted directory; `open_read_only` now opens via the SQLite URI form
`file:<absolute-path>?immutable=1` (`SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI`), which tells SQLite no
connection will ever modify the file, skipping WAL reader-coordination and touching the directory
not at all. `execute()` now separates `results` from `errors` (a scope's query failure — e.g. a
corrupted index — lands in `errors`, rendered with the full `.context()` chain, and is never counted
in `results`/`count`) and reports `mode_applied: "keyword"` so a `semantic` request can't look like
it ran. Still a disclosed, deliberate narrowing to keyword-only search for this tool specifically
(semantic/vector mode needs the write-capable path's optional `sqlite-vec` extension loading).

**`genome_read` pages now (review round 5, P2/Codex) and validates its paging params for real
(review round 6, LOW):** it used to return the whole `GENOME.md` (or whole matched section)
unbounded, risking `LoopTrip::ContextBudget` on a large genome. `max_chars` (default 12,000, capped
at 32,000) and `offset` window the content exactly the way `document_read`'s own `window()` does
(same field names: `content`/`returned_chars`/`truncated`/`next_offset`), reimplemented locally
since `ion_repl::tool_document` is `office-support`-gated and `genome_read` is not. Round 6: the
doc comment claims parity with `document_read`'s `parse_request`, but the original parsing silently
clamped `max_chars: 0` up to 1 and silently fell back to defaults for a negative or non-integer
`max_chars`/`offset`; it now mirrors `parse_request` exactly, bailing with `ToolError::InvalidParams`
on all of those instead of guessing.

Internals otherwise unchanged: `genome_read` reads `<impulse_dir>/GENOME.md`; `memory_search`
queries `RetrievalStore`'s own `search_history_keyword`/`search_genome_keyword` directly.
- **Source of truth:** `src/ion_repl/registry.rs`, `src/ion_repl/mod.rs`
  (`ReplContext::sandbox_tool_context`), `src/tooling/builtin/mod.rs`
  (`resolve_and_validate_memory_dir`), `src/tooling/builtin/{memory_search,genome_read}.rs`,
  `src/retrieval/store.rs` (`RetrievalStore::open_read_only`).

### agent registry — `[code]`
The catalog of platform identity and launch metadata. It answers what can be named, detected, and
launched; daemon/runtime telemetry separately answers what is currently running.
- **Invariant:** ids and aliases have one owner, identity-collision registration fails
  transactionally, and explicit command overrides remain observable.

### terminal runtime — `[code]`
The PTY/process lifecycle boundary for spawn, input, resize, focus, exit, and cleanup. Terminal
mechanics publish state; they do not own durable project truth.
- **Source of truth:** `impulse-term/src/backend.rs` and `impulse-desktop/src/runtime.rs`.

### Dioxus cockpit — `[code]`
The operator-facing desktop composition of terminals, agents, context, artifacts, and controls.
It consumes daemon/runtime state through typed host commands/events and must not become a second
policy or persistence authority.
- **Source of truth:** `impulse-desktop/src/{desktop_host,host_bridge,host_commands,views}.rs`.

### Dioxus host invoke wire — `[code]`
The id-correlated request/response channel between the cockpit JavaScript and Rust host. Requests
use `kind: host_invoke`; every normal or rejected Rust response must use
`kind: host_invoke_result` so the strict JavaScript router can settle the matching promise. Host
events remain a separate `kind: host_event` stream. A missing discriminator is a protocol failure,
not a browser fallback opportunity.
- **Source of truth:** `impulse-desktop/src/host_bridge.rs`.

### tool capability — `[code]`
A deny-by-default permission required by a typed tool. Tool availability varies by runtime bridge;
conceptual parity does not imply identical enforcement.
- **Source of truth:** `src/tooling/{traits,registry,executor}.rs`, `src/mcp/`, and desktop MCP.

### supervisor policy — `[code]`
The concrete permission and confirmation policy for supervisor actions such as monitoring, memory
search, focus, input, context operations, and permission changes. It is the first role-specific
policy, not a generalized role system.
- **Source of truth:** `SupervisorPermissionPolicy`/`SupervisorPermissionState` in
  `impulse-ops/src/lib.rs` and enforcement in `src/daemon/handlers.rs`.

### governed producer profile — `[code]`
`rust_workspace_v1` combines env-routed/typed claim intent, fixed detached Rust verification, and
criteria-bound, history/tool-free API Supervisor review. It runs host-trusted code, not a sandbox;
receipts/task locks do not close crash-before-receipt. External harness review fails closed.

### operator capability — `[code]`
A per-daemon-run secret (32 random bytes, hex) written mode 0600 beside the socket while the daemon
listens and removed on shutdown. A connection is `non_operator` until it presents the capability
from a peer uid matching the daemon's own; only an operator-class connection may record an operator
decision, or mark a profiled task's launch lifecycle. Governed panes never receive it — every
inherited `IMPULSE_*` key is scrubbed before a pane spawns — which is what separates the operator
surface from a launched runtime holding `IMPULSE_SOCKET_PATH`. It is a structural boundary, not
protection against a same-uid process that deliberately reads the file.
- **Source of truth:** `src/daemon/actor_provenance.rs`, `src/daemon/mod.rs`, ADR-0018.

### memory / genome — `[code]`
Scoped durable continuity: session history plus verified decisions/preferences, retrieval indexes,
and review-first context injection. Memory records must carry project/session provenance.
- **Boundary:** pending accepted-run candidates are review state, not curated memory. Curated
  memory now has two independent artifacts: hand-written `GENOME.md` and the promoted-record log
  `MEMORY.jsonl` behind its projection.
- **Source of truth:** `src/{state,memory,retrieval,injection,stewardship}/`.

### artifact — `[code]`
A typed, reviewable output with project/agent/session provenance, status, view hints, and permitted
actions. Artifacts keep worker claims separate from evidence and operator decisions.
- **Source of truth:** `ArtifactEnvelope` and artifact IPC in `impulse-ops/src/lib.rs`.

### verification gate — `[code]`
Evidence that claimed work holds before a session/task is accepted. Governed profiled verification
is daemon-observed against the claimed commit in a symlink-free detached source tree with a
committed regular root `Cargo.lock`. It persists fixed argv plus digests, never raw output;
session-end and Ion verification remain separate contracts.

### governed actor — `[code]`
A typed provenance claim (`system`, `worker`, `verifier`, `supervisor`, or `operator`) checked by the
task transition machine. It is not cryptographic same-user authentication; local processes that can
reach the user-restricted daemon socket remain inside the current trust boundary.

### voice engine / ElevenLabs Agent bridge — `[code]`
ElevenLabs Conversational Agent is the **primary** voice backend. Implemented **MCP-style in Rust**:
`VoiceServer` holds `Arc<ToolRegistry>` + `ToolContext` (like `McpServer`), exposes JSON-line
`tools/list` + `tools/call` (stdio/TCP) and HTTP `POST /voice/tools` for server tools, exports
client-tool schemas from the live registry, then executes through `VoiceToolBridge` → real
`ToolRegistry::execute` with deny-by-default for mutating capabilities.
- **Source of truth:** `impulse-rs/src/voice/` (`server`, `adapter`, `schema`, `policy`, `envelope`,
  `webhook`, `provider`); CLI `impulse-rs voice serve|schema|tool-call`; docs
  `docs/voice-elevenlabs-tool-bridge.md`.
- **Boundary:** live ElevenLabs WebSocket session I/O and dashboard agent provisioning are optional;
  core contract is fixture-tested without network.

---

## Live-versus-direction boundary (2026-07-15)

- **Live foundation:** PTY/workbench truth, managed turns, registry-backed platforms/Ion, shared
  services, profiled Builder launch, routed claims, detached verification, strict API review,
  operator acceptance, and repairable pending candidates with no `GENOME`/`HISTORY` mutation.
- **Next:** stronger same-user actor authorization and one full launched Builder/Supervisor proof.
- **Later:** explicit candidate promotion/dismissal, reassignment/resume, generalized
  roles/adapters/capabilities, multi-project routing, and typed cross-agent messaging.
