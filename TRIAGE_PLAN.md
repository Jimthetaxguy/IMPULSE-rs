# impulse-rs Triage Plan

**Date:** 2026-04-22
**Author:** Claude (on behalf of James Pustorino)
**Status:** EXECUTING

---

## 1. Current State Inventory

### Overview

| Metric | Value |
|---|---|
| Total folder size | ~44 GB |
| Source code size (no build artifacts) | ~4.2 MB |
| Build artifact size | ~45 GB |
| .rs files (excluding targets) | 240 |
| Workspace crates | 4 (impulse-rs, impulse-ops, impulse-term, impulse-gui) |
| Version control | **NONE** — no .git directory |
| Location | `~/Desktop/VibeCode_Prime/CLI_CU_L8R/impulse-rs` |

### Source Code Breakdown

| Directory | .rs files | Purpose |
|---|---|---|
| `src/` (+ subdirs) | ~160 | Core CLI — handlers, tools, agent, memory, retrieval, etc. |
| `impulse-gui/src/` | ~38 | GUI frontend (views, widgets, agent panel) |
| `impulse-term/src/` | 8 | Terminal UI crate |
| `impulse-gui-legacy-adapter/src/` | 6 | Legacy GUI bridge |
| `impulse-ops/` | ~5 | Ops tooling crate |
| `tests/` | 4 | Integration tests |

### Source Modules (src/)

The core binary has 33 modules covering: handlers (19 files), tooling/builtin (14), UI (13), context_lifecycle (9), build_hygiene (9), tools (8), retrieval (8), stewardship (7), monty (7), token_tracker (6), tooling (6), office (5), guardrail (5), docs (4), state (4), plugin (4), injection (4), daemon (4), delegation (4), credentials (4), agent (4), agent_discovery, mcp, memory, notification, orchestration, semantic_diff, storage, client, llm_backends, verify.

### Build Artifacts (THE PROBLEM)

| Path | Size | Contents |
|---|---|---|
| `target/` | 23 GB | Primary build dir — debug deps (14 GB), incremental cache (6.7 GB), release (900 MB) |
| `target.old/` | 18 GB | Previous target backup — fully stale |
| `target2/` | 2.3 GB | Another target backup — fully stale |
| `impulse-gui/target/` | 1.7 GB | GUI crate's own target (should use workspace target via .cargo/config.toml) |
| `Impulse-0.1.0-macos-arm64.dmg` | 11 MB | macOS installer disk image |
| `Impulse.app/` | 21 MB | macOS app bundle |
| **Total artifacts** | **~45 GB** | |

### Configuration & Metadata Files

| Path | Size | Notes |
|---|---|---|
| `.cargo/config.toml` | < 1 KB | Build config (shared target dir) |
| `.claude/hooks/hooks.json` | < 1 KB | Claude hooks config |
| `.impulse/` | ~120 KB | Runtime state — config, retrieval DB, history, sessions |
| `.opencode/` | ~5 files | Agent definitions for opencode integration |
| `.gitignore` | 62 bytes | Already covers target/, target.old/, target2/, .superpowers/, Impulse.app/, *.dmg |
| `Cargo.toml` | 2.8 KB | Workspace root manifest |
| `Cargo.lock` | 193 KB | Current dependency lock |
| `Cargo 2.lock` | 123 KB | Old duplicate lock file — stale |

### Documentation & Supporting Files

| Path | Size | Notes |
|---|---|---|
| `docs/` | 240 KB | Specs, plans, design docs (15 files) |
| `README.md` | 3.5 KB | Project readme |
| `QUICKSTART.md` | 2.6 KB | Quick start guide |
| `PLATFORMS.md` | 4.3 KB | Platform support doc |
| `LICENSE` | 11 KB | MIT license |
| `NOTICE` | 395 B | Attribution notice |
| `scripts/build-macos-app.sh` | < 4 KB | macOS app build script |
| `impulse.sh` | 1 KB | Shell helper |
| `json-reports/` | < 4 KB | Phase 4 report |
| `yaml-docs/` | < 8 KB | Verification/merger docs |

