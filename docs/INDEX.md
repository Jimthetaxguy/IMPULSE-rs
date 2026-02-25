---
title: Documentation Index
description: Master navigation hub for Impulse documentation
version: '1.0'
updated: 2026-02-24
type: doc
category: navigation
phase: all
status: active
audience: everyone
tags: [index, navigation, discovery]
last_updated: 2026-02-24
authors:
  - name: James Pustorino
    role: Creator
    email: James.s.Pustorino@gmail.com
    github: jamespustorino
---

# Documentation Index — Impulse

> **Master navigation hub.** Find any document by category, phase, or topic.
> **Quick start:** Read [`spec/RUST-CANONICAL-CONTRACT.md`](spec/RUST-CANONICAL-CONTRACT.md) first.
>
> **Canonical stack: Rust (impulse-rs)**
> **Roadmap contract: Now=Rust core, Next=semantic retrieval, Later=advanced coordination UX**

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
| [HONEST-ROADMAP.md](HONEST-ROADMAP.md)                        | **⚠️ READ THIS FIRST** — Limitations, unvalidated assumptions, corrections | all   |
| [COMPETITIVE-POSITIONING.md](spec/COMPETITIVE-POSITIONING.md) | Market analysis and differentiation                                        | all   |
| [PERFORMANCE-TARGETS.md](spec/PERFORMANCE-TARGETS.md)         | Performance budgets and benchmarks                                         | 1-2   |

### Architecture Decisions (ADRs)

| ADR                                           | Title                                | Status   |
| --------------------------------------------- | ------------------------------------ | -------- |
| [0001](decisions/0001-claude-code-primary.md) | Claude Code as primary agent         | Accepted |
| [0002](decisions/0002-file-first-memory.md)   | File-first memory (no DB in Phase 1) | Accepted |
| [0003](decisions/0003-progressive-search.md)  | Progressive search (FTS5 → vectors)  | Accepted |
| [0004](decisions/0004-extraction-strategy.md) | LLM extraction strategy              | Accepted |
| [0005](decisions/0005-distribution-model.md)  | npm distribution model               | Accepted |

See [decisions/README.md](decisions/README.md) for full decision log.

### Developer Guides

| Document                                                        | Topic                            |
| --------------------------------------------------------------- | -------------------------------- |
| [BEST-PRACTICES.md](guides/BEST-PRACTICES.md)                   | Coding conventions and patterns  |
| [TESTING-FRAMEWORK.md](guides/TESTING-FRAMEWORK.md)             | Test infrastructure and patterns |
| [SYNTHETIC-TESTING-GUIDE.md](guides/SYNTHETIC-TESTING-GUIDE.md) | Synthetic test generation        |
| [ERROR-HANDLING-GUIDE.md](guides/ERROR-HANDLING-GUIDE.md)       | Error handling patterns          |
| [DATABASE-GUIDE.md](guides/DATABASE-GUIDE.md)                   | Database usage (Phase 2+)        |
| [SECURITY-REVIEW.md](guides/SECURITY-REVIEW.md)                 | Security audit and practices     |
| [PERFORMANCE-PROFILING.md](guides/PERFORMANCE-PROFILING.md)     | Profiling and optimization       |
| [DEPLOYMENT-FRAMEWORK.md](guides/DEPLOYMENT-FRAMEWORK.md)       | Deployment and distribution      |
| [TEAM-ONBOARDING.md](guides/TEAM-ONBOARDING.md)                 | New contributor guide            |
| [INTEGRATION-COOKBOOK.md](guides/INTEGRATION-COOKBOOK.md)       | Integration patterns and recipes |
| [TOOLS-STATUS.md](guides/TOOLS-STATUS.md)                     | Tool installation and validation |

### Phase Planning

| Document                                                    | Description              |
| ----------------------------------------------------------- | ------------------------ |
| [PHASE1-CHECKLIST.md](phases/PHASE1-CHECKLIST.md)           | Implementation checklist |
| [PHASE1.5-COORDINATION.md](phases/PHASE1.5-COORDINATION.md) | Multi-agent coordination |
| [PHASE2-PERSISTENCE.md](phases/PHASE2-PERSISTENCE.md)       | Persistence layer design |
| [PHASE2-MIGRATION-PLAN.md](phases/PHASE2-MIGRATION-PLAN.md) | Migration strategy       |

> **NOTE:** `spec/RUST-CANONICAL-CONTRACT.md` is authoritative for implementation. Legacy TypeScript/Bun docs are retained as historical reference unless explicitly marked active and aligned.

> **Quick reference:** See `AGENTS.md` for simplified roadmap (Now/Next/Later/Future)

### Research & Analysis

| Document                                                                  | Topic                                                       |
| ------------------------------------------------------------------------- | ----------------------------------------------------------- |
| [AGENT-HARNESS-ANALYSIS.md](research/AGENT-HARNESS-ANALYSIS.md)           | Agent harness architecture analysis                         |
| [LLM-CODING-PROBLEMS.md](research/LLM-CODING-PROBLEMS.md)                 | Common LLM coding failure modes                             |
| [MEMORY-EXTRACTION-ANALYSIS.md](research/MEMORY-EXTRACTION-ANALYSIS.md)   | Memory extraction patterns                                  |
| [SEARCH-LAYER-ANALYSIS.md](research/SEARCH-LAYER-ANALYSIS.md)             | Search layer architecture                                   |
| [TERMINAL-LAYER-ANALYSIS.md](research/TERMINAL-LAYER-ANALYSIS.md)         | Terminal layer architecture                                 |
| [RESEARCH-DIGEST.md](research/RESEARCH-DIGEST.md)                         | Consolidated research findings                              |
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

