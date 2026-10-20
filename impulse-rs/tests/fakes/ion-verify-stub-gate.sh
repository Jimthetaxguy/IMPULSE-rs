#!/usr/bin/env bash
# Fake Pi gate launcher for `run_ion_verify` tests (TUI_SPEC.md T3,
# src/handlers/ion.rs). Drives PiAdapter through a full round trip without
# the real MiniMax-backed gate: reads the HarnessRequest on stdin, echoes its
# request_id, then emits a single-line, spec-a-valid HarnessResponse (APPROVE,
# one commands_run entry so HarnessResponse::validate() passes) on stdout.
#
# Mirrors the pattern of impulse-ion/tests/fakes/hang-gate.sh (T2), which
# only covers the timeout path — this one covers the happy path so
# run_ion_verify can be exercised end-to-end without a live gate.
set -euo pipefail
request_id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["request_id"])')
printf '%s\n' "{\"contract_version\":\"0\",\"request_id\":\"${request_id}\",\"verdict\":\"APPROVE\",\"findings\":[],\"commands_run\":[{\"command\":\"cargo test\",\"exit_code\":0,\"output_ref\":\"log-1\"}],\"output_logs\":{},\"metrics\":{\"tokens_in\":0,\"tokens_out\":0,\"latency_ms\":0}}"
