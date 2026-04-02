---
title: Tool Status
description: Historical tooling status note retained for context after the Rust-first product reset
version: '1.1'
updated: 2026-04-02
type: guide
category: tooling
phase: historical
status: deprecated
audience: builder
tags: [tools, setup, historical]
---

# Tool Validation Status — Historical Reference

> **Date:** 2026-02-20
> **Status:** Deprecated
> **Important:** This document reflects an earlier TypeScript/Bun-era setup contract and should not be used as the current Impulse environment baseline.
> For the live Rust-first contract, start with [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md), [`../spec/USER-STORY-MAP.md`](../spec/USER-STORY-MAP.md), and [`../../AGENTS.md`](../../AGENTS.md).

---

## Tool Inventory

### ✓ Already Available

| Tool | Version | Status |
|------|---------|--------|
| Bun | 1.3.4 | ✓ (Runtime for TypeScript harness) |
| Rust | 1.92.0 | ✓ (Zellij plugins) |
| sqlite3 | 3.51.0 | ✓ (Database) |
| wasm32-wasip1 | Latest | ✓ (Cargo target for WASM) |
| sentence-transformers | Latest | ✓ (Python embeddings) |

### ✗ Missing (Installation Required)

| Tool | Why | Install Command |
|------|-----|-----------------|
| Zellij | ≥0.42 session manager | `brew install zellij` or `cargo install zellij` |
| Ghostty | GPU terminal emulator | `brew install ghostty` |
| Python 3.12 | Memory pipeline runtime | `pyenv install 3.12 && pyenv local 3.12` |
| sqlite-vec | Python SQLite extension | `pip install sqlite-vec` |
| mem0ai | Fact extraction | `pip install mem0ai` |

### ⚠ Needs Attention

| Issue | Current | Target | Action |
|-------|---------|--------|--------|
| Python version | 3.9.6 | 3.12 | Use pyenv or create venv |
| sqlite-vec Python | Not installed | Latest | `pip install sqlite-vec` |
| mem0ai Python | Not installed | Latest | `pip install mem0ai` |

---

## Installation Guide

### macOS (Homebrew)

```bash
# Terminal infrastructure
brew install zellij
brew install ghostty

# Python 3.12 (via pyenv)
brew install pyenv
pyenv install 3.12.0
pyenv local 3.12.0  # Set for this project

# Python packages
pip install sqlite-vec mem0ai
```

### Linux (Ubuntu/Debian)

```bash
# Zellij
cargo install zellij

# Python 3.12
sudo apt install python3.12 python3.12-venv

# Python packages
python3.12 -m pip install sqlite-vec mem0ai
```

### Windows (WSL2)

```bash
# Within WSL2 Ubuntu
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install zellij

# Python 3.12
sudo apt install python3.12

# Python packages
python3.12 -m pip install sqlite-vec mem0ai
```

---

## Validation Script

Run after installing missing tools:

```bash
cd <project-root>
mise tasks validate-tools
mise tasks test-basic
```

---

## Acceptance Criteria for Phase 0 Completion

- [ ] Zellij ≥0.42 installed and verified
- [ ] Bun ≥1.0 available (already ✓)
- [ ] Rust ≥1.75 with wasm32-wasip1 target (already ✓)
- [ ] Python 3.12 available
- [ ] sqlite-vec Python package installed
- [ ] mem0ai Python package installed
- [ ] All validation scripts pass
- [ ] SPEC-v1.1.md, ARCHITECTURE.md, STEWARD.md, DATA-MODELS.md, BENCHMARKS.md created ✓
- [ ] 3 ADRs written ✓
- [ ] RESEARCH-INDEX.md created (pending)

---

## Estimated Time to Ready

- Zellij + Ghostty: 5 min (brew install)
- Python 3.12 setup: 10-15 min (pyenv)
- Python packages: 2-3 min (pip install)
- **Total: 20-25 minutes**
