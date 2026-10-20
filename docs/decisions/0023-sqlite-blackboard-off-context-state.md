---
title: "ADR-0023: SQLite Blackboard for Off-Context Agent State"
description: Large agent results go to a durable SQLite blackboard and the model receives a reference it can page through, separating the wire layer, the blackboard, and the execution engine
status: review
created: 2026-10-03
updated: 2026-10-03
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, ion, context, sqlite, blackboard, tools, governed-tasks]
---

# ADR-0023: SQLite Blackboard for Off-Context Agent State

## Status

Proposed and implemented on lane `claude/ion-blackboard-20261003`, stacked on
`claude/ion-scout-subagent-20261002` (ADR-0022). Accepted on merge. The orchestrator role that
completes the five-tool surface (rule 7) is not built.

## Context

Every byte a tool returns to Ion becomes part of the conversation. A `bash_exec` that prints a test
log, a `file_read` of a generated file, or a `memory_search` with many hits goes into the next
request whole, and stays in history until ADR-0017's context-budget compaction replaces it with a
stub. Compaction keeps the window bounded, but it discards: once a result is compacted, the model
cannot get it back.

Governed claims have the opposite problem. `GovernedClaimRequest::validate` caps a claim summary at
`MAX_PROFILED_CLAIM_SUMMARY_BYTES` (4 KiB) and refuses anything larger, so a Builder with a long
account of its work has to cut it down before the daemon will record it.

Both are the same missing piece. Agents have nowhere to put a large result except in context, and
nowhere durable that another agent, or the same agent after a restart, can read it from. Impulse's
existing stores do not fit:

- `LIVE_STATE.json` and the other session files are JSON documents rewritten whole, owned by one
  process, and sized for small state.
- `retrieval.db` is SQLite, but it is a search index the project treats as rebuildable. Durable
  agent output does not belong in a file whose contract allows deleting it.
- `GOVERNED_TASKS.json` holds bounded, revisioned task records. It deliberately keeps large text
  out.

So there is no session-state SQLite database to share, and the blackboard gets its own file.

## Decision

### Three layers

```
  model / client
        ▲  small text only: previews, references, one page at a time
        │
 ┌──────┴───────────────────────────────────────────────┐
 │ wire layer      Ion chat loop (tool_result assembly), │
 │                 daemon socket, MCP server             │
 └──────┬───────────────────────────────▲───────────────┘
        │ oversized result              │ page request
        ▼                               │
 ┌──────────────────────────────────────┴───────────────┐
 │ blackboard      .impulse/blackboard.db (SQLite, WAL)  │
 │                 durable, shared across processes      │
 └──────▲───────────────────────────────────────────────┘
        │ full result
 ┌──────┴───────────────────────────────────────────────┐
 │ execution engine   ReplTool::run, bridged tools,      │
 │                    photon, governed producers         │
 └──────────────────────────────────────────────────────┘
```

1. **The execution engine produces results and does not shape context.** Tools return whatever
   they return. None of them needs to know about the threshold.
2. **The wire layer decides what crosses into context.** It sends a result inline when it is small
   and a reference when it is not. This is the only place the spill policy lives
   (`ReplToolExecutor::finish_result` in `ion_repl::chat`).
3. **The blackboard holds everything else.** It is durable, it is shared by every process in the
   project, and nothing in it is cached in memory, so a crash leaves only what SQLite committed.

The rule for where state lives follows from this. Data that dies with the session (the
conversation, the untrusted-output flag, the photon spend counter) stays in the process, behind the
existing `Arc`/atomic/`Mutex` holders. Data another agent or a restarted process needs goes to
SQLite.

### Demand paging

