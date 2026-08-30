---
title: Legacy UI Retirement Plan
description: Gated retirement of the nonfunctional egui surface, unwired affordances, and temporary Tauri compatibility adapter while preserving Dioxus, ratatui, CLI, PTY, and daemon authority
version: '2.0'
type: doc
category: roadmap
phase: all
status: active
updated: 2026-08-17
audience: builders
tags: [roadmap, decommission, egui, eframe, dioxus, tauri, desktop, cleanup]
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# Legacy UI Retirement Plan — EGUI First

> **Planning boundary:** This revision plans the retirement; it does not remove UI source, Cargo
> dependencies, release jobs, or compatibility code. Physical removal requires its own reviewed
> implementation lane, a verified recovery artifact, and the gates below.

## Outcome

Impulse will carry only functional product surfaces:

- **Keep:** Dioxus Desktop + xterm.js as the graphical cockpit.
- **Keep:** ratatui as the terminal-native control surface.
- **Keep:** CLI commands and daemon/control-plane contracts as authoritative operations.
- **Remove first:** the frozen `impulse-gui` egui application and the egui/eframe presentation
  layer still enabled by default in `impulse-term`.
- **Remove from retained shells:** visible controls that are placeholders, permanently disabled, or
  disconnected from an authoritative command/event contract.
- **Remove separately:** the Tauri-shaped compatibility adapter after the packaged Dioxus host
  proves the real bridge and required operator workflows end to end.

The stable boundary is not a UI framework. PTY lifecycle, runtime identity, policy, memory,
telemetry, review, and verification remain Rust backend responsibilities. A cockpit renders and
invokes those contracts; it never becomes a second control plane.

EGUI deletion does **not** wait on Tauri retirement and does **not** wait on a vague “Dioxus
parity” gate. It waits on an honest release state (Track A / R1).

## Verified Baseline

