---
status: active
phase: all
audience: builder
tags: [research, analysis, reference]
last_updated: 2026-02-20
---

# Research Index — Impulse

> Navigator for research documents, organized by phase relevance.

---

## Document Inventory

### Deep Analysis (from Phase 0 source code review)

| Document | Size | Topic |
|----------|------|-------|
| [AGENT-HARNESS-ANALYSIS.md](AGENT-HARNESS-ANALYSIS.md) | ~40 KB | Agent harness architecture patterns |
| [LLM-CODING-PROBLEMS.md](LLM-CODING-PROBLEMS.md) | ~24 KB | Common LLM coding failure modes |
| [MEMORY-EXTRACTION-ANALYSIS.md](MEMORY-EXTRACTION-ANALYSIS.md) | ~45 KB | Memory extraction pipeline analysis |
| [SEARCH-LAYER-ANALYSIS.md](SEARCH-LAYER-ANALYSIS.md) | ~41 KB | Search layer architecture |
| [TERMINAL-LAYER-ANALYSIS.md](TERMINAL-LAYER-ANALYSIS.md) | ~44 KB | Terminal layer architecture |

### Synthesis & Framework

| Document | Size | Topic |
|----------|------|-------|
| [RESEARCH-DIGEST.md](RESEARCH-DIGEST.md) | ~13 KB | Consolidated research findings |
| [TOOL-STACK-ANALYSIS.md](TOOL-STACK-ANALYSIS.md) | ~15 KB | Tool selection with risk analysis |
| [REALISTIC-FRAMEWORK.md](REALISTIC-FRAMEWORK.md) | ~25 KB | Architectural pivot rationale (SWARM → 3 files) |
| [RECONCILIATION-ANALYSIS.md](RECONCILIATION-ANALYSIS.md) | ~15 KB | Spec reconciliation analysis |

### External Research (moved from project root)

| Document | Original Name | Topic |
|----------|---------------|-------|
| [impulse-memory-architecture.md](impulse-memory-architecture.md) | `Impulse Memory Architecture.md` | 5-tier memory system, retrieval patterns |
| [impulse-feature-plan.md](impulse-feature-plan.md) | `Claude Code Impulse Feature Plan.md` | Zellij layouts, UI components |
| [cross-model-consensus.md](cross-model-consensus.md) | `Where Models Agree.md` | Cross-model tool consensus (~118 KB) |
| [cli-language-analysis.md](cli-language-analysis.md) | `what are clis usually coded in...md` | CLI language selection (~65 KB) |
| [deep-research-compaction.md](deep-research-compaction.md) | `ai_coding_impulse_gpt_deep-research-report.md` | AI coding impulse landscape |
| [deep-research-spec-dev.md](deep-research-spec-dev.md) | `ai_coding_spec-dev_impulse_gpt_deep-research-report.md` | Spec-driven development |

---

## Reading Sequences by Phase

### Phase 1: MVP ("Three Files and a Hook")

| Order | Document | Focus | Time |
|-------|----------|-------|------|
| 1 | [impulse-feature-plan.md](impulse-feature-plan.md) | Sections 2-4: layouts, status bar | 45 min |
| 2 | [cross-model-consensus.md](cross-model-consensus.md) | Sections 2-3: implementation patterns | 30 min |
| 3 | [REALISTIC-FRAMEWORK.md](REALISTIC-FRAMEWORK.md) | Architectural pivot rationale | 20 min |

### Phase 2: Semantic Search

| Order | Document | Focus | Time |
|-------|----------|-------|------|
| 1 | [impulse-memory-architecture.md](impulse-memory-architecture.md) | Sections 5-7: pattern detection, decay | 1.5 hr |
| 2 | [MEMORY-EXTRACTION-ANALYSIS.md](MEMORY-EXTRACTION-ANALYSIS.md) | Extraction pipeline patterns | 1 hr |
| 3 | [SEARCH-LAYER-ANALYSIS.md](SEARCH-LAYER-ANALYSIS.md) | FTS5 and vector search | 45 min |

### Phase 3: Advanced UI

| Order | Document | Focus | Time |
|-------|----------|-------|------|
| 1 | [impulse-feature-plan.md](impulse-feature-plan.md) | Sections 5-7: advanced UI, dashboard | 45 min |
| 2 | [TERMINAL-LAYER-ANALYSIS.md](TERMINAL-LAYER-ANALYSIS.md) | Terminal integration patterns | 1 hr |

---

## Quick Lookup by Topic

| Topic | Document | Section |
|-------|----------|---------|
| 5-tier memory model | impulse-memory-architecture.md | Section 2 |
| Pattern detection | impulse-memory-architecture.md | Section 5 |
| Confidence decay | impulse-memory-architecture.md | Section 7 |
| Why Zellij over tmux | cross-model-consensus.md | Section 1 |
| Why sqlite-vec | impulse-memory-architecture.md | Section 2 |
| Bun vs Node | cross-model-consensus.md | Section 2 |
| TF-IDF scoring | MEMORY-EXTRACTION-ANALYSIS.md | -- |
| Status bar design | impulse-feature-plan.md | Section 4 |
| Language selection | cli-language-analysis.md | Comparison table |

---

## Validation

These documents were validated against source code from:
- OpenCode plugin SDK
- claude-historian-mcp (TF-IDF scoring)
- sqlite-vec C API
- Zellij plugin system
- mem0 extraction pipeline

Corrections discovered are documented in the [ADRs](../decisions/README.md).
