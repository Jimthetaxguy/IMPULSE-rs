---
title: Packaged Dioxus Host Acceptance
description: Work card for codex-dioxus-packaged-acceptance-20260830
updated: 2026-08-30
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, release, dioxus, acceptance, provenance]
---

# Packaged Dioxus Host Acceptance

## Lane Facts

- Owner: Codex root orchestrator; runtime, packaging, adversarial, and MiniMax reviewers are
  read-only proposal/review inputs.
- Role: Isolated integration lane for source-bound packaging and real mounted-DMG host acceptance.
- Branch: `agent/codex-dioxus-packaged-acceptance-20260830`.
- Worktree: `.worktrees/dioxus-packaged-acceptance-20260830` (repository-relative).
- Base: exact remote-backed release-truth commit
  `69b2491ec7a2c924fddd66a1113378049eadbbe4`, tree
  `e635117ad08276faf2b9710a3b0d63512237aa71`.
- Owned paths:
  - `impulse-rs/impulse-desktop/src/{lib,host_bridge,packaged_acceptance,ui,daemon_ops,runtime}.rs`
  - `impulse-rs/impulse-desktop/src/bin/impulse_desktop.rs`
  - `impulse-rs/impulse-desktop/tests/{desktop_contract,macos_packaging_contract,packaged_live_host_acceptance}.rs`
  - `impulse-rs/impulse-ops/{Cargo.toml,src/lib.rs}`
  - `impulse-rs/src/daemon/{mod,handlers,protocol,tests}.rs`
  - `impulse-rs/impulse-desktop/Cargo.toml` and only the directly resulting reviewed
    `Cargo.lock` change, if any
  - `impulse-rs/scripts/{build-macos-app,verify-macos-app,write-macos-provenance,verify-packaged-host}.sh`
  - `.github/workflows/{ci,release}.yml`, limited to external package target paths required by the
    new source-isolation contract
  - this work card; after behavior is proven, the narrow status/vocabulary updates in
    `CONTEXT.md` and `docs/plans/EGUI-DECOMMISSION.md`
- Blocked/shared paths: canonical dirty checkout files; `.github/workflows/*` unless the existing
  `--dmg` call cannot transitively exercise the gate; canonical contract/index files; EGUI or
  Tauri removal; license selection; signing, notarization, installation, tagging, publication,
  and any live user-state mutation.
- Plan/spec: R1 and the R6 entry gate in `docs/plans/EGUI-DECOMMISSION.md`.
- Verification: TDD-focused Rust/script contracts; real arm64 app/DMG rebuild from a clean commit;
  source-bound structural/provenance verification; GUI-capable mounted-DMG acceptance; complete
  locked workspace gate; docs validation; diff/leak/commit review.
- Latest status: the reviewed packaging implementation is committed locally on this unpushed
  branch at `7e25412682900ca67170032d9ed632d9aab3c5b9`,
  with bounded external and Rust-transcript diagnostics committed at `ecd9ce54cfe9b5685e3949848fd2d0eb7918ca86`
  and `11b105c0834481454046d2b7d5e195412ff569b0`. Two exact-commit DMGs passed provenance,
  structural, image, and read-only mount verification before the real mounted host timed out.
  The improved transcript evidence exposed a confirmed blocker consistent with that timeout: Rust
  responses omitted the JavaScript router's required `kind: host_invoke_result` discriminator.
  Both response paths and focused resolve/reject regressions are now fixed locally; only a fresh
  mounted run can show whether another packaged blocker remains. A third diagnostic candidate
  correctly failed closed when this source edit occurred during verification; it is source-drift
  evidence, not a product result. The post-fix desktop, external-harness, packaging, complete locked
  workspace, all-target check, strict-Clippy, formatting, shell-syntax, and diff-hygiene gates are
  green. Exact staging/commit review and a fresh exact-commit mounted acceptance remain.

## User-Visible Outcome

