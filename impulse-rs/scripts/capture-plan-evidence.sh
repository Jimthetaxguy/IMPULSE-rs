#!/usr/bin/env bash
set -euo pipefail
# capture-plan-evidence.sh 
# from repo root
SCRATCH=${SCRATCH:-/var/folders/zy/mwkg1s5x2839c2xgfzqt27n00000gn/T/grok-goal-bd088a7e3032/implementer}
mkdir -p "$SCRATCH"

echo "HEAD=$(git rev-parse HEAD)" | tee "$SCRATCH/capture-head.txt"

# allow only documented ?? archive/ and context/work card etc per plan/ADR
if git status --short | grep -v -E '(\?\? archive/|\?\? CONTEXT.md|\?\? docs/decisions/|\?\? docs/plans/worktrees/|\?\? impulse-rs/scripts/)' | grep -q '^[ MADRC]'; then
  echo "ERROR: dirty git (only pre-documented ?? allowed)"
  git status --short
  exit 1
fi

echo "=== STEP2 SMOKE ===" | tee "$SCRATCH/verify-step2.log"
(cd impulse-rs/impulse-desktop && npm run vendor:xterm 2>&1 | tail -3 && npm run dioxus:host:smoke 2>&1) | tee "$SCRATCH/dioxus-host-smoke.log"
echo "smoke_exit=$?" | tee -a "$SCRATCH/dioxus-host-smoke.log"

echo "=== STEP3 STATUS x2 ===" | tee "$SCRATCH/verify-step3.log"
(cd impulse-rs && cargo run -- status --format text 2>&1 | tail -10 ; cargo run -- status --format json 2>&1 | head -30) | tee "$SCRATCH/impulse-status-1.log"
(cd impulse-rs && cargo run -- status --format text 2>&1 | tail -10 ; cargo run -- status --format json 2>&1 | head -30) | tee "$SCRATCH/impulse-status-2.log"

echo "=== STEP4 GATE ===" | tee "$SCRATCH/verify-step4.log"
(cd impulse-rs && cargo build --workspace 2>&1 | tail -3 && cargo test --workspace 2>&1 | grep -E 'test result:|passed|failed' | tail -10 && cargo clippy --workspace -- -D warnings 2>&1 | tail -3 && cargo fmt --check 2>&1) | tee "$SCRATCH/verify-gate.log"

echo "=== STEP5 HOST/ORCH ===" | tee "$SCRATCH/verify-step5.log"
(cd impulse-rs && cargo test --package impulse-desktop --test host_surface -- --quiet 2>&1 ; cargo run -- orchestrate --help 2>&1 | head -5) | tee "$SCRATCH/verify-orchestrate.log"

echo "=== STEP6 LS + TAIL ===" | tee "$SCRATCH/verify-step6.log"
ls -l "$SCRATCH"/dioxus-host-smoke.log "$SCRATCH"/impulse-status-*.log "$SCRATCH"/verify-gate.log | tee -a "$SCRATCH/verify-step6.log"
(cd impulse-rs && cargo test -p impulse-desktop 2>&1 | tail -5) | tee -a "$SCRATCH/verify-step6.log"

echo "CAPTURE_COMPLETE for HEAD=$(git rev-parse HEAD)" | tee "$SCRATCH/capture-complete.txt"
