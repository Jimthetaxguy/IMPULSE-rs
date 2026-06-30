---
title: EGUI Decommission Plan
description: Phased, gated removal of the legacy egui/eframe desktop surface (impulse-gui + impulse-term egui rendering) once the Dioxus desktop host is operationally authoritative
version: '1.0'
type: doc
category: roadmap
phase: all
status: active
updated: 2026-06-30
audience: builder
tags: [roadmap, decommission, egui, dioxus, desktop, cleanup]
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# EGUI Decommission Plan — Impulse

> **For agentic workers:** This is a phased *removal* plan, not a feature build. Each phase ends with the verification gate (`cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`) staying green and the doc validator (`python3 docs/validate_docs.py --all`) passing. Steps use checkbox (`- [ ]`) syntax. Do not start a phase until its gate is satisfied.

**Goal:** Fully remove the legacy egui/eframe desktop surface from the active workspace, retiring ~15K lines of frozen `impulse-gui` plus the egui-coupled rendering layer in `impulse-term`, once the Dioxus desktop host reaches operational parity.

**Architecture:** The egui surface is *layered*, not a single crate. Removal is bottom-gated: `impulse-gui` (the frozen workbench binary) is the only consumer of `impulse-term`'s `egui` feature; the Dioxus host uses an xterm.js bridge and the ratatui TUI uses its own renderer, so neither depends on egui. Once `impulse-gui` is gone, `impulse-term`'s `egui` feature and the eframe-based modules become dead and can be deleted, leaving a lean PTY/vt100 + ratatui core.

**Tech Stack:** Rust workspace (edition 2021, rust 1.82+), eframe 0.31 (to be removed), Dioxus desktop host (replacement), ratatui TUI (retained).

## Global Constraints

- Verification gate is non-negotiable per phase: `cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`.
- Doc validator must pass after any doc change: `python3 docs/validate_docs.py --all`.
- Canonical contract wins on conflict: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md). The contract's roadmap marker line ("Legacy=egui compile-maintenance only") may only be updated to reflect *removed* status by the same change that physically removes the code — never ahead of it.
- Archive-don't-delete: physical removal of `impulse-gui/` archives the tree to a timestamped tarball before `git rm`. No `rm -rf` of source.
- `impulse-gui` is already `exclude`d from the workspace (`impulse-rs/Cargo.toml`), so it is not built today — this plan removes the dead weight, it does not change default build behavior.

---

## Current State (verified 2026-06-30)

| Layer | Location | Status | LOC |
|------|----------|--------|-----|
| Frozen workbench binary | `impulse-rs/impulse-gui/` (38 files) | `exclude`d from workspace; compile-maintenance only | ~15K |
| egui rendering modules | `impulse-term/src/{renderer,panel,status_bar,input}.rs` + egui types in `theme.rs` | Built only under `impulse-term`'s `egui` feature (`default = ["egui"]`) | part of ~4K |
| eframe dependency | `impulse-term/Cargo.toml` (`eframe = { version = "0.31", optional = true }`) | Pulled in by default feature | — |
| macOS bundler | `impulse-rs/scripts/build-macos-app.sh` | Bundles the `impulse-gui` binary | — |
| Replacement | `impulse-rs/impulse-desktop/` (Dioxus host) | Scaffold + bridge parity complete; real launch bridge still stubbed (see gate below) | ~7.8K |

**Consumer check:** `impulse-term`'s `egui` feature is consumed only by `impulse-gui`. The Dioxus host (`impulse-desktop`) and the ratatui TUI (in `impulse-rs`) do not depend on the `egui` feature. This is what makes bottom-up removal safe.

---

## Phase 0 — Parity Gate (BLOCKING; do not remove anything until satisfied)

**Files:** read-only verification — `docs/plans/worktrees/2026-06-14-codex-dioxus-native-host.md`, `docs/ROADMAP-PLAN.md`.

Removal is gated on the Dioxus host being *operationally authoritative*, not merely scaffold-complete. The host work card's "Next Cleanup Queue" still lists a Shark-priority item: the real desktop launcher installs a manifest-only `window.__IMPULSE_DESKTOP_HOST` and the host smoke still stubs working invoke/listen in Playwright.

- [ ] **Step 1: Confirm the Dioxus launch bridge is real, not stubbed**

Run: `cd impulse-rs && CARGO_TARGET_DIR=/tmp/impulse-gate-target npm run dioxus:host:smoke`
Expected: smoke asserts a live eval/message bridge routing to `DesktopRuntime` (not a Playwright stub). If it still stubs invoke/listen, STOP — Phase 0 is not met.

- [ ] **Step 2: Confirm no egui consumer remains except impulse-gui**

