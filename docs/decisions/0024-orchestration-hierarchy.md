---
title: "ADR-0024: Orchestration Hierarchy (Super Orchestrator, Workspace Orchestrators, Workers)"
description: A super orchestrator and per-workspace orchestrators delegate to tool-using workers; bounded reports go up, briefs go down, and the cockpit shows full output
status: draft
created: 2026-10-04
updated: 2026-10-04
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, orchestrator, context, blackboard, workspaces, delegation, harness]
---

# ADR-0024: Orchestration Hierarchy

## Status

Proposed on lane `claude/orchestration-hierarchy-adr-20261004`, stacked on the model-provider lane
(ADR-0022 stage 2) and the blackboard lane (ADR-0023). Nothing here is built. It records the
direction James set on 2026-10-04 so the remaining orchestrator work (ADR-0023 rule 7's
`delegate_task` and `approve_gate`, VISION's `impulse harness`) has one shape to build toward.

## Context

Today the operator is the orchestrator. The Dioxus cockpit launches governed Builder panes (ADR-0010,
ADR-0019), Ion runs one conversation that holds every tool result it ever saw, and photon (ADR-0022)
is the only subagent: disposable, read-only, and called by Ion's model.

That shape has three costs:

- **Context tax.** Every tool result lands in one conversation. ADR-0023's blackboard moves large
  results out and hands back a reference, but the conversation that asked for the work is still the
  one that reads the work.
- **No continuity per workspace.** Ion's history ends with its REPL. Sessions and governed tasks are
  recorded per workspace, but the reasoning that connected them is not, so each new conversation
  starts from the files and `GENOME.md`.
- **No place for the big picture.** Cross-workspace priorities and decisions live in the operator's
  head and in chat logs that compaction eventually discards.

James's direction: a main orchestrator that is a long-running, focused conversation whose job is
mostly reporting to the user; subagent results that are packaged and shown to the user
programmatically without entering that conversation, which only receives a summary; workspace
orchestrators that keep an ongoing thread per workspace, seeded from the main one; and tool calls
that live with subagents, so the orchestrators' own tools are mostly about orchestrating other
agents.

## Decision

1. **Three levels, each with one job.**
   - The **super orchestrator** is long-running and holds goals, priorities, and decisions that cross
     workspaces. It talks to the user and delegates to workspace orchestrators. It has no file,
     shell, or network tools.
   - A **workspace orchestrator** owns one workspace's plan and state for as long as the workspace
     is in use. It delegates to workers and keeps the workspace's thread. It also has no file, shell,
     or network tools; it reads the workspace through workers such as photon.
   - **Workers** do the work with real tools under the existing controls: Ion, Claude Code, or Codex
     panes as governed Builders (claim, verify, review, promote; ADR-0011 to ADR-0019), and photon for
     read-only questions. A worker is disposable; its record is not.

2. **Orchestrator tools are orchestration tools.** Both orchestrator levels use the five-name surface
   ADR-0023 fixed: `search_tools`, `blackboard_store`, `blackboard_fetch`, `delegate_task`,
   `approve_gate`. `delegate_task` creates or resumes work at the level below (a workspace
   orchestrator, a governed task, or a photon run) and returns at once with a handle; results come
   back as reports (rule 3), not as tool output.

3. **Only a report travels up.** A level boundary is crossed upward by one typed `AgentReport`:
   status, a bounded summary, decisions made, open questions, approvals needed, and references to
   full output (`EntryRef`s into the blackboard, artifact ids, governed task ids). Raw output never
   crosses. The parent sees the report; it pages full output in with `blackboard_fetch` only when it
   needs to. The worker writes its report against the schema and the harness validates it, the same
   division ADR-0011 uses for claims.

4. **Only a brief travels down.** Delegation carries a `Brief`: goal, constraints, the decisions
   that bind the work, acceptance criteria, and a budget. A parent's transcript is never inherited,
   which is also what keeps a hostile document read by one worker from steering its siblings.

5. **The user sees full output in the cockpit, not through an orchestrator.** Reports and the
   blackboard entries and artifacts they reference are rendered by the Dioxus cockpit (and the TUI)
   from the daemon's artifact store (`impulse_ops::ArtifactEnvelope`), next to the conversation. An
   orchestrator narrates and decides; it does not restate a worker's output to show it.

6. **Each workspace thread is durable and summarized.** A workspace orchestrator's conversation,
   its compaction checkpoints, and a living workspace brief (current state, open work, decisions,
   next steps) are stored under that workspace's `.impulse/`, daemon-owned like governed tasks. The
   super orchestrator reads workspace briefs and reports, never workspace transcripts. Each level
   compacts on its own schedule, and what survives compaction is the brief plus the decision log
   (`GENOME.md` and the ADR-0020 memory candidates), not chat text.

7. **Approval belongs to the operator at every level.** `approve_gate` raises a request to the user
   from wherever it starts. No orchestrator approves on the operator's behalf; acceptance and
   promotion stay operator-class actions (ADR-0018, ADR-0019), so adding levels adds no new
   authority.

8. **Governance stays where the work is.** Verified work remains a governed task. Orchestrators
   create, track, and report on governed tasks through the daemon; they never claim, verify, or
   accept themselves, and the producers that do are unchanged.

9. **Each level runs on its own endpoint role.** The super and workspace orchestrators use ADR-0022's
   `orchestrator` role (or a per-level role added when they diverge); workers keep `ion` and `photon`.
   A cheap or local model can run orchestration while a stronger one does the work, or the reverse.

## Consequences

- An orchestrator's context grows with decisions and reports, not with tool output, so it can run
  for a long time and compact rarely.
- Workspaces keep continuity across sessions and restarts; switching workspace resumes its thread
  instead of starting from the files.
- Several workspaces can make progress in parallel under one super orchestrator.
- Summaries lose detail. The references in every report, and paging with `blackboard_fetch`, are the
  mitigation; the cockpit shows the full output either way.
- Delegation adds latency and moving parts: a durable thread store, report validation, and a
  daemon-side scheduler for long-running orchestrators.
- Worker failures stay contained: a crashed worker produces a failed report (or none, which the
  parent sees as a stalled handle) rather than taking an orchestrator's context with it.

## Order of work

Expressed as dependencies; nothing here is scheduled.

1. `AgentReport` and `Brief` types in `impulse-ops`, and cockpit rendering of reports with their
   blackboard entries. Depends on the blackboard (ADR-0023) merging.
2. `delegate_task` over photon runs and governed tasks. Depends on 1.
3. Durable workspace threads and the workspace brief. Depends on 1; can proceed alongside 2.
4. `approve_gate`, using the ADR-0018 operator class. Depends on 2.
5. Workspace orchestrators, then the super orchestrator as `impulse harness`, on the `orchestrator`
   endpoint role. Depends on 2, 3, and 4.

## Open questions

- Where long-running orchestrators live: a daemon-owned process the cockpit and TUI attach to
  (recommended, matching how governed tasks are owned) or a cockpit pane.
- The report's size bound and whether the summary is the worker's own or produced by a separate
  summarizer step (recommended: the worker's own, schema-checked, as with claims).
- How a workspace brief is kept honest over time: rewritten by the workspace orchestrator at each
  compaction, or derived from the decision log and governed task records.

## Alternatives considered

- **One agent with everything (status quo plus a bigger context window).** Rejected: the context tax
  grows with every tool call, and compaction discards exactly the decisions that need to persist.
- **A flat pool of peer agents with shared memory.** Rejected for now: without a level that owns the
  big picture, priorities and approvals have no home, and shared mutable memory among peers is hard
  to govern.
- **Orchestrators that read full worker output.** Rejected: it re-imports the context tax the
  blackboard removed; references plus on-demand paging give the same access at a fraction of the
  cost.

## Related

- ADR-0011 to ADR-0019: governed tasks, producers, operator class, staged worktrees.
- ADR-0020: memory promotion (the decision log a long-running orchestrator relies on).
- ADR-0022: typed model endpoints and the `orchestrator` role.
- ADR-0023: the blackboard, demand paging, and the five-tool orchestrator surface.
