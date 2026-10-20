#!/usr/bin/env bash
# Fake Pi gate that abstains with NEEDS DISCUSSION and no commands_run.
# That verdict is allowed to carry empty evidence. Callers must classify it
# as undecided, not as a refusal and not as a pass.
set -euo pipefail
request_id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["request_id"])')
printf '%s\n' "{\"contract_version\":\"0\",\"request_id\":\"${request_id}\",\"verdict\":\"NEEDS DISCUSSION\",\"findings\":[],\"commands_run\":[],\"output_logs\":{},\"metrics\":{\"tokens_in\":0,\"tokens_out\":0,\"latency_ms\":0}}"
