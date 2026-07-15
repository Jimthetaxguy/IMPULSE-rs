---
title: Documentation Summary
description: Unified navigation structure for Impulse documentation
version: 2.6
updated: 2026-07-13
schema_version: '1.0'
---

# Documentation Summary

## Quick Reference

| Guide | Description | Start Here |
| ----- | ----------- | ---------- |
| [VISION.md](../VISION.md) | Living product north star and target governed vertical slice | ✅ Yes |
| [RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md) | Authoritative product contract | ✅ Yes |
| [USER-STORY-MAP.md](spec/USER-STORY-MAP.md) | Rust-first user stories and acceptance criteria | ✅ Yes |
| [TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md) | Current story-to-test coverage map | ✅ Yes |
| [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md) | Current control-plane boundary and enforcement map | ✅ Yes |
| [AGENTS.md](../AGENTS.md) | AI agent guidelines | ✅ Yes |
| [COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md) | Agent lane, worktree, and handoff rules | ✅ Yes |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | Human and agent contribution rules | ✅ Yes |
| [INDEX.md](INDEX.md) | Master navigation | ✅ Yes |

## Active Track

Roadmap contract: Now=control-plane foundations + daemon-owned governed task lifecycle; Next=real claim/verification producers + supervisor/builder process proof + accepted-run memory promotion; Later=general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.

`HONEST-ROADMAP.md` is a historical hook/memory validation register, not the current product
roadmap or whole-product risk authority.

- [VISION.md](../VISION.md)
- [spec/RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md)
- [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md)
- [spec/USER-STORY-MAP.md](spec/USER-STORY-MAP.md)
- [spec/TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md)
- [guides/COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md)

## By Phase

### Now

- [VISION.md](../VISION.md)
- [spec/RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md)
- [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md)

### Next

- [VISION.md](../VISION.md) — real claim/verification producers, launched-supervisor proof, accepted-run memory promotion, and remaining hierarchy/enforcement ADRs
- [spec/USER-STORY-MAP.md](spec/USER-STORY-MAP.md)
- [spec/TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md)

### Later

- [VISION.md](../VISION.md) — general roles, negotiated runtimes, messaging, and attention
- [vision/CLI-ARCHITECTURE.md](vision/CLI-ARCHITECTURE.md)
- [archive/TUI-AUGMENTATION-VISION.md](archive/TUI-AUGMENTATION-VISION.md)

## By Category

### Specifications

- [spec/RUST-CANONICAL-CONTRACT.md](spec/RUST-CANONICAL-CONTRACT.md)
- [spec/USER-STORY-MAP.md](spec/USER-STORY-MAP.md)
- [spec/TEST-TRACEABILITY.md](spec/TEST-TRACEABILITY.md)

### Roadmap

- [ROADMAP-PLAN.md](ROADMAP-PLAN.md) — superseded Rust/Dioxus implementation history
- [plans/IMPLEMENTATION-HANDOFF.md](plans/IMPLEMENTATION-HANDOFF.md) — historical desktop handoff
- [../impulse-rs/docs/IMPULSE_TERM_STATUS.md](../impulse-rs/docs/IMPULSE_TERM_STATUS.md)

### Validation and Historical Risk

- [HONEST-ROADMAP.md](HONEST-ROADMAP.md) — historical register for unresolved hook and memory hypotheses
- [guides/HOOK-VALIDATION-GUIDE.md](guides/HOOK-VALIDATION-GUIDE.md) — current evidence procedure

### Architecture & Vision

- [VISION.md](../VISION.md)
- [ARCHITECTURE-CLARIFICATION.md](ARCHITECTURE-CLARIFICATION.md)
- [vision/CLI-ARCHITECTURE.md](vision/CLI-ARCHITECTURE.md)
- [vision/DATA-MODELS.md](vision/DATA-MODELS.md)
- [vision/DYNAMIC-CLI-VISION.md](vision/DYNAMIC-CLI-VISION.md)
- [archive/TUI-AUGMENTATION-VISION.md](archive/TUI-AUGMENTATION-VISION.md)
- [vision/BENCHMARKS.md](vision/BENCHMARKS.md)

### Research

- [research/README.md](research/README.md)
- [research/SEARCH-LAYER-ANALYSIS.md](research/SEARCH-LAYER-ANALYSIS.md)
- [research/TERMINAL-LAYER-ANALYSIS.md](research/TERMINAL-LAYER-ANALYSIS.md)
- [research/MEMORY-EXTRACTION-ANALYSIS.md](research/MEMORY-EXTRACTION-ANALYSIS.md)
- [research/LLM-CODING-PROBLEMS.md](research/LLM-CODING-PROBLEMS.md)
- [research/PAGEINDEX-FEASIBILITY-DECISION.md](research/PAGEINDEX-FEASIBILITY-DECISION.md)
- [research/TOKEN-TRACKING-ALGORITHM.md](research/TOKEN-TRACKING-ALGORITHM.md)

