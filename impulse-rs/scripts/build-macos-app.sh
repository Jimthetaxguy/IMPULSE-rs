#!/usr/bin/env bash
# Compatibility entry point: desktop packaging is deliberately unavailable.
set -euo pipefail

cat >&2 <<'MESSAGE'
Desktop app and DMG packaging is unavailable.
Tagged releases currently contain the impulse-rs CLI for macOS and Linux only.
For the source CLI, run: cargo build --locked --release --bin impulse-rs
Dioxus packaging requires the build, bundle, and real launch gates documented in:
  docs/plans/EGUI-DECOMMISSION.md (Track A / R1)
This retired entry point does not build or modify any artifacts.
MESSAGE
exit 1