### Archive & Stale Items

| Path | Size | Notes |
|---|---|---|
| `_archive-2026-04-22-L104/` | 2.1 MB | Archived Tauri shell UI + build artifacts from old GUI attempt |
| `Cargo 2.lock` | 123 KB | Duplicate/stale lock file |
| `impulse-ops 2/` | 0 (empty) | Empty duplicate directory |
| `.superpowers/` | unknown | Already gitignored |
| `.DS_Store` (multiple) | ~16 KB | macOS metadata junk |

---

## 2. What to KEEP

### Critical Source (MUST preserve)

- `src/` — all 160+ .rs files, the entire core binary
- `impulse-gui/src/` — GUI frontend source
- `impulse-term/src/` — terminal UI source
- `impulse-gui-legacy-adapter/src/` — legacy adapter source
- `impulse-ops/` — ops crate source (but NOT `impulse-ops 2/` which is empty)
- `tests/` — integration tests

### Build Configuration

- `Cargo.toml` (root workspace manifest)
- `Cargo.lock` (current, pinned dependencies)
- `impulse-gui/Cargo.toml`
- `impulse-term/Cargo.toml`
- `impulse-ops/Cargo.toml`
- `impulse-gui-legacy-adapter/Cargo.toml`
- `.cargo/config.toml` (shared target dir config)

### Documentation

- `README.md`, `QUICKSTART.md`, `PLATFORMS.md`
- `LICENSE`, `NOTICE`
- `docs/` (all specs, plans, design docs)

### Project Metadata

- `.gitignore` (will be updated)
- `.claude/` (hooks config)
- `.impulse/` (runtime state, config, retrieval DB)
- `.opencode/` (agent definitions)
- `scripts/build-macos-app.sh`
- `impulse.sh`
- `json-reports/`, `yaml-docs/`
- `_archive-2026-04-22-L104/` (keep for now — only 2.1 MB, has old Tauri reference code)

### To Remove Later (low priority, keep for initial commit)

- `Cargo 2.lock` — stale duplicate, but harmless (123 KB)
- `impulse-ops 2/` — empty directory
- `.DS_Store` files — macOS junk

---

## 3. What to DELETE

### Build Artifacts — ~45 GB total

| # | Path | Size | Reason |
|---|---|---|---|
| 1 | `target/` | 23 GB | Primary build directory — fully reproducible from source via `cargo build` |
| 2 | `target.old/` | 18 GB | Stale backup of previous target — zero value |
| 3 | `target2/` | 2.3 GB | Another stale target backup — zero value |
| 4 | `impulse-gui/target/` | 1.7 GB | GUI crate build dir — reproducible; .cargo/config.toml should route this to workspace target |
| 5 | `Impulse-0.1.0-macos-arm64.dmg` | 11 MB | Built installer — reproducible via `scripts/build-macos-app.sh` |
| 6 | `Impulse.app/` | 21 MB | Built app bundle — reproducible |

**Total reclaimed: ~45 GB**

---

## 4. Backup Steps (Execute BEFORE deleting anything)

### Step 1: Initialize Git

```bash
cd ~/Desktop/VibeCode_Prime/CLI_CU_L8R/impulse-rs
git init
```

### Step 2: Update .gitignore

Replace existing `.gitignore` with comprehensive version covering all build artifacts, IDE files, macOS junk, and runtime state that shouldn't be tracked:

```
# Build artifacts
target/
target.old/
target2/

# macOS app bundles & installers
Impulse.app/
*.dmg

# Runtime state (regenerated on run)
.impulse/retrieval.db
.impulse/SESSIONS.json
.impulse/LIVE_STATE.json
.impulse/sockets/

# IDE / editor
.vscode/
.idea/
*.swp
*.swo
*~

# macOS
.DS_Store
**/.DS_Store

# Misc
.superpowers/
```