An unsigned developer-preview DMG can no longer pass merely because its files exist. The build
must embed exact source/payload provenance, and the read-only mounted app must launch its real
Dioxus/WKWebView host, load bundled xterm assets, traverse the real JS-to-Rust bridge, connect to a
packaged isolated daemon, and complete a controlled temporary terminal lifecycle before the
candidate is described as packaged-host accepted.

## Decisions

- 2026-08-30: Preserve the dirty canonical checkout and branch only from the clean pushed
  release-truth commit.
- 2026-08-30: Keep build provenance separate from runtime acceptance. Use one canonical embedded
  `ReleaseProvenance.v1.tsv` with a single explicit self-exclusion; never put its own digest inside
  itself. Detached manifest/DMG digests remain external run evidence.
- 2026-08-30: The packaged observer is passive and opt-in. It may not install or replace a host
  API, load fallback assets, use Tauri, use the injected browser smoke API, or write a receipt file.
  Rust validates one fresh nonce-bound observation and emits one JSON receipt to captured stderr.
- 2026-08-30: The real test runs from the read-only mounted DMG with isolated `HOME`,
  `CFFIXED_USER_HOME`, `IMPULSE_HOME`, `TMPDIR`, cwd, and daemon socket. Process-group RAII covers
  controlled/catchable error and signal paths; before/after wardens prove protected fingerprints
  unchanged when the harness completes. A separate controller regression proves the real PTY shell
  exits after its harness parent receives uncatchable `SIGKILL`.
- 2026-08-30: Include read-only host round trips, unknown-command rejection, temporary workspace
  registration/listing, real packaged-daemon connection evidence, and terminal
  open/input/output/resize/focus/exit/close. Do not claim role-aware agent launch or review
  mutation until those are separately exercised.
- 2026-08-30: Repair the existing terminal initialization race by bounded-polling for the real
  host, xterm constructor, FitAddon constructor, and loaded xterm stylesheet. The acceptance
  observer only observes the fix; it never repairs product state.
- 2026-08-30: Compile both the package and its acceptance verifier from a detached worktree at the
  exact source commit. Use a stable authorized output root and a fresh, empty, private Cargo target
  contained beneath it for each build or acceptance run. Keep those targets outside the source
  snapshot, original worktree, canonical checkout, mounted app, and live Impulse home; reject
  overlap, escape, or symlink redirection before Cargo starts.
- 2026-08-30: Bind packaged daemon evidence to protocol v7 identity on the same Unix stream used
  for every expected-identity operation: `Ping`, `SubscribeOps`, `PublishTerminalOps`, and
  `RegisterGovernedTask`. Require the kernel-reported peer PID to equal the launched daemon child
  PID on macOS/Linux, alongside its canonical Impulse root, protocol version, and the digest of a
  harness nonce supplied directly to both the daemon and desktop acceptance path but never
  accepted back as raw proof.
- 2026-08-30: Exercise xterm through its public `input()` and `resize()` APIs and verify the real
  active buffer. Direct host callbacks or observer-owned output listeners are not xterm evidence.
- 2026-08-30: Warden claims cover Git-visible state plus named project-local `.impulse` roots and
  live `~/.impulse`. Ignored build targets, `node_modules`, and unrelated worktrees are explicitly
  outside that unchanged-state claim.
- 2026-08-30: Active CI and release-candidate workflows place package targets under
  `${{ runner.temp }}`. Repository-relative targets are incompatible with the source-isolation
  contract and are rejected before Cargo starts.
- 2026-08-30: Treat the DMG as immutable evidence across verification: require exactly one
  non-symlink `Impulse.app` at the mounted volume root, run final `hdiutil verify`, detach, and
  require the post-detach SHA-256 to equal the pre-mount digest before reporting the final digest.
- 2026-08-30: Keep the live JavaScript router strict and make every Rust invoke response use the
  exact discriminated envelope `kind: host_invoke_result`, including both normal worker results
  and queue-rejection results. The request id remains the sole pending-promise correlation key.
  Keep the internal response non-serializable so future send paths cannot bypass the envelope.

## Non-Goals

- No Developer ID signature, hardened-runtime claim, notarization, stapling, installation, tag,
  GitHub Release, deployment, or public artifact retention.
