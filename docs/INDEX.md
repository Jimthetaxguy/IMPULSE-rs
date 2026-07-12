---
title: Documentation Index
description: Master navigation hub for Impulse documentation
version: '1.4'
updated: 2026-07-12
type: doc
category: navigation
phase: all
status: active
audience: everyone
tags: [index, navigation, discovery]
last_updated: 2026-07-12
authors:
  - name: Impulse Maintainers
    role: Maintainer
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---

# Documentation Index — Impulse

> **Master navigation hub.** Find any document by category, phase, or topic.
> **Quick start:** Read the living product north star [`../VISION.md`](../VISION.md), then the
> current implementation contract [`spec/RUST-CANONICAL-CONTRACT.md`](spec/RUST-CANONICAL-CONTRACT.md).
>
> **Canonical stack: Rust (impulse-rs)**
> **Roadmap contract:** Now=control-plane foundations; Next=one governed supervisor/builder vertical slice + hierarchy/enforcement ADR; Later=general roles + negotiated runtimes; Legacy=egui compile-maintenance only.
> **Collaboration playbook:** [`guides/COLLABORATIVE-AGENTIC-CODING.md`](guides/COLLABORATIVE-AGENTIC-CODING.md)
> **Narrow validation register:** [`HONEST-ROADMAP.md`](HONEST-ROADMAP.md) preserves unresolved
> hook, compaction, extraction, and memory-quality risks from the legacy phase-era design. It is
> not the current whole-product roadmap or control-plane risk authority.

> **Prefer `kdb` for searching.** The knowledge database indexes canonical docs with FTS5 full-text search:
>
> ```bash
> memory-pipeline/kdb "hooks architecture"   # Search everything
> memory-pipeline/kdb --summary              # Project overview
> memory-pipeline/kdb --json "query"         # JSON for agents
> ```
>
> See [`memory-pipeline/README.md`](../memory-pipeline/README.md) for full command reference.

---

## By Category

### Specifications (Source of Truth)

| Document                                                      | Description                                                                | Phase |
| ------------------------------------------------------------- | -------------------------------------------------------------------------- | ----- |
| [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) | **Authoritative contract for current product behavior**                    | all   |
| [USER-STORY-MAP.md](spec/USER-STORY-MAP.md)                   | Rust-first product stories, acceptance criteria, and story status          | all   |
| [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md)             | Story-to-test map for the current Rust workspace                           | all   |
| [COMPETITIVE-POSITIONING.md](spec/COMPETITIVE-POSITIONING.md) | Market analysis and differentiation                                        | all   |
| [PERFORMANCE-TARGETS.md](spec/PERFORMANCE-TARGETS.md)         | Performance budgets and benchmarks                                         | 1-2   |

### Product North Star and Boundaries

| Document | Description |
| --- | --- |
| [VISION.md](../VISION.md) | **Living product north star:** control-plane promise, hierarchy, live-versus-target boundary, and complete vertical slice |
| [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) | Authoritative contract for current implemented behavior |
| [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) | Current code boundary matrix and enforcement truth |

Do not split separate role/runtime/supervisor schema documents until the hierarchy, adapter, and
enforcement-strength ADR decisions listed in `VISION.md` are resolved.

### Validation and Historical Risk

| Document | Scope |
| --- | --- |
| [HONEST-ROADMAP.md](HONEST-ROADMAP.md) | Historical hook/memory validation register; not current roadmap authority |
| [HOOK-VALIDATION-GUIDE.md](guides/HOOK-VALIDATION-GUIDE.md) | Current procedure for generating real Claude hook evidence |

### Architecture Decisions (ADRs)

| ADR | Title | Decision status | Current scope |
| --- | --- | --- | --- |
| [0001](decisions/0001-claude-code-primary.md) | Claude Code as primary agent | Accepted | Historical integration choice; external runtimes are now registry-backed |
| [0002](decisions/0002-file-first-memory.md) | File-first memory (no DB in Phase 1) | Accepted | Historical Phase 1 memory decision; not current persistence authority |
| [0003](decisions/0003-progressive-search.md) | Progressive search (FTS5 → vectors) | Accepted | Historical phase framing; current retrieval behavior comes from Rust code/contract |
| [0004](decisions/0004-extraction-strategy.md) | LLM extraction strategy | Accepted | Historical extraction design; retained for memory provenance |
| [0005](decisions/0005-distribution-model.md) | npm distribution model | Accepted | Historical distribution decision superseded in practice by the Rust workspace |
| [0006](decisions/0006-hook-enhancement-conflict-resolution.md) | Hook enhancement conflict resolution | Accepted | Narrow hook/guardrail policy scope |
| [0007](decisions/0007-desktop-shell-stack.md) | Desktop shell stack | Superseded | Superseded by ADR-0008 |
| [0008](decisions/0008-dioxus-desktop-host.md) | Dioxus Desktop host | Accepted | Current desktop host authority |
| [0009](decisions/0009-reconcile-impulse-copies.md) | Reconcile duplicate Impulse codebases | Accepted | Current canonical-tree decision |

