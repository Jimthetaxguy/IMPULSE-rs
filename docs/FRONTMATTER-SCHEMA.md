---
title: Front Matter Schema
description: YAML front matter schema for Impulse documentation
version: '1.1'
updated: 2026-07-12
type: schema
status: active
---

# Front Matter Schema

This document defines the standard YAML front matter for all Impulse documentation files.

## Recommended Active-Document Fields

The validator accepts partial front matter so point-in-time and legacy records remain readable.
New or actively maintained documents should carry these fields:

| Field         | Type   | Description                      |
| ------------- | ------ | -------------------------------- |
| `title`       | string | Document title                   |
| `description` | string | Brief description (50-200 chars) |
| `updated`     | date   | Last update date (YYYY-MM-DD)    |

Living guides, specifications, schemas, and references are checked for staleness. Point-in-time
ADRs, research, vision records, phase records, and documents with non-authoritative status are
exempt; do not date-bump them merely to silence validation.

## Optional Fields

| Field      | Type   | Description           | Default      |
| ---------- | ------ | --------------------- | ------------ |
| `version`  | string | Semantic version      | "1.0"        |
| `type`     | enum   | Document type         | "doc"        |
| `category` | string | Category for grouping | "general"    |
| `phase`    | enum   | Project phase         | "all"        |
| `status`   | enum   | Document status       | "draft"      |
| `tags`     | array  | Searchable tags       | []           |
| `audience` | string | Target audience       | "developers" |
| `authors`  | array  | Author information    | []           |

## Type Enum

```yaml
type:
  - agent_guidelines # AI agent instructions
  - specification # Product/tech spec
  - guide # How-to guide
  - decision # ADR (Architecture Decision Record)
  - research # Research document
  - vision # Future vision
  - metadata # Metadata/navigation
  - schema # Schema definition
  - doc # General document
  - reference # Durable reference material
```

## Phase Enum

```yaml
phase:
  - phase1 # Core infrastructure
  - phase1.5 # Coordination-era records
  - phase2 # Persistence
  - phase3 # Semantic search
  - all # Cross-phase
  - historical # Historical-only material
```

## Status Enum

```yaml
status:
  - draft # Under development
  - review # Under review
  - active # Currently used
  - deprecated # No longer maintained
  - complete # Finished
  - superseded # Replaced by a newer authority
  - archive # Retained for provenance only
  - accepted # Accepted decision or research outcome
```

## Example: Complete Front Matter

```yaml
---
title: Document Title
description: A brief description of the document
version: '1.0'
updated: 2026-02-23
type: guide
category: development
phase: phase2
status: active
tags: [rust, tui, ratatui]
audience: developers
authors:
  - name: Impulse Maintainers
    role: Maintainer
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---
```

## Example: Minimal Front Matter

```yaml
---
title: My Document
description: Brief description
updated: 2026-02-23
---
```

## Validation

Use this schema to validate front matter in documentation files:

```bash
# Metadata, links, freshness, and product-contract drift
python3 docs/validate_docs.py --all

# Machine-readable output (same validation result)
python3 docs/validate_docs.py --all --json
```

## Quick Reference

| Document             | Recommended Fields                                                                           | Typical Type     |
| -------------------- | -------------------------------------------------------------------------------------------- | ---------------- |
| AGENTS.md            | title, description, version, updated, type, category, phase, status, tags, audience, authors | agent_guidelines |
| INDEX.md             | title, description, updated, status, phase, audience, tags                                   | doc              |
| decisions/XXXX-\*.md | title, description, updated, status, type                                                    | decision         |
| guides/\*.md         | title, description, updated, phase, status                                                   | guide            |
| vision/\*.md         | title, description, updated, phase                                                           | vision           |
| spec/\*.md           | title, description, updated, phase, status                                                   | specification    |
