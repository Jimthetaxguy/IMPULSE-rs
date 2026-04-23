---
id: STORY-182
title: Supervisor-tagged spawn — privilege boundary via env vars
spec: SPEC-EGUI-RETIRE
status: done
priority: high
created: 2026-04-23
completed: 2026-04-23
commit: 97f563b
---

# Supervisor-tagged spawn (Plan 2 / Loop 182)

## Outcome

`PaneRole::spawn_env_vars()` extended with `pane_id: Option<Uuid>`.
- Workers receive `IMPULSE_CMD_SOCKET` + `IMPULSE_WORKER_PANE_ID=<uuid>`
- Only Supervisor receives `IMPULSE_SUPERVISOR=1`

`LiveWorkerPane` resolves socket from `IMPULSE_CMD_SOCKET` env var,
generates a stable per-pane uuid via `use_signal`.

## Architectural decision

The privilege boundary is now `IMPULSE_SUPERVISOR=1`, **not** socket
access. Workers need the socket to *emit* `@impulse` commands; the daemon
authorizes verbs by inspecting `IMPULSE_PANE_ROLE` + `IMPULSE_WORKER_PANE_ID`
on the request payload. This unblocks STORY-183.

## Acceptance

- [x] `PaneRole::spawn_env_vars` round-trip tests for all 4 role/pane_id combinations
- [x] Existing `test_build_env_vars_for_role_worker_has_no_supervisor_env` updated to reflect new design
- [x] `cargo test --workspace`: 1,993 → 1,996 (+3 new role tests)
- [x] All 6 gates green incl. `--features experimental-runtime` build + clippy
