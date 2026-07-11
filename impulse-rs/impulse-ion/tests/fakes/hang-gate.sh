#!/usr/bin/env bash
# Fake Pi gate launcher for PiAdapter timeout tests (T2 — pi_adapter.rs).
#
# Deliberately never writes to stdout or stderr and outlives any sane test
# timeout. PiAdapter::verify must kill this process on its configured
# timeout and return AdapterError::TimedOut instead of hanging on
# read_line/child.wait() forever.
set -euo pipefail
sleep 60