- No license choice, Tauri removal, EGUI removal, or broad release-workflow rewrite.
- No external provider call, model turn, role-aware agent launch, review mutation, or persistent
  user workspace registration.
- No claim of reproducible build, publisher authenticity, production readiness, full R6 parity,
  or deployment.

## Acceptance Criteria

### Source and payload provenance

- Building requires a clean coordinator checkout, then performs the build from a detached
  worktree at the exact source commit. It rechecks commit, tree, lock digest, and clean state after
  compilation before packaging.
- `ReleaseProvenance.v1.tsv` has fixed schema/order, source object format, commit, tree,
  `Cargo.lock` SHA-256, version/profile/target, one self-exclusion, and one sorted file row for every
  other regular payload under `Contents`.
- Verification rejects malformed, duplicate, unsorted, missing, unexpected, symlinked, traversal,
  absolute-path, mode, size, digest, source-state, target-slice, or self-reference mismatches.
- The staged and read-only-mounted bundles have identical manifests and payloads. The mounted
  volume root contains exactly one non-symlink `Impulse.app`. External evidence records the
  manifest digest without embedding it into the manifest and reports the DMG SHA-256 only after
  final image verification, detach, and a post-detach digest match.

### Real packaged host

- The directly executed mounted `Contents/MacOS/impulse-desktop` remains alive until one fresh
  receipt is observed; pending bootstrap, Tauri, injected test host, or source-checkout assets
  cannot satisfy the gate.
- The real Dioxus host reports exact `dioxus-eval-bridge-ready` only in a receipt that follows
  successful JS-to-Rust-to-JS command round trips.
- Bundled xterm JS, FitAddon, and stylesheet load from Dioxus-local asset URLs while cwd contains no
  source assets. Acceptance calls the real xterm `input()` and `resize()` APIs and requires the
  expected line to appear in `buffer.active`.
- `agent_snapshot`, `agent_platforms`, `list_workspaces`, `mcp_descriptors`, and `review_queue`
  return the expected array shapes; an unknown command rejects through the same bridge.
- A temporary workspace is registered and can be read back. A fresh desktop accepts daemon
  connection only after a same-stream protocol-v7 Ping returns the exact child PID, canonical
  Impulse root, protocol version, and nonce digest expected by the harness, and the kernel reports
  that child PID as the peer. Every subsequent expected-identity operation repeats the typed Ping
  and kernel-peer check on its own stream before it can subscribe, publish, or register governed
  work. The external validator independently repeats the typed Ping and kernel-peer check while
  that child and its isolated socket remain live at receipt. This is point-in-time identity and
  connection evidence, not authentication against a malicious same-user process.
- A temporary shell PTY proves open, input, output marker, resize, focus, close, and exit over the
  real bridge. The child receives no canonical/live paths, runs under isolated cwd/environment, and
  leaves the canonical project and live Impulse-home fingerprints unchanged.
- The Rust validator—not JavaScript—derives pass/fail. The receipt binds schema, nonce, child PID,
  package version, provenance-manifest digest, and bounded observation fields.

### Isolation and cleanup

- Missing/invalid nonce, root, home, cwd, socket, executable-bundle shape, or provenance digest
  fails closed before a passing receipt is possible.
- Desktop and daemon run in their own process groups; early exit, timeout, parse failure, ordinary
  Rust panic, and handled `SIGINT`/`SIGTERM` terminate/reap children with bounded escalation. A
  macOS regression independently identifies the real PTY shell by nonce/PID/PPID/PGID/SID, kills
  only the harness-parent PID, and requires that exact shell identity to disappear.
- The isolated socket is no longer connectable after cleanup. Source/canonical HEAD and index,
  tracked contents, non-ignored untracked contents, named project-local `.impulse` roots, and live
  `~/.impulse` match before/after; private contents are never printed. Ignored build targets,
  `node_modules`, and unrelated worktrees are not covered by this warden claim.