### Step 3: Stage and Commit

```bash
git add -A
git commit -m "Initial commit: impulse-rs source code (240 .rs files, 4 workspace crates)

Preserves all source, configs, docs, and project metadata.
Build artifacts excluded via .gitignore (~45 GB not tracked)."
```

### Step 4: Verify Commit

```bash
git log --oneline
git diff --stat HEAD  # should be empty
git status            # should be clean
```

---

## 5. Cleanup Execution Order

**IMPORTANT: Steps must be executed in this exact order. Backup (git commit) MUST complete before any deletion.**

1. `git init` — create repository
2. Write updated `.gitignore`
3. `git add -A && git commit` — commit all source
4. Verify commit is clean
5. **BEGIN DELETIONS** (only after step 4 succeeds):
   - `rm -rf target/` (23 GB)
   - `rm -rf target.old/` (18 GB)
   - `rm -rf target2/` (2.3 GB)
   - `rm -rf impulse-gui/target/` (1.7 GB)
   - `rm -rf Impulse.app/` (21 MB)
   - `rm Impulse-0.1.0-macos-arm64.dmg` (11 MB)
6. Verify final state — `du -sh .` should show < 20 MB
7. Run `git status` — should still be clean (all deleted items were gitignored)

---

## 6. Future Recommendations

### Immediate (this session)

- [x] Initialize git and commit source
- [x] Delete ~45 GB of build artifacts
- [ ] Push to GitHub as private repo for offsite backup

### Short-term

- **Push to GitHub:** `git remote add origin git@github.com:jamespustorino/impulse-rs.git && git push -u origin main`
- **Clean up stale files:** Remove `Cargo 2.lock`, empty `impulse-ops 2/`, scattered `.DS_Store` files
- **Audit commands:** Previous analysis found ~70 CLI commands; consolidate to 10-15 essential ones
- **Consider workspace consolidation:** `impulse-gui/target/` existed because something bypassed `.cargo/config.toml` — investigate

### Medium-term

- **CI/CD:** Add GitHub Actions for `cargo check`, `cargo test`, `cargo clippy`
- **Release automation:** Automate DMG/app bundle builds via CI instead of local `scripts/build-macos-app.sh`
- **Dependency audit:** Run `cargo audit` to check for known vulnerabilities
- **Feature flag cleanup:** Evaluate if `monty-support` and `datafusion-support` features are still needed

### Long-term

- **Command consolidation:** Reduce from ~70 commands to 10-15 (per prior audit). Group related functionality behind subcommands
- **Crate restructuring:** Consider whether 4 workspace crates is the right split, or if `impulse-gui-legacy-adapter` can be retired
- **Archive policy:** The `_archive-2026-04-22-L104/` directory (old Tauri shell) is only 2.1 MB — decide whether to keep in-tree or move to a separate branch

---

## Appendix: Expected Final State

After cleanup, the folder should contain approximately:

```
impulse-rs/           (~10-15 MB total)
├── .git/             (~3-5 MB — initial commit)
├── .cargo/
├── .claude/
├── .gitignore
├── .impulse/         (~120 KB runtime state)
├── .opencode/
├── _archive-.../     (2.1 MB)
├── Cargo.toml
├── Cargo.lock
├── Cargo 2.lock      (stale, clean up later)
├── docs/             (240 KB)
├── impulse-gui/      (source only, no target/)
├── impulse-gui-legacy-adapter/
├── impulse-ops/
├── impulse-term/
├── json-reports/
├── LICENSE
├── NOTICE
├── PLATFORMS.md
├── QUICKSTART.md
├── README.md
├── scripts/
├── src/              (2.6 MB — the core)
├── tests/
├── TRIAGE_PLAN.md    (this file)
└── yaml-docs/
```

**Disk savings: ~44 GB → ~15 MB (99.97% reduction)**
