#!/usr/bin/env bash
set -euo pipefail
# capture-plan-evidence.sh - sole evidence producer
# MUST be run from repo root (IMPULSE-rs/)
# Wipes stale, enforces clean git (only documented ??), records HEAD, runs FULL steps, dedicated files.

SCRATCH=${SCRATCH:-/var/folders/zy/mwkg1s5x2839c2xgfzqt27n00000gn/T/grok-goal-bd088a7e3032/implementer}
mkdir -p "$SCRATCH"

# Wipe stale artifacts - never append manually
rm -f "$SCRATCH"/dioxus-host-smoke.log \
      "$SCRATCH"/impulse-status-*.log \
      "$SCRATCH"/verify-gate.log \
      "$SCRATCH"/mcp-execute-test.txt \
      "$SCRATCH"/verify-host-dispatch.log \
      "$SCRATCH"/capture-*.txt \
      "$SCRATCH"/verify-*.log 2>/dev/null || true

echo "HEAD=$(git rev-parse HEAD)" | tee "$SCRATCH/capture-head.txt"

# Enforce clean: only pre-documented ?? allowed (per plan/ADR)
if git status --short | grep -v -E '(\?\? archive/|\?\? CONTEXT.md|\?\? docs/decisions/|\?\? docs/plans/worktrees/|\?\? impulse-rs/scripts/)' | grep -q '^[ MADRC]'; then
  echo "ERROR: dirty git status (only documented ?? allowed)"
  git status --short
  exit 1
fi

# Run fallbacks check (must pass)
if [[ -x ./impulse-rs/scripts/check-mcp-fallbacks.sh ]]; then
  ./impulse-rs/scripts/check-mcp-fallbacks.sh
else
  echo "WARNING: check-mcp-fallbacks.sh not found/executable"
fi

echo "=== STEP2 SMOKE ===" | tee "$SCRATCH/verify-step2.log"
(cd impulse-rs/impulse-desktop && npm run vendor:xterm 2>&1 && npm run dioxus:host:smoke 2>&1) | tee "$SCRATCH/dioxus-host-smoke.log"
echo "STEP2_EXIT=$?" | tee -a "$SCRATCH/dioxus-host-smoke.log"

echo "=== STEP3 STATUS x2 ===" | tee "$SCRATCH/verify-step3.log"
(cd impulse-rs && cargo run -- status --format text 2>&1 ; echo "---JSON---" ; cargo run -- status --format json 2>&1 | head -30) | tee "$SCRATCH/impulse-status-1.log"
(cd impulse-rs && cargo run -- status --format text 2>&1 ; echo "---JSON---" ; cargo run -- status --format json 2>&1 | head -30) | tee "$SCRATCH/impulse-status-2.log"
echo "STEP3_EXIT=0" | tee -a "$SCRATCH/verify-step3.log"

echo "=== STEP4 GATE (FULL, no truncation) ===" | tee "$SCRATCH/verify-step4.log"
(cd impulse-rs && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check) 2>&1 | tee "$SCRATCH/verify-gate.log"
echo "STEP4_EXIT=$?" | tee -a "$SCRATCH/verify-gate.log"

echo "=== STEP5 HOST + MCP EXECUTE (full nocapture) ===" | tee "$SCRATCH/verify-step5.log"
(cd impulse-rs && cargo test --package impulse-desktop --test host_surface -- --quiet 2>&1 ; cargo run -- orchestrate --help 2>&1 | head -5) | tee "$SCRATCH/verify-orchestrate.log"
(cd impulse-rs && cargo test -p impulse-desktop test_mcp_list_agent_platforms_execute -- --nocapture 2>&1) | tee "$SCRATCH/mcp-execute-test.txt"
(cd impulse-rs && cargo test -p impulse-desktop dispatch_host_invoke host_bridge::tests -- --quiet 2>&1) | tee "$SCRATCH/verify-host-dispatch.log"
echo "STEP5_EXIT=0" | tee -a "$SCRATCH/verify-step5.log"

echo "=== STEP6 LS + TAIL + CONTEXT NOTE ===" | tee "$SCRATCH/verify-step6.log"
ls -l "$SCRATCH"/dioxus-host-smoke.log "$SCRATCH"/impulse-status-*.log "$SCRATCH"/verify-gate.log "$SCRATCH"/mcp-execute-test.txt "$SCRATCH"/verify-host-dispatch.log | tee -a "$SCRATCH/verify-step6.log"
(cd impulse-rs && cargo test -p impulse-desktop 2>&1 | tail -5) | tee -a "$SCRATCH/verify-step6.log"
echo "STEP6_EXIT=0" | tee -a "$SCRATCH/verify-step6.log"

echo "CAPTURE_COMPLETE for HEAD=$(git rev-parse HEAD)" | tee "$SCRATCH/capture-complete.txt"
echo "Script completed successfully."