See [research/README.md](research/README.md) for reading sequences by phase.

### Vision (Phase 2+)

| Document                                              | Topic                   |
| ----------------------------------------------------- | ----------------------- |
| [CLI-ARCHITECTURE.md](vision/CLI-ARCHITECTURE.md)     | Future CLI architecture |
| [DASHBOARD-DESIGN.md](vision/DASHBOARD-DESIGN.md)     | Dashboard UI design     |
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
| [ARCHITECTURE.md](archive/ARCHITECTURE.md)                                     | REALISTIC-FRAMEWORK.md + PRODUCT-SPEC-v2.md |
| [MVP-SPECIFICATION.md](archive/MVP-SPECIFICATION.md)                           | PRODUCT-SPEC-v2.md                          |
| [SPEC-v1.1.md](archive/SPEC-v1.1.md)                                           | PRODUCT-SPEC-v2.md                          |
| [STEWARD.md](archive/STEWARD.md)                                               | Hooks-based architecture                    |
| [OPENCODE-INTEGRATION.md](archive/OPENCODE-INTEGRATION.md)                     | impulse-plugin/ implementation              |
| [IMPLEMENTATION-ROADMAP.md](archive/phases/IMPLEMENTATION-ROADMAP.md)           | RUST-CANONICAL-CONTRACT.md                  |
| [PRODUCT-SPEC-v2.md](archive/spec/PRODUCT-SPEC-v2.md)                         | RUST-CANONICAL-CONTRACT.md                  |
| [PRD-v1.md](archive/research/PRD-v1.md)                                       | RUST-CANONICAL-CONTRACT.md                  |
| [REALISTIC-FRAMEWORK.md](archive/research/REALISTIC-FRAMEWORK.md)             | RUST-CANONICAL-CONTRACT.md                  |
| [cockpit-feature-plan.md](archive/research/cockpit-feature-plan.md)             | Current impulse-rs implementation            |
| Session logs (11 files)                                                         | [DEVELOPMENT-HISTORY.md](session-logs/DEVELOPMENT-HISTORY.md) |

See [archive/README.md](archive/README.md) for full supersession history.

---

## By Phase

### Now: Rust CLI + Hybrid Mode

Start here:

1. [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) — **Start here** - Product contract and interfaces
2. [AGENTS.md](../AGENTS.md) — Current architecture and operational guidance
3. [EFFICIENCY-ANALYSIS.md](research/EFFICIENCY-ANALYSIS.md) — Implementation patterns
4. [BEST-PRACTICES.md](guides/BEST-PRACTICES.md) — Coding conventions
5. [TESTING-FRAMEWORK.md](guides/TESTING-FRAMEWORK.md) — Test patterns
6. [ADR-0002: File-first memory](decisions/0002-file-first-memory.md) — Why no DB

### Next: Retrieval Foundation + Review-First Injection

1. [PHASE2-PERSISTENCE.md](phases/PHASE2-PERSISTENCE.md) — Persistence design
2. [PHASE2-MIGRATION-PLAN.md](phases/PHASE2-MIGRATION-PLAN.md) — Migration strategy
3. [ADR-0003: Progressive search](decisions/0003-progressive-search.md) — FTS5 first
4. [DATABASE-GUIDE.md](guides/DATABASE-GUIDE.md) — Database patterns
5. [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) — Retrieval + injection command/artifact contract
6. [PAGEINDEX-FEASIBILITY-DECISION.md](research/PAGEINDEX-FEASIBILITY-DECISION.md) — Current NO-GO decision

### Future: Advanced UI

1. [DASHBOARD-DESIGN.md](vision/DASHBOARD-DESIGN.md) — Dashboard concepts
2. [CLI-ARCHITECTURE.md](vision/CLI-ARCHITECTURE.md) — CLI evolution

---

## By Tag

| Tag            | Documents                                                                   |
| -------------- | --------------------------------------------------------------------------- |
| `spec`         | RUST-CANONICAL-CONTRACT, COMPETITIVE-POSITIONING, PERFORMANCE-TARGETS       |
| `hooks`        | RUST-CANONICAL-CONTRACT, PHASE1-CHECKLIST, BEST-PRACTICES                   |
| `memory`       | impulse-memory-architecture, MEMORY-EXTRACTION-ANALYSIS, PHASE2-PERSISTENCE |
| `testing`      | TESTING-FRAMEWORK, SYNTHETIC-TESTING-GUIDE                                  |
| `architecture` | RESEARCH-DIGEST, ADRs                                                       |
| `tools`        | TOOL-STACK-ANALYSIS, cross-model-consensus, cli-language-analysis           |
| `security`     | SECURITY-REVIEW                                                             |
| `deployment`   | DEPLOYMENT-FRAMEWORK, ADR-0005                                              |
