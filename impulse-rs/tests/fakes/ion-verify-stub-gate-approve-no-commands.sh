#!/usr/bin/env bash
# Fake Pi gate launcher for `run_ion_verify` tests (T7 ReplTool regression:
# `ok` semantics must match the CLI's `!passed() || validate().is_err()`).
# Emits verdict APPROVE with no CRITICAL finding (so `HarnessResponse::passed()`
# is true) but an empty `commands_run` (so `HarnessResponse::validate()` fails
# with `ContractViolation::MissingCommandsRun`). This isolates the case Opus's
# T7 review flagged: `passed()` alone would report `ok: true` here, which is
# wrong -- the CLI's `handle_ion_verify` treats this as a failing gate.
set -euo pipefail
cat >/dev/null
echo '{"contract_version":"0","request_id":"req-stub-no-commands","verdict":"APPROVE","findings":[],"commands_run":[],"output_logs":{},"metrics":{"tokens_in":0,"tokens_out":0,"latency_ms":0}}'
