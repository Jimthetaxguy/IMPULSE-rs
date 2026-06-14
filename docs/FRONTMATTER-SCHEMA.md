---
title: Front Matter Schema
description: YAML front matter schema for Impulse documentation
version: '1.0'
updated: 2026-02-23
type: schema
---

# Front Matter Schema

This document defines the standard YAML front matter for all Impulse documentation files.

## Required Fields

| Field         | Type   | Description                      |
| ------------- | ------ | -------------------------------- |
| `title`       | string | Document title                   |
| `description` | string | Brief description (50-200 chars) |
| `updated`     | date   | Last update date (YYYY-MM-DD)    |

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
```

## Phase Enum

```yaml
phase:
  - phase1 # Core infrastructure
  - phase2 # Persistence
  - phase3 # Semantic search
  - all # Cross-phase
```

## Status Enum

```yaml
status:
  - draft # Under development
  - review # Under review
  - active # Currently used
  - deprecated # No longer maintained
  - complete # Finished
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
# Check all docs have valid front matter
grep -L "^---" docs/**/*.md

# Extract and validate YAML
python3 -c "import yaml; yaml.safe_load(open('docs/example.md').read())"
```

## Quick Reference

| Document             | Required Fields                                                                              | Typical Type     |
| -------------------- | -------------------------------------------------------------------------------------------- | ---------------- |
| AGENTS.md            | title, description, version, updated, type, category, phase, status, tags, audience, authors | agent_guidelines |
| INDEX.md             | title, description, updated, status, phase, audience, tags                                   | doc              |
| decisions/XXXX-\*.md | title, description, updated, status, type                                                    | decision         |
| guides/\*.md         | title, description, updated, phase, status                                                   | guide            |
| vision/\*.md         | title, description, updated, phase                                                           | vision           |
| spec/\*.md           | title, description, updated, phase, status                                                   | specification    |
