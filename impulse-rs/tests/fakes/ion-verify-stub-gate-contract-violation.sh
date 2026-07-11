#!/usr/bin/env bash
# Fake Pi gate launcher for `run_ion_verify` tests (TUI_SPEC.md T4, scenario 3:
# "contract-violating response"). Reads and discards the HarnessRequest on
# stdin, then emits a syntactically valid, spec-a-*invalid* HarnessResponse:
# verdict APPROVE together with a CRITICAL finding, which
# `HarnessResponse::validate()` (impulse-ion/src/lib.rs,
# `ContractViolation::CriticalBlocksApprove`) rejects. `commands_run` is kept
# non-empty so the ONLY violation triggered is the critical/approve rule, not
# also `MissingCommandsRun` — this isolates the specific invariant under test.
#
# A misbehaving or under-constrained model (Pi has no --json-schema
# enforcement, per pi_adapter.rs's module doc) could plausibly emit exactly
# this shape; `run_ion_verify` must still return Ok(response) (validation is
# the caller's job per G1), while `response.validate()` must catch it.
set -euo pipefail
cat >/dev/null
echo '{"contract_version":"0","request_id":"req-stub-contract-violation","verdict":"APPROVE","findings":[{"severity":"CRITICAL","category":"security","file":"src/auth.rs","line":42,"message":"token never expires"}],"commands_run":[{"command":"cargo test","exit_code":0,"output_ref":"log-1"}],"output_logs":{},"metrics":{"tokens_in":0,"tokens_out":0,"latency_ms":0}}'