See [decisions/README.md](decisions/README.md) for full decision log.

### Developer Guides

| Document                                                        | Topic                            |
| --------------------------------------------------------------- | -------------------------------- |
| [BEST-PRACTICES.md](guides/BEST-PRACTICES.md)                   | Coding conventions and patterns  |
| [COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md) | Agent lanes, worktrees, handoffs, and verification |
| [CONTRIBUTING.md](../CONTRIBUTING.md)                           | Contribution rules for humans and agents |
| [TESTING-FRAMEWORK.md](guides/TESTING-FRAMEWORK.md)             | Legacy TypeScript-era testing guide |
| [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md)               | Current Rust-first test baseline and gap map |
| [SYNTHETIC-TESTING-GUIDE.md](guides/SYNTHETIC-TESTING-GUIDE.md) | Synthetic test generation        |
| [ERROR-HANDLING-GUIDE.md](guides/ERROR-HANDLING-GUIDE.md)       | Error handling patterns          |
| [DATABASE-GUIDE.md](guides/DATABASE-GUIDE.md)                   | Database usage (Phase 2+)        |
| [SECURITY-REVIEW.md](guides/SECURITY-REVIEW.md)                 | Security audit and practices     |
| [PERFORMANCE-PROFILING.md](guides/PERFORMANCE-PROFILING.md)     | Profiling and optimization       |
| [DEPLOYMENT-FRAMEWORK.md](guides/DEPLOYMENT-FRAMEWORK.md)       | Deployment and distribution      |
| [TEAM-ONBOARDING.md](guides/TEAM-ONBOARDING.md)                 | New contributor guide            |
| [INTEGRATION-COOKBOOK.md](guides/INTEGRATION-COOKBOOK.md)       | Integration patterns and recipes |
| [TOOLS-STATUS.md](guides/TOOLS-STATUS.md)                     | Tool installation and validation |
| [HOOK-VALIDATION-GUIDE.md](guides/HOOK-VALIDATION-GUIDE.md)   | Real Claude hook proof before product claims |
| [RUST-MULTI-AGENT-PATTERNS.md](guides/RUST-MULTI-AGENT-PATTERNS.md) | Rust-first harness and coordination patterns |

### Phase Planning

| Document                                                    | Description              |
| ----------------------------------------------------------- | ------------------------ |
| [PHASE1-CHECKLIST.md](phases/PHASE1-CHECKLIST.md)           | Implementation checklist |
| [PHASE1.5-COORDINATION.md](phases/PHASE1.5-COORDINATION.md) | Multi-agent coordination |
| [PHASE2-PERSISTENCE.md](phases/PHASE2-PERSISTENCE.md)       | Persistence layer design |
| [PHASE2-MIGRATION-PLAN.md](phases/PHASE2-MIGRATION-PLAN.md) | Migration strategy       |
| [ROADMAP-PLAN.md](ROADMAP-PLAN.md)                          | Superseded Rust/Dioxus migration roadmap retained for history |
| [IMPLEMENTATION-HANDOFF.md](plans/IMPLEMENTATION-HANDOFF.md)| Historical desktop migration execution handoff |
| [LONG-RANGE-ENHANCEMENTS.md](LONG-RANGE-ENHANCEMENTS.md) | PR-organized enhancement backlog across 8 lanes |

### Current Control-Plane Foundation

| Document | Description |
| -------- | ----------- |
| [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) | Authoritative product and desktop shell contract |
| [DESKTOP-SHELL-ARCHITECTURE.md](spec/DESKTOP-SHELL-ARCHITECTURE.md) | Dioxus Desktop + xterm.js layer boundaries |
| [IMPULSE_TERM_STATUS.md](../impulse-rs/docs/IMPULSE_TERM_STATUS.md) | Terminal backend status and desktop bridge implications |

