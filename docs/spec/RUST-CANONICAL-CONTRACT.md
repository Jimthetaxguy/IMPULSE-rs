---
title: Rust Canonical Product Contract
description: Authoritative product contract for Impulse based on impulse-rs
version: '1.3'
updated: 2026-02-24
type: specification
category: core
phase: all
status: active
audience: builders
tags: [contract, rust, canonical, roadmap]
authors:
  - name: James Pustorino
    role: Creator
    email: James.s.Pustorino@gmail.com
    github: jamespustorino
---

# Rust Canonical Product Contract

> **Canonical implementation:** `impulse-rs`
> **Contract policy:** If a document conflicts with this file, this file wins.

## 1) Product Purpose

Impulse is a terminal-native sidecar for AI coding agents that preserves session continuity across direct hook invocations and daemon/TUI workflows.

Core outcomes:
- Persistent project memory (`GENOME`, session history, active state)
- Cross-session continuity for Claude Code and OpenCode integrations
- Operationally safe session lifecycle with verification-before-completion gates
- Human-visible observability through TUI and CLI artifacts

## 2) Canonical Scope and Roadmap

### Roadmap Contract

| Stage | Focus | Status |
| --- | --- | --- |
| **Now** | Rust memory core + hooks + TUI + verification | Active |
| **Next** | Semantic retrieval foundation + review-first context injection (feature-flagged, additive) | Active |
| **Later** | Advanced coordination UX (timeline, orchestration depth, agent dashboarding) | Planned |

### Out of Scope for Current Contract
- Full SWARM semantic injection runtime
- Large architecture rewrites unrelated to contract correctness
- Multi-week UX redesigns before core reliability goals

## 3) Public Interface Contract

### CLI Contract (Stable)

Primary commands that must remain documented and regression-tested:

**Session lifecycle:**
- `session-start`
- `session-end --verify`
- `track-write`
- `track-tool`

**Info and status:**
- `status`
- `history`
- `genome`
- `activity`
- `summary`
- `health`
- `system`
- `analyze`

**Retrieval and search:**
- `index-memory`
- `search-history`
- `search-genome`
- `retrieval-status`

**Orchestration and context:**
- `orchestrate`
- `handoff`
- `sync-context`
- `hooks`
- `verify`

**Stewardship:**
- `steward` (subcommands: status, analyze, compact, approve, reject)

**Tool and model management:**
- `tools` (subcommands: list, init, update)
- `docs` (subcommands: list, fetch)
- `model`
- `credentials` (subcommands: set, get, list, proxy)

**Utilities:**
- `calc`
- `exec`
- `run` (TUI mode)
- `config`

### State and Artifact Contract

| Path | Purpose | Notes |
| --- | --- | --- |
| `.impulse/LIVE_STATE.json` | Active sessions/files/tools | Ephemeral runtime state |
| `.impulse/HISTORY.jsonl` | Session history (append-only) | Durable project memory |
| `.impulse/GENOME.md` | Durable decisions/preferences | Durable project memory |
| `.impulse/config.json` | Runtime configuration | Durable config |
| `.impulse/context/current-task.md` | Shared current context | Generated via `sync-context` |
| `.impulse/context/handoff-*.md` | Tool handoff artifacts | Generated via `handoff` |
| `.impulse/context/routing-log.jsonl` | Orchestration log | Append-only audit |
| `.impulse/context/injections/injection-log.jsonl` | Injection audit log | Append-only staged/apply metadata |
| `.impulse/context/injections/inject-*.md` | Staged injection bundles | Review/apply artifacts |
| `.impulse/retrieval.db` | Retrieval cache/index database | Rebuildable cache (gitignored) |
| `.impulse/retrieval_index_state.json` | Retrieval index metadata/state | Rebuildable metadata |
| `.impulse/embeddings/*` | Optional embedding temp artifacts | Runtime cache (gitignored) |
| `.impulse/retrieval.lock` | Indexing lock guard | Runtime safety artifact |

### Retrieval Command Extensions (Additive)

- `search-history` / `search-genome`:
  - `--backend auto|sqlite-vec|rust-cosine|keyword`
  - `--explain`
- `retrieval-status`:
  - `--check`
  - `--json`
- Context injection overrides (additive):
  - `--daemon chat --inject-mode off|review|apply --inject-explain`
  - `orchestrate --inject-mode off|review|apply --inject-explain`
  - `handoff --inject-mode off|review|apply --inject-explain`
  - `sync-context --inject-mode off|review|apply --inject-explain`

### Context Injection Config Contract (Additive)

