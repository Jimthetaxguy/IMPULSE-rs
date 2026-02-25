---
title: PageIndex Feasibility Decision
description: Decision record for optional PageIndex alignment with Impulse retrieval goals
version: '1.0'
updated: 2026-02-23
type: research
category: analysis
phase: phase3
status: active
audience: builders
tags: [pageindex, retrieval, benchmark, decision]
---

# PageIndex Feasibility Decision

## Purpose

Determine whether PageIndex capabilities should be integrated as an optional Impulse retrieval backend while preserving local-first guarantees.

## Inputs

- Benchmark harness: `memory-pipeline/pageindex_feasibility_benchmark.py`
- Query set: `memory-pipeline/pageindex_eval_queries.sample.json` (or project-specific labeled set)
- Output report: `docs/research/pageindex-feasibility-report.json`

## Decision Gates

1. Quality uplift:
   - PageIndex local-structure must improve precision@k by >=10% on labeled structural queries.
2. Latency:
   - p95 and p99 must stay within acceptable operational limits for local workflows.
3. Operational complexity:
   - No required cloud dependency for baseline path.
   - Failure modes must preserve keyword fallback guarantees.

## Result

- Decision: `NO-GO`
- Date: 2026-02-23
- Reviewer(s): Impulse runtime alignment sprint (Stage 3.2)

## Evidence Summary

- Baseline p@k: 0.0667
- PageIndex local p@k: 0.0000
- Precision uplift (%): -100.0%
- Baseline MRR / nDCG: 0.2247 / 0.1290
- PageIndex MRR / nDCG: 0.0540 / 0.0000
- Baseline p95/p99 latency (ms): 34.24 / 34.24
- PageIndex p95/p99 latency (ms): 27.31 / 27.31
- Error/fallback rate: 0.0

Source report: `docs/research/pageindex-feasibility-report.json`

## Recommendation

### If `NO-GO`
- Keep PageIndex in research-only track.
- Revisit when stronger labeled query set exists or retrieval quality target changes.
- Current benchmark does not meet the >=10% precision@k uplift gate and materially underperforms baseline quality.

### If `GO-OPTIONAL`
- Add plugin adapter behind feature flag.
- Keep default backend unchanged.
- Require explicit docs and operator guardrails.

### If `GO-LIMITED`
- Restrict to docs-heavy/structured corpora only.
- Exclude session history path by default.

## Follow-up Tasks

- [x] Update `docs/spec/RUST-CANONICAL-CONTRACT.md` if decision changes runtime contract.
- [x] Update `AGENTS.md`, `CLAUDE.md`, and `docs/INDEX.md` references.
- [x] Add or adjust regression/perf tests for selected path.