Run: `cd impulse-rs && grep -rln "impulse-term/egui\|features.*egui\|eframe" --include=Cargo.toml . | grep -v impulse-gui`
Expected: only `impulse-term/Cargo.toml` (defining the feature) appears — no other crate enables it.

- [ ] **Step 3: Record the parity decision**

Append a one-line decision to `docs/ROADMAP-PLAN.md` "Later" stage and to this plan: "Phase 0 met YYYY-MM-DD — Dioxus host operationally authoritative; egui removal unblocked." Commit.

```bash
git add docs/ROADMAP-PLAN.md docs/plans/EGUI-DECOMMISSION.md
git commit -m "docs(decommission): record egui-removal parity gate met"
```

---

## Phase 1 — Remove the frozen `impulse-gui` crate

**Files:**
- Archive then remove: `impulse-rs/impulse-gui/` (whole tree, 38 files, ~15K LOC)
- Modify: `impulse-rs/Cargo.toml:8` (drop `exclude = ["impulse-gui"]`)
- Modify: `impulse-rs/scripts/build-macos-app.sh` (it bundles the `impulse-gui` binary)

**Interfaces:**
- Consumes: nothing — `impulse-gui` is a leaf binary, no workspace crate depends on it.
- Produces: a workspace with no egui *application*; `impulse-term`'s egui feature is now unconsumed (handled in Phase 2).

- [ ] **Step 1: Archive the crate (insurance before removal)**

Run:
```bash
cd /Users/jamespustorino/code/IMPULSE-rs && \
  tar -czf ~/code/_archive/$(date +%Y-%m-%d)-impulse-gui-frozen.tar.gz impulse-rs/impulse-gui
```
Expected: tarball created (~few MB). Verify with `tar -tzf <tarball> | head`.

- [ ] **Step 2: Decide the macOS bundler's fate**

`build-macos-app.sh` bundles `impulse-gui`. Either (a) repoint it to the Dioxus desktop binary if that is the shipping app, or (b) remove the script if macOS bundling now lives in `impulse-desktop`. Inspect first:

Run: `cd impulse-rs && grep -n "impulse-gui\|impulse-desktop\|cargo-bundle\|dx bundle" scripts/build-macos-app.sh`
Expected: lines 14/18/19/68/69/74/78 reference `impulse-gui`. Replace those with the Dioxus bundling path, or `git rm scripts/build-macos-app.sh` if `impulse-desktop` owns bundling.

- [ ] **Step 3: Remove the crate from the workspace and tree**

```bash
cd impulse-rs && git rm -r impulse-gui
```
Then edit `impulse-rs/Cargo.toml` to delete the line `exclude = ["impulse-gui"]`.

- [ ] **Step 4: Run the verification gate**

Run: `cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: PASS — test count unchanged (impulse-gui was excluded, its tests never counted), no new warnings.

- [ ] **Step 5: Commit**

```bash
git add impulse-rs/Cargo.toml impulse-rs/scripts/build-macos-app.sh
git commit -m "chore(decommission): remove frozen impulse-gui crate + macOS bundler refs"
```

---

## Phase 2 — Strip the egui rendering layer from `impulse-term`

After Phase 1, `impulse-term`'s `egui` feature has no consumer. Remove it so the crate is a pure PTY/vt100 core.

**Files:**
- Remove: `impulse-rs/impulse-term/src/{renderer,panel,status_bar,input}.rs` (egui widgets)
- Modify: `impulse-rs/impulse-term/src/theme.rs` (strip `egui::Color32` conversions; keep RGB triplet model)
- Modify: `impulse-rs/impulse-term/src/lib.rs` (drop `#[cfg(feature = "egui")]` module gates)
- Modify: `impulse-rs/impulse-term/Cargo.toml` (delete `egui` feature + `eframe` optional dep; drop `default = ["egui"]`)
- Modify/Remove: `impulse-rs/impulse-term/tests/boundary_tests.rs` (egui-feature-gated assertions)

**Interfaces:**
- Consumes: nothing egui-related after Phase 1.
- Produces: `impulse-term` with no eframe dependency; `theme.rs` exposes RGB triplets (`[u8; 3]`) instead of `egui::Color32`.

- [ ] **Step 1: Confirm no live consumer of the egui modules**

Run: `cd impulse-rs && grep -rln "impulse_term::\(renderer\|panel\|status_bar\)\|TerminalPanel\|TerminalRenderer" --include=*.rs . | grep -v impulse-term/`
Expected: no matches (the Dioxus host and ratatui TUI do not use these). If matches appear, STOP and reassess — a non-egui consumer exists.

- [ ] **Step 2: Remove the egui widget modules**

```bash
cd impulse-rs && git rm impulse-term/src/renderer.rs impulse-term/src/panel.rs impulse-term/src/status_bar.rs impulse-term/src/input.rs
```

