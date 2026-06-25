# CONTEXT — Impulse Ubiquitous Language

> **Read this first.** Shared vocabulary for Impulse. Every Claude/Codex/Cursor session should
> read this before touching the codebase so we all mean the same thing by the same word. It is a
> **living document** — when a term's meaning shifts in the code, update the definition here in
> the same change. (Per the `project-context-glossary` canonical rule, 2026-06-20.)
>
> Each entry is tagged **`[code]`** (a real construct; the cited file is the source of truth) or
> **`[vocabulary]`** (how we *talk* about Impulse; "Closest in code" names what carries it today).
>
> Cross-agent contract: `AGENTS.md`. Plan: `PLAN.md`. Handbook: `HANDBOOK.md`.

---

## What Impulse is

Impulse is a **terminal-native memory sidecar for AI coding agents** (Claude Code, Codex, Cursor and similar CLI-TUI agents). The Impulse Agent acts as an always-on tech lead that manages and augments other agents. Agents primarily operate in the terminal and "login" (connect/attach) via supported CLI-TUI entrypoints; Impulse can help manage/monitor them, augment their context, and provide extra tools. The (light) UI allows picking and cycling between multiple project folders/workspaces in one place so you can work across one or many project spaces + one or many agents per space without switching interfaces. Agents wired to Impulse gain Rust type-safe built-in plugins/extensions for tools and capabilities. A further goal is reducing agent machine load via subagents and workflows.

It preserves session continuity *across* tools and conversations: it tracks what you touched,
remembers what you decided (the **genome**), and injects relevant past context into new sessions
— review-first, never silently. It is **not** an LLM inference engine; the models live in the
coding agents. Impulse is the durable memory + retrieval + ops layer around them.

Workspace: `impulse-rs/` (Cargo workspace). Crates: `impulse-desktop`, `impulse-gui`,
`impulse-ops`, `impulse-term`. Persistence is local SQLite (FTS5). Edition 2021, rust 1.82+.

---

## Glossary

### session — `[code]`
A bounded unit of agent work with a start, tracked activity, and a verified end. Created with a
name + platform; ended with a summary behind a **verification gate**.
- **Closest in code:** `WorkbenchDaemonRequest::{CreateSession, EndSession}` in
  `impulse-rs/impulse-ops/src/lib.rs`; CLI `session-start` / `session-end --verify`.

### genome — `[vocabulary]`
The project's durable memory: decisions + preferences that survive across sessions (as opposed
to per-session history). The "what we already decided" store that prevents re-litigating.
- **Closest in code:** the project genome store behind retrieval/injection (SQLite).

### context injection — `[vocabulary]`
Surfacing relevant past context into a new session, **review-first** (the operator sees what will
be injected before it lands). The mechanism that ends "re-explain your architecture every time."

### tracking — `[code]`
Recording files touched, tools used, and decisions made during a session — normally driven by
**platform hooks**, not by hand.
- **Closest in code:** `WorkbenchDaemonRequest::TrackFile`; CLI `track-write`.

### platform hook — `[code]`
An auto-generated integration that lets a coding agent (Claude Code / Codex; legacy OpenCode) feed
activity to Impulse automatically. OpenCode is legacy-compat only, not a peer active platform.

### daemon / workbench — `[code]`
The long-running process that owns session state and serves the GUI/operator workbench over a
versioned protocol. The single coordination point for ops.
- **Source of truth:** `impulse-rs/impulse-ops/src/lib.rs` (`DAEMON_PROTOCOL_VERSION`,
  `WorkbenchDaemonRequest` / response enums).

### retrieval — `[code]`
Finding past context: **FTS5 keyword search** + **semantic search** over session history and the
genome. The retrieval backend is the natural deep-module boundary (see `embedding provider`).

### embedding provider — `[vocabulary]` → *proposed boundary*
The swappable backend that turns text into vectors for semantic search. Today this is implicit;
the `interface-boundaries-as-control-plane` rule says it should be an explicit trait so the engine
(e.g. local Ollama `nomic-embed-text`, or a future in-process Rust embedder) can change without
touching retrieval internals. **Scaffold:** `_working-files/20260620-054745-claude-impulse-embedding-provider-boundary.md`.

### context stewardship — `[vocabulary]`
Monitoring context-window usage and proposing cleanup strategies before the window blows out.
Impulse's answer to the playbooks' "Headroom" pattern.

### agent registry — `[code]`
The record of which agents/tools are active and what they're working on (multi-agent awareness).
- **Source of truth:** `impulse-rs/impulse-ops/src/agent_registry.rs`.

