---
title: Tool-call admission at the handoff gate
description: Work card for URL evidence admission, context stamps, and field-level refusals
updated: 2026-10-20
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, admission]
---

# Tool-call admission at the handoff gate

## Lane Facts
- Owner: cursor agent
- Role: follow-up to PR #96. Tighten URL admission without network access inside the gate.
- Branch: `cursor/tool-admission-b5cb`
- Base: `cursor/handoff-gate-4741` (PR #96, head `2ba676d`). This lane does not commit to that branch.
- Worktree: `/workspace`
- Owned paths: `impulse-rs/src/handoff/`, `impulse-rs/src/tooling/builtin/handoff.rs`, this card
- Blocked/shared paths: PR #96's branch, `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, desktop MCP tool list, `orchestration::write_handoff`, shared-artifact generation protocol
- Plan/spec: brief 02 (tool-call admission) and Jim's decisions recorded below
- Verification: `CARGO_TARGET_DIR=$PWD/target-agent cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` from `impulse-rs`
- Latest status: implemented on `cursor/tool-admission-b5cb`. Draft pull request targets `cursor/handoff-gate-4741`. PR #96 is untouched.

## Decisions
- 2026-10-20: URL evidence counts only when the recorded status is 2xx (200–299). 3xx, including 304, and 4xx/5xx are `fetch_unusable`.
- 2026-10-20: `fetched_at` must be at or before the gate clock and no older than `DEFAULT_FETCH_MAX_AGE_SECS` (24 hours). A caller of `dispatch_for` may pass a shorter or longer positive `fetch_max_age`. The `handoff` tool reads `IMPULSE_HANDOFF_FETCH_MAX_AGE_SECS` and passes that duration in. The gate does not read the environment variable itself. Blank, zero, negative, and unparseable values keep the 24-hour default. An age equal to the max is still evidence; one second older is not.
- 2026-10-20: The gate never reads the network. Live PR state is evidence only after the caller has fetched it and recorded that fetch. There is no `GitHubPr` source and no credential inside the gate.
- 2026-10-20: `execution_origin` and `session_id` are stamped from `AdmissionContext`, which the tool builds from `ToolContext`. Payload `session_id`, `execution_origin`, and `recorded_via` are ignored. A URL claim resolves only when the fetch's session equals the submit's session, or both are absent.
- 2026-10-20: `sender` / `from_agent` remain the declared parties of the handoff and the ledger key. They are not taken from the payload as a substitute for the session stamp. The gate does not alias drifted names such as `from`.
- 2026-10-20: Every request-shape refusal and every fetch-usability refusal sets `GateFailure.field` to the JSON field to change. The gate does not rewrite arguments.
- 2026-10-20: Ledger schema is 3. Schema 1 still migrates through the schema 2 sender/URL shape, then schema 3 adds `recorded_via`. Migrated rows use `execution_origin: "unrecorded"` and `session_id: null`.

## Changes
- `CheckKind::FetchUnusable` is separate from `SourceResolves`. A recorded fetch is evidence only when its status is 200–299, `fetched_at` is not in the future, and its age is at most `fetch_max_age` (default 24 hours).
- `record_fetch` still stores a non-2xx status. Submit refuses it and leaves the stored status unchanged.
- Ledger schema is 3. Each fetch and handoff stores `recorded_via` `{ execution_origin, session_id }` taken from `AdmissionContext`. Schema 1 migrates through the schema 2 sender/URL shape, then schema 3 stamps `execution_origin: "unrecorded"` and `session_id: null`.
- The `handoff` tool builds that context from `ToolContext` (`execution_origin`, `session_id`) and passes `IMPULSE_HANDOFF_FETCH_MAX_AGE_SECS` through `parse_fetch_max_age_secs`. Library `dispatch` uses `AdmissionContext::direct()`.
- A URL claim resolves only when the fetch session equals the submit session, or both are absent. Payload `session_id`, `execution_origin`, and `recorded_via` are ignored. `from` is not aliased to `from_agent`.
- Request-shape errors become a pending result whose `GateFailure.field` names the JSON field to change. `HandoffTool::descriptor()` embeds `action_schemas()` after `ACTION_SCHEMAS=`.

## Tests
- Named brief cases are in `impulse-rs/src/handoff/tests.rs`: 404, non-2xx, future `fetched_at`, older than max age, fresh 2xx, shorter max age, cross-session fetch, ledger origin/session stamp, schema 2 migration, missing `from_agent`, drifted `from`, and descriptor schema.
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` passed with `CARGO_TARGET_DIR=$PWD/target-agent`.
- `cargo test --workspace` stopped after the impulse-rs library target (fail-fast). That target: 2444 passed, 5 failed, 4 ignored. The five failures are governed verification (`Failed` where the test expects `Passed` or `Inconclusive`). They are outside this lane. The verifier scrubs the environment to `ENV_ALLOWLIST`, which does not include `RUSTUP_HOME`. The same note is on the PR #96 work card.
- Packages the fail-fast run did not execute were run separately and passed: impulse-step-model 16, impulse-term 94 + 19 + 3, impulse-desktop/ion/ops doctests 0, impulse-rs doctests 3 passed and 1 ignored, impulse-rs bins 0, and the integration targets other than `governed_process_flow`.
- `tests/governed_process_flow.rs::real_daemon_builder_cli_verification_and_restart_preserve_evidence` failed the same way (`VerificationFailed` vs `AwaitingSupervisor`). It is the same scrubbed verifier, not a handoff change.

## Handoff Notes
- PR #96 stays untouched. This work opens a separate draft pull request whose base is `cursor/handoff-gate-4741`.
- The gate still does not re-fetch URLs, compare `body_digest` to a response body, or change URL subjects away from the host.
- GitHub Actions is blocked by billing, so the local fmt, clippy, and test results above are the verification evidence.