> **NOTE:** `spec/RUST-CANONICAL-CONTRACT.md` is authoritative for implementation. Legacy TypeScript/Bun docs are retained as historical reference unless explicitly marked active and aligned.

> **Quick reference:** See `AGENTS.md` for the enforced roadmap contract. Consult
> `HONEST-ROADMAP.md` only for the retained hook/memory validation risks.

### Research & Analysis

Historical desktop migration notes, including the superseded Tauri+Dioxus handoff, are retained under planning docs for provenance but are not active implementation guidance.

| Document                                                                  | Topic                                                       |
| ------------------------------------------------------------------------- | ----------------------------------------------------------- |
| [AGENT-HARNESS-ANALYSIS.md](research/AGENT-HARNESS-ANALYSIS.md)           | Agent harness architecture analysis                         |
| [LLM-CODING-PROBLEMS.md](research/LLM-CODING-PROBLEMS.md)                 | Common LLM coding failure modes                             |
| [MEMORY-EXTRACTION-ANALYSIS.md](research/MEMORY-EXTRACTION-ANALYSIS.md)   | Memory extraction patterns                                  |
| [SEARCH-LAYER-ANALYSIS.md](research/SEARCH-LAYER-ANALYSIS.md)             | Search layer architecture                                   |
| [TERMINAL-LAYER-ANALYSIS.md](research/TERMINAL-LAYER-ANALYSIS.md)         | Terminal layer architecture                                 |
| [RESEARCH-DIGEST.md](research/RESEARCH-DIGEST.md)                         | Consolidated research findings                              |
| [META-HARNESS-RUST-MULTI-AGENT.md](research/META-HARNESS-RUST-MULTI-AGENT.md) | Meta-Harness, Rust, and multi-agent coordination synthesis |
| [TOOL-STACK-ANALYSIS.md](research/TOOL-STACK-ANALYSIS.md)                 | Tool selection analysis                                     |
| [RECONCILIATION-ANALYSIS.md](research/RECONCILIATION-ANALYSIS.md)         | Spec reconciliation                                         |
| [EFFICIENCY-ANALYSIS.md](research/EFFICIENCY-ANALYSIS.md)                 | **Implementation efficiency patterns (read before coding)** |
| [impulse-memory-architecture.md](research/impulse-memory-architecture.md) | 5-tier memory system design                                 |
| [cross-model-consensus.md](research/cross-model-consensus.md)             | Cross-model tool consensus                                  |
| [cli-language-analysis.md](research/cli-language-analysis.md)             | CLI language selection analysis                             |
| [deep-research-compaction.md](research/deep-research-compaction.md)       | Deep research: AI coding impulse                            |
| [deep-research-spec-dev.md](research/deep-research-spec-dev.md)           | Deep research: spec-driven dev                              |
| [PAGEINDEX-FEASIBILITY-DECISION.md](research/PAGEINDEX-FEASIBILITY-DECISION.md) | PageIndex feasibility decision record                 |
| [pageindex-feasibility-report.json](research/pageindex-feasibility-report.json) | PageIndex benchmark output artifact                   |
| [retrieval-perf-report.json](research/retrieval-perf-report.json)         | Retrieval + injection perf benchmark report                 |
| [algorithm-validation.md](research/algorithm-validation.md)               | Algorithm validation iterations (Ralph Loop)                |
| [TOKEN-TRACKING-ALGORITHM.md](research/TOKEN-TRACKING-ALGORITHM.md)       | Token tracking algorithm design and metrics                 |
| [2026-06-30-multi-agent-provenance-divergence.md](research/2026-06-30-multi-agent-provenance-divergence.md) | **Consolidated-view read model** — provenance-tagged, federated per-agent takes; divergence as a settled-facts-only hard gate |

See [research/README.md](research/README.md) for reading sequences by phase.

### Product Vision and Future Concepts

| Document                                              | Topic                   |
| ----------------------------------------------------- | ----------------------- |
| [VISION.md](../VISION.md)                             | Living control-plane product north star |
| [CLI-ARCHITECTURE.md](vision/CLI-ARCHITECTURE.md)     | Future CLI architecture |
| [TUI-AUGMENTATION-VISION.md](archive/TUI-AUGMENTATION-VISION.md) | Historical UI augmentation reference |
| [DYNAMIC-CLI-VISION.md](vision/DYNAMIC-CLI-VISION.md) | Dynamic CLI concepts    |
| [BENCHMARKS.md](vision/BENCHMARKS.md)                 | Performance benchmarks  |
| [DATA-MODELS.md](vision/DATA-MODELS.md)               | Data model designs      |

