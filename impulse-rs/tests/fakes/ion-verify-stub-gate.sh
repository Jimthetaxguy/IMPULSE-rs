#!/usr/bin/env bash
# Fake Pi gate launcher for `run_ion_verify` tests (TUI_SPEC.md T3,
# src/handlers/ion.rs). Drives PiAdapter through a full round trip without
# the real MiniMax-backed gate: reads and discards the HarnessRequest on
# stdin, then emits a single-line, spec-a-valid HarnessResponse (APPROVE,
# one commands_run entry so HarnessResponse::validate() passes) on stdout.
#
# Mirrors the pattern of impulse-ion/tests/fakes/hang-gate.sh (T2), which
# only covers the timeout path — this one covers the happy path so
# run_ion_verify can be exercised end-to-end without a live gate.
set -euo pipefail
cat >/dev/null
echo '{"contract_version":"0","request_id":"req-stub","verdict":"APPROVE","findings":[],"commands_run":[{"command":"cargo test","exit_code":0,"output_ref":"log-1"}],"output_logs":{},"metrics":{"tokens_in":0,"tokens_out":0,"latency_ms":0}}'
