#!/usr/bin/env bash
# Fake Pi gate launcher for `run_ion_verify` tests (TUI_SPEC.md T4, scenario 4:
# "non-zero exit"). Reads and discards the HarnessRequest on stdin, writes a
# diagnostic to stderr, and exits non-zero WITHOUT ever producing a
# parseable HarnessResponse on stdout — reproducing a gate crash (e.g. the
# underlying `pi` process erroring out before emitting JSON). PiAdapter must
# surface this as `AdapterError::NonZeroExit { code, stderr }`, and
# `run_ion_verify` must propagate it as an `Err`, not silently return a
# default/empty response.
set -euo pipefail
cat >/dev/null
echo "stub gate: simulated crash before emitting a response" >&2
exit 7