### workspace (project space) — `[code]`
A distinct project folder/root that can be registered so the desktop host (and MCP/tools) can surface it for cycling between multiple project spaces. Multiple agents (terminal CLI sessions) can attach to workspaces.
- **Closest in code:** `WorkspaceRegistry` + `WorkspaceTarget`/`WorkspaceEntry` in `impulse-desktop/src/workspace.rs` (host) and related registration in the Dioxus desktop + MCP surface. (Pure contracts/workspace types from the duplicate .clean tree archived under active/archive/_archived-... for reference.)

### terminal agent (CLI-TUI login) — `[vocabulary]`
A coding agent (claude-code, codex, cursor-agent, etc.) running in a PTY/terminal pane that is "logged in" (spawned/attached/monitored) under Impulse supervision. Impulse augments via context, tools (type-safe Rust), and coordination.
- **Closest in code:** `AgentDescriptor`/`AgentRegistry` (impulse-ops), PTY via impulse-term, spawn via desktop runtime/MCP `impulse.agent_spawn`, orchestration.

### verification gate — `[code]`
The check that a session's claimed work actually holds before the session can close
(`session-end --verify`). The local instance of the canonical `verification-before-completion` rule.

---

## Current state (update at session end)

- **2026-06-20:** CONTEXT.md created (keystone `project-context-glossary` artifact). Active branch at
  creation: `agent/codex-dioxus-host-goal-cleanup` (concurrent Codex work — file is additive/untracked).
- **2026-06-25:** Reconciliation of duplicate codebases complete. The IMPULSE-rs.clean sibling checkout was relocated (mv) into the active tree's archive/ (_archived-IMPULSE-rs.clean-... and _archived-...-source); original path removed from active location (git worktree prune + ls confirm only canonical active checkout remains, prunable entry cleaned). Full contents of the duplicate (clean/crates/ with contracts + workspace pure types, etc.) archived inside canonical tree. Centralized pure AgentPlatformsReport + resolve_launch_command in impulse-ops drive status (text/json via envelope), List*Tools, and spawn. --format supported. Committed archive proof test. Vision language embedded. cargo run -- status (and --format json) output the indicators (claude-code, codex, multi-workspace).
- Dioxus host scaffold + bridge parity + cleanup: smoke captured (ok, live status asserted in sim + real dispatch exercised in unit tests), status consistent, gate 1363 passed 0 failed. Ratchet: err_to_string helper + extended body() + dispatch_host_invoke tests (array/scalar/malformed, agent_write error path, workspaces/mcp). All to correct SCRATCH (bd088a7e3032). Changes committed post-verif.
- Dioxus Desktop launch scaffold + live terminal bridge parity: complete (smoke asserts "dioxus-eval-bridge-ready" + exercised without pending; real dispatch_host_invoke + body tests cover claude/codex; registry_for_runtime centralized with proper error policy, no silent fallbacks in mcp/runtime; MCP List* execute test added; ROADMAP marked complete; capture script + 3 atomic commits landed).
- Open architecture thread: Dioxus host migration (`impulse-interface-dioxus-roadmap-spec`).
- Proposed next boundary: explicit `EmbeddingProvider` trait for semantic search (scaffold in `_working-files/`).

## Goal completion note (2026-06-25)
Reconciled duplicate codebases per goal (AC1). Only one canonical active tree (IMPULSE-rs checkout); .clean relocated/archived under active/archive/ (no parallel maintenance). Vision (tech lead, terminal logins, multi project spaces/agents cycling, Rust type-safe augments/plugins, subagents for load reduction) codified in living docs. Registry-driven launch/monitoring (status, spawn, ListAgents/ListPlatforms). Full verif gate (build/test/clippy/fmt clean on relevant + full outputs saved) + pure cargo run -- status (text+json) captured with indicators. Direct unit tests (report, resolve, archive proof, multi reg/list) + evidence in SCRATCH. (Untracked: CONTEXT.md, decisions, work card — per lane.)

Dioxus Desktop launch scaffold + terminal bridge parity complete (AC1): desktop-app feature check/build passes, dioxus:host:smoke x N runs exit 0 with "dioxus host readiness smoke ok", xterm assets/globals, live status "dioxus-eval-bridge-ready" exercised (post-pending sim), host commands (snapshot/write/resize/workspace etc) flow without pending rejection on exercised path. Core CLI status (text/json) consistent with claude-code/codex + multi-workspace indicators (AC2). Cleanup/optimize via subagent + autoresearch ratchet loop: extracted err_to_string helper (dedup 18 closures in host_commands), added body() boundary test in host_bridge (non-object err); all targeted tests + full gate (1363+ passed, 0 failed) re-verified. Living docs + plan checklist updated. All plan verif steps executed with durable SCRATCH proof. (AC3/4)