### Guides

- [guides/COLLABORATIVE-AGENTIC-CODING.md](guides/COLLABORATIVE-AGENTIC-CODING.md)
- [guides/HOOK-VALIDATION-GUIDE.md](guides/HOOK-VALIDATION-GUIDE.md)
- [guides/RUST-MULTI-AGENT-PATTERNS.md](guides/RUST-MULTI-AGENT-PATTERNS.md)
- [FRONTMATTER-SCHEMA.md](FRONTMATTER-SCHEMA.md)

### Historical Reference

- [spec/COMPETITIVE-POSITIONING.md](spec/COMPETITIVE-POSITIONING.md)
- [spec/PERFORMANCE-TARGETS.md](spec/PERFORMANCE-TARGETS.md)
- [guides/SECURITY-REVIEW.md](guides/SECURITY-REVIEW.md)
- [guides/INTEGRATION-COOKBOOK.md](guides/INTEGRATION-COOKBOOK.md)
- [guides/SYNTHETIC-TESTING-GUIDE.md](guides/SYNTHETIC-TESTING-GUIDE.md)
- [guides/BEST-PRACTICES.md](guides/BEST-PRACTICES.md)
- [guides/DEPLOYMENT-FRAMEWORK.md](guides/DEPLOYMENT-FRAMEWORK.md)
- [guides/TEAM-ONBOARDING.md](guides/TEAM-ONBOARDING.md)
- [guides/TESTING-STRATEGY-ENHANCEMENTS.md](guides/TESTING-STRATEGY-ENHANCEMENTS.md)
- [guides/RELEASE-NOTES-TEMPLATE.md](guides/RELEASE-NOTES-TEMPLATE.md)
- [guides/DATABASE-GUIDE.md](guides/DATABASE-GUIDE.md)
- [guides/PERFORMANCE-PROFILING.md](guides/PERFORMANCE-PROFILING.md)
- [guides/CLI-REFERENCE-ENHANCEMENTS.md](guides/CLI-REFERENCE-ENHANCEMENTS.md)
- [guides/ERROR-HANDLING-GUIDE.md](guides/ERROR-HANDLING-GUIDE.md)
- [guides/TESTING-FRAMEWORK.md](guides/TESTING-FRAMEWORK.md)
- [guides/TOOLS-STATUS.md](guides/TOOLS-STATUS.md)

### Decisions (ADRs)

- [decisions/README.md](decisions/README.md)
- [decisions/0001-claude-code-primary.md](decisions/0001-claude-code-primary.md)
- [decisions/0002-file-first-memory.md](decisions/0002-file-first-memory.md)
- [decisions/0003-progressive-search.md](decisions/0003-progressive-search.md)
- [decisions/0004-extraction-strategy.md](decisions/0004-extraction-strategy.md)
- [decisions/0005-distribution-model.md](decisions/0005-distribution-model.md)
- [decisions/0006-hook-enhancement-conflict-resolution.md](decisions/0006-hook-enhancement-conflict-resolution.md)
- [decisions/0007-desktop-shell-stack.md](decisions/0007-desktop-shell-stack.md)
- [decisions/0008-dioxus-desktop-host.md](decisions/0008-dioxus-desktop-host.md)
- [decisions/0009-reconcile-impulse-copies.md](decisions/0009-reconcile-impulse-copies.md)
- [decisions/0010-product-role-launch-contract.md](decisions/0010-product-role-launch-contract.md)
- [decisions/0011-governed-task-run-lifecycle.md](decisions/0011-governed-task-run-lifecycle.md)

### Session Logs

- [session-logs/DEVELOPMENT-HISTORY.md](session-logs/DEVELOPMENT-HISTORY.md)

### Archive

- [archive/TUI-AUGMENTATION-VISION.md](archive/TUI-AUGMENTATION-VISION.md)
- [archive/ralph-plans/README.md](archive/ralph-plans/README.md)

Historical Ralph loop plans now live under `archive/ralph-plans/` and are provenance only, not current implementation guidance.

## Metadata

| Field | Value |
| ----- | ----- |
| Total Documents | 138 |
| Categories | 9 |
| Decisions (ADRs) | 11 |
| Phases | 3 |
| Status | Active Development |

## Generation

This file is auto-generated from YAML source. Edit `SUMMARY.yaml` to modify structure.
