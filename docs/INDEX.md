---
title: Documentation Index
description: Master navigation hub for Impulse documentation
version: '1.7'
updated: 2026-08-17
type: doc
category: navigation
phase: all
status: active
audience: everyone
tags: [index, navigation, discovery]
last_updated: 2026-08-17
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
> **Roadmap contract:** Now=control-plane foundations + governed runtime producers + accepted-run review candidates; Next=stronger same-user actor authorization + full launched Builder/Supervisor proof; Later=explicit memory promotion/dismissal + general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.
> **Current governed slice:** profiled Builder preflight, exact criteria, daemon-attested clean Git
> subjects, daemon-derived claim/verification, strict API Supervisor review, operator acceptance,
> and deterministic pending candidates that do not mutate `GENOME`/`HISTORY`.
> **Next governed slice:** stronger same-user actor authorization and one full launched
> Builder/Supervisor proof; explicit candidate promotion/dismissal remains later.
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

### Product North Star and Boundaries

| Document | Description |
| --- | --- |
| [VISION.md](../VISION.md) | **Living product north star:** control-plane promise, hierarchy, live-versus-target boundary, and complete vertical slice |
| [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) | Authoritative contract for current implemented behavior |
| [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) | Current code boundary matrix and enforcement truth |

ADR-0010 accepts role/task launch preflight, ADR-0011 accepts the governed-task lifecycle,
ADR-0012 accepts the first daemon-owned producer profile, and ADR-0013 accepts deterministic
pending accepted-run candidates. Do not split separate
role/runtime/supervisor schema documents until the
remaining hierarchy, adapter, reassignment, and generalized capability-negotiation decisions listed
in `VISION.md` are resolved.

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
| [0010](decisions/0010-product-role-launch-contract.md) | Product role launch contract | Accepted | Current explicit role/task launch-preflight boundary |
| [0011](decisions/0011-governed-task-run-lifecycle.md) | Daemon-owned governed task lifecycle | Accepted | Durable task/evidence/review authority and operator-required acceptance |
| [0012](decisions/0012-daemon-owned-governed-runtime-producers.md) | Daemon-owned governed runtime producers | Accepted | Profiled clean-Git claims, detached Rust verification, and strict API Supervisor review |
| [0013](decisions/0013-deterministic-accepted-run-memory-candidates.md) | Deterministic accepted-run memory candidates | Accepted | Owner-only pending review projection, provenance/source assurance, and no `GENOME`/`HISTORY` mutation |
| [0014](decisions/0014-work-item-and-comparative-settlement.md) | WorkItem identity and comparative settlement | Proposed | Proposed planning identity, fan-out effect bound, and four-part settlement record |
| [0015](decisions/0015-harness-owned-step-model.md) | Harness-owned step model choice | Accepted | Pure Impulse-owned per-step policy; hosts retain inference, provider, and evidence authority |
| [0017](decisions/0017-canonical-loop-contract.md) | Canonical loop contract | Proposed | Typed loop budgets, breaker-evaluated stop conditions, and termination evidence for Ion tool loops |
| [0018](decisions/0018-socket-actor-provenance.md) | Socket actor provenance | Proposed | Connection classes from peer credentials plus a per-run operator capability; only an operator surface can mint `accepted` |

See [decisions/README.md](decisions/README.md) for full decision log.

### Current Developer Guides

| Document                                                        | Topic                            |
| --------------------------------------------------------------- | -------------------------------- |
| [COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md) | Agent lanes, worktrees, handoffs, and verification |
| [CONTRIBUTING.md](../CONTRIBUTING.md)                           | Contribution rules for humans and agents |
| [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md)               | Current Rust-first test baseline and gap map |
| [HOOK-VALIDATION-GUIDE.md](guides/HOOK-VALIDATION-GUIDE.md)   | Real Claude hook proof before product claims |
| [RUST-MULTI-AGENT-PATTERNS.md](guides/RUST-MULTI-AGENT-PATTERNS.md) | Rust-first harness and coordination patterns |
| [FRONTMATTER-SCHEMA.md](FRONTMATTER-SCHEMA.md) | Documentation metadata and authority vocabulary |

### Historical Product and TypeScript/Bun References

These documents are retained for provenance and reusable ideas. Their front matter and opening
banners mark them non-authoritative; they must not override the product vision, Rust contract, live
CLI help, or current tests.

