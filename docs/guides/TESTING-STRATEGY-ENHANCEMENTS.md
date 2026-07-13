---
title: Testing Strategy - Hooks, Semantic Search, and Conflict Resolution
description: Test coverage plan for Phase 1-3 enhancements
version: '1.0'
updated: 2026-03-02
type: doc
category: testing
status: superseded
audience: builder
tags: [testing, strategy, implementation]
---

# Testing Strategy — Hooks, Search, and Conflict Resolution

> **Historical phase plan — superseded.** Use
> [`../spec/TEST-TRACEABILITY.md`](../spec/TEST-TRACEABILITY.md) for the current Rust test map and
> evidence gaps.

> **Generated:** 2026-03-02 | **Purpose:** Define test coverage for Phase 1-3 enhancements

---

## Test Pyramid

```
        ┌─────────────┐
        │ Integration │
        │   Tests    │
        ├─────────────┤
        │  Unit      │
        │   Tests    │
        ├─────────────┤
        │   Fast     │
        │  Tests     │
        └─────────────┘
```

---

## Phase 1: Hook Enhancement Tests

### Unit Tests (guardrail/)

| Test | Purpose | Location |
|------|---------|----------|
| `test_should_block_returns_true_for_block_action` | Verify block detection | `guardrail/types.rs` |
| `test_should_block_returns_false_for_warn` | Verify warn doesn't block | `guardrail/types.rs` |
| `test_guard_config_loads_from_file` | Config file loading | `guardrail/config.rs` |
| `test_merge_rules_precedence` | CLI > file > defaults | `guardrail/config.rs` |
| `test_block_flag_exits_with_code_1` | CLI exits correctly | `main.rs` tests |

### Integration Tests

| Test | Purpose | Location |
|------|---------|----------|
| `test_hook_blocks_conflicting_write` | Full blocking flow | `integration_tests.rs` |
| `test_hook_allows_non_conflicting` | Non-blocked passes | `integration_tests.rs` |
| `test_guard_config_file_versioning` | Version handling | `integration_tests.rs` |

### Test Commands

```bash
# Run guardrail unit tests
cargo test guardrail

# Run hook integration tests
cargo test hooks
```

---

## Phase 2: Semantic Search Tests

### Unit Tests (retrieval/)

| Test | Purpose | Location |
|------|---------|----------|
| `test_cache_get_returns_cached` | Cache hit | `retrieval/embedding.rs` |
| `test_cache_evicts_lru` | LRU eviction | `retrieval/embedding.rs` |
| `test_cache_ttl_expiration` | TTL expires | `retrieval/embedding.rs` |
| `test_fallback_reason_logged` | Fallback tracking | `retrieval/query.rs` |
| `test_batch_embedding_single_call` | Batch works | `retrieval/embedding.rs` |

### Integration Tests

| Test | Purpose | Location |
|------|---------|----------|
| `test_semantic_fallback_without_vec` | sqlite-vec missing | `integration_tests.rs` |
| `test_semantic_fallback_without_python` | Python missing | `integration_tests.rs` |
| `test_cache_improves_performance` | Cache speedup | Manual benchmark |
| `test_batch_vs_sequential_timing` | Batch efficiency | Manual benchmark |

### Fallback Test Matrix

| Backend | Available | Fallback To | Test |
|---------|-----------|-------------|------|
| sqlite-vec | Yes | — | Primary path |
| sqlite-vec | No | rust-cosine | `test_fallback_to_rust_cosine` |
| rust-cosine | No | keyword | `test_fallback_to_keyword` |
| python | No | rust-cosine | `test_fallback_no_python` |

### Test Commands

```bash
# Run retrieval tests
cargo test retrieval

# Run with coverage
cargo test retrieval -- --nocapture
```

---

## Phase 3: Conflict Resolution Tests

### Unit Tests (agent/coordinator.rs)

| Test | Purpose | Location |
|------|---------|----------|
| `test_detect_file_conflicts_single` | One file, one conflict | `coordinator.rs` |
| `test_detect_file_conflicts_none` | No conflicts | `coordinator.rs` |
| `test_detect_file_conflicts_multiple` | Multiple files | `coordinator.rs` |
| `test_blocking_info_included` | Blocking data returned | `coordinator.rs` |

### Integration Tests

| Test | Purpose | Location |
|------|---------|----------|
| `test_hook_blocks_conflict_enabled` | Blocking ON works | `integration_tests.rs` |
| `test_hook_ignores_conflict_disabled` | Blocking OFF passes | `integration_tests.rs` |
| `test_tui_conflict_banner_shows` | GUI shows banner | Manual test |
| `test_handoff_doc_created` | Auto-handoff works | `integration_tests.rs` |

### Test Commands

```bash
# Run coordinator tests
cargo test coordinator

# Run conflict tests specifically
cargo test conflict
```

---

## Test Fixtures

### Mock GuardConfig

```rust
fn mock_guard_config() -> GuardConfig {
    GuardConfig {
        version: 1,
        enabled: true,
        conflict_check: true,
        conflict_blocking_enabled: false,
        rules: vec![
            GuardRule {
                id: "test-block".to_string(),
                pattern: "dangerous".to_string(),
                action: GuardAction::Block,
                target: GuardTarget::Bash,
                reason: "Test rule".to_string(),
                suggestion: None,
                enabled: true,
            }
        ],
    }
}
```

### Mock Session with active_files

```rust
fn mock_session_with_files() -> Session {
    Session {
        id: "test-session".to_string(),
        active_files: vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
        ],
        ..Default::default()
    }
}
```

### Mock EmbeddingCache

```rust
fn mock_cache() -> EmbeddingCache {
    EmbeddingCache::new(100, Duration::from_secs(300))
}
```

---

## Running Tests

### All Tests

```bash
cargo test
```

### By Phase

```bash
# Phase 1: Hooks
cargo test guardrail

# Phase 2: Search
cargo test retrieval

# Phase 3: Conflicts
cargo test coordinator
```

### Integration Only

```bash
cargo test integration_tests
```

### With Output

```bash
cargo test -- --nocapture
```

---

## Coverage Targets

| Module | Current | Target |
|--------|---------|--------|
| guardrail/ | 85% | 90% |
| retrieval/ | 80% | 85% |
| coordinator | 85% | 90% |

---

## Flaky Test Handling

### Known Flaky Tests

| Test | Issue | Mitigation |
|------|-------|-------------|
| `embedding_subprocess_env_propagation` | Race condition | Ignore in CI, test fallback |
| `semantic_search_timing` | Environmental variance | Use --mean instead of exact |

### Running Flaky Tests

```bash
# Run with retries
cargo test -- --test-threads=1

# Run specific flaky test
cargo test test_name -- --ignored
```

---

## Related Documents

- [IMPLEMENTATION-HANDOFF.md](../plans/IMPLEMENTATION-HANDOFF.md)
- [TESTING-FRAMEWORK.md](./TESTING-FRAMEWORK.md)

---

*Generated: 2026-03-02*