- The normal passing path requires successful DMG detach. The EXIT trap attempts detach on every
  catchable failure path; a failed trap detach may leave a mount that requires explicit cleanup and
  does not convert the original failing gate into a pass.

## Test-First Sequence

1. Add failing packaging/provenance and passive-observer contract tests.
2. Add failing validator/config/nonce/receipt and delayed-asset regressions.
3. Implement canonical provenance generation and closed verification.
4. Implement the passive packaged observer plus terminal race repair.
5. Implement the external Rust process/warden harness and mounted-DMG wrapper.
6. Commit the clean implementation, rebuild from that exact commit, then run the real acceptance
   and complete final gate. A pre-commit artifact cannot prove the committed tree.

## Tests

- RED — `cargo test -p impulse-desktop --test macos_packaging_contract --locked -- --nocapture`:
  8 passed and the intended 3 failed (missing provenance writer, missing manifest accepted, and
  unexpected `Contents/MacOS` payload accepted).
- RED — isolated target `desktop_contract::test_terminal_interop_waits_for_real_host_and_all_packaged_xterm_assets`:
  failed on the missing bounded asset poll token.
- RED — isolated target `desktop_contract::test_packaged_acceptance_observer_is_passive_product_bridge_evidence`:
  failed because `src/packaged_acceptance.rs` did not yet exist.
- GREEN — `macos_packaging_contract`: 32 passed; desktop library: 167 passed;
  `packaged_live_host_acceptance`: 14 passed, 0 failed, and the one real mounted-app test remains
  explicitly ignored until the package recipe supplies a read-only mounted app. The external
  harness suite was rerun with real macOS process/socket permissions and includes the real
  parent-`SIGKILL`/PTY-shell cleanup regression plus copied-identity rejection against the wrong
  kernel peer. The added diagnostic contracts preserve bounded receipt failure reasons and partial
  Rust transcript failures without weakening any passing predicate.
- RED/GREEN — focused `response_envelope` tests first failed because no discriminated Rust response
  helper existed, then passed after both live send paths adopted the helper. A Node-driven smoke
  loads the real live-bridge script and proves a sequential success resolves while a typed error
  rejects; either missing discriminator would instead hit its 500 ms deadline.
- GREEN — focused protocol/runtime verification: `impulse-ops` tests, 106 daemon-focused library
  tests, desktop library tests, desktop integration compilation, desktop-app binary check, and
  strict clippy for `impulse-ops`, `impulse-desktop`, and `impulse-rs`. Rustfmt, shell syntax, and
  diff checks pass on the post-fix snapshot. The only emitted warning is the pre-existing future
  incompatibility notice for `block v0.1.6`.
- GREEN — complete locked workspace verification: `cargo test --workspace --locked`,
  `cargo check --workspace --all-targets --locked`, and
  `cargo clippy --workspace --all-targets --locked -- -D warnings` all exited zero. Rustfmt, shell
  syntax, and working-tree diff checks also pass. The independent final review found no remaining
  packaging/provenance code blocker.
- EXACT-ARTIFACT FAILURES — `7e25412` and `ecd9ce5` each produced a structurally valid,
  provenance-bound DMG and reached the mounted host, then timed out at Rust transcript validation.
  The second run reported `packaged observer timed out`, which led to the confirmed invoke-envelope
  blocker; the fresh mounted rerun must determine whether any additional blocker remains. The
  `11b105c` run was intentionally rejected by the source-state warden after the local fix changed
  its source worktree during verification; that candidate is invalid and not reusable.
- Pending: exact staging/diff/leak review, commit, a fresh source-bound app/DMG rebuild with no
  concurrent source writes, and mounted live-host acceptance.

## Handoff Notes

- The preserved diagnostic DMGs are not release candidates: two encode known failing commits and
  one was rejected for source drift. No passing exact-HEAD DMG exists yet, and none may be reused
  to claim acceptance.
- Browser `host_readiness_smoke.mjs` remains useful injected contract coverage, but it must never be
  represented as packaged live-host proof.
- Full R6 still requires role-aware agent launch and real review decision behavior after this lane.