- `context_injection_mode`: `off|review|apply` (default `review`)
- `context_injection_scope`: `daemon|direct|both` (default `both`)
- `context_injection_max_items`: integer (default `5`)
- `context_injection_max_chars`: integer (default `2000`)
- `context_injection_min_score`: float `0.0..1.0` (default `0.60`)
- `context_injection_use_semantic`: bool (default `true`)
- `context_injection_emit_artifacts`: bool (default `true`)

## 4) Capability Matrix

| Capability | Status | Interface | Tests |
| --- | --- | --- | --- |
| Session lifecycle tracking | Implemented | `session-start`, `session-end` | Rust unit + integration |
| File/tool activity tracking | Implemented | `track-write`, `track-tool` | Rust unit + integration |
| TUI tabs (dashboard/session/history/etc.) | Implemented | `run` TUI mode | Rust UI tests |
| Daemon socket operations | Implemented | `daemon`, `--daemon ...` | Daemon tests |
| Context-aware chat (daemon) | Implemented | `--daemon chat` | Daemon + provider tests |
| Hook config generation | Implemented | `hooks --platform ...` | Integration tests |
| Orchestration handoff/context files | Implemented | `orchestrate`, `handoff`, `sync-context` | Rust tests |
| Verification gate | Implemented | `verify`, `session-end --verify` | Rust tests |
| Retrieval indexing + keyword search | Implemented | `index-memory`, `search-history --mode keyword`, `search-genome --mode keyword` | Rust unit + integration |
| Semantic search (feature-flagged) | Implemented (fallback-safe) | `search-* --mode semantic` with keyword fallback | Rust unit + integration |
| Retrieval health diagnostics | Implemented | `retrieval-status --check --json` | Rust integration |
| Retrieval explainability metadata | Implemented | `search-* --explain` + JSON metadata (`backend_used`, `fallback_code`, `timing_ms`) | Rust integration |
| Review-first context injection | Implemented (additive) | daemon chat + `orchestrate`/`handoff`/`sync-context` with `--inject-mode` | Rust unit + integration |
| Injection staging artifacts | Implemented | `.impulse/context/injections/*` | Rust unit + integration |
| Context stewardship | Implemented | `steward` (status/analyze/compact/approve/reject) | Rust unit + integration |
| Token tracking algorithm | Implemented | Internal metrics for compaction measurement | Rust unit + integration |
| Tool management | Implemented | `tools` (list/init/update) | Rust unit |
| Credential management | Implemented | `credentials` (set/get/list/proxy) | Rust unit |
| Documentation fetcher | Implemented | `docs` (list/fetch) | Rust unit |
| System utilities | Implemented | `calc`, `exec`, `system`, `health` | Rust unit |
| SWARM semantic coordination runtime | Planned | Future orchestration engine | Not started |

## 5) Claude/OpenCode Parity Contract

| Area | Claude Code | OpenCode | Contract Expectation |
| --- | --- | --- | --- |
| Session lifecycle hooks | Generated config | Generated config | Equivalent coverage |
| File write tracking | Supported | Supported | Equivalent behavior |
| Tool tracking | Supported | Supported | Equivalent behavior |
| Session end verification (`--verify`) | Included in generated hook command | Included in generated hook command | Required |
| Context handoff artifacts | Shared `.impulse/context/*` | Shared `.impulse/context/*` | Required |
| Known deltas | Hook event payload shape differs by platform | Hook event payload shape differs by platform | Handle by adapter mapping, not by feature removal |

## 6) Governance and Ownership

### Contract Ownership

The following files define product truth and must be updated together for contract changes:
- `docs/spec/RUST-CANONICAL-CONTRACT.md` (authoritative contract)
- `AGENTS.md` (operator-facing guidance)
- `CLAUDE.md` (project technical context)
- `docs/INDEX.md` (navigation + source-of-truth routing)
- `docs/SUMMARY.md` (high-level map)

### Required Update Checklist for Any Interface Change

When adding/changing CLI commands, hooks, state files, or roadmap stage definitions:
1. Update this contract doc.
2. Update command/state references in `AGENTS.md` and `CLAUDE.md`.
3. Update `docs/INDEX.md` and `docs/SUMMARY.md` links/status.
4. Run `python3 docs/validate_docs.py --contract`.
5. Include release note fields from `docs/guides/RELEASE-NOTES-TEMPLATE.md`.

## 7) Validation and Drift Prevention

Documentation contract validation command:

```bash
python3 docs/validate_docs.py --contract
```

This command must fail on:
- Missing canonical references in source-of-truth docs
- Contradictory active-doc claims (for example, active docs claiming TypeScript/Bun-only core)
- Missing roadmap contract markers in key top-level docs
