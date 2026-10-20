#!/usr/bin/env bash
# Fake Pi gate launcher for `run_ion_verify` tests (TUI_SPEC.md T4, scenario 2:
# "changes requested"). Reads the HarnessRequest on stdin, echoes its
# request_id, then emits a single-line, spec-a-valid HarnessResponse with
# verdict `CHANGES REQUESTED` and one non-empty (WARNING-severity, not
# CRITICAL) finding plus a non-empty commands_run entry so
# HarnessResponse::validate() still passes — this fixture exercises the
# "gate ran cleanly but flagged a real issue" path, distinct from
# ion-verify-stub-gate.sh's APPROVE path and from
# ion-verify-stub-gate-contract-violation.sh's invariant-violating path.
#
# Mirrors the header/style convention of ion-verify-stub-gate.sh.
set -euo pipefail
request_id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["request_id"])')
printf '%s\n' "{\"contract_version\":\"0\",\"request_id\":\"${request_id}\",\"verdict\":\"CHANGES REQUESTED\",\"findings\":[{\"severity\":\"WARNING\",\"category\":\"correctness\",\"file\":\"src/lib.rs\",\"line\":12,\"message\":\"off-by-one in loop bound\"}],\"commands_run\":[{\"command\":\"cargo test\",\"exit_code\":1,\"output_ref\":\"log-1\"}],\"output_logs\":{},\"metrics\":{\"tokens_in\":0,\"tokens_out\":0,\"latency_ms\":0}}"
