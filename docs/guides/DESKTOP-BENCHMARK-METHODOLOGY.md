---
title: Desktop Benchmark Methodology
status: active
version: 1.0.0
created: 2026-04-15
updated: 2026-04-15
---

# Desktop Benchmark Methodology

This document defines the exact procedure for benchmarking the desktop shell migration and the acceptance thresholds that determine whether the new stack is accepted.

---

## Baselines

Four baselines must be measured on the **same machine** under the same conditions:

| Baseline | Binary | Description |
|---|---|---|
| B1 | `impulse-gui` (legacy egui) | Legacy desktop baseline, release build |
| B2 | `impulse-rs` (ratatui CLI) | Standalone terminal-native operator |
| B3 | Tauri + Dioxus - static shell | No live PTY, no daemon connection |
| B4a | Tauri + Dioxus - 2 PTY panes | Two live xterm.js terminal panes |
| B4b | Tauri + Dioxus - 4 PTY panes | Four live xterm.js terminal panes |

---

## Metrics

### 1. Cold Start to First Interactive Paint

**Definition:** Time from process launch to the moment the UI is interactive.

**Measurement:**
```bash
time open -W /path/to/ImpulseApp.app
```
Or instrument with a startup timestamp log line at the end of the Tauri `setup()` hook.

**Record:** Median of 5 cold launches (process not in memory cache).

---

### 2. Warm Start to First Interactive Paint

**Definition:** Time from process launch when binary and libraries are already in the OS file cache.

**Measurement:** Run 3 times in sequence; take measurements 2 and 3 as warm starts.

**Record:** Median of warm starts.

---

### 3. Idle RSS (Resident Set Size)

**Definition:** Physical memory used by the process after launch with no active PTY sessions, after 30 seconds of idle.

**Measurement:**
```bash
pid=$(pgrep -f ImpulseApp)
ps -o rss= -p $pid  # RSS in KB
```

**Record:** Single reading at T+30s idle.

---

### 4. Idle CPU

**Definition:** CPU usage with no active PTY sessions after 30 seconds of idle.

**Measurement:**
```bash
pid=$(pgrep -f ImpulseApp)
top -pid $pid -l 3 | tail -1 | awk '{print $3}'  # CPU%
```

**Record:** Average of 3 readings, 10 seconds apart, at idle.

---

### 5. PTY Input-to-Echo Latency

**Definition:** Time between a keypress arriving at PTY stdin and the echoed character appearing rendered in the terminal pane.

**Measurement (automated):** Send a byte to `WriteQueue::write_user_input()` and measure time until `has_new_output_since()` returns true. This measures Rust-side latency; add a fixed display latency estimate for the webview render path.

**Record:** Median of 20 keypress events.

---

### 6. PTY Resize Latency

**Definition:** Time between a resize event being sent (`terminal_resize` command) and terminal content reflowing correctly.

**Measurement:** Trigger a resize (drag the pane divider) while long output is visible. Measure time from resize event to stable reflow.

**Record:** Median of 5 resize events.

---

### 7. Daemon Snapshot Refresh Latency

**Definition:** Time between a daemon state change and the corresponding side panel updating in the desktop shell.

**Measurement:** Instrument daemon's `PublishTerminalOps` with a timestamp. Instrument the Dioxus `ops_update` handler with a receipt timestamp. Compute the delta.

**Record:** Median of 10 snapshot events.

---

### 8. WriteQueue Injection Latency

**Definition:** Time for `write_injection()` to complete when not blocked by the quiet guard.

**Measurement:** Use the existing `WriteQueue` unit test infrastructure to time `write_injection()` calls in isolation.

**Record:** Rust-only metric. Baseline from B1 and B2 should be identical.

---

## Acceptance Thresholds

| Metric | Threshold | Rationale |
|---|---|---|
| PTY input-to-echo latency | Must not regress vs B1 by more than 10% | Core terminal responsiveness is non-negotiable |
| PTY resize latency | Must not regress vs B1 by more than 10% | Resize must feel instant |
| Daemon snapshot refresh latency | Must not regress vs B1 by more than 10% | Side panels must stay live |
| Idle RSS | May exceed B2; should stay <= B1 or within 25% above | Webview overhead expected; must not be runaway |
| Idle CPU | Should be <= B1 at idle (no immediate-mode repaints) | Dioxus virtual DOM should be cheaper at idle than the legacy egui baseline |
| Cold start | Should be <= 2x B2; informational vs B1 | Webview startup is slower than TUI; acceptable |
| Warm start | Should be within 50% of B1 | After OS caching, startup should be fast |

---

## Decision Rule

- **Accept** if all hard thresholds (PTY latency, resize latency, daemon latency) are met.
- **Narrow shell scope** if memory or CPU significantly exceeds thresholds - reduce frontend behavior before adding more.
- **Blocker** if PTY input latency regresses by more than 10% - must be resolved before Phase 4.

---

## Benchmark Log Format

Record results in `docs/research/DESKTOP-BENCHMARK-RESULTS.md` as each phase completes:

```markdown
## Baseline: B3 - Static Shell (YYYY-MM-DD, macOS version, chip)

| Metric | Value |
|---|---|
| Cold start | XXX ms |
| Warm start | XXX ms |
| Idle RSS | XXX MB |
| Idle CPU | X.X% |
| PTY input-to-echo | N/A (no PTY) |
| PTY resize | N/A (no PTY) |
| Daemon snapshot | N/A (no daemon) |
```

Fill in B1, B2, B3, B4a, B4b as each phase is reached. Do not interpolate or estimate values - measure them.