Verified on `origin/main` at `99396f9` on 2026-08-17 (post PR #24/#25/#26).

| Surface | Live fact | Retirement consequence |
|---|---|---|
| `impulse-gui` | Excluded leaf crate; 38 Rust files / 14,998 lines | Remove the entire crate after recovery proof; do not port it feature-for-feature |
| `impulse-term` egui layer | `default = ["egui"]` still activates `eframe` 0.31 in normal workspace resolution | Removing it materially reduces the active dependency graph |
| Framework-neutral terminal core | `backend.rs`, `context.rs`, and `paste.rs`; Dioxus already uses `default-features = false` | Preserve and strengthen this boundary |
| Dioxus host | Live eval bridge and xterm panes are implemented | Keep as the graphical product surface |
| Dioxus host smoke | Browser smoke can inject a test host API | Useful contract proof, but not packaged real-bridge acceptance |
| Dioxus dead affordances | Disabled Cmd-K and Settings buttons say “coming soon”; artifact action buttons only set `latest_shell_intent` | Hide/remove until backed by a real typed operation, or wire and prove them |
| Dioxus MemoryView | Renders daemon snapshot plus `memory_candidates` (ADR-0013 accepted-run review cards) | Keep; this is a live read/review surface, not a placeholder |
| Voice / ElevenLabs | `impulse_rs::voice` is live library/CLI; no Dioxus Voice view on this baseline | Keep the library. Do not invent a cockpit control to retire |
| Kernel settlement / basis | `impulse_rs::{basis,settlement}` plus proposed ADR-0014; no cockpit affordance | Keep as backend. No Track B action until a visible control exists |
| Governed producers | Dioxus shows terminal command guidance (`governed-verify` / `governed-review`) rather than producer buttons | Keep as truthful guidance. Do not add fake producer buttons |
| macOS release | `.github/workflows/release.yml` `build-dmg` calls `scripts/build-macos-app.sh`, which copies excluded `impulse-gui` output | Broken/stale release claim must be repaired or removed before EGUI deletion |
| Tauri adapter | Optional `legacy-tauri-runtime` / deprecated `tauri-runtime` features, cfg wrappers, and `window.__TAURI__` fallback remain | Retire in a distinct tranche after Dioxus packaged acceptance |
| Agent automation | `.claude/skills/impulse-development/SKILL.md` still routes “Add a GUI view” to `impulse-gui` | Neutralize before deleting code to prevent resurrection |

## Removal Policy

Whole legacy stacks, dead affordances inside retained shells, and compatibility adapters are
different retirement problems and must not share one all-or-nothing gate.

### Track A — EGUI/eframe

EGUI is not a functional fallback today: the crate is excluded from the workspace and the release
script does not build the binary it later tries to copy. Its deletion therefore does **not** wait
for Tauri-adapter retirement. It does wait for an honest release state:

1. **Preferred:** replace the stale bundle path with a tested Dioxus `.app`/DMG pipeline.
2. **Allowed fallback:** if desktop packaging is deliberately deferred, remove the broken DMG job
   and publish CLI artifacts only, with desktop distribution explicitly marked unavailable.

The preferred route is the product-aligned route. The fallback is honest but does not satisfy the
first-class desktop distribution goal.

### Track B — Dead affordances in retained surfaces

An active cockpit must not advertise controls that cannot perform their named operation. Every
visible action must satisfy one of these states:

1. **Functional:** it invokes a typed backend command and renders success/failure evidence.
2. **Truthfully unavailable:** it is omitted from the product surface; capability/help text may
   explain what is not installed without presenting a fake action.
3. **Read-only:** it is visibly non-actionable and never styled or announced as an executable
   control.

“Coming soon,” permanently disabled buttons, local-only intent handlers, and fixture-backed actions
do not qualify. In the current Dioxus shell, remove the disabled command-palette/settings buttons
until implemented, and hide artifact action buttons until they dispatch a real typed operation.

### Track C — Tauri compatibility

Tauri-shaped compatibility stays until the Dioxus binary itself—not an injected browser test
transport—proves launch, invoke/listen, terminal lifecycle, workspace launch, review actions, and
daemon-backed state. Removing Tauri must be a later, independently reversible commit/PR.

## Scope Classification

### Keep

- `impulse-rs/impulse-desktop/` Dioxus UI, xterm.js assets, live bridge, host commands, and runtime.
- Root ratatui UI and CLI.
- `impulse-term/src/backend.rs`, `context.rs`, and `paste.rs`.
- `impulse-term/tests/backend_tests.rs`.
- `impulse-term/tests/boundary_tests.rs`, reworded to prove the post-EGUI public API.
- Daemon/control-plane, role, runtime, memory, telemetry, review, artifact, verification, voice,
  basis, and settlement code.
- Dioxus MemoryView / accepted-run candidate cards and governed-task evidence cards that render
  daemon truth (including terminal producer command guidance).
- Historical ADRs, archived plans, and design research as provenance.

### Migrate

- `Impulse.icns` to Dioxus packaging if the product keeps that asset.
- macOS bundle metadata and release automation to a real Dioxus package path.
- `.claude/skills/impulse-development/` routing from EGUI views to Dioxus views and host contracts.
- Any still-useful documentation concepts to framework-neutral or Dioxus-specific guidance.
- `impulse-term/src/lib.rs` and README to a backend/context/paste-only contract.
- Root `Cargo.lock` after dependency removal, with a reviewed dependency diff.

### Remove in Track A

- Entire `impulse-rs/impulse-gui/`, including its standalone manifest, lockfile, README, resources,
  and source tree.
- `impulse-term/src/input.rs`, `panel.rs`, `renderer.rs`, `status_bar.rs`, and `theme.rs`.
- `impulse-term`'s `egui` feature, default feature, optional `eframe` dependency, cfg gates, and
  EGUI public re-exports.
- Root workspace `exclude = ["impulse-gui"]` entry.
- Stale EGUI build/release instructions, active GUI-view guide, and running Ralph state.
- Active comments or docs that direct new work into deleted modules.

`theme.rs` is removed rather than retained as a speculative RGB model: no non-EGUI consumer exists.
If a future cockpit needs a shared theme schema, define it at that consumer boundary from current
requirements instead of preserving dead presentation code inside the PTY crate.

### Remove in Track B

- Disabled `Cmd-K` and `Settings` “coming soon” buttons in the active Dioxus shell, unless they are
  first wired to real operations and covered by acceptance tests.
- Artifact action buttons whose handler only updates `latest_shell_intent`; keep the read-only
  artifact envelope display until a typed artifact-action command exists.
- Skill/table rows that tell agents to add views in `impulse-gui`.
- Any additional visible Dioxus or ratatui action found by the R2 audit that has no authoritative
  command, event, or read model behind it.
- Production fixture/demo/fallback data that makes an unavailable function appear operational.

### Remove in Track C

- `legacy-tauri-runtime` and deprecated `tauri-runtime` Cargo features.
- Optional `tauri` dependency and Tauri-only cfg wrappers/event sink.
- `window.__TAURI__` fallback and legacy host-mode smoke.
- Tests and active docs whose only purpose is Tauri compatibility.

## Execution Tranches

### R0 — Recovery and external-consumer proof

- [ ] Refresh `git status`, `git fetch`, upstream divergence, worktrees, and open PR dependencies.
- [ ] Prove no active crate or external package consumes `TerminalPanel`, `TerminalRenderer`, the
      EGUI theme types, or `impulse-gui` as a binary artifact.
- [ ] Push a backup ref for the exact pre-removal commit.
- [ ] Create a deterministic `git archive` of the EGUI crate, EGUI-only terminal files, current
      packaging files, and active automation into a user-approved non-repository archive directory.
- [ ] Record the archive manifest and SHA-256; list-test the archive before any deletion.
- [ ] Obtain explicit approval for the mass-removal implementation lane.

**Stop if:** an unclassified consumer, published artifact contract, unbacked branch, or concurrent
owner of the same paths appears.

### R1 — Make release state truthful

Preferred Dioxus packaging path:

- [ ] Build `impulse-desktop` with `--features desktop-app --bin impulse-desktop` for each target.
- [ ] Move bundle metadata/icon ownership out of `impulse-gui`.
- [ ] Package the Dioxus binary, required `impulse-rs` companion, and native `ion` sibling
      explicitly.
- [ ] Add an artifact inspection and macOS launch smoke that fails if the app exits immediately,
      cannot load local xterm assets, or never reports the live Dioxus bridge status.
- [ ] Make the tag release consume only the proven Dioxus package recipe.

Fallback CLI-only path, only if explicitly chosen:

- [ ] Remove the broken DMG job and stale GUI bundler from active release automation.
- [ ] State clearly that tagged releases contain CLI artifacts only until Dioxus packaging lands.
- [ ] Keep Track C blocked; CLI-only release truth is not Dioxus operational acceptance.

**Gate:** no active release script or workflow may reference `impulse-gui`.

#### 2026-08-29 release-truth checkpoint

The isolated `agent/codex-dioxus-release-truth-20260829` lane removes `impulse-gui` from active
CI/release-readiness automation, explicitly builds the Dioxus host, control-plane companion, and
native Ion sibling, then adds portable structure checks plus real native `.app`/DMG mount
inspection. Candidate outputs are non-distributable developer previews with no Developer ID bundle
signature and are not retained by public-repository workflows; ad-hoc Mach-O signatures may exist.

This checkpoint does not close R1. A hosted universal run, packaged live-host acceptance, license
reconciliation, authorized signing, notarization/stapling, and explicit public-release authority
remain required. The tag-triggered publisher stays removed until those gates exist.

### R2 — Remove resurrection vectors and dead affordances

- [ ] Rewrite the project development skill to route graphical view work to Dioxus.
- [ ] Replace the EGUI view guide with a Dioxus view/host-contract guide; preserve old guidance only
      under an explicitly historical/archive path.
- [ ] Retire any stale Ralph-loop EGUI continuation state so automation cannot resume an EGUI queue.
- [ ] Update live code comments that use deleted GUI modules as a current comparison point.
- [ ] Inventory every visible Dioxus and ratatui control against a typed command/event/read-model
      path; record KEEP, WIRE, or REMOVE for each unmatched control.
- [ ] Remove the disabled `Cmd-K` and `Settings` “coming soon” buttons until they are functional.
- [ ] Remove/hide artifact action buttons that only set a local status string; retain read-only
      artifact cards until a real artifact-action command exists.
- [ ] Add SSR/contract coverage that rejects `coming soon` controls and actionable buttons without
      a backed handler contract.

**Gate:** active agent configuration contains no instruction to create or modify EGUI views, and
retained product shells contain no known placeholder or local-only action affordance.

### R3 — Remove the frozen application

- [ ] Preserve/migrate the icon only if R1's Dioxus package uses it.
- [ ] Remove `impulse-rs/impulse-gui/` from the tracked tree.
- [ ] Remove the root workspace exclusion.
- [ ] Run the complete Rust and retained-surface gates before continuing.

**Stop if:** removal changes a nonlegacy API, Dioxus build, ratatui behavior, CLI behavior, or release
artifact outside the classified paths.

### R4 — Remove EGUI from `impulse-term`

- [ ] Delete the five EGUI-only modules: `input`, `panel`, `renderer`, `status_bar`, and `theme`.
- [ ] Remove their module declarations and re-exports from `lib.rs`.
- [ ] Remove the default `egui` feature and optional `eframe` dependency.
- [ ] Retain and reword boundary tests around the framework-neutral API.
- [ ] Update the terminal crate README and module-level architecture description.
- [ ] Regenerate and review `Cargo.lock`; reject unrelated lockfile churn.
- [ ] Confirm workspace metadata and dependency trees contain neither `eframe` nor EGUI packages.

**Gate:** Dioxus, ratatui, CLI, backend/context tests, and the temporary Tauri compatibility feature
all still compile and pass.

### R5 — Make the canonical contract describe removal

- [ ] In the **implementation** PR that physically removes EGUI, add the next free ADR (0016 as of
      2026-08-17; 0013–0015 are already taken) recording EGUI removal, the chosen release path,
      preserved surfaces, recovery reference, and the fact that Tauri compatibility remains separate.
      This planning revision does **not** add that ADR.
- [ ] In the same implementation PR, flip the exact roadmap marker from
      `Legacy=egui compile-maintenance only` to `Legacy=egui removed` everywhere enforced by the
      docs validator.
- [ ] Update current README, handbook, architecture, desktop spec, metadata, quickstart, skill, and
      collaboration guidance. Do not rewrite historical ADRs/design research as if EGUI never existed.
- [ ] Mark this plan's Track A complete only after source, manifests, release automation, agent
      instructions, docs, and dependency resolution agree.

### R6 — Retire Tauri compatibility independently (Track C)

Entry gate: a packaged Dioxus app must exercise the real `LiveDesktopApp` eval transport without
an injected test host API or `window.__TAURI__` and prove:

- [ ] app launch and live bridge readiness;
- [ ] terminal open, input, output, resize, focus, exit, and close;
- [ ] workspace list/register and role-aware agent launch;
- [ ] review queue and review decision actions;
- [ ] daemon-backed snapshot/telemetry subscription and fail-closed degradation;
- [ ] local xterm assets in the packaged artifact.

Then, in a separate commit/PR:

- [ ] remove Cargo feature aliases and the optional Tauri dependency;
- [ ] simplify Tauri cfg wrappers and delete the Tauri event sink;
- [ ] delete the JS fallback, legacy smoke mode, and compatibility-only tests;
- [ ] update ADR-0008 and current desktop docs to record the completed migration;
- [ ] rerun the packaged Dioxus acceptance suite and full workspace gates.

### R7 — Final sweep and closure

- [ ] Confirm zero EGUI references in shipping Rust, Cargo manifests/locks, shell scripts, release
      workflows, or active agent configuration.
- [ ] Allowlist historical/archive references instead of rewriting provenance.
- [ ] Confirm zero Tauri runtime references after Track C; framework-neutral mentions in ADR history
      may remain.
- [ ] Mark this plan complete and record removed files/LOC, dependency change, release proof,
      verification evidence, recovery ref, and PRs.

## Verification Contract

Use an isolated `CARGO_TARGET_DIR` for every Rust gate so concurrent worktrees cannot poison a
shared target directory. Run from `impulse-rs/` unless noted:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
cargo test -p impulse-term
cargo test -p impulse-term --no-default-features
cargo check -p impulse-desktop --features desktop-app --bin impulse-desktop
cargo check -p impulse-desktop --features legacy-tauri-runtime  # Track A only
```

Run from `impulse-rs/impulse-desktop/` if the host-smoke script is still the documented contract
check. Track C additionally requires the packaged real-bridge acceptance test described in R6;
browser contract smoke alone is insufficient.

Run from the repository root:

```bash
python3 docs/validate_docs.py --self-test
python3 docs/validate_docs.py --all
git diff --check
```

The implementation gate must also assert through Cargo metadata/tree inspection that `eframe` and
EGUI packages are absent after Track A, and that `tauri` is absent after Track C. Expected
zero-match searches must be written so an empty result is success, not mistaken for a failed gate.
Do not hard-code aggregate test counts; record the live command output.

## Commit and PR Boundaries

Keep the work reviewable and reversible:

1. Release truth and asset migration.
2. Active automation/skill migration.
3. Dead-affordance removal from retained shells.
4. `impulse-gui` removal.
5. `impulse-term` EGUI/eframe removal plus lockfile.
6. Track A contract/docs/ADR alignment.
7. Tauri compatibility removal after its independent entry gate.

Every commit must leave retained surfaces buildable. EGUI removal, dead-affordance cleanup, and
Tauri compatibility removal must remain independently reviewable; Track A and Track C must never
be collapsed into one destructive commit.

## Critical Path

```text
R0 recovery proof
  -> R1 release truth
  -> R2 resurrection cleanup
  -> R3 impulse-gui removal
  -> R4 impulse-term de-EGUI
  -> R5 canonical contract alignment

R1 preferred Dioxus packaging + packaged real-bridge acceptance
  -> R6 Tauri compatibility removal
  -> R7 final closure
```

The CLI-only R1 fallback unblocks EGUI removal but does **not** unblock Track C or satisfy the
first-class desktop distribution goal.

## Definition of Done

- No nonfunctional UI is presented as a supported product surface.
- Every visible action in retained Dioxus/ratatui surfaces is backed by an authoritative operation,
  or is removed until that operation exists.
- EGUI/eframe is absent from shipping source, workspace resolution, release automation, and active
  agent instructions.
- Dioxus, ratatui, CLI, PTY, daemon, roles, tools, memory, telemetry, review, and verification remain
  functional and authoritative at their intended layers.
- The release workflow only publishes artifacts it actually builds and tests.
- Tauri compatibility is either explicitly temporary with a live removal gate, or fully removed
  after packaged Dioxus acceptance.
- Recovery evidence and complete verification output are attached to the implementation PRs.

## Related work that is out of scope here

- GitHub draft [#17](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/17) (`codex/legacy-ui-retirement-plan`) is abandoned as a merge vehicle; its unique intent is this rewrite.
- `codex/dioxus-egui-retirement` is a later implementation lane, not this planning PR.
