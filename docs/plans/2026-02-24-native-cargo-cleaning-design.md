# Native Cargo Cleaning — Design Doc

> **Date:** 2026-02-24
> **Status:** Approved
> **Goal:** Remove external tool dependencies (cargo-sweep, cargo-wipe) from build_hygiene module

## Problem

The build_hygiene module delegates all cleaning to external cargo tools. If `cargo-sweep` or `cargo-wipe` aren't installed, the commands bail with an error. The only self-contained path is `clean_all_manual()` which runs `cargo clean` per project.

## Solution: Pure Filesystem Walk

Implement native Rust artifact cleaning using `std::fs` — no external binaries required.

### New File: `native.rs`

- `native_sweep(path, max_age_days, dry_run) -> CleanResult` — walk target/, delete stale files by mtime
- `native_wipe(path, dry_run) -> CleanResult` — remove entire target/ directories
- `walk_and_collect_stale(target_dir, cutoff) -> Vec<StaleFile>` — identify stale files with sizes
- `remove_empty_parents(dir, stop_at)` — clean up empty dirs after file removal

### Changes to Existing Files

- `sweep.rs`: Fall through to `native_sweep()` when cargo-sweep missing (instead of bail)
- `wipe.rs`: Fall through to `native_wipe()` when cargo-wipe missing (instead of bail)
- `mod.rs`: Add `pub mod native;` declaration
- `tests.rs`: Add native operation tests with tempdir fixtures

### Algorithm

**Stale sweep:**
1. Walk target/ recursively
2. For each file, check `metadata().modified()`
3. If mtime older than cutoff (`now - max_age_days`), mark for removal
4. In dry-run: report sizes. In live: delete files, track freed bytes
5. After deletions, remove empty directories bottom-up

**Wipe:**
1. Use existing `discover_rust_projects()` to find projects
2. For each project, measure target/ size (already done by discovery)
3. In dry-run: report. In live: `remove_dir_all(target/)`

### Safety

- Dry-run defaults preserved (existing behavior)
- Permission errors collected as non-fatal errors in CleanResult
- Symlinks: do not follow symlinks out of target/ (use `symlink_metadata`)
- No `unwrap()` on production paths
