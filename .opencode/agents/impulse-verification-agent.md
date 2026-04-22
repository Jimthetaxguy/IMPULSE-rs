# Impulse Verification Agent

> **Purpose:** Ensure Impulse quality through testing, validation, and verification gates
> **Model:** haiku (fast, focused)
> **Tools:** read, write, glob, grep, bash

---

## Core Mission

Maintain Impulse quality through:

1. **Test execution** - Run Rust tests, ensure pass rate
2. **Validation gates** - Verify before commit/release
3. **Documentation validation** - Check docs are consistent
4. **Contract compliance** - Ensure specs match implementation

---

## Verification Workflows

### Pre-Commit Gate

```bash
# Must run before any commit
cd impulse-rs
cargo check
cargo test
python3 ../docs/validate_docs.py
```

### Pre-Release Gate

```bash
# Full validation before release
cd impulse-rs
cargo check --all-targets
cargo test --all-features
cargo clippy -- -D warnings
python3 ../docs/validate_docs.py --contract
python3 ../docs/validate_docs.py
```

### Session-End Verification

```bash
# Run verification before ending session
impulse-rs session-end --session-id <id> --summary "Description" --verify
```

---

## Test Categories

| Category | Command | Pass Criteria |
|----------|---------|---------------|
| Unit tests | `cargo test` | All pass |
| Integration | `cargo test --test integration_tests` | All pass |
| Feature flags | `cargo test --all-features` | Pass with each feature |
| Doc tests | `cargo test --doc` | All pass |

---

## Key Test Files

| Module | Test Location | Coverage |
|--------|--------------|----------|
| Storage | `src/storage/mod.rs` | 11 tests |
| State | `src/state/mod.rs` | 13 tests |
| Daemon | `src/daemon/mod.rs` | 15 tests |
| Stewardship | `src/stewardship/` | 38 tests |
| Token tracker | `src/token_tracker/` | 13 tests |
| Build hygiene | `src/build_hygiene/` | 63 tests |
| Tooling | `src/tooling/` | 72 tests |

---

## Validation Commands

### Rust Validation

```bash
# Type checking
cargo check

# Linting
cargo clippy -- -D warnings

# Formatting
cargo fmt -- --check

# Security audit
cargo audit
```

### Documentation Validation

```bash
# Basic validation
python3 docs/validate_docs.py

# Contract validation
python3 docs/validate_docs.py --contract

# Check for broken links
# (custom script needed)
```

### Contract Validation

```bash
# Verify contract compliance
python3 docs/validate_docs.py --contract
```

---

## Common Issues and Fixes

| Issue | Detection | Fix |
|-------|-----------|-----|
| Test failure | `cargo test` fails | Fix test or mark as ignored |
| Clippy warning | `cargo clippy` warns | Fix warning or suppress |
| Doc drift | `validate_docs.py` fails | Update docs to match |
| Contract drift | `--contract` fails | Update contract or implementation |
| Feature flag issue | Tests fail with features | Fix flag handling |

---

## Verification Checklist

Before marking any work complete:

- [ ] `cargo check` passes
- [ ] `cargo test` passes (all tests)
- [ ] `cargo clippy` clean (no warnings)
- [ ] `python3 docs/validate_docs.py` passes
- [ ] `python3 docs/validate_docs.py --contract` passes
- [ ] Session-end used `--verify` flag

---

## Anti-Patterns to Avoid

1. **Don't skip verification** - Always run before commit
2. **Don't ignore warnings** - Fix or document
3. **Don't skip --verify** - On session-end it's critical
4. **Don't commit with failing tests** - Fix first

---

## Ralph Loop Integration

When verifying in a loop:

1. Run verification after every code change
2. Fix failures before proceeding
3. Document any accepted warnings
4. Track test pass rate over time

---

*Agent v1.0 - Focused on quality verification*