A spilled result is replaced by a reference: tool name, size, `task_id`, content type, SHA-256, a
preview of at most 1 KiB (a quarter of the threshold when that is smaller), and the exact
`blackboard_fetch` call that reads the next window. The model then reads the parts it needs, one
window of at most 8,192 bytes per call (the PRD's read limit). A JSON entry can be narrowed first
with an RFC 6901 pointer, so the model can select `/results/3` instead of paging through everything
before it.

This is demand paging in the operating-system sense: the working set stays in context and the rest
is fetched when referenced. Compared with truncation or compaction, nothing is lost; the model pays
in context only for what it reads. Compared with summarizing, nothing passes through a lossy model
step; the bytes are the tool's bytes, and the hash lets a reader confirm it.

### Rules

1. **Store.** `.impulse/blackboard.db` in the directory Ion's memory tools already resolve
   (`IMPULSE_HOME` when explicitly set, else `<repo_root>/.impulse`; the daemon uses its own
   `.impulse`). One table:
   `blackboard(task_id TEXT PRIMARY KEY, payload BLOB, content_type TEXT, created_at INTEGER,
   ttl_seconds INTEGER NULL, metadata TEXT)`. `metadata` is a JSON object. `PRAGMA user_version`
   records the schema version; a newer version is refused, not overwritten. A new file is mode
   0600 because tool output can contain anything a command printed.
2. **Insert-only for live keys.** Writing a key that holds a live row fails with `KeyExists`, so
   one agent cannot overwrite another's entry. An expired key may be reused. The check and the write
   are one `INSERT … ON CONFLICT DO UPDATE … WHERE expired` statement, so two processes cannot both
   win.
3. **Expired means absent.** Reads treat an expired row as missing whether or not it has been
   purged. Purging is disk hygiene, never correctness. Rows are purged when any process opens the
   database, when the daemon starts, and every `blackboard.purge_interval_secs` (default 300) after
   that.
4. **Spill.** A tool result larger than `blackboard.spill_threshold_bytes` (default 4,096; bounds
   1,024 to 1 MiB) is stored with `blackboard.spill_ttl_seconds` (default 7 days) and replaced by the
   reference. The lower bound keeps the reference, envelope included, smaller than what it replaces.
   `blackboard_fetch` and `blackboard_store` are exempt: a page is already bounded, spilling it would
   answer a page request with another reference, and a store acknowledgement is one line.
   `search_tools` is not exempt, because with schemas included its output grows with the catalog. A
   result above the 16 MiB row limit is stored truncated and the reference says so. If the store
   fails, the full output goes inline with the failure reason in front of it; a result is never
   dropped.
5. **The guard scan sees everything.** The `GuardTarget::ToolCall` scan that sets Ion's
   untrusted-output flag runs on the full result, not the preview, because the model can fetch any
   part of it. A `blackboard_fetch` scans the whole entry as well as the page, so text split across
   a page boundary, or written by another agent or an earlier session, is still seen. The reference
   is wrapped in the same nonce envelope as any tool result, and so is every page. Spilling, hashing,
   and the whole-entry read run on Tokio's blocking pool, so a busy database cannot stall the
   runtime thread or the tool loop's wall-clock timeout.
5a. **Compaction keeps the key.** When ADR-0017's context budget replaces a spill reference with a
   compaction stub, the stub keeps the entry's key (`ToolExecutor::compaction_note`), so the result
   is still reachable until its TTL expires. The key is re-validated against the key charset before
   it goes into the stub.
6. **Governed claims.** When a claim summary exceeds the smaller of the spill threshold and the
   daemon's 4 KiB claim limit, Ion stores the whole summary under `claim:<task>:<id>` with no TTL
   (it is evidence attached to a governed record), submits a preview that ends with the reference,
   and appends `blackboard:<key>` to `artifact_ids`. A claim the daemon used to refuse is now
   recorded. Four details keep this honest:
   - The summary goes into the **canonical project's** `.impulse`, derived from the daemon socket
     the claim is sent to (`<impulse>/sockets/<name>`), not into the session's own directory. A
     Builder in an ADR-0019 staged worktree therefore writes where the daemon looks, and Discard,
     which deletes the staged tree, does not delete the evidence.
   - The daemon's nonblank and NUL-free checks are applied to the whole summary before anything is
     stored, so spilling never admits a summary the daemon would have refused. Leading whitespace is
     dropped from the preview so it always carries text.
   - The entry is written after the daemon lookup and deleted again unless the daemon records the
     claim, so a refused, conflicting, or failed submission leaves nothing behind.
   - Supervisor review is tool-free and sees only the preview. Content past the preview is stored
     and referenced but not reviewed (see Known gaps).
7. **Tool surface.** Ion's default registry gains `blackboard_store`, `blackboard_fetch`, and
   `search_tools`. `search_tools` returns names and one-line descriptions, and input schemas only
   when asked. `ORCHESTRATOR_TOOL_SURFACE` fixes the orchestrator role's advertised tools at five:
   `search_tools`, `blackboard_store`, `blackboard_fetch`, `delegate_task`, `approve_gate`.
   Everything else an orchestrator uses is found through `search_tools`. A test keeps the list at
   five.
8. **Clean governed subjects.** `blackboard.db` and its `-wal`/`-shm` sidecars are exempt from the
   governed clean-subject check and are in the ignore list `impulse init` writes. Ion opens the
   database inside whatever worktree it runs in, including a staged Builder worktree, and without
   the exemption the first spill would make the tree the Builder is about to claim look dirty.
9. **Not gated.** `blackboard_store` writes only to the project's own blackboard, under a validated
   key, bounded size, and no overwrite of live rows. It stays outside the confirmation prompt like
   `file_read`, because a prompt on every store would defeat its purpose.

## Consequences

- Large tool results no longer flood Ion's context, and a spilled result stays readable after
  compaction removes its reference from history, until the spill TTL expires.
- Long governed claim summaries are recorded instead of refused.
- A restarted daemon resumes with the same blackboard. Its first maintenance pass purges rows that
  expired while it was down and logs how many live entries remain. The daemon never creates the
  file in a project that has not written one.
- Ion opens a connection per operation. That costs a file open per spill or fetch, and keeps no WAL
  reader pinned between turns.
- `config.json` gains a `blackboard` section. `Config` keeps it as raw JSON, so persisting any other
  key writes it back exactly as it was, and a typo in it cannot stop the rest of the configuration,
  or the daemon, from loading. Blackboard consumers parse and validate it
  (`BlackboardConfig::from_section`): unknown fields and out-of-range values are errors, reported by
  Ion at startup and by the daemon in its log, and both then use the defaults, because turning
  spilling off would let one large result flood the context.

### Known gaps

- **Supervisor sees the preview only.** Claim summaries are stored where the daemon can read them,
  but governed Supervisor review is deliberately tool-free (ADR-0012), so it judges the preview.
  Feeding the referenced text into review is a change to the review contract, not to the
  blackboard.
- **Spilled tool results in staged worktrees.** Ordinary spills still go to the session's own
  `.impulse`. In a staged worktree that is the staged tree's, so they disappear on Discard, which is
  acceptable for a seven-day scratch TTL but worth knowing.
- **The orchestrator is not built.** `delegate_task` and `approve_gate` are reserved names. Ion
  remains a full coding agent and still advertises every registered tool; advertising only the five
  and adding tools to a turn after `search_tools` finds them needs the tool loop to accept a tool
  list that changes between rounds, which is follow-up work with the orchestrator role.
- **No daemon IPC.** Agents share the blackboard through the file. An agent without filesystem
  access to the project, or the Dioxus cockpit, needs a protocol endpoint; that is a protocol
  version bump and a separate decision.
- **No per-agent ownership.** Rows carry no actor. Insert-only stops overwrites; it does not stop a
  reader in the same project from reading another agent's entry. That matches the rest of
  `.impulse`, which is same-user state.

## Alternatives Considered

- **Put the rows in `retrieval.db`.** One fewer file, but that database is a rebuildable index, and
  durable output would be lost the first time someone rebuilt it.
- **One file per result under `.impulse/`.** Simple to read by hand, but TTL, atomic insert-only
  writes, and cross-process access would each need their own code, and a governed subject would need
  an exemption pattern per file.
- **Truncate large results.** Bounded context, but the model can never see what was cut.
- **Rely on compaction alone.** Compaction runs after the large result has already been sent at
  least once, and what it discards is gone.
- **`sqlx` instead of `rusqlite`.** `rusqlite` is already a dependency with the bundled SQLite build,
  and every operation here is a short local call. An async driver would add a dependency without
  removing a blocking cost.