| Document | Historical scope | Replaced by |
| --- | --- | --- |
| [COMPETITIVE-POSITIONING.md](spec/COMPETITIVE-POSITIONING.md) | Memory-plugin positioning | [VISION.md](../VISION.md) |
| [PERFORMANCE-TARGETS.md](spec/PERFORMANCE-TARGETS.md) | Hook/plugin budgets | [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) |
| [BEST-PRACTICES.md](guides/BEST-PRACTICES.md) | TypeScript conventions | [CONTRIBUTING.md](../CONTRIBUTING.md) |
| [TEAM-ONBOARDING.md](guides/TEAM-ONBOARDING.md) | SWARM onboarding | [COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md) |
| [SECURITY-REVIEW.md](guides/SECURITY-REVIEW.md) | Phase-1 plugin review | [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) |
| [INTEGRATION-COOKBOOK.md](guides/INTEGRATION-COOKBOOK.md) | Phase-era workflows | [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) |
| [SYNTHETIC-TESTING-GUIDE.md](guides/SYNTHETIC-TESTING-GUIDE.md) and [TESTING-STRATEGY-ENHANCEMENTS.md](guides/TESTING-STRATEGY-ENHANCEMENTS.md) | Planned test designs | [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md) |
| [DATABASE-GUIDE.md](guides/DATABASE-GUIDE.md), [ERROR-HANDLING-GUIDE.md](guides/ERROR-HANDLING-GUIDE.md), and [PERFORMANCE-PROFILING.md](guides/PERFORMANCE-PROFILING.md) | TypeScript implementation guidance | Live Rust source and canonical contract |
| [DEPLOYMENT-FRAMEWORK.md](guides/DEPLOYMENT-FRAMEWORK.md) | SWARM deployment design | [README.md](../README.md) real-systems boundary |
| [CLI-REFERENCE-ENHANCEMENTS.md](guides/CLI-REFERENCE-ENHANCEMENTS.md) | Proposed commands | [CLI-COMMANDS.md](CLI-COMMANDS.md) and live help |
| [RELEASE-NOTES-TEMPLATE.md](guides/RELEASE-NOTES-TEMPLATE.md) | Earlier release gate | [AGENTS.md](../AGENTS.md) verification contract |

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
| [ADR-0010](decisions/0010-product-role-launch-contract.md) | Current explicit product-role/task launch-preflight boundary |
| [ADR-0011](decisions/0011-governed-task-run-lifecycle.md) | Current daemon-owned governed task evidence/decision boundary |
| [ADR-0012](decisions/0012-daemon-owned-governed-runtime-producers.md) | Current profiled claim/verification/Supervisor producer boundary |
| [ADR-0013](decisions/0013-deterministic-accepted-run-memory-candidates.md) | Current deterministic accepted-run review-projection boundary |

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
`spec/RUST-CANONICAL-CONTRACT.md`, `decisions/0008-dioxus-desktop-host.md`,
`decisions/0010-product-role-launch-contract.md`,
`decisions/0011-governed-task-run-lifecycle.md`, and
`decisions/0012-daemon-owned-governed-runtime-producers.md` for current guidance.

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

### Next: Complete Launched-Runtime Governed Workflow

1. [0013-deterministic-accepted-run-memory-candidates.md](decisions/0013-deterministic-accepted-run-memory-candidates.md) — Live pending review projection and explicit no-promotion boundary
2. [0012-daemon-owned-governed-runtime-producers.md](decisions/0012-daemon-owned-governed-runtime-producers.md) — Live first producer profile and explicit security limits
3. [VISION.md](../VISION.md) — Ten-step workflow, accepted-run review boundary, and launched-runtime proof
4. [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) — Live foundations and contract boundary
5. [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) — Enforcement and authority boundary map
6. [USER-STORY-MAP.md](spec/USER-STORY-MAP.md) — Acceptance criteria for current surfaces
7. [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md) — Evidence map and known gaps

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
| `spec`         | RUST-CANONICAL-CONTRACT, USER-STORY-MAP, TEST-TRACEABILITY                   |
| `hooks`        | RUST-CANONICAL-CONTRACT, HOOK-VALIDATION-GUIDE                               |
| `memory`       | impulse-memory-architecture, MEMORY-EXTRACTION-ANALYSIS, PHASE2-PERSISTENCE |
| `testing`      | TEST-TRACEABILITY; phase-era testing guides are historical                   |
| `validation`   | HOOK-VALIDATION-GUIDE, HONEST-ROADMAP                                        |
| `architecture` | RESEARCH-DIGEST, ADRs                                                       |
| `tools`        | TOOL-STACK-ANALYSIS, cross-model-consensus, cli-language-analysis           |
| `security`     | ARCHITECTURE-CLARIFICATION; SECURITY-REVIEW is historical                    |
| `deployment`   | README real-systems boundary; DEPLOYMENT-FRAMEWORK is historical             |
