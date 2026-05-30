# impulse-term

Custom terminal widget for Impulse with context lifecycle integration. Replaces `egui_term` with a terminal backend that gives zero-copy, in-process access to both context extraction (reading agent output) and context injection (writing context blocks into agent terminals).

---

## Architecture

```
Agent Process (claude, codex, opencode, shell)
    |
    | PTY (pseudoterminal pair)
    v
TerminalBackend
    |-- reader thread --> vt100::Parser (background, continuous)
    |-- writer handle --> WriteQueue (serialized, priority-aware)
    +-- alive: AtomicBool (process status)
    |
    |--- TerminalRenderer reads vt100::Screen --> egui paint calls
    |--- ContextBridge reads screen_text() for extraction
    +--- ContextBridge writes inject_context() via WriteQueue
    |
    v
TerminalPanel (assembled widget: backend + renderer + input + theme + context + status bar)
```

Data flows in one direction: the agent process writes to the PTY slave, the reader thread feeds bytes into the vt100 parser, the optional egui renderer converts the parsed screen into draw calls, and the context bridge extracts insights from the visible text. Injection flows the opposite way through the WriteQueue.

The core boundary is framework-neutral. `TerminalBackend`, `WriteQueue`, `ContextBridge`, context data types, and bracketed paste helpers compile without egui via `cargo test -p impulse-term --no-default-features`. The default feature set still enables the legacy egui panel, renderer, input, status-bar, and theme modules for compatibility.

---

## Key Modules

| Module | Lines | Tests | Purpose |
|--------|------:|------:|---------|
| `backend.rs` | ~540 | 10 | PTY spawn via `portable-pty`, vt100 parser, background reader thread, `WriteQueue` for serialized writes with user-input priority and injection debounce |
| `context.rs` | ~950 | 28 | `ContextBridge` wrapping extraction, compaction detection, token estimation, and injection. Defines `AgentKind`, `ContextTier`, `InsightType`, `ExtractedInsight` |
| `paste.rs` | ~20 | 1 | Framework-neutral bracketed paste helper used by context injection and egui paste handling |
| `panel.rs` | ~880 | 14 | egui-only `TerminalPanel` assembled widget combining all modules. Handles keyboard/paste input, dynamic PTY resize, scroll-guard, context overlay (Ctrl+Shift+C), `EnvGuard` RAII |
| `renderer.rs` | ~340 | 4 | egui-only run-based rendering: groups consecutive cells with identical attributes into runs, reducing draw calls from ~4,800/frame to ~100-300/frame |
| `input.rs` | ~370 | 13 | egui-only key translation from `Key` + `Modifiers` into VT100/xterm escape sequences. Ctrl+letter, Alt+key, arrow keys, function keys, bracketed paste re-export |
| `theme.rs` | ~690 | 28 | egui-only full 16-color ANSI palette resolution (named, 216-cube, grayscale, RGB). `AgentThemeConfig` with user overrides and two built-in presets |
| `status_bar.rs` | ~130 | 0 | egui-only status bar widget: alive indicator, title + dimensions, context tier icon + usage %, compaction/injection counters, copy button |
| `lib.rs` | ~40 | 0 | Module declarations and public re-exports |

---

## Context Lifecycle

The `ContextBridge` is the core integration point between the terminal and Impulse's context awareness.

**Extraction** (call `extract_tick()` every ~3 seconds):

1. Updates token estimate from visible character count (ANSI-stripped via vt100, multiplied by 1.6x to account for invisible context like system prompts)
2. Scans for compaction events using pattern matching against known agent compaction phrases (debounced to 60s)
3. Diffs screen text against previous snapshot to find new content
4. Extracts structured insights from new content: `FileModified`, `ErrorEncountered`, `DecisionMade`, `TaskCompleted`
5. Deduplicates and stores insights in a bounded buffer (max 50)

**Token tiers** (based on estimated usage of context window):

| Tier | Usage | Behavior |
|------|-------|----------|
| None | 0-44% | No context pressure |
| Essential | 45-59% | Include essential context only |
| Critical | 60-79% | Critical context only |
| Minimal | 80%+ | Bare minimum |
| PostCompaction | After compaction detected | Reset to ~10% estimate |

**Injection** (call `inject_context()`):

1. Respects a 60-second debounce between injections
2. Wraps content in agent-appropriate delimiters (XML for Claude Code, Markdown headers for others)
3. Uses bracketed paste to avoid triggering agent hotkeys
4. Writes atomically through the `WriteQueue`, which skips the write if the user typed within 500ms

---

## Building

```bash
cd impulse-rs

# Build
cargo build -p impulse-term

# Test
cargo test -p impulse-term

# Test the framework-neutral boundary without egui
cargo test -p impulse-term --no-default-features

# Clippy
cargo clippy -p impulse-term -- -D warnings
```

---

## Testing

110 tests total (91 unit + 19 doc/integration). Key coverage areas:

- **backend**: WriteQueue serialization, injection blocking after user input, concurrent write integrity, timestamp tracking
- **context**: Agent kind detection, tier ordering, token estimation, compaction scanning, insight extraction from realistic agent output fixtures (Claude Code, OpenCode), diff-based content detection, injection wrapping
- **panel**: Env var construction, `EnvGuard` RAII restore (including on panic), scroll-guard state machine, token formatting
- **renderer**: Run building from vt100 screen (single cell, multi-run, empty row)
- **input**: Special keys, Ctrl+letter, Alt+key, arrow modes, function keys, printable char exclusion, bracketed paste
- **theme**: Color resolution (named, 216-cube, grayscale, RGB passthrough), agent themes, serde round-trips, preset validation

---

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `portable-pty` | 0.9 | Cross-platform PTY spawn and management |
| `vt100` | 0.15 | Terminal state machine (parser + screen model) |
| `eframe` | 0.31 | Optional egui framework behind the default `egui` feature; used by renderer, input, panel, status bar, and theme |
| `parking_lot` | 0.12 | `FairMutex` (prevents reader-thread starvation) and `Mutex` |
| `chrono` | 0.4 | Timestamps on extracted insights |
| `serde` | 1.0 | Serialization for `AgentTheme`, `AgentKind`, `ContextTier`, `InsightType` |