### Session Logs

| Document                                                                       | Description                                        |
| ------------------------------------------------------------------------------ | -------------------------------------------------- |
| [DEVELOPMENT-HISTORY.md](session-logs/DEVELOPMENT-HISTORY.md)                   | Consolidated chronological summary of all sessions |

### Archive (Superseded)

| Document                                                                       | Superseded By                               |
| ------------------------------------------------------------------------------ | ------------------------------------------- |
| [TUI-AUGMENTATION-VISION.md](archive/TUI-AUGMENTATION-VISION.md)              | Current Rust roadmap and workbench docs     |
| [Ralph plans 1-6](archive/ralph-plans/README.md)                              | Provenance only; use current roadmap/spec/ADR docs |
| Session logs (11 files)                                                         | [DEVELOPMENT-HISTORY.md](session-logs/DEVELOPMENT-HISTORY.md) |

Older Ralph loop plans and `ROADMAP-PLAN.md` are preserved for provenance only. Use
[`VISION.md`](../VISION.md),
`spec/RUST-CANONICAL-CONTRACT.md`, and `decisions/0008-dioxus-desktop-host.md` for current guidance.

---

## By Phase

### Now: Control-Plane Foundations

Start here:

1. [VISION.md](../VISION.md) — **Start here** - Product north star and live-versus-target boundary
2. [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) — Current product contract and interfaces
3. [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) — Current code boundary matrix
4. [USER-STORY-MAP.md](spec/USER-STORY-MAP.md) — Current user stories and acceptance criteria
5. [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md) — Current automated coverage and known gaps
6. [AGENTS.md](../AGENTS.md) — Current architecture and operational guidance
7. [COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md) — Agent lane, worktree, and handoff rules
8. [ROADMAP-PLAN.md](ROADMAP-PLAN.md) — Superseded desktop implementation history
9. [BEST-PRACTICES.md](guides/BEST-PRACTICES.md) — Coding conventions

### Next: Governed Supervisor/Builder Vertical Slice + Hierarchy/Enforcement ADR

1. [VISION.md](../VISION.md) — Ten-step governed workflow and unresolved hierarchy decisions
2. [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) — Live foundations and contract boundary
3. [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) — Enforcement and authority boundary map
4. [USER-STORY-MAP.md](spec/USER-STORY-MAP.md) — Acceptance criteria for current surfaces
5. [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md) — Evidence map and known gaps

### Validation Side Lane: Hook and Memory Evidence

1. [HOOK-VALIDATION-GUIDE.md](guides/HOOK-VALIDATION-GUIDE.md) — Current evidence procedure
2. [HONEST-ROADMAP.md](HONEST-ROADMAP.md) — Historical register for the remaining hook/memory hypotheses

### Later: General Roles + Negotiated Runtimes

1. [VISION.md](../VISION.md) — Target role, adapter, messaging, attention, and resource contracts
2. [CLI-ARCHITECTURE.md](vision/CLI-ARCHITECTURE.md) — Historical/future CLI concepts
3. [TUI-AUGMENTATION-VISION.md](archive/TUI-AUGMENTATION-VISION.md) — Historical UI reference

---

## By Tag

| Tag            | Documents                                                                   |
| -------------- | --------------------------------------------------------------------------- |
| `spec`         | RUST-CANONICAL-CONTRACT, COMPETITIVE-POSITIONING, PERFORMANCE-TARGETS       |
| `hooks`        | RUST-CANONICAL-CONTRACT, PHASE1-CHECKLIST, BEST-PRACTICES                   |
| `memory`       | impulse-memory-architecture, MEMORY-EXTRACTION-ANALYSIS, PHASE2-PERSISTENCE |
| `testing`      | TEST-TRACEABILITY, TESTING-FRAMEWORK, SYNTHETIC-TESTING-GUIDE               |
| `validation`   | HOOK-VALIDATION-GUIDE, HONEST-ROADMAP                                        |
| `architecture` | RESEARCH-DIGEST, ADRs                                                       |
| `tools`        | TOOL-STACK-ANALYSIS, cross-model-consensus, cli-language-analysis           |
| `security`     | SECURITY-REVIEW                                                             |
| `deployment`   | DEPLOYMENT-FRAMEWORK, ADR-0005                                              |