- [ ] **Step 3: De-egui `theme.rs` and `lib.rs`**

In `theme.rs`, replace `egui::Color32` return types with the existing serializable RGB representation (`[u8; 3]` / the `ThemeColors` triplet model already present for serialization). In `lib.rs`, delete every `#[cfg(feature = "egui")]` block and the `use eframe::egui;` re-exports.

- [ ] **Step 4: Drop the dependency in Cargo.toml**

Edit `impulse-rs/impulse-term/Cargo.toml`: remove `default = ["egui"]` (set `default = []`), the `egui = ["dep:eframe"]` feature line, and `eframe = { version = "0.31", optional = true }`.

- [ ] **Step 5: Run the verification gate**

Run: `cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: PASS. `impulse-term` test count drops (egui-gated tests in `input.rs`/`theme.rs` removed); update the canonical counts in Step 6's commit. Confirm `cargo tree -p impulse-term | grep eframe` returns nothing.

- [ ] **Step 6: Update canonical counts + commit**

Update the workspace test totals in `CLAUDE.md`, `AGENTS.md`, `HANDBOOK.md`, and `docs/spec/RUST-CANONICAL-CONTRACT.md` to the new post-removal numbers, then:

```bash
git add impulse-rs/impulse-term CLAUDE.md AGENTS.md HANDBOOK.md docs/spec/RUST-CANONICAL-CONTRACT.md
git commit -m "chore(decommission): drop eframe/egui rendering layer from impulse-term"
```

---

## Phase 3 — Scrub egui from active docs + contract

**Files:**
- Modify: `AGENTS.md` ("Desktop Shell Status", "Architecture", "egui imports" convention row, Roadmap contract marker)
- Modify: `CLAUDE.md` (Roadmap contract marker)
- Modify: `docs/spec/RUST-CANONICAL-CONTRACT.md`, `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
- Modify: `docs/INDEX.md`, `docs/SUMMARY.md`, `docs/ROADMAP-PLAN.md`
- Modify: `docs/validate_docs.py` (the `CONTRACT_REQUIRED_MARKERS` "Legacy=egui compile-maintenance only" marker becomes "Legacy=egui removed")

**Interfaces:**
- Consumes: the removed state from Phases 1–2.
- Produces: docs where egui is described as *removed*, not *frozen*. Archived/historical docs keep their egui references for provenance (do not touch `docs/archive/**`).

- [ ] **Step 1: Flip the roadmap-contract marker in lockstep**

The validator enforces the exact marker string in `AGENTS.md`, `CLAUDE.md`, and `INDEX.md`. Change "Legacy=egui compile-maintenance only" → "Legacy=egui removed" in all three **and** in `CONTRACT_REQUIRED_MARKERS` inside `docs/validate_docs.py` in the same commit, or the validator fails.

- [ ] **Step 2: Update prose references**

In `AGENTS.md`, replace the "Desktop Shell Status" section and the "egui imports" convention row with a one-line note that egui was removed on YYYY-MM-DD (link this plan). Remove the "egui workbench — LEGACY" architecture bullet. Mirror in the canonical contract and desktop-shell-architecture spec.

- [ ] **Step 3: Run the doc validator**

Run: `cd /Users/jamespustorino/code/IMPULSE-rs && python3 docs/validate_docs.py --all`
Expected: PASS (no missing contract markers, no forbidden active-egui phrases).

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md CLAUDE.md docs/
git commit -m "docs(decommission): mark egui removed across active contract + docs"
```

---

## Phase 4 — Final sweep

- [ ] **Step 1: Confirm zero active egui references remain**

Run:
```bash
cd /Users/jamespustorino/code/IMPULSE-rs && \
  grep -rln "egui\|eframe\|impulse-gui" --include=*.rs --include=*.toml --include=*.sh --include=*.md . \
  | grep -v "/archive/\|/_archive\|/.worktrees/\|/target/\|EGUI-DECOMMISSION.md"
```
Expected: no output. Any hit is either a missed reference (fix it) or an intentional archived/historical doc (leave it).

- [ ] **Step 2: Mark this plan complete**

Set this doc's frontmatter `status: complete` and add a closing note with the final reclaimed LOC. Commit.

---

## Why this ordering

`impulse-gui` is the only thing keeping `impulse-term`'s egui layer alive, and the docs describe a state the code no longer matches. Removing the binary first (Phase 1) makes the rendering layer provably dead, so Phase 2 is a safe deletion rather than a risky refactor. Docs are scrubbed last (Phase 3) so the contract never claims "removed" before the code is gone — preserving the invariant that the canonical contract describes reality. The parity gate (Phase 0) ensures we do not strand users on a non-authoritative Dioxus host.
